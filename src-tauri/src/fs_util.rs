use std::fs::{self, File, OpenOptions};
use std::io::{Error, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Metadata returned to the renderer only after a host artifact has passed
/// validation and been atomically installed at its final path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostArtifact {
    pub local_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// Arbitrary `adb pull` output. Empty device files are valid.
    AnyFile,
    Png,
    Apk,
    AndroidBackup,
}

#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("could not create an atomic staging file: {0}")]
    Stage(#[source] std::io::Error),
    #[error("host artifact failed validation: {0}")]
    Invalid(String),
    #[error("could not flush the staged host artifact: {0}")]
    Flush(#[source] std::io::Error),
    #[error("could not atomically install the host artifact: {0}")]
    Commit(#[source] std::io::Error),
}

impl ArtifactError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Stage(_) => "artifact_stage_failed",
            Self::Invalid(_) => "artifact_invalid",
            Self::Flush(_) => "artifact_flush_failed",
            Self::Commit(_) => "artifact_commit_failed",
        }
    }
}

/// A sibling temporary output that is removed unless [`Self::commit`] reaches
/// the final atomic rename. Keeping it beside the destination guarantees the
/// rename cannot cross filesystem boundaries.
#[derive(Debug)]
pub struct StagedArtifact {
    final_path: PathBuf,
    stage_path: PathBuf,
    committed: bool,
}

impl StagedArtifact {
    pub fn new(final_path: &Path) -> Result<Self, ArtifactError> {
        if final_path.is_dir() {
            return Err(ArtifactError::Invalid(format!(
                "destination is a directory: {}",
                final_path.display()
            )));
        }
        let parent = final_path.parent().ok_or_else(|| {
            ArtifactError::Invalid(format!(
                "destination has no parent directory: {}",
                final_path.display()
            ))
        })?;
        if !parent.is_dir() {
            return Err(ArtifactError::Invalid(format!(
                "destination parent does not exist: {}",
                parent.display()
            )));
        }
        final_path.file_name().ok_or_else(|| {
            ArtifactError::Invalid(format!(
                "destination has no file name: {}",
                final_path.display()
            ))
        })?;

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        for _ in 0..64 {
            let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
            let stage_name = format!(".droidsmith-{}-{sequence}.partial", std::process::id());
            let stage_path = parent.join(stage_name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&stage_path)
            {
                Ok(_) => {
                    return Ok(Self {
                        final_path: final_path.to_path_buf(),
                        stage_path,
                        committed: false,
                    });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(ArtifactError::Stage(error)),
            }
        }

        Err(ArtifactError::Stage(Error::new(
            ErrorKind::AlreadyExists,
            "could not allocate a unique sibling staging path",
        )))
    }

    pub fn path(&self) -> &Path {
        &self.stage_path
    }

    pub fn commit(mut self, kind: ArtifactKind) -> Result<HostArtifact, ArtifactError> {
        let metadata = fs::symlink_metadata(&self.stage_path).map_err(ArtifactError::Stage)?;
        if !metadata.is_file() {
            return Err(ArtifactError::Invalid(
                "staged output is not a regular file".to_string(),
            ));
        }

        validate_artifact(&self.stage_path, metadata.len(), kind)?;
        let sha256 = sha256_file(&self.stage_path).map_err(ArtifactError::Flush)?;

        // The producer has closed its handle before commit. Flush the exact
        // staged inode before making it visible at the final destination.
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.stage_path)
            .and_then(|file| file.sync_all())
            .map_err(ArtifactError::Flush)?;

        let canonical_path =
            canonical_destination(&self.final_path).map_err(ArtifactError::Commit)?;
        atomic_replace(&self.stage_path, &self.final_path).map_err(ArtifactError::Commit)?;
        self.committed = true;

        // The file itself is durable above. Syncing the directory additionally
        // persists the name update on Unix; some filesystems do not support it,
        // so it is intentionally best-effort after the successful rename.
        #[cfg(unix)]
        if let Some(parent) = self.final_path.parent() {
            let _ = File::open(parent).and_then(|directory| directory.sync_all());
        }

        Ok(HostArtifact {
            local_path: display_path(&canonical_path),
            size_bytes: metadata.len(),
            sha256,
        })
    }
}

impl Drop for StagedArtifact {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.stage_path);
        }
    }
}

fn validate_artifact(
    path: &Path,
    size_bytes: u64,
    kind: ArtifactKind,
) -> Result<(), ArtifactError> {
    if kind == ArtifactKind::AnyFile {
        return Ok(());
    }
    if size_bytes == 0 {
        return Err(ArtifactError::Invalid("output is empty".to_string()));
    }

    let mut header = [0_u8; 24];
    let mut file = File::open(path).map_err(ArtifactError::Stage)?;
    let read = file.read(&mut header).map_err(ArtifactError::Stage)?;
    let header = &header[..read];
    let valid = match kind {
        ArtifactKind::AnyFile => true,
        ArtifactKind::Png => {
            size_bytes >= 24
                && header.starts_with(b"\x89PNG\r\n\x1a\n")
                && header.get(12..16) == Some(b"IHDR".as_slice())
        }
        ArtifactKind::Apk => size_bytes >= 30 && header.starts_with(b"PK\x03\x04"),
        ArtifactKind::AndroidBackup => header.starts_with(b"ANDROID BACKUP\n"),
    };
    if valid {
        Ok(())
    } else {
        Err(ArtifactError::Invalid(format!(
            "output does not have the expected {} header",
            match kind {
                ArtifactKind::AnyFile => "file",
                ArtifactKind::Png => "PNG",
                ArtifactKind::Apk => "APK/ZIP",
                ArtifactKind::AndroidBackup => "Android backup",
            }
        )))
    }
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn canonical_destination(path: &Path) -> std::io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("destination has no parent: {}", path.display()),
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("destination has no file name: {}", path.display()),
        )
    })?;
    Ok(fs::canonicalize(parent)?.join(file_name))
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
#[allow(unsafe_code)] // one documented kernel32 call over owned NUL-terminated path buffers
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both pointers reference locally-owned, NUL-terminated UTF-16
    // buffers for the duration of the call. MoveFileExW does not retain them.
    let succeeded = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
pub(crate) fn display_path(path: &Path) -> String {
    let raw = path.display().to_string();
    if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string()
    }
}

#[cfg(not(windows))]
pub(crate) fn display_path(path: &Path) -> String {
    path.display().to_string()
}

pub fn read_to_string_limited(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    if let Ok(meta) = path.metadata() {
        if meta.is_file() && meta.len() > max_bytes {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!(
                    "file is too large ({} bytes; limit is {max_bytes} bytes)",
                    meta.len()
                ),
            ));
        }
    }

    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    let mut limited = file.by_ref().take(max_bytes + 1);
    limited.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("file is too large (limit is {max_bytes} bytes)"),
        ));
    }
    String::from_utf8(bytes).map_err(|e| Error::new(ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "droidsmith-fs-util-{name}-{}-{sequence}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_small_utf8_file() {
        let dir = std::env::temp_dir().join("droidsmith-read-limit-small");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.txt");
        std::fs::write(&path, "hello").unwrap();
        assert_eq!(read_to_string_limited(&path, 16).unwrap(), "hello");
    }

    #[test]
    fn rejects_oversized_file() {
        let dir = std::env::temp_dir().join("droidsmith-read-limit-large");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("large.txt");
        std::fs::write(&path, "hello").unwrap();
        let err = read_to_string_limited(&path, 4).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn atomic_commit_replaces_existing_file_and_reports_metadata() {
        let dir = test_dir("replace");
        let destination = dir.join("artifact.bin");
        fs::write(&destination, b"old").unwrap();
        let staged = StagedArtifact::new(&destination).unwrap();
        fs::write(staged.path(), b"new artifact").unwrap();

        let result = staged.commit(ArtifactKind::AnyFile).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"new artifact");
        assert_eq!(result.size_bytes, 12);
        assert_eq!(
            result.sha256,
            "ce59a877c98d548305dbcf07b6d61ecfe4ec595f8fcafef8b8947a3dd33ceab3"
        );
        assert_eq!(PathBuf::from(result.local_path), destination);
        assert!(fs::read_dir(&dir).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".partial")));
    }

    #[test]
    fn invalid_output_preserves_existing_destination_and_cleans_stage() {
        let dir = test_dir("invalid");
        let destination = dir.join("screenshot.png");
        fs::write(&destination, b"known good").unwrap();
        let stage_path = {
            let staged = StagedArtifact::new(&destination).unwrap();
            fs::write(staged.path(), b"not a png").unwrap();
            let stage_path = staged.path().to_path_buf();
            let error = staged.commit(ArtifactKind::Png).unwrap_err();
            assert_eq!(error.code(), "artifact_invalid");
            stage_path
        };

        assert_eq!(fs::read(&destination).unwrap(), b"known good");
        assert!(!stage_path.exists());
    }

    #[test]
    fn abandoned_stage_is_removed() {
        let dir = test_dir("drop");
        let destination = dir.join("artifact.bin");
        fs::write(&destination, b"existing artifact").unwrap();
        let stage_path = {
            let staged = StagedArtifact::new(&destination).unwrap();
            let path = staged.path().to_path_buf();
            assert!(path.exists());
            path
        };
        assert!(!stage_path.exists());
        assert_eq!(fs::read(destination).unwrap(), b"existing artifact");
    }

    #[test]
    fn validates_supported_artifact_headers() {
        let dir = test_dir("headers");
        for (name, bytes, kind) in [
            (
                "capture.png",
                b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR12345678".as_slice(),
                ArtifactKind::Png,
            ),
            (
                "package.apk",
                b"PK\x03\x04abcdefghijklmnopqrstuvwxyz".as_slice(),
                ArtifactKind::Apk,
            ),
            (
                "package.ab",
                b"ANDROID BACKUP\n5\n0\nnone\nbody".as_slice(),
                ArtifactKind::AndroidBackup,
            ),
        ] {
            let destination = dir.join(name);
            let staged = StagedArtifact::new(&destination).unwrap();
            fs::write(staged.path(), bytes).unwrap();
            staged.commit(kind).unwrap();
            assert_eq!(fs::read(destination).unwrap(), bytes);
        }
    }
}
