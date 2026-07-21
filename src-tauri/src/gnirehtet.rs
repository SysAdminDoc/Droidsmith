//! Gnirehtet reverse-tethering session supervision (R-084).
//!
//! Gnirehtet (same author as scrcpy) shares the PC's internet with an Android
//! device over USB — useful when the device has no Wi-Fi or is on a restricted
//! network. Like [`crate::scrcpy`], Droidsmith does not reimplement the tool;
//! it detects the `gnirehtet` binary and supervises a `gnirehtet run <serial>`
//! process so the renderer can start, poll, and stop a sharing session.
//!
//! `gnirehtet run` installs/starts the on-device client, launches the relay
//! server, and — on termination — stops the client and restores the device's
//! default network. Stopping the supervised process therefore tears the
//! session down cleanly.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::captured_tail::CapturedTail;

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GnirehtetSession {
    pub id: u64,
    pub serial: String,
    pub pid: u32,
    pub args: Vec<String>,
    pub started_at: String,
    pub state: GnirehtetSessionState,
    pub exit_code: Option<i32>,
    pub exit_reason: Option<GnirehtetExitReason>,
    pub stderr_tail: String,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GnirehtetSessionState {
    Running,
    Exited,
    Stopped,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GnirehtetExitReason {
    UserStopped,
    DeviceDisconnected,
    RelayFailed,
    ClientInstallFailed,
    AdbFailed,
    Signaled,
    ProcessExited,
}

struct ManagedSession {
    session: GnirehtetSession,
    child: Child,
    stderr: CapturedTail,
}

fn sessions() -> &'static Mutex<HashMap<u64, ManagedSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<u64, ManagedSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_session_id() -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Build the argument vector for a supervised `gnirehtet run <serial>` session.
pub fn build_args(serial: &str) -> Vec<String> {
    vec!["run".to_string(), serial.to_string()]
}

pub fn start(
    gnirehtet_path: &Path,
    serial: String,
    started_at: String,
) -> Result<GnirehtetSession, String> {
    let args = build_args(&serial);
    let mut command = Command::new(gnirehtet_path);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    crate::process_tree::configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to launch gnirehtet: {error}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture gnirehtet stderr".to_string())?;
    let stderr = CapturedTail::spawn(stderr);

    let session = GnirehtetSession {
        id: next_session_id(),
        serial,
        pid: child.id(),
        args,
        started_at,
        state: GnirehtetSessionState::Running,
        exit_code: None,
        exit_reason: None,
        stderr_tail: String::new(),
    };
    let mut guard = sessions()
        .lock()
        .map_err(|_| "gnirehtet session supervisor lock poisoned".to_string())?;
    reap_locked(&mut guard, None);
    guard.insert(
        session.id,
        ManagedSession {
            session: session.clone(),
            child,
            stderr,
        },
    );
    Ok(session)
}

/// Return the supervised session currently running for `serial`, if any, so a
/// renderer remount can re-attach instead of spawning a duplicate (persist
/// tethering across navigation). Reaping first ensures only live sessions match.
pub fn find_running_by_serial(serial: &str) -> Result<Option<GnirehtetSession>, String> {
    let mut guard = sessions()
        .lock()
        .map_err(|_| "gnirehtet session supervisor lock poisoned".to_string())?;
    reap_locked(&mut guard, None);
    // After reap_locked(None), every retained session is Running.
    let found = guard
        .values()
        .find(|managed| managed.session.serial == serial)
        .map(|managed| managed.session.clone());
    Ok(found)
}

pub fn status(session_id: u64) -> Result<GnirehtetSession, String> {
    let mut guard = sessions()
        .lock()
        .map_err(|_| "gnirehtet session supervisor lock poisoned".to_string())?;
    let session = {
        let managed = guard
            .get_mut(&session_id)
            .ok_or_else(|| format!("gnirehtet session {session_id} is not tracked"))?;
        refresh_status(managed)?;
        managed.session.clone()
    };
    reap_locked(&mut guard, Some(session_id));
    Ok(session)
}

pub fn stop(session_id: u64) -> Result<GnirehtetSession, String> {
    let mut guard = sessions()
        .lock()
        .map_err(|_| "gnirehtet session supervisor lock poisoned".to_string())?;
    let managed = guard
        .get_mut(&session_id)
        .ok_or_else(|| format!("gnirehtet session {session_id} is not tracked"))?;
    refresh_status(managed)?;
    if managed.session.state == GnirehtetSessionState::Running {
        let exit_status = crate::process_tree::terminate(&mut managed.child)
            .map_err(|error| format!("failed to stop gnirehtet session {session_id}: {error}"))?;
        managed.session.state = GnirehtetSessionState::Stopped;
        managed.session.exit_code = exit_status.code();
        managed.session.exit_reason = Some(GnirehtetExitReason::UserStopped);
        managed.stderr.wait_for_eof();
        managed.session.stderr_tail = managed.stderr.snapshot();
    }
    let session = managed.session.clone();
    reap_locked(&mut guard, Some(session_id));
    Ok(session)
}

fn refresh_status(managed: &mut ManagedSession) -> Result<(), String> {
    if managed.session.state != GnirehtetSessionState::Running {
        return Ok(());
    }
    if let Some(status) = managed
        .child
        .try_wait()
        .map_err(|error| format!("failed to poll gnirehtet session: {error}"))?
    {
        managed.session.state = GnirehtetSessionState::Exited;
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

fn reap_locked(sessions: &mut HashMap<u64, ManagedSession>, keep: Option<u64>) {
    for (id, managed) in sessions.iter_mut() {
        if Some(*id) != keep {
            let _ = refresh_status(managed);
        }
    }
    sessions.retain(|id, managed| {
        Some(*id) == keep || managed.session.state == GnirehtetSessionState::Running
    });
}

fn classify_exit_reason(code: Option<i32>, stderr: &str) -> GnirehtetExitReason {
    let haystack = stderr.to_ascii_lowercase();
    if haystack.contains("device")
        && (haystack.contains("not found") || haystack.contains("offline"))
    {
        return GnirehtetExitReason::DeviceDisconnected;
    }
    if haystack.contains("cannot start client")
        || haystack.contains("cannot install")
        || haystack.contains("install failed")
    {
        return GnirehtetExitReason::ClientInstallFailed;
    }
    if haystack.contains("cannot start relay")
        || haystack.contains("address already in use")
        || haystack.contains("relay")
    {
        return GnirehtetExitReason::RelayFailed;
    }
    if haystack.contains("adb")
        && (haystack.contains("no such file") || haystack.contains("failed"))
    {
        return GnirehtetExitReason::AdbFailed;
    }
    match code {
        None => GnirehtetExitReason::Signaled,
        Some(_) => GnirehtetExitReason::ProcessExited,
    }
}

#[cfg(test)]
mod tests {
    use super::{build_args, classify_exit_reason, GnirehtetExitReason};

    #[test]
    fn build_args_runs_the_named_serial() {
        assert_eq!(build_args("emulator-5554"), vec!["run", "emulator-5554"]);
    }

    #[test]
    fn classifies_disconnected_device() {
        assert_eq!(
            classify_exit_reason(Some(1), "error: device 'X' not found"),
            GnirehtetExitReason::DeviceDisconnected
        );
    }

    #[test]
    fn classifies_relay_port_conflict() {
        assert_eq!(
            classify_exit_reason(Some(1), "Cannot start relay server: Address already in use"),
            GnirehtetExitReason::RelayFailed
        );
    }

    #[test]
    fn classifies_client_install_failure() {
        assert_eq!(
            classify_exit_reason(Some(1), "Cannot install client"),
            GnirehtetExitReason::ClientInstallFailed
        );
    }

    #[test]
    fn classifies_signal_versus_exit_when_unmatched() {
        assert_eq!(
            classify_exit_reason(None, "interrupted"),
            GnirehtetExitReason::Signaled
        );
        assert_eq!(
            classify_exit_reason(Some(0), "shutting down"),
            GnirehtetExitReason::ProcessExited
        );
    }
}
