//! Versioned declarative device-setup profiles shared by the GUI and CLI.
//!
//! Version 2 makes the Android-user target and device compatibility checks
//! explicit. Version 1 is never applied implicitly: callers must inspect it,
//! review the migration, and save the returned v2 document.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adb::{
    actions::{ActionContext, ActionKind, ActionRequest, ConfirmationSource},
    AndroidUser, DeviceTarget,
};

pub const PROFILE_SCHEMA_VERSION: &str = "2";
pub const LEGACY_PROFILE_SCHEMA_VERSION: &str = "1";
pub(crate) const PROFILE_SCHEMA_MIGRATION: &str =
    "profile v1 is inspected and migrated explicitly to v2; review the profile-level Android user target before saving or applying it";

const MAX_PROFILE_BYTES: u64 = 256 * 1024;
const MAX_PROFILE_ACTIONS: usize = 2_000;
const MAX_PROFILE_TEXT: usize = 4_096;

#[derive(
    schemars::JsonSchema, specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    #[schemars(extend("const" = PROFILE_SCHEMA_VERSION))]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub device: ProfileDeviceMatch,
    #[serde(default)]
    pub user: ProfileUserTarget,
    pub actions: Vec<ProfileAction>,
}

#[derive(
    schemars::JsonSchema, specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct ProfileDeviceMatch {
    #[serde(default)]
    pub require_serial_prefix: String,
    #[serde(default)]
    pub require_manufacturer: String,
    #[serde(default)]
    pub require_model: String,
    #[serde(default)]
    pub require_android_min: Option<u32>,
    #[serde(default)]
    pub require_android_max: Option<u32>,
}

#[derive(
    schemars::JsonSchema,
    specta::Type,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ProfileUserMode {
    #[default]
    Owner,
    Current,
    Explicit,
}

#[derive(
    schemars::JsonSchema, specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct ProfileUserTarget {
    #[serde(default)]
    pub mode: ProfileUserMode,
    #[serde(default)]
    pub id: Option<u32>,
}

#[derive(
    schemars::JsonSchema, specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct ProfileAction {
    pub kind: ActionKind,
    pub package: String,
    #[serde(default)]
    pub note: String,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileDocument {
    Current { profile: Profile },
    MigrationAvailable { migration: ProfileMigration },
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileMigration {
    pub from_version: String,
    pub to_version: String,
    pub profile: Profile,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("profile {path} failed validation: {reasons}")]
    Validate { path: PathBuf, reasons: String },
    #[error("could not serialize profile: {0}")]
    Serialize(String),
    #[error("could not save profile: {0}")]
    Save(String),
}

#[derive(Deserialize)]
struct VersionProbe {
    version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyProfile {
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    device: LegacyDeviceMatch,
    actions: Vec<LegacyAction>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDeviceMatch {
    #[serde(default)]
    require_serial_prefix: String,
    #[serde(default)]
    require_manufacturer: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyAction {
    kind: ActionKind,
    package: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    user: u32,
}

pub fn inspect(path: &Path) -> Result<ProfileDocument, ProfileError> {
    let text =
        crate::fs_util::read_to_string_limited(path, MAX_PROFILE_BYTES).map_err(|source| {
            ProfileError::Read {
                path: path.to_path_buf(),
                source,
            }
        })?;
    inspect_text(&text, path)
}

pub fn inspect_text(text: &str, source_path: &Path) -> Result<ProfileDocument, ProfileError> {
    let probe: VersionProbe =
        serde_yaml_ng::from_str(text).map_err(|error| ProfileError::Parse {
            path: source_path.to_path_buf(),
            message: error.to_string(),
        })?;

    match probe.version.as_str() {
        PROFILE_SCHEMA_VERSION => {
            let profile: Profile = parse_yaml(text, source_path)?;
            validate(&profile, source_path)?;
            Ok(ProfileDocument::Current { profile })
        }
        LEGACY_PROFILE_SCHEMA_VERSION => migrate_v1_text(text, source_path)
            .map(|migration| ProfileDocument::MigrationAvailable { migration }),
        version => Err(ProfileError::Validate {
            path: source_path.to_path_buf(),
            reasons: format!(
                "unsupported profile version {version:?}; supported: {PROFILE_SCHEMA_VERSION:?}; only v1 has an explicit migration path"
            ),
        }),
    }
}

/// Load only a current profile. Legacy documents must go through `migrate-v1`
/// (CLI) or the reviewed GUI migration flow before they can be planned.
pub fn load(path: &Path) -> Result<Profile, ProfileError> {
    match inspect(path)? {
        ProfileDocument::Current { profile } => Ok(profile),
        ProfileDocument::MigrationAvailable { .. } => Err(ProfileError::Validate {
            path: path.to_path_buf(),
            reasons: "profile v1 requires explicit migration to v2 before use".to_string(),
        }),
    }
}

pub fn migrate_v1(path: &Path) -> Result<ProfileMigration, ProfileError> {
    let text =
        crate::fs_util::read_to_string_limited(path, MAX_PROFILE_BYTES).map_err(|source| {
            ProfileError::Read {
                path: path.to_path_buf(),
                source,
            }
        })?;
    migrate_v1_text(&text, path)
}

fn migrate_v1_text(text: &str, path: &Path) -> Result<ProfileMigration, ProfileError> {
    let legacy: LegacyProfile = parse_yaml(text, path)?;
    if legacy.version != LEGACY_PROFILE_SCHEMA_VERSION {
        return Err(ProfileError::Validate {
            path: path.to_path_buf(),
            reasons: format!("expected profile v1, got {:?}", legacy.version),
        });
    }

    let mut users = legacy.actions.iter().map(|action| action.user);
    let user_id = users.next().unwrap_or_default();
    if users.any(|candidate| candidate != user_id) {
        return Err(ProfileError::Validate {
            path: path.to_path_buf(),
            reasons: "v1 actions target multiple Android users; split the file by user before migrating to v2"
                .to_string(),
        });
    }

    let user = if user_id == 0 {
        ProfileUserTarget::default()
    } else {
        ProfileUserTarget {
            mode: ProfileUserMode::Explicit,
            id: Some(user_id),
        }
    };
    let profile = Profile {
        name: legacy.name,
        version: PROFILE_SCHEMA_VERSION.to_string(),
        description: legacy.description,
        device: ProfileDeviceMatch {
            require_serial_prefix: legacy.device.require_serial_prefix,
            require_manufacturer: legacy.device.require_manufacturer,
            ..Default::default()
        },
        user,
        actions: legacy
            .actions
            .into_iter()
            .map(|action| ProfileAction {
                kind: action.kind,
                package: action.package,
                note: action.note,
            })
            .collect(),
    };
    validate(&profile, path)?;
    Ok(ProfileMigration {
        from_version: LEGACY_PROFILE_SCHEMA_VERSION.to_string(),
        to_version: PROFILE_SCHEMA_VERSION.to_string(),
        profile,
        warnings: vec![
            "v1 per-action user ids were replaced by one reviewed profile-level user target"
                .to_string(),
        ],
    })
}

fn parse_yaml<T: for<'de> Deserialize<'de>>(text: &str, path: &Path) -> Result<T, ProfileError> {
    serde_yaml_ng::from_str(text).map_err(|error| ProfileError::Parse {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn validate(profile: &Profile, path: &Path) -> Result<(), ProfileError> {
    let issues = lint(profile);
    if issues.is_empty() {
        Ok(())
    } else {
        Err(ProfileError::Validate {
            path: path.to_path_buf(),
            reasons: issues.join("; "),
        })
    }
}

pub fn lint(profile: &Profile) -> Vec<String> {
    let mut issues = Vec::new();
    if profile.name.trim().is_empty() {
        issues.push("name is empty".to_string());
    }
    if profile.name.len() > 200 {
        issues.push("name exceeds 200 bytes".to_string());
    }
    if profile.description.len() > MAX_PROFILE_TEXT {
        issues.push(format!("description exceeds {MAX_PROFILE_TEXT} bytes"));
    }
    if profile.version != PROFILE_SCHEMA_VERSION {
        issues.push(format!(
            "unsupported profile version {:?} (supported: {:?})",
            profile.version, PROFILE_SCHEMA_VERSION
        ));
    }
    if profile.actions.is_empty() {
        issues.push("profile has no actions".to_string());
    }
    if profile.actions.len() > MAX_PROFILE_ACTIONS {
        issues.push(format!(
            "profile has too many actions (maximum {MAX_PROFILE_ACTIONS})"
        ));
    }
    if let (Some(min), Some(max)) = (
        profile.device.require_android_min,
        profile.device.require_android_max,
    ) {
        if min > max {
            issues.push("device Android minimum exceeds maximum".to_string());
        }
    }
    match (profile.user.mode, profile.user.id) {
        (ProfileUserMode::Explicit, None) => {
            issues.push("explicit user mode requires an id".to_string())
        }
        (ProfileUserMode::Owner | ProfileUserMode::Current, Some(_)) => {
            issues.push("owner/current user mode must not include an id".to_string())
        }
        _ => {}
    }
    for (index, action) in profile.actions.iter().enumerate() {
        if !supported_action(action.kind) {
            issues.push(format!(
                "action #{}: {:?} is not supported by profile schema v2",
                index + 1,
                action.kind
            ));
        }
        if !crate::adb::packages::valid_package_name(&action.package) {
            issues.push(format!(
                "action #{}: invalid package id {:?}",
                index + 1,
                action.package
            ));
        }
        if action.note.len() > MAX_PROFILE_TEXT {
            issues.push(format!("action #{} note is too long", index + 1));
        }
    }
    issues
}

fn supported_action(kind: ActionKind) -> bool {
    matches!(
        kind,
        ActionKind::Disable
            | ActionKind::Enable
            | ActionKind::UninstallForUser
            | ActionKind::RestoreExistingForUser
            | ActionKind::ClearData
            | ActionKind::ForceStop
    )
}

pub fn serialize(profile: &Profile) -> Result<String, ProfileError> {
    validate(profile, Path::new("<profile>"))?;
    serde_yaml_ng::to_string(profile).map_err(|error| ProfileError::Serialize(error.to_string()))
}

pub fn save(path: &Path, profile: &Profile) -> Result<crate::fs_util::HostArtifact, ProfileError> {
    let yaml = serialize(profile)?;
    let staged = crate::fs_util::StagedArtifact::new(path)
        .map_err(|error| ProfileError::Save(error.to_string()))?;
    std::fs::write(staged.path(), yaml).map_err(|error| ProfileError::Save(error.to_string()))?;
    staged
        .commit(crate::fs_util::ArtifactKind::AnyFile)
        .map_err(|error| ProfileError::Save(error.to_string()))
}

pub fn resolve_user(profile: &Profile, users: &[AndroidUser]) -> Result<u32, Vec<String>> {
    let selected = match profile.user.mode {
        ProfileUserMode::Owner => users.iter().find(|user| user.id == 0),
        ProfileUserMode::Current => users.iter().find(|user| user.current),
        ProfileUserMode::Explicit => profile
            .user
            .id
            .and_then(|id| users.iter().find(|user| user.id == id)),
    };
    selected.map(|user| user.id).ok_or_else(|| {
        vec![match profile.user.mode {
            ProfileUserMode::Owner => "profile requires Android owner user 0".to_string(),
            ProfileUserMode::Current => {
                "profile requires a device-reported current Android user".to_string()
            }
            ProfileUserMode::Explicit => format!(
                "profile requires Android user {}",
                profile.user.id.unwrap_or_default()
            ),
        }]
    })
}

pub fn device_match_issues(
    profile: &Profile,
    serial: &str,
    manufacturer: Option<&str>,
    model: Option<&str>,
    android_sdk: Option<u32>,
) -> Vec<String> {
    let mut issues = Vec::new();
    let prefix = profile.device.require_serial_prefix.trim();
    if !prefix.is_empty() && !serial.starts_with(prefix) {
        issues.push(format!(
            "profile requires a serial starting with {prefix:?}, got {serial:?}"
        ));
    }
    match_required_text(
        &mut issues,
        "manufacturer",
        &profile.device.require_manufacturer,
        manufacturer,
    );
    match_required_text(&mut issues, "model", &profile.device.require_model, model);
    if let Some(min) = profile.device.require_android_min {
        match android_sdk {
            Some(actual) if actual < min => issues.push(format!(
                "profile requires Android SDK {min} or newer, got {actual}"
            )),
            None => issues.push(format!(
                "profile requires Android SDK {min} or newer, but the device did not report it"
            )),
            _ => {}
        }
    }
    if let Some(max) = profile.device.require_android_max {
        match android_sdk {
            Some(actual) if actual > max => issues.push(format!(
                "profile requires Android SDK {max} or older, got {actual}"
            )),
            None => issues.push(format!(
                "profile requires Android SDK {max} or older, but the device did not report it"
            )),
            _ => {}
        }
    }
    issues
}

fn match_required_text(
    issues: &mut Vec<String>,
    label: &str,
    expected: &str,
    actual: Option<&str>,
) {
    let expected = expected.trim();
    if expected.is_empty() {
        return;
    }
    match actual.map(str::trim).filter(|value| !value.is_empty()) {
        Some(actual) if actual.eq_ignore_ascii_case(expected) => {}
        Some(actual) => issues.push(format!(
            "profile requires {label} {expected:?}, got {actual:?}"
        )),
        None => issues.push(format!(
            "profile requires {label} {expected:?}, but the device did not report one"
        )),
    }
}

pub fn requests_for(
    profile: &Profile,
    target: &DeviceTarget,
    user_id: u32,
    confirmation_source: ConfirmationSource,
) -> Vec<ActionRequest> {
    profile
        .actions
        .iter()
        .map(|action| ActionRequest {
            serial: target.serial.clone(),
            target: target.clone(),
            package: action.package.clone(),
            kind: action.kind,
            user_id,
            pack_context: None,
            context: ActionContext {
                confirmation_source,
                ..Default::default()
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const V2: &str = r#"
name: Refurb baseline
version: "2"
description: Fresh-from-box Pixel baseline
device:
  require_manufacturer: Google
  require_android_min: 30
user:
  mode: explicit
  id: 10
actions:
  - kind: disable
    package: com.google.android.apps.subscriptions.red
    note: YouTube Premium nag
  - kind: enable
    package: com.android.bookmarkprovider
"#;

    const V1: &str = r#"
name: Legacy
version: "1"
device:
  require_manufacturer: Google
actions:
  - kind: disable
    package: com.example.one
    user: 10
  - kind: enable
    package: com.example.two
    user: 10
"#;

    #[test]
    fn v2_parses_and_lints_clean() {
        let document = inspect_text(V2, Path::new("profile.yaml")).unwrap();
        let ProfileDocument::Current { profile } = document else {
            panic!("expected current profile");
        };
        assert_eq!(profile.actions.len(), 2);
        assert_eq!(profile.user.id, Some(10));
        assert!(lint(&profile).is_empty());
    }

    #[test]
    fn current_and_legacy_profiles_reject_unknown_fields() {
        let current = V2.replace(
            "    note: YouTube Premium nag",
            "    note: YouTube Premium nag\n    unexpected: true",
        );
        let error = inspect_text(&current, Path::new("profile.yaml"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field"));

        let legacy = V1.replace("    user: 10", "    user: 10\n    unexpected: true");
        let error = inspect_text(&legacy, Path::new("legacy.yaml"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn v1_requires_explicit_semantics_preserving_migration() {
        let document = inspect_text(V1, Path::new("legacy.yaml")).unwrap();
        let ProfileDocument::MigrationAvailable { migration } = document else {
            panic!("expected migration");
        };
        assert_eq!(migration.profile.version, "2");
        assert_eq!(migration.profile.user.mode, ProfileUserMode::Explicit);
        assert_eq!(migration.profile.user.id, Some(10));
        assert!(migration
            .profile
            .actions
            .iter()
            .all(|action| action.note.is_empty()));
    }

    #[test]
    fn mixed_user_v1_migration_fails_closed() {
        let mixed = V1.replacen("user: 10", "user: 0", 1);
        let error = inspect_text(&mixed, Path::new("mixed.yaml")).unwrap_err();
        assert!(error.to_string().contains("multiple Android users"));
    }

    #[test]
    fn lint_rejects_incoherent_constraints_and_unsafe_action_kinds() {
        let mut profile = match inspect_text(V2, Path::new("profile.yaml")).unwrap() {
            ProfileDocument::Current { profile } => profile,
            _ => unreachable!(),
        };
        profile.device.require_android_min = Some(35);
        profile.device.require_android_max = Some(30);
        profile.user.mode = ProfileUserMode::Owner;
        profile.actions[0].kind = ActionKind::Shell;
        let issues = lint(&profile).join("; ");
        assert!(issues.contains("minimum exceeds maximum"));
        assert!(issues.contains("must not include an id"));
        assert!(issues.contains("not supported"));
    }

    #[test]
    fn resolves_owner_current_and_explicit_users() {
        let users = vec![
            AndroidUser {
                id: 0,
                name: "Owner".to_string(),
                running: true,
                current: false,
            },
            AndroidUser {
                id: 10,
                name: "Work".to_string(),
                running: true,
                current: true,
            },
        ];
        let mut profile = match inspect_text(V2, Path::new("profile.yaml")).unwrap() {
            ProfileDocument::Current { profile } => profile,
            _ => unreachable!(),
        };
        assert_eq!(resolve_user(&profile, &users), Ok(10));
        profile.user = ProfileUserTarget::default();
        assert_eq!(resolve_user(&profile, &users), Ok(0));
        profile.user.mode = ProfileUserMode::Current;
        assert_eq!(resolve_user(&profile, &users), Ok(10));
    }

    #[test]
    fn device_constraints_report_every_mismatch() {
        let profile = match inspect_text(V2, Path::new("profile.yaml")).unwrap() {
            ProfileDocument::Current { profile } => profile,
            _ => unreachable!(),
        };
        let issues = device_match_issues(&profile, "XYZ", Some("Samsung"), Some("S24"), Some(29));
        assert_eq!(issues.len(), 2);
        assert!(issues.iter().any(|issue| issue.contains("manufacturer")));
        assert!(issues.iter().any(|issue| issue.contains("SDK")));
    }

    #[test]
    fn requests_bind_one_reviewed_user_and_source() {
        let profile = match inspect_text(V2, Path::new("profile.yaml")).unwrap() {
            ProfileDocument::Current { profile } => profile,
            _ => unreachable!(),
        };
        let target = DeviceTarget {
            serial: "abc-123".into(),
            transport_id: Some(4),
            connection_generation: 5,
            model: None,
            product: None,
            device: None,
            build_fingerprint: Some("build/test".into()),
            transport_kind: crate::adb::DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        };
        let requests = requests_for(&profile, &target, 10, ConfirmationSource::CliApply);
        assert!(requests.iter().all(|request| request.user_id == 10));
        assert!(requests
            .iter()
            .all(|request| request.context.confirmation_source == ConfirmationSource::CliApply));
    }

    #[test]
    fn serialization_round_trip_is_current_and_deterministic() {
        let profile = match inspect_text(V2, Path::new("profile.yaml")).unwrap() {
            ProfileDocument::Current { profile } => profile,
            _ => unreachable!(),
        };
        let first = serialize(&profile).unwrap();
        let second = serialize(&profile).unwrap();
        assert_eq!(first, second);
        assert!(matches!(
            inspect_text(&first, Path::new("roundtrip.yaml")).unwrap(),
            ProfileDocument::Current { .. }
        ));
    }
}
