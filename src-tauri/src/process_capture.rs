//! Bounded capture for short-lived subprocesses.
//!
//! Callers provide a configured command and wall-clock timeout. This module
//! owns pipe draining, byte budgets, process-tree termination, and reaping so
//! adb/fastboot/tool probes cannot diverge or collect unbounded output.

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_STREAM_LIMIT_BYTES: usize = 4 * 1024 * 1024;
const STDOUT_LIMIT_BIT: u8 = 0b01;
const STDERR_LIMIT_BIT: u8 = 0b10;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Post-termination grace for the pipe readers to observe EOF. A cleanly
/// exited child can leak its pipe write-ends to a detached descendant (the
/// classic `adb` server autostart), so an unbounded join would hang forever.
pub(crate) const READER_EOF_GRACE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureStream {
    Stdout,
    Stderr,
    Both,
}

impl std::fmt::Display for CaptureStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Both => "stdout and stderr",
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CaptureLimits {
    pub(crate) stdout_bytes: usize,
    pub(crate) stderr_bytes: usize,
}

impl CaptureLimits {
    pub(crate) const fn symmetric(bytes: usize) -> Self {
        Self {
            stdout_bytes: bytes,
            stderr_bytes: bytes,
        }
    }
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self::symmetric(DEFAULT_STREAM_LIMIT_BYTES)
    }
}

#[derive(Debug)]
pub(crate) enum CaptureTermination {
    Exited(ExitStatus),
    TimedOut,
    OutputLimitExceeded {
        stream: CaptureStream,
        limit_bytes: usize,
    },
}

#[derive(Debug)]
pub(crate) struct CapturedOutput {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) termination: CaptureTermination,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CaptureError {
    #[error("failed to spawn child process: {0}")]
    Spawn(std::io::Error),
    #[error("child process did not expose its {0} pipe")]
    MissingPipe(&'static str),
    #[error("failed while waiting for child process: {0}")]
    Wait(std::io::Error),
    #[error("failed to terminate child process tree: {0}")]
    Terminate(std::io::Error),
    #[error("failed while reading child {stream}: {source}")]
    Read {
        stream: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{0} capture worker panicked")]
    ReaderPanicked(&'static str),
}

enum StopReason {
    Exited(ExitStatus),
    TimedOut,
    OutputLimit,
    WaitFailed(std::io::Error),
}

/// Run `command` with closed stdin and independently bounded stdout/stderr.
/// Readers keep draining after their budget is exhausted so a producer cannot
/// deadlock before the supervisor observes the limit and kills the full tree.
pub(crate) fn run(
    command: &mut Command,
    timeout: Duration,
    limits: CaptureLimits,
) -> Result<CapturedOutput, CaptureError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process_tree::configure(command);
    let mut child = command.spawn().map_err(CaptureError::Spawn)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(CaptureError::MissingPipe("stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(CaptureError::MissingPipe("stderr"))?;

    let exceeded = Arc::new(AtomicU8::new(0));
    let stdout_reader = spawn_reader(
        stdout,
        "stdout",
        limits.stdout_bytes,
        STDOUT_LIMIT_BIT,
        Arc::clone(&exceeded),
    );
    let stderr_reader = spawn_reader(
        stderr,
        "stderr",
        limits.stderr_bytes,
        STDERR_LIMIT_BIT,
        Arc::clone(&exceeded),
    );

    let started = Instant::now();
    let reason = loop {
        if exceeded.load(Ordering::Acquire) != 0 {
            // Give the sibling reader one scheduling slice to report a
            // simultaneous overflow, while both readers continue draining
            // without retaining additional bytes.
            thread::sleep(POLL_INTERVAL);
            crate::process_tree::terminate(&mut child).map_err(CaptureError::Terminate)?;
            break StopReason::OutputLimit;
        }
        if started.elapsed() >= timeout {
            crate::process_tree::terminate(&mut child).map_err(CaptureError::Terminate)?;
            break StopReason::TimedOut;
        }
        match child.try_wait() {
            Ok(Some(status)) => break StopReason::Exited(status),
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(error) => {
                crate::process_tree::terminate(&mut child).map_err(CaptureError::Terminate)?;
                break StopReason::WaitFailed(error);
            }
        }
    };

    wait_for_pipe_readers(&mut child, &stdout_reader.handle, &stderr_reader.handle);
    let stdout = collect_capture(stdout_reader, "stdout")?;
    let stderr = collect_capture(stderr_reader, "stderr")?;
    if let StopReason::WaitFailed(error) = reason {
        return Err(CaptureError::Wait(error));
    }

    let exceeded = exceeded.load(Ordering::Acquire);
    let termination = if exceeded != 0 {
        let stream = match exceeded & (STDOUT_LIMIT_BIT | STDERR_LIMIT_BIT) {
            STDOUT_LIMIT_BIT => CaptureStream::Stdout,
            STDERR_LIMIT_BIT => CaptureStream::Stderr,
            _ => CaptureStream::Both,
        };
        let limit_bytes = match stream {
            CaptureStream::Stdout => limits.stdout_bytes,
            CaptureStream::Stderr => limits.stderr_bytes,
            CaptureStream::Both => limits.stdout_bytes.min(limits.stderr_bytes),
        };
        CaptureTermination::OutputLimitExceeded {
            stream,
            limit_bytes,
        }
    } else {
        match reason {
            StopReason::Exited(status) => CaptureTermination::Exited(status),
            StopReason::TimedOut => CaptureTermination::TimedOut,
            StopReason::OutputLimit => unreachable!("output limit bit must be set"),
            StopReason::WaitFailed(_) => unreachable!("wait failure returned above"),
        }
    };

    Ok(CapturedOutput {
        stdout,
        stderr,
        termination,
    })
}

/// A pipe reader whose captured bytes live behind a shared handle, so the
/// supervisor can take a snapshot and detach the thread if a leaked write-end
/// keeps the pipe open after the child exited.
struct StreamCapture {
    bytes: Arc<Mutex<Vec<u8>>>,
    handle: thread::JoinHandle<Result<(), CaptureError>>,
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    stream: &'static str,
    limit: usize,
    limit_bit: u8,
    exceeded: Arc<AtomicU8>,
) -> StreamCapture {
    let bytes = Arc::new(Mutex::new(Vec::with_capacity(limit.min(4096))));
    let shared = Arc::clone(&bytes);
    let handle = thread::spawn(move || {
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Err(source) => return Err(CaptureError::Read { stream, source }),
                Ok(count) => {
                    let mut captured = shared.lock().unwrap_or_else(|error| error.into_inner());
                    let available = limit.saturating_sub(captured.len());
                    captured.extend_from_slice(&buffer[..count.min(available)]);
                    if count > available {
                        exceeded.fetch_or(limit_bit, Ordering::Release);
                    }
                }
            }
        }
        Ok(())
    });
    StreamCapture { bytes, handle }
}

/// Wait a bounded interval for both pipe readers to observe EOF. If the child
/// (or its killed tree) closed the pipes this returns almost immediately; if a
/// detached descendant inherited the write-ends, kill the process group to
/// close them and give the readers one more bounded slice. Style mirrors
/// `CapturedTail::wait_for_eof` in `captured_tail.rs`. Shared with
/// `operations::execute_child`, which has the same post-exit join hazard.
pub(crate) fn wait_for_pipe_readers<A, B>(
    child: &mut Child,
    stdout: &thread::JoinHandle<A>,
    stderr: &thread::JoinHandle<B>,
) {
    let started = Instant::now();
    while !(stdout.is_finished() && stderr.is_finished()) && started.elapsed() < READER_EOF_GRACE {
        thread::sleep(POLL_INTERVAL);
    }
    if stdout.is_finished() && stderr.is_finished() {
        return;
    }
    let _ = crate::process_tree::terminate(child);
    let started = Instant::now();
    while !(stdout.is_finished() && stderr.is_finished()) && started.elapsed() < READER_EOF_GRACE {
        thread::sleep(POLL_INTERVAL);
    }
}

/// Join a finished reader (surfacing read errors/panics), or detach a stuck
/// one and return the bytes captured so far. A detached thread holds only its
/// bounded buffer and exits when the leaked pipe finally closes.
fn collect_capture(capture: StreamCapture, stream: &'static str) -> Result<Vec<u8>, CaptureError> {
    if capture.handle.is_finished() {
        capture
            .handle
            .join()
            .map_err(|_| CaptureError::ReaderPanicked(stream))??;
    }
    let mut bytes = capture
        .bytes
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    Ok(std::mem::take(&mut *bytes))
}

/// Retain only the newest `limit` bytes. Streaming operations and long-lived
/// supervisors share this helper so their in-memory tails follow the same
/// overflow arithmetic as short-lived process capture.
pub(crate) fn append_tail(target: &mut Vec<u8>, bytes: &[u8], limit: usize) {
    if bytes.len() >= limit {
        target.clear();
        target.extend_from_slice(&bytes[bytes.len() - limit..]);
        return;
    }
    let overflow = target
        .len()
        .saturating_add(bytes.len())
        .saturating_sub(limit);
    if overflow > 0 {
        target.drain(..overflow);
    }
    target.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingReader {
        emitted: bool,
    }

    impl Read for FailingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if !self.emitted {
                self.emitted = true;
                buffer[..4].copy_from_slice(b"data");
                return Ok(4);
            }
            Err(std::io::Error::other("injected read failure"))
        }
    }

    #[test]
    fn reader_errors_are_not_reported_as_clean_eof() {
        let exceeded = Arc::new(AtomicU8::new(0));
        let capture = spawn_reader(FailingReader { emitted: false }, "stdout", 32, 1, exceeded);
        let result = capture.handle.join().unwrap();
        assert!(matches!(
            result,
            Err(CaptureError::Read {
                stream: "stdout",
                ..
            })
        ));
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn leaked_pipe_write_end_does_not_block_capture_after_clean_exit() {
        // The child exits immediately but hands its stdout write-end to a
        // detached long-lived descendant. An unbounded reader join would
        // block until that descendant exits (~30 s); the bounded wait must
        // return promptly with the output captured before the exit.
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args([
                "/C",
                "echo leaked-pipe-marker& start /B ping -n 8 127.0.0.1& exit 0",
            ]);
            command
        };
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", "echo leaked-pipe-marker; sleep 30 & exit 0"]);
            command
        };

        let started = Instant::now();
        let output = run(
            &mut command,
            Duration::from_secs(20),
            CaptureLimits::default(),
        )
        .expect("capture must complete");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "capture blocked on a leaked pipe write-end for {:?}",
            started.elapsed()
        );
        assert!(matches!(output.termination, CaptureTermination::Exited(_)));
        assert!(String::from_utf8_lossy(&output.stdout).contains("leaked-pipe-marker"));
    }
}
