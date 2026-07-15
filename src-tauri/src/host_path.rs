use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const DEFAULT_GRANT_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_ACTIVE_GRANTS: usize = 64;
/// How many recently-produced artifact paths stay eligible for a "reveal in
/// folder" request. Bounded so a long session cannot grow the set without limit.
const MAX_REVEALABLE_ARTIFACTS: usize = 64;

/// A native-dialog purpose is also the authorization scope. Read grants can
/// never reach write commands (or another read command), and vice versa.
#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPathPurpose {
    DiagnosticsSave,
    BugreportSave,
    ScrcpyRecordSave,
    LogcatSave,
    PackageExportSave,
    BackupSave,
    ScreenshotSave,
    PullSave,
    ExtractApkSave,
    RecoveryBaselineSave,
    ProfileSave,
    PushOpen,
    InstallOpen,
    RecoveryBaselineOpen,
    ProfileOpen,
}

impl HostPathPurpose {
    pub const fn is_write(self) -> bool {
        matches!(
            self,
            Self::DiagnosticsSave
                | Self::BugreportSave
                | Self::ScrcpyRecordSave
                | Self::LogcatSave
                | Self::PackageExportSave
                | Self::BackupSave
                | Self::ScreenshotSave
                | Self::PullSave
                | Self::ExtractApkSave
                | Self::RecoveryBaselineSave
                | Self::ProfileSave
        )
    }

    pub const fn dialog_title(self) -> &'static str {
        match self {
            Self::DiagnosticsSave => "Save Droidsmith support bundle",
            Self::BugreportSave => "Save sensitive Android bugreport",
            Self::ScrcpyRecordSave => "Save scrcpy recording",
            Self::LogcatSave => "Export Logcat",
            Self::PackageExportSave => "Export package APKs",
            Self::BackupSave => "Save advanced legacy data export",
            Self::ScreenshotSave => "Save screenshot",
            Self::PullSave => "Save device file",
            Self::ExtractApkSave => "Save extracted APK",
            Self::RecoveryBaselineSave => "Export recovery baseline",
            Self::ProfileSave => "Export Droidsmith profile",
            Self::PushOpen => "Choose file to push",
            Self::InstallOpen => "Choose Android package",
            Self::RecoveryBaselineOpen => "Inspect recovery baseline",
            Self::ProfileOpen => "Import Droidsmith profile",
        }
    }

    pub const fn filter(self) -> Option<(&'static str, &'static [&'static str])> {
        match self {
            Self::DiagnosticsSave => Some(("JSON", &["json"])),
            Self::BugreportSave => Some(("Android bugreport", &["zip"])),
            Self::ScrcpyRecordSave => Some(("scrcpy recording", &["mp4", "mkv"])),
            Self::LogcatSave => Some(("Logcat", &["log", "txt"])),
            Self::PackageExportSave => Some(("Droidsmith package export", &["zip"])),
            Self::BackupSave => Some(("Droidsmith legacy data export", &["zip"])),
            Self::ScreenshotSave => Some(("PNG", &["png"])),
            Self::ExtractApkSave => Some(("APK", &["apk"])),
            Self::RecoveryBaselineSave | Self::RecoveryBaselineOpen => {
                Some(("Droidsmith recovery baseline", &["json"]))
            }
            Self::ProfileSave | Self::ProfileOpen => Some(("Droidsmith profile", &["yaml", "yml"])),
            Self::InstallOpen => Some(("Android package", &["apk", "apks", "xapk", "apkm"])),
            Self::PullSave | Self::PushOpen => None,
        }
    }
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// Display paths of artifacts Droidsmith has written this session, kept so a
    /// later "reveal in folder" request can be authorized against a path the app
    /// itself produced instead of an arbitrary renderer-supplied path.
    revealable: Mutex<VecDeque<String>>,
    ttl: Duration,
}

impl Default for PathGrantStore {
    fn default() -> Self {
        Self {
            grants: Mutex::new(HashMap::new()),
            revealable: Mutex::new(VecDeque::new()),
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
        drop(grants);
        let local_path = crate::fs_util::display_path(&path);
        // Save-dialog destinations are the only paths a reveal request may later
        // target. Read grants (file pickers) never become revealable.
        if purpose.is_write() {
            self.record_revealable(&local_path);
        }
        Ok(HostPathGrant {
            id,
            purpose,
            local_path,
        })
    }

    fn record_revealable(&self, local_path: &str) {
        let Ok(mut revealable) = self.revealable.lock() else {
            return;
        };
        if revealable.iter().any(|existing| existing == local_path) {
            return;
        }
        if revealable.len() >= MAX_REVEALABLE_ARTIFACTS {
            revealable.pop_front();
        }
        revealable.push_back(local_path.to_string());
    }

    /// True only for a path Droidsmith itself wrote via a save-dialog grant this
    /// session. Everything else — including arbitrary renderer-supplied paths —
    /// is rejected, so the renderer can never drive an open of an unrelated path.
    pub fn is_revealable(&self, local_path: &str) -> bool {
        self.revealable
            .lock()
            .map(|revealable| revealable.iter().any(|existing| existing == local_path))
            .unwrap_or(false)
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

/// Resolve the OS file-manager invocation that reveals `path`. Windows and
/// macOS select the file inside its folder; other platforms open the containing
/// directory (no portable "select" primitive exists). Kept pure so the argv is
/// unit-tested without spawning a real file manager.
pub fn reveal_command(path: &Path) -> (String, Vec<String>) {
    if cfg!(target_os = "windows") {
        (
            "explorer.exe".to_string(),
            vec![format!("/select,{}", path.display())],
        )
    } else if cfg!(target_os = "macos") {
        (
            "open".to_string(),
            vec!["-R".to_string(), path.display().to_string()],
        )
    } else {
        let target = path.parent().unwrap_or(path);
        ("xdg-open".to_string(), vec![target.display().to_string()])
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
            revealable: Mutex::new(VecDeque::new()),
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
    fn reveal_command_targets_the_produced_artifact() {
        let path = if cfg!(target_os = "windows") {
            PathBuf::from(r"C:\Users\tester\export.zip")
        } else {
            PathBuf::from("/home/tester/export.zip")
        };
        let (program, args) = reveal_command(&path);
        if cfg!(target_os = "windows") {
            assert_eq!(program, "explorer.exe");
            assert_eq!(args, vec![format!("/select,{}", path.display())]);
        } else if cfg!(target_os = "macos") {
            assert_eq!(program, "open");
            assert_eq!(args, vec!["-R".to_string(), path.display().to_string()]);
        } else {
            assert_eq!(program, "xdg-open");
            assert_eq!(args, vec!["/home/tester".to_string()]);
        }
    }

    #[test]
    fn only_written_artifacts_become_revealable() {
        let dir = test_dir("reveal");
        let saved = dir.join("export.zip");
        let store = PathGrantStore::default();

        // A picked (read) source never becomes revealable.
        let source = dir.join("input.apk");
        fs::write(&source, b"apk").unwrap();
        store.issue(&source, HostPathPurpose::InstallOpen).unwrap();
        assert!(!store.is_revealable(&crate::fs_util::display_path(&source)));

        // A save destination is revealable by the exact display path returned.
        let grant = store
            .issue(&saved, HostPathPurpose::PackageExportSave)
            .unwrap();
        assert!(store.is_revealable(&grant.local_path));
        // An arbitrary renderer-supplied path is rejected.
        assert!(!store.is_revealable("/etc/passwd"));
        assert!(!store.is_revealable(&crate::fs_util::display_path(&dir.join("other.zip"))));
    }

    #[test]
    fn revealable_registry_is_bounded_and_deduplicated() {
        // Exercise the registry directly: `issue` also consumes the separate
        // one-shot grant map, whose own cap is unrelated to this bound.
        let store = PathGrantStore::default();
        store.record_revealable("/artifacts/a.zip");
        store.record_revealable("/artifacts/a.zip");
        assert_eq!(store.revealable.lock().unwrap().len(), 1);
        assert!(store.is_revealable("/artifacts/a.zip"));

        for index in 0..MAX_REVEALABLE_ARTIFACTS {
            store.record_revealable(&format!("/artifacts/fill-{index}.zip"));
        }
        assert_eq!(
            store.revealable.lock().unwrap().len(),
            MAX_REVEALABLE_ARTIFACTS
        );
        // The oldest entry was evicted once the bound was exceeded.
        assert!(!store.is_revealable("/artifacts/a.zip"));
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
