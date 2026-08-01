//! Fleet run reports — the stable, re-readable record of a `droidsmith-cli run`.
//!
//! The CLI has always emitted per-device JSON, but the document was
//! write-only: it carried a serial and a per-action status and nothing that
//! could prove a later rerun was targeting the same inputs. That made
//! "continue where the interrupted batch stopped" impossible to do safely,
//! because the two facts a resume must establish — *is this the same work* and
//! *is this the same device* — were both absent.
//!
//! Schema v2 adds exactly those facts and nothing else:
//!
//! - a **profile fingerprint** over the whole document and a separate
//!   **action-set fingerprint** over just the ordered `(kind, package)` pairs,
//!   so an edited description is distinguishable from an edited action list;
//! - a per-device **hashed identity** (serial + verified build fingerprint),
//!   matching the redaction posture of recovery baselines — the report keeps
//!   the serial because a rerun has to address the device, but every identity
//!   comparison is made against the digest;
//! - the per-device **Android user** and **transport kind** that were actually
//!   resolved, so a resume can refuse a device that moved users or dropped from
//!   USB to an unauthenticated TCP transport;
//! - the **action kind** alongside each result, so a completed action can be
//!   recognised and excluded rather than replayed.
//!
//! Retry planning ([`plan_retry`]) is pure and total: it consumes a loaded
//! report plus the profile the operator passed and returns the devices to
//! retry, the actions already completed on each, and every reason a device was
//! excluded. It never touches a device — the CLI performs live validation
//! (identity/user/transport) at execution time against this plan.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::adb::actions::ActionKind;
use crate::adb::device::DeviceTransportKind;
use crate::device_identity::DeviceIdentity;
use crate::profile::Profile;

/// Current fleet-report schema. v1 (pre-fingerprint, pre-identity) is readable
/// only far enough to report that it cannot be resumed.
pub const FLEET_REPORT_SCHEMA_VERSION: u32 = 2;
/// The oldest schema this build can name in a migration message.
pub const LEGACY_FLEET_REPORT_SCHEMA_VERSION: u32 = 1;

const MAX_REPORT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FleetReportError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("fleet report {path} exceeds the {max} byte limit")]
    TooLarge { path: PathBuf, max: u64 },
    #[error("could not parse {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("fleet report {path} failed validation: {message}")]
    Validate { path: PathBuf, message: String },
}

impl FleetReportError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Read { .. } => "fleet_report_unreadable",
            Self::TooLarge { .. } => "fleet_report_too_large",
            Self::Parse { .. } => "fleet_report_invalid",
            Self::Validate { .. } => "fleet_report_unsupported",
        }
    }
}

/// One planned command, as rendered into the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunPlanOutput {
    pub index: usize,
    pub package: String,
    pub action: ActionKind,
    pub user_id: u32,
    pub before_state: String,
    pub description: String,
    pub adb_args: Vec<String>,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Applied,
    Failed,
    /// Deliberately not executed on this run. Only a resume produces this:
    /// the action already succeeded in the source report.
    Skipped,
}

/// One executed (or deliberately skipped) action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunApplyOutput {
    pub index: usize,
    pub package: String,
    /// Present since schema v2. v1 reports carry no kind, which is why they
    /// cannot be resumed: a bare package name cannot prove which action of a
    /// multi-action profile completed.
    pub action: ActionKind,
    pub status: ActionStatus,
    pub error: Option<String>,
}

/// One device's slot in a fleet run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOutput {
    pub schema_version: u32,
    pub command: String,
    pub mode: String,
    pub profile_name: String,
    pub profile_version: String,
    pub device_serial: String,
    /// SHA-256 of the canonical device identity (serial + verified build
    /// fingerprint). Raw serials stay in `device_serial` because a resume has
    /// to address the device; every *comparison* uses this digest.
    pub device_identity_sha256: String,
    pub transport_kind: DeviceTransportKind,
    pub android_user: u32,
    pub compatible: bool,
    pub plans: Vec<RunPlanOutput>,
    pub results: Vec<RunApplyOutput>,
    pub success: bool,
}

/// Per-device error prior to or during execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceErrorOutput {
    pub device_serial: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum DeviceRunResult {
    // Boxed: RunOutput is far larger than the other variants.
    Ran(Box<RunOutput>),
    Error(DeviceErrorOutput),
    Skipped {
        device_serial: String,
        reason: String,
    },
}

impl DeviceRunResult {
    pub fn device_serial(&self) -> &str {
        match self {
            Self::Ran(output) => &output.device_serial,
            Self::Error(error) => &error.device_serial,
            Self::Skipped { device_serial, .. } => device_serial,
        }
    }
}

/// The profile a report was produced from, reduced to what a resume must prove.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportProfile {
    pub name: String,
    pub version: String,
    /// Digest over the whole profile document.
    pub fingerprint_sha256: String,
    /// Digest over only the ordered `(kind, package)` pairs. Equal action sets
    /// with differing prose are a benign drift; a differing action set is not.
    pub action_set_sha256: String,
    pub action_count: usize,
}

/// Provenance of a resumed run, written into the report the resume produces.
///
/// The source file's own path is deliberately absent: it is host layout, it is
/// not needed to establish lineage, and reports are shared in support tickets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportLineage {
    /// SHA-256 of the source report's bytes exactly as they were read.
    pub source_sha256: String,
    pub source_generated_at: String,
    /// 1 for the first resume of an original run, incrementing thereafter.
    pub retry_generation: u32,
    /// Serials selected for this resume.
    pub retried_devices: Vec<String>,
    /// Devices the source report proved complete. They are absent from
    /// `devices` because this report's scope is what remained; recording them
    /// here keeps the chain auditable without making a further resume touch
    /// finished hardware.
    pub excluded_devices: Vec<RetryExclusion>,
    /// Drift the operator explicitly accepted with `--accept-drift`.
    pub accepted_drift: Vec<DriftItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetRunReport {
    pub schema_version: u32,
    pub command: String,
    pub mode: String,
    pub apply: bool,
    pub generated_at: String,
    pub profile: ReportProfile,
    #[serde(default)]
    pub lineage: Option<ReportLineage>,
    pub devices: Vec<DeviceRunResult>,
    pub success: bool,
}

/// Digest over the whole profile document.
pub fn profile_fingerprint(profile: &Profile) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"droidsmith-fleet-report-profile-v1\0");
    // `Profile` serializes field-by-field in declaration order, so this is
    // deterministic without a canonicalization pass.
    let encoded = serde_json::to_vec(profile).expect("profile is serializable");
    hasher.update(&encoded);
    format!("{:x}", hasher.finalize())
}

/// Digest over only the ordered action list.
pub fn action_set_fingerprint(profile: &Profile) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"droidsmith-fleet-report-action-set-v1\0");
    for action in &profile.actions {
        let encoded = serde_json::to_vec(&action.kind).expect("action kind is serializable");
        hasher.update(&encoded);
        hasher.update(b"\0");
        hasher.update(action.package.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

/// SHA-256 of a canonical device identity, domain-separated from every other
/// digest Droidsmith writes.
pub fn hashed_identity(identity: &DeviceIdentity) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"droidsmith-fleet-report-device-v1\0");
    hasher.update(identity.canonical().as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn describe_profile(profile: &Profile) -> ReportProfile {
    ReportProfile {
        name: profile.name.clone(),
        version: profile.version.clone(),
        fingerprint_sha256: profile_fingerprint(profile),
        action_set_sha256: action_set_fingerprint(profile),
        action_count: profile.actions.len(),
    }
}

/// A loaded report plus the digest of the exact bytes it was read from, so the
/// resume's lineage names a file content rather than a file path.
#[derive(Debug, Clone)]
pub struct LoadedReport {
    pub report: FleetRunReport,
    pub source_sha256: String,
}

pub fn load(path: &Path) -> Result<LoadedReport, FleetReportError> {
    let metadata = std::fs::metadata(path).map_err(|source| FleetReportError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_REPORT_BYTES {
        return Err(FleetReportError::TooLarge {
            path: path.to_path_buf(),
            max: MAX_REPORT_BYTES,
        });
    }
    let bytes = std::fs::read(path).map_err(|source| FleetReportError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse(path, &bytes)
}

/// Split out from [`load`] so validation is testable without a filesystem.
pub fn parse(path: &Path, bytes: &[u8]) -> Result<LoadedReport, FleetReportError> {
    #[derive(Deserialize)]
    struct VersionProbe {
        #[serde(default)]
        schema_version: u32,
        #[serde(default)]
        command: String,
        #[serde(default)]
        mode: String,
    }

    let probe: VersionProbe =
        serde_json::from_slice(bytes).map_err(|error| FleetReportError::Parse {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    if probe.command != "run" {
        return Err(FleetReportError::Validate {
            path: path.to_path_buf(),
            message: format!(
                "expected a `run` report, found {:?}; only `run --all-devices --json` output can be resumed",
                probe.command
            ),
        });
    }
    if probe.mode != "all_devices" {
        return Err(FleetReportError::Validate {
            path: path.to_path_buf(),
            message: format!(
                "expected mode `all_devices`, found {:?}; single-device runs are rerun with --device",
                probe.mode
            ),
        });
    }
    if probe.schema_version != FLEET_REPORT_SCHEMA_VERSION {
        let guidance = if probe.schema_version <= LEGACY_FLEET_REPORT_SCHEMA_VERSION {
            "v1 reports record neither the profile fingerprint nor per-action kinds, so a resume cannot prove which work completed; rerun the fleet from the profile"
        } else {
            "the report was written by a newer Droidsmith; upgrade this build to read it"
        };
        return Err(FleetReportError::Validate {
            path: path.to_path_buf(),
            message: format!(
                "unsupported fleet report schema {} (expected {FLEET_REPORT_SCHEMA_VERSION}): {guidance}",
                probe.schema_version
            ),
        });
    }
    let report: FleetRunReport =
        serde_json::from_slice(bytes).map_err(|error| FleetReportError::Parse {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    if report.devices.is_empty() {
        return Err(FleetReportError::Validate {
            path: path.to_path_buf(),
            message: "report contains no devices; there is nothing to resume".to_string(),
        });
    }
    let mut hasher = Sha256::new();
    hasher.update(b"droidsmith-fleet-report-source-v1\0");
    hasher.update(bytes);
    Ok(LoadedReport {
        report,
        source_sha256: format!("{:x}", hasher.finalize()),
    })
}

/// An action proven complete by the source report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedAction {
    pub package: String,
    pub kind: ActionKind,
}

/// Why a device was selected for the resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryReason {
    /// The source run errored before or during execution.
    Errored,
    /// The source run skipped the device entirely (offline, unauthorized,
    /// unauthenticated transport).
    SkippedBySource,
    /// The source run reached the device but one or more actions failed.
    PartiallyApplied,
    /// The source run was a dry-run, so nothing was applied anywhere.
    NeverApplied,
}

impl RetryReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Errored => "the source run errored on this device",
            Self::SkippedBySource => "the source run skipped this device",
            Self::PartiallyApplied => "the source run left actions failed on this device",
            Self::NeverApplied => "the source run was a dry-run, so nothing was applied",
        }
    }
}

/// One device the resume intends to touch, with everything the live run must
/// re-prove before it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryTarget {
    pub serial: String,
    /// `None` when the source never bound the device (it errored or was
    /// skipped), in which case there is no recorded identity to re-prove.
    pub identity_sha256: Option<String>,
    pub android_user: Option<u32>,
    pub transport_kind: Option<DeviceTransportKind>,
    pub reason: RetryReason,
    /// Actions the source report proves succeeded. Never replayed.
    pub completed: Vec<CompletedAction>,
}

/// A device present in the report that the resume deliberately leaves alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryExclusion {
    pub serial: String,
    pub reason: String,
}

/// A mismatch between the source report and the inputs of the resume.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftItem {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPlan {
    /// Drift affecting the whole resume (profile document / action set).
    pub drift: Vec<DriftItem>,
    pub targets: Vec<RetryTarget>,
    pub excluded: Vec<RetryExclusion>,
    /// Generation this resume will record; 1 for the first resume.
    pub retry_generation: u32,
}

impl RetryPlan {
    pub fn has_drift(&self) -> bool {
        !self.drift.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Read-only rendering
// ---------------------------------------------------------------------------
//
// A saved report is opened and rendered without touching a device. The view
// below is the entire IPC surface for that, and it exists to make one property
// structural rather than a convention the renderer has to remember: no raw
// serial reaches it. Every device is named by the digest the report already
// carries, or — for a device the run never bound — by a serial-only digest in
// the same domain, flagged so the UI can say which of the two it is showing.

/// How a device is named in a rendered report.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RedactedDevice {
    pub identity_sha256: String,
    /// `true` when the digest covers a verified build fingerprint as well as
    /// the serial. `false` means the run never bound the device (it errored or
    /// was skipped before the fingerprint probe), so the digest covers the
    /// serial alone and cannot distinguish two devices reporting the same one.
    pub fingerprint_bound: bool,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceOutcome {
    Ran,
    Error,
    Skipped,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FleetReportActionView {
    pub index: usize,
    pub package: String,
    pub action: ActionKind,
    pub user_id: u32,
    pub before_state: String,
    pub description: String,
    /// `None` in a dry-run report, where the action was planned but nothing
    /// was executed.
    pub status: Option<ActionStatus>,
    pub error: Option<String>,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FleetReportDeviceView {
    pub device: RedactedDevice,
    pub outcome: DeviceOutcome,
    pub transport_kind: Option<DeviceTransportKind>,
    pub android_user: Option<u32>,
    /// `None` for devices that never ran.
    pub success: Option<bool>,
    /// Stable error code for an `error` outcome.
    pub failure_code: Option<String>,
    /// The error message or the skip cause, whichever applies.
    pub failure_reason: Option<String>,
    pub actions: Vec<FleetReportActionView>,
}

#[derive(specta::Type, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FleetReportTotals {
    pub devices: usize,
    pub ran: usize,
    pub errored: usize,
    pub skipped: usize,
    pub actions_planned: usize,
    pub actions_applied: usize,
    pub actions_failed: usize,
    pub actions_skipped: usize,
}

/// [`ReportLineage`] with its serials replaced by digests.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RedactedLineage {
    pub source_sha256: String,
    pub source_generated_at: String,
    pub retry_generation: u32,
    pub retried_devices: Vec<RedactedDevice>,
    pub excluded_devices: Vec<RedactedExclusion>,
    pub accepted_drift: Vec<DriftItem>,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RedactedExclusion {
    pub device: RedactedDevice,
    pub reason: String,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FleetReportView {
    pub schema_version: u32,
    pub generated_at: String,
    /// `true` when the report records an apply; `false` for a dry-run, where
    /// no action was executed and every status is absent.
    pub apply: bool,
    pub profile: ReportProfile,
    pub lineage: Option<RedactedLineage>,
    pub totals: FleetReportTotals,
    pub devices: Vec<FleetReportDeviceView>,
    pub success: bool,
}

/// Name a device that the run never bound, using the serial alone.
fn redact_serial(serial: &str) -> RedactedDevice {
    RedactedDevice {
        identity_sha256: hashed_identity(&DeviceIdentity::legacy_serial_only(serial)),
        fingerprint_bound: false,
    }
}

/// Build the read-only render of a loaded report. Pure: no device access, no
/// network, no clock.
pub fn view(report: &FleetRunReport) -> FleetReportView {
    let mut totals = FleetReportTotals {
        devices: report.devices.len(),
        ..FleetReportTotals::default()
    };
    let devices = report
        .devices
        .iter()
        .map(|device| match device {
            DeviceRunResult::Skipped {
                device_serial,
                reason,
            } => {
                totals.skipped += 1;
                FleetReportDeviceView {
                    device: redact_serial(device_serial),
                    outcome: DeviceOutcome::Skipped,
                    transport_kind: None,
                    android_user: None,
                    success: None,
                    failure_code: None,
                    failure_reason: Some(reason.clone()),
                    actions: Vec::new(),
                }
            }
            DeviceRunResult::Error(error) => {
                totals.errored += 1;
                FleetReportDeviceView {
                    device: redact_serial(&error.device_serial),
                    outcome: DeviceOutcome::Error,
                    transport_kind: None,
                    android_user: None,
                    success: None,
                    failure_code: Some(error.code.clone()),
                    failure_reason: Some(error.message.clone()),
                    actions: Vec::new(),
                }
            }
            DeviceRunResult::Ran(output) => {
                totals.ran += 1;
                let actions = output
                    .plans
                    .iter()
                    .map(|plan| {
                        // Results are keyed by the plan index the same run
                        // produced, so a dry-run (no results) and an interrupted
                        // apply (fewer results than plans) both render without
                        // inventing a status.
                        let result = output
                            .results
                            .iter()
                            .find(|result| result.index == plan.index);
                        totals.actions_planned += 1;
                        match result.map(|result| result.status) {
                            Some(ActionStatus::Applied) => totals.actions_applied += 1,
                            Some(ActionStatus::Failed) => totals.actions_failed += 1,
                            Some(ActionStatus::Skipped) => totals.actions_skipped += 1,
                            None => {}
                        }
                        FleetReportActionView {
                            index: plan.index,
                            package: plan.package.clone(),
                            action: plan.action,
                            user_id: plan.user_id,
                            before_state: plan.before_state.clone(),
                            description: plan.description.clone(),
                            status: result.map(|result| result.status),
                            error: result.and_then(|result| result.error.clone()),
                        }
                    })
                    .collect();
                FleetReportDeviceView {
                    device: RedactedDevice {
                        identity_sha256: output.device_identity_sha256.clone(),
                        fingerprint_bound: true,
                    },
                    outcome: DeviceOutcome::Ran,
                    transport_kind: Some(output.transport_kind),
                    android_user: Some(output.android_user),
                    success: Some(output.success),
                    failure_code: None,
                    failure_reason: None,
                    actions,
                }
            }
        })
        .collect();

    FleetReportView {
        schema_version: report.schema_version,
        generated_at: report.generated_at.clone(),
        apply: report.apply,
        profile: report.profile.clone(),
        lineage: report.lineage.as_ref().map(|lineage| RedactedLineage {
            source_sha256: lineage.source_sha256.clone(),
            source_generated_at: lineage.source_generated_at.clone(),
            retry_generation: lineage.retry_generation,
            retried_devices: lineage
                .retried_devices
                .iter()
                .map(|serial| redact_serial(serial))
                .collect(),
            excluded_devices: lineage
                .excluded_devices
                .iter()
                .map(|excluded| RedactedExclusion {
                    device: redact_serial(&excluded.serial),
                    reason: excluded.reason.clone(),
                })
                .collect(),
            accepted_drift: lineage.accepted_drift.clone(),
        }),
        totals,
        devices,
        success: report.success,
    }
}

/// Build the resume plan. Pure: nothing here reads a device or a clock.
///
/// A device is selected when the source did not finish it successfully. A
/// device the source completed is excluded rather than silently re-run, and
/// every individual action the source proved applied is carried in
/// [`RetryTarget::completed`] so the executor can exclude it too.
pub fn plan_retry(loaded: &LoadedReport, profile: &Profile) -> RetryPlan {
    let report = &loaded.report;
    let mut drift = Vec::new();
    let current = describe_profile(profile);

    if current.action_set_sha256 != report.profile.action_set_sha256 {
        drift.push(DriftItem {
            code: "action_set_changed".to_string(),
            message: format!(
                "the profile's action set changed since the report was written ({} actions now, {} then); resumed work would not be the work that was interrupted",
                current.action_count, report.profile.action_count
            ),
        });
    } else if current.fingerprint_sha256 != report.profile.fingerprint_sha256 {
        // Same commands, different document: notes, description, or device
        // match constraints moved. Worth naming, not the same class of risk.
        drift.push(DriftItem {
            code: "profile_document_changed".to_string(),
            message:
                "the profile document changed since the report was written, but its action set is identical"
                    .to_string(),
        });
    }
    if current.name != report.profile.name || current.version != report.profile.version {
        drift.push(DriftItem {
            code: "profile_identity_changed".to_string(),
            message: format!(
                "the report was produced by profile {:?} v{}, not {:?} v{}",
                report.profile.name, report.profile.version, current.name, current.version
            ),
        });
    }

    let mut targets = Vec::new();
    let mut excluded = Vec::new();
    for device in &report.devices {
        match device {
            // The source's skip reason is deliberately not carried forward: a
            // resume re-derives eligibility from the live fleet, and a stale
            // "unauthorized" would misreport a device that has since been
            // authorized.
            DeviceRunResult::Skipped { device_serial, .. } => targets.push(RetryTarget {
                serial: device_serial.clone(),
                identity_sha256: None,
                android_user: None,
                transport_kind: None,
                reason: RetryReason::SkippedBySource,
                completed: Vec::new(),
            }),
            DeviceRunResult::Error(error) => targets.push(RetryTarget {
                serial: error.device_serial.clone(),
                identity_sha256: None,
                android_user: None,
                transport_kind: None,
                reason: RetryReason::Errored,
                completed: Vec::new(),
            }),
            DeviceRunResult::Ran(output) => {
                let completed: Vec<CompletedAction> = output
                    .results
                    .iter()
                    .filter(|result| result.status == ActionStatus::Applied)
                    .map(|result| CompletedAction {
                        package: result.package.clone(),
                        kind: result.action,
                    })
                    .collect();
                if output.success && report.apply {
                    excluded.push(RetryExclusion {
                        serial: output.device_serial.clone(),
                        reason: format!(
                            "every action already applied ({} of {})",
                            completed.len(),
                            output.plans.len()
                        ),
                    });
                    continue;
                }
                targets.push(RetryTarget {
                    serial: output.device_serial.clone(),
                    identity_sha256: Some(output.device_identity_sha256.clone()),
                    android_user: Some(output.android_user),
                    transport_kind: Some(output.transport_kind),
                    reason: if report.apply {
                        RetryReason::PartiallyApplied
                    } else {
                        RetryReason::NeverApplied
                    },
                    completed,
                });
            }
        }
    }

    RetryPlan {
        drift,
        targets,
        excluded,
        retry_generation: report
            .lineage
            .as_ref()
            .map(|lineage| lineage.retry_generation.saturating_add(1))
            .unwrap_or(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{ProfileAction, ProfileDeviceMatch, ProfileUserTarget};

    fn profile(actions: &[(ActionKind, &str)]) -> Profile {
        Profile {
            name: "fleet".to_string(),
            version: "2".to_string(),
            description: String::new(),
            device: ProfileDeviceMatch::default(),
            user: ProfileUserTarget::default(),
            actions: actions
                .iter()
                .map(|(kind, package)| ProfileAction {
                    kind: *kind,
                    package: (*package).to_string(),
                    note: String::new(),
                })
                .collect(),
        }
    }

    fn ran(
        serial: &str,
        identity: &str,
        user: u32,
        results: Vec<(&str, ActionKind, ActionStatus)>,
        success: bool,
    ) -> DeviceRunResult {
        DeviceRunResult::Ran(Box::new(RunOutput {
            schema_version: FLEET_REPORT_SCHEMA_VERSION,
            command: "run".to_string(),
            mode: "apply".to_string(),
            profile_name: "fleet".to_string(),
            profile_version: "2".to_string(),
            device_serial: serial.to_string(),
            device_identity_sha256: identity.to_string(),
            transport_kind: DeviceTransportKind::Usb,
            android_user: user,
            compatible: true,
            plans: results
                .iter()
                .enumerate()
                .map(|(index, (package, kind, _))| RunPlanOutput {
                    index: index + 1,
                    package: (*package).to_string(),
                    action: *kind,
                    user_id: user,
                    before_state: "installed".to_string(),
                    description: String::new(),
                    adb_args: Vec::new(),
                })
                .collect(),
            results: results
                .into_iter()
                .enumerate()
                .map(|(index, (package, kind, status))| RunApplyOutput {
                    index: index + 1,
                    package: package.to_string(),
                    action: kind,
                    status,
                    error: None,
                })
                .collect(),
            success,
        }))
    }

    fn report(apply: bool, devices: Vec<DeviceRunResult>, source: &Profile) -> LoadedReport {
        LoadedReport {
            report: FleetRunReport {
                schema_version: FLEET_REPORT_SCHEMA_VERSION,
                command: "run".to_string(),
                mode: "all_devices".to_string(),
                apply,
                generated_at: "2026-08-01T00:00:00Z".to_string(),
                profile: describe_profile(source),
                lineage: None,
                devices,
                success: false,
            },
            source_sha256: "0".repeat(64),
        }
    }

    #[test]
    fn only_unfinished_devices_are_retried_and_applied_actions_are_carried_forward() {
        let source = profile(&[
            (ActionKind::Disable, "com.a"),
            (ActionKind::Disable, "com.b"),
        ]);
        let loaded = report(
            true,
            vec![
                ran(
                    "DONE",
                    "a".repeat(64).as_str(),
                    0,
                    vec![
                        ("com.a", ActionKind::Disable, ActionStatus::Applied),
                        ("com.b", ActionKind::Disable, ActionStatus::Applied),
                    ],
                    true,
                ),
                ran(
                    "PARTIAL",
                    "b".repeat(64).as_str(),
                    0,
                    vec![
                        ("com.a", ActionKind::Disable, ActionStatus::Applied),
                        ("com.b", ActionKind::Disable, ActionStatus::Failed),
                    ],
                    false,
                ),
                DeviceRunResult::Skipped {
                    device_serial: "OFFLINE".to_string(),
                    reason: "device is not actionable".to_string(),
                },
                DeviceRunResult::Error(DeviceErrorOutput {
                    device_serial: "BROKE".to_string(),
                    code: "device_probe_failed".to_string(),
                    message: "boom".to_string(),
                }),
            ],
            &source,
        );

        let plan = plan_retry(&loaded, &source);
        assert!(!plan.has_drift(), "identical profile must not drift");
        assert_eq!(plan.retry_generation, 1);
        let serials: Vec<&str> = plan
            .targets
            .iter()
            .map(|target| target.serial.as_str())
            .collect();
        assert_eq!(serials, vec!["PARTIAL", "OFFLINE", "BROKE"]);
        assert_eq!(plan.excluded.len(), 1);
        assert_eq!(plan.excluded[0].serial, "DONE");

        let partial = &plan.targets[0];
        assert_eq!(partial.reason, RetryReason::PartiallyApplied);
        assert_eq!(
            partial.completed,
            vec![CompletedAction {
                package: "com.a".to_string(),
                kind: ActionKind::Disable,
            }]
        );
        assert_eq!(partial.android_user, Some(0));
        assert_eq!(partial.transport_kind, Some(DeviceTransportKind::Usb));

        // Devices the source never bound carry no identity to re-prove.
        assert_eq!(plan.targets[1].reason, RetryReason::SkippedBySource);
        assert!(plan.targets[1].identity_sha256.is_none());
        assert_eq!(plan.targets[2].reason, RetryReason::Errored);
    }

    #[test]
    fn a_successful_dry_run_still_leaves_every_device_to_apply() {
        // A dry-run report proves nothing was changed, so "success" there must
        // not be read as "already done".
        let source = profile(&[(ActionKind::Disable, "com.a")]);
        let loaded = report(
            false,
            vec![ran("USB1", "c".repeat(64).as_str(), 0, Vec::new(), true)],
            &source,
        );
        let plan = plan_retry(&loaded, &source);
        assert!(plan.excluded.is_empty());
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].reason, RetryReason::NeverApplied);
        assert!(plan.targets[0].completed.is_empty());
    }

    #[test]
    fn a_changed_action_set_drifts_but_reordered_prose_is_a_softer_drift() {
        let source = profile(&[(ActionKind::Disable, "com.a")]);
        let loaded = report(
            true,
            vec![ran(
                "USB1",
                "d".repeat(64).as_str(),
                0,
                vec![("com.a", ActionKind::Disable, ActionStatus::Failed)],
                false,
            )],
            &source,
        );

        let mut edited_note = source.clone();
        edited_note.actions[0].note = "reviewed".to_string();
        let soft = plan_retry(&loaded, &edited_note);
        assert_eq!(soft.drift.len(), 1);
        assert_eq!(soft.drift[0].code, "profile_document_changed");

        let changed = profile(&[
            (ActionKind::Disable, "com.a"),
            (ActionKind::Disable, "com.c"),
        ]);
        let hard = plan_retry(&loaded, &changed);
        assert!(hard
            .drift
            .iter()
            .any(|item| item.code == "action_set_changed"));

        let renamed = {
            let mut renamed = source.clone();
            renamed.name = "other".to_string();
            renamed
        };
        let identity = plan_retry(&loaded, &renamed);
        assert!(identity
            .drift
            .iter()
            .any(|item| item.code == "profile_identity_changed"));
    }

    #[test]
    fn the_action_set_digest_ignores_prose_but_not_order() {
        let a = profile(&[
            (ActionKind::Disable, "com.a"),
            (ActionKind::Disable, "com.b"),
        ]);
        let mut noted = a.clone();
        noted.description = "notes".to_string();
        noted.actions[1].note = "why".to_string();
        assert_eq!(action_set_fingerprint(&a), action_set_fingerprint(&noted));
        assert_ne!(profile_fingerprint(&a), profile_fingerprint(&noted));

        let reordered = profile(&[
            (ActionKind::Disable, "com.b"),
            (ActionKind::Disable, "com.a"),
        ]);
        assert_ne!(
            action_set_fingerprint(&a),
            action_set_fingerprint(&reordered),
            "order is part of the contract; profiles apply in sequence"
        );
    }

    #[test]
    fn a_package_boundary_cannot_be_smuggled_into_the_action_digest() {
        // Without the explicit separators, ("com.ab", "c") and ("com.a", "bc")
        // would hash identically.
        let left = profile(&[(ActionKind::Disable, "com.ab"), (ActionKind::Disable, "c")]);
        let right = profile(&[(ActionKind::Disable, "com.a"), (ActionKind::Disable, "bc")]);
        assert_ne!(
            action_set_fingerprint(&left),
            action_set_fingerprint(&right)
        );
    }

    #[test]
    fn lineage_generations_increment_across_repeated_resumes() {
        let source = profile(&[(ActionKind::Disable, "com.a")]);
        let mut loaded = report(
            true,
            vec![ran(
                "USB1",
                "e".repeat(64).as_str(),
                0,
                vec![("com.a", ActionKind::Disable, ActionStatus::Failed)],
                false,
            )],
            &source,
        );
        loaded.report.lineage = Some(ReportLineage {
            source_sha256: "f".repeat(64),
            source_generated_at: "2026-07-31T00:00:00Z".to_string(),
            retry_generation: 2,
            retried_devices: vec!["USB1".to_string()],
            excluded_devices: Vec::new(),
            accepted_drift: Vec::new(),
        });
        assert_eq!(plan_retry(&loaded, &source).retry_generation, 3);
    }

    #[test]
    fn hashed_identity_separates_devices_and_is_a_full_digest() {
        let first = hashed_identity(&DeviceIdentity::new("A", Some("brand/x:16/X/1:user")));
        let second = hashed_identity(&DeviceIdentity::new("A", Some("brand/y:16/Y/2:user")));
        assert_ne!(first, second);
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        // Domain separation: the same canonical string under the recovery
        // baseline domain must not collide with this one.
        assert_ne!(
            first,
            crate::recovery_baseline::hashed_device_identity(
                &DeviceIdentity::new("A", Some("brand/x:16/X/1:user")).canonical()
            )
        );
    }

    #[test]
    fn v1_and_foreign_reports_are_refused_with_migration_guidance() {
        let path = Path::new("report.json");
        let v1 = br#"{"schema_version":1,"command":"run","mode":"all_devices","apply":true,"devices":[],"success":false}"#;
        let error = parse(path, v1).unwrap_err();
        assert_eq!(error.code(), "fleet_report_unsupported");
        assert!(error.to_string().contains("rerun the fleet"), "{error}");

        let future = br#"{"schema_version":99,"command":"run","mode":"all_devices","apply":true,"devices":[],"success":false}"#;
        assert!(parse(path, future)
            .unwrap_err()
            .to_string()
            .contains("upgrade this build"));

        let baseline = br#"{"schema_version":2,"command":"baseline-export","mode":"all_devices","devices":[]}"#;
        assert!(parse(path, baseline)
            .unwrap_err()
            .to_string()
            .contains("only `run"));

        let single = br#"{"schema_version":2,"command":"run","mode":"dry_run","devices":[]}"#;
        assert!(parse(path, single)
            .unwrap_err()
            .to_string()
            .contains("--device"));
    }

    #[test]
    fn a_report_round_trips_through_json_with_a_stable_source_digest() {
        let source = profile(&[(ActionKind::Disable, "com.a")]);
        let loaded = report(
            true,
            vec![ran(
                "USB1",
                "a".repeat(64).as_str(),
                0,
                vec![("com.a", ActionKind::Disable, ActionStatus::Failed)],
                false,
            )],
            &source,
        );
        let bytes = serde_json::to_vec(&loaded.report).expect("report serializes");
        let reparsed = parse(Path::new("r.json"), &bytes).expect("report round-trips");
        assert_eq!(reparsed.report, loaded.report);
        // The digest covers the bytes, so re-encoding the same document twice
        // yields the same lineage anchor.
        let again = parse(Path::new("r.json"), &bytes).expect("stable");
        assert_eq!(again.source_sha256, reparsed.source_sha256);
        assert_ne!(reparsed.source_sha256, loaded.source_sha256);
    }

    #[test]
    fn the_rendered_view_never_carries_a_raw_serial() {
        // The redaction is the point of the render path: a report is a support
        // artifact, and a serial identifies a specific handset.
        let source = profile(&[(ActionKind::Disable, "com.a")]);
        let mut loaded = report(
            true,
            vec![
                ran(
                    "R5CT60ZQR4M",
                    "a".repeat(64).as_str(),
                    0,
                    vec![("com.a", ActionKind::Disable, ActionStatus::Applied)],
                    true,
                ),
                DeviceRunResult::Error(DeviceErrorOutput {
                    device_serial: "SECRETSERIAL2".to_string(),
                    code: "device_probe_failed".to_string(),
                    message: "boom".to_string(),
                }),
                DeviceRunResult::Skipped {
                    device_serial: "192.168.1.5:5555".to_string(),
                    reason: "unauthenticated transport".to_string(),
                },
            ],
            &source,
        );
        loaded.report.lineage = Some(ReportLineage {
            source_sha256: "b".repeat(64),
            source_generated_at: "2026-07-31T00:00:00Z".to_string(),
            retry_generation: 1,
            retried_devices: vec!["SECRETSERIAL2".to_string()],
            excluded_devices: vec![RetryExclusion {
                serial: "R5CT60ZQR4M".to_string(),
                reason: "already applied".to_string(),
            }],
            accepted_drift: Vec::new(),
        });

        let rendered = view(&loaded.report);
        let json = serde_json::to_string(&rendered).expect("view serializes");
        for serial in ["R5CT60ZQR4M", "SECRETSERIAL2", "192.168.1.5:5555"] {
            assert!(
                !json.contains(serial),
                "raw serial {serial} leaked into the rendered view: {json}"
            );
        }
        assert_eq!(rendered.devices.len(), 3);
        assert!(rendered.devices[0].device.fingerprint_bound);
        // A device the run never bound is named by the serial alone, and says so.
        assert!(!rendered.devices[1].device.fingerprint_bound);
        assert!(!rendered.devices[2].device.fingerprint_bound);
        let lineage = rendered.lineage.expect("lineage renders");
        assert_eq!(lineage.retried_devices.len(), 1);
        assert_eq!(lineage.excluded_devices[0].reason, "already applied");
    }

    #[test]
    fn the_rendered_view_totals_and_joins_actions_to_their_plans() {
        let source = profile(&[
            (ActionKind::Disable, "com.a"),
            (ActionKind::Disable, "com.b"),
        ]);
        let loaded = report(
            true,
            vec![
                ran(
                    "USB1",
                    "a".repeat(64).as_str(),
                    0,
                    vec![
                        ("com.a", ActionKind::Disable, ActionStatus::Applied),
                        ("com.b", ActionKind::Disable, ActionStatus::Failed),
                    ],
                    false,
                ),
                DeviceRunResult::Skipped {
                    device_serial: "OFFLINE".to_string(),
                    reason: "device is not actionable".to_string(),
                },
            ],
            &source,
        );
        let rendered = view(&loaded.report);
        assert_eq!(rendered.totals.devices, 2);
        assert_eq!(rendered.totals.ran, 1);
        assert_eq!(rendered.totals.skipped, 1);
        assert_eq!(rendered.totals.errored, 0);
        assert_eq!(rendered.totals.actions_planned, 2);
        assert_eq!(rendered.totals.actions_applied, 1);
        assert_eq!(rendered.totals.actions_failed, 1);
        let device = &rendered.devices[0];
        assert_eq!(device.outcome, DeviceOutcome::Ran);
        assert_eq!(device.android_user, Some(0));
        assert_eq!(device.actions[1].status, Some(ActionStatus::Failed));
        assert_eq!(device.actions[1].package, "com.b");
        assert_eq!(
            rendered.devices[1].failure_reason.as_deref(),
            Some("device is not actionable")
        );
    }

    #[test]
    fn a_dry_run_renders_plans_with_no_status_rather_than_a_guessed_one() {
        // The absence of a result is information: nothing was executed. It must
        // not render as "applied" or as "failed".
        let source = profile(&[(ActionKind::Disable, "com.a")]);
        let mut loaded = report(
            false,
            vec![ran(
                "USB1",
                "a".repeat(64).as_str(),
                0,
                vec![("com.a", ActionKind::Disable, ActionStatus::Applied)],
                true,
            )],
            &source,
        );
        if let DeviceRunResult::Ran(output) = &mut loaded.report.devices[0] {
            output.results.clear();
            output.mode = "dry_run".to_string();
        }
        let rendered = view(&loaded.report);
        assert!(!rendered.apply);
        assert_eq!(rendered.totals.actions_planned, 1);
        assert_eq!(rendered.totals.actions_applied, 0);
        assert_eq!(rendered.devices[0].actions[0].status, None);
    }

    #[test]
    fn an_empty_device_list_is_not_resumable() {
        let source = profile(&[(ActionKind::Disable, "com.a")]);
        let mut empty = report(true, Vec::new(), &source);
        empty.report.devices.clear();
        let bytes = serde_json::to_vec(&empty.report).expect("serializes");
        assert!(parse(Path::new("r.json"), &bytes)
            .unwrap_err()
            .to_string()
            .contains("nothing to resume"));
    }
}
