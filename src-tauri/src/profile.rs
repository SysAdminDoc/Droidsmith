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

use crate::adb::actions::{ActionKind, ActionRequest};

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
        source: serde_yml::Error,
    },
    #[error("profile {path} failed validation: {reasons}")]
    Validate {
        path: std::path::PathBuf,
        reasons: String,
    },
}

pub fn load(path: &std::path::Path) -> Result<Profile, ProfileError> {
    let text = std::fs::read_to_string(path).map_err(|source| ProfileError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let profile: Profile = serde_yml::from_str(&text).map_err(|source| ProfileError::Parse {
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
    if p.version != "1" {
        issues.push(format!(
            "unsupported profile version {:?} (this build expects \"1\")",
            p.version
        ));
    }
    if p.actions.is_empty() {
        issues.push("profile has no actions".to_string());
    }
    for (i, action) in p.actions.iter().enumerate() {
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
pub fn requests_for(profile: &Profile, serial: &str) -> Vec<ActionRequest> {
    profile
        .actions
        .iter()
        .map(|a| ActionRequest {
            serial: serial.to_string(),
            package: a.package.clone(),
            kind: a.kind,
        })
        .collect()
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
        let p: Profile = serde_yml::from_str(GOOD).unwrap();
        assert_eq!(p.actions.len(), 2);
        assert!(lint(&p).is_empty());
    }

    #[test]
    fn refuses_unsupported_version() {
        let bad = GOOD.replace("version: \"1\"", "version: \"2\"");
        let p: Profile = serde_yml::from_str(&bad).unwrap();
        let issues = lint(&p);
        assert!(issues
            .iter()
            .any(|i| i.contains("unsupported profile version")));
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
        let p: Profile = serde_yml::from_str(bad).unwrap();
        let issues = lint(&p);
        assert!(issues.iter().any(|i| i.contains("invalid package id")));
    }

    #[test]
    fn requests_for_attaches_serial() {
        let p: Profile = serde_yml::from_str(GOOD).unwrap();
        let rs = requests_for(&p, "abc-123");
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[0].serial, "abc-123");
        assert_eq!(rs[0].kind, ActionKind::Disable);
        assert_eq!(rs[1].kind, ActionKind::Enable);
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
