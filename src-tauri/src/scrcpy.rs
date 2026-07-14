use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LaunchScrcpyRequest {
    pub serial: String,
    pub max_size: Option<u32>,
    pub bit_rate: Option<String>,
    pub no_audio: bool,
    pub record_path: Option<String>,
    pub keyboard_mode: Option<String>,
    pub turn_screen_off: bool,
    pub stay_awake: bool,
    pub show_touches: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScrcpySession {
    pub id: u64,
    pub serial: String,
    pub pid: u32,
    pub args: Vec<String>,
    pub started_at: String,
    pub state: ScrcpySessionState,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrcpySessionState {
    Running,
    Exited,
    Stopped,
}

struct ManagedScrcpySession {
    session: ScrcpySession,
    child: Child,
}

fn sessions() -> &'static Mutex<HashMap<u64, ManagedScrcpySession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<u64, ManagedScrcpySession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_session_id() -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn launch(
    scrcpy_path: &Path,
    request: LaunchScrcpyRequest,
    started_at: String,
) -> Result<ScrcpySession, String> {
    let args = build_args(&request)?;
    let child = Command::new(scrcpy_path)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to launch scrcpy: {e}"))?;

    let session = ScrcpySession {
        id: next_session_id(),
        serial: request.serial,
        pid: child.id(),
        args,
        started_at,
        state: ScrcpySessionState::Running,
        exit_code: None,
    };
    let mut guard = sessions()
        .lock()
        .map_err(|_| "scrcpy session supervisor lock poisoned".to_string())?;
    reap_locked(&mut guard);
    guard.insert(
        session.id,
        ManagedScrcpySession {
            session: session.clone(),
            child,
        },
    );
    Ok(session)
}

pub fn status(session_id: u64) -> Result<ScrcpySession, String> {
    let mut guard = sessions()
        .lock()
        .map_err(|_| "scrcpy session supervisor lock poisoned".to_string())?;
    let managed = guard
        .get_mut(&session_id)
        .ok_or_else(|| format!("scrcpy session {session_id} is not tracked"))?;
    refresh_status(managed)?;
    Ok(managed.session.clone())
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
        managed
            .child
            .kill()
            .map_err(|e| format!("failed to stop scrcpy session {session_id}: {e}"))?;
        let exit_status = managed
            .child
            .wait()
            .map_err(|e| format!("failed to wait for scrcpy session {session_id}: {e}"))?;
        managed.session.state = ScrcpySessionState::Stopped;
        managed.session.exit_code = exit_status.code();
    }
    Ok(managed.session.clone())
}

fn refresh_status(managed: &mut ManagedScrcpySession) -> Result<(), String> {
    if managed.session.state != ScrcpySessionState::Running {
        return Ok(());
    }
    if let Some(status) = managed
        .child
        .try_wait()
        .map_err(|e| format!("failed to poll scrcpy session: {e}"))?
    {
        managed.session.state = ScrcpySessionState::Exited;
        managed.session.exit_code = status.code();
    }
    Ok(())
}

fn reap_locked(sessions: &mut HashMap<u64, ManagedScrcpySession>) {
    for managed in sessions.values_mut() {
        let _ = refresh_status(managed);
    }
    sessions.retain(|_, managed| managed.session.state == ScrcpySessionState::Running);
}

pub fn build_args(request: &LaunchScrcpyRequest) -> Result<Vec<String>, String> {
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
    if request.no_audio {
        args.push("--no-audio".to_string());
    }
    if let Some(record_path) = request
        .record_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        // A leading '-' would be parsed by scrcpy as an option flag rather
        // than the recording filename.
        if record_path.starts_with('-') {
            return Err(format!(
                "scrcpy recording path must not start with '-': {record_path}"
            ));
        }
        args.push("--record".to_string());
        args.push(record_path.to_string());
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
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::{build_args, LaunchScrcpyRequest};

    fn request() -> LaunchScrcpyRequest {
        LaunchScrcpyRequest {
            serial: "DEVICE123".to_string(),
            max_size: Some(1280),
            bit_rate: Some("12M".to_string()),
            no_audio: true,
            record_path: Some("session.mp4".to_string()),
            keyboard_mode: Some("uhid".to_string()),
            turn_screen_off: true,
            stay_awake: true,
            show_touches: true,
        }
    }

    #[test]
    fn builds_documented_scrcpy_session_args() {
        let args = build_args(&request()).unwrap();
        assert_eq!(
            args,
            vec![
                "-s",
                "DEVICE123",
                "--max-size",
                "1280",
                "--video-bit-rate",
                "12M",
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
    fn rejects_unknown_keyboard_modes() {
        let mut req = request();
        req.keyboard_mode = Some("surprise".to_string());
        assert!(build_args(&req)
            .unwrap_err()
            .contains("unsupported scrcpy keyboard mode"));
    }

    #[test]
    fn rejects_record_path_that_looks_like_a_flag() {
        let mut req = request();
        req.record_path = Some("--version".to_string());
        assert!(build_args(&req)
            .unwrap_err()
            .contains("must not start with"));
    }
}
