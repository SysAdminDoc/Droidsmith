//! Bounded capture for short-lived subprocesses.
//!
//! Callers provide a configured command and wall-clock timeout. This module
//! owns pipe draining, byte budgets, process-tree termination, and reaping so
//! adb/fastboot/tool probes cannot diverge or collect unbounded output.

use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_STREAM_LIMIT_BYTES: usize = 4 * 1024 * 1024;
const STDOUT_LIMIT_BIT: u8 = 0b01;
const STDERR_LIMIT_BIT: u8 = 0b10;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

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
        limits.stdout_bytes,
        STDOUT_LIMIT_BIT,
        Arc::clone(&exceeded),
    );
    let stderr_reader = spawn_reader(
        stderr,
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

    let stdout = stdout_reader
        .join()
        .map_err(|_| CaptureError::ReaderPanicked("stdout"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| CaptureError::ReaderPanicked("stderr"))?;
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

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    limit: usize,
    limit_bit: u8,
    exceeded: Arc<AtomicU8>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut captured = Vec::with_capacity(limit.min(4096));
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    let available = limit.saturating_sub(captured.len());
                    captured.extend_from_slice(&buffer[..count.min(available)]);
                    if count > available {
                        exceeded.fetch_or(limit_bit, Ordering::Release);
                    }
                }
            }
        }
        captured
    })
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
