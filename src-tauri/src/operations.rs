//! Cancellable background subprocess execution for long-running ADB work.
//!
//! Tauri commands call this module from `spawn_blocking`, keeping the webview
//! thread responsive. Every operation is registered before its child starts;
//! `cancel` flips the shared flag and the runner kills and reaps the child.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;

const CAPTURE_LIMIT_BYTES: usize = 1024 * 1024;
/// Maximum stdin payload for [`RegisteredOperation::run_stage_with_input`],
/// matching the smallest common OS pipe buffer so a full pre-spawn write can
/// never block against an already-chatty child.
pub(crate) const MAX_STAGE_INPUT_BYTES: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(40);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);
const CANCELLATION_HISTORY_LIMIT: usize = 256;

#[derive(specta::Type, Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationEventKind {
    Started,
    Output,
    Progress,
    Reconnecting,
    Finished,
    Cancelled,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OperationEvent {
    pub operation_id: String,
    pub kind: OperationEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
}

impl OperationEvent {
    fn status(operation_id: &str, kind: OperationEventKind, message: impl Into<String>) -> Self {
        Self {
            operation_id: operation_id.to_string(),
            kind,
            stream: None,
            chunk: None,
            message: Some(message.into()),
            elapsed_ms: None,
            attempt: None,
        }
    }
}

pub type EventSink = Arc<dyn Fn(OperationEvent) + Send + Sync>;

pub fn channel_sink(channel: Channel<OperationEvent>) -> EventSink {
    Arc::new(move |event| {
        let _ = channel.send(event);
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
}

impl ProcessOutput {
    pub fn success(&self) -> bool {
        !self.timed_out && !self.cancelled && self.code == Some(0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OperationError {
    #[error("invalid operation id {0:?}")]
    InvalidId(String),
    #[error("operation {0:?} is already running")]
    DuplicateId(String),
    #[error("failed to spawn {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed while waiting for child process: {0}")]
    Wait(#[source] std::io::Error),
    #[error("failed while writing subprocess input: {0}")]
    Input(#[source] std::io::Error),
    #[error("failed to terminate child process tree: {0}")]
    Terminate(#[source] std::io::Error),
    #[error("failed while reading child {stream}: {source}")]
    OutputRead {
        stream: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{0} output worker panicked")]
    ReaderPanicked(&'static str),
    #[error("long-lived process exited unexpectedly with code {0:?}")]
    UnexpectedExit(Option<i32>),
    #[error("operation was cancelled")]
    Cancelled,
    #[error("operation timed out after {0:?}")]
    Timeout(Duration),
    #[error("operation output exceeded the {0}-byte limit")]
    OutputTooLarge(u64),
}

#[derive(Default)]
struct OperationRegistry {
    active: HashMap<String, Arc<AtomicBool>>,
    pending_cancellations: VecDeque<String>,
    completed: VecDeque<String>,
}

fn registry() -> &'static Mutex<OperationRegistry> {
    static REGISTRY: OnceLock<Mutex<OperationRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(OperationRegistry::default()))
}

struct Registration {
    operation_id: String,
    cancelled: Arc<AtomicBool>,
}

pub(crate) struct CancellationGuard {
    registration: Registration,
}

impl CancellationGuard {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.registration.cancelled.load(Ordering::Acquire)
    }
}

pub(crate) fn register_cancellable(
    operation_id: &str,
) -> Result<CancellationGuard, OperationError> {
    Ok(CancellationGuard {
        registration: Registration::new(operation_id)?,
    })
}

/// A cancellable multi-stage operation whose later command arguments depend on
/// earlier output (for example, Android package-install session IDs).
/// Registration lives for the full workflow, so a single Cancel action covers
/// archive preparation and every subprocess stage.
pub(crate) struct RegisteredOperation {
    operation_id: String,
    sink: EventSink,
    cancellation: CancellationGuard,
    started: Instant,
}

impl RegisteredOperation {
    pub(crate) fn new(
        operation_id: &str,
        label: &str,
        sink: EventSink,
    ) -> Result<Self, OperationError> {
        let cancellation = register_cancellable(operation_id)?;
        sink(OperationEvent::status(
            operation_id,
            OperationEventKind::Started,
            label,
        ));
        Ok(Self {
            operation_id: operation_id.to_string(),
            sink,
            cancellation,
            started: Instant::now(),
        })
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub(crate) fn run_stage(
        &mut self,
        program: &Path,
        args: &[String],
        timeout: Duration,
        label: &str,
    ) -> Result<ProcessOutput, OperationError> {
        if self.is_cancelled() {
            return Err(OperationError::Cancelled);
        }
        (self.sink)(OperationEvent {
            operation_id: self.operation_id.clone(),
            kind: OperationEventKind::Progress,
            stream: None,
            chunk: None,
            message: Some(label.to_string()),
            elapsed_ms: Some(saturating_millis(self.started.elapsed())),
            attempt: None,
        });
        execute_child(
            program,
            args,
            timeout,
            &self.operation_id,
            &self.cancellation.registration.cancelled,
            &self.sink,
            ChildOptions::captured(),
        )
    }

    /// Run a stage with bounded caller-owned stdin. Used for text configs that
    /// should not be persisted as host or device temporary files.
    pub(crate) fn run_stage_with_input(
        &mut self,
        program: &Path,
        args: &[String],
        input: &[u8],
        timeout: Duration,
        label: &str,
    ) -> Result<ProcessOutput, OperationError> {
        // Pipe-buffer protection: stdin is written in full before the output
        // readers start, so input larger than the OS pipe buffer (64 KiB) can
        // deadlock against a child that emits output before draining stdin.
        if input.len() > MAX_STAGE_INPUT_BYTES {
            return Err(OperationError::Input(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "stage input is {} bytes; the {MAX_STAGE_INPUT_BYTES}-byte cap prevents a stdin pipe deadlock",
                    input.len()
                ),
            )));
        }
        if self.is_cancelled() {
            return Err(OperationError::Cancelled);
        }
        (self.sink)(OperationEvent {
            operation_id: self.operation_id.clone(),
            kind: OperationEventKind::Progress,
            stream: None,
            chunk: None,
            message: Some(label.to_string()),
            elapsed_ms: Some(saturating_millis(self.started.elapsed())),
            attempt: None,
        });
        execute_child(
            program,
            args,
            timeout,
            &self.operation_id,
            &self.cancellation.registration.cancelled,
            &self.sink,
            ChildOptions::captured_with_input(input),
        )
    }

    /// Cleanup must still be attempted after the user cancels the producing
    /// stage. It is independently timeout-bounded and backend-controlled.
    pub(crate) fn run_cleanup_stage(
        &self,
        program: &Path,
        args: &[String],
        timeout: Duration,
        label: &str,
    ) -> Result<ProcessOutput, OperationError> {
        (self.sink)(OperationEvent {
            operation_id: self.operation_id.clone(),
            kind: OperationEventKind::Progress,
            stream: None,
            chunk: None,
            message: Some(label.to_string()),
            elapsed_ms: Some(saturating_millis(self.started.elapsed())),
            attempt: None,
        });
        let cleanup_cancelled = Arc::new(AtomicBool::new(false));
        execute_child(
            program,
            args,
            timeout,
            &self.operation_id,
            &cleanup_cancelled,
            &self.sink,
            ChildOptions::captured(),
        )
    }

    pub(crate) fn finish(&self, message: &str) {
        (self.sink)(OperationEvent::status(
            &self.operation_id,
            OperationEventKind::Finished,
            message,
        ));
    }

    pub(crate) fn cancelled(&self, message: &str) {
        (self.sink)(OperationEvent::status(
            &self.operation_id,
            OperationEventKind::Cancelled,
            message,
        ));
    }
}

impl Registration {
    fn new(operation_id: &str) -> Result<Self, OperationError> {
        if !valid_operation_id(operation_id) {
            return Err(OperationError::InvalidId(operation_id.to_string()));
        }
        let mut operations = registry().lock().unwrap_or_else(|e| e.into_inner());
        if operations.active.contains_key(operation_id) {
            return Err(OperationError::DuplicateId(operation_id.to_string()));
        }
        let cancelled_before_registration = operations
            .pending_cancellations
            .iter()
            .position(|candidate| candidate == operation_id)
            .and_then(|index| operations.pending_cancellations.remove(index))
            .is_some();
        operations
            .completed
            .retain(|candidate| candidate != operation_id);
        let cancelled = Arc::new(AtomicBool::new(cancelled_before_registration));
        operations
            .active
            .insert(operation_id.to_string(), Arc::clone(&cancelled));
        Ok(Self {
            operation_id: operation_id.to_string(),
            cancelled,
        })
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let mut operations = registry().lock().unwrap_or_else(|e| e.into_inner());
        operations.active.remove(&self.operation_id);
        operations.completed.push_back(self.operation_id.clone());
        while operations.completed.len() > CANCELLATION_HISTORY_LIMIT {
            operations.completed.pop_front();
        }
    }
}

fn valid_operation_id(value: &str) -> bool {
    (8..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub fn cancel(operation_id: &str) -> bool {
    if !valid_operation_id(operation_id) {
        return false;
    }
    let mut operations = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(flag) = operations.active.get(operation_id) {
        flag.store(true, Ordering::Release);
        return true;
    }
    if operations
        .completed
        .iter()
        .any(|candidate| candidate == operation_id)
    {
        return false;
    }
    if !operations
        .pending_cancellations
        .iter()
        .any(|candidate| candidate == operation_id)
    {
        operations
            .pending_cancellations
            .push_back(operation_id.to_string());
        while operations.pending_cancellations.len() > CANCELLATION_HISTORY_LIMIT {
            operations.pending_cancellations.pop_front();
        }
    }
    true
}

pub fn run_process(
    program: &Path,
    args: &[String],
    timeout: Duration,
    operation_id: &str,
    label: &str,
    sink: EventSink,
) -> Result<ProcessOutput, OperationError> {
    run_process_inner(program, args, timeout, operation_id, label, sink, None)
}

/// Run a cancellable process while enforcing a live size ceiling on one
/// producer-owned output file. The child tree is terminated as soon as the
/// observed file grows beyond the budget, preventing an export from consuming
/// unbounded host storage before post-run validation.
pub fn run_process_with_file_budget(
    program: &Path,
    args: &[String],
    timeout: Duration,
    operation_id: &str,
    label: &str,
    sink: EventSink,
    file_budget: (&Path, u64),
) -> Result<ProcessOutput, OperationError> {
    run_process_inner(
        program,
        args,
        timeout,
        operation_id,
        label,
        sink,
        Some(file_budget),
    )
}

fn run_process_inner(
    program: &Path,
    args: &[String],
    timeout: Duration,
    operation_id: &str,
    label: &str,
    sink: EventSink,
    file_budget: Option<(&Path, u64)>,
) -> Result<ProcessOutput, OperationError> {
    let registration = Registration::new(operation_id)?;
    sink(OperationEvent::status(
        operation_id,
        OperationEventKind::Started,
        label,
    ));
    let result = execute_child(
        program,
        args,
        timeout,
        operation_id,
        &registration.cancelled,
        &sink,
        ChildOptions {
            capture_output: true,
            file_budget,
            stdin_data: None,
        },
    );
    match &result {
        Ok(output) => {
            sink(OperationEvent::status(
                operation_id,
                OperationEventKind::Finished,
                if output.success() {
                    "Operation completed"
                } else {
                    "Operation exited with an error"
                },
            ));
        }
        Err(OperationError::Cancelled) => sink(OperationEvent::status(
            operation_id,
            OperationEventKind::Cancelled,
            "Operation cancelled",
        )),
        Err(_) => {}
    }
    result
}

/// Run an ordered set of subprocess stages under an existing cancellation
/// token. The sequence stops at the first non-zero exit so recovery workflows
/// cannot report later steps as successful after a prerequisite failed.
pub(crate) fn run_registered_sequence(
    program: &Path,
    stages: &[(String, Vec<String>)],
    timeout_per_stage: Duration,
    operation_id: &str,
    sink: EventSink,
    cancellation: &CancellationGuard,
) -> Result<Vec<ProcessOutput>, OperationError> {
    let started = Instant::now();
    if cancellation.is_cancelled() {
        sink(OperationEvent::status(
            operation_id,
            OperationEventKind::Cancelled,
            "Recovery cancelled",
        ));
        return Err(OperationError::Cancelled);
    }
    sink(OperationEvent::status(
        operation_id,
        OperationEventKind::Started,
        "Recovery sequence started",
    ));
    let mut outputs = Vec::with_capacity(stages.len());

    for (index, (label, args)) in stages.iter().enumerate() {
        sink(OperationEvent {
            operation_id: operation_id.to_string(),
            kind: OperationEventKind::Progress,
            stream: None,
            chunk: None,
            message: Some(format!("Step {}/{}: {label}", index + 1, stages.len())),
            elapsed_ms: Some(saturating_millis(started.elapsed())),
            attempt: None,
        });
        match execute_child(
            program,
            args,
            timeout_per_stage,
            operation_id,
            &cancellation.registration.cancelled,
            &sink,
            ChildOptions::captured(),
        ) {
            Ok(output) => {
                let success = output.success();
                outputs.push(output);
                if !success {
                    sink(OperationEvent::status(
                        operation_id,
                        OperationEventKind::Finished,
                        format!("Recovery stopped after {label}"),
                    ));
                    return Ok(outputs);
                }
            }
            Err(OperationError::Cancelled) => {
                sink(OperationEvent::status(
                    operation_id,
                    OperationEventKind::Cancelled,
                    "Recovery cancelled",
                ));
                return Err(OperationError::Cancelled);
            }
            Err(error) => return Err(error),
        }
    }

    sink(OperationEvent::status(
        operation_id,
        OperationEventKind::Finished,
        "Recovery sequence completed",
    ));
    Ok(outputs)
}

/// Run one long-lived Logcat process, reconnecting after unexpected exits until
/// the renderer cancels the operation. Output is delivered only through the
/// channel; each child capture is bounded so a long session cannot grow Rust
/// memory without limit.
pub fn stream_logcat(
    adb_path: &Path,
    args: &[String],
    operation_id: &str,
    sink: EventSink,
) -> Result<(), OperationError> {
    let registration = Registration::new(operation_id)?;
    sink(OperationEvent::status(
        operation_id,
        OperationEventKind::Started,
        "Logcat stream started",
    ));
    let mut attempt = 1_u32;

    loop {
        if registration.cancelled.load(Ordering::Acquire) {
            sink(OperationEvent::status(
                operation_id,
                OperationEventKind::Cancelled,
                "Logcat stream stopped",
            ));
            return Ok(());
        }

        let started = Instant::now();
        let result = execute_child(
            adb_path,
            args,
            Duration::from_secs(24 * 60 * 60),
            operation_id,
            &registration.cancelled,
            &sink,
            ChildOptions::streamed(),
        );
        if matches!(result, Err(OperationError::Cancelled)) {
            sink(OperationEvent::status(
                operation_id,
                OperationEventKind::Cancelled,
                "Logcat stream stopped",
            ));
            return Ok(());
        }
        // A stream that ran for a while before dropping is healthy: clear the
        // transient-failure budget so the cap counts consecutive rapid spawn
        // failures rather than lifetime reconnects across a long session.
        if started.elapsed() >= Duration::from_secs(30) {
            attempt = 1;
        }
        let failure = stream_failure(result);
        if rapid_stream_failures_exhausted(attempt, started.elapsed()) {
            return Err(failure);
        }

        attempt = attempt.saturating_add(1);
        sink(OperationEvent {
            operation_id: operation_id.to_string(),
            kind: OperationEventKind::Reconnecting,
            stream: None,
            chunk: None,
            message: Some("Logcat disconnected; reconnecting".to_string()),
            elapsed_ms: None,
            attempt: Some(attempt),
        });
        for _ in 0..15 {
            if registration.cancelled.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn stream_failure(result: Result<ProcessOutput, OperationError>) -> OperationError {
    match result {
        Ok(output) => OperationError::UnexpectedExit(output.code),
        Err(error) => error,
    }
}

fn rapid_stream_failures_exhausted(attempt: u32, elapsed: Duration) -> bool {
    elapsed < Duration::from_secs(30) && attempt >= 5
}

#[derive(Clone, Copy)]
struct ChildOptions<'a> {
    capture_output: bool,
    file_budget: Option<(&'a Path, u64)>,
    stdin_data: Option<&'a [u8]>,
}

impl ChildOptions<'_> {
    const fn captured() -> Self {
        Self {
            capture_output: true,
            file_budget: None,
            stdin_data: None,
        }
    }

    const fn captured_with_input(input: &[u8]) -> ChildOptions<'_> {
        ChildOptions {
            capture_output: true,
            file_budget: None,
            stdin_data: Some(input),
        }
    }

    const fn streamed() -> Self {
        Self {
            capture_output: false,
            file_budget: None,
            stdin_data: None,
        }
    }
}

fn execute_child(
    program: &Path,
    args: &[String],
    timeout: Duration,
    operation_id: &str,
    cancelled: &Arc<AtomicBool>,
    sink: &EventSink,
    options: ChildOptions<'_>,
) -> Result<ProcessOutput, OperationError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(OperationError::Cancelled);
    }
    let mut command = Command::new(program);
    command.args(args);
    if options.stdin_data.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    crate::process_tree::configure(&mut command);
    let mut child = command.spawn().map_err(|source| OperationError::Spawn {
        program: program.display().to_string(),
        source,
    })?;

    if let Some(input) = options.stdin_data {
        let write_result = child
            .stdin
            .take()
            .expect("piped stdin must exist")
            .write_all(input);
        if let Err(error) = write_result {
            crate::process_tree::terminate(&mut child).map_err(OperationError::Terminate)?;
            return Err(OperationError::Input(error));
        }
    }

    let stdout = child.stdout.take().expect("piped stdout must exist");
    let stderr = child.stderr.take().expect("piped stderr must exist");
    let stdout_reader = read_stream(
        stdout,
        operation_id.to_string(),
        "stdout",
        Arc::clone(sink),
        options.capture_output,
    );
    let stderr_reader = read_stream(
        stderr,
        operation_id.to_string(),
        "stderr",
        Arc::clone(sink),
        options.capture_output,
    );

    let started = Instant::now();
    let mut last_progress = started;
    let mut termination = None;
    let mut output_too_large = None;
    while termination.is_none() {
        if cancelled.load(Ordering::Acquire) {
            crate::process_tree::terminate(&mut child).map_err(OperationError::Terminate)?;
            termination = Some((None, false, true));
            continue;
        }
        if started.elapsed() >= timeout {
            crate::process_tree::terminate(&mut child).map_err(OperationError::Terminate)?;
            termination = Some((None, true, false));
            continue;
        }
        if let Some((path, max_bytes)) = options.file_budget {
            if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > max_bytes) {
                crate::process_tree::terminate(&mut child).map_err(OperationError::Terminate)?;
                output_too_large = Some(max_bytes);
                termination = Some((None, false, false));
                continue;
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => termination = Some((status.code(), false, false)),
            Ok(None) => {
                if last_progress.elapsed() >= PROGRESS_INTERVAL {
                    sink(OperationEvent {
                        operation_id: operation_id.to_string(),
                        kind: OperationEventKind::Progress,
                        stream: None,
                        chunk: None,
                        message: Some("Operation still running".to_string()),
                        elapsed_ms: Some(saturating_millis(started.elapsed())),
                        attempt: None,
                    });
                    last_progress = Instant::now();
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(error) => {
                crate::process_tree::terminate(&mut child).map_err(OperationError::Terminate)?;
                crate::process_capture::wait_for_pipe_readers(
                    &mut child,
                    &stdout_reader.handle,
                    &stderr_reader.handle,
                );
                return Err(OperationError::Wait(error));
            }
        }
    }

    crate::process_capture::wait_for_pipe_readers(
        &mut child,
        &stdout_reader.handle,
        &stderr_reader.handle,
    );
    let stdout_result = collect_reader(stdout_reader, "stdout");
    let stderr_result = collect_reader(stderr_reader, "stderr");
    let (code, timed_out, was_cancelled) = termination.expect("termination is assigned");
    if was_cancelled {
        return Err(OperationError::Cancelled);
    }
    if timed_out {
        return Err(OperationError::Timeout(timeout));
    }
    if let Some(max_bytes) = output_too_large {
        return Err(OperationError::OutputTooLarge(max_bytes));
    }
    let stdout = String::from_utf8_lossy(&stdout_result?).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_result?).into_owned();
    Ok(ProcessOutput {
        stdout,
        stderr,
        code,
        timed_out: false,
        cancelled: false,
    })
}

/// A pipe reader whose captured bytes live behind a shared handle, so
/// `execute_child` can take a snapshot and detach the thread if a leaked pipe
/// write-end keeps it blocked after the child exited.
struct StreamCapture {
    bytes: Arc<Mutex<Vec<u8>>>,
    handle: std::thread::JoinHandle<std::io::Result<()>>,
}

fn read_stream<R: Read + Send + 'static>(
    mut reader: R,
    operation_id: String,
    stream: &'static str,
    sink: EventSink,
    capture_output: bool,
) -> StreamCapture {
    let bytes = Arc::new(Mutex::new(Vec::with_capacity(4096)));
    let shared = Arc::clone(&bytes);
    let handle = std::thread::spawn(move || {
        let mut utf8_pending = Vec::with_capacity(4);
        let mut emitted_bytes = 0usize;
        let mut emitted_omission_notice = false;
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    let final_text = decode_utf8_incremental(&mut utf8_pending, &[], true);
                    emit_output(
                        &sink,
                        &operation_id,
                        stream,
                        &final_text,
                        capture_output,
                        &mut emitted_bytes,
                        &mut emitted_omission_notice,
                    );
                    break;
                }
                Err(error) => return Err(error),
                Ok(count) => {
                    if capture_output {
                        let mut captured = shared.lock().unwrap_or_else(|error| error.into_inner());
                        crate::process_capture::append_tail(
                            &mut captured,
                            &buffer[..count],
                            CAPTURE_LIMIT_BYTES,
                        );
                    }
                    let text = decode_utf8_incremental(&mut utf8_pending, &buffer[..count], false);
                    emit_output(
                        &sink,
                        &operation_id,
                        stream,
                        &text,
                        capture_output,
                        &mut emitted_bytes,
                        &mut emitted_omission_notice,
                    );
                }
            }
        }
        Ok(())
    });
    StreamCapture { bytes, handle }
}

/// Join a finished reader (surfacing read errors/panics), or detach a stuck
/// one and return the bytes captured so far. A detached thread holds only its
/// bounded tail and exits when the leaked pipe finally closes.
fn collect_reader(capture: StreamCapture, stream: &'static str) -> Result<Vec<u8>, OperationError> {
    if capture.handle.is_finished() {
        capture
            .handle
            .join()
            .map_err(|_| OperationError::ReaderPanicked(stream))?
            .map_err(|source| OperationError::OutputRead { stream, source })?;
    }
    let mut bytes = capture
        .bytes
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    Ok(std::mem::take(&mut *bytes))
}

fn emit_output(
    sink: &EventSink,
    operation_id: &str,
    stream: &'static str,
    text: &str,
    capture_output: bool,
    emitted_bytes: &mut usize,
    emitted_omission_notice: &mut bool,
) {
    if text.is_empty() {
        return;
    }
    let visible = if capture_output {
        let remaining = CAPTURE_LIMIT_BYTES.saturating_sub(*emitted_bytes);
        let mut end = remaining.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    } else {
        text
    };
    if !visible.is_empty() {
        *emitted_bytes = (*emitted_bytes).saturating_add(visible.len());
        sink(OperationEvent {
            operation_id: operation_id.to_string(),
            kind: OperationEventKind::Output,
            stream: Some(stream),
            chunk: Some(visible.to_string()),
            message: None,
            elapsed_ms: None,
            attempt: None,
        });
    }
    if capture_output && visible.len() < text.len() && !*emitted_omission_notice {
        *emitted_omission_notice = true;
        sink(OperationEvent {
            operation_id: operation_id.to_string(),
            kind: OperationEventKind::Output,
            stream: Some(stream),
            chunk: Some(format!(
                "\n[additional {stream} output omitted from the live view]\n"
            )),
            message: None,
            elapsed_ms: None,
            attempt: None,
        });
    }
}

fn decode_utf8_incremental(pending: &mut Vec<u8>, chunk: &[u8], eof: bool) -> String {
    pending.extend_from_slice(chunk);
    let mut output = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(text) => {
                output.push_str(text);
                pending.clear();
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid > 0 {
                    output.push_str(
                        std::str::from_utf8(&pending[..valid])
                            .expect("valid_up_to prefix is valid UTF-8"),
                    );
                    pending.drain(..valid);
                }
                match error.error_len() {
                    Some(length) => {
                        output.push('\u{fffd}');
                        pending.drain(..length.min(pending.len()));
                    }
                    None if eof => {
                        output.push_str(&String::from_utf8_lossy(pending));
                        pending.clear();
                        break;
                    }
                    None => break,
                }
            }
        }
    }
    output
}

fn saturating_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_events() -> EventSink {
        Arc::new(|_| {})
    }

    #[test]
    fn ids_are_bounded_and_shell_safe() {
        assert!(valid_operation_id("pull-12345678"));
        assert!(!valid_operation_id("short"));
        assert!(!valid_operation_id("operation/with/path"));
        assert!(!valid_operation_id(&"x".repeat(129)));
    }

    #[test]
    fn capture_retains_only_the_bounded_tail() {
        let mut captured = vec![b'a'; CAPTURE_LIMIT_BYTES - 2];
        crate::process_capture::append_tail(&mut captured, b"wxyz", CAPTURE_LIMIT_BYTES);
        assert_eq!(captured.len(), CAPTURE_LIMIT_BYTES);
        assert_eq!(&captured[captured.len() - 4..], b"wxyz");
    }

    #[test]
    fn utf8_chunks_preserve_codepoints_split_at_read_boundaries() {
        let text = "Привет 📱";
        let bytes = text.as_bytes();
        let split = bytes.len() - 2;
        let mut pending = Vec::new();
        let first = decode_utf8_incremental(&mut pending, &bytes[..split], false);
        let second = decode_utf8_incremental(&mut pending, &bytes[split..], false);
        assert_eq!(format!("{first}{second}"), text);
        assert!(pending.is_empty());
    }

    #[test]
    fn captured_live_output_is_capped_and_reports_omission_once() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured_events = Arc::clone(&events);
        let sink: EventSink = Arc::new(move |event| captured_events.lock().unwrap().push(event));
        let mut emitted = 0;
        let mut notice = false;
        let oversized = "x".repeat(CAPTURE_LIMIT_BYTES + 128);
        emit_output(
            &sink,
            "bounded-output-test",
            "stdout",
            &oversized,
            true,
            &mut emitted,
            &mut notice,
        );
        emit_output(
            &sink,
            "bounded-output-test",
            "stdout",
            "more output",
            true,
            &mut emitted,
            &mut notice,
        );

        assert_eq!(emitted, CAPTURE_LIMIT_BYTES);
        let chunks = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| event.chunk.as_deref())
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            chunks
                .iter()
                .filter(|chunk| chunk.contains("output omitted"))
                .count(),
            1
        );
        assert_eq!(chunks[0].len(), CAPTURE_LIMIT_BYTES);
    }

    #[test]
    fn rapid_normal_or_nonzero_stream_exit_exhausts_reconnect_budget() {
        for code in [Some(0), Some(7), None] {
            assert!(matches!(
                stream_failure(Ok(ProcessOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    code,
                    timed_out: false,
                    cancelled: false,
                })),
                OperationError::UnexpectedExit(actual) if actual == code
            ));
        }
        assert!(!rapid_stream_failures_exhausted(4, Duration::from_secs(1)));
        assert!(rapid_stream_failures_exhausted(5, Duration::from_secs(1)));
        assert!(!rapid_stream_failures_exhausted(5, Duration::from_secs(31)));
    }

    #[test]
    fn oversized_stage_input_is_rejected_before_any_spawn() {
        let mut operation =
            RegisteredOperation::new("stage-input-cap-test", "cap test", no_events()).unwrap();
        let oversized = vec![b'x'; MAX_STAGE_INPUT_BYTES + 1];
        let error = operation
            .run_stage_with_input(
                Path::new("droidsmith-nonexistent-tool"),
                &[],
                &oversized,
                Duration::from_secs(1),
                "oversized input",
            )
            .unwrap_err();
        assert!(
            matches!(&error, OperationError::Input(source)
                if source.kind() == std::io::ErrorKind::InvalidInput),
            "unexpected error: {error}"
        );
        assert!(error.to_string().contains("stdin pipe deadlock"));

        // Input exactly at the cap passes the guard (and fails later only
        // because the program does not exist).
        let at_cap = vec![b'x'; MAX_STAGE_INPUT_BYTES];
        let error = operation
            .run_stage_with_input(
                Path::new("droidsmith-nonexistent-tool"),
                &[],
                &at_cap,
                Duration::from_secs(1),
                "at-cap input",
            )
            .unwrap_err();
        assert!(matches!(error, OperationError::Spawn { .. }));
    }

    #[test]
    fn cancellation_kills_and_reaps_the_child() {
        #[cfg(windows)]
        let (program, args) = (
            Path::new("powershell.exe"),
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Start-Sleep -Seconds 10".to_string(),
            ],
        );
        #[cfg(not(windows))]
        let (program, args) = (Path::new("sleep"), vec!["10".to_string()]);

        let id = "cancel-kill-test";
        let thread = std::thread::spawn(move || {
            run_process(
                program,
                &args,
                Duration::from_secs(20),
                id,
                "test",
                no_events(),
            )
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active
            .contains_key(id)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(cancel(id));
        let started = Instant::now();
        assert!(matches!(
            thread.join().unwrap(),
            Err(OperationError::Cancelled)
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!cancel(id));
    }

    #[test]
    fn cancel_before_registration_prevents_process_spawn() {
        let id = "cancel-before-register-test";
        assert!(cancel(id));
        let result = run_process(
            Path::new("definitely-missing-droidsmith-test-program"),
            &[],
            Duration::from_secs(2),
            id,
            "test",
            no_events(),
        );
        assert!(matches!(result, Err(OperationError::Cancelled)));
        assert!(!cancel(id));
    }

    #[test]
    fn sequence_stops_after_the_first_failed_stage() {
        #[cfg(windows)]
        let (program, fail_args, later_args) = (
            Path::new("cmd.exe"),
            vec!["/C".to_string(), "exit 7".to_string()],
            vec!["/C".to_string(), "exit 0".to_string()],
        );
        #[cfg(not(windows))]
        let (program, fail_args, later_args) = (
            Path::new("sh"),
            vec!["-c".to_string(), "exit 7".to_string()],
            vec!["-c".to_string(), "exit 0".to_string()],
        );
        let stages = vec![
            ("fails".to_string(), fail_args),
            ("must not run".to_string(), later_args),
        ];
        let cancellation = register_cancellable("sequence-failure-test").unwrap();
        let outputs = run_registered_sequence(
            program,
            &stages,
            Duration::from_secs(2),
            "sequence-failure-test",
            no_events(),
            &cancellation,
        )
        .unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].code, Some(7));
    }
}
