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

use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::adb::device::{valid_serial, DeviceTarget};
use crate::adb::transport::{validate_device_target, AdbTransport, TransportError};

#[derive(
    schemars::JsonSchema, specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// `pm disable-user --user 0 <pkg>` — reversible with `pm enable`.
    Disable,
    /// `pm enable <pkg>` — reverses Disable.
    Enable,
    /// Android 15+ `pm archive` keeps user data while removing APK/cache.
    Archive,
    /// Android 15+ `pm request-unarchive` asks the responsible installer to
    /// restore an archived package.
    RequestUnarchive,
    /// `pm uninstall --user 0 <pkg>` — effectively permanent for the
    /// current user; the APK remains in `/system/` (system apps) but
    /// is removed from `/data/app/` (user apps).
    UninstallForUser,
    /// Backend-only inverse for a verified retained system package.
    /// `cmd package install-existing` is followed by an explicit restoration
    /// and verification of the prior enabled state.
    RestoreExistingForUser,
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

#[derive(specta::Type, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    ProfilePreview,
    FileManagerReview,
    JournalUndo,
    RecoveryBaseline,
}

#[derive(specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Backend-only state carried from a verified uninstall journal row into
    /// its `install-existing` inverse.
    #[serde(default)]
    pub restore_enabled_state: Option<bool>,
    /// Backend-issued identifier shared by every item in one reviewed batch.
    /// The renderer may display this value but cannot assign it through the
    /// single-action planner/apply boundary.
    #[serde(default)]
    pub batch_id: Option<String>,
}

/// Install an APK on a device using `adb install`. The APK path is a
/// local filesystem path that gets pushed to the device via adb's
/// built-in staging.
pub fn install_apk(
    adb_path: &std::path::Path,
    target: &DeviceTarget,
    apk_path: &str,
) -> Result<String, TransportError> {
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
    command.args(&args);
    let (stdout, stderr, status) = capture_action_command(&mut command, timeout)?;
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

fn capture_action_command(
    command: &mut Command,
    timeout: Duration,
) -> Result<(String, String, ExitStatus), TransportError> {
    let output = crate::process_capture::run(
        command,
        timeout,
        crate::process_capture::CaptureLimits::default(),
    )
    .map_err(crate::adb::transport::capture_error)?;
    match output.termination {
        crate::process_capture::CaptureTermination::Exited(status) => Ok((
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
            status,
        )),
        crate::process_capture::CaptureTermination::TimedOut => {
            Err(TransportError::Timeout(timeout))
        }
        crate::process_capture::CaptureTermination::OutputLimitExceeded {
            stream,
            limit_bytes,
        } => Err(TransportError::OutputLimit {
            stream: crate::adb::transport::output_stream(stream),
            limit_bytes,
        }),
    }
}

/// Pull an APK from a device using `adb pull`.
pub fn extract_apk(
    adb_path: &std::path::Path,
    target: &DeviceTarget,
    remote_path: &str,
    local_path: &str,
) -> Result<String, TransportError> {
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
    command.args(&args);
    let (stdout, stderr, status) = capture_action_command(&mut command, timeout)?;
    if status.success() {
        Ok(stdout)
    } else if let Some(code) = status.code() {
        Err(TransportError::Exit { code, stderr })
    } else {
        Err(TransportError::Signaled { stderr })
    }
}

impl ActionKind {
    /// True for actions that are unconditionally reversible. An
    /// `UninstallForUser` becomes reversible only after its journal row proves
    /// a retained preinstalled package, so it remains false here.
    ///
    /// Kept alongside `inverse` for symmetry; the renderer will use
    /// this to disable the "Undo" button on irreversible rows.
    #[allow(dead_code)]
    pub fn is_reversible(self) -> bool {
        matches!(
            self,
            Self::Disable
                | Self::Enable
                | Self::Archive
                | Self::RequestUnarchive
                | Self::GrantPermission
                | Self::RevokePermission
        )
    }

    pub fn inverse(self) -> Option<ActionKind> {
        match self {
            Self::Disable => Some(Self::Enable),
            Self::Enable => Some(Self::Disable),
            Self::Archive => Some(Self::RequestUnarchive),
            Self::RequestUnarchive => Some(Self::Archive),
            Self::GrantPermission => Some(Self::RevokePermission),
            Self::RevokePermission => Some(Self::GrantPermission),
            Self::UninstallForUser => Some(Self::RestoreExistingForUser),
            Self::RestoreExistingForUser | Self::ClearData | Self::ForceStop | Self::Shell => None,
        }
    }
}

#[derive(specta::Type, Debug, Clone, Serialize, Deserialize)]
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

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(specta::Type, Debug, Clone, Serialize, Deserialize)]
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
#[derive(specta::Type, Debug, Clone, Serialize, Deserialize)]
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
        ActionKind::Archive => vec![
            "pm".into(),
            "archive".into(),
            "--user".into(),
            user,
            r.package.clone(),
        ],
        ActionKind::RequestUnarchive => vec![
            "pm".into(),
            "request-unarchive".into(),
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
        ActionKind::RestoreExistingForUser => vec![
            "cmd".into(),
            "package".into(),
            "install-existing".into(),
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
        ActionKind::Archive => format!(
            "Archive {} for user {u}; Android removes APK/cache but keeps user data",
            r.package
        ),
        ActionKind::RequestUnarchive => format!(
            "Request restoration of archived {} for user {u} from its responsible installer",
            r.package
        ),
        ActionKind::UninstallForUser => format!(
            "Remove {} for user {u}; undo is offered only if PackageManager verifies a retained preinstalled APK",
            r.package
        ),
        ActionKind::RestoreExistingForUser => format!(
            "Restore retained preinstalled package {} for user {u} and restore its prior enabled state",
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
    if matches!(
        plan.request.kind,
        ActionKind::Archive | ActionKind::RequestUnarchive
    ) {
        let capability = crate::adb::packages::archive_capability(transport, &plan.request.target);
        if !capability.supported {
            return Err(TransportError::Parse(capability.reason));
        }
        let valid_before = match plan.request.kind {
            ActionKind::Archive => matches!(
                before_state.as_str(),
                "user_installed_enabled" | "user_installed_disabled"
            ),
            ActionKind::RequestUnarchive => before_state == "archived",
            _ => unreachable!(),
        };
        if !valid_before {
            return Err(TransportError::Parse(format!(
                "refusing {:?} because the verified package state is {before_state}",
                plan.request.kind
            )));
        }
    }
    let stdout = if plan.request.kind == ActionKind::RestoreExistingForUser {
        restore_existing_for_user(transport, &plan.request, &plan.args, &before_state)?
    } else {
        let argv: Vec<&str> = plan.args.iter().map(String::as_str).collect();
        checked_package_command(transport, &plan.request.target, &argv)?
    };
    // `pm disable-user --user 0 com.foo` prints "Package com.foo new
    // state: disabled" on success and "Failure [...]" on failure. We
    // surface the raw text and let UI / journal layers decide.
    let after_state = if matches!(
        plan.request.kind,
        ActionKind::Archive | ActionKind::RequestUnarchive
    ) {
        wait_for_archive_transition(transport, &plan.request)?
    } else {
        capture_state(transport, &plan.request)
    };
    if plan.request.context.batch_id.is_some()
        && !verified_batch_transition(plan.request.kind, &before_state, &after_state)
    {
        return Err(TransportError::Parse(format!(
            "batch action {:?} did not complete a reversible state transition: {before_state} -> {after_state}",
            plan.request.kind
        )));
    }
    if plan.request.kind == ActionKind::RestoreExistingForUser {
        let expected = if plan.request.context.restore_enabled_state == Some(true) {
            "preinstalled_enabled"
        } else {
            "preinstalled_disabled"
        };
        if after_state != expected {
            return Err(TransportError::Parse(format!(
                "install-existing did not restore the verified package state: expected {expected}, got {after_state}"
            )));
        }
    }
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

pub fn reversible_batch_before_state(kind: ActionKind, state: &str) -> bool {
    match kind {
        ActionKind::Disable => state.ends_with("_enabled"),
        ActionKind::Enable => state.ends_with("_disabled"),
        ActionKind::Archive => {
            matches!(state, "user_installed_enabled" | "user_installed_disabled")
        }
        ActionKind::RequestUnarchive => state == "archived",
        _ => false,
    }
}

fn verified_batch_transition(kind: ActionKind, before: &str, after: &str) -> bool {
    if !reversible_batch_before_state(kind, before) {
        return false;
    }
    match kind {
        ActionKind::Disable => after.ends_with("_disabled"),
        ActionKind::Enable => after.ends_with("_enabled"),
        ActionKind::Archive => after == "archived",
        ActionKind::RequestUnarchive => {
            matches!(after, "user_installed_enabled" | "user_installed_disabled")
        }
        _ => false,
    }
}

pub fn capture_state(transport: &dyn AdbTransport, request: &ActionRequest) -> String {
    match request.kind {
        ActionKind::Disable
        | ActionKind::Enable
        | ActionKind::ClearData
        | ActionKind::ForceStop => {
            package_state(transport, request).unwrap_or_else(|_| "unknown".to_string())
        }
        ActionKind::UninstallForUser | ActionKind::RestoreExistingForUser => {
            removal_state(transport, request).unwrap_or_else(|_| "unknown".to_string())
        }
        ActionKind::Archive | ActionKind::RequestUnarchive => {
            removal_state(transport, request).unwrap_or_else(|_| "unknown".to_string())
        }
        ActionKind::GrantPermission | ActionKind::RevokePermission => transport
            .shell_target(&request.target, &["dumpsys", "package", &request.package])
            .map(|output| permission_state(&output, request.context.permission.as_deref()))
            .unwrap_or_else(|_| "unknown".to_string()),
        ActionKind::Shell
            if request.context.confirmation_source == ConfirmationSource::FileManagerReview =>
        {
            crate::remote_files::capture_state(
                transport,
                &request.target,
                &request.context.shell_argv,
            )
        }
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

fn removal_state(
    transport: &dyn AdbTransport,
    request: &ActionRequest,
) -> Result<String, TransportError> {
    use crate::adb::packages::PackagePresence;

    Ok(match crate::adb::packages::inspect_package_presence(
        transport,
        &request.target,
        request.user_id,
        &request.package,
    )? {
        PackagePresence::Installed {
            enabled: true,
            system: true,
        } => "preinstalled_enabled",
        PackagePresence::Installed {
            enabled: false,
            system: true,
        } => "preinstalled_disabled",
        PackagePresence::Installed {
            enabled: true,
            system: false,
        } => "user_installed_enabled",
        PackagePresence::Installed {
            enabled: false,
            system: false,
        } => "user_installed_disabled",
        PackagePresence::Archived => "archived",
        PackagePresence::Retained { system: true } => "retained_preinstalled",
        PackagePresence::Retained { system: false } => "retained_unclassified",
        PackagePresence::Missing => "not_installed",
    }
    .to_string())
}

fn wait_for_archive_transition(
    transport: &dyn AdbTransport,
    request: &ActionRequest,
) -> Result<String, TransportError> {
    for _ in 0..40 {
        let state = removal_state(transport, request)?;
        let complete = match request.kind {
            ActionKind::Archive => state == "archived",
            ActionKind::RequestUnarchive => matches!(
                state.as_str(),
                "user_installed_enabled" | "user_installed_disabled"
            ),
            _ => unreachable!(),
        };
        if complete {
            return Ok(state);
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Err(TransportError::Parse(
        "Android accepted the archive request but the expected package state was not observed within 10 seconds"
            .to_string(),
    ))
}

fn checked_package_command(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
    argv: &[&str],
) -> Result<String, TransportError> {
    let output = transport.shell_target(target, argv)?;
    if let Some(error) = pm_failure_marker(&output) {
        return Err(TransportError::Exit {
            code: 1,
            stderr: error.to_string(),
        });
    }
    Ok(output)
}

fn restore_existing_for_user(
    transport: &dyn AdbTransport,
    request: &ActionRequest,
    install_args: &[String],
    before_state: &str,
) -> Result<String, TransportError> {
    let mut output = String::new();
    match before_state {
        "retained_preinstalled" => {
            let argv: Vec<&str> = install_args.iter().map(String::as_str).collect();
            output.push_str(&checked_package_command(transport, &request.target, &argv)?);
        }
        "preinstalled_enabled" | "preinstalled_disabled" => {
            output.push_str("Package was already restored; reconciling enabled state.\n");
        }
        state => {
            return Err(TransportError::Parse(format!(
                "refusing install-existing because the package is not a retained preinstalled package ({state})"
            )));
        }
    }

    let user = request.user_id.to_string();
    let enabled = request.context.restore_enabled_state.ok_or_else(|| {
        TransportError::Parse("install-existing is missing the verified prior enabled state".into())
    })?;
    let state_args = [
        "pm",
        if enabled { "enable" } else { "disable-user" },
        "--user",
        &user,
        &request.package,
    ];
    output.push_str(&checked_package_command(
        transport,
        &request.target,
        &state_args,
    )?);
    Ok(output)
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
    if let Some(batch_id) = plan.request.context.batch_id.as_deref() {
        if !valid_batch_id(batch_id) {
            return Err(TransportError::Parse(
                "action plan has an invalid batch id".to_string(),
            ));
        }
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
            if !valid_permission(permission)
                || !plan.request.context.shell_argv.is_empty()
                || plan.request.context.restore_enabled_state.is_some()
            {
                return Err(TransportError::Parse(
                    "permission action has invalid canonical context".to_string(),
                ));
            }
        }
        ActionKind::Shell => {
            if !plan.request.package.is_empty()
                || plan.request.context.permission.is_some()
                || !valid_shell_argv(&plan.request.context.shell_argv)
                || plan.request.context.restore_enabled_state.is_some()
            {
                return Err(TransportError::Parse(
                    "shell action has invalid canonical context".to_string(),
                ));
            }
        }
        ActionKind::RestoreExistingForUser => {
            if plan.request.context.confirmation_source != ConfirmationSource::JournalUndo
                || plan.request.context.permission.is_some()
                || !plan.request.context.shell_argv.is_empty()
                || plan.request.context.restore_enabled_state.is_none()
            {
                return Err(TransportError::Parse(
                    "install-existing action is not a verified journal inverse".to_string(),
                ));
            }
        }
        _ if plan.request.context.permission.is_some()
            || !plan.request.context.shell_argv.is_empty()
            || plan.request.context.restore_enabled_state.is_some() =>
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

pub fn valid_batch_id(batch_id: &str) -> bool {
    batch_id.starts_with("batch-")
        && batch_id.len() <= 96
        && batch_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
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
    fn archive_plans_are_user_scoped_and_canonical() {
        let request = |kind| ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.example.app".into(),
            kind,
            user_id: 10,
            pack_context: None,
            context: ActionContext {
                confirmation_source: ConfirmationSource::AppsPreview,
                ..Default::default()
            },
        };
        assert_eq!(
            plan(request(ActionKind::Archive)).args,
            ["pm", "archive", "--user", "10", "com.example.app"]
        );
        assert_eq!(
            plan(request(ActionKind::RequestUnarchive)).args,
            ["pm", "request-unarchive", "--user", "10", "com.example.app"]
        );
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
        assert_eq!(
            ActionKind::Archive.inverse(),
            Some(ActionKind::RequestUnarchive)
        );
        assert_eq!(
            ActionKind::RequestUnarchive.inverse(),
            Some(ActionKind::Archive)
        );
        assert!(ActionKind::Archive.is_reversible());
        assert!(ActionKind::Disable.is_reversible());
        assert!(ActionKind::Enable.is_reversible());
        assert!(!ActionKind::UninstallForUser.is_reversible());
        assert_eq!(
            ActionKind::UninstallForUser.inverse(),
            Some(ActionKind::RestoreExistingForUser)
        );
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
                restore_enabled_state: None,
                batch_id: None,
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
                restore_enabled_state: None,
                batch_id: None,
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
                    restore_enabled_state: None,
                    batch_id: None,
                },
            }),
            "2026-07-14T12:00:00Z",
        )
        .unwrap();
        assert_eq!(applied.before_state, "revoked");
        assert_eq!(applied.after_state, "granted");
    }

    #[test]
    fn archive_and_unarchive_complete_a_verified_round_trip() {
        let mock = MockTransport::new().with_devices(vec![device("abc")]);
        let disabled = "package:/data/app/Example/base.apk=com.example.app\n";
        let package_probe = [
            "sh",
            "-c",
            "pm get-archived-package-metadata --user \"$1\" \"$2\" >/dev/null",
            "droidsmith",
            "0",
            "com.example.app",
        ];

        // Archive: verify a user-installed source, execute the canonical API 35
        // command, then observe the archived metadata predicate.
        expect_owner(&mock);
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "0",
                "-d",
                "-f",
                "com.example.app",
            ],
            Ok(disabled.into()),
        );
        mock.expect_shell(
            "abc",
            &["getprop", "ro.build.version.sdk"],
            Ok("35\n".into()),
        );
        mock.expect_shell(
            "abc",
            &["pm", "archive", "--user", "0", "com.example.app"],
            Ok("Success\n".into()),
        );
        for flag in ["-d", "-e"] {
            mock.expect_shell(
                "abc",
                &[
                    "pm",
                    "list",
                    "packages",
                    "--user",
                    "0",
                    flag,
                    "-f",
                    "com.example.app",
                ],
                Ok(String::new()),
            );
        }
        mock.expect_shell(
            "abc",
            &["getprop", "ro.build.version.sdk"],
            Ok("35\n".into()),
        );
        mock.expect_shell("abc", &package_probe, Ok(String::new()));

        let archived = apply(
            &mock,
            plan(ActionRequest {
                serial: "abc".into(),
                target: target("abc"),
                package: "com.example.app".into(),
                kind: ActionKind::Archive,
                user_id: 0,
                pack_context: None,
                context: ActionContext::default(),
            }),
            "2026-07-15T12:00:00Z",
        )
        .unwrap();
        assert_eq!(archived.before_state, "user_installed_disabled");
        assert_eq!(archived.after_state, "archived");

        // Undo: require the archived source state, submit Android's asynchronous
        // unarchive request, and wait for PackageManager to restore the app.
        expect_owner(&mock);
        for flag in ["-d", "-e"] {
            mock.expect_shell(
                "abc",
                &[
                    "pm",
                    "list",
                    "packages",
                    "--user",
                    "0",
                    flag,
                    "-f",
                    "com.example.app",
                ],
                Ok(String::new()),
            );
        }
        mock.expect_shell(
            "abc",
            &["getprop", "ro.build.version.sdk"],
            Ok("35\n".into()),
        );
        mock.expect_shell("abc", &package_probe, Ok(String::new()));
        mock.expect_shell(
            "abc",
            &["getprop", "ro.build.version.sdk"],
            Ok("35\n".into()),
        );
        mock.expect_shell(
            "abc",
            &["pm", "request-unarchive", "--user", "0", "com.example.app"],
            Ok("Success\n".into()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "0",
                "-d",
                "-f",
                "com.example.app",
            ],
            Ok(disabled.into()),
        );

        let restored = apply(
            &mock,
            plan(ActionRequest {
                serial: "abc".into(),
                target: target("abc"),
                package: "com.example.app".into(),
                kind: ActionKind::RequestUnarchive,
                user_id: 0,
                pack_context: None,
                context: ActionContext {
                    confirmation_source: ConfirmationSource::JournalUndo,
                    ..Default::default()
                },
            }),
            "2026-07-15T12:00:01Z",
        )
        .unwrap();
        assert_eq!(restored.before_state, "archived");
        assert_eq!(restored.after_state, "user_installed_disabled");
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
                    restore_enabled_state: None,
                    batch_id: None,
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
    fn uninstall_records_only_verified_retained_system_recovery_state() {
        let mock = MockTransport::new().with_devices(vec![device("abc")]);
        expect_owner(&mock);
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "0",
                "-d",
                "-f",
                "com.system",
            ],
            Ok(String::new()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "0",
                "-e",
                "-f",
                "com.system",
            ],
            Ok("package:/system/app/System/System.apk=com.system\n".into()),
        );
        mock.expect_shell(
            "abc",
            &["pm", "uninstall", "--user", "0", "com.system"],
            Ok("Success\n".into()),
        );
        for flag in ["-d", "-e"] {
            mock.expect_shell(
                "abc",
                &[
                    "pm",
                    "list",
                    "packages",
                    "--user",
                    "0",
                    flag,
                    "-f",
                    "com.system",
                ],
                Ok(String::new()),
            );
        }
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "0",
                "-u",
                "-f",
                "com.system",
            ],
            Ok("package:/system/app/System/System.apk=com.system\n".into()),
        );

        let applied = apply(
            &mock,
            plan(ActionRequest {
                serial: "abc".into(),
                target: target("abc"),
                package: "com.system".into(),
                kind: ActionKind::UninstallForUser,
                user_id: 0,
                pack_context: None,
                context: ActionContext::default(),
            }),
            "2026-07-15T12:00:00Z",
        )
        .unwrap();

        assert_eq!(applied.before_state, "preinstalled_enabled");
        assert_eq!(applied.after_state, "retained_preinstalled");
    }

    #[test]
    fn restore_existing_reconciles_and_verifies_prior_disabled_state() {
        let mock = MockTransport::new().with_devices(vec![device("abc")]);
        mock.expect_shell(
            "abc",
            &["pm", "list", "users"],
            Ok("Users:\n  UserInfo{0:Owner:c13} running (current)\n  UserInfo{10:Work:30} running\n".into()),
        );
        mock.expect_shell("abc", &["am", "get-current-user"], Ok("0\n".into()));
        mock.expect_shell(
            "abc",
            &[
                "cmd",
                "package",
                "install-existing",
                "--user",
                "10",
                "com.system",
            ],
            Ok("Package com.system installed for user: 10\n".into()),
        );
        mock.expect_shell(
            "abc",
            &["pm", "disable-user", "--user", "10", "com.system"],
            Ok("Package com.system new state: disabled-user\n".into()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "10",
                "-d",
                "-f",
                "com.system",
            ],
            Ok("package:/product/app/System/System.apk=com.system\n".into()),
        );
        let mut restore = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.system".into(),
            kind: ActionKind::RestoreExistingForUser,
            user_id: 10,
            pack_context: None,
            context: ActionContext {
                confirmation_source: ConfirmationSource::JournalUndo,
                restore_enabled_state: Some(false),
                ..Default::default()
            },
        });
        restore.before_state = "retained_preinstalled".into();

        let applied = apply(&mock, restore, "2026-07-15T12:05:00Z").unwrap();
        assert_eq!(applied.after_state, "preinstalled_disabled");
        assert!(applied.display_stdout.contains("installed for user"));
    }

    #[test]
    fn restore_existing_surfaces_unsupported_oem_command_without_claiming_success() {
        let mock = MockTransport::new().with_devices(vec![device("abc")]);
        expect_owner(&mock);
        mock.expect_shell(
            "abc",
            &[
                "cmd",
                "package",
                "install-existing",
                "--user",
                "0",
                "com.system",
            ],
            Ok("Error: unknown command 'install-existing'\n".into()),
        );
        let mut restore = plan(ActionRequest {
            serial: "abc".into(),
            target: target("abc"),
            package: "com.system".into(),
            kind: ActionKind::RestoreExistingForUser,
            user_id: 0,
            pack_context: None,
            context: ActionContext {
                confirmation_source: ConfirmationSource::JournalUndo,
                restore_enabled_state: Some(true),
                ..Default::default()
            },
        });
        restore.before_state = "retained_preinstalled".into();

        assert!(matches!(
            apply(&mock, restore, "2026-07-15T12:10:00Z"),
            Err(TransportError::Exit { .. })
        ));
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
