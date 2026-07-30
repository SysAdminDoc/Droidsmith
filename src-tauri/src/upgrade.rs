//! Isolated public-release upgrade preservation verification.
//!
//! This module never resolves or opens the real application data directory.
//! Callers must provide an immutable fixture and a new, empty scratch
//! directory. The verifier installs the fixture atomically, takes a complete
//! pre-upgrade backup, opens every persisted format through current loaders,
//! proves a second pass is byte-idempotent, and restores the backup exactly.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::journal::Journal;
use crate::profile::ProfileDocument;

const FIXTURE_MANIFEST: &str = "fixture.json";
const SUPPORTED_FIXTURE_SCHEMA: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpgradeFixture {
    schema_version: u32,
    source_version: String,
    journals: Vec<JournalExpectation>,
    profiles: Vec<ProfileExpectation>,
    recovery_baselines: Vec<BaselineExpectation>,
    future_settings_store: String,
    future_profile: String,
    future_recovery_baseline: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalExpectation {
    serial: String,
    expected_entries: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileExpectation {
    path: String,
    kind: ProfileKind,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProfileKind {
    Current,
    MigrationAvailable,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BaselineExpectation {
    path: String,
    expected_packages: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeCheckReport {
    pub source_version: String,
    pub settings_version: String,
    pub journal_entries: usize,
    pub profiles_checked: usize,
    pub recovery_baselines_checked: usize,
    pub future_versions_rejected: bool,
    pub idempotent: bool,
    pub backup_restored_byte_exact: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UpgradeCheckError {
    #[error("upgrade fixture I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("upgrade fixture is invalid: {0}")]
    Invalid(String),
    #[error("upgrade fixture validation failed: {0}")]
    Validation(String),
}

/// Verify one historical fixture entirely below `scratch_root`.
///
/// `scratch_root` must be empty and outside `fixture_root`. This explicit
/// boundary prevents a release check from ever consulting or mutating the
/// user's platform-specific Droidsmith data directory.
pub fn verify_upgrade_fixture(
    fixture_root: &Path,
    scratch_root: &Path,
) -> Result<UpgradeCheckReport, UpgradeCheckError> {
    let fixture_root = fixture_root.canonicalize()?;
    validate_empty_scratch(&fixture_root, scratch_root)?;
    fs::create_dir_all(scratch_root)?;

    let fixture: UpgradeFixture =
        serde_json::from_slice(&fs::read(fixture_root.join(FIXTURE_MANIFEST))?)
            .map_err(|error| UpgradeCheckError::Invalid(error.to_string()))?;
    if fixture.schema_version != SUPPORTED_FIXTURE_SCHEMA {
        return Err(UpgradeCheckError::Invalid(format!(
            "unsupported fixture schema {}",
            fixture.schema_version
        )));
    }
    if fixture.source_version.trim().is_empty() {
        return Err(UpgradeCheckError::Invalid(
            "sourceVersion is required".to_string(),
        ));
    }

    let active = scratch_root.join("active");
    let backup = scratch_root.join("pre-migration-backup");
    replace_tree_atomically(&fixture_root, &active)?;
    replace_tree_atomically(&active, &backup)?;
    let source_before = snapshot_tree(&fixture_root)?;
    let backup_before = snapshot_tree(&backup)?;
    let active_before = snapshot_tree(&active)?;
    if active_before != source_before || backup_before != source_before {
        return Err(UpgradeCheckError::Validation(
            "fixture install or pre-migration backup changed bytes".to_string(),
        ));
    }

    let first = validate_installed_fixture(&active, &fixture)?;
    let after_first = snapshot_tree(&active)?;
    let second = validate_installed_fixture(&active, &fixture)?;
    let after_second = snapshot_tree(&active)?;
    if first != second || after_first != after_second {
        return Err(UpgradeCheckError::Validation(
            "upgrade validation is not idempotent".to_string(),
        ));
    }

    assert_future_data_fails_closed(&active, &fixture)?;
    replace_tree_atomically(&backup, &active)?;
    let restored = snapshot_tree(&active)?;
    if restored != backup_before {
        return Err(UpgradeCheckError::Validation(
            "pre-migration backup did not restore byte-equivalent state".to_string(),
        ));
    }
    if snapshot_tree(&fixture_root)? != source_before {
        return Err(UpgradeCheckError::Validation(
            "immutable source fixture was modified".to_string(),
        ));
    }

    Ok(UpgradeCheckReport {
        source_version: fixture.source_version,
        settings_version: first.settings_version,
        journal_entries: first.journal_entries,
        profiles_checked: fixture.profiles.len(),
        recovery_baselines_checked: fixture.recovery_baselines.len(),
        future_versions_rejected: true,
        idempotent: true,
        backup_restored_byte_exact: true,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct ValidationSummary {
    settings_version: String,
    journal_entries: usize,
}

fn validate_installed_fixture(
    active: &Path,
    fixture: &UpgradeFixture,
) -> Result<ValidationSummary, UpgradeCheckError> {
    let settings =
        crate::settings::initialize(active, crate::settings::LegacySettingsImport::default())
            .map_err(|error| UpgradeCheckError::Validation(error.to_string()))?;

    let journal_dir = active.join("journal");
    let mut journal_entries = 0usize;
    for expected in &fixture.journals {
        let journal = Journal::open(&journal_dir, &expected.serial)?;
        if journal.entries().len() != expected.expected_entries {
            return Err(UpgradeCheckError::Validation(format!(
                "journal {:?} has {} entries; expected {}",
                expected.serial,
                journal.entries().len(),
                expected.expected_entries
            )));
        }
        journal_entries = journal_entries.saturating_add(journal.entries().len());
    }

    for expected in &fixture.profiles {
        let path = checked_fixture_path(active, &expected.path)?;
        let document = crate::profile::inspect(&path)
            .map_err(|error| UpgradeCheckError::Validation(error.to_string()))?;
        let matches = matches!(
            (expected.kind, document),
            (ProfileKind::Current, ProfileDocument::Current { .. })
                | (
                    ProfileKind::MigrationAvailable,
                    ProfileDocument::MigrationAvailable { .. }
                )
        );
        if !matches {
            return Err(UpgradeCheckError::Validation(format!(
                "profile {} produced an unexpected compatibility state",
                expected.path
            )));
        }
    }

    for expected in &fixture.recovery_baselines {
        let path = checked_fixture_path(active, &expected.path)?;
        let baseline = crate::recovery_baseline::load(&path)
            .map_err(|error| UpgradeCheckError::Validation(error.to_string()))?;
        if baseline.packages.len() != expected.expected_packages {
            return Err(UpgradeCheckError::Validation(format!(
                "recovery baseline {} has {} packages; expected {}",
                expected.path,
                baseline.packages.len(),
                expected.expected_packages
            )));
        }
    }

    Ok(ValidationSummary {
        settings_version: settings.settings.version,
        journal_entries,
    })
}

fn assert_future_data_fails_closed(
    active: &Path,
    fixture: &UpgradeFixture,
) -> Result<(), UpgradeCheckError> {
    let settings_store = checked_fixture_path(active, &fixture.future_settings_store)?;
    let before = snapshot_tree(&settings_store)?;
    if crate::settings::initialize(
        &settings_store,
        crate::settings::LegacySettingsImport::default(),
    )
    .is_ok()
    {
        return Err(UpgradeCheckError::Validation(
            "future settings version was accepted".to_string(),
        ));
    }
    if snapshot_tree(&settings_store)? != before {
        return Err(UpgradeCheckError::Validation(
            "future settings rejection mutated the store".to_string(),
        ));
    }

    let future_profile = checked_fixture_path(active, &fixture.future_profile)?;
    let before = fs::read(&future_profile)?;
    if crate::profile::inspect(&future_profile).is_ok() || fs::read(&future_profile)? != before {
        return Err(UpgradeCheckError::Validation(
            "future profile did not fail closed".to_string(),
        ));
    }

    let future_baseline = checked_fixture_path(active, &fixture.future_recovery_baseline)?;
    let before = fs::read(&future_baseline)?;
    if crate::recovery_baseline::load(&future_baseline).is_ok()
        || fs::read(&future_baseline)? != before
    {
        return Err(UpgradeCheckError::Validation(
            "future recovery baseline did not fail closed".to_string(),
        ));
    }
    Ok(())
}

fn validate_empty_scratch(
    fixture_root: &Path,
    scratch_root: &Path,
) -> Result<(), UpgradeCheckError> {
    let scratch_absolute = if scratch_root.is_absolute() {
        scratch_root.to_path_buf()
    } else {
        std::env::current_dir()?.join(scratch_root)
    };
    if scratch_absolute.starts_with(fixture_root) {
        return Err(UpgradeCheckError::Invalid(
            "scratch directory must be outside the fixture".to_string(),
        ));
    }
    if scratch_root.exists() && fs::read_dir(scratch_root)?.next().is_some() {
        return Err(UpgradeCheckError::Invalid(
            "scratch directory must be empty".to_string(),
        ));
    }
    Ok(())
}

fn checked_fixture_path(root: &Path, relative: &str) -> Result<PathBuf, UpgradeCheckError> {
    let candidate = Path::new(relative);
    if candidate.is_absolute()
        || candidate
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(UpgradeCheckError::Invalid(format!(
            "fixture path must be a clean relative path: {relative:?}"
        )));
    }
    Ok(root.join(candidate))
}

fn replace_tree_atomically(source: &Path, destination: &Path) -> Result<(), UpgradeCheckError> {
    if !source.is_dir() {
        return Err(UpgradeCheckError::Invalid(format!(
            "upgrade source is not a directory: {}",
            source.display()
        )));
    }
    let parent = destination.parent().ok_or_else(|| {
        UpgradeCheckError::Invalid("upgrade destination has no parent".to_string())
    })?;
    fs::create_dir_all(parent)?;
    let token = uuid::Uuid::new_v4();
    let stage = parent.join(format!(".droidsmith-upgrade-stage-{token}"));
    let retired = parent.join(format!(".droidsmith-upgrade-retired-{token}"));
    copy_tree(source, &stage)?;

    let had_destination = destination.exists();
    if had_destination {
        fs::rename(destination, &retired)?;
    }
    if let Err(error) = fs::rename(&stage, destination) {
        if had_destination {
            let _ = fs::rename(&retired, destination);
        }
        let _ = fs::remove_dir_all(&stage);
        return Err(error.into());
    }
    if had_destination {
        fs::remove_dir_all(retired)?;
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), UpgradeCheckError> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(UpgradeCheckError::Invalid(format!(
                "fixture symlinks are not allowed: {}",
                source_path.display()
            )));
        }
        if metadata.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn snapshot_tree(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, UpgradeCheckError> {
    let mut snapshot = BTreeMap::new();
    snapshot_tree_inner(root, root, &mut snapshot)?;
    Ok(snapshot)
}

fn snapshot_tree_inner(
    root: &Path,
    current: &Path,
    snapshot: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), UpgradeCheckError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(UpgradeCheckError::Invalid(format!(
                "fixture symlinks are not allowed: {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            snapshot_tree_inner(root, &path, snapshot)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| UpgradeCheckError::Invalid(error.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            snapshot.insert(relative, fs::read(path)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "droidsmith-upgrade-{name}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn public_v053_fixture_is_idempotent_and_byte_restorable() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("upgrade")
            .join("v0.5.3");
        let scratch = temp_dir("v053");
        let report = verify_upgrade_fixture(&fixture, &scratch).unwrap();
        assert_eq!(report.source_version, "0.5.3");
        assert_eq!(report.settings_version, crate::settings::SETTINGS_VERSION);
        assert_eq!(report.journal_entries, 1);
        assert_eq!(report.profiles_checked, 2);
        assert_eq!(report.recovery_baselines_checked, 1);
        assert!(report.future_versions_rejected);
        assert!(report.idempotent);
        assert!(report.backup_restored_byte_exact);
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn failed_atomic_install_preserves_an_existing_destination() {
        let root = temp_dir("atomic-failure");
        let destination = root.join("active");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("marker"), b"preserve me").unwrap();
        assert!(replace_tree_atomically(&root.join("missing"), &destination).is_err());
        assert_eq!(
            fs::read(destination.join("marker")).unwrap(),
            b"preserve me"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
