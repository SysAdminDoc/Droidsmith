//! Privacy-gated Android bugreport capture.
//!
//! The report body is treated as opaque sensitive data: Droidsmith validates
//! only ZIP structure and expected entry names. It never scans, redacts,
//! uploads, attaches, or opens report contents.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::adb::DeviceTarget;
use crate::fs_util::{self, ArtifactError, ArtifactKind, HostArtifact, StagedArtifact};
use crate::operations::{self, EventSink, OperationError};

const CAPTURE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_REPORT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Serialize)]
pub struct BugreportCaptureResult {
    pub report: HostArtifact,
    pub sidecar: HostArtifact,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct BugreportSidecar {
    schema_version: u32,
    captured_at: String,
    target_identity_sha256: String,
    build_identity_sha256: Option<String>,
    droidsmith_version: String,
    platform_tools_version: Option<String>,
    report_size_bytes: u64,
    report_sha256: String,
    privacy: BugreportPrivacy,
}

#[derive(Debug, Clone, Serialize)]
struct BugreportPrivacy {
    content_inspected: bool,
    content_redaction_performed: bool,
    uploads_performed: bool,
    report_opened: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum BugreportError {
    #[error("bugreport destination must use the .zip extension")]
    InvalidExtension,
    #[error("ADB did not create a zipped bugreport; Android 7.0 or newer is required")]
    ZipUnsupported,
    #[error("ADB bugreport failed: {0}")]
    Adb(String),
    #[error("bugreport ZIP is invalid: {0}")]
    InvalidZip(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
    #[error(transparent)]
    Operation(#[from] OperationError),
    #[error("could not encode bugreport sidecar: {0}")]
    Encode(#[from] serde_json::Error),
}

impl BugreportError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidExtension => "bugreport_invalid_extension",
            Self::ZipUnsupported => "bugreport_zip_unsupported",
            Self::Adb(_) => "bugreport_adb_failed",
            Self::InvalidZip(_) | Self::Zip(_) => "bugreport_invalid_zip",
            Self::Io(_) => "bugreport_io_failed",
            Self::Artifact(error) => error.code(),
            Self::Operation(OperationError::Cancelled) => "operation_cancelled",
            Self::Operation(OperationError::Timeout(_)) => "operation_timeout",
            Self::Operation(OperationError::OutputTooLarge(_)) => "operation_output_too_large",
            Self::Operation(_) => "bugreport_operation_failed",
            Self::Encode(_) => "bugreport_sidecar_encode_failed",
        }
    }
}

pub fn capture(
    adb_path: &Path,
    target: &DeviceTarget,
    destination: &Path,
    platform_tools_version: Option<String>,
    operation_id: &str,
    sink: EventSink,
) -> Result<BugreportCaptureResult, BugreportError> {
    let mut args = target.adb_selector();
    capture_process(
        adb_path,
        destination,
        target,
        platform_tools_version,
        operation_id,
        sink,
        CAPTURE_TIMEOUT,
        MAX_REPORT_BYTES,
        move |stage_path| {
            args.extend(["bugreport".to_string(), stage_path.display().to_string()]);
            args
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn capture_process<F>(
    program: &Path,
    destination: &Path,
    target: &DeviceTarget,
    platform_tools_version: Option<String>,
    operation_id: &str,
    sink: EventSink,
    timeout: Duration,
    max_report_bytes: u64,
    build_args: F,
) -> Result<BugreportCaptureResult, BugreportError>
where
    F: FnOnce(&Path) -> Vec<String>,
{
    if !destination
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("zip"))
    {
        return Err(BugreportError::InvalidExtension);
    }

    let staged_report = StagedArtifact::new_with_suffix(destination, ".zip")?;
    let args = build_args(staged_report.path());
    let output = operations::run_process_with_file_budget(
        program,
        &args,
        timeout,
        operation_id,
        "Capturing sensitive Android bugreport",
        sink,
        (staged_report.path(), max_report_bytes),
    )?;
    if !output.success() {
        let detail = if output.stderr.trim().is_empty() {
            output.stdout.trim()
        } else {
            output.stderr.trim()
        };
        return Err(BugreportError::Adb(detail.chars().take(4_096).collect()));
    }
    if !staged_report.path().is_file()
        || fs::metadata(staged_report.path()).map_or(true, |metadata| metadata.len() == 0)
    {
        return Err(BugreportError::ZipUnsupported);
    }
    validate_bugreport_zip(staged_report.path(), max_report_bytes)?;

    let report_size_bytes = fs::metadata(staged_report.path())?.len();
    let report_sha256 = fs_util::sha256_file(staged_report.path())?;
    let captured_at = crate::time::iso_utc_now();
    let sidecar_data = build_sidecar(
        target,
        platform_tools_version,
        captured_at.clone(),
        report_size_bytes,
        report_sha256.clone(),
    );

    let sidecar_destination = sidecar_path(destination)?;
    let staged_sidecar = StagedArtifact::new(&sidecar_destination)?;
    let mut sidecar_file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(staged_sidecar.path())?;
    serde_json::to_writer_pretty(&mut sidecar_file, &sidecar_data)?;
    sidecar_file.write_all(b"\n")?;
    sidecar_file.flush()?;
    sidecar_file.sync_all()?;
    drop(sidecar_file);

    let report = staged_report.commit(ArtifactKind::Zip)?;
    if report.size_bytes != report_size_bytes || report.sha256 != report_sha256 {
        return Err(BugreportError::InvalidZip(
            "report changed between validation and atomic commit".to_string(),
        ));
    }
    let sidecar = staged_sidecar.commit(ArtifactKind::AnyFile)?;
    Ok(BugreportCaptureResult {
        report,
        sidecar,
        captured_at,
    })
}

fn validate_bugreport_zip(path: &Path, max_report_bytes: u64) -> Result<(), BugreportError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > max_report_bytes {
        return Err(OperationError::OutputTooLarge(max_report_bytes).into());
    }
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    if archive.is_empty() || archive.len() > MAX_ZIP_ENTRIES {
        return Err(BugreportError::InvalidZip(format!(
            "expected 1..={MAX_ZIP_ENTRIES} entries, found {}",
            archive.len()
        )));
    }

    let mut has_version = false;
    let mut has_report = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name();
        let path = Path::new(name);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        {
            return Err(BugreportError::InvalidZip(
                "entry name is not a safe relative path".to_string(),
            ));
        }
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        has_version |= file_name.eq_ignore_ascii_case("version.txt");
        has_report |= file_name.starts_with("bugreport-") && file_name.ends_with(".txt");
    }
    if !has_version || !has_report {
        return Err(BugreportError::InvalidZip(
            "expected version.txt and bugreport-*.txt metadata entries".to_string(),
        ));
    }
    Ok(())
}

fn sidecar_path(destination: &Path) -> Result<PathBuf, BugreportError> {
    let file_name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| BugreportError::InvalidZip("destination has no file name".to_string()))?;
    Ok(destination.with_file_name(format!("{file_name}.metadata.json")))
}

fn build_sidecar(
    target: &DeviceTarget,
    platform_tools_version: Option<String>,
    captured_at: String,
    report_size_bytes: u64,
    report_sha256: String,
) -> BugreportSidecar {
    BugreportSidecar {
        schema_version: 1,
        captured_at,
        target_identity_sha256: hash_target(target),
        build_identity_sha256: target
            .build_fingerprint
            .as_deref()
            .map(|value| hash_identity("build", value)),
        droidsmith_version: env!("CARGO_PKG_VERSION").to_string(),
        platform_tools_version,
        report_size_bytes,
        report_sha256,
        privacy: BugreportPrivacy {
            content_inspected: false,
            content_redaction_performed: false,
            uploads_performed: false,
            report_opened: false,
        },
    }
}

fn hash_target(target: &DeviceTarget) -> String {
    let transport_id = target
        .transport_id
        .map_or_else(String::new, |id| id.to_string());
    let generation = target.connection_generation.to_string();
    let fields = [
        target.serial.as_str(),
        transport_id.as_str(),
        generation.as_str(),
        target.transport_kind.label(),
        target.model.as_deref().unwrap_or_default(),
        target.product.as_deref().unwrap_or_default(),
        target.device.as_deref().unwrap_or_default(),
    ];
    hash_identity("target", &fields.join("\0"))
}

fn hash_identity(kind: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"droidsmith-bugreport-v1\0");
    hasher.update(kind.as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;
    use zip::write::SimpleFileOptions;

    fn test_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "droidsmith-bugreport-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn target() -> DeviceTarget {
        DeviceTarget {
            serial: "SECRET-SERIAL".to_string(),
            transport_id: Some(7),
            connection_generation: 9,
            transport_kind: crate::adb::DeviceTransportKind::Usb,
            untrusted_transport_override: false,
            model: Some("Pixel".to_string()),
            product: None,
            device: None,
            build_fingerprint: Some("secret/build/fingerprint".to_string()),
        }
    }

    fn no_events() -> EventSink {
        Arc::new(|_| {})
    }

    fn write_fixture(path: &Path, include_report: bool) {
        let mut writer = zip::ZipWriter::new(File::create(path).unwrap());
        let options = SimpleFileOptions::default();
        writer.start_file("version.txt", options).unwrap();
        writer.write_all(b"16").unwrap();
        if include_report {
            writer
                .start_file("bugreport-build-2026-07-15.txt", options)
                .unwrap();
            writer.write_all(b"opaque sensitive fixture").unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn structural_validation_never_requires_reading_report_contents() {
        let dir = test_dir("zip");
        let valid = dir.join("valid.zip");
        write_fixture(&valid, true);
        validate_bugreport_zip(&valid, 1024 * 1024).unwrap();

        let invalid = dir.join("invalid.zip");
        write_fixture(&invalid, false);
        assert!(matches!(
            validate_bugreport_zip(&invalid, 1024 * 1024),
            Err(BugreportError::InvalidZip(_))
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sidecar_hashes_identifiers_and_records_no_automatic_handling() {
        let metadata = build_sidecar(
            &target(),
            Some("37.0.0".to_string()),
            "2026-07-15T10:15:00Z".to_string(),
            4096,
            "a".repeat(64),
        );
        let encoded = serde_json::to_string(&metadata).unwrap();

        assert!(!encoded.contains("SECRET-SERIAL"));
        assert!(!encoded.contains("secret/build/fingerprint"));
        assert!(encoded.contains("2026-07-15T10:15:00Z"));
        assert!(encoded.contains("\"platform_tools_version\":\"37.0.0\""));
        assert!(encoded.contains("\"content_inspected\":false"));
        assert!(encoded.contains("\"content_redaction_performed\":false"));
        assert!(encoded.contains("\"uploads_performed\":false"));
        assert!(encoded.contains("\"report_opened\":false"));
    }

    #[test]
    fn timeout_preserves_destination_and_cleans_partial_zip() {
        let dir = test_dir("timeout");
        let destination = dir.join("report.zip");
        fs::write(&destination, b"existing").unwrap();
        let (program, args) = sleep_command();
        let result = capture_process(
            program,
            &destination,
            &target(),
            Some("37.0.0".to_string()),
            "bugreport-timeout-test",
            no_events(),
            Duration::from_millis(100),
            1024,
            |_| args,
        );
        assert!(matches!(
            result,
            Err(BugreportError::Operation(OperationError::Timeout(_)))
        ));
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
        assert_no_partials(&dir);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancellation_preserves_destination_and_cleans_partial_zip() {
        let dir = test_dir("cancel");
        let destination = dir.join("report.zip");
        fs::write(&destination, b"existing").unwrap();
        let run_destination = destination.clone();
        let (program, args) = sleep_command();
        let thread = std::thread::spawn(move || {
            capture_process(
                program,
                &run_destination,
                &target(),
                Some("37.0.0".to_string()),
                "bugreport-cancel-test",
                no_events(),
                Duration::from_secs(10),
                1024,
                |_| args,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !operations::cancel("bugreport-cancel-test") && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(
            thread.join().unwrap(),
            Err(BugreportError::Operation(OperationError::Cancelled))
        ));
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
        assert_no_partials(&dir);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn live_size_budget_stops_the_writer_and_cleans_partial_zip() {
        let dir = test_dir("budget");
        let destination = dir.join("report.zip");
        fs::write(&destination, b"existing").unwrap();
        let result = capture_process(
            writer_program(),
            &destination,
            &target(),
            Some("37.0.0".to_string()),
            "bugreport-budget-test",
            no_events(),
            Duration::from_secs(5),
            1024,
            writer_args,
        );
        assert!(matches!(
            result,
            Err(BugreportError::Operation(OperationError::OutputTooLarge(
                1024
            )))
        ));
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
        assert_no_partials(&dir);
        fs::remove_dir_all(dir).unwrap();
    }

    fn assert_no_partials(dir: &Path) {
        assert!(fs::read_dir(dir).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".partial")));
    }

    #[cfg(windows)]
    fn sleep_command() -> (&'static Path, Vec<String>) {
        (
            Path::new("powershell.exe"),
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Start-Sleep -Seconds 5".to_string(),
            ],
        )
    }

    #[cfg(not(windows))]
    fn sleep_command() -> (&'static Path, Vec<String>) {
        (
            Path::new("sh"),
            vec!["-c".to_string(), "sleep 5".to_string()],
        )
    }

    #[cfg(windows)]
    fn writer_program() -> &'static Path {
        Path::new("powershell.exe")
    }

    #[cfg(not(windows))]
    fn writer_program() -> &'static Path {
        Path::new("sh")
    }

    #[cfg(windows)]
    fn writer_args(path: &Path) -> Vec<String> {
        let path = path.display().to_string().replace('\'', "''");
        vec![
            "-NoProfile".to_string(),
            "-Command".to_string(),
            format!(
                "$bytes = New-Object byte[] 4096; [IO.File]::WriteAllBytes('{path}', $bytes); Start-Sleep -Seconds 5"
            ),
        ]
    }

    #[cfg(not(windows))]
    fn writer_args(path: &Path) -> Vec<String> {
        let path = path.display().to_string().replace('\'', "'\\''");
        vec![
            "-c".to_string(),
            format!("dd if=/dev/zero of='{path}' bs=4096 count=1 2>/dev/null; sleep 5"),
        ]
    }
}
