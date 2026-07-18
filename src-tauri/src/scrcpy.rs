use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::adb::DeviceTarget;

const LOG_CAPTURE_BYTES: usize = 64 * 1024;
const EXPOSED_LOG_CHARS: usize = 16 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const ENCODER_PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(specta::Type, Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LaunchScrcpyRequest {
    pub serial: String,
    pub target: DeviceTarget,
    pub max_size: Option<u32>,
    pub bit_rate: Option<String>,
    pub no_audio: bool,
    pub keyboard_mode: Option<String>,
    pub video_codec: Option<String>,
    pub video_encoder: Option<String>,
    pub turn_screen_off: bool,
    pub stay_awake: bool,
    pub show_touches: bool,
    pub flex_display: bool,
    pub keep_active: bool,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScrcpyVideoEncoder {
    pub codec: String,
    pub name: String,
    pub software: bool,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScrcpyCapabilities {
    pub path: String,
    pub version: String,
    pub available_video_codecs: Vec<String>,
    pub video_encoders: Vec<ScrcpyVideoEncoder>,
    pub probe_warning: Option<String>,
    pub cache_hit: bool,
    pub supports_flex_display: bool,
    pub supports_keep_active: bool,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScrcpySession {
    pub id: u64,
    pub serial: String,
    pub pid: u32,
    pub args: Vec<String>,
    pub started_at: String,
    pub state: ScrcpySessionState,
    pub exit_code: Option<i32>,
    pub exit_reason: Option<ScrcpyExitReason>,
    pub stderr_tail: String,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrcpySessionState {
    Running,
    Exited,
    Stopped,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrcpyExitReason {
    UserStopped,
    UnsupportedOption,
    DeviceDisconnected,
    EncoderFailed,
    PermissionDenied,
    AdbFailed,
    Signaled,
    ProcessExited,
}

struct ManagedScrcpySession {
    session: ScrcpySession,
    child: Child,
    stderr: CapturedTail,
}

#[derive(Clone)]
struct CapturedTail {
    bytes: Arc<Mutex<Vec<u8>>>,
    done: Arc<AtomicBool>,
}

impl CapturedTail {
    fn spawn<R>(mut reader: R) -> Self
    where
        R: Read + Send + 'static,
    {
        let capture = Self {
            bytes: Arc::new(Mutex::new(Vec::new())),
            done: Arc::new(AtomicBool::new(false)),
        };
        let bytes = Arc::clone(&capture.bytes);
        let done = Arc::clone(&capture.done);
        thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let mut captured = bytes.lock().unwrap_or_else(|error| error.into_inner());
                        append_tail(&mut captured, &buffer[..count], LOG_CAPTURE_BYTES);
                    }
                }
            }
            done.store(true, Ordering::Release);
        });
        capture
    }

    fn snapshot(&self) -> String {
        let bytes = self.bytes.lock().unwrap_or_else(|error| error.into_inner());
        sanitize_log(&String::from_utf8_lossy(&bytes))
    }

    fn wait_for_eof(&self) {
        let started = Instant::now();
        while !self.done.load(Ordering::Acquire) && started.elapsed() < Duration::from_millis(150) {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BinaryFingerprint {
    path: PathBuf,
    length: u64,
    modified: SystemTime,
}

impl BinaryFingerprint {
    fn read(path: &Path) -> Result<Self, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("could not inspect scrcpy binary: {error}"))?;
        Ok(Self {
            path: path.to_path_buf(),
            length: metadata.len(),
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        })
    }
}

#[derive(Clone)]
struct CachedCapabilities {
    binary: BinaryFingerprint,
    target_key: String,
    value: ScrcpyCapabilities,
}

fn capability_cache() -> &'static Mutex<Option<CachedCapabilities>> {
    static CACHE: OnceLock<Mutex<Option<CachedCapabilities>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn sessions() -> &'static Mutex<HashMap<u64, ManagedScrcpySession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<u64, ManagedScrcpySession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_session_id() -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn capabilities(
    scrcpy_path: &Path,
    target: &DeviceTarget,
) -> Result<ScrcpyCapabilities, String> {
    let binary = BinaryFingerprint::read(scrcpy_path)?;
    let target_key = format!(
        "{}|{:?}|{}|{:?}",
        target.serial, target.transport_id, target.connection_generation, target.build_fingerprint
    );
    {
        let cache = capability_cache()
            .lock()
            .map_err(|_| "scrcpy capability cache lock poisoned".to_string())?;
        if let Some(cached) = cache.as_ref() {
            if cached.binary == binary && cached.target_key == target_key {
                let mut value = cached.value.clone();
                value.cache_hit = true;
                return Ok(value);
            }
        }
    }

    let version_output = run_probe(scrcpy_path, &["--version"], PROBE_TIMEOUT)?;
    if version_output.code != Some(0) {
        return Err(format!(
            "scrcpy version probe failed: {}",
            probe_failure_text(&version_output)
        ));
    }
    let version = parse_version(&version_output.combined())
        .ok_or_else(|| "scrcpy version output was not recognized".to_string())?;

    let mut warnings = Vec::new();
    let tool_codecs = match run_probe(scrcpy_path, &["--help"], PROBE_TIMEOUT) {
        Ok(output) if output.code == Some(0) => parse_tool_video_codecs(&output.combined()),
        Ok(output) => {
            warnings.push(format!(
                "scrcpy help probe failed: {}",
                probe_failure_text(&output)
            ));
            vec!["h264".to_string()]
        }
        Err(error) => {
            warnings.push(error);
            vec!["h264".to_string()]
        }
    };

    let encoder_args = ["-s", target.serial.as_str(), "--list-encoders"];
    let mut video_encoders = match run_probe(scrcpy_path, &encoder_args, ENCODER_PROBE_TIMEOUT) {
        Ok(output) if output.code == Some(0) => parse_video_encoders(&output.combined()),
        Ok(output) => {
            warnings.push(format!(
                "device encoder probe failed: {}",
                probe_failure_text(&output)
            ));
            Vec::new()
        }
        Err(error) => {
            warnings.push(error);
            Vec::new()
        }
    };
    video_encoders.retain(|encoder| tool_codecs.contains(&encoder.codec));

    let mut available_video_codecs = Vec::new();
    for codec in &tool_codecs {
        if video_encoders.iter().any(|encoder| &encoder.codec == codec) {
            available_video_codecs.push(codec.clone());
        }
    }
    if available_video_codecs.is_empty() && tool_codecs.iter().any(|codec| codec == "h264") {
        available_video_codecs.push("h264".to_string());
    }

    let version_at_least_4 = version_gte(&version, 4, 0);
    let value = ScrcpyCapabilities {
        path: scrcpy_path.display().to_string(),
        version,
        available_video_codecs,
        video_encoders,
        probe_warning: (!warnings.is_empty()).then(|| warnings.join(" ")),
        cache_hit: false,
        supports_flex_display: version_at_least_4,
        supports_keep_active: version_at_least_4,
    };
    let mut cache = capability_cache()
        .lock()
        .map_err(|_| "scrcpy capability cache lock poisoned".to_string())?;
    *cache = Some(CachedCapabilities {
        binary,
        target_key,
        value: value.clone(),
    });
    Ok(value)
}

pub fn launch(
    scrcpy_path: &Path,
    request: LaunchScrcpyRequest,
    record_path: Option<&Path>,
    started_at: String,
    capabilities: &ScrcpyCapabilities,
) -> Result<ScrcpySession, String> {
    let args = build_args(&request, record_path, capabilities)?;
    let mut command = Command::new(scrcpy_path);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    crate::process_tree::configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to launch scrcpy: {error}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture scrcpy stderr".to_string())?;
    let stderr = CapturedTail::spawn(stderr);

    let session = ScrcpySession {
        id: next_session_id(),
        serial: request.serial,
        pid: child.id(),
        args,
        started_at,
        state: ScrcpySessionState::Running,
        exit_code: None,
        exit_reason: None,
        stderr_tail: String::new(),
    };
    let mut guard = sessions()
        .lock()
        .map_err(|_| "scrcpy session supervisor lock poisoned".to_string())?;
    reap_locked(&mut guard, None);
    guard.insert(
        session.id,
        ManagedScrcpySession {
            session: session.clone(),
            child,
            stderr,
        },
    );
    Ok(session)
}

pub fn status(session_id: u64) -> Result<ScrcpySession, String> {
    let mut guard = sessions()
        .lock()
        .map_err(|_| "scrcpy session supervisor lock poisoned".to_string())?;
    let session = {
        let managed = guard
            .get_mut(&session_id)
            .ok_or_else(|| format!("scrcpy session {session_id} is not tracked"))?;
        refresh_status(managed)?;
        managed.session.clone()
    };
    // Evict other terminated sessions so the map does not grow unbounded when a
    // long-lived renderer polls one session but never launches another. The
    // queried session is preserved so this poll still observes its final state.
    reap_locked(&mut guard, Some(session_id));
    Ok(session)
}

pub fn stop(session_id: u64) -> Result<ScrcpySession, String> {
    let mut guard = sessions()
        .lock()
        .map_err(|_| "scrcpy session supervisor lock poisoned".to_string())?;
    let managed = guard
        .get_mut(&session_id)
        .ok_or_else(|| format!("scrcpy session {session_id} is not tracked"))?;
    refresh_status(managed)?;
    if managed.session.state == ScrcpySessionState::Running {
        let exit_status = crate::process_tree::terminate(&mut managed.child)
            .map_err(|error| format!("failed to stop scrcpy session {session_id}: {error}"))?;
        managed.session.state = ScrcpySessionState::Stopped;
        managed.session.exit_code = exit_status.code();
        managed.session.exit_reason = Some(ScrcpyExitReason::UserStopped);
        managed.stderr.wait_for_eof();
        managed.session.stderr_tail = managed.stderr.snapshot();
    }
    let session = managed.session.clone();
    // Drop any other terminated sessions now that we hold the lock.
    reap_locked(&mut guard, Some(session_id));
    Ok(session)
}

fn refresh_status(managed: &mut ManagedScrcpySession) -> Result<(), String> {
    if managed.session.state != ScrcpySessionState::Running {
        return Ok(());
    }
    if let Some(status) = managed
        .child
        .try_wait()
        .map_err(|error| format!("failed to poll scrcpy session: {error}"))?
    {
        managed.session.state = ScrcpySessionState::Exited;
        managed.session.exit_code = status.code();
        managed.stderr.wait_for_eof();
        managed.session.stderr_tail = managed.stderr.snapshot();
        managed.session.exit_reason = Some(classify_exit_reason(
            status.code(),
            &managed.session.stderr_tail,
        ));
    } else {
        managed.session.stderr_tail = managed.stderr.snapshot();
    }
    Ok(())
}

fn reap_locked(sessions: &mut HashMap<u64, ManagedScrcpySession>, keep: Option<u64>) {
    for (id, managed) in sessions.iter_mut() {
        if Some(*id) != keep {
            let _ = refresh_status(managed);
        }
    }
    sessions.retain(|id, managed| {
        Some(*id) == keep || managed.session.state == ScrcpySessionState::Running
    });
}

pub fn build_args(
    request: &LaunchScrcpyRequest,
    record_path: Option<&Path>,
    capabilities: &ScrcpyCapabilities,
) -> Result<Vec<String>, String> {
    let mut args = vec!["-s".to_string(), request.serial.clone()];
    if let Some(max_size) = request.max_size.filter(|value| *value > 0) {
        args.push("--max-size".to_string());
        args.push(max_size.to_string());
    }
    if let Some(bit_rate) = request
        .bit_rate
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args.push("--video-bit-rate".to_string());
        args.push(bit_rate.to_string());
    }
    let codec = request
        .video_codec
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("h264");
    if !capabilities
        .available_video_codecs
        .iter()
        .any(|available| available == codec)
    {
        return Err(format!(
            "scrcpy codec {codec} is not available for this binary and device"
        ));
    }
    args.push(format!("--video-codec={codec}"));
    if let Some(encoder) = request
        .video_encoder
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !capabilities
            .video_encoders
            .iter()
            .any(|available| available.codec == codec && available.name == encoder)
        {
            return Err(format!(
                "scrcpy encoder {encoder} is not available for codec {codec}"
            ));
        }
        args.push(format!("--video-encoder={encoder}"));
    }
    if request.no_audio {
        args.push("--no-audio".to_string());
    }
    if let Some(record_path) = record_path {
        let extension = record_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !matches!(extension.to_ascii_lowercase().as_str(), "mp4" | "mkv") {
            return Err("scrcpy recording must use the .mp4 or .mkv extension".to_string());
        }
        args.push("--record".to_string());
        args.push(record_path.display().to_string());
    }
    if let Some(mode) = request
        .keyboard_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "default")
    {
        match mode {
            "sdk" | "uhid" | "aoa" | "disabled" => {
                args.push(format!("--keyboard={mode}"));
            }
            _ => return Err(format!("unsupported scrcpy keyboard mode: {mode}")),
        }
    }
    if request.turn_screen_off {
        args.push("--turn-screen-off".to_string());
    }
    if request.stay_awake {
        args.push("--stay-awake".to_string());
    }
    if request.show_touches {
        args.push("--show-touches".to_string());
    }
    if request.flex_display {
        if !version_gte(&capabilities.version, 4, 0) {
            return Err("flex display requires scrcpy 4.0 or later".to_string());
        }
        args.push("--flex-display".to_string());
    }
    if request.keep_active {
        if !version_gte(&capabilities.version, 4, 0) {
            return Err("keep active requires scrcpy 4.0 or later".to_string());
        }
        args.push("--keep-active".to_string());
    }
    Ok(args)
}

#[derive(Debug)]
struct ProbeOutput {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

impl ProbeOutput {
    fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

fn run_probe(program: &Path, args: &[&str], timeout: Duration) -> Result<ProbeOutput, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process_tree::configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to run scrcpy probe: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to capture scrcpy probe stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture scrcpy probe stderr".to_string())?;
    let stdout = CapturedTail::spawn(stdout);
    let stderr = CapturedTail::spawn(stderr);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                let _ = crate::process_tree::terminate(&mut child);
                return Err(format!("scrcpy probe timed out after {timeout:?}"));
            }
            Err(error) => {
                let _ = crate::process_tree::terminate(&mut child);
                return Err(format!("failed to poll scrcpy probe: {error}"));
            }
        }
    };
    stdout.wait_for_eof();
    stderr.wait_for_eof();
    Ok(ProbeOutput {
        stdout: stdout.snapshot(),
        stderr: stderr.snapshot(),
        code: status.code(),
    })
}

pub(crate) fn parse_version(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()?.eq_ignore_ascii_case("scrcpy"))
            .then(|| {
                fields
                    .next()
                    .map(|value| value.trim_start_matches('v').to_string())
            })
            .flatten()
    })
}

pub(crate) fn parse_tool_video_codecs(output: &str) -> Vec<String> {
    const CODECS: [&str; 5] = ["h264", "h265", "av1", "vp8", "vp9"];
    let lower = output.to_ascii_lowercase();
    let Some(start) = lower.find("select a video codec") else {
        return vec!["h264".to_string()];
    };
    let mut end = (start + 320).min(lower.len());
    // `to_ascii_lowercase` leaves multi-byte UTF-8 intact, so the 320-byte
    // window can land mid-character; walk back to a char boundary before
    // slicing to avoid a panic on non-ASCII scrcpy help text.
    while !lower.is_char_boundary(end) {
        end -= 1;
    }
    let window = &lower[start..end];
    let codecs: Vec<String> = CODECS
        .iter()
        .filter(|codec| window.contains(**codec))
        .map(|codec| (*codec).to_string())
        .collect();
    if codecs.is_empty() {
        vec!["h264".to_string()]
    } else {
        codecs
    }
}

pub(crate) fn parse_video_encoders(output: &str) -> Vec<ScrcpyVideoEncoder> {
    let mut encoders = Vec::new();
    for line in output.lines() {
        let Some(codec) = option_value(line, "--video-codec=") else {
            continue;
        };
        let Some(name) = option_value(line, "--video-encoder=") else {
            continue;
        };
        let encoder = ScrcpyVideoEncoder {
            codec,
            name,
            software: line.to_ascii_lowercase().contains("(sw)"),
        };
        if !encoders.contains(&encoder) {
            encoders.push(encoder);
        }
    }
    encoders
}

fn option_value(line: &str, marker: &str) -> Option<String> {
    let value = line.split_once(marker)?.1.trim_start();
    let first = value.chars().next()?;
    let result = if matches!(first, '\'' | '"') {
        value[1..].split(first).next()?
    } else {
        value.split_whitespace().next()?
    };
    let result = result.trim_matches(|character: char| character == '\'' || character == '"');
    (!result.is_empty()).then(|| result.to_string())
}

fn classify_exit_reason(code: Option<i32>, stderr: &str) -> ScrcpyExitReason {
    let lower = stderr.to_ascii_lowercase();
    if [
        "unknown option",
        "unrecognized option",
        "could not find any option",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        ScrcpyExitReason::UnsupportedOption
    } else if lower.contains("device disconnected")
        || lower.contains("device not found")
        || lower.contains("connection closed")
    {
        ScrcpyExitReason::DeviceDisconnected
    } else if lower.contains("injecting input events requires")
        || lower.contains("permission denied")
    {
        ScrcpyExitReason::PermissionDenied
    } else if lower.contains("mediacodec")
        || (lower.contains("encoder") && (lower.contains("error") || lower.contains("failed")))
    {
        ScrcpyExitReason::EncoderFailed
    } else if lower.contains("adb") && (lower.contains("error") || lower.contains("failed")) {
        ScrcpyExitReason::AdbFailed
    } else if code.is_none() {
        ScrcpyExitReason::Signaled
    } else {
        ScrcpyExitReason::ProcessExited
    }
}

fn version_gte(version: &str, major: u32, minor: u32) -> bool {
    let mut parts = version.split('.');
    let parsed_major = parts
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let parsed_minor = parts
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    (parsed_major, parsed_minor) >= (major, minor)
}

fn probe_failure_text(output: &ProbeOutput) -> String {
    let text = sanitize_log(&output.combined());
    if text.trim().is_empty() {
        format!("process exited with code {:?}", output.code)
    } else {
        text.chars().take(512).collect()
    }
}

fn append_tail(target: &mut Vec<u8>, bytes: &[u8], limit: usize) {
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

fn sanitize_log(value: &str) -> String {
    let mut chars: Vec<char> = value
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'))
        .collect();
    if chars.len() > EXPOSED_LOG_CHARS {
        chars.drain(..chars.len() - EXPOSED_LOG_CHARS);
    }
    chars.into_iter().collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        build_args, classify_exit_reason, parse_tool_video_codecs, parse_version,
        parse_video_encoders, LaunchScrcpyRequest, ScrcpyCapabilities, ScrcpyExitReason,
        ScrcpyVideoEncoder,
    };
    use crate::adb::DeviceTarget;
    use std::path::Path;

    fn request() -> LaunchScrcpyRequest {
        LaunchScrcpyRequest {
            serial: "DEVICE123".to_string(),
            target: DeviceTarget {
                serial: "DEVICE123".into(),
                transport_id: Some(1),
                connection_generation: 2,
                model: None,
                product: None,
                device: None,
                build_fingerprint: Some("build/test".into()),
                transport_kind: crate::adb::DeviceTransportKind::Usb,
                untrusted_transport_override: false,
            },
            max_size: Some(1280),
            bit_rate: Some("12M".to_string()),
            no_audio: true,
            keyboard_mode: Some("uhid".to_string()),
            video_codec: Some("h265".to_string()),
            video_encoder: Some("c2.vendor.hevc.encoder".to_string()),
            turn_screen_off: true,
            stay_awake: true,
            show_touches: true,
            flex_display: false,
            keep_active: false,
        }
    }

    fn capabilities() -> ScrcpyCapabilities {
        ScrcpyCapabilities {
            path: "scrcpy".to_string(),
            version: "4.0".to_string(),
            available_video_codecs: vec!["h264".to_string(), "h265".to_string()],
            video_encoders: vec![ScrcpyVideoEncoder {
                codec: "h265".to_string(),
                name: "c2.vendor.hevc.encoder".to_string(),
                software: false,
            }],
            probe_warning: None,
            cache_hit: false,
            supports_flex_display: true,
            supports_keep_active: true,
        }
    }

    #[test]
    fn builds_only_negotiated_scrcpy_session_args() {
        let args = build_args(&request(), Some(Path::new("session.mp4")), &capabilities()).unwrap();
        assert_eq!(
            args,
            vec![
                "-s",
                "DEVICE123",
                "--max-size",
                "1280",
                "--video-bit-rate",
                "12M",
                "--video-codec=h265",
                "--video-encoder=c2.vendor.hevc.encoder",
                "--no-audio",
                "--record",
                "session.mp4",
                "--keyboard=uhid",
                "--turn-screen-off",
                "--stay-awake",
                "--show-touches"
            ]
        );
    }

    #[test]
    fn rejects_unreported_codecs_and_encoders() {
        let mut request = request();
        request.video_codec = Some("vp9".to_string());
        assert!(build_args(&request, None, &capabilities())
            .unwrap_err()
            .contains("not available"));
        request.video_codec = Some("h265".to_string());
        request.video_encoder = Some("unreported.encoder".to_string());
        assert!(build_args(&request, None, &capabilities())
            .unwrap_err()
            .contains("not available"));
    }

    #[test]
    fn parses_current_and_future_codec_capabilities() {
        let help = "Select a video codec (h264, h265, av1, vp8 or vp9). Default is h264.";
        assert_eq!(
            parse_tool_video_codecs(help),
            vec!["h264", "h265", "av1", "vp8", "vp9"]
        );
        let output = "scrcpy 4.0 <https://github.com/Genymobile/scrcpy>\n\
[server] INFO: List of video encoders:\n\
 --video-codec=h264 --video-encoder='c2.vendor.avc.encoder' (hw) [vendor]\n\
 --video-codec=h265 --video-encoder=c2.android.hevc.encoder (sw)";
        assert_eq!(parse_version(output).as_deref(), Some("4.0"));
        assert_eq!(
            parse_video_encoders(output),
            vec![
                ScrcpyVideoEncoder {
                    codec: "h264".to_string(),
                    name: "c2.vendor.avc.encoder".to_string(),
                    software: false,
                },
                ScrcpyVideoEncoder {
                    codec: "h265".to_string(),
                    name: "c2.android.hevc.encoder".to_string(),
                    software: true,
                }
            ]
        );
    }

    #[test]
    fn classifies_actionable_exit_reasons_from_bounded_stderr() {
        assert_eq!(
            classify_exit_reason(Some(1), "[server] ERROR: MediaCodec encoder failed"),
            ScrcpyExitReason::EncoderFailed
        );
        assert_eq!(
            classify_exit_reason(Some(1), "WARN: Device disconnected"),
            ScrcpyExitReason::DeviceDisconnected
        );
        assert_eq!(
            classify_exit_reason(None, "terminated"),
            ScrcpyExitReason::Signaled
        );
    }

    #[test]
    fn rejects_unknown_keyboard_modes() {
        let mut request = request();
        request.keyboard_mode = Some("surprise".to_string());
        assert!(build_args(&request, None, &capabilities())
            .unwrap_err()
            .contains("unsupported scrcpy keyboard mode"));
    }

    #[test]
    fn emits_flex_display_and_keep_active_for_scrcpy_4() {
        let mut req = request();
        req.flex_display = true;
        req.keep_active = true;
        let args = build_args(&req, None, &capabilities()).unwrap();
        assert!(args.contains(&"--flex-display".to_string()));
        assert!(args.contains(&"--keep-active".to_string()));
    }

    #[test]
    fn rejects_flex_display_on_old_scrcpy() {
        let mut caps = capabilities();
        caps.version = "3.3.4".to_string();
        caps.supports_flex_display = false;
        caps.supports_keep_active = false;
        let mut req = request();
        req.flex_display = true;
        assert!(build_args(&req, None, &caps)
            .unwrap_err()
            .contains("scrcpy 4.0"));
    }

    #[test]
    fn rejects_keep_active_on_old_scrcpy() {
        let mut caps = capabilities();
        caps.version = "3.3.4".to_string();
        caps.supports_flex_display = false;
        caps.supports_keep_active = false;
        let mut req = request();
        req.keep_active = true;
        assert!(build_args(&req, None, &caps)
            .unwrap_err()
            .contains("scrcpy 4.0"));
    }

    #[test]
    fn version_comparison() {
        assert!(super::version_gte("4.0", 4, 0));
        assert!(super::version_gte("4.1", 4, 0));
        assert!(super::version_gte("5.0", 4, 0));
        assert!(!super::version_gte("3.3.4", 4, 0));
        assert!(!super::version_gte("3.99", 4, 0));
    }

    #[test]
    fn rejects_unsupported_recording_extensions() {
        assert!(
            build_args(&request(), Some(Path::new("session.txt")), &capabilities())
                .unwrap_err()
                .contains(".mp4 or .mkv")
        );
    }
}
