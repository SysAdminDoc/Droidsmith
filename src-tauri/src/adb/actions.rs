//! Destructive ADB actions: disable, uninstall, enable, clear data.
//!
//! Two-step API by design:
//!
//! - [`plan`] synthesises a [`PlannedAction`] from a high-level
//!   [`ActionRequest`]. No I/O. The plan can be shown to the user as a
//!   preview ("we will run `adb -s X shell pm disable-user --user 0 Y`").
//! - [`apply`] takes the plan and runs it via the supplied transport.
//!   On success it returns an [`AppliedAction`] suitable for journalling.
//!
//! This split lets the GUI do "Preview → Confirm → Apply" cleanly and
//! lets the CLI (`droidsmith-cli`) do `--dry-run` by stopping after
//! [`plan`].

use serde::{Deserialize, Serialize};

use crate::adb::device::{valid_serial, DeviceTarget};
use crate::adb::transport::{validate_device_target, AdbTransport, TransportError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// `pm disable-user --user 0 <pkg>` — reversible with `pm enable`.
    Disable,
    /// `pm enable <pkg>` — reverses Disable.
    Enable,
    /// `pm uninstall --user 0 <pkg>` — effectively permanent for the
    /// current user; the APK remains in `/system/` (system apps) but
    /// is removed from `/data/app/` (user apps).
    UninstallForUser,
    /// `pm clear <pkg>` — wipes the package's data and cache.
    ClearData,
    /// `am force-stop <pkg>` — non-destructive; included for symmetry.
    ForceStop,
}

/// Install an APK on a device using `adb install`. The APK path is a
/// local filesystem path that gets pushed to the device via adb's
/// built-in staging.
pub fn install_apk(
    adb_path: &std::path::Path,
    target: &DeviceTarget,
    apk_path: &str,
) -> Result<String, TransportError> {
    use std::io::Read as IoRead;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    if !crate::adb::device::valid_serial(&target.serial) || target.connection_generation == 0 {
        return Err(TransportError::Parse(format!(
            "invalid device target for serial {:?}",
            target.serial
        )));
    }

    let timeout = std::time::Duration::from_secs(300);
    let mut args = target.adb_selector();
    args.extend([
        "install".to_string(),
        "-r".to_string(),
        apk_path.to_string(),
    ]);
    let mut child = Command::new(adb_path)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(TransportError::Spawn)?;

    let mut stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| TransportError::Parse("no stdout pipe".to_string()))?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| TransportError::Parse("no stderr pipe".to_string()))?;

    let stdout_reader = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::with_capacity(4096);
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let stderr_reader = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::with_capacity(1024);
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => break None,
        }
    };

    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    let stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();
    let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();

    match exit_status {
        None => Err(TransportError::Timeout(timeout)),
        Some(status) => {
            if status.success() {
                if let Some(err) = pm_failure_marker(&stdout) {
                    return Err(TransportError::Exit {
                        code: 1,
                        stderr: err.to_string(),
                    });
                }
                Ok(stdout)
            } else if let Some(code) = status.code() {
                Err(TransportError::Exit { code, stderr })
            } else {
                Err(TransportError::Signaled { stderr })
            }
        }
    }
}

/// Pull an APK from a device using `adb pull`.
pub fn extract_apk(
    adb_path: &std::path::Path,
    target: &DeviceTarget,
    remote_path: &str,
    local_path: &str,
) -> Result<String, TransportError> {
    use std::io::Read as IoRead;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    if !crate::adb::device::valid_serial(&target.serial) || target.connection_generation == 0 {
        return Err(TransportError::Parse(format!(
            "invalid device target for serial {:?}",
            target.serial
        )));
    }

    let timeout = std::time::Duration::from_secs(120);
    let mut args = target.adb_selector();
    args.extend([
        "pull".to_string(),
        remote_path.to_string(),
        local_path.to_string(),
    ]);
    let mut child = Command::new(adb_path)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(TransportError::Spawn)?;

    let mut stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| TransportError::Parse("no stdout pipe".to_string()))?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| TransportError::Parse("no stderr pipe".to_string()))?;

    let stdout_reader = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::with_capacity(4096);
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let stderr_reader = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::with_capacity(1024);
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => break None,
        }
    };

    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    let stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();
    let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();

    match exit_status {
        None => Err(TransportError::Timeout(timeout)),
        Some(status) => {
            if status.success() {
                Ok(stdout)
            } else if let Some(code) = status.code() {
                Err(TransportError::Exit { code, stderr })
            } else {
                Err(TransportError::Signaled { stderr })
            }
        }
    }
}

impl ActionKind {
    /// True for actions that the journal can losslessly undo by issuing
    /// the inverse `ActionKind` on the same package. `UninstallForUser`
    /// and `ClearData` cannot be losslessly undone — undo from the
    /// journal will surface that explicitly.
    ///
    /// Kept alongside `inverse` for symmetry; the renderer will use
    /// this to disable the "Undo" button on irreversible rows.
    #[allow(dead_code)]
    pub fn is_reversible(self) -> bool {
        matches!(self, Self::Disable | Self::Enable)
    }

    pub fn inverse(self) -> Option<ActionKind> {
        match self {
            Self::Disable => Some(Self::Enable),
            Self::Enable => Some(Self::Disable),
            Self::UninstallForUser | Self::ClearData | Self::ForceStop => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    pub serial: String,
    /// Immutable live target captured from `list_devices`. Legacy journal
    /// rows deserialize to an invalid default and must be rebound by the
    /// explicit undo request before execution.
    #[serde(default)]
    pub target: DeviceTarget,
    pub package: String,
    pub kind: ActionKind,
    /// Android user id the action targets (`pm --user`). Defaults to `0`
    /// (the primary/owner user) so legacy journal entries and older
    /// callers that predate multi-user targeting deserialize unchanged.
    #[serde(default)]
    pub user_id: u32,
}

/// Synthesised plan. The `args` field is exactly what the action will
/// pass to `adb shell` — no further interpolation happens at apply
/// time. `description` is human-readable for the confirmation dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedAction {
    pub request: ActionRequest,
    pub args: Vec<String>,
    pub description: String,
}

/// Applied action — the journal record. `stdout`/`stderr` are kept so
/// support tickets can include the raw response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedAction {
    pub plan: PlannedAction,
    pub stdout: String,
    /// ISO-8601 UTC timestamp.
    pub applied_at: String,
}

pub fn plan(request: ActionRequest) -> PlannedAction {
    if !crate::adb::packages::valid_package_name(&request.package) {
        // We still return a plan but with a "looks suspicious"
        // description so the confirmation UI can flag it.
        let args = synth_args(&request);
        return PlannedAction {
            description: format!(
                "[suspicious package id: {:?}] {}",
                request.package,
                describe(&request)
            ),
            args,
            request,
        };
    }
    let args = synth_args(&request);
    let description = describe(&request);
    PlannedAction {
        request,
        args,
        description,
    }
}

fn synth_args(r: &ActionRequest) -> Vec<String> {
    // Every package/user command carries the explicit `--user <id>` the
    // request targets. `pm enable/disable-user/uninstall/clear` and
    // `am force-stop` all accept `--user` (AOSP Pm.java / Am.java), so a
    // debloat sweep on the work profile (user 10) never silently mutates
    // the owner (user 0).
    let user = r.user_id.to_string();
    match r.kind {
        ActionKind::Disable => vec![
            "pm".into(),
            "disable-user".into(),
            "--user".into(),
            user,
            r.package.clone(),
        ],
        ActionKind::Enable => vec![
            "pm".into(),
            "enable".into(),
            "--user".into(),
            user,
            r.package.clone(),
        ],
        ActionKind::UninstallForUser => vec![
            "pm".into(),
            "uninstall".into(),
            "--user".into(),
            user,
            r.package.clone(),
        ],
        ActionKind::ClearData => vec![
            "pm".into(),
            "clear".into(),
            "--user".into(),
            user,
            r.package.clone(),
        ],
        ActionKind::ForceStop => vec![
            "am".into(),
            "force-stop".into(),
            "--user".into(),
            user,
            r.package.clone(),
        ],
    }
}

fn describe(r: &ActionRequest) -> String {
    let u = r.user_id;
    match r.kind {
        ActionKind::Disable => format!("Disable {} for user {u}", r.package),
        ActionKind::Enable => format!("Re-enable {} for user {u}", r.package),
        ActionKind::UninstallForUser => format!(
            "Uninstall {} for user {u} (system APK preserved on /system)",
            r.package
        ),
        ActionKind::ClearData => format!("Clear data and cache for {} (user {u})", r.package),
        ActionKind::ForceStop => format!("Force-stop {} (user {u})", r.package),
    }
}

pub fn apply(
    transport: &dyn AdbTransport,
    plan: PlannedAction,
    now_iso: &str,
) -> Result<AppliedAction, TransportError> {
    validate_plan(&plan)?;
    validate_device_target(transport, &plan.request.target)?;
    let users = crate::adb::users::list_users(transport, &plan.request.target)?;
    if !users.iter().any(|user| user.id == plan.request.user_id) {
        return Err(TransportError::Parse(format!(
            "Android user {} is no longer available on the selected device",
            plan.request.user_id
        )));
    }
    let argv: Vec<&str> = plan.args.iter().map(String::as_str).collect();
    let stdout = transport.shell_target(&plan.request.target, &argv)?;
    // `pm disable-user --user 0 com.foo` prints "Package com.foo new
    // state: disabled" on success and "Failure [...]" on failure. We
    // surface the raw text and let UI / journal layers decide.
    if let Some(err) = pm_failure_marker(&stdout) {
        return Err(TransportError::Exit {
            code: 1,
            stderr: err.to_string(),
        });
    }
    Ok(AppliedAction {
        plan,
        stdout,
        applied_at: now_iso.to_string(),
    })
}

/// Validate an IPC/CLI-supplied plan before execution. Plans cross a
/// trust boundary: the renderer can send arbitrary JSON, so execution
/// must prove the args match the canonical plan for the request instead
/// of trusting `plan.args`.
pub fn validate_plan(plan: &PlannedAction) -> Result<(), TransportError> {
    if !valid_serial(&plan.request.serial) {
        return Err(TransportError::Parse(format!(
            "invalid device serial {:?}",
            plan.request.serial
        )));
    }
    if plan.request.target.serial != plan.request.serial
        || plan.request.target.connection_generation == 0
    {
        return Err(TransportError::Parse(
            "action plan is missing the validated device target".to_string(),
        ));
    }
    if !crate::adb::packages::valid_package_name(&plan.request.package) {
        return Err(TransportError::Parse(format!(
            "invalid package id {:?}",
            plan.request.package
        )));
    }

    let expected = synth_args(&plan.request);
    if plan.args != expected {
        return Err(TransportError::Parse(
            "planned adb args do not match the requested action".to_string(),
        ));
    }
    Ok(())
}

/// `pm` exits 0 even when the package action fails — the failure shows
/// up in stdout. This recognises the common shapes:
///
///   `Failure [DELETE_FAILED_INTERNAL_ERROR]`
///   `Error: ...`
fn pm_failure_marker(stdout: &str) -> Option<&str> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Failure") || trimmed.starts_with("Error:") {
            return Some(trimmed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::{device::DeviceState, transport::MockTransport, Device};

    fn target(serial: &str) -> DeviceTarget {
        DeviceTarget {
            serial: serial.into(),
            transport_id: Some(1),
            connection_generation: 2,
            model: None,
            product: None,
            device: None,
            build_fingerprint: Some("build/test".into()),
        }
    }

    fn device(serial: &str) -> Device {
        Device {
            serial: serial.into(),
            state: DeviceState::Device,
            model: None,
            product: None,
            device: None,
            build_fingerprint: Some("build/test".into()),
            transport_id: Some(1),
            connection_generation: 0,
            wireless: false,
        }
    }

    fn expect_owner(mock: &MockTransport) {
        mock.expect_shell(
            "abc",
            &["pm", "list", "users"],
            Ok("Users:\n  UserInfo{0:Owner:c13} running (current)\n".into()),
        );
        mock.expect_shell("abc", &["am", "get-current-user"], Ok("0\n".into()));
    }

    #[test]
    fn plan_disable_emits_pm_disable_user() {
        let r = ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.facebook.appmanager".into(),
            kind: ActionKind::Disable,
            user_id: 0,
        };
        let p = plan(r);
        assert_eq!(
            p.args,
            vec![
                "pm",
                "disable-user",
                "--user",
                "0",
                "com.facebook.appmanager"
            ]
        );
        assert!(p.description.contains("Disable"));
    }

    #[test]
    fn plan_enable_emits_pm_enable() {
        let p = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.x".into(),
            kind: ActionKind::Enable,
            user_id: 0,
        });
        assert_eq!(p.args, vec!["pm", "enable", "--user", "0", "com.x"]);
    }

    #[test]
    fn plan_uninstall_user_emits_pm_uninstall() {
        let p = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.x".into(),
            kind: ActionKind::UninstallForUser,
            user_id: 0,
        });
        assert_eq!(p.args, vec!["pm", "uninstall", "--user", "0", "com.x"]);
    }

    #[test]
    fn plan_targets_explicit_secondary_user() {
        // A debloat action aimed at the work profile (user 10) must carry
        // --user 10 through every command shape, proving user 0 and user
        // 10 stay independent.
        for (kind, head) in [
            (ActionKind::Disable, vec!["pm", "disable-user"]),
            (ActionKind::Enable, vec!["pm", "enable"]),
            (ActionKind::UninstallForUser, vec!["pm", "uninstall"]),
            (ActionKind::ClearData, vec!["pm", "clear"]),
            (ActionKind::ForceStop, vec!["am", "force-stop"]),
        ] {
            let p = plan(ActionRequest {
                serial: "abc".into(),
                target: target("abc"),
                package: "com.x".into(),
                kind,
                user_id: 10,
            });
            let mut expected: Vec<String> = head.into_iter().map(String::from).collect();
            expected.push("--user".into());
            expected.push("10".into());
            expected.push("com.x".into());
            assert_eq!(p.args, expected, "kind {kind:?} must target user 10");
            assert!(p.description.contains("user 10"));
        }
    }

    #[test]
    fn plan_flags_suspicious_package_id_but_still_emits_args() {
        let p = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: ".dotleading".into(),
            kind: ActionKind::Disable,
            user_id: 0,
        });
        assert!(p.description.starts_with("[suspicious package id"));
        // Args still synthesised — the caller decides whether to
        // refuse.
        assert!(!p.args.is_empty());
    }

    #[test]
    fn inverse_and_reversibility() {
        assert_eq!(ActionKind::Disable.inverse(), Some(ActionKind::Enable));
        assert_eq!(ActionKind::Enable.inverse(), Some(ActionKind::Disable));
        assert!(ActionKind::Disable.is_reversible());
        assert!(ActionKind::Enable.is_reversible());
        assert!(!ActionKind::UninstallForUser.is_reversible());
        assert!(ActionKind::UninstallForUser.inverse().is_none());
    }

    #[test]
    fn apply_records_stdout_and_timestamp() {
        let mock = MockTransport::new().with_devices(vec![device("abc")]);
        mock.expect_shell(
            "abc",
            &["pm", "disable-user", "--user", "0", "com.x"],
            Ok("Package com.x new state: disabled\n".into()),
        );
        expect_owner(&mock);
        let p = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.x".into(),
            kind: ActionKind::Disable,
            user_id: 0,
        });
        let applied = apply(&mock, p, "2026-05-25T12:00:00Z").unwrap();
        assert_eq!(applied.applied_at, "2026-05-25T12:00:00Z");
        assert!(applied.stdout.contains("new state: disabled"));
    }

    #[test]
    fn apply_surfaces_pm_failure_marker() {
        let mock = MockTransport::new().with_devices(vec![device("abc")]);
        mock.expect_shell(
            "abc",
            &["pm", "uninstall", "--user", "0", "com.x"],
            Ok("Failure [DELETE_FAILED_INTERNAL_ERROR]\n".into()),
        );
        expect_owner(&mock);
        let p = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.x".into(),
            kind: ActionKind::UninstallForUser,
            user_id: 0,
        });
        let err = apply(&mock, p, "2026-05-25T12:00:00Z").unwrap_err();
        match err {
            TransportError::Exit { code, stderr } => {
                assert_eq!(code, 1);
                assert!(stderr.contains("DELETE_FAILED_INTERNAL_ERROR"));
            }
            other => panic!("expected Exit, got {other:?}"),
        }
    }

    #[test]
    fn apply_refuses_tampered_plan_args() {
        let mock = MockTransport::new();
        let mut p = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.x".into(),
            kind: ActionKind::Disable,
            user_id: 0,
        });
        p.args = vec!["pm".into(), "clear".into(), "com.x".into()];
        let err = apply(&mock, p, "2026-05-25T12:00:00Z").unwrap_err();
        assert!(matches!(err, TransportError::Parse(_)));
    }

    #[test]
    fn apply_refuses_invalid_package_even_if_plan_exists() {
        let mock = MockTransport::new();
        let p = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: ".dotleading".into(),
            kind: ActionKind::Disable,
            user_id: 0,
        });
        let err = apply(&mock, p, "2026-05-25T12:00:00Z").unwrap_err();
        assert!(matches!(err, TransportError::Parse(_)));
    }

    #[test]
    fn apply_refuses_invalid_serial() {
        let mock = MockTransport::new();
        let p = plan(ActionRequest {
            serial: "../journal".into(),
            target: target("../journal"),
            package: "com.x".into(),
            kind: ActionKind::Disable,
            user_id: 0,
        });
        let err = apply(&mock, p, "2026-05-25T12:00:00Z").unwrap_err();
        assert!(matches!(err, TransportError::Parse(_)));
    }

    #[test]
    fn apply_refuses_a_user_that_disappeared_after_planning() {
        let mock = MockTransport::new().with_devices(vec![device("abc")]);
        mock.expect_shell(
            "abc",
            &["pm", "list", "users"],
            Ok("Users:\n  UserInfo{10:Work:1030} running (current)\n".into()),
        );
        mock.expect_shell("abc", &["am", "get-current-user"], Ok("10\n".into()));
        let p = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.x".into(),
            kind: ActionKind::Disable,
            user_id: 0,
        });
        let err = apply(&mock, p, "2026-05-25T12:00:00Z").unwrap_err();
        assert!(err.to_string().contains("user 0 is no longer available"));
    }

    #[test]
    fn pm_failure_marker_recognises_error_prefix() {
        assert!(pm_failure_marker("Error: package not found").is_some());
        assert!(pm_failure_marker("nothing wrong here").is_none());
    }
}
