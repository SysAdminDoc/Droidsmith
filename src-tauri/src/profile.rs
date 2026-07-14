//! Declarative device-setup profiles.
//!
//! A profile is a YAML file describing a sequence of actions to apply
//! to a freshly-prepared Android device. Schema:
//!
//! ```yaml
//! name: "Fresh Pixel setup"
//! version: "1"
//! description: "Standard refurb baseline for Pixel devices."
//! device:
//!   require_serial_prefix: ""        # optional sanity check
//!   require_manufacturer: "Google"   # case-insensitive equality
//! actions:
//!   - kind: disable
//!     package: com.google.android.apps.subscriptions.red
//!     note: "YouTube Premium nag"
//!   - kind: enable
//!     package: com.android.bookmarkprovider
//! ```
//!
//! The CLI (`droidsmith-cli run profile.yaml --device <serial>`) loads
//! a profile, translates each entry into the action layer's
//! `ActionRequest`, and either previews (dry-run) or applies them in
//! order, journaling each result.

use serde::{Deserialize, Serialize};

use crate::adb::{
    actions::{ActionKind, ActionRequest},
    DeviceTarget,
};

pub const PROFILE_SCHEMA_VERSION: &str = "1";

const MAX_PROFILE_BYTES: u64 = 256 * 1024;
const PROFILE_SCHEMA_MIGRATION: &str =
    "convert the file to the v1 profile schema in src-tauri/src/profile.rs, set version: \"1\", then rerun the CLI profile lint/load path";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub device: ProfileDeviceMatch,
    pub actions: Vec<ProfileAction>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileDeviceMatch {
    /// Optional substring the device serial must start with. Empty
    /// string disables the check.
    #[serde(default)]
    pub require_serial_prefix: String,
    /// Optional manufacturer constraint, case-insensitive equality.
    #[serde(default)]
    pub require_manufacturer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileAction {
    pub kind: ActionKind,
    pub package: String,
    #[serde(default)]
    pub note: String,
    /// Android user id this action targets. Defaults to `0` (owner) so
    /// existing v1 profiles authored before multi-user targeting keep the
    /// same behavior.
    #[serde(default)]
    pub user: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("could not read {path}: {source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {path}: {source}")]
    Parse {
        path: std::path::PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },
    #[error("profile {path} failed validation: {reasons}")]
    Validate {
        path: std::path::PathBuf,
        reasons: String,
    },
}

pub fn load(path: &std::path::Path) -> Result<Profile, ProfileError> {
    let text =
        crate::fs_util::read_to_string_limited(path, MAX_PROFILE_BYTES).map_err(|source| {
            ProfileError::Read {
                path: path.to_path_buf(),
                source,
            }
        })?;
    let profile: Profile =
        serde_yaml_ng::from_str(&text).map_err(|source| ProfileError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    let issues = lint(&profile);
    if !issues.is_empty() {
        return Err(ProfileError::Validate {
            path: path.to_path_buf(),
            reasons: issues.join("; "),
        });
    }
    Ok(profile)
}

pub fn lint(p: &Profile) -> Vec<String> {
    let mut issues = Vec::new();
    if p.name.trim().is_empty() {
        issues.push("name is empty".to_string());
    }
    if p.version != PROFILE_SCHEMA_VERSION {
        issues.push(format!(
            "unsupported profile version {:?} (supported: {:?}; migration path: {PROFILE_SCHEMA_MIGRATION})",
            p.version, PROFILE_SCHEMA_VERSION
        ));
    }
    if p.actions.is_empty() {
        issues.push("profile has no actions".to_string());
    }
    for (i, action) in p.actions.iter().enumerate() {
        if !matches!(
            action.kind,
            ActionKind::Disable
                | ActionKind::Enable
                | ActionKind::UninstallForUser
                | ActionKind::ClearData
                | ActionKind::ForceStop
        ) {
            issues.push(format!(
                "action #{}: {:?} is not supported by profile schema v1",
                i + 1,
                action.kind
            ));
        }
        if !crate::adb::packages::valid_package_name(&action.package) {
            issues.push(format!(
                "action #{}: invalid package id {:?}",
                i + 1,
                action.package
            ));
        }
    }
    issues
}

/// Lift a profile into a sequence of `ActionRequest`s targeting one
/// device. The serial is bound at run-time; profiles are
/// serial-agnostic by design.
pub fn requests_for(profile: &Profile, target: &DeviceTarget) -> Vec<ActionRequest> {
    profile
        .actions
        .iter()
        .map(|a| ActionRequest {
            serial: target.serial.clone(),
            target: target.clone(),
            package: a.package.clone(),
            kind: a.kind,
            user_id: a.user,
            pack_context: None,
            context: crate::adb::actions::ActionContext {
                confirmation_source: crate::adb::actions::ConfirmationSource::CliApply,
                ..Default::default()
            },
        })
        .collect()
}

pub fn serial_match_issues(profile: &Profile, serial: &str) -> Vec<String> {
    let mut issues = Vec::new();
    let prefix = profile.device.require_serial_prefix.trim();
    if !prefix.is_empty() && !serial.starts_with(prefix) {
        issues.push(format!(
            "profile requires a device serial starting with {prefix:?}, got {serial:?}"
        ));
    }
    issues
}

pub fn manufacturer_match_issues(profile: &Profile, manufacturer: Option<&str>) -> Vec<String> {
    let mut issues = Vec::new();
    let expected = profile.device.require_manufacturer.trim();
    if expected.is_empty() {
        return issues;
    }

    let Some(actual) = manufacturer.map(str::trim).filter(|s| !s.is_empty()) else {
        issues.push(format!(
            "profile requires manufacturer {expected:?}, but the device did not report one"
        ));
        return issues;
    };

    if !actual.eq_ignore_ascii_case(expected) {
        issues.push(format!(
            "profile requires manufacturer {expected:?}, got {actual:?}"
        ));
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
name: "Refurb baseline"
version: "1"
description: "Fresh-from-box Pixel baseline"
device:
  require_manufacturer: "Google"
actions:
  - kind: disable
    package: com.google.android.apps.subscriptions.red
    note: "YouTube Premium nag"
  - kind: enable
    package: com.android.bookmarkprovider
"#;

    #[test]
    fn parses_and_lints_clean() {
        let p: Profile = serde_yaml_ng::from_str(GOOD).unwrap();
        assert_eq!(p.actions.len(), 2);
        assert!(lint(&p).is_empty());
    }

    #[test]
    fn refuses_unsupported_version() {
        let bad = GOOD.replace("version: \"1\"", "version: \"2\"");
        let p: Profile = serde_yaml_ng::from_str(&bad).unwrap();
        let issues = lint(&p);
        assert!(issues
            .iter()
            .any(|i| i.contains("unsupported profile version")));
        assert!(issues.iter().any(|i| i.contains("migration path")));
    }

    #[test]
    fn flags_invalid_package_id() {
        let bad = r#"
name: "x"
version: "1"
actions:
  - kind: disable
    package: ".bad"
"#;
        let p: Profile = serde_yaml_ng::from_str(bad).unwrap();
        let issues = lint(&p);
        assert!(issues.iter().any(|i| i.contains("invalid package id")));
    }

    #[test]
    fn rejects_operation_kinds_outside_profile_v1() {
        let profile: Profile =
            serde_yaml_ng::from_str(&GOOD.replace("kind: disable", "kind: shell")).unwrap();
        assert!(lint(&profile)
            .iter()
            .any(|issue| issue.contains("not supported by profile schema v1")));
    }

    #[test]
    fn requests_for_attaches_serial() {
        let p: Profile = serde_yaml_ng::from_str(GOOD).unwrap();
        let target = DeviceTarget {
            serial: "abc-123".into(),
            transport_id: Some(4),
            connection_generation: 5,
            model: None,
            product: None,
            device: None,
            build_fingerprint: Some("build/test".into()),
        };
        let rs = requests_for(&p, &target);
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[0].serial, "abc-123");
        assert_eq!(rs[0].target, target);
        assert_eq!(rs[0].kind, ActionKind::Disable);
        assert_eq!(rs[1].kind, ActionKind::Enable);
    }

    #[test]
    fn device_match_checks_serial_prefix_and_manufacturer() {
        let p: Profile = serde_yaml_ng::from_str(
            r#"
name: "x"
version: "1"
device:
  require_serial_prefix: "ABC"
  require_manufacturer: "Google"
actions:
  - kind: disable
    package: com.x
"#,
        )
        .unwrap();

        assert!(serial_match_issues(&p, "ABC123").is_empty());
        assert!(!serial_match_issues(&p, "XYZ123").is_empty());
        assert!(manufacturer_match_issues(&p, Some("google")).is_empty());
        assert!(!manufacturer_match_issues(&p, Some("Samsung")).is_empty());
        assert!(!manufacturer_match_issues(&p, None).is_empty());
    }

    #[test]
    fn load_round_trips_through_tempfile() {
        let dir = std::env::temp_dir().join("droidsmith-profile-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("good.yaml");
        std::fs::write(&path, GOOD).unwrap();
        let p = load(&path).unwrap();
        assert_eq!(p.actions.len(), 2);
    }
}
