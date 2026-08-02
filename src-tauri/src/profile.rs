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

pub const PROFILE_SCHEMA_VERSION: &str = "3";
/// v2 stays loadable and runnable: it is a strict subset of v3 (every v2
/// action is a concrete package), so nothing about it is ambiguous or unsafe.
/// An explicit reviewed upgrade to v3 is offered, but not required — unlike
/// v1, whose per-action user ids genuinely could not be interpreted.
pub const PROFILE_V2_SCHEMA_VERSION: &str = "2";
pub const LEGACY_PROFILE_SCHEMA_VERSION: &str = "1";
pub(crate) const PROFILE_SCHEMA_MIGRATION: &str =
    "profile v1 is inspected and migrated explicitly to the current schema; review the profile-level Android user target before saving or applying it. v2 loads and runs as-is, and separately offers a reviewed upgrade to v3";

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

/// One step of a profile: either a concrete package, or — from schema v3 — a
/// predicate resolved against the live inventory at plan time.
///
/// Exactly one of the two is set. They are separate fields rather than an
/// enum because a v2 document must keep deserializing byte-for-byte, and
/// because YAML with a tag discriminator is markedly worse to hand-author.
#[derive(
    schemars::JsonSchema, specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct ProfileAction {
    pub kind: ActionKind,
    /// A concrete package id. Empty when `filter` is set.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub package: String,
    /// A schema-v3 predicate over package attributes. Empty when `package` is
    /// set. See [`crate::profile_filter`] for the grammar.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub filter: String,
    #[serde(default)]
    pub note: String,
}

impl ProfileAction {
    /// A profile step is filter-driven when it carries a predicate instead of
    /// a package id.
    pub fn is_filter(&self) -> bool {
        !self.filter.is_empty()
    }
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileDocument {
    Current {
        profile: Profile,
    },
    /// Loadable and runnable as-is, with a reviewed upgrade to the current
    /// schema also available. Only v2 reaches this state.
    UpgradeAvailable {
        profile: Profile,
        migration: ProfileMigration,
    },
    /// Not runnable until the migration is reviewed and saved. Only v1
    /// reaches this state.
    MigrationAvailable {
        migration: ProfileMigration,
    },
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
        // v2 runs as-is and separately offers a reviewed upgrade. The two are
        // reported together because the native read grant is one-shot: making
        // the caller re-open the file to see the upgrade would be worse than
        // computing both from the bytes already in hand.
        PROFILE_V2_SCHEMA_VERSION => {
            let profile: Profile = parse_yaml(text, source_path)?;
            validate(&profile, source_path)?;
            let migration = upgrade_v2(&profile);
            Ok(ProfileDocument::UpgradeAvailable { profile, migration })
        }
        LEGACY_PROFILE_SCHEMA_VERSION => migrate_v1_text(text, source_path)
            .map(|migration| ProfileDocument::MigrationAvailable { migration }),
        version => Err(ProfileError::Validate {
            path: source_path.to_path_buf(),
            reasons: format!(
                "unsupported profile version {version:?}; supported: {PROFILE_SCHEMA_VERSION:?} and {PROFILE_V2_SCHEMA_VERSION:?}; only v1 has an explicit migration path"
            ),
        }),
    }
}

/// Load a runnable profile. v3 and v2 both load; v1 must go through
/// `migrate-v1` (CLI) or the reviewed GUI migration flow first, because its
/// per-action user ids cannot be interpreted without a decision.
pub fn load(path: &Path) -> Result<Profile, ProfileError> {
    match inspect(path)? {
        ProfileDocument::Current { profile } => Ok(profile),
        ProfileDocument::UpgradeAvailable { profile, .. } => Ok(profile),
        ProfileDocument::MigrationAvailable { .. } => Err(ProfileError::Validate {
            path: path.to_path_buf(),
            reasons: "profile v1 requires explicit migration before use".to_string(),
        }),
    }
}

/// Build the reviewed v2 → v3 upgrade for an already-validated v2 profile.
///
/// The upgrade is purely a version bump: v2 carries only concrete packages,
/// which v3 still supports unchanged. It is offered rather than applied so the
/// saved document is one the user chose, matching how v1 → v2 works.
pub fn upgrade_v2(profile: &Profile) -> ProfileMigration {
    let mut upgraded = profile.clone();
    upgraded.version = PROFILE_SCHEMA_VERSION.to_string();
    ProfileMigration {
        from_version: PROFILE_V2_SCHEMA_VERSION.to_string(),
        to_version: PROFILE_SCHEMA_VERSION.to_string(),
        profile: upgraded,
        warnings: vec![
            "v2 actions are concrete package ids and carry over unchanged; v3 additionally allows filter predicates resolved against the live device"
                .to_string(),
        ],
    }
}

pub fn migrate_v2(path: &Path) -> Result<ProfileMigration, ProfileError> {
    match inspect(path)? {
        ProfileDocument::UpgradeAvailable { migration, .. } => Ok(migration),
        ProfileDocument::Current { .. } => Err(ProfileError::Validate {
            path: path.to_path_buf(),
            reasons: format!("profile is already schema v{PROFILE_SCHEMA_VERSION}"),
        }),
        ProfileDocument::MigrationAvailable { .. } => Err(ProfileError::Validate {
            path: path.to_path_buf(),
            reasons: "profile is v1; migrate it with migrate-v1 first".to_string(),
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
                filter: String::new(),
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
    let filters_allowed = profile.version == PROFILE_SCHEMA_VERSION;
    if !filters_allowed && profile.version != PROFILE_V2_SCHEMA_VERSION {
        issues.push(format!(
            "unsupported profile version {:?} (supported: {:?}, {:?})",
            profile.version, PROFILE_SCHEMA_VERSION, PROFILE_V2_SCHEMA_VERSION
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
        match (action.package.is_empty(), action.filter.is_empty()) {
            (true, true) => issues.push(format!(
                "action #{}: needs either a package id or a filter",
                index + 1
            )),
            (false, false) => issues.push(format!(
                "action #{}: set either a package id or a filter, not both",
                index + 1
            )),
            (false, true) => {
                if !crate::adb::packages::valid_package_name(&action.package) {
                    issues.push(format!(
                        "action #{}: invalid package id {:?}",
                        index + 1,
                        action.package
                    ));
                }
            }
            (true, false) => {
                if !filters_allowed {
                    issues.push(format!(
                        "action #{}: filters require profile schema v{PROFILE_SCHEMA_VERSION}",
                        index + 1
                    ));
                }
                // Parse at validation time, not at plan time: an unparseable
                // predicate is a broken document, and finding that out only
                // once a device is attached would be far too late.
                if let Err(error) = crate::profile_filter::parse(&action.filter) {
                    issues.push(format!("action #{}: invalid filter — {error}", index + 1));
                }
            }
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

/// What one filter action selected from the live inventory.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FilterMatch {
    /// 1-based position in `profile.actions`.
    pub action_index: usize,
    pub filter: String,
    pub kind: ActionKind,
    /// Matched packages, sorted, so the same device and profile always produce
    /// the same reviewable order.
    pub packages: Vec<String>,
}

/// A package a predicate could not decide, and therefore did not select.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterExclusion {
    pub action_index: usize,
    pub filter: String,
    pub package: String,
    /// The attribute the device did not report.
    pub attribute: String,
}

/// One concrete request plus the profile step it came from. A filter step
/// produces many of these; a concrete step produces exactly one.
#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ResolvedRequest {
    /// 1-based position in `profile.actions`.
    pub action_index: usize,
    /// The predicate that selected this package, empty for a concrete step.
    pub filter: String,
    pub request: ActionRequest,
}

#[derive(specta::Type, Debug, Clone, Default, Serialize)]
pub struct ResolvedProfile {
    pub requests: Vec<ResolvedRequest>,
    /// One entry per filter action, including actions that matched nothing —
    /// "this predicate selected zero packages" is a reviewable outcome, not an
    /// absence.
    pub matches: Vec<FilterMatch>,
    /// Every package excluded because a predicate could not be decided. Never
    /// silently dropped.
    pub exclusions: Vec<FilterExclusion>,
}

/// Turn a profile into the concrete requests it will run, resolving any v3
/// filter predicates against the live inventory.
///
/// Resolution is total: every package is matched, not matched, or reported as
/// undecidable. A package whose predicate could not be decided is excluded and
/// listed in `exclusions` — it never reaches `requests`.
pub fn resolve(
    profile: &Profile,
    target: &DeviceTarget,
    user_id: u32,
    inventory: &[crate::adb::packages::AppPackage],
    confirmation_source: ConfirmationSource,
) -> ResolvedProfile {
    let mut resolved = ResolvedProfile::default();
    let request = |package: String, kind: ActionKind| ActionRequest {
        serial: target.serial.clone(),
        target: target.clone(),
        package,
        kind,
        user_id,
        pack_context: None,
        context: ActionContext {
            confirmation_source,
            ..Default::default()
        },
    };

    for (index, action) in profile.actions.iter().enumerate() {
        let action_index = index + 1;
        if !action.is_filter() {
            resolved.requests.push(ResolvedRequest {
                action_index,
                filter: String::new(),
                request: request(action.package.clone(), action.kind),
            });
            continue;
        }
        // `lint` already parsed every filter, so a document that reached here
        // cannot fail to parse. Treat a parse failure as "selects nothing"
        // rather than panicking: this is the apply path.
        let Ok(expr) = crate::profile_filter::parse(&action.filter) else {
            resolved.matches.push(FilterMatch {
                action_index,
                filter: action.filter.clone(),
                kind: action.kind,
                packages: Vec::new(),
            });
            continue;
        };
        let mut packages = Vec::new();
        for package in inventory {
            let context = crate::profile_filter::FilterContext {
                package,
                android_user: user_id,
            };
            match crate::profile_filter::evaluate(&expr, &context) {
                Ok(true) => packages.push(package.package.clone()),
                Ok(false) => {}
                Err(unresolvable) => resolved.exclusions.push(FilterExclusion {
                    action_index,
                    filter: action.filter.clone(),
                    package: package.package.clone(),
                    attribute: unresolvable.attribute.to_string(),
                }),
            }
        }
        packages.sort();
        packages.dedup();
        for package in &packages {
            resolved.requests.push(ResolvedRequest {
                action_index,
                filter: action.filter.clone(),
                request: request(package.clone(), action.kind),
            });
        }
        resolved.matches.push(FilterMatch {
            action_index,
            filter: action.filter.clone(),
            kind: action.kind,
            packages,
        });
    }
    resolved
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

    const V3: &str = r#"
name: Refurb baseline
version: "3"
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
  - kind: disable
    filter: system & enabled & installer == "com.vendor.store"
    note: carrier preloads, wherever they landed on this handset
"#;

    /// Both loadable schemas produce a runnable profile; only v1 does not.
    fn loaded(text: &str) -> Profile {
        match inspect_text(text, Path::new("profile.yaml")).unwrap() {
            ProfileDocument::Current { profile } => profile,
            ProfileDocument::UpgradeAvailable { profile, .. } => profile,
            ProfileDocument::MigrationAvailable { .. } => {
                panic!("expected a loadable profile, got a required migration")
            }
        }
    }

    fn inventory(entries: &[(&str, bool, bool, Option<&str>)]) -> Vec<crate::adb::AppPackage> {
        entries
            .iter()
            .map(
                |(name, system, enabled, installer)| crate::adb::AppPackage {
                    package: (*name).to_string(),
                    enabled: *enabled,
                    system: *system,
                    apk_path: None,
                    uid: None,
                    installer: installer.map(str::to_string),
                    archived: false,
                    retained: false,
                },
            )
            .collect()
    }

    fn test_target() -> DeviceTarget {
        DeviceTarget {
            serial: "abc-123".into(),
            transport_id: Some(4),
            connection_generation: 5,
            model: None,
            product: None,
            device: None,
            build_fingerprint: Some("build/test".into()),
            transport_kind: crate::adb::DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        }
    }

    #[test]
    fn v2_parses_and_lints_clean() {
        let document = inspect_text(V2, Path::new("profile.yaml")).unwrap();
        // v2 is still runnable; the v3 upgrade rides along as an offer.
        let ProfileDocument::UpgradeAvailable { profile, migration } = document else {
            panic!("expected a loadable v2 profile with an upgrade offer");
        };
        assert_eq!(profile.actions.len(), 2);
        assert_eq!(profile.user.id, Some(10));
        assert!(lint(&profile).is_empty());
        assert_eq!(migration.from_version, PROFILE_V2_SCHEMA_VERSION);
        assert_eq!(migration.to_version, PROFILE_SCHEMA_VERSION);
        // The upgrade changes the version and nothing else: v2 carries only
        // concrete packages, which v3 still supports unchanged.
        assert_eq!(migration.profile.actions, profile.actions);
    }

    #[test]
    fn v3_filters_resolve_against_the_live_inventory_in_a_stable_order() {
        let profile = loaded(V3);
        assert!(lint(&profile).is_empty(), "{:?}", lint(&profile));
        let live = inventory(&[
            // Matches: system, enabled, right installer.
            ("com.vendor.zeta", true, true, Some("com.vendor.store")),
            ("com.vendor.alpha", true, true, Some("com.vendor.store")),
            // Misses on each attribute in turn.
            ("com.vendor.disabled", true, false, Some("com.vendor.store")),
            ("com.user.app", false, true, Some("com.vendor.store")),
            ("com.vendor.other", true, true, Some("com.android.vending")),
            // Undecidable: the device reported no installer.
            ("com.vendor.unknown", true, true, None),
            // The concrete action's package, which the filter must not claim.
            (
                "com.google.android.apps.subscriptions.red",
                true,
                true,
                Some("com.android.vending"),
            ),
        ]);
        let resolved = resolve(
            &profile,
            &test_target(),
            10,
            &live,
            ConfirmationSource::CliApply,
        );

        let packages: Vec<&str> = resolved
            .requests
            .iter()
            .map(|resolved| resolved.request.package.as_str())
            .collect();
        assert_eq!(
            packages,
            vec![
                // The concrete step keeps its position...
                "com.google.android.apps.subscriptions.red",
                // ...and the filter step expands in sorted order, so the same
                // device and profile always review identically.
                "com.vendor.alpha",
                "com.vendor.zeta",
            ]
        );

        assert_eq!(resolved.matches.len(), 1);
        assert_eq!(resolved.matches[0].action_index, 2);
        assert_eq!(
            resolved.matches[0].packages,
            vec!["com.vendor.alpha", "com.vendor.zeta"]
        );

        // Undecidable is excluded and reported, never quietly selected.
        assert_eq!(resolved.exclusions.len(), 1);
        assert_eq!(resolved.exclusions[0].package, "com.vendor.unknown");
        assert_eq!(resolved.exclusions[0].attribute, "installer");
        assert_eq!(resolved.exclusions[0].action_index, 2);
        assert!(!packages.contains(&"com.vendor.unknown"));
    }

    #[test]
    fn a_filter_that_matches_nothing_is_reported_rather_than_omitted() {
        let profile = loaded(V3);
        let resolved = resolve(
            &profile,
            &test_target(),
            10,
            &inventory(&[("com.user.app", false, true, Some("com.other"))]),
            ConfirmationSource::CliApply,
        );
        // "This predicate selected zero packages" is a reviewable outcome.
        assert_eq!(resolved.matches.len(), 1);
        assert!(resolved.matches[0].packages.is_empty());
        assert_eq!(resolved.requests.len(), 1);
        assert!(resolved.exclusions.is_empty());
    }

    #[test]
    fn filters_are_rejected_by_v2_and_validated_before_a_device_is_involved() {
        let v2_with_filter = V3.replace("version: \"3\"", "version: \"2\"");
        let error = inspect_text(&v2_with_filter, Path::new("profile.yaml")).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("filters require profile schema v3"),
            "{error}"
        );

        // A broken predicate is a broken document, caught at validation rather
        // than once a device is attached.
        let broken = V3.replace("system & enabled", "system & nonsense");
        let error = inspect_text(&broken, Path::new("profile.yaml")).unwrap_err();
        assert!(error.to_string().contains("invalid filter"), "{error}");

        // Neither field, or both, is incoherent.
        let mut both = loaded(V3);
        both.actions[1].package = "com.example.one".to_string();
        assert!(lint(&both).iter().any(|issue| issue.contains("not both")));
        let mut neither = loaded(V3);
        neither.actions[1].filter = String::new();
        assert!(lint(&neither)
            .iter()
            .any(|issue| issue.contains("either a package id or a filter")));
    }

    #[test]
    fn a_v3_profile_round_trips_with_its_filters_intact() {
        let profile = loaded(V3);
        let yaml = serialize(&profile).unwrap();
        assert!(yaml.contains("filter:"), "{yaml}");
        // The concrete action must not gain an empty `filter:` key, and the
        // filter action must not gain an empty `package:` key.
        assert_eq!(yaml.matches("filter:").count(), 1);
        assert_eq!(yaml.matches("package:").count(), 1);
        let reparsed = loaded(&yaml);
        assert_eq!(reparsed, profile);
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
        assert_eq!(migration.profile.version, PROFILE_SCHEMA_VERSION);
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
        let mut profile = loaded(V2);
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
        let mut profile = loaded(V2);
        assert_eq!(resolve_user(&profile, &users), Ok(10));
        profile.user = ProfileUserTarget::default();
        assert_eq!(resolve_user(&profile, &users), Ok(0));
        profile.user.mode = ProfileUserMode::Current;
        assert_eq!(resolve_user(&profile, &users), Ok(10));
    }

    #[test]
    fn device_constraints_report_every_mismatch() {
        let profile = loaded(V2);
        let issues = device_match_issues(&profile, "XYZ", Some("Samsung"), Some("S24"), Some(29));
        assert_eq!(issues.len(), 2);
        assert!(issues.iter().any(|issue| issue.contains("manufacturer")));
        assert!(issues.iter().any(|issue| issue.contains("SDK")));
    }

    #[test]
    fn requests_bind_one_reviewed_user_and_source() {
        let profile = loaded(V2);
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
        let resolved = resolve(&profile, &target, 10, &[], ConfirmationSource::CliApply);
        assert!(resolved
            .requests
            .iter()
            .all(|resolved| resolved.request.user_id == 10));
        assert!(resolved.requests.iter().all(|resolved| {
            resolved.request.context.confirmation_source == ConfirmationSource::CliApply
        }));
    }

    #[test]
    fn serialization_round_trip_is_current_and_deterministic() {
        let profile = loaded(V3);
        let first = serialize(&profile).unwrap();
        let second = serialize(&profile).unwrap();
        assert_eq!(first, second);
        assert!(matches!(
            inspect_text(&first, Path::new("roundtrip.yaml")).unwrap(),
            ProfileDocument::Current { .. }
        ));

        // A v2 document round-trips just as deterministically, and comes back
        // as loadable-with-an-upgrade rather than as an error.
        let v2 = loaded(V2);
        let encoded = serialize(&v2).unwrap();
        assert_eq!(encoded, serialize(&v2).unwrap());
        assert!(matches!(
            inspect_text(&encoded, Path::new("roundtrip-v2.yaml")).unwrap(),
            ProfileDocument::UpgradeAvailable { .. }
        ));
    }
}
