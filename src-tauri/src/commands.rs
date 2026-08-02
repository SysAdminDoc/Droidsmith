//! Shared command-boundary primitives.
//!
//! Renderer-callable entry points live in the domain modules below. This core
//! owns only cross-domain error conversion, target/transport validation,
//! journal execution, native path guards, and bounded subprocess helpers.

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::adb::device::valid_serial;
use crate::adb::packages::valid_package_name;
use crate::adb::parsers::{
    audit_layout_nodes, parse_effective_density, parse_fastboot_devices, parse_ls_output,
    parse_ps_output, parse_running_services, parse_ss_output, parse_uiautomator_dump,
    FastbootDevice, LayoutAuditFinding, LayoutNode, NetworkConnection, ProcessInfo,
    RemoteFileEntry, RunningService,
};
use crate::adb::transport::AdbTransport;
use crate::adb::{self, actions};
use crate::apk_metadata;
use crate::backup;
use crate::bugreport;
use crate::device_identity::DeviceIdentity;
use crate::fleet_report;
use crate::fs_util::{ArtifactError, ArtifactKind, HostArtifact, StagedArtifact};
use crate::host_path::{
    open_directory_command, open_with_command, reveal_command, validate_suggested_file_name,
    HostPathGrant, HostPathPurpose, PathGrantError, PathGrantStore,
};
use crate::install;
use crate::journal::{self, Journal, JournalEntry};
use crate::operations::{self, OperationEvent};
use crate::perfetto;
use crate::profile;
use crate::quirks::{self, DeviceContext, Quirk};
use crate::recovery_baseline::{self, BaselineActionInput, BaselinePack, RecoveryBaselineDiff};
use crate::remote_files;
use crate::settings;
use crate::support_bundle;

mod diagnostics;
pub(crate) use diagnostics::*;
mod devices;
pub(crate) use devices::*;
mod wireless;
pub(crate) use wireless::*;
mod packages;
pub(crate) use packages::*;
mod profiles;
pub(crate) use profiles::*;
mod plans;
pub(crate) use plans::*;
mod settings_commands;
pub(crate) use settings_commands::*;
mod files;
pub(crate) use files::*;
mod actions_commands;
pub(crate) use actions_commands::*;
mod console;
pub(crate) use console::*;
mod system;
pub(crate) use system::*;
mod mirror;
pub(crate) use mirror::*;
mod installs;
pub(crate) use installs::*;
mod packs;
pub(crate) use packs::*;

/// Generic Tauri-command error envelope so the JS side gets the same
/// shape regardless of whether the underlying failure was a transport
/// error or a filesystem error from the journal.
#[derive(specta::Type, Debug, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct CommandError {
    /// Stable string code for client-side branching (e.g. "adb_not_found").
    pub code: &'static str,
    pub message: String,
}

impl From<adb::TransportError> for CommandError {
    fn from(e: adb::TransportError) -> Self {
        let code: &'static str = match &e {
            adb::TransportError::AdbNotFound => "adb_not_found",
            adb::TransportError::Spawn(_) => "spawn_failed",
            adb::TransportError::Exit { .. } => "adb_exit",
            adb::TransportError::Signaled { .. } => "adb_signaled",
            adb::TransportError::Timeout(_) => "adb_timeout",
            adb::TransportError::OutputLimit { .. } => "subprocess_output_limit",
            adb::TransportError::Parse(_) => "parse_error",
        };
        Self {
            code,
            message: e.to_string(),
        }
    }
}

impl From<std::io::Error> for CommandError {
    fn from(e: std::io::Error) -> Self {
        Self {
            code: "io_error",
            message: e.to_string(),
        }
    }
}

impl From<ArtifactError> for CommandError {
    fn from(error: ArtifactError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<PathGrantError> for CommandError {
    fn from(error: PathGrantError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<recovery_baseline::RecoveryBaselineError> for CommandError {
    fn from(error: recovery_baseline::RecoveryBaselineError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<profile::ProfileError> for CommandError {
    fn from(error: profile::ProfileError) -> Self {
        let code = match &error {
            profile::ProfileError::Read { .. } => "profile_read_failed",
            profile::ProfileError::Parse { .. } => "profile_parse_failed",
            profile::ProfileError::Validate { .. } => "profile_invalid",
            profile::ProfileError::Serialize(_) => "profile_serialize_failed",
            profile::ProfileError::Save(_) => "profile_save_failed",
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}

impl From<fleet_report::FleetReportError> for CommandError {
    fn from(error: fleet_report::FleetReportError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<remote_files::RemoteFileError> for CommandError {
    fn from(error: remote_files::RemoteFileError) -> Self {
        Self {
            code: "invalid_remote_file_operation",
            message: error.to_string(),
        }
    }
}

impl From<operations::OperationError> for CommandError {
    fn from(error: operations::OperationError) -> Self {
        let code = match &error {
            operations::OperationError::InvalidId(_) => "invalid_operation_id",
            operations::OperationError::DuplicateId(_) => "operation_already_running",
            operations::OperationError::Spawn { .. } => "spawn_failed",
            operations::OperationError::Wait(_) => "process_wait_failed",
            operations::OperationError::Input(_) => "process_input_failed",
            operations::OperationError::Terminate(_) => "process_terminate_failed",
            operations::OperationError::OutputRead { .. } => "process_output_read_failed",
            operations::OperationError::ReaderPanicked(_) => "process_output_reader_failed",
            operations::OperationError::UnexpectedExit(_) => "process_exited_unexpectedly",
            operations::OperationError::Cancelled => "operation_cancelled",
            operations::OperationError::Timeout(_) => "operation_timeout",
            operations::OperationError::OutputTooLarge(_) => "operation_output_too_large",
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}

impl From<install::InstallError> for CommandError {
    fn from(error: install::InstallError) -> Self {
        match error {
            install::InstallError::InvalidSource(message) => Self {
                code: "invalid_install_source",
                message,
            },
            install::InstallError::Archive(error) => Self {
                code: "invalid_install_archive",
                message: error.to_string(),
            },
            install::InstallError::Io(error) => Self::from(error),
            install::InstallError::Operation(error) => Self::from(error),
        }
    }
}

impl From<backup::BackupError> for CommandError {
    fn from(error: backup::BackupError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<bugreport::BugreportError> for CommandError {
    fn from(error: bugreport::BugreportError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<perfetto::PerfettoError> for CommandError {
    fn from(error: perfetto::PerfettoError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<apk_metadata::MetadataError> for CommandError {
    fn from(error: apk_metadata::MetadataError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<settings::SettingsError> for CommandError {
    fn from(error: settings::SettingsError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

async fn spawn_blocking_operation<T, F>(operation: F) -> Result<T, CommandError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CommandError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| CommandError {
            code: "operation_join_failed",
            message: format!("background operation task failed: {error}"),
        })?
}

fn completed_adb_output(
    output: operations::ProcessOutput,
    program_name: &str,
) -> Result<String, CommandError> {
    if output.success() {
        Ok(output.stdout)
    } else {
        Err(CommandError {
            code: "adb_exit",
            message: format!(
                "{program_name} exited with code {}: {}",
                output.code.unwrap_or(-1),
                if output.stderr.trim().is_empty() {
                    output.stdout.trim()
                } else {
                    output.stderr.trim()
                }
            ),
        })
    }
}

fn journal_dir(app: &tauri::AppHandle) -> Result<PathBuf, CommandError> {
    let base = app.path().app_data_dir().map_err(|e| CommandError {
        code: "no_app_data_dir",
        message: e.to_string(),
    })?;
    Ok(base.join("journal"))
}

fn validate_serial_arg(serial: &str) -> Result<(), CommandError> {
    if valid_serial(serial) {
        Ok(())
    } else {
        Err(CommandError {
            code: "invalid_serial",
            message: format!("invalid device serial {serial:?}"),
        })
    }
}

fn execute_journaled(
    journal: &mut Journal,
    transport: &dyn AdbTransport,
    mut plan: actions::PlannedAction,
    undoes: Option<u64>,
) -> Result<ApplyActionResult, CommandError> {
    actions::validate_plan(&plan)?;
    adb::validate_device_target(transport, &plan.request.target)?;
    if plan.before_state.is_empty() {
        plan.before_state = actions::capture_state(transport, &plan.request);
    }
    // R-122: prove reinstall feasibility while the package still exists. After
    // the uninstall the evidence is gone, so an undo decision made later would
    // have nothing to read. The renderer cannot supply this — it is derived
    // here, on the same validated transport that performs the mutation.
    if plan.request.kind == actions::ActionKind::UninstallForUser {
        plan.recovery = Some(adb::packages::assess_uninstall_recovery(
            transport,
            &plan.request.target,
            plan.request.user_id,
            &plan.request.package,
        ));
    }
    let started_at = iso_now();
    let entry = journal
        .execute(plan, undoes, &started_at, |plan| {
            actions::apply(transport, plan, &iso_now())
        })
        .map_err(|error| match error {
            journal::ExecuteError::Journal(error) => CommandError::from(error),
            journal::ExecuteError::Operation(error) => CommandError {
                code: "package_action_failed",
                message: actions::package_action_failure_message(&error.to_string()),
            },
        })?;
    Ok(ApplyActionResult {
        stdout: entry.applied.display_stdout.clone(),
        entry,
    })
}

fn execute_remote_file_journaled(
    journal: &mut Journal,
    transport: &adb::ShellTransport,
    mut plan: actions::PlannedAction,
) -> Result<ApplyActionResult, CommandError> {
    actions::validate_plan(&plan)?;
    adb::validate_device_target(transport, &plan.request.target)?;
    plan.before_state = actions::capture_state(transport, &plan.request);
    let started_at = iso_now();
    let entry = journal
        .execute(
            plan,
            None,
            &started_at,
            |plan| -> Result<_, adb::TransportError> {
                let applied = actions::apply(transport, plan, &iso_now())?;
                remote_files::verify_transition(
                    transport,
                    &applied.plan.request.target,
                    &applied.plan.args,
                )?;
                Ok(applied)
            },
        )
        .map_err(map_remote_file_execute_error)?;
    Ok(ApplyActionResult {
        stdout: entry.applied.display_stdout.clone(),
        entry,
    })
}

fn map_remote_file_execute_error<E: std::fmt::Display>(
    error: journal::ExecuteError<E>,
) -> CommandError {
    match error {
        journal::ExecuteError::Journal(error) => CommandError::from(error),
        journal::ExecuteError::Operation(error) => CommandError {
            code: "remote_file_operation_failed",
            message: error.to_string(),
        },
    }
}

fn current_android_user(
    transport: &adb::ShellTransport,
    target: &adb::DeviceTarget,
) -> Result<u32, CommandError> {
    adb::list_users(transport, target)?
        .into_iter()
        .find(|user| user.current)
        .map(|user| user.id)
        .ok_or(CommandError {
            code: "current_user_missing",
            message: "could not bind the remote file operation to the current Android user"
                .to_string(),
        })
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ApplyActionResult {
    pub entry: JournalEntry,
    /// Raw output is returned only to the initiating view and is excluded from
    /// the persisted journal, which carries the redacted/bounded copy.
    pub stdout: String,
}

const MAX_ACTION_BATCH_ITEMS: usize = 100;

#[derive(specta::Type, Debug, Clone, Serialize, Deserialize)]
pub struct BatchActionPlan {
    pub plans: Vec<actions::PlannedAction>,
    pub description: String,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct BatchActionItemResult {
    pub package: String,
    pub entry: Option<JournalEntry>,
    pub stdout: String,
    pub error: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct BatchActionResult {
    pub batch_id: String,
    pub items: Vec<BatchActionItemResult>,
}

fn validated_transport_with_device(
    target: &adb::DeviceTarget,
) -> Result<(adb::ShellTransport, adb::Device), CommandError> {
    validate_serial_arg(&target.serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    let device = adb::validate_device_target(&transport, target)?;
    Ok((transport, device))
}

fn validated_transport(target: &adb::DeviceTarget) -> Result<adb::ShellTransport, CommandError> {
    validated_transport_with_device(target).map(|(transport, _)| transport)
}

fn accepted_transport_override(
    kind: adb::DeviceTransportKind,
    acknowledged: bool,
) -> Result<Option<adb::DeviceTransportKind>, ()> {
    if kind.requires_override() {
        acknowledged.then_some(Some(kind)).ok_or(())
    } else {
        Ok(None)
    }
}

/// Revalidate transport provenance at the privileged boundary. Renderer
/// acknowledgement is authorization metadata, never evidence that a TCP
/// endpoint is authenticated.
fn privileged_transport(
    target: &adb::DeviceTarget,
) -> Result<(adb::ShellTransport, Option<adb::DeviceTransportKind>), CommandError> {
    let (transport, device) = validated_transport_with_device(target)?;
    let override_kind = accepted_transport_override(
        device.transport_kind,
        target.untrusted_transport_override,
    )
    .map_err(|()| CommandError {
        code: "untrusted_transport_override_required",
        message: format!(
            "{} is connected over an unauthenticated {} transport; explicitly acknowledge the warning before running this operation",
            target.serial,
            device.transport_kind.label()
        ),
    })?;
    Ok((transport, override_kind))
}

fn validate_package_arg(package: &str) -> Result<(), CommandError> {
    if valid_package_name(package) {
        Ok(())
    } else {
        Err(CommandError {
            code: "invalid_package",
            message: format!("invalid package name {package:?}"),
        })
    }
}

/// Validate a device-side (remote) path before it reaches `adb pull`,
/// `adb push`, or `pm`. Argv-scoped calls have no shell-metachar risk,
/// but a leading `-` would be parsed by adb as an option flag, so reject
/// it — and require an absolute device path so callers can't smuggle a
/// flag or relative token across the IPC boundary.
fn validate_remote_path(remote_path: &str) -> Result<String, CommandError> {
    remote_files::validate_path(remote_path).map_err(|error| CommandError {
        code: "invalid_remote_path",
        message: error.to_string(),
    })
}

fn validate_fastboot_key(key: &str) -> Result<(), CommandError> {
    if key.is_empty() || key.len() > 128 {
        return Err(CommandError {
            code: "invalid_key",
            message: "fastboot variable key is empty or too long".to_string(),
        });
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(CommandError {
            code: "invalid_key",
            message: format!("fastboot variable key contains invalid characters: {key:?}"),
        });
    }
    Ok(())
}

/// Typed result of one captured subprocess execution. Unlike the old
/// stdout-only helper, this keeps both streams plus the exit disposition
/// so callers such as `fastboot getvar` (which prints successful values
/// to stderr) can read the right stream without a blind retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
    /// Process exit code, or `None` when killed by signal.
    pub code: Option<i32>,
    /// True when the child was killed because it exceeded the timeout.
    pub timed_out: bool,
}

impl ProcessOutput {
    fn success(&self) -> bool {
        !self.timed_out && self.code == Some(0)
    }
}

/// Run a subprocess once, capturing stdout, stderr, exit code, and
/// timeout state in a single execution.
fn run_captured(
    program: &std::path::Path,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<ProcessOutput, CommandError> {
    use std::process::Command;

    let mut command = Command::new(program);
    command.args(args);
    let output = crate::process_capture::run(
        &mut command,
        timeout,
        crate::process_capture::CaptureLimits::default(),
    )
    .map_err(|error| CommandError {
        code: match error {
            crate::process_capture::CaptureError::Spawn(_) => "spawn_failed",
            _ => "subprocess_capture_failed",
        },
        message: format!("failed to run {}: {error}", program.display()),
    })?;
    let (code, timed_out) = match output.termination {
        crate::process_capture::CaptureTermination::Exited(status) => (status.code(), false),
        crate::process_capture::CaptureTermination::TimedOut => (None, true),
        crate::process_capture::CaptureTermination::OutputLimitExceeded {
            stream,
            limit_bytes,
        } => {
            return Err(CommandError {
                code: "subprocess_output_limit",
                message: format!(
                    "{} {stream} exceeded the {limit_bytes}-byte capture limit",
                    program.display()
                ),
            });
        }
    };
    Ok(ProcessOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code,
        timed_out,
    })
}

/// Backwards-compatible helper for callers that only care about stdout
/// on success (push/pull/df/ls/fastboot devices). Failures and timeouts
/// still surface stderr in the error message.
fn run_adb_simple(
    adb_path: &std::path::Path,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<String, CommandError> {
    let out = run_captured(adb_path, args, timeout)?;
    if out.timed_out {
        return Err(CommandError {
            code: "adb_timeout",
            message: format!("adb timed out after {timeout:?}"),
        });
    }
    if out.success() {
        Ok(out.stdout)
    } else {
        Err(CommandError {
            code: "adb_exit",
            message: format!(
                "adb exited with code {}: {}",
                out.code.unwrap_or(-1),
                out.stderr
            ),
        })
    }
}

fn iso_now() -> String {
    crate::time::iso_utc_now()
}

#[cfg(test)]
mod tests {
    use super::{
        accepted_transport_override, append_host_operation, classify_shell, debloat_target_ids,
        diagnostic_text, execute_batch_plans, is_allowed_device_control, load_all_packs,
        load_runtime_packs, pack_error_to_load_error, parse_fastboot_getvar, plan_action_batch,
        profile_preview_rows, unique_screenshot_remote, validate_action_batch_plan,
        validate_backup_target, validate_remote_path, AdbRecoveryOutcome, AdbRecoveryRecord,
        ProcessOutput, ProfilePreviewStatus, ShellClassification,
    };
    use crate::adb::device::DeviceState;
    use crate::adb::transport::MockTransport;
    use crate::adb::{
        actions::{ActionKind, ActionRequest},
        AppPackage, Device, DeviceTarget, DeviceTransportKind,
    };

    fn batch_device() -> Device {
        Device {
            serial: "batch-device".to_string(),
            state: DeviceState::Device,
            model: Some("Pixel".to_string()),
            product: Some("pixel".to_string()),
            device: Some("husky".to_string()),
            bus_address: None,
            connection_type: None,
            negotiated_speed: None,
            max_speed: None,
            build_fingerprint: Some("google/husky/build".to_string()),
            transport_id: Some(9),
            connection_generation: 10,
            transport_kind: DeviceTransportKind::Usb,
            wireless: false,
        }
    }

    fn batch_request(package: &str, kind: ActionKind) -> ActionRequest {
        let device = batch_device();
        ActionRequest {
            serial: device.serial.clone(),
            target: device.target(),
            package: package.to_string(),
            kind,
            user_id: 0,
            pack_context: None,
            context: Default::default(),
        }
    }

    #[test]
    fn batch_planner_rejects_mixed_or_duplicate_targets() {
        let duplicate = plan_action_batch(vec![
            batch_request("com.example.one", ActionKind::Disable),
            batch_request("com.example.one", ActionKind::Disable),
        ])
        .unwrap_err();
        assert_eq!(duplicate.code, "duplicate_batch_package");

        let mixed = plan_action_batch(vec![
            batch_request("com.example.one", ActionKind::Disable),
            batch_request("com.example.two", ActionKind::Enable),
        ])
        .unwrap_err();
        assert_eq!(mixed.code, "mixed_action_batch");

        let irreversible = plan_action_batch(vec![
            batch_request("com.example.one", ActionKind::ClearData),
            batch_request("com.example.two", ActionKind::ClearData),
        ])
        .unwrap_err();
        assert_eq!(irreversible.code, "invalid_action_kind");
    }

    #[test]
    fn batch_executor_continues_after_a_package_failure() {
        let mut batch = plan_action_batch(vec![
            batch_request("com.example.ok", ActionKind::Disable),
            batch_request("com.example.fail", ActionKind::Disable),
        ])
        .unwrap();
        assert!(validate_action_batch_plan(&batch).is_ok());
        for plan in &mut batch.plans {
            plan.request.context.batch_id = Some("batch-test-1".to_string());
        }

        let device = batch_device();
        let mock = MockTransport::new().with_devices(vec![device]);
        for package in ["com.example.ok", "com.example.fail"] {
            mock.expect_shell(
                "batch-device",
                &["pm", "list", "packages", "--user", "0", "-d", package],
                Ok(String::new()),
            );
            mock.expect_shell(
                "batch-device",
                &["pm", "list", "packages", "--user", "0", package],
                Ok(format!("package:{package}\n")),
            );
            mock.expect_shell(
                "batch-device",
                &["pm", "list", "users"],
                Ok("Users:\n  UserInfo{0:Owner:c13} running (current)\n".to_string()),
            );
            mock.expect_shell(
                "batch-device",
                &["am", "get-current-user"],
                Ok("0\n".to_string()),
            );
        }
        mock.expect_shell(
            "batch-device",
            &["pm", "disable-user", "--user", "0", "com.example.ok"],
            Ok("Package com.example.ok new state: disabled-user\n".to_string()),
        );
        mock.expect_shell(
            "batch-device",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "0",
                "-d",
                "com.example.ok",
            ],
            Ok("package:com.example.ok\n".to_string()),
        );
        mock.expect_shell(
            "batch-device",
            &["pm", "disable-user", "--user", "0", "com.example.fail"],
            Ok("Failure [PACKAGE_NOT_FOUND]\n".to_string()),
        );

        let dir = std::env::temp_dir().join(format!(
            "droidsmith-batch-test-{}-{}",
            std::process::id(),
            crate::time::iso_utc_now().replace([':', '.'], "-")
        ));
        let mut journal = crate::journal::Journal::open(
            &dir,
            &crate::device_identity::DeviceIdentity::new("batch-device", Some("build/test")),
        )
        .unwrap();
        let items = execute_batch_plans(&mut journal, &mock, batch.plans, None).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].error.is_none());
        assert!(items[1].error.is_some());
        assert_eq!(journal.entries().len(), 2);
        assert_eq!(
            journal.entries()[0].outcome,
            crate::journal::JournalOutcome::Succeeded
        );
        assert_eq!(
            journal.entries()[1].outcome,
            crate::journal::JournalOutcome::Failed
        );
        assert!(journal.entries().iter().all(|entry| {
            entry.applied.plan.request.context.batch_id.as_deref() == Some("batch-test-1")
        }));
        drop(journal);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn profile_preview_reports_ready_matching_and_missing_rows_without_mutation() {
        let profile = crate::profile::Profile {
            name: "preview-test".to_string(),
            version: crate::profile::PROFILE_SCHEMA_VERSION.to_string(),
            description: String::new(),
            device: Default::default(),
            user: Default::default(),
            actions: vec![
                crate::profile::ProfileAction {
                    kind: ActionKind::Disable,
                    package: "com.example.enabled".to_string(),
                    filter: String::new(),
                    note: String::new(),
                },
                crate::profile::ProfileAction {
                    kind: ActionKind::Disable,
                    package: "com.example.disabled".to_string(),
                    filter: String::new(),
                    note: String::new(),
                },
                crate::profile::ProfileAction {
                    kind: ActionKind::Enable,
                    package: "com.example.missing".to_string(),
                    filter: String::new(),
                    note: String::new(),
                },
            ],
        };
        let target = DeviceTarget {
            serial: "QA123".to_string(),
            transport_id: Some(7),
            connection_generation: 2,
            transport_kind: DeviceTransportKind::Usb,
            untrusted_transport_override: false,
            model: Some("Pixel QA".to_string()),
            product: None,
            device: None,
            build_fingerprint: Some("google/qa/qa:17/test".to_string()),
        };
        let package = |name: &str, enabled: bool| AppPackage {
            package: name.to_string(),
            enabled,
            system: false,
            apk_path: None,
            uid: None,
            installer: None,
            archived: false,
            retained: false,
        };
        let rows = profile_preview_rows(
            &profile,
            &target,
            10,
            &[
                package("com.example.enabled", true),
                package("com.example.disabled", false),
            ],
        );

        assert!(matches!(rows[0].status, ProfilePreviewStatus::Ready));
        assert!(matches!(
            rows[1].status,
            ProfilePreviewStatus::AlreadyMatches
        ));
        assert!(matches!(rows[2].status, ProfilePreviewStatus::Missing));
        assert!(rows.iter().all(|row| row.plan.request.user_id == 10));
        assert!(rows
            .iter()
            .all(|row| row.plan.request.context.confirmation_source
                == crate::adb::actions::ConfirmationSource::ProfilePreview));
    }

    #[test]
    fn debloat_targets_exclude_archived_and_retained_packages() {
        let package = |name: &str, archived: bool, retained: bool| AppPackage {
            package: name.to_string(),
            enabled: true,
            system: false,
            apk_path: None,
            uid: None,
            installer: None,
            archived,
            retained,
        };
        let ids = debloat_target_ids(vec![
            package("com.keep.enabled", false, false),
            package("com.skip.archived", true, false),
            package("com.skip.retained", false, true),
        ]);
        assert!(ids.contains("com.keep.enabled"));
        // Archived and uninstalled-for-user "retained" remnants cannot be
        // disabled and must not be counted as installed debloat targets.
        assert!(!ids.contains("com.skip.archived"));
        assert!(!ids.contains("com.skip.retained"));
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn host_recovery_records_are_newline_delimited_and_synced() {
        let dir = std::env::temp_dir().join(format!(
            "droidsmith-host-audit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("host-operations.jsonl");
        let record = AdbRecoveryRecord {
            schema_version: 1,
            operation_id: "adb-recovery-test".to_string(),
            operation: "adb_server_recovery",
            confirmation_source: "devices_health_review",
            outcome: AdbRecoveryOutcome::Pending,
            started_at: "2026-07-14T18:00:00Z".to_string(),
            completed_at: None,
            commands: vec![vec!["kill-server".to_string()]],
            health_before: None,
            health_after: None,
            failure: None,
        };
        append_host_operation(&path, &record).unwrap();
        append_host_operation(&path, &record).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn host_diagnostic_text_is_bounded_and_drops_controls() {
        let value = format!("ok\0{}", "x".repeat(2_000));
        let sanitized = diagnostic_text(&value);
        assert!(!sanitized.contains('\0'));
        assert_eq!(sanitized.chars().count(), 1_024);
    }

    #[test]
    fn remote_path_rejects_flags_relative_and_traversal() {
        // A leading '-' would reach adb as an option flag.
        assert_eq!(
            validate_remote_path("-a").unwrap_err().code,
            "invalid_remote_path"
        );
        // Relative and empty are rejected; only absolute device paths pass.
        assert_eq!(
            validate_remote_path("sdcard/x").unwrap_err().code,
            "invalid_remote_path"
        );
        assert_eq!(
            validate_remote_path("   ").unwrap_err().code,
            "invalid_remote_path"
        );
        assert_eq!(
            validate_remote_path("/sdcard/../data/secret")
                .unwrap_err()
                .code,
            "invalid_remote_path"
        );
        assert_eq!(
            validate_remote_path("/sdcard/./Download").unwrap_err().code,
            "invalid_remote_path"
        );
        assert_eq!(
            validate_remote_path("/sdcard/Download\nsecret")
                .unwrap_err()
                .code,
            "invalid_remote_path"
        );
        assert_eq!(
            validate_remote_path("/sdcard/Download/app.apk").unwrap(),
            "/sdcard/Download/app.apk"
        );
    }

    #[test]
    fn shell_classifier_fails_unknown_commands_into_review() {
        let args = |values: &[&str]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            classify_shell(&args(&["getprop", "ro.product.model"])),
            ShellClassification::ReadOnly
        );
        assert_eq!(
            classify_shell(&args(&["wm", "density", "420"])),
            ShellClassification::Mutation
        );
        assert_eq!(
            classify_shell(&args(&["rm", "-rf", "/sdcard/data"])),
            ShellClassification::Dangerous
        );
        // A read-only head must not launder a chained/redirected mutation past
        // the classifier: any shell control metacharacter forces Dangerous so
        // shell_run rejects it and plan_shell_action routes it through review.
        assert_eq!(
            classify_shell(&args(&["getprop", "ro.build;", "pm", "uninstall", "com.x"])),
            ShellClassification::Dangerous
        );
        assert_eq!(
            classify_shell(&args(&["cat", "/proc/version", "&&", "reboot"])),
            ShellClassification::Dangerous
        );
        assert_eq!(
            classify_shell(&args(&["settings", "get", "secure", "$(reboot)"])),
            ShellClassification::Dangerous
        );
        assert_eq!(
            classify_shell(&args(&["getprop", ">", "/sdcard/x"])),
            ShellClassification::Dangerous
        );
        // Plain read-only commands with dotted/underscored operands still pass.
        assert_eq!(
            classify_shell(&args(&["settings", "get", "global", "adb_enabled"])),
            ShellClassification::ReadOnly
        );
        assert_eq!(
            classify_shell(&args(&["logcat", "-d", "ActivityManager:I", "*:S"])),
            ShellClassification::ReadOnly
        );
        for values in [
            &["logcat", "-c"][..],
            &["logcat", "--file", "/sdcard/log.txt"][..],
            &["logcat", "-G", "16M"][..],
            &["dumpsys", "battery", "set", "level", "1"][..],
            &["dumpsys", "deviceidle", "force-idle"][..],
        ] {
            assert_ne!(
                classify_shell(&args(values)),
                ShellClassification::ReadOnly,
                "{values:?} must go through reviewed execution"
            );
        }
        assert_eq!(
            classify_shell(&args(&["dumpsys", "battery"])),
            ShellClassification::ReadOnly
        );
    }

    #[test]
    fn device_control_allowlist_is_exact_and_bounded() {
        let args = |values: &[&str]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        };
        assert!(is_allowed_device_control(&args(&[
            "input", "keyevent", "3"
        ])));
        assert!(is_allowed_device_control(&args(&["wm", "density", "420"])));
        assert!(!is_allowed_device_control(&args(&[
            "wm", "density", "5000"
        ])));
        assert!(!is_allowed_device_control(&args(&[
            "settings",
            "delete",
            "secure",
            "adb_enabled"
        ])));
    }

    #[test]
    fn broken_pack_maps_to_stable_error_code() {
        let dir = std::env::temp_dir().join("droidsmith-pack-err-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Invalid YAML → parse error.
        let bad_parse = dir.join("broken.yaml");
        std::fs::write(&bad_parse, "name: [unterminated\n").unwrap();
        let err = crate::packs::load(&bad_parse).unwrap_err();
        let mapped = pack_error_to_load_error("broken.yaml".to_string(), &err);
        assert_eq!(mapped.code, "pack_parse");
        assert_eq!(mapped.file, "broken.yaml");
        assert!(!mapped.message.is_empty());

        // Well-formed YAML (all required fields present) that fails lint
        // on an empty name → validate error.
        let bad_validate = dir.join("empty.yaml");
        std::fs::write(
            &bad_validate,
            "name: \"\"\nversion: \"1\"\ndescription: \"x\"\npackages:\n  - id: com.x\n    removal: recommended\n    description: y\n",
        )
        .unwrap();
        let err = crate::packs::load(&bad_validate).unwrap_err();
        let mapped = pack_error_to_load_error("empty.yaml".to_string(), &err);
        assert_eq!(mapped.code, "pack_validate");
    }

    #[test]
    fn runtime_pack_loader_never_lists_underscore_templates() {
        let dir = std::env::temp_dir().join("droidsmith-runtime-pack-filter-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let yaml = r#"
id: runtime-test
revision: 1
name: Runtime test
version: "1"
description: Runtime loader test.
targets:
  user_scope: any
provenance:
  source: https://example.invalid/test
  license: MIT
packages:
  - id: com.example.runtime
    removal: recommended
    description: Runtime test package.
"#;
        std::fs::write(dir.join("runtime.yaml"), yaml).unwrap();
        std::fs::write(
            dir.join("_template.yaml"),
            yaml.replace("runtime-test", "template-test"),
        )
        .unwrap();

        let (packs, errors) = load_runtime_packs(&dir).unwrap();
        assert!(errors.is_empty());
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "runtime-test");
    }

    #[test]
    fn runtime_pack_loader_rejects_duplicate_stable_ids() {
        let dir = std::env::temp_dir().join("droidsmith-runtime-pack-id-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let yaml = r#"
id: duplicate-test
revision: 1
name: Duplicate test
version: "1"
description: Duplicate stable ID test.
targets:
  user_scope: any
provenance:
  source: https://example.invalid/test
  license: MIT
packages:
  - id: com.example.duplicate
    removal: recommended
    description: Duplicate test package.
"#;
        std::fs::write(dir.join("one.yaml"), yaml).unwrap();
        std::fs::write(dir.join("two.yaml"), yaml).unwrap();

        let (packs, errors) = load_runtime_packs(&dir).unwrap();
        assert!(packs.is_empty());
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|error| error.code == "pack_duplicate_id"));
    }

    fn pack_yaml(id: &str, package: &str) -> String {
        format!(
            r#"
id: {id}
revision: 1
name: Pack {id}
version: "1"
description: Merge loader test pack.
targets:
  user_scope: any
provenance:
  source: https://example.invalid/test
  license: MIT
packages:
  - id: {package}
    removal: recommended
    description: Merge test package.
"#
        )
    }

    fn merge_dirs(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("droidsmith-merge-pack-{name}"));
        let _ = std::fs::remove_dir_all(&base);
        let bundled = base.join("bundled");
        let user = base.join("user");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&user).unwrap();
        (bundled, user)
    }

    #[test]
    fn merged_loader_flags_bundled_and_imported_packs() {
        let (bundled, user) = merge_dirs("merge");
        std::fs::write(
            bundled.join("shipped.yaml"),
            pack_yaml("shipped-pack", "com.example.shipped"),
        )
        .unwrap();
        std::fs::write(
            user.join("imported.yaml"),
            pack_yaml("imported-pack", "com.example.imported"),
        )
        .unwrap();

        let (packs, errors) = load_all_packs(&bundled, &user).unwrap();
        assert!(errors.is_empty());
        assert_eq!(packs.len(), 2);
        // Sorted by id: imported-pack < shipped-pack.
        assert_eq!(packs[0].0.id, "imported-pack");
        assert!(packs[0].1, "imported pack is flagged imported");
        assert_eq!(packs[1].0.id, "shipped-pack");
        assert!(!packs[1].1, "bundled pack is not flagged imported");
    }

    #[test]
    fn merged_loader_rejects_imported_pack_shadowing_bundled_id() {
        let (bundled, user) = merge_dirs("shadow");
        std::fs::write(
            bundled.join("shipped.yaml"),
            pack_yaml("shared-id", "com.example.bundled"),
        )
        .unwrap();
        std::fs::write(
            user.join("shadow.yaml"),
            pack_yaml("shared-id", "com.example.user"),
        )
        .unwrap();

        let (packs, errors) = load_all_packs(&bundled, &user).unwrap();
        // Only the bundled pack survives; the shadowing import is an error.
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].0.id, "shared-id");
        assert!(!packs[0].1);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "pack_duplicate_id");
    }

    #[test]
    fn merged_loader_tolerates_a_missing_user_directory() {
        let (bundled, user) = merge_dirs("no-user");
        std::fs::write(
            bundled.join("shipped.yaml"),
            pack_yaml("only-bundled", "com.example.only"),
        )
        .unwrap();
        std::fs::remove_dir_all(&user).unwrap();

        let (packs, errors) = load_all_packs(&bundled, &user).unwrap();
        assert!(errors.is_empty());
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].0.id, "only-bundled");
    }

    #[test]
    fn screenshot_remote_paths_are_unique() {
        let a = unique_screenshot_remote();
        let b = unique_screenshot_remote();
        assert_ne!(a, b);
        assert!(a.starts_with("/sdcard/droidsmith-screenshot-"));
        assert!(a.ends_with(".png"));
    }

    fn fake(stdout: &str, stderr: &str, code: Option<i32>, timed_out: bool) -> ProcessOutput {
        ProcessOutput {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            code,
            timed_out,
        }
    }

    #[test]
    fn fastboot_getvar_reads_value_from_stderr() {
        // Real fastboot prints the value to stderr and exits 0 with empty
        // stdout — the exact case the old stdout-only path dropped.
        let out = fake(
            "",
            "version-bootloader: SLIDER-1.2\nfinished. total time: 0.001s\n",
            Some(0),
            false,
        );
        let value = parse_fastboot_getvar("version-bootloader", &out).unwrap();
        assert_eq!(value, "SLIDER-1.2");
    }

    #[test]
    fn fastboot_getvar_reads_value_from_stdout_fallback() {
        let out = fake("product: oriole\n", "", Some(0), false);
        assert_eq!(parse_fastboot_getvar("product", &out).unwrap(), "oriole");
    }

    #[test]
    fn fastboot_getvar_surfaces_error_with_both_streams() {
        let out = fake(
            "",
            "getvar:bogus FAILED (remote: 'unknown variable')\n",
            Some(1),
            false,
        );
        let err = parse_fastboot_getvar("bogus", &out).unwrap_err();
        assert_eq!(err.code, "fastboot_exit");
        assert!(err.message.contains("unknown variable"));
    }

    #[test]
    fn fastboot_getvar_reports_timeout() {
        let out = fake("", "", None, true);
        let err = parse_fastboot_getvar("version", &out).unwrap_err();
        assert_eq!(err.code, "fastboot_timeout");
    }

    #[test]
    fn fastboot_getvar_no_value_on_clean_but_empty() {
        let out = fake("", "finished. total time: 0.000s\n", Some(0), false);
        let err = parse_fastboot_getvar("version", &out).unwrap_err();
        assert_eq!(err.code, "fastboot_no_value");
    }

    #[test]
    fn backup_target_rejects_empty_and_relative_paths() {
        let empty = validate_backup_target("   ").unwrap_err();
        assert_eq!(empty.code, "invalid_backup_path");

        let relative = validate_backup_target("package.ab").unwrap_err();
        assert_eq!(relative.code, "invalid_backup_path");
    }

    #[test]
    fn backup_target_requires_existing_parent_and_file_target() {
        let dir = std::env::temp_dir();
        let dir_err = validate_backup_target(&dir.display().to_string()).unwrap_err();
        assert_eq!(dir_err.code, "invalid_backup_path");

        let missing_parent = dir
            .join("droidsmith-missing-backup-parent")
            .join("package.ab");
        let missing_err =
            validate_backup_target(&missing_parent.display().to_string()).unwrap_err();
        assert_eq!(missing_err.code, "invalid_backup_path");

        let valid = dir.join("package.ab");
        assert_eq!(
            validate_backup_target(&valid.display().to_string()).unwrap(),
            valid
        );
    }

    #[test]
    fn unsafe_transport_requires_explicit_acknowledgement() {
        assert_eq!(
            accepted_transport_override(DeviceTransportKind::Usb, false),
            Ok(None)
        );
        assert_eq!(
            accepted_transport_override(DeviceTransportKind::TlsWifi, false),
            Ok(None)
        );
        assert!(accepted_transport_override(DeviceTransportKind::LegacyTcp, false).is_err());
        assert_eq!(
            accepted_transport_override(DeviceTransportKind::LegacyTcp, true),
            Ok(Some(DeviceTransportKind::LegacyTcp))
        );
        assert!(accepted_transport_override(DeviceTransportKind::UnknownTcp, false).is_err());
    }
}
