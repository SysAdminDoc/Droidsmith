use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::adb::DeviceTarget;
use crate::captured_tail::{sanitize_log, CapturedTail};

const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const ENCODER_PROBE_TIMEOUT: Duration = Duration::from_secs(15);

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
    #[serde(default)]
    pub max_fps: Option<u32>,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub always_on_top: bool,
    #[serde(default)]
    pub no_control: bool,
    #[serde(default)]
    pub crop: Option<String>,
    #[serde(default)]
    pub display_orientation: Option<String>,
    #[serde(default)]
    pub screen_off_timeout: Option<u32>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    #[serde(default)]
    pub new_display: Option<String>,
    #[serde(default)]
    pub audio_source: Option<String>,
    /// `"camera"` selects camera mirroring; anything else (or unset) is the
    /// default display source.
    #[serde(default)]
    pub video_source: Option<String>,
    #[serde(default)]
    pub camera_facing: Option<String>,
    #[serde(default)]
    pub camera_size: Option<String>,
    /// `--display-ime-policy=local|hide|fallback` — where the soft keyboard
    /// renders relative to a virtual display (scrcpy 3.2+).
    #[serde(default)]
    pub display_ime_policy: Option<String>,
    /// `--no-vd-destroy-content` — keep apps on a virtual display alive after
    /// the scrcpy window closes (scrcpy 3.1+).
    #[serde(default)]
    pub no_vd_destroy_content: bool,
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
    /// `--new-display` (virtual/secondary display) landed in scrcpy 3.0.
    pub supports_new_display: bool,
    /// The expanded `--audio-source=mic-*`/`voice-call`/`playback` set landed in
    /// scrcpy 3.2. `output` and `mic` predate it.
    pub supports_audio_source_expansion: bool,
    /// Camera mirroring (`--video-source=camera`) landed in scrcpy 2.7.
    pub supports_camera: bool,
    /// `--display-ime-policy` (soft-keyboard placement on a virtual display)
    /// landed in scrcpy 3.2.
    pub supports_display_ime_policy: bool,
    /// `--no-vd-destroy-content` (keep virtual-display apps alive on close)
    /// landed in scrcpy 3.1.
    pub supports_no_vd_destroy_content: bool,
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
    record_path: Option<PathBuf>,
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
        supports_new_display: version_gte(&version, 3, 0),
        supports_audio_source_expansion: version_gte(&version, 3, 2),
        supports_camera: version_gte(&version, 2, 7),
        supports_display_ime_policy: version_gte(&version, 3, 2),
        supports_no_vd_destroy_content: version_gte(&version, 3, 1),
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
            record_path: record_path.map(Path::to_path_buf),
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

/// Return a completed recording only after scrcpy has exited and closed it.
/// The command layer then registers this exact regular file for Reveal/Open
/// With; merely selecting a recording destination grants no such authority.
pub fn finished_recording(session_id: u64) -> Result<Option<String>, String> {
    let guard = sessions()
        .lock()
        .map_err(|_| "scrcpy session supervisor lock poisoned".to_string())?;
    let managed = guard
        .get(&session_id)
        .ok_or_else(|| format!("scrcpy session {session_id} is not tracked"))?;
    if managed.session.state == ScrcpySessionState::Running {
        return Ok(None);
    }
    let Some(path) = managed.record_path.as_deref() else {
        return Ok(None);
    };
    let metadata = std::fs::symlink_metadata(path).ok();
    Ok(metadata
        .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .map(|_| crate::fs_util::display_path(path)))
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

const VALID_DISPLAY_ORIENTATIONS: [&str; 8] = [
    "0", "90", "180", "270", "flip0", "flip90", "flip180", "flip270",
];

/// scrcpy audio sources. `output` (the default) is never emitted; the rest are
/// allowlisted so no argument metacharacters can slip through the transport.
const AUDIO_SOURCES: [&str; 6] = [
    "mic",
    "mic-unprocessed",
    "mic-voice-communication",
    "mic-voice-recognition",
    "voice-call",
    "playback",
];

/// Audio sources that require scrcpy 3.2+ (`mic` predates it).
const AUDIO_SOURCES_V3_2: [&str; 5] = [
    "mic-unprocessed",
    "mic-voice-communication",
    "mic-voice-recognition",
    "voice-call",
    "playback",
];

/// scrcpy `--new-display` is `<width>x<height>`, `<width>x<height>/<dpi>`, or
/// `/<dpi>` (all non-negative integers). Accept only digits, one `x`, and one
/// optional `/` so no argument metacharacters reach the device transport.
fn valid_new_display(value: &str) -> bool {
    let (size, dpi) = match value.split_once('/') {
        Some((size, dpi)) => (size, Some(dpi)),
        None => (value, None),
    };
    // Size is either "<width>x<height>" or empty (only valid in the "/<dpi>" form).
    let size_ok = if size.is_empty() {
        dpi.is_some()
    } else {
        match size.split_once('x') {
            Some((w, h)) => is_small_int(w) && is_small_int(h),
            None => false,
        }
    };
    // A dpi, when present, must be a small integer; its absence is fine.
    let dpi_ok = match dpi {
        Some(dpi) => is_small_int(dpi),
        None => true,
    };
    size_ok && dpi_ok
}

fn is_small_int(value: &str) -> bool {
    !value.is_empty() && value.len() <= 5 && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// scrcpy `--crop` is `width:height:x:y` (all non-negative integers). Accept
/// only digits and exactly three colons so no shell/argument metacharacters can
/// slip through the join into the device transport.
fn valid_crop(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 6 && part.bytes().all(|b| b.is_ascii_digit())
        })
}

pub fn build_args(
    request: &LaunchScrcpyRequest,
    record_path: Option<&Path>,
    capabilities: &ScrcpyCapabilities,
) -> Result<Vec<String>, String> {
    let mut args = vec!["-s".to_string(), request.serial.clone()];

    // Camera mirroring is a video-source *mode* change: the device screen is not
    // captured, so display-geometry flags (crop, orientation, new/flex display,
    // touch overlay, turn-screen-off) do not apply and are suppressed below.
    let camera_mode = request.video_source.as_deref() == Some("camera");
    if camera_mode {
        if !capabilities.supports_camera {
            return Err("camera mirroring requires scrcpy 2.7 or later".to_string());
        }
        args.push("--video-source=camera".to_string());
        if let Some(facing) = request
            .camera_facing
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if !matches!(facing, "front" | "back" | "external") {
                return Err(format!("unsupported scrcpy camera facing: {facing}"));
            }
            args.push(format!("--camera-facing={facing}"));
        }
        if let Some(size) = request
            .camera_size
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            match size.split_once('x') {
                Some((w, h)) if is_small_int(w) && is_small_int(h) => {
                    args.push(format!("--camera-size={size}"));
                }
                _ => {
                    return Err(format!(
                        "scrcpy camera size must be <width>x<height>: {size}"
                    ))
                }
            }
        }
    }

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
    if !camera_mode && request.turn_screen_off {
        args.push("--turn-screen-off".to_string());
    }
    if request.stay_awake {
        args.push("--stay-awake".to_string());
    }
    if !camera_mode && request.show_touches {
        args.push("--show-touches".to_string());
    }
    if let Some(max_fps) = request.max_fps.filter(|value| *value > 0) {
        args.push(format!("--max-fps={max_fps}"));
    }
    if request.fullscreen {
        args.push("--fullscreen".to_string());
    }
    if request.always_on_top {
        args.push("--always-on-top".to_string());
    }
    if request.no_control {
        args.push("--no-control".to_string());
    }
    if let Some(crop) = request
        .crop
        .as_deref()
        .map(str::trim)
        .filter(|value| !camera_mode && !value.is_empty())
    {
        if !valid_crop(crop) {
            return Err(format!(
                "scrcpy crop must be width:height:x:y (digits and colons): {crop}"
            ));
        }
        args.push(format!("--crop={crop}"));
    }
    if let Some(orientation) = request
        .display_orientation
        .as_deref()
        .map(str::trim)
        .filter(|value| !camera_mode && !value.is_empty())
    {
        if !VALID_DISPLAY_ORIENTATIONS.contains(&orientation) {
            return Err(format!(
                "unsupported scrcpy display orientation: {orientation}"
            ));
        }
        args.push(format!("--display-orientation={orientation}"));
    }
    if let Some(timeout) = request.screen_off_timeout.filter(|value| *value > 0) {
        args.push(format!("--screen-off-timeout={timeout}"));
    }
    if let Some(codec) = request
        .audio_codec
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !matches!(codec, "opus" | "aac" | "flac" | "raw") {
            return Err(format!("unsupported scrcpy audio codec: {codec}"));
        }
        args.push(format!("--audio-codec={codec}"));
    }
    if let Some(source) = request
        .audio_source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "output")
    {
        if !AUDIO_SOURCES.contains(&source) {
            return Err(format!("unsupported scrcpy audio source: {source}"));
        }
        if AUDIO_SOURCES_V3_2.contains(&source) && !capabilities.supports_audio_source_expansion {
            return Err(format!(
                "scrcpy audio source {source} requires scrcpy 3.2 or later"
            ));
        }
        args.push(format!("--audio-source={source}"));
    }
    if let Some(new_display) = request
        .new_display
        .as_deref()
        .map(str::trim)
        .filter(|value| !camera_mode && !value.is_empty())
    {
        if !capabilities.supports_new_display {
            return Err("virtual display (--new-display) requires scrcpy 3.0 or later".to_string());
        }
        if !valid_new_display(new_display) {
            return Err(format!(
                "scrcpy new-display must be <width>x<height>, <width>x<height>/<dpi>, or /<dpi>: {new_display}"
            ));
        }
        args.push(format!("--new-display={new_display}"));
    }
    if !camera_mode && request.flex_display {
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
    if let Some(policy) = request
        .display_ime_policy
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !capabilities.supports_display_ime_policy {
            return Err(
                "display IME policy (--display-ime-policy) requires scrcpy 3.2 or later"
                    .to_string(),
            );
        }
        if !matches!(policy, "local" | "hide" | "fallback") {
            return Err(format!(
                "scrcpy display-ime-policy must be local, hide, or fallback: {policy}"
            ));
        }
        args.push(format!("--display-ime-policy={policy}"));
    }
    if request.no_vd_destroy_content {
        if !capabilities.supports_no_vd_destroy_content {
            return Err(
                "keeping virtual-display content (--no-vd-destroy-content) requires scrcpy 3.1 or later"
                    .to_string(),
            );
        }
        args.push("--no-vd-destroy-content".to_string());
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
    command.args(args);
    let output = crate::process_capture::run(
        &mut command,
        timeout,
        crate::process_capture::CaptureLimits::default(),
    )
    .map_err(|error| format!("failed to run scrcpy probe: {error}"))?;
    let status = match output.termination {
        crate::process_capture::CaptureTermination::Exited(status) => status,
        crate::process_capture::CaptureTermination::TimedOut => {
            return Err(format!("scrcpy probe timed out after {timeout:?}"));
        }
        crate::process_capture::CaptureTermination::OutputLimitExceeded {
            stream,
            limit_bytes,
        } => {
            return Err(format!(
                "scrcpy probe {stream} exceeded the {limit_bytes}-byte capture limit"
            ));
        }
    };
    Ok(ProbeOutput {
        stdout: sanitize_log(&String::from_utf8_lossy(&output.stdout)),
        stderr: sanitize_log(&String::from_utf8_lossy(&output.stderr)),
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
            max_fps: None,
            fullscreen: false,
            always_on_top: false,
            no_control: false,
            crop: None,
            display_orientation: None,
            screen_off_timeout: None,
            audio_codec: None,
            new_display: None,
            audio_source: None,
            video_source: None,
            camera_facing: None,
            camera_size: None,
            display_ime_policy: None,
            no_vd_destroy_content: false,
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
            supports_new_display: true,
            supports_audio_source_expansion: true,
            supports_camera: true,
            supports_display_ime_policy: true,
            supports_no_vd_destroy_content: true,
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
    fn emits_window_and_control_flags() {
        let mut req = request();
        req.max_fps = Some(60);
        req.fullscreen = true;
        req.always_on_top = true;
        req.no_control = true;
        let args = build_args(&req, None, &capabilities()).unwrap();
        assert!(args.contains(&"--max-fps=60".to_string()));
        assert!(args.contains(&"--fullscreen".to_string()));
        assert!(args.contains(&"--always-on-top".to_string()));
        assert!(args.contains(&"--no-control".to_string()));
    }

    #[test]
    fn omits_window_flags_when_unset() {
        // A zero/absent max-fps and unset toggles must not emit any flag.
        let mut req = request();
        req.max_fps = Some(0);
        let args = build_args(&req, None, &capabilities()).unwrap();
        assert!(!args.iter().any(|arg| arg.starts_with("--max-fps")));
        assert!(!args.contains(&"--fullscreen".to_string()));
        assert!(!args.contains(&"--always-on-top".to_string()));
        assert!(!args.contains(&"--no-control".to_string()));
    }

    #[test]
    fn emits_and_gates_display_ime_policy_and_vd_content() {
        let mut req = request();
        req.display_ime_policy = Some("local".to_string());
        req.no_vd_destroy_content = true;
        let args = build_args(&req, None, &capabilities()).unwrap();
        assert!(args.contains(&"--display-ime-policy=local".to_string()));
        assert!(args.contains(&"--no-vd-destroy-content".to_string()));

        // An unknown IME policy is rejected.
        let mut bad = request();
        bad.display_ime_policy = Some("elsewhere".to_string());
        assert!(build_args(&bad, None, &capabilities())
            .unwrap_err()
            .contains("display-ime-policy"));

        // Both flags are gated on their introducing scrcpy version.
        let mut old = capabilities();
        old.version = "3.0".to_string();
        old.supports_display_ime_policy = false;
        old.supports_no_vd_destroy_content = false;
        let mut ime = request();
        ime.display_ime_policy = Some("hide".to_string());
        assert!(build_args(&ime, None, &old).unwrap_err().contains("3.2"));
        let mut vd = request();
        vd.no_vd_destroy_content = true;
        assert!(build_args(&vd, None, &old).unwrap_err().contains("3.1"));
    }

    #[test]
    fn emits_crop_orientation_timeout_and_audio_codec() {
        let mut req = request();
        req.crop = Some("1224:1440:0:0".to_string());
        req.display_orientation = Some("90".to_string());
        req.screen_off_timeout = Some(300);
        req.audio_codec = Some("opus".to_string());
        let args = build_args(&req, None, &capabilities()).unwrap();
        assert!(args.contains(&"--crop=1224:1440:0:0".to_string()));
        assert!(args.contains(&"--display-orientation=90".to_string()));
        assert!(args.contains(&"--screen-off-timeout=300".to_string()));
        assert!(args.contains(&"--audio-codec=opus".to_string()));
    }

    #[test]
    fn rejects_malformed_crop_orientation_and_audio_codec() {
        let mut bad_crop = request();
        bad_crop.crop = Some("1224x1440".to_string());
        assert!(build_args(&bad_crop, None, &capabilities())
            .unwrap_err()
            .contains("crop"));

        let mut bad_crop_meta = request();
        bad_crop_meta.crop = Some("1224:1440:0:$(rm)".to_string());
        assert!(build_args(&bad_crop_meta, None, &capabilities()).is_err());

        let mut bad_orientation = request();
        bad_orientation.display_orientation = Some("45".to_string());
        assert!(build_args(&bad_orientation, None, &capabilities())
            .unwrap_err()
            .contains("orientation"));

        let mut bad_codec = request();
        bad_codec.audio_codec = Some("mp3".to_string());
        assert!(build_args(&bad_codec, None, &capabilities())
            .unwrap_err()
            .contains("audio codec"));
    }

    #[test]
    fn emits_new_display_and_audio_source_when_supported() {
        let mut req = request();
        req.new_display = Some("1920x1080/240".to_string());
        req.audio_source = Some("mic-unprocessed".to_string());
        let args = build_args(&req, None, &capabilities()).unwrap();
        assert!(args.contains(&"--new-display=1920x1080/240".to_string()));
        assert!(args.contains(&"--audio-source=mic-unprocessed".to_string()));

        // The default "output" audio source is never emitted.
        let mut default_source = request();
        default_source.audio_source = Some("output".to_string());
        let args = build_args(&default_source, None, &capabilities()).unwrap();
        assert!(!args.iter().any(|arg| arg.starts_with("--audio-source")));
    }

    #[test]
    fn accepts_new_display_size_and_dpi_forms() {
        for value in ["1920x1080", "1920x1080/240", "/320"] {
            let mut req = request();
            req.new_display = Some(value.to_string());
            assert!(
                build_args(&req, None, &capabilities()).is_ok(),
                "new-display {value} should be accepted"
            );
        }
    }

    #[test]
    fn rejects_new_display_and_audio_source_when_unsupported_or_malformed() {
        // Gated off on older scrcpy.
        let mut old = capabilities();
        old.supports_new_display = false;
        old.supports_audio_source_expansion = false;
        let mut req = request();
        req.new_display = Some("1920x1080".to_string());
        assert!(build_args(&req, None, &old)
            .unwrap_err()
            .contains("new-display"));

        let mut expanded = request();
        expanded.audio_source = Some("voice-call".to_string());
        assert!(build_args(&expanded, None, &old)
            .unwrap_err()
            .contains("3.2"));

        // Malformed / metacharacter-bearing values are rejected.
        for bad in ["1920", "1920x", "1920x1080/", "1920x1080/$(x)", "/"] {
            let mut req = request();
            req.new_display = Some(bad.to_string());
            assert!(
                build_args(&req, None, &capabilities()).is_err(),
                "new-display {bad} should be rejected"
            );
        }

        let mut bad_source = request();
        bad_source.audio_source = Some("speaker".to_string());
        assert!(build_args(&bad_source, None, &capabilities())
            .unwrap_err()
            .contains("audio source"));
    }

    #[test]
    fn camera_mode_emits_camera_args_and_suppresses_display_flags() {
        let mut req = request();
        req.video_source = Some("camera".to_string());
        req.camera_facing = Some("front".to_string());
        req.camera_size = Some("1920x1080".to_string());
        // Display-only flags must be suppressed in camera mode.
        req.crop = Some("1224:1440:0:0".to_string());
        req.display_orientation = Some("90".to_string());
        req.new_display = Some("1920x1080".to_string());
        req.show_touches = true;
        req.turn_screen_off = true;
        req.flex_display = true;

        let args = build_args(&req, None, &capabilities()).unwrap();
        assert!(args.contains(&"--video-source=camera".to_string()));
        assert!(args.contains(&"--camera-facing=front".to_string()));
        assert!(args.contains(&"--camera-size=1920x1080".to_string()));
        for suppressed in [
            "--crop",
            "--display-orientation",
            "--new-display",
            "--show-touches",
            "--turn-screen-off",
            "--flex-display",
        ] {
            assert!(
                !args.iter().any(|arg| arg.starts_with(suppressed)),
                "{suppressed} must be suppressed in camera mode"
            );
        }
    }

    #[test]
    fn camera_mode_rejected_on_old_scrcpy_and_bad_facing() {
        let mut old = capabilities();
        old.supports_camera = false;
        let mut req = request();
        req.video_source = Some("camera".to_string());
        assert!(build_args(&req, None, &old).unwrap_err().contains("2.7"));

        let mut bad_facing = request();
        bad_facing.video_source = Some("camera".to_string());
        bad_facing.camera_facing = Some("sideways".to_string());
        assert!(build_args(&bad_facing, None, &capabilities())
            .unwrap_err()
            .contains("camera facing"));
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
