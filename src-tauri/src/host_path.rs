use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const DEFAULT_GRANT_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_ACTIVE_GRANTS: usize = 64;

/// A native-dialog purpose is also the authorization scope. Read grants can
/// never reach write commands (or another read command), and vice versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPathPurpose {
    DiagnosticsSave,
    LogcatSave,
    BackupSave,
    ScreenshotSave,
    PullSave,
    ExtractApkSave,
    RecoveryBaselineSave,
    PushOpen,
    InstallOpen,
    RecoveryBaselineOpen,
}

impl HostPathPurpose {
    pub const fn is_write(self) -> bool {
        matches!(
            self,
            Self::DiagnosticsSave
                | Self::LogcatSave
                | Self::BackupSave
                | Self::ScreenshotSave
                | Self::PullSave
                | Self::ExtractApkSave
                | Self::RecoveryBaselineSave
        )
    }

    pub const fn dialog_title(self) -> &'static str {
        match self {
            Self::DiagnosticsSave => "Save Droidsmith support bundle",
            Self::LogcatSave => "Export Logcat",
            Self::BackupSave => "Save Android backup",
            Self::ScreenshotSave => "Save screenshot",
            Self::PullSave => "Save device file",
            Self::ExtractApkSave => "Save extracted APK",
            Self::RecoveryBaselineSave => "Export recovery baseline",
            Self::PushOpen => "Choose file to push",
            Self::InstallOpen => "Choose Android package",
            Self::RecoveryBaselineOpen => "Inspect recovery baseline",
        }
    }

    pub const fn filter(self) -> Option<(&'static str, &'static [&'static str])> {
        match self {
            Self::DiagnosticsSave => Some(("JSON", &["json"])),
            Self::LogcatSave => Some(("Logcat", &["log", "txt"])),
            Self::BackupSave => Some(("Android backup", &["ab"])),
            Self::ScreenshotSave => Some(("PNG", &["png"])),
            Self::ExtractApkSave => Some(("APK", &["apk"])),
            Self::RecoveryBaselineSave | Self::RecoveryBaselineOpen => {
                Some(("Droidsmith recovery baseline", &["json"]))
            }
            Self::InstallOpen => Some(("Android package", &["apk", "apks", "xapk", "apkm"])),
            Self::PullSave | Self::PushOpen => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostPathGrant {
    pub id: String,
    pub purpose: HostPathPurpose,
    pub local_path: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PathGrantError {
    #[error("invalid native-dialog path: {0}")]
    InvalidPath(String),
    #[error("path grant is missing, expired, or already used")]
    Missing,
    #[error("path grant is expired")]
    Expired,
    #[error("path grant is scoped to a different operation")]
    WrongPurpose,
    #[error("too many native-dialog path grants are pending")]
    RegistryFull,
    #[error("path grant registry is unavailable")]
    RegistryUnavailable,
}

impl PathGrantError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidPath(_) => "path_grant_invalid_path",
            Self::Missing => "path_grant_missing",
            Self::Expired => "path_grant_expired",
            Self::WrongPurpose => "path_grant_wrong_purpose",
            Self::RegistryFull => "path_grant_registry_full",
            Self::RegistryUnavailable => "path_grant_registry_unavailable",
        }
    }
}

#[derive(Debug)]
struct GrantEntry {
    purpose: HostPathPurpose,
    path: PathBuf,
    expires_at: Instant,
}

#[derive(Debug)]
pub struct PathGrantStore {
    grants: Mutex<HashMap<String, GrantEntry>>,
    ttl: Duration,
}

impl Default for PathGrantStore {
    fn default() -> Self {
        Self {
            grants: Mutex::new(HashMap::new()),
            ttl: DEFAULT_GRANT_TTL,
        }
    }
}

impl PathGrantStore {
    /// Record a path that came directly from the backend-owned native dialog.
    pub fn issue(
        &self,
        selected_path: &Path,
        purpose: HostPathPurpose,
    ) -> Result<HostPathGrant, PathGrantError> {
        self.issue_at(selected_path, purpose, Instant::now())
    }

    fn issue_at(
        &self,
        selected_path: &Path,
        purpose: HostPathPurpose,
        now: Instant,
    ) -> Result<HostPathGrant, PathGrantError> {
        let path = normalize_selected_path(selected_path, purpose)?;
        let mut grants = self
            .grants
            .lock()
            .map_err(|_| PathGrantError::RegistryUnavailable)?;
        grants.retain(|_, entry| entry.expires_at > now);
        if grants.len() >= MAX_ACTIVE_GRANTS {
            return Err(PathGrantError::RegistryFull);
        }

        let id = uuid::Uuid::new_v4().to_string();
        grants.insert(
            id.clone(),
            GrantEntry {
                purpose,
                path: path.clone(),
                expires_at: now + self.ttl,
            },
        );
        Ok(HostPathGrant {
            id,
            purpose,
            local_path: crate::fs_util::display_path(&path),
        })
    }

    /// Consume exactly one matching grant. Wrong-purpose probes do not burn a
    /// legitimate grant, while successful consumption removes it before any
    /// filesystem or device operation can begin.
    pub fn consume(&self, id: &str, purpose: HostPathPurpose) -> Result<PathBuf, PathGrantError> {
        self.consume_at(id, purpose, Instant::now())
    }

    fn consume_at(
        &self,
        id: &str,
        purpose: HostPathPurpose,
        now: Instant,
    ) -> Result<PathBuf, PathGrantError> {
        if id.is_empty() || id.len() > 64 {
            return Err(PathGrantError::Missing);
        }
        let mut grants = self
            .grants
            .lock()
            .map_err(|_| PathGrantError::RegistryUnavailable)?;
        let Some(entry) = grants.get(id) else {
            return Err(PathGrantError::Missing);
        };
        if entry.expires_at <= now {
            grants.remove(id);
            return Err(PathGrantError::Expired);
        }
        if entry.purpose != purpose {
            return Err(PathGrantError::WrongPurpose);
        }
        Ok(grants.remove(id).expect("grant exists above").path)
    }
}

pub fn validate_suggested_file_name(
    name: Option<String>,
) -> Result<Option<String>, PathGrantError> {
    let Some(name) = name else {
        return Ok(None);
    };
    if name.is_empty()
        || name.len() > 255
        || name != name.trim()
        || name.chars().any(char::is_control)
        || name.contains(['/', '\\'])
        || matches!(name.as_str(), "." | "..")
    {
        return Err(PathGrantError::InvalidPath(
            "suggested file name must be a single normalized component".to_string(),
        ));
    }
    Ok(Some(name))
}

fn normalize_selected_path(
    selected_path: &Path,
    purpose: HostPathPurpose,
) -> Result<PathBuf, PathGrantError> {
    if !selected_path.is_absolute()
        || selected_path
            .as_os_str()
            .to_string_lossy()
            .chars()
            .any(char::is_control)
        || selected_path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(PathGrantError::InvalidPath(
            "selected path must be normalized and absolute".to_string(),
        ));
    }

    if !purpose.is_write() {
        let canonical = fs::canonicalize(selected_path)
            .map_err(|error| PathGrantError::InvalidPath(error.to_string()))?;
        if !canonical.is_file() {
            return Err(PathGrantError::InvalidPath(
                "selected read path is not a regular file".to_string(),
            ));
        }
        return Ok(canonical);
    }

    if fs::symlink_metadata(selected_path)
        .is_ok_and(|metadata| metadata.file_type().is_symlink() || metadata.is_dir())
    {
        return Err(PathGrantError::InvalidPath(
            "selected write path must not be a directory or symbolic link".to_string(),
        ));
    }
    let parent = selected_path.parent().ok_or_else(|| {
        PathGrantError::InvalidPath("selected write path has no parent".to_string())
    })?;
    let file_name = selected_path.file_name().ok_or_else(|| {
        PathGrantError::InvalidPath("selected write path has no file name".to_string())
    })?;
    let canonical_parent =
        fs::canonicalize(parent).map_err(|error| PathGrantError::InvalidPath(error.to_string()))?;
    if !canonical_parent.is_dir() {
        return Err(PathGrantError::InvalidPath(
            "selected write parent is not a directory".to_string(),
        ));
    }
    Ok(canonical_parent.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "droidsmith-path-grant-{name}-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn direct_use_without_a_grant_fails_closed() {
        let store = PathGrantStore::default();
        let error = store
            .consume("not-a-real-grant", HostPathPurpose::InstallOpen)
            .unwrap_err();
        assert_eq!(error.code(), "path_grant_missing");
    }

    #[test]
    fn grant_is_purpose_scoped_and_one_shot() {
        let dir = test_dir("one-shot");
        let source = dir.join("package.apk");
        fs::write(&source, b"package").unwrap();
        let store = PathGrantStore::default();
        let grant = store.issue(&source, HostPathPurpose::InstallOpen).unwrap();

        let wrong = store
            .consume(&grant.id, HostPathPurpose::PushOpen)
            .unwrap_err();
        assert_eq!(wrong.code(), "path_grant_wrong_purpose");
        assert_eq!(
            store
                .consume(&grant.id, HostPathPurpose::InstallOpen)
                .unwrap(),
            fs::canonicalize(&source).unwrap()
        );
        assert_eq!(
            store
                .consume(&grant.id, HostPathPurpose::InstallOpen)
                .unwrap_err()
                .code(),
            "path_grant_missing"
        );
    }

    #[test]
    fn grants_expire_and_are_removed() {
        let dir = test_dir("expiry");
        let destination = dir.join("capture.png");
        let store = PathGrantStore {
            grants: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(5),
        };
        let issued_at = Instant::now();
        let grant = store
            .issue_at(&destination, HostPathPurpose::ScreenshotSave, issued_at)
            .unwrap();
        let error = store
            .consume_at(
                &grant.id,
                HostPathPurpose::ScreenshotSave,
                issued_at + Duration::from_secs(6),
            )
            .unwrap_err();
        assert_eq!(error.code(), "path_grant_expired");
        assert_eq!(
            store
                .consume_at(
                    &grant.id,
                    HostPathPurpose::ScreenshotSave,
                    issued_at + Duration::from_secs(6)
                )
                .unwrap_err()
                .code(),
            "path_grant_missing"
        );
    }

    #[test]
    fn read_and_write_intents_validate_different_filesystem_states() {
        let dir = test_dir("intent");
        let missing = dir.join("new.log");
        let store = PathGrantStore::default();
        assert!(store.issue(&missing, HostPathPurpose::LogcatSave).is_ok());
        assert_eq!(
            store
                .issue(&missing, HostPathPurpose::PushOpen)
                .unwrap_err()
                .code(),
            "path_grant_invalid_path"
        );

        fs::write(&missing, b"content").unwrap();
        assert!(store.issue(&missing, HostPathPurpose::PushOpen).is_ok());
    }

    #[test]
    fn suggested_names_cannot_escape_the_dialog_directory() {
        assert_eq!(
            validate_suggested_file_name(Some("capture.png".to_string())).unwrap(),
            Some("capture.png".to_string())
        );
        for invalid in ["", ".", "..", "../escape", "sub/file", "sub\\file"] {
            assert_eq!(
                validate_suggested_file_name(Some(invalid.to_string()))
                    .unwrap_err()
                    .code(),
                "path_grant_invalid_path"
            );
        }
    }
}
