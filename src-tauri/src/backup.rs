//! Evidence-based package export and legacy ADB backup handling.
//!
//! APK export is the dependable default. The deprecated `adb backup` path is
//! deliberately separate: it is capability-gated, emits an uncompressed
//! archive so its TAR payload can be validated, and never claims restore
//! success.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

use crate::adb::device::DeviceTarget;
use crate::adb::transport::AdbTransport;
use crate::fs_util::{self, ArtifactKind, HostArtifact, StagedArtifact};
use crate::operations::{EventSink, RegisteredOperation};

const MAX_APK_PARTS: usize = 64;
const MAX_TAR_ENTRIES: usize = 100_000;
const TAR_BLOCK_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupCapability {
    ApkExport,
    LegacyDataEligible,
    LegacyDataBlocked,
    LegacyDataUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LegacyEligibilityEvidence {
    pub device_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
    pub debuggable: Option<bool>,
    pub allow_backup: Option<bool>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageBackupPreflight {
    pub package: String,
    pub android_user: u32,
    pub default_capability: BackupCapability,
    pub legacy_capability: BackupCapability,
    pub apk_paths: Vec<String>,
    pub evidence: LegacyEligibilityEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportMode {
    ApkExport,
    LegacyData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyContent {
    AppDataEntriesDetected,
    NoAppDataEntries,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HashedDeviceIdentity {
    pub device_identity_sha256: String,
    pub build_identity_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportedArtifact {
    pub name: String,
    pub role: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageExportManifest {
    pub format: &'static str,
    pub schema_version: u32,
    pub created_at: String,
    pub mode: ExportMode,
    pub package: String,
    pub android_user: u32,
    pub device: HashedDeviceIdentity,
    pub eligibility: LegacyEligibilityEvidence,
    pub legacy_content: Option<LegacyContent>,
    pub artifacts: Vec<ExportedArtifact>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageExportResult {
    pub artifact: HostArtifact,
    pub manifest: PackageExportManifest,
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("package export preflight failed: {0}")]
    Preflight(String),
    #[error("legacy data export is blocked: {0}")]
    LegacyBlocked(String),
    #[error("ADB export failed: {0}")]
    Adb(String),
    #[error("legacy Android backup failed structural validation: {0}")]
    InvalidLegacy(String),
    #[error("package export filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("package export archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Artifact(#[from] fs_util::ArtifactError),
    #[error(transparent)]
    Operation(#[from] crate::operations::OperationError),
}

impl BackupError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Preflight(_) => "backup_preflight_failed",
            Self::LegacyBlocked(_) => "legacy_backup_blocked",
            Self::Adb(_) => "adb_exit",
            Self::InvalidLegacy(_) => "legacy_backup_invalid",
            Self::Io(_) => "io_error",
            Self::Zip(_) => "artifact_archive_failed",
            Self::Artifact(error) => error.code(),
            Self::Operation(_) => "operation_failed",
        }
    }
}

/// Inspect the exact package/user without mutating the device. `pm path`
/// supplies every base/split APK; dumpsys flags provide the best available
/// evidence for deprecated data-backup eligibility.
pub fn preflight(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
    package: &str,
    user_id: u32,
) -> Result<PackageBackupPreflight, BackupError> {
    let user = user_id.to_string();
    let path_output = transport
        .shell_target(target, &["pm", "path", "--user", &user, package])
        .map_err(|error| BackupError::Preflight(error.to_string()))?;
    let apk_paths = parse_pm_paths(&path_output, package)?;
    let details = transport
        .shell_target(target, &["dumpsys", "package", package])
        .map_err(|error| BackupError::Preflight(error.to_string()))?;
    let device_sdk = transport
        .shell_target(target, &["getprop", "ro.build.version.sdk"])
        .ok()
        .and_then(|raw| raw.trim().parse().ok());
    let parsed = parse_package_details(&details);
    let (legacy_capability, reason) = classify_legacy(
        device_sdk,
        parsed.target_sdk,
        parsed.debuggable,
        parsed.allow_backup,
    );

    Ok(PackageBackupPreflight {
        package: package.to_string(),
        android_user: user_id,
        default_capability: BackupCapability::ApkExport,
        legacy_capability,
        apk_paths,
        evidence: LegacyEligibilityEvidence {
            device_sdk,
            target_sdk: parsed.target_sdk,
            debuggable: parsed.debuggable,
            allow_backup: parsed.allow_backup,
            reason: reason.to_string(),
        },
    })
}

pub fn export_apks(
    adb_path: &Path,
    target: &DeviceTarget,
    output_target: &Path,
    preflight: PackageBackupPreflight,
    operation_id: &str,
    sink: EventSink,
) -> Result<PackageExportResult, BackupError> {
    let temporary = TemporaryDirectory::new(output_target, "apk-export")?;
    let mut operation = RegisteredOperation::new(operation_id, "Exporting package APKs", sink)?;
    let mut local_parts = Vec::with_capacity(preflight.apk_paths.len());

    for (index, remote) in preflight.apk_paths.iter().enumerate() {
        let local = temporary.path.join(format!("part-{index:02}.apk"));
        let mut args = target.adb_selector();
        args.extend([
            "pull".to_string(),
            remote.clone(),
            local.display().to_string(),
        ]);
        let output = operation.run_stage(
            adb_path,
            &args,
            Duration::from_secs(120),
            &format!(
                "Exporting APK part {}/{}",
                index + 1,
                preflight.apk_paths.len()
            ),
        )?;
        ensure_adb_success(&output, "adb pull")?;
        let size = fs::metadata(&local)?.len();
        fs_util::validate_artifact(&local, size, ArtifactKind::Apk)?;
        local_parts.push((bundle_apk_name(remote, index), local));
    }

    let artifacts = artifact_records(&local_parts, "apk")?;
    let manifest = manifest(target, &preflight, ExportMode::ApkExport, None, artifacts);
    let artifact = write_bundle(output_target, &manifest, &local_parts)?;
    operation.finish("Package APK export complete");
    Ok(PackageExportResult { artifact, manifest })
}

pub fn export_legacy_data(
    adb_path: &Path,
    target: &DeviceTarget,
    output_target: &Path,
    preflight: PackageBackupPreflight,
    operation_id: &str,
    sink: EventSink,
) -> Result<PackageExportResult, BackupError> {
    if preflight.legacy_capability == BackupCapability::LegacyDataBlocked {
        return Err(BackupError::LegacyBlocked(
            preflight.evidence.reason.clone(),
        ));
    }

    let temporary = TemporaryDirectory::new(output_target, "legacy-data")?;
    let legacy_path = temporary.path.join("legacy-data.ab");
    let mut operation = RegisteredOperation::new(operation_id, "Running legacy data export", sink)?;
    let mut args = target.adb_selector();
    args.extend([
        "backup".to_string(),
        "-f".to_string(),
        legacy_path.display().to_string(),
        "-noapk".to_string(),
        "-noobb".to_string(),
        "-noshared".to_string(),
        "-nocompress".to_string(),
        preflight.package.clone(),
    ]);
    let output = operation.run_stage(
        adb_path,
        &args,
        Duration::from_secs(300),
        "Waiting for device confirmation for deprecated adb backup",
    )?;
    ensure_adb_success(&output, "adb backup")?;

    let inspection = inspect_uncompressed_android_backup(&legacy_path, &preflight.package)?;
    let local_parts = vec![("legacy-data.ab".to_string(), legacy_path)];
    let artifacts = artifact_records(&local_parts, "legacy_android_backup")?;
    let legacy_content = if inspection.app_data_entries > 0 {
        LegacyContent::AppDataEntriesDetected
    } else {
        LegacyContent::NoAppDataEntries
    };
    let manifest = manifest(
        target,
        &preflight,
        ExportMode::LegacyData,
        Some(legacy_content),
        artifacts,
    );
    let artifact = write_bundle(output_target, &manifest, &local_parts)?;
    operation.finish("Legacy data export inspected and packaged");
    Ok(PackageExportResult { artifact, manifest })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedPackageDetails {
    target_sdk: Option<u32>,
    debuggable: Option<bool>,
    allow_backup: Option<bool>,
}

fn parse_package_details(output: &str) -> ParsedPackageDetails {
    let mut parsed = ParsedPackageDetails::default();
    let mut saw_flags = false;
    let mut flags_debuggable = false;
    let mut flags_allow_backup = false;

    for line in output.lines().map(str::trim) {
        for token in line.split_whitespace() {
            if let Some(value) = token.strip_prefix("targetSdk=") {
                parsed.target_sdk = value.trim_end_matches(',').parse().ok();
            }
        }
        if let Some(value) = bool_field(line, "debuggable=") {
            parsed.debuggable = Some(value);
        }
        if let Some(value) = bool_field(line, "allowBackup=") {
            parsed.allow_backup = Some(value);
        }
        if line.contains("flags=[") || line.contains("pkgFlags=[") {
            saw_flags = true;
            flags_debuggable |= contains_flag(line, "DEBUGGABLE");
            flags_allow_backup |= contains_flag(line, "ALLOW_BACKUP");
        }
    }

    if saw_flags {
        parsed.debuggable.get_or_insert(flags_debuggable);
        parsed.allow_backup.get_or_insert(flags_allow_backup);
    }
    parsed
}

fn bool_field(line: &str, prefix: &str) -> Option<bool> {
    let value = line
        .split_whitespace()
        .find_map(|token| token.strip_prefix(prefix))?;
    match value.trim_end_matches(',') {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn contains_flag(line: &str, flag: &str) -> bool {
    line.split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|token| token == flag)
}

fn classify_legacy(
    device_sdk: Option<u32>,
    target_sdk: Option<u32>,
    debuggable: Option<bool>,
    allow_backup: Option<bool>,
) -> (BackupCapability, &'static str) {
    if allow_backup == Some(false) {
        return (
            BackupCapability::LegacyDataBlocked,
            "application flags disable Android backup",
        );
    }
    if allow_backup != Some(true) {
        return (
            BackupCapability::LegacyDataUnknown,
            "the OEM package dump did not expose a reliable allowBackup flag",
        );
    }
    match device_sdk {
        Some(sdk) if sdk < 31 => (
            BackupCapability::LegacyDataEligible,
            "device platform predates the Android 12 target-SDK exclusion",
        ),
        Some(_) => match (target_sdk, debuggable) {
            (_, Some(true)) => (
                BackupCapability::LegacyDataEligible,
                "debuggable package is exempt from the Android 12 target-SDK exclusion",
            ),
            (Some(target), Some(false)) if target >= 31 => (
                BackupCapability::LegacyDataBlocked,
                "Android 12+ excludes non-debuggable apps targeting API 31 or newer",
            ),
            (Some(target), _) if target < 31 => (
                BackupCapability::LegacyDataEligible,
                "package targets an API below the Android 12 exclusion threshold",
            ),
            _ => (
                BackupCapability::LegacyDataUnknown,
                "target SDK or debuggable evidence is missing from the OEM package dump",
            ),
        },
        None => (
            BackupCapability::LegacyDataUnknown,
            "device API level could not be verified",
        ),
    }
}

fn parse_pm_paths(output: &str, package: &str) -> Result<Vec<String>, BackupError> {
    let mut paths = Vec::new();
    for line in output.lines().map(str::trim) {
        let Some(path) = line.strip_prefix("package:") else {
            continue;
        };
        if !path.starts_with('/')
            || path.starts_with("/-")
            || path.chars().any(char::is_control)
            || !path.ends_with(".apk")
        {
            return Err(BackupError::Preflight(format!(
                "PackageManager returned an unsafe APK path for {package}"
            )));
        }
        if !paths.iter().any(|existing| existing == path) {
            paths.push(path.to_string());
        }
    }
    if paths.is_empty() {
        return Err(BackupError::Preflight(format!(
            "PackageManager returned no APK paths for {package} and the selected Android user"
        )));
    }
    if paths.len() > MAX_APK_PARTS {
        return Err(BackupError::Preflight(format!(
            "PackageManager returned more than {MAX_APK_PARTS} APK parts for {package}"
        )));
    }
    Ok(paths)
}

fn ensure_adb_success(
    output: &crate::operations::ProcessOutput,
    program: &str,
) -> Result<(), BackupError> {
    if output.success() {
        return Ok(());
    }
    let detail = if output.stderr.trim().is_empty() {
        output.stdout.trim()
    } else {
        output.stderr.trim()
    };
    Err(BackupError::Adb(format!(
        "{program} exited with code {}: {detail}",
        output.code.unwrap_or(-1)
    )))
}

fn manifest(
    target: &DeviceTarget,
    preflight: &PackageBackupPreflight,
    mode: ExportMode,
    legacy_content: Option<LegacyContent>,
    artifacts: Vec<ExportedArtifact>,
) -> PackageExportManifest {
    PackageExportManifest {
        format: "droidsmith_package_export",
        schema_version: 1,
        created_at: crate::time::iso_utc_now(),
        mode,
        package: preflight.package.clone(),
        android_user: preflight.android_user,
        device: HashedDeviceIdentity {
            device_identity_sha256: hash_text(&target.serial),
            build_identity_sha256: target.build_fingerprint.as_deref().map(hash_text),
        },
        eligibility: preflight.evidence.clone(),
        legacy_content,
        artifacts,
    }
}

fn artifact_records(
    parts: &[(String, PathBuf)],
    role: &str,
) -> Result<Vec<ExportedArtifact>, BackupError> {
    parts
        .iter()
        .map(|(name, path)| {
            Ok(ExportedArtifact {
                name: name.clone(),
                role: role.to_string(),
                size_bytes: fs::metadata(path)?.len(),
                sha256: fs_util::sha256_file(path)?,
            })
        })
        .collect()
}

fn write_bundle(
    output_target: &Path,
    manifest: &PackageExportManifest,
    parts: &[(String, PathBuf)],
) -> Result<HostArtifact, BackupError> {
    let staged = StagedArtifact::new(output_target)?;
    let file = File::options()
        .write(true)
        .truncate(true)
        .open(staged.path())?;
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o600);

    for (name, path) in parts {
        writer.start_file(name, options)?;
        let mut source = File::open(path)?;
        std::io::copy(&mut source, &mut writer)?;
    }
    writer.start_file("manifest.json", options)?;
    serde_json::to_writer_pretty(&mut writer, manifest)
        .map_err(|error| BackupError::Preflight(error.to_string()))?;
    writer.write_all(b"\n")?;
    let mut file = writer.finish()?;
    file.flush()?;
    file.sync_all()?;
    Ok(staged.commit(ArtifactKind::Zip)?)
}

fn bundle_apk_name(remote: &str, index: usize) -> String {
    let source_name = remote.rsplit('/').next().unwrap_or("part.apk");
    if source_name.eq_ignore_ascii_case("base.apk") || index == 0 {
        return "base.apk".to_string();
    }
    let safe = source_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("split-{index:02}-{safe}")
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new(output_target: &Path, label: &str) -> Result<Self, std::io::Error> {
        let parent = output_target.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "output has no parent")
        })?;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        for _ in 0..64 {
            let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                ".droidsmith-{label}-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate package-export staging directory",
        ))
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct LegacyInspection {
    app_data_entries: usize,
}

fn inspect_uncompressed_android_backup(
    path: &Path,
    package: &str,
) -> Result<LegacyInspection, BackupError> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let magic = read_header_line(&mut reader)?;
    let version = read_header_line(&mut reader)?;
    let compressed = read_header_line(&mut reader)?;
    let encryption = read_header_line(&mut reader)?;
    if magic != "ANDROID BACKUP"
        || !matches!(version.as_str(), "1" | "2" | "3" | "4" | "5")
        || compressed != "0"
        || encryption != "none"
    {
        return Err(BackupError::InvalidLegacy(
            "unexpected header, version, compression, or encryption mode".to_string(),
        ));
    }

    let mut block = [0_u8; TAR_BLOCK_BYTES];
    let mut zero_blocks = 0;
    let mut entry_count = 0;
    let mut app_data_entries = 0;
    loop {
        reader
            .read_exact(&mut block)
            .map_err(|_| BackupError::InvalidLegacy("truncated TAR payload".to_string()))?;
        if block.iter().all(|byte| *byte == 0) {
            zero_blocks += 1;
            if zero_blocks == 2 {
                break;
            }
            continue;
        }
        if zero_blocks != 0 {
            return Err(BackupError::InvalidLegacy(
                "non-zero TAR data followed an end marker".to_string(),
            ));
        }
        entry_count += 1;
        if entry_count > MAX_TAR_ENTRIES {
            return Err(BackupError::InvalidLegacy(
                "TAR entry limit exceeded".to_string(),
            ));
        }
        validate_tar_checksum(&block)?;
        let name = tar_entry_name(&block)?;
        let size = tar_octal(&block[124..136])?;
        if is_app_data_entry(&name, package, size) {
            app_data_entries += 1;
        }
        let mut remaining = size;
        let mut buffer = [0_u8; 64 * 1024];
        while remaining > 0 {
            let amount =
                usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
            reader
                .read_exact(&mut buffer[..amount])
                .map_err(|_| BackupError::InvalidLegacy("truncated TAR entry".to_string()))?;
            remaining -= amount as u64;
        }
        let padding =
            (TAR_BLOCK_BYTES as u64 - size % TAR_BLOCK_BYTES as u64) % TAR_BLOCK_BYTES as u64;
        if padding > 0 {
            reader
                .read_exact(&mut block[..padding as usize])
                .map_err(|_| BackupError::InvalidLegacy("truncated TAR padding".to_string()))?;
        }
    }
    let mut trailing = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut trailing)?;
        if read == 0 {
            break;
        }
        if trailing[..read].iter().any(|byte| *byte != 0) {
            return Err(BackupError::InvalidLegacy(
                "non-zero bytes follow the TAR end marker".to_string(),
            ));
        }
    }
    Ok(LegacyInspection { app_data_entries })
}

fn read_header_line(reader: &mut impl BufRead) -> Result<String, BackupError> {
    let mut line = Vec::new();
    let bytes = reader.read_until(b'\n', &mut line)?;
    if bytes == 0 || bytes > 128 || line.last() != Some(&b'\n') {
        return Err(BackupError::InvalidLegacy(
            "invalid or oversized header line".to_string(),
        ));
    }
    line.pop();
    String::from_utf8(line)
        .map_err(|_| BackupError::InvalidLegacy("header is not UTF-8".to_string()))
}

fn validate_tar_checksum(block: &[u8; TAR_BLOCK_BYTES]) -> Result<(), BackupError> {
    let expected = tar_octal(&block[148..156])?;
    let actual: u64 = block
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                u64::from(b' ')
            } else {
                u64::from(*byte)
            }
        })
        .sum();
    if expected == actual {
        Ok(())
    } else {
        Err(BackupError::InvalidLegacy(
            "TAR header checksum mismatch".to_string(),
        ))
    }
}

fn tar_octal(field: &[u8]) -> Result<u64, BackupError> {
    let text = std::str::from_utf8(field)
        .map_err(|_| BackupError::InvalidLegacy("invalid TAR numeric field".to_string()))?
        .trim_matches(|character| character == '\0' || character == ' ');
    if text.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(text, 8)
        .map_err(|_| BackupError::InvalidLegacy("invalid TAR octal field".to_string()))
}

fn tar_entry_name(block: &[u8; TAR_BLOCK_BYTES]) -> Result<String, BackupError> {
    let name = tar_text(&block[0..100])?;
    let prefix = tar_text(&block[345..500])?;
    let combined = if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    };
    if combined.is_empty()
        || combined.starts_with('/')
        || combined.split('/').any(|part| part == "..")
    {
        return Err(BackupError::InvalidLegacy(
            "unsafe TAR entry name".to_string(),
        ));
    }
    Ok(combined)
}

fn tar_text(field: &[u8]) -> Result<String, BackupError> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    let text = std::str::from_utf8(&field[..end])
        .map_err(|_| BackupError::InvalidLegacy("invalid TAR entry name".to_string()))?;
    Ok(text.to_string())
}

fn is_app_data_entry(name: &str, package: &str, size: u64) -> bool {
    if size == 0 {
        return false;
    }
    let prefix = format!("apps/{package}/");
    let Some(relative) = name.strip_prefix(&prefix) else {
        return false;
    };
    relative != "_manifest"
        && relative != "manifest"
        && !relative.starts_with("a/")
        && !relative.starts_with("apk/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::transport::MockTransport;

    fn target() -> DeviceTarget {
        DeviceTarget {
            serial: "fixture-device".to_string(),
            transport_id: Some(7),
            connection_generation: 1,
            build_fingerprint: Some("vendor/device:14/test".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn preflight_reports_split_apks_and_android_12_block() {
        let mock = MockTransport::new();
        mock.expect_shell(
            "fixture-device",
            &["pm", "path", "--user", "10", "com.example.app"],
            Ok("package:/data/app/example/base.apk\npackage:/data/app/example/split_config.en.apk\n".to_string()),
        );
        mock.expect_shell(
            "fixture-device",
            &["dumpsys", "package", "com.example.app"],
            Ok("targetSdk=35\nflags=[ HAS_CODE ALLOW_BACKUP ]\n".to_string()),
        );
        mock.expect_shell(
            "fixture-device",
            &["getprop", "ro.build.version.sdk"],
            Ok("35\n".to_string()),
        );

        let result = preflight(&mock, &target(), "com.example.app", 10).unwrap();
        assert_eq!(result.default_capability, BackupCapability::ApkExport);
        assert_eq!(
            result.legacy_capability,
            BackupCapability::LegacyDataBlocked
        );
        assert_eq!(result.apk_paths.len(), 2);
        assert_eq!(result.evidence.target_sdk, Some(35));
        assert_eq!(result.evidence.debuggable, Some(false));
        assert_eq!(result.evidence.allow_backup, Some(true));
    }

    #[test]
    fn legacy_capability_distinguishes_eligible_blocked_and_unknown() {
        assert_eq!(
            classify_legacy(Some(35), Some(35), Some(true), Some(true)).0,
            BackupCapability::LegacyDataEligible
        );
        assert_eq!(
            classify_legacy(Some(35), Some(30), Some(false), Some(true)).0,
            BackupCapability::LegacyDataEligible
        );
        assert_eq!(
            classify_legacy(Some(35), Some(35), Some(false), Some(true)).0,
            BackupCapability::LegacyDataBlocked
        );
        assert_eq!(
            classify_legacy(Some(35), None, None, Some(true)).0,
            BackupCapability::LegacyDataUnknown
        );
        assert_eq!(
            classify_legacy(Some(30), Some(35), Some(false), Some(true)).0,
            BackupCapability::LegacyDataEligible
        );
    }

    #[test]
    fn package_details_parse_explicit_and_flag_evidence() {
        assert_eq!(
            parse_package_details("targetSdk=34\n  flags=[ DEBUGGABLE HAS_CODE ALLOW_BACKUP ]\n"),
            ParsedPackageDetails {
                target_sdk: Some(34),
                debuggable: Some(true),
                allow_backup: Some(true),
            }
        );
        assert_eq!(
            parse_package_details("targetSdk=28\nallowBackup=false debuggable=false\n"),
            ParsedPackageDetails {
                target_sdk: Some(28),
                debuggable: Some(false),
                allow_backup: Some(false),
            }
        );
    }

    #[test]
    fn pm_paths_reject_unsafe_or_missing_rows() {
        assert!(parse_pm_paths("package:../../base.apk\n", "com.example").is_err());
        assert!(parse_pm_paths("Error: package not found\n", "com.example").is_err());
        assert_eq!(
            parse_pm_paths("package:/data/app/base.apk\n", "com.example").unwrap(),
            vec!["/data/app/base.apk"]
        );
    }

    #[test]
    fn uncompressed_backup_validation_requires_sound_tar_and_classifies_data() {
        let dir = std::env::temp_dir().join(format!(
            "droidsmith-backup-fixture-{}-{}",
            std::process::id(),
            hash_text("valid-tar")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fixture.ab");
        let mut bytes = b"ANDROID BACKUP\n5\n0\nnone\n".to_vec();
        bytes.extend(tar_entry("apps/com.example/f/files.json", b"evidence"));
        // Tar writers may pad the required two zero blocks to a larger record.
        bytes.extend([0_u8; TAR_BLOCK_BYTES * 20]);
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            inspect_uncompressed_android_backup(&path, "com.example").unwrap(),
            LegacyInspection {
                app_data_entries: 1
            }
        );

        let mut malformed = fs::read(&path).unwrap();
        malformed[40] ^= 1;
        fs::write(&path, malformed).unwrap();
        assert!(inspect_uncompressed_android_backup(&path, "com.example").is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bundle_manifest_hashes_identity_and_every_inner_artifact() {
        let dir = std::env::temp_dir().join(format!(
            "droidsmith-package-bundle-{}-{}",
            std::process::id(),
            hash_text("bundle-manifest")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let base = dir.join("source.apk");
        fs::write(&base, b"PK\x03\x04abcdefghijklmnopqrstuvwxyz").unwrap();
        let parts = vec![("base.apk".to_string(), base)];
        let preflight = PackageBackupPreflight {
            package: "com.example.app".to_string(),
            android_user: 10,
            default_capability: BackupCapability::ApkExport,
            legacy_capability: BackupCapability::LegacyDataUnknown,
            apk_paths: vec!["/data/app/example/base.apk".to_string()],
            evidence: LegacyEligibilityEvidence {
                device_sdk: None,
                target_sdk: None,
                debuggable: None,
                allow_backup: None,
                reason: "fixture".to_string(),
            },
        };
        let manifest = manifest(
            &target(),
            &preflight,
            ExportMode::ApkExport,
            None,
            artifact_records(&parts, "apk").unwrap(),
        );
        let destination = dir.join("export.zip");
        let artifact = write_bundle(&destination, &manifest, &parts).unwrap();
        assert_eq!(artifact.sha256.len(), 64);
        assert_ne!(manifest.device.device_identity_sha256, target().serial);
        assert_eq!(
            manifest.artifacts[0].sha256,
            fs_util::sha256_file(&parts[0].1).unwrap()
        );

        let file = File::open(&destination).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert!(archive.by_name("base.apk").is_ok());
        let mut manifest_json = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut manifest_json)
            .unwrap();
        assert!(manifest_json.contains("droidsmith_package_export"));
        assert!(!manifest_json.contains("fixture-device"));
        assert!(!manifest_json.contains("vendor/device:14/test"));
        fs::remove_dir_all(dir).unwrap();
    }

    fn tar_entry(name: &str, contents: &[u8]) -> Vec<u8> {
        let mut header = [0_u8; TAR_BLOCK_BYTES];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..108].copy_from_slice(b"0000600\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        let size = format!("{:011o}\0", contents.len());
        header[124..136].copy_from_slice(size.as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
        let checksum = format!("{checksum:06o}\0 ");
        header[148..156].copy_from_slice(checksum.as_bytes());
        let mut out = header.to_vec();
        out.extend(contents);
        out.resize(
            TAR_BLOCK_BYTES + contents.len().div_ceil(TAR_BLOCK_BYTES) * TAR_BLOCK_BYTES,
            0,
        );
        out
    }
}
