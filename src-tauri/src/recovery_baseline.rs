//! Portable, redacted pre-change package baselines.
//!
//! Baselines deliberately carry no raw device serial, package UID, installer,
//! or APK path. Import is a pure compatibility/diff operation; callers must
//! present the returned canonical plans for a separate reviewed apply.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::adb::actions::{
    self, ActionContext, ActionKind, ActionRequest, ConfirmationSource, PlannedAction,
};
use crate::adb::packages::{valid_package_name, AppPackage};
use crate::adb::{AndroidUser, DeviceTarget};
use crate::fs_util::{ArtifactError, ArtifactKind, HostArtifact, StagedArtifact};

pub const RECOVERY_BASELINE_FORMAT: &str = "droidsmith_recovery_baseline";
pub const RECOVERY_BASELINE_SCHEMA_VERSION: u32 = 1;
pub const MAX_RECOVERY_BASELINE_BYTES: u64 = 1024 * 1024;
pub const MAX_RECOVERY_BASELINE_PACKAGES: usize = 2_048;

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryBaseline {
    pub format: String,
    pub schema_version: u32,
    pub exported_at: String,
    pub device: BaselineDevice,
    pub android_user: u32,
    #[serde(default)]
    pub pack: Option<BaselinePack>,
    pub packages: Vec<BaselinePackage>,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineDevice {
    pub identity_sha256: String,
    pub build_fingerprint: String,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselinePack {
    pub id: String,
    pub revision: u32,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineActionInput {
    pub package: String,
    pub kind: ActionKind,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselinePackage {
    pub package: String,
    pub present: bool,
    pub enabled: Option<bool>,
    pub system: Option<bool>,
    pub requested_action: ActionKind,
    pub undo_plan: Option<BaselineUndoPlan>,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineUndoPlan {
    pub kind: ActionKind,
    pub user_id: u32,
}

/// Which half of the OTA round trip a diff is planning.
///
/// The community workflow around a debloated phone is "restore everything,
/// take the update, re-debloat", and the two halves are not symmetric: one
/// walks the device back to the state the baseline recorded, the other walks
/// it forward to the state the recorded actions produced. Inferring the
/// direction from live state — which is what a single undirected diff has to
/// do — gets it wrong exactly when it matters, because immediately after an
/// update a reverted package and a never-changed package look identical.
///
/// Choosing the direction makes the plan reviewable: a pre-OTA restore never
/// contains a re-debloat action, and a post-OTA re-apply never contains an
/// action that undoes one.
#[derive(specta::Type, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineRoundTrip {
    /// Pre-OTA: return every recoverable package to its baseline state so the
    /// update runs against the state the device shipped with.
    #[default]
    Restore,
    /// Post-OTA: re-apply the recorded actions to packages the update
    /// reverted, and only to those.
    Reapply,
}

impl BaselineRoundTrip {
    pub fn label(self) -> &'static str {
        match self {
            Self::Restore => "restore to baseline",
            Self::Reapply => "re-apply recorded actions",
        }
    }
}

/// A package the portable baseline cannot act on in either direction, named
/// explicitly rather than left as one skipped row among many.
///
/// The baseline records enable-state only, by design: it must survive the OTA
/// that changes the build fingerprint, so it deliberately carries no APK, no
/// data, and no installer provenance. An action that changed anything else is
/// therefore outside what it can promise, at both ends of the round trip.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BaselineIrreversible {
    pub package: String,
    pub requested_action: ActionKind,
    pub reason: String,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct RecoveryBaselineDiff {
    pub baseline: RecoveryBaseline,
    pub compatibility: BaselineCompatibility,
    pub round_trip: BaselineRoundTrip,
    pub rows: Vec<BaselineDiffRow>,
    pub plans: Vec<PlannedAction>,
    /// Packages neither direction can act on. Populated identically for both
    /// directions so the operator sees the same list before and after the
    /// update.
    pub irreversible: Vec<BaselineIrreversible>,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BaselineCompatibility {
    pub device_identity_matches: bool,
    pub build_fingerprint_matches: bool,
    pub android_user_available: bool,
    pub current_device_identity_sha256: String,
    pub current_build_fingerprint: String,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineDiffStatus {
    Ready,
    AlreadyMatches,
    Drifted,
    Skipped,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BaselineDiffRow {
    pub package: String,
    pub baseline_present: bool,
    pub baseline_enabled: Option<bool>,
    pub live_present: bool,
    pub live_enabled: Option<bool>,
    pub requested_action: ActionKind,
    pub status: BaselineDiffStatus,
    pub reason_code: Option<&'static str>,
    pub reason: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RecoveryBaselineError {
    #[error("could not read recovery baseline: {0}")]
    Read(#[source] std::io::Error),
    #[error("could not parse recovery baseline JSON: {0}")]
    Parse(#[source] serde_json::Error),
    #[error("could not write recovery baseline: {0}")]
    Write(#[source] std::io::Error),
    #[error("recovery baseline failed validation: {0}")]
    Validate(String),
    #[error("could not encode recovery baseline: {0}")]
    Encode(#[source] serde_json::Error),
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
}

impl RecoveryBaselineError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Read(_) => "recovery_baseline_read_failed",
            Self::Parse(_) => "recovery_baseline_parse_failed",
            Self::Write(_) => "recovery_baseline_write_failed",
            Self::Validate(_) => "recovery_baseline_invalid",
            Self::Encode(_) => "recovery_baseline_encode_failed",
            Self::Artifact(error) => error.code(),
        }
    }
}

/// Baseline ownership is keyed on the serial alone, deliberately.
///
/// Everything else Droidsmith persists per device mixes the build fingerprint
/// into its identity so duplicate serials cannot share a store (see
/// [`crate::device_identity`]). A recovery baseline is the one store that must
/// *not*: its whole purpose is to survive the OTA that changes the
/// fingerprint, and mixing it in would make every updated device disown its
/// own baseline.
///
/// The fingerprint is therefore reported as a separate compatibility axis
/// (`build_fingerprint_matches`) rather than folded into identity. That leaves
/// one case unresolved: a second device reporting the same serial on a
/// different build is indistinguishable from the same device after an update.
/// Both surface as "identity matches, build changed", which is the strongest
/// claim the available evidence supports.
pub fn hashed_device_identity(serial: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"droidsmith-recovery-baseline-device-v1\0");
    hasher.update(serial.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn build(
    target: &DeviceTarget,
    android_user: u32,
    pack: Option<BaselinePack>,
    inventory: &[AppPackage],
    requested: Vec<BaselineActionInput>,
    exported_at: String,
) -> Result<RecoveryBaseline, RecoveryBaselineError> {
    if requested.is_empty() {
        return Err(RecoveryBaselineError::Validate(
            "at least one requested action is required".to_string(),
        ));
    }
    if requested.len() > MAX_RECOVERY_BASELINE_PACKAGES {
        return Err(RecoveryBaselineError::Validate(format!(
            "requested action count exceeds {MAX_RECOVERY_BASELINE_PACKAGES}"
        )));
    }
    let fingerprint = target
        .build_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            RecoveryBaselineError::Validate(
                "live target has no verified build fingerprint".to_string(),
            )
        })?;
    validate_text(fingerprint, "build fingerprint", 4_096)?;
    if exported_at.trim().is_empty() || exported_at.len() > 64 {
        return Err(RecoveryBaselineError::Validate(
            "export timestamp is missing or too long".to_string(),
        ));
    }
    if let Some(pack) = &pack {
        validate_text(&pack.id, "pack id", 255)?;
    }

    let inventory: HashMap<&str, &AppPackage> = inventory
        .iter()
        .map(|package| (package.package.as_str(), package))
        .collect();
    let mut seen = HashSet::new();
    let mut packages = Vec::with_capacity(requested.len());
    for action in requested {
        if !valid_package_name(&action.package) {
            return Err(RecoveryBaselineError::Validate(format!(
                "invalid package name {:?}",
                action.package
            )));
        }
        if !seen.insert(action.package.clone()) {
            return Err(RecoveryBaselineError::Validate(format!(
                "duplicate requested package {:?}",
                action.package
            )));
        }
        if !is_package_action(action.kind) {
            return Err(RecoveryBaselineError::Validate(format!(
                "unsupported recovery-baseline action {:?}",
                action.kind
            )));
        }
        let current = inventory.get(action.package.as_str()).copied();
        let undo_plan = current.and_then(|package| match (action.kind, package.enabled) {
            (ActionKind::Disable, true) => Some(BaselineUndoPlan {
                kind: ActionKind::Enable,
                user_id: android_user,
            }),
            (ActionKind::Enable, false) => Some(BaselineUndoPlan {
                kind: ActionKind::Disable,
                user_id: android_user,
            }),
            _ => None,
        });
        packages.push(BaselinePackage {
            package: action.package,
            present: current.is_some(),
            enabled: current.map(|package| package.enabled),
            system: current.map(|package| package.system),
            requested_action: action.kind,
            undo_plan,
        });
    }
    packages.sort_by(|left, right| left.package.cmp(&right.package));

    let baseline = RecoveryBaseline {
        format: RECOVERY_BASELINE_FORMAT.to_string(),
        schema_version: RECOVERY_BASELINE_SCHEMA_VERSION,
        exported_at,
        device: BaselineDevice {
            identity_sha256: hashed_device_identity(&target.serial),
            build_fingerprint: fingerprint.to_string(),
        },
        android_user,
        pack,
        packages,
    };
    validate(&baseline)?;
    Ok(baseline)
}

pub fn save(
    path: &Path,
    baseline: &RecoveryBaseline,
) -> Result<HostArtifact, RecoveryBaselineError> {
    validate(baseline)?;
    let staged = StagedArtifact::new(path)?;
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(staged.path())
            .map_err(RecoveryBaselineError::Write)?;
        serde_json::to_writer_pretty(&mut file, baseline).map_err(RecoveryBaselineError::Encode)?;
        file.write_all(b"\n")
            .map_err(RecoveryBaselineError::Write)?;
    }
    Ok(staged.commit(ArtifactKind::AnyFile)?)
}

pub fn load(path: &Path) -> Result<RecoveryBaseline, RecoveryBaselineError> {
    let text = crate::fs_util::read_to_string_limited(path, MAX_RECOVERY_BASELINE_BYTES)
        .map_err(RecoveryBaselineError::Read)?;
    let baseline = serde_json::from_str(&text).map_err(RecoveryBaselineError::Parse)?;
    validate(&baseline)?;
    Ok(baseline)
}

/// Diff a baseline against a live device for the pre-OTA restore direction.
///
/// Kept as the default entry point because restoring is what the recovery
/// surface has always done; the post-OTA half is reached through
/// [`inspect_round_trip`].
pub fn inspect(
    baseline: RecoveryBaseline,
    target: &DeviceTarget,
    users: &[AndroidUser],
    live_packages: &[AppPackage],
) -> Result<RecoveryBaselineDiff, RecoveryBaselineError> {
    inspect_round_trip(
        baseline,
        target,
        users,
        live_packages,
        BaselineRoundTrip::Restore,
    )
}

/// The state a package must be in for one half of the round trip to consider
/// it done, or `None` when this direction cannot act on it at all.
///
/// `Restore` targets the enable state the baseline recorded. `Reapply` targets
/// the state the recorded action produces — which is why re-applying is total
/// only for the two enable-state actions; everything else changed something
/// the baseline never captured.
fn target_enabled_state(package: &BaselinePackage, round_trip: BaselineRoundTrip) -> Option<bool> {
    match round_trip {
        BaselineRoundTrip::Restore => package.enabled,
        BaselineRoundTrip::Reapply => match package.requested_action {
            ActionKind::Disable => Some(false),
            ActionKind::Enable => Some(true),
            _ => None,
        },
    }
}

/// Why a recorded action is outside the portable baseline's reach, or `None`
/// when it is an enable-state action the baseline can both undo and redo.
fn irreversible_reason(kind: ActionKind) -> Option<&'static str> {
    match kind {
        ActionKind::Disable | ActionKind::Enable => None,
        ActionKind::UninstallForUser => Some(
            "the package was uninstalled for this user; the portable baseline records no APK, so only the same-device Activity journal can offer a reinstall",
        ),
        ActionKind::ClearData => {
            Some("app data was cleared; the baseline records no data to restore")
        }
        ActionKind::ForceStop => Some(
            "force-stop leaves no persistent state, so there is nothing to restore or re-apply",
        ),
        _ => Some("the recorded action is not an enable-state change the baseline can reverse"),
    }
}

pub fn inspect_round_trip(
    baseline: RecoveryBaseline,
    target: &DeviceTarget,
    users: &[AndroidUser],
    live_packages: &[AppPackage],
    round_trip: BaselineRoundTrip,
) -> Result<RecoveryBaselineDiff, RecoveryBaselineError> {
    validate(&baseline)?;
    let current_fingerprint = target
        .build_fingerprint
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            RecoveryBaselineError::Validate(
                "live target has no verified build fingerprint".to_string(),
            )
        })?;
    let current_identity = hashed_device_identity(&target.serial);
    let identity_matches = baseline.device.identity_sha256 == current_identity;
    let user_available = users.iter().any(|user| user.id == baseline.android_user);
    let live: HashMap<&str, &AppPackage> = live_packages
        .iter()
        .map(|package| (package.package.as_str(), package))
        .collect();
    let mut rows = Vec::with_capacity(baseline.packages.len());
    let mut plans = Vec::new();
    let mut irreversible = Vec::new();

    for package in &baseline.packages {
        // Named for both directions, not only the one being planned: the
        // operator has to know before the update what will not come back
        // after it.
        if let Some(reason) = irreversible_reason(package.requested_action) {
            irreversible.push(BaselineIrreversible {
                package: package.package.clone(),
                requested_action: package.requested_action,
                reason: reason.to_string(),
            });
        }
        let current = live.get(package.package.as_str()).copied();
        let wanted = target_enabled_state(package, round_trip);
        let (status, reason_code, reason, plan_kind) = if !identity_matches {
            skipped(
                "device_identity_mismatch",
                "baseline belongs to a different device identity",
            )
        } else if !user_available {
            skipped(
                "android_user_missing",
                "baseline Android user is not available on the live device",
            )
        } else if !package.present {
            skipped(
                "baseline_package_absent",
                "package was absent before the recorded change",
            )
        } else if current.is_none() {
            skipped(
                "live_package_absent",
                "package is absent from the live Android user",
            )
        } else if current.map(|entry| entry.system) != package.system {
            skipped(
                "system_class_changed",
                "package changed between system and user-installed classification",
            )
        } else if let Some(wanted) = wanted {
            if current.map(|entry| entry.enabled) == Some(wanted) {
                // The end state is already reached, so this direction has
                // nothing to do. A re-apply reaching this branch is precisely
                // the case that must never be replayed.
                (
                    BaselineDiffStatus::AlreadyMatches,
                    None,
                    match round_trip {
                        BaselineRoundTrip::Restore => {
                            "live package already matches the pre-change baseline".to_string()
                        }
                        BaselineRoundTrip::Reapply => {
                            "live package already reflects the recorded action".to_string()
                        }
                    },
                    None,
                )
            } else {
                let kind = if wanted {
                    ActionKind::Enable
                } else {
                    ActionKind::Disable
                };
                match round_trip {
                    BaselineRoundTrip::Restore => (
                        BaselineDiffStatus::Ready,
                        None,
                        "review this canonical enable-state recovery action".to_string(),
                        Some(kind),
                    ),
                    // Post-OTA, a package that no longer reflects its recorded
                    // action was almost certainly reverted by the update. Say
                    // so rather than presenting it as routine.
                    BaselineRoundTrip::Reapply => (
                        BaselineDiffStatus::Drifted,
                        Some("post_change_reverted"),
                        "package state was reverted, likely by a system update".to_string(),
                        Some(kind),
                    ),
                }
            }
        } else {
            skipped(
                "irreversible_action",
                "the recorded action changed state the portable baseline does not carry",
            )
        };
        if let Some(kind) = plan_kind {
            plans.push(actions::plan(ActionRequest {
                serial: target.serial.clone(),
                target: target.clone(),
                package: package.package.clone(),
                kind,
                user_id: baseline.android_user,
                pack_context: None,
                context: ActionContext {
                    confirmation_source: ConfirmationSource::RecoveryBaseline,
                    ..Default::default()
                },
            }));
        }
        rows.push(BaselineDiffRow {
            package: package.package.clone(),
            baseline_present: package.present,
            baseline_enabled: package.enabled,
            live_present: current.is_some(),
            live_enabled: current.map(|entry| entry.enabled),
            requested_action: package.requested_action,
            status,
            reason_code,
            reason,
        });
    }

    Ok(RecoveryBaselineDiff {
        compatibility: BaselineCompatibility {
            device_identity_matches: identity_matches,
            build_fingerprint_matches: baseline.device.build_fingerprint == current_fingerprint,
            android_user_available: user_available,
            current_device_identity_sha256: current_identity,
            current_build_fingerprint: current_fingerprint,
        },
        baseline,
        round_trip,
        rows,
        plans,
        irreversible,
    })
}

fn skipped(
    code: &'static str,
    reason: &'static str,
) -> (
    BaselineDiffStatus,
    Option<&'static str>,
    String,
    Option<ActionKind>,
) {
    (
        BaselineDiffStatus::Skipped,
        Some(code),
        reason.to_string(),
        None,
    )
}

fn validate(baseline: &RecoveryBaseline) -> Result<(), RecoveryBaselineError> {
    if baseline.format != RECOVERY_BASELINE_FORMAT {
        return Err(RecoveryBaselineError::Validate(format!(
            "unsupported format {:?}",
            baseline.format
        )));
    }
    if baseline.schema_version != RECOVERY_BASELINE_SCHEMA_VERSION {
        return Err(RecoveryBaselineError::Validate(format!(
            "unsupported schema version {} (supported: {}; export a new baseline with a current Droidsmith build)",
            baseline.schema_version, RECOVERY_BASELINE_SCHEMA_VERSION
        )));
    }
    if baseline.exported_at.trim().is_empty() || baseline.exported_at.len() > 64 {
        return Err(RecoveryBaselineError::Validate(
            "export timestamp is missing or too long".to_string(),
        ));
    }
    if baseline.device.identity_sha256.len() != 64
        || !baseline
            .device
            .identity_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(RecoveryBaselineError::Validate(
            "device identity must be a lowercase SHA-256 digest".to_string(),
        ));
    }
    validate_text(
        &baseline.device.build_fingerprint,
        "build fingerprint",
        4_096,
    )?;
    if baseline.packages.is_empty() || baseline.packages.len() > MAX_RECOVERY_BASELINE_PACKAGES {
        return Err(RecoveryBaselineError::Validate(format!(
            "package count must be between 1 and {MAX_RECOVERY_BASELINE_PACKAGES}"
        )));
    }
    if let Some(pack) = &baseline.pack {
        validate_text(&pack.id, "pack id", 255)?;
    }
    let mut seen = HashSet::new();
    for package in &baseline.packages {
        if !valid_package_name(&package.package) || !seen.insert(package.package.as_str()) {
            return Err(RecoveryBaselineError::Validate(format!(
                "invalid or duplicate package {:?}",
                package.package
            )));
        }
        if !is_package_action(package.requested_action) {
            return Err(RecoveryBaselineError::Validate(format!(
                "unsupported requested action {:?}",
                package.requested_action
            )));
        }
        if package.present != package.enabled.is_some()
            || package.present != package.system.is_some()
        {
            return Err(RecoveryBaselineError::Validate(format!(
                "package {:?} has inconsistent presence metadata",
                package.package
            )));
        }
        if let Some(undo) = &package.undo_plan {
            if undo.user_id != baseline.android_user
                || !matches!(undo.kind, ActionKind::Disable | ActionKind::Enable)
            {
                return Err(RecoveryBaselineError::Validate(format!(
                    "package {:?} has an invalid undo plan",
                    package.package
                )));
            }
        }
        let expected_undo = match (package.present, package.enabled, package.requested_action) {
            (true, Some(true), ActionKind::Disable) => Some(BaselineUndoPlan {
                kind: ActionKind::Enable,
                user_id: baseline.android_user,
            }),
            (true, Some(false), ActionKind::Enable) => Some(BaselineUndoPlan {
                kind: ActionKind::Disable,
                user_id: baseline.android_user,
            }),
            _ => None,
        };
        if package.undo_plan != expected_undo {
            return Err(RecoveryBaselineError::Validate(format!(
                "package {:?} undo plan does not match its pre-change state and requested action",
                package.package
            )));
        }
    }
    Ok(())
}

fn is_package_action(kind: ActionKind) -> bool {
    matches!(
        kind,
        ActionKind::Disable
            | ActionKind::Enable
            | ActionKind::UninstallForUser
            | ActionKind::ClearData
            | ActionKind::ForceStop
    )
}

fn validate_text(value: &str, label: &str, max_chars: usize) -> Result<(), RecoveryBaselineError> {
    if value.trim().is_empty()
        || value.chars().count() > max_chars
        || value.chars().any(char::is_control)
    {
        return Err(RecoveryBaselineError::Validate(format!(
            "{label} is empty, too long, or contains control characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use BaselineDiffStatus::{AlreadyMatches, Drifted, Ready, Skipped};

    fn row<'a>(diff: &'a RecoveryBaselineDiff, package: &str) -> &'a BaselineDiffRow {
        diff.rows
            .iter()
            .find(|row| row.package == package)
            .unwrap_or_else(|| panic!("diff has no row for {package}"))
    }

    fn plan_kinds(diff: &RecoveryBaselineDiff, package: &str) -> Vec<ActionKind> {
        diff.plans
            .iter()
            .filter(|plan| plan.request.package == package)
            .map(|plan| plan.request.kind)
            .collect()
    }

    fn target(serial: &str, fingerprint: &str) -> DeviceTarget {
        DeviceTarget {
            serial: serial.to_string(),
            transport_id: Some(1),
            connection_generation: 2,
            model: Some("Pixel".to_string()),
            product: None,
            device: None,
            build_fingerprint: Some(fingerprint.to_string()),
            transport_kind: crate::adb::DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        }
    }

    fn package(name: &str, enabled: bool, system: bool) -> AppPackage {
        AppPackage {
            package: name.to_string(),
            enabled,
            system,
            apk_path: Some(if system {
                "/system/app/Test/base.apk".to_string()
            } else {
                "/data/app/Test/base.apk".to_string()
            }),
            uid: Some(10_042),
            installer: Some("com.android.vending".to_string()),
            archived: false,
            retained: false,
        }
    }

    fn user(id: u32) -> AndroidUser {
        AndroidUser {
            id,
            name: "Owner".to_string(),
            running: true,
            current: true,
        }
    }

    #[test]
    fn export_is_redacted_and_records_only_safe_undo() {
        let baseline = build(
            &target("SECRET-SERIAL", "google/build:a"),
            0,
            Some(BaselinePack {
                id: "pixel-safe".to_string(),
                revision: 4,
            }),
            &[
                package("com.example.enabled", true, true),
                package("com.example.clear", true, false),
            ],
            vec![
                BaselineActionInput {
                    package: "com.example.enabled".to_string(),
                    kind: ActionKind::Disable,
                },
                BaselineActionInput {
                    package: "com.example.clear".to_string(),
                    kind: ActionKind::ClearData,
                },
            ],
            "2026-07-15T12:00:00Z".to_string(),
        )
        .unwrap();
        let json = serde_json::to_string(&baseline).unwrap();
        assert!(!json.contains("SECRET-SERIAL"));
        assert!(!json.contains("com.android.vending"));
        assert!(!json.contains("/system/"));
        assert!(!json.contains("10042"));
        assert_eq!(
            baseline.packages[1].undo_plan.as_ref().unwrap().kind,
            ActionKind::Enable
        );
        assert!(baseline.packages[0].undo_plan.is_none());

        let dir = std::env::temp_dir().join(format!(
            "droidsmith-recovery-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let destination = dir.join("baseline.json");
        std::fs::write(&destination, "stale").unwrap();
        let artifact = save(&destination, &baseline).unwrap();
        let saved = std::fs::read_to_string(&destination).unwrap();
        assert!(!saved.contains("SECRET-SERIAL"));
        assert_eq!(artifact.size_bytes, saved.len() as u64);
        assert_eq!(artifact.sha256.len(), 64);
        assert_eq!(load(&destination).unwrap(), baseline);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn inspect_is_read_only_and_skips_mismatches_with_reasons() {
        let baseline = build(
            &target("serial-a", "build/old"),
            10,
            None,
            &[
                package("com.example.ready", true, true),
                package("com.example.missing", true, true),
                package("com.example.changed", true, true),
            ],
            vec![
                BaselineActionInput {
                    package: "com.example.ready".to_string(),
                    kind: ActionKind::Disable,
                },
                BaselineActionInput {
                    package: "com.example.missing".to_string(),
                    kind: ActionKind::Disable,
                },
                BaselineActionInput {
                    package: "com.example.changed".to_string(),
                    kind: ActionKind::Disable,
                },
            ],
            "2026-07-15T12:00:00Z".to_string(),
        )
        .unwrap();
        let diff = inspect(
            baseline,
            &target("serial-a", "build/new"),
            &[user(10)],
            &[
                package("com.example.ready", false, true),
                package("com.example.changed", false, false),
            ],
        )
        .unwrap();
        assert!(!diff.compatibility.build_fingerprint_matches);
        assert_eq!(diff.plans.len(), 1);
        assert_eq!(diff.plans[0].request.kind, ActionKind::Enable);
        assert_eq!(
            diff.plans[0].request.context.confirmation_source,
            ConfirmationSource::RecoveryBaseline
        );
        assert_eq!(diff.rows[0].reason_code, Some("system_class_changed"));
        assert_eq!(diff.rows[1].reason_code, Some("live_package_absent"));
    }

    #[test]
    fn identity_and_user_mismatches_never_create_recovery_plans() {
        let baseline = build(
            &target("serial-a", "build/a"),
            10,
            None,
            &[package("com.example.app", true, true)],
            vec![BaselineActionInput {
                package: "com.example.app".to_string(),
                kind: ActionKind::Disable,
            }],
            "2026-07-15T12:00:00Z".to_string(),
        )
        .unwrap();
        let wrong_device = inspect(
            baseline.clone(),
            &target("serial-b", "build/a"),
            &[user(10)],
            &[package("com.example.app", false, true)],
        )
        .unwrap();
        assert!(wrong_device.plans.is_empty());
        assert_eq!(
            wrong_device.rows[0].reason_code,
            Some("device_identity_mismatch")
        );

        let missing_user =
            inspect(baseline, &target("serial-a", "build/a"), &[user(0)], &[]).unwrap();
        assert!(missing_user.plans.is_empty());
        assert_eq!(
            missing_user.rows[0].reason_code,
            Some("android_user_missing")
        );
    }

    #[test]
    fn a_changed_build_is_reported_as_a_separate_axis_from_identity() {
        // IMP-99 mixed the build fingerprint into every other per-device store.
        // Baselines deliberately opt out: an OTA changes the fingerprint, and a
        // baseline that disowned its device after an update would be useless
        // exactly when it is needed. The build change is reported instead.
        let baseline = build(
            &target("SHARED", "brand/a:16/A/1:user"),
            0,
            None,
            &[package("com.example.app", true, true)],
            vec![BaselineActionInput {
                package: "com.example.app".to_string(),
                kind: ActionKind::Disable,
            }],
            "2026-08-01T12:00:00Z".to_string(),
        )
        .unwrap();

        let updated = inspect(
            baseline,
            &target("SHARED", "brand/b:16/B/2:user"),
            &[user(0)],
            &[package("com.example.app", false, true)],
        )
        .unwrap();
        assert!(updated.compatibility.device_identity_matches);
        assert!(!updated.compatibility.build_fingerprint_matches);
        assert!(
            !updated.plans.is_empty(),
            "a post-OTA device must still be able to restore from its baseline"
        );
    }

    #[test]
    fn detects_post_ota_drift_when_disable_is_reverted() {
        let baseline = build(
            &target("serial-a", "build/v1"),
            0,
            None,
            &[
                package("com.example.held", true, true),
                package("com.example.drifted", true, true),
            ],
            vec![
                BaselineActionInput {
                    package: "com.example.held".to_string(),
                    kind: ActionKind::Disable,
                },
                BaselineActionInput {
                    package: "com.example.drifted".to_string(),
                    kind: ActionKind::Disable,
                },
            ],
            "2026-07-18T10:00:00Z".to_string(),
        )
        .unwrap();
        // Post-OTA device: `held` is still disabled as recorded, `drifted` was
        // re-enabled by the update. The two directions must read this exact
        // same device differently — that asymmetry is the whole point of
        // asking which half of the round trip is being planned.
        let updated = target("serial-a", "build/v2");
        let live = [
            package("com.example.held", false, true),
            package("com.example.drifted", true, true),
        ];

        let restore = inspect_round_trip(
            baseline.clone(),
            &updated,
            &[user(0)],
            &live,
            BaselineRoundTrip::Restore,
        )
        .unwrap();
        // Restoring walks back to the pre-change state: the still-disabled
        // package needs enabling, the reverted one is already there.
        assert_eq!(row(&restore, "com.example.held").status, Ready);
        assert_eq!(
            plan_kinds(&restore, "com.example.held"),
            vec![ActionKind::Enable]
        );
        assert_eq!(row(&restore, "com.example.drifted").status, AlreadyMatches);
        assert!(plan_kinds(&restore, "com.example.drifted").is_empty());

        let reapply = inspect_round_trip(
            baseline,
            &updated,
            &[user(0)],
            &live,
            BaselineRoundTrip::Reapply,
        )
        .unwrap();
        // Re-applying walks forward to the recorded action: the reverted
        // package is re-disabled, and the one that survived the update is
        // never touched again.
        let drifted_row = row(&reapply, "com.example.drifted");
        assert_eq!(drifted_row.status, Drifted);
        assert_eq!(drifted_row.reason_code, Some("post_change_reverted"));
        assert_eq!(
            plan_kinds(&reapply, "com.example.drifted"),
            vec![ActionKind::Disable]
        );
        assert_eq!(row(&reapply, "com.example.held").status, AlreadyMatches);
        assert!(plan_kinds(&reapply, "com.example.held").is_empty());

        // Neither direction plans anything against a package it already
        // matches, so a repeated apply is a no-op rather than a replay.
        assert_eq!(restore.plans.len(), 1);
        assert_eq!(reapply.plans.len(), 1);
    }

    #[test]
    fn a_non_enable_state_action_is_named_irreversible_in_both_directions() {
        let baseline = build(
            &target("serial-a", "build/v1"),
            0,
            None,
            &[
                package("com.example.wiped", true, true),
                package("com.example.disabled", true, true),
            ],
            vec![
                BaselineActionInput {
                    package: "com.example.wiped".to_string(),
                    kind: ActionKind::ClearData,
                },
                BaselineActionInput {
                    package: "com.example.disabled".to_string(),
                    kind: ActionKind::Disable,
                },
            ],
            "2026-08-01T10:00:00Z".to_string(),
        )
        .unwrap();
        let live = [
            package("com.example.wiped", true, true),
            package("com.example.disabled", false, true),
        ];

        for round_trip in [BaselineRoundTrip::Restore, BaselineRoundTrip::Reapply] {
            let diff = inspect_round_trip(
                baseline.clone(),
                &target("serial-a", "build/v1"),
                &[user(0)],
                &live,
                round_trip,
            )
            .unwrap();
            // Named at both ends: the operator must know before the update
            // what the baseline will not bring back after it.
            assert_eq!(diff.irreversible.len(), 1, "{round_trip:?}");
            assert_eq!(diff.irreversible[0].package, "com.example.wiped");
            assert_eq!(diff.irreversible[0].requested_action, ActionKind::ClearData);
            assert!(
                diff.irreversible[0].reason.contains("data"),
                "{:?}",
                diff.irreversible[0].reason
            );
            assert_eq!(diff.round_trip, round_trip);
            // No direction ever plans an action for it.
            assert!(plan_kinds(&diff, "com.example.wiped").is_empty());
        }

        // Re-applying refuses the cleared package explicitly rather than
        // silently treating its untouched enable state as success.
        let reapply = inspect_round_trip(
            baseline,
            &target("serial-a", "build/v1"),
            &[user(0)],
            &live,
            BaselineRoundTrip::Reapply,
        )
        .unwrap();
        let wiped = row(&reapply, "com.example.wiped");
        assert_eq!(wiped.status, Skipped);
        assert_eq!(wiped.reason_code, Some("irreversible_action"));
    }

    #[test]
    fn load_rejects_future_unknown_and_oversized_documents() {
        let dir = std::env::temp_dir().join(format!(
            "droidsmith-recovery-baseline-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("baseline.json");
        let fixture = include_str!("../fixtures/recovery-baselines/v1-valid.json");
        std::fs::write(&path, fixture).unwrap();
        assert_eq!(load(&path).unwrap().schema_version, 1);

        std::fs::write(
            &path,
            fixture.replace("\"schema_version\": 1", "\"schema_version\": 2"),
        )
        .unwrap();
        assert!(matches!(
            load(&path),
            Err(RecoveryBaselineError::Validate(_))
        ));
        std::fs::write(
            &path,
            fixture.replace(
                "\"schema_version\": 1,",
                "\"schema_version\": 1, \"unknown\": true,",
            ),
        )
        .unwrap();
        assert!(matches!(load(&path), Err(RecoveryBaselineError::Parse(_))));
        std::fs::write(&path, vec![b' '; MAX_RECOVERY_BASELINE_BYTES as usize + 1]).unwrap();
        assert!(matches!(load(&path), Err(RecoveryBaselineError::Read(_))));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
