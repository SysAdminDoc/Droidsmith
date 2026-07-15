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

use std::sync::atomic::{AtomicU64, Ordering};

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
    /// `pm grant --user <id> <pkg> <permission>`.
    GrantPermission,
    /// `pm revoke --user <id> <pkg> <permission>`.
    RevokePermission,
    /// A reviewed arbitrary shell mutation. Read-only shell commands use the
    /// separate `shell_run` path and never enter the mutation journal.
    Shell,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationSource {
    #[default]
    Unspecified,
    Internal,
    AppsPreview,
    DebloatPreview,
    PermissionToggle,
    ConsoleReview,
    DeviceControl,
    CliApply,
    JournalUndo,
    RecoveryBaseline,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionContext {
    #[serde(default)]
    pub confirmation_source: ConfirmationSource,
    #[serde(default)]
    pub permission: Option<String>,
    #[serde(default)]
    pub shell_argv: Vec<String>,
    /// Set by the trusted backend only after it revalidates and accepts an
    /// explicit renderer acknowledgement for an unauthenticated transport.
    #[serde(default)]
    pub transport_override: Option<super::DeviceTransportKind>,
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
    let mut command = Command::new(adb_path);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process_tree::configure(&mut command);
    let mut child = command.spawn().map_err(TransportError::Spawn)?;

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
                    let _ = crate::process_tree::terminate(&mut child);
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => {
                let _ = crate::process_tree::terminate(&mut child);
                break None;
            }
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
    let mut command = Command::new(adb_path);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process_tree::configure(&mut command);
    let mut child = command.spawn().map_err(TransportError::Spawn)?;

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
                    let _ = crate::process_tree::terminate(&mut child);
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => {
                let _ = crate::process_tree::terminate(&mut child);
                break None;
            }
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
        matches!(
            self,
            Self::Disable | Self::Enable | Self::GrantPermission | Self::RevokePermission
        )
    }

    pub fn inverse(self) -> Option<ActionKind> {
        match self {
            Self::Disable => Some(Self::Enable),
            Self::Enable => Some(Self::Disable),
            Self::GrantPermission => Some(Self::RevokePermission),
            Self::RevokePermission => Some(Self::GrantPermission),
            Self::UninstallForUser | Self::ClearData | Self::ForceStop | Self::Shell => None,
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
    /// Structured provenance for actions planned from a debloat pack. This is
    /// persisted in the journal, including any explicit compatibility override.
    #[serde(default)]
    pub pack_context: Option<PackActionContext>,
    /// Operation-specific canonical data and the UI/CLI confirmation source.
    #[serde(default)]
    pub context: ActionContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackActionContext {
    pub pack_id: String,
    pub revision: u32,
    pub provenance_source: String,
    pub provenance_license: String,
    pub compatibility_status: String,
    pub override_accepted: bool,
}

/// Synthesised plan. The `args` field is exactly what the action will
/// pass to `adb shell` — no further interpolation happens at apply
/// time. `description` is human-readable for the confirmation dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedAction {
    pub request: ActionRequest,
    pub args: Vec<String>,
    pub description: String,
    /// Stable identifier carried by the write-ahead intent and terminal row.
    #[serde(default)]
    pub incident_id: String,
    /// Snapshot captured immediately before the durable write-ahead intent.
    #[serde(default)]
    pub before_state: String,
}

/// Applied action — the journal record. `stdout`/`stderr` are kept so
/// support tickets can include the raw response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedAction {
    pub plan: PlannedAction,
    pub stdout: String,
    /// Raw output is returned to the initiating view but never serialized or
    /// persisted. `stdout` above is the bounded/redacted audit copy.
    #[serde(skip)]
    pub display_stdout: String,
    #[serde(default)]
    pub before_state: String,
    #[serde(default)]
    pub after_state: String,
    /// ISO-8601 UTC timestamp.
    pub applied_at: String,
}

pub fn plan(mut request: ActionRequest) -> PlannedAction {
    if request.context.confirmation_source == ConfirmationSource::Unspecified {
        request.context.confirmation_source = ConfirmationSource::Internal;
    }
    let incident_id = next_incident_id();
    if request.kind != ActionKind::Shell
        && !crate::adb::packages::valid_package_name(&request.package)
    {
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
            incident_id,
            before_state: String::new(),
        };
    }
    let args = synth_args(&request);
    let description = describe(&request);
    PlannedAction {
        request,
        args,
        description,
        incident_id,
        before_state: String::new(),
    }
}

fn next_incident_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(
        "op-{nanos:x}-{:x}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
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
        ActionKind::GrantPermission | ActionKind::RevokePermission => vec![
            "pm".into(),
            if r.kind == ActionKind::GrantPermission {
                "grant".into()
            } else {
                "revoke".into()
            },
            "--user".into(),
            user,
            r.package.clone(),
            r.context.permission.clone().unwrap_or_default(),
        ],
        ActionKind::Shell => r.context.shell_argv.clone(),
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
        ActionKind::GrantPermission => format!(
            "Grant {} to {} for user {u}",
            r.context.permission.as_deref().unwrap_or("<missing>"),
            r.package
        ),
        ActionKind::RevokePermission => format!(
            "Revoke {} from {} for user {u}",
            r.context.permission.as_deref().unwrap_or("<missing>"),
            r.package
        ),
        ActionKind::Shell => format!(
            "Run reviewed shell mutation: {}",
            r.context.shell_argv.join(" ")
        ),
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
    let selected_user = users.iter().find(|user| user.id == plan.request.user_id);
    if selected_user.is_none() {
        return Err(TransportError::Parse(format!(
            "Android user {} is no longer available on the selected device",
            plan.request.user_id
        )));
    }
    if plan.request.kind == ActionKind::Shell && !selected_user.is_some_and(|user| user.current) {
        return Err(TransportError::Parse(format!(
            "Android user {} is no longer the current user for this shell mutation",
            plan.request.user_id
        )));
    }
    let before_state = if plan.before_state.is_empty() {
        capture_state(transport, &plan.request)
    } else {
        plan.before_state.clone()
    };
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
    let after_state = capture_state(transport, &plan.request);
    let audit_stdout = redact_journal_text(&plan.request, &stdout);
    Ok(AppliedAction {
        plan,
        stdout: audit_stdout,
        display_stdout: stdout,
        before_state,
        after_state,
        applied_at: now_iso.to_string(),
    })
}

pub fn capture_state(transport: &dyn AdbTransport, request: &ActionRequest) -> String {
    match request.kind {
        ActionKind::Disable
        | ActionKind::Enable
        | ActionKind::UninstallForUser
        | ActionKind::ClearData
        | ActionKind::ForceStop => {
            package_state(transport, request).unwrap_or_else(|_| "unknown".to_string())
        }
        ActionKind::GrantPermission | ActionKind::RevokePermission => transport
            .shell_target(&request.target, &["dumpsys", "package", &request.package])
            .map(|output| permission_state(&output, request.context.permission.as_deref()))
            .unwrap_or_else(|_| "unknown".to_string()),
        ActionKind::Shell
            if request
                .context
                .shell_argv
                .starts_with(&["wm".into(), "density".into()]) =>
        {
            transport
                .shell_target(&request.target, &["wm", "density"])
                .map(|output| output.trim().to_string())
                .unwrap_or_else(|_| "unknown".to_string())
        }
        ActionKind::Shell => "not_captured".to_string(),
    }
}

fn package_state(
    transport: &dyn AdbTransport,
    request: &ActionRequest,
) -> Result<String, TransportError> {
    let user = request.user_id.to_string();
    let disabled = transport.shell_target(
        &request.target,
        &[
            "pm",
            "list",
            "packages",
            "--user",
            &user,
            "-d",
            &request.package,
        ],
    )?;
    if disabled
        .lines()
        .any(|line| line.trim() == format!("package:{}", request.package))
    {
        return Ok("installed_disabled".to_string());
    }
    let installed = transport.shell_target(
        &request.target,
        &["pm", "list", "packages", "--user", &user, &request.package],
    )?;
    Ok(
        if installed
            .lines()
            .any(|line| line.trim() == format!("package:{}", request.package))
        {
            "installed_enabled".to_string()
        } else {
            "not_installed".to_string()
        },
    )
}

fn permission_state(output: &str, permission: Option<&str>) -> String {
    let Some(permission) = permission else {
        return "unknown".to_string();
    };
    output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(permission) && line.contains("granted="))
        .map(|line| {
            if line.contains("granted=true") {
                "granted".to_string()
            } else {
                "revoked".to_string()
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn redact_journal_text(request: &ActionRequest, output: &str) -> String {
    if request.kind == ActionKind::Shell {
        return format!("[shell output redacted; {} byte(s)]", output.len());
    }
    let redacted = output.replace(&request.serial, "[device]");
    redacted.chars().take(8_192).collect()
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
    if plan.incident_id.is_empty() || !plan.incident_id.starts_with("op-") {
        return Err(TransportError::Parse(
            "action plan is missing its incident id".to_string(),
        ));
    }
    if plan.request.context.confirmation_source == ConfirmationSource::Unspecified {
        return Err(TransportError::Parse(
            "action plan is missing its confirmation source".to_string(),
        ));
    }
    if plan.request.kind != ActionKind::Shell
        && !crate::adb::packages::valid_package_name(&plan.request.package)
    {
        return Err(TransportError::Parse(format!(
            "invalid package id {:?}",
            plan.request.package
        )));
    }

    match plan.request.kind {
        ActionKind::GrantPermission | ActionKind::RevokePermission => {
            let permission = plan
                .request
                .context
                .permission
                .as_deref()
                .unwrap_or_default();
            if !valid_permission(permission) || !plan.request.context.shell_argv.is_empty() {
                return Err(TransportError::Parse(
                    "permission action has invalid canonical context".to_string(),
                ));
            }
        }
        ActionKind::Shell => {
            if !plan.request.package.is_empty()
                || plan.request.context.permission.is_some()
                || !valid_shell_argv(&plan.request.context.shell_argv)
            {
                return Err(TransportError::Parse(
                    "shell action has invalid canonical context".to_string(),
                ));
            }
        }
        _ if plan.request.context.permission.is_some()
            || !plan.request.context.shell_argv.is_empty() =>
        {
            return Err(TransportError::Parse(
                "package action has unexpected canonical context".to_string(),
            ));
        }
        _ => {}
    }

    let expected = synth_args(&plan.request);
    if plan.args != expected {
        return Err(TransportError::Parse(
            "planned adb args do not match the requested action".to_string(),
        ));
    }
    Ok(())
}

pub fn valid_permission(permission: &str) -> bool {
    !permission.is_empty()
        && permission.len() <= 256
        && permission
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_'))
}

pub fn valid_shell_argv(argv: &[String]) -> bool {
    !argv.is_empty()
        && argv.len() <= 128
        && argv.iter().map(String::len).sum::<usize>() <= 16_384
        && argv.iter().all(|argument| {
            !argument.is_empty()
                && !argument.contains('\0')
                && !argument.contains('\n')
                && !argument.contains('\r')
        })
}

/// `pm` exits 0 even when the package action fails — the failure shows
/// up in stdout. This recognises the common shapes:
///
///   `Failure [DELETE_FAILED_INTERNAL_ERROR]`
///   `Error: ...`
pub(crate) fn pm_failure_marker(stdout: &str) -> Option<&str> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Failure") || trimmed.starts_with("Error:") {
            return Some(trimmed);
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageActionFailure {
    pub code: &'static str,
    pub cause: &'static str,
    pub remedy: &'static str,
}

/// Turn common `pm`/OEM policy failures into a specific next action while
/// retaining the original ADB text. Unknown errors deliberately return None so
/// callers do not invent a remedy.
pub fn classify_package_action_failure(raw: &str) -> Option<PackageActionFailure> {
    let normalized = raw.to_ascii_uppercase();
    if normalized.contains("DELETE_FAILED_DEVICE_POLICY_MANAGER")
        || normalized.contains("DEVICE POLICY MANAGER")
    {
        Some(PackageActionFailure {
            code: "DEVICE_POLICY_BLOCK",
            cause: "A device-owner, profile-owner, or work policy protects this package.",
            remedy: "Remove the governing policy or use its management console; Droidsmith will not bypass device policy.",
        })
    } else if normalized.contains("SECURITYEXCEPTION")
        || normalized.contains("NOT ALLOWED TO DISABLE")
        || normalized.contains("CANNOT DISABLE A PROTECTED PACKAGE")
        || normalized.contains("PERMISSION DENIAL")
    {
        Some(PackageActionFailure {
            code: "ANDROID_SECURITY_BLOCK",
            cause: "Android or the OEM security layer denied this package action.",
            remedy: "Check USB debugging security settings, the active Android user, and OEM restrictions; do not retry blindly.",
        })
    } else if normalized.contains("UNKNOWN PACKAGE") || normalized.contains("PACKAGE_NOT_FOUND") {
        Some(PackageActionFailure {
            code: "PACKAGE_NOT_FOUND",
            cause: "The package is no longer present for the requested operation.",
            remedy:
                "Refresh the package list and confirm the selected Android user before retrying.",
        })
    } else if normalized.contains("NOT INSTALLED FOR") {
        Some(PackageActionFailure {
            code: "PACKAGE_NOT_INSTALLED_FOR_USER",
            cause: "The package is not installed for the selected Android user.",
            remedy: "Switch to a user where the package is installed or refresh the current user's package list.",
        })
    } else {
        None
    }
}

pub fn package_action_failure_message(raw: &str) -> String {
    let Some(advice) = classify_package_action_failure(raw) else {
        return raw.to_string();
    };
    format!(
        "{}: {} Remedy: {} Raw ADB: {}",
        advice.code, advice.cause, advice.remedy, raw
    )
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
            transport_kind: crate::adb::DeviceTransportKind::Usb,
            untrusted_transport_override: false,
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
            transport_kind: crate::adb::DeviceTransportKind::Usb,
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
            pack_context: None,
            context: ActionContext::default(),
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
            pack_context: None,
            context: ActionContext::default(),
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
            pack_context: None,
            context: ActionContext::default(),
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
                pack_context: None,
                context: ActionContext::default(),
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
            pack_context: None,
            context: ActionContext::default(),
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
        assert_eq!(
            ActionKind::GrantPermission.inverse(),
            Some(ActionKind::RevokePermission)
        );
        assert!(ActionKind::GrantPermission.is_reversible());
    }

    #[test]
    fn permission_and_shell_plans_are_canonical() {
        let permission = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.x".into(),
            kind: ActionKind::GrantPermission,
            user_id: 10,
            pack_context: None,
            context: ActionContext {
                confirmation_source: ConfirmationSource::PermissionToggle,
                permission: Some("android.permission.CAMERA".into()),
                shell_argv: Vec::new(),
                transport_override: None,
            },
        });
        assert_eq!(
            permission.args,
            vec![
                "pm",
                "grant",
                "--user",
                "10",
                "com.x",
                "android.permission.CAMERA"
            ]
        );
        assert!(validate_plan(&permission).is_ok());

        let shell = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: String::new(),
            kind: ActionKind::Shell,
            user_id: 0,
            pack_context: None,
            context: ActionContext {
                confirmation_source: ConfirmationSource::ConsoleReview,
                permission: None,
                shell_argv: vec![
                    "settings".into(),
                    "put".into(),
                    "global".into(),
                    "x".into(),
                    "1".into(),
                ],
                transport_override: None,
            },
        });
        assert_eq!(shell.args, shell.request.context.shell_argv);
        assert!(validate_plan(&shell).is_ok());
        assert!(redact_journal_text(&shell.request, "secret device output").contains("redacted"));
    }

    #[test]
    fn permission_apply_captures_before_and_after_state() {
        let mock = MockTransport::new().with_devices(vec![device("abc")]);
        expect_owner(&mock);
        mock.expect_shell(
            "abc",
            &["dumpsys", "package", "com.x"],
            Ok("android.permission.CAMERA: granted=false\n".into()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "grant",
                "--user",
                "0",
                "com.x",
                "android.permission.CAMERA",
            ],
            Ok(String::new()),
        );
        mock.expect_shell(
            "abc",
            &["dumpsys", "package", "com.x"],
            Ok("android.permission.CAMERA: granted=true\n".into()),
        );
        let applied = apply(
            &mock,
            plan(ActionRequest {
                serial: "abc".into(),
                target: target("abc"),
                package: "com.x".into(),
                kind: ActionKind::GrantPermission,
                user_id: 0,
                pack_context: None,
                context: ActionContext {
                    confirmation_source: ConfirmationSource::PermissionToggle,
                    permission: Some("android.permission.CAMERA".into()),
                    shell_argv: Vec::new(),
                    transport_override: None,
                },
            }),
            "2026-07-14T12:00:00Z",
        )
        .unwrap();
        assert_eq!(applied.before_state, "revoked");
        assert_eq!(applied.after_state, "granted");
    }

    #[test]
    fn shell_apply_returns_raw_output_but_redacts_audit_copy() {
        let mock = MockTransport::new().with_devices(vec![device("abc")]);
        expect_owner(&mock);
        mock.expect_shell(
            "abc",
            &["settings", "put", "global", "qa", "secret"],
            Ok("secret output from abc".into()),
        );
        let applied = apply(
            &mock,
            plan(ActionRequest {
                serial: "abc".into(),
                target: target("abc"),
                package: String::new(),
                kind: ActionKind::Shell,
                user_id: 0,
                pack_context: None,
                context: ActionContext {
                    confirmation_source: ConfirmationSource::ConsoleReview,
                    permission: None,
                    shell_argv: vec![
                        "settings".into(),
                        "put".into(),
                        "global".into(),
                        "qa".into(),
                        "secret".into(),
                    ],
                    transport_override: None,
                },
            }),
            "2026-07-14T12:00:00Z",
        )
        .unwrap();
        assert_eq!(applied.display_stdout, "secret output from abc");
        assert!(applied.stdout.contains("redacted"));
        assert!(!applied.stdout.contains("secret output"));
        assert_eq!(applied.before_state, "not_captured");
        assert_eq!(applied.after_state, "not_captured");
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
            pack_context: None,
            context: ActionContext::default(),
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
            pack_context: None,
            context: ActionContext::default(),
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
            pack_context: None,
            context: ActionContext::default(),
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
            pack_context: None,
            context: ActionContext::default(),
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
            pack_context: None,
            context: ActionContext::default(),
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
            pack_context: None,
            context: ActionContext::default(),
        });
        let err = apply(&mock, p, "2026-05-25T12:00:00Z").unwrap_err();
        assert!(err.to_string().contains("user 0 is no longer available"));
    }

    #[test]
    fn pm_failure_marker_recognises_error_prefix() {
        assert!(pm_failure_marker("Error: package not found").is_some());
        assert!(pm_failure_marker("nothing wrong here").is_none());
    }

    #[test]
    fn package_action_failures_map_to_human_remediation() {
        let policy =
            classify_package_action_failure("Failure [DELETE_FAILED_DEVICE_POLICY_MANAGER]")
                .unwrap();
        assert_eq!(policy.code, "DEVICE_POLICY_BLOCK");
        let security = classify_package_action_failure(
            "java.lang.SecurityException: not allowed to disable this package",
        )
        .unwrap();
        assert_eq!(security.code, "ANDROID_SECURITY_BLOCK");
        let message = package_action_failure_message("Error: Unknown package: com.example");
        assert!(message.contains("PACKAGE_NOT_FOUND"));
        assert!(message.contains("Raw ADB:"));
        assert!(classify_package_action_failure("vendor status 9").is_none());
    }
}
