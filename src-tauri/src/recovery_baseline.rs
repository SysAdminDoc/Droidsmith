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

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct RecoveryBaselineDiff {
    pub baseline: RecoveryBaseline,
    pub compatibility: BaselineCompatibility,
    pub rows: Vec<BaselineDiffRow>,
    pub plans: Vec<PlannedAction>,
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

pub fn inspect(
    baseline: RecoveryBaseline,
    target: &DeviceTarget,
    users: &[AndroidUser],
    live_packages: &[AppPackage],
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

    for package in &baseline.packages {
        let current = live.get(package.package.as_str()).copied();
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
        } else if current.map(|entry| entry.enabled) == package.enabled {
            (
                BaselineDiffStatus::AlreadyMatches,
                None,
                "live package already matches the pre-change baseline".to_string(),
                None,
            )
        } else {
            let kind = if package.enabled == Some(true) {
                ActionKind::Enable
            } else {
                ActionKind::Disable
            };
            (
                BaselineDiffStatus::Ready,
                None,
                "review this canonical enable-state recovery action".to_string(),
                Some(kind),
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
        rows,
        plans,
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
