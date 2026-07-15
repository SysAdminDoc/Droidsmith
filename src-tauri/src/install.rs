//! Safe single-APK and split-package installation.
//!
//! Archive inputs are treated as untrusted ZIPs. Only bounded APK entries are
//! extracted into an app-owned staging directory, and device-side installs use
//! PackageInstaller sessions so a partial split set is never committed.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::adb::actions::pm_failure_marker;
use crate::adb::{DeviceTarget, DeviceTransportKind, ShellTransport};
use crate::operations::{self, EventSink, ProcessOutput, RegisteredOperation};

const MAX_ARCHIVE_ENTRIES: usize = 4_096;
const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_APK_FILES: usize = 256;
const MAX_APK_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_TOTAL_APK_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_RAW_OUTPUT_CHARS: usize = 16 * 1024;

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallSourceKind {
    Apk,
    Apks,
    Xapk,
    Apkm,
}

#[derive(specta::Type, Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct InstallOptions {
    #[serde(default)]
    pub allow_downgrade: bool,
    #[serde(default)]
    pub bypass_low_target_sdk_block: bool,
    #[serde(default)]
    pub override_confirmed: bool,
}

impl InstallOptions {
    fn validate(self) -> Result<Self, InstallError> {
        if (self.allow_downgrade || self.bypass_low_target_sdk_block) && !self.override_confirmed {
            return Err(InstallError::InvalidSource(
                "install overrides require a separate explicit confirmation".to_string(),
            ));
        }
        Ok(self)
    }

    fn append_flags(self, args: &mut Vec<String>) {
        if self.allow_downgrade {
            args.push("-d".to_string());
        }
        if self.bypass_low_target_sdk_block {
            args.push("--bypass-low-target-sdk-block".to_string());
        }
    }
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedInstallOverride {
    AllowDowngrade,
    BypassLowTargetSdkBlock,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InstallFailure {
    pub code: String,
    pub cause: String,
    pub remedy: String,
    pub suggested_override: Option<SuggestedInstallOverride>,
    pub raw_output: String,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct InstallPackageResult {
    pub succeeded: bool,
    pub source_kind: InstallSourceKind,
    pub file_count: usize,
    pub total_bytes: u64,
    pub output: String,
    pub failure: Option<InstallFailure>,
    pub audit_id: String,
    /// A fresh one-shot grant for the same native-dialog-selected source is
    /// issued only when the backend recommends an explicit override retry.
    pub retry_path_grant: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("invalid install source: {0}")]
    InvalidSource(String),
    #[error("could not read install archive: {0}")]
    Archive(#[from] zip::result::ZipError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Operation(#[from] operations::OperationError),
}

#[derive(Debug)]
struct InstallFile {
    local_path: PathBuf,
    install_name: String,
    size: u64,
}

#[derive(Debug)]
struct PreparedPackage {
    kind: InstallSourceKind,
    files: Vec<InstallFile>,
    staging_dir: Option<PathBuf>,
}

impl PreparedPackage {
    fn total_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

impl Drop for PreparedPackage {
    fn drop(&mut self) {
        let Some(path) = self.staging_dir.as_ref() else {
            return;
        };
        if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir()) {
            let _ = fs::remove_dir_all(path);
        }
    }
}

struct StagingGuard(Option<PathBuf>);

impl StagingGuard {
    fn into_path(mut self) -> PathBuf {
        self.0.take().expect("staging path is present")
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.as_ref() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[derive(specta::Type, Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AuditOutcome {
    Pending,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(specta::Type, Debug, Serialize)]
struct InstallAuditRecord {
    schema_version: u32,
    operation_id: String,
    device_serial: String,
    source_kind: InstallSourceKind,
    file_count: usize,
    total_bytes: u64,
    allow_downgrade: bool,
    bypass_low_target_sdk_block: bool,
    override_confirmed: bool,
    transport_kind: DeviceTransportKind,
    untrusted_transport_override: Option<DeviceTransportKind>,
    outcome: AuditOutcome,
    started_at: String,
    completed_at: Option<String>,
    failure_code: Option<String>,
    failure_summary: Option<String>,
}

pub fn install_package(
    transport: &ShellTransport,
    target: &DeviceTarget,
    source_path: &Path,
    app_data_dir: &Path,
    operation_id: &str,
    options: InstallOptions,
    sink: EventSink,
) -> Result<InstallPackageResult, InstallError> {
    let options = options.validate()?;
    let mut runner = RegisteredOperation::new(operation_id, "Installing app package", sink)?;
    let prepared = match prepare_package(source_path, app_data_dir, operation_id, || {
        runner.is_cancelled()
    }) {
        Ok(prepared) => prepared,
        Err(error) => {
            if matches!(
                error,
                InstallError::Operation(operations::OperationError::Cancelled)
            ) {
                runner.cancelled("App package preparation cancelled");
            } else {
                runner.finish("App package preparation failed safely");
            }
            return Err(error);
        }
    };
    let started_at = crate::time::iso_utc_now();
    let audit_path = app_data_dir.join("install-operations.jsonl");
    let mut audit = InstallAuditRecord {
        schema_version: 2,
        operation_id: operation_id.to_string(),
        device_serial: target.serial.clone(),
        source_kind: prepared.kind,
        file_count: prepared.files.len(),
        total_bytes: prepared.total_bytes(),
        allow_downgrade: options.allow_downgrade,
        bypass_low_target_sdk_block: options.bypass_low_target_sdk_block,
        override_confirmed: options.override_confirmed,
        transport_kind: target.transport_kind,
        // The Tauri command revalidates this target and rejects unsafe
        // transports without acknowledgement before entering the installer.
        untrusted_transport_override: (target.untrusted_transport_override
            && target.transport_kind.requires_override())
        .then_some(target.transport_kind),
        outcome: AuditOutcome::Pending,
        started_at,
        completed_at: None,
        failure_code: None,
        failure_summary: None,
    };
    append_audit_record(&audit_path, &audit)?;

    let execution = if prepared.kind == InstallSourceKind::Apk {
        install_single(
            transport,
            target,
            &prepared,
            operation_id,
            options,
            &mut runner,
        )
    } else {
        install_archive(
            transport,
            target,
            &prepared,
            operation_id,
            options,
            &mut runner,
        )
    };

    match execution {
        Ok(mut result) => {
            audit.outcome = if result.succeeded {
                AuditOutcome::Succeeded
            } else {
                AuditOutcome::Failed
            };
            if let Some(failure) = result.failure.as_ref() {
                audit.failure_code = Some(failure.code.clone());
                audit.failure_summary = Some(failure.cause.clone());
            }
            audit.completed_at = Some(crate::time::iso_utc_now());
            append_audit_record(&audit_path, &audit)?;
            result.audit_id = operation_id.to_string();
            runner.finish(if result.succeeded {
                "App package installed"
            } else {
                "App package install failed safely"
            });
            Ok(result)
        }
        Err(error) => {
            audit.outcome = if matches!(
                error,
                InstallError::Operation(operations::OperationError::Cancelled)
            ) {
                AuditOutcome::Cancelled
            } else {
                AuditOutcome::Failed
            };
            audit.failure_code = Some(match &error {
                InstallError::Operation(operations::OperationError::Cancelled) => {
                    "operation_cancelled".to_string()
                }
                _ => "install_runtime_error".to_string(),
            });
            audit.failure_summary = Some(bounded(&error.to_string()));
            audit.completed_at = Some(crate::time::iso_utc_now());
            append_audit_record(&audit_path, &audit)?;
            if audit.outcome == AuditOutcome::Cancelled {
                runner.cancelled("App package install cancelled");
            } else {
                runner.finish("App package install failed safely");
            }
            Err(error)
        }
    }
}

fn install_single(
    transport: &ShellTransport,
    target: &DeviceTarget,
    package: &PreparedPackage,
    operation_id: &str,
    options: InstallOptions,
    runner: &mut RegisteredOperation,
) -> Result<InstallPackageResult, InstallError> {
    let file = &package.files[0];
    let mut args = target.adb_selector();
    args.extend(["install".to_string(), "-r".to_string()]);
    options.append_flags(&mut args);
    args.push(file.local_path.display().to_string());
    let output = runner.run_stage(
        &transport.adb_path,
        &args,
        INSTALL_TIMEOUT,
        "Installing the APK",
    )?;
    Ok(result_from_output(
        package,
        operation_id,
        output,
        "adb install",
    ))
}

fn install_archive(
    transport: &ShellTransport,
    target: &DeviceTarget,
    package: &PreparedPackage,
    operation_id: &str,
    options: InstallOptions,
    runner: &mut RegisteredOperation,
) -> Result<InstallPackageResult, InstallError> {
    let mut create_args = target.adb_selector();
    create_args.extend([
        "shell".to_string(),
        "pm".to_string(),
        "install-create".to_string(),
        "-r".to_string(),
    ]);
    options.append_flags(&mut create_args);
    create_args.extend(["-S".to_string(), package.total_bytes().to_string()]);
    let create_output = runner.run_stage(
        &transport.adb_path,
        &create_args,
        Duration::from_secs(30),
        "Creating an atomic Android install session",
    )?;
    if let Some(raw) = failed_output(&create_output, "pm install-create") {
        return Ok(failed_result(package, operation_id, &raw));
    }
    let Some(session_id) = parse_session_id(&create_output.stdout) else {
        return Ok(failed_result(
            package,
            operation_id,
            "pm install-create succeeded but did not return a session ID",
        ));
    };

    let mut session = DeviceInstallSession::new(transport, target, session_id);
    for (index, file) in package.files.iter().enumerate() {
        let remote = format!("/data/local/tmp/droidsmith-{operation_id}-{index:03}.apk");
        session.track_remote(remote.clone());
        let mut push_args = target.adb_selector();
        push_args.extend([
            "push".to_string(),
            file.local_path.display().to_string(),
            remote.clone(),
        ]);
        let push = runner.run_stage(
            &transport.adb_path,
            &push_args,
            TRANSFER_TIMEOUT,
            &format!(
                "Transferring package part {}/{}",
                index + 1,
                package.files.len()
            ),
        )?;
        if let Some(raw) = failed_output(&push, "adb push") {
            return Ok(failed_result(package, operation_id, &raw));
        }

        let mut write_args = target.adb_selector();
        write_args.extend([
            "shell".to_string(),
            "pm".to_string(),
            "install-write".to_string(),
            "-S".to_string(),
            file.size.to_string(),
            session.id().to_string(),
            file.install_name.clone(),
            remote.clone(),
        ]);
        let write = runner.run_stage(
            &transport.adb_path,
            &write_args,
            TRANSFER_TIMEOUT,
            &format!("Writing package part {}/{}", index + 1, package.files.len()),
        )?;
        session.remove_remote(&remote);
        if let Some(raw) = failed_output(&write, "pm install-write") {
            return Ok(failed_result(package, operation_id, &raw));
        }
    }

    let mut commit_args = target.adb_selector();
    commit_args.extend([
        "shell".to_string(),
        "pm".to_string(),
        "install-commit".to_string(),
        session.id().to_string(),
    ]);
    let commit = runner.run_stage(
        &transport.adb_path,
        &commit_args,
        INSTALL_TIMEOUT,
        "Committing the complete package set",
    )?;
    if let Some(raw) = failed_output(&commit, "pm install-commit") {
        return Ok(failed_result(package, operation_id, &raw));
    }
    session.mark_committed();
    Ok(success_result(
        package,
        operation_id,
        combined_output(&commit),
    ))
}

fn cleanup_remote(transport: &ShellTransport, target: &DeviceTarget, remote: &str) {
    let _ = transport.adb_target(target, &["shell", "rm", "-f", remote]);
}

/// RAII guard for device-side temporary APKs and the PackageInstaller session.
/// Every early return (ADB failure, cancellation, timeout, or parse error)
/// removes staged files and issues `install-abandon`. Only a verified commit
/// disarms the abandon step.
struct DeviceInstallSession<'a> {
    transport: &'a ShellTransport,
    target: &'a DeviceTarget,
    id: String,
    remote_paths: Vec<String>,
    committed: bool,
}

impl<'a> DeviceInstallSession<'a> {
    fn new(transport: &'a ShellTransport, target: &'a DeviceTarget, id: String) -> Self {
        Self {
            transport,
            target,
            id,
            remote_paths: Vec::new(),
            committed: false,
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn track_remote(&mut self, path: String) {
        self.remote_paths.push(path);
    }

    fn remove_remote(&mut self, path: &str) {
        cleanup_remote(self.transport, self.target, path);
        self.remote_paths.retain(|candidate| candidate != path);
    }

    fn mark_committed(&mut self) {
        self.committed = true;
    }
}

impl Drop for DeviceInstallSession<'_> {
    fn drop(&mut self) {
        for path in &self.remote_paths {
            cleanup_remote(self.transport, self.target, path);
        }
        if !self.committed {
            let _ = self
                .transport
                .adb_target(self.target, &["shell", "pm", "install-abandon", &self.id]);
        }
    }
}

fn result_from_output(
    package: &PreparedPackage,
    operation_id: &str,
    output: ProcessOutput,
    label: &str,
) -> InstallPackageResult {
    match failed_output(&output, label) {
        Some(raw) => failed_result(package, operation_id, &raw),
        None => success_result(package, operation_id, combined_output(&output)),
    }
}

fn success_result(
    package: &PreparedPackage,
    operation_id: &str,
    output: String,
) -> InstallPackageResult {
    InstallPackageResult {
        succeeded: true,
        source_kind: package.kind,
        file_count: package.files.len(),
        total_bytes: package.total_bytes(),
        output: bounded(&output),
        failure: None,
        audit_id: operation_id.to_string(),
        retry_path_grant: None,
    }
}

fn failed_result(package: &PreparedPackage, operation_id: &str, raw: &str) -> InstallPackageResult {
    InstallPackageResult {
        succeeded: false,
        source_kind: package.kind,
        file_count: package.files.len(),
        total_bytes: package.total_bytes(),
        output: String::new(),
        failure: Some(classify_install_failure(raw)),
        audit_id: operation_id.to_string(),
        retry_path_grant: None,
    }
}

fn failed_output(output: &ProcessOutput, label: &str) -> Option<String> {
    let combined = combined_output(output);
    if !output.success() {
        return Some(format!(
            "{label} exited with code {}: {}",
            output.code.unwrap_or(-1),
            combined.trim()
        ));
    }
    pm_failure_marker(&output.stdout)
        .or_else(|| pm_failure_marker(&output.stderr))
        .map(str::to_string)
}

fn combined_output(output: &ProcessOutput) -> String {
    match (output.stdout.trim(), output.stderr.trim()) {
        ("", stderr) => stderr.to_string(),
        (stdout, "") => stdout.to_string(),
        (stdout, stderr) => format!("{stdout}\n{stderr}"),
    }
}

pub fn classify_install_failure(raw: &str) -> InstallFailure {
    let normalized = raw.to_ascii_uppercase();
    let (code, cause, remedy, suggested_override) = if normalized
        .contains("INSTALL_FAILED_VERSION_DOWNGRADE")
    {
        (
            "INSTALL_FAILED_VERSION_DOWNGRADE",
            "The selected package has a lower version code than the installed app.",
            "Use a newer build, uninstall the existing app if its data is expendable, or review the downgrade override.",
            Some(SuggestedInstallOverride::AllowDowngrade),
        )
    } else if normalized.contains("INSTALL_FAILED_DEPRECATED_SDK_VERSION")
        || normalized.contains("LOW TARGET SDK")
        || normalized.contains("TARGET SDK VERSION") && normalized.contains("TOO LOW")
    {
        (
            "INSTALL_FAILED_DEPRECATED_SDK_VERSION",
            "Android blocked an app that targets an obsolete SDK level.",
            "Prefer an updated app. Only review the low-target-SDK override when you trust this exact package.",
            Some(SuggestedInstallOverride::BypassLowTargetSdkBlock),
        )
    } else if normalized.contains("INSTALL_FAILED_MISSING_SPLIT")
        || normalized.contains("MISSING SPLIT")
        || normalized.contains("SPLIT") && normalized.contains("MISMATCH")
    {
        (
            "INSTALL_FAILED_MISSING_SPLIT",
            "The base APK and its configuration splits do not form a complete matching set.",
            "Download all splits from the same release and device variant, then install the original APKS/XAPK/APKM archive.",
            None,
        )
    } else if normalized.contains("INSTALL_FAILED_NO_MATCHING_ABIS") {
        (
            "INSTALL_FAILED_NO_MATCHING_ABIS",
            "None of the package's native libraries match this device CPU architecture.",
            "Choose a build for the device ABI or a universal package.",
            None,
        )
    } else if normalized.contains("INSTALL_FAILED_INSUFFICIENT_STORAGE") {
        (
            "INSTALL_FAILED_INSUFFICIENT_STORAGE",
            "The device does not have enough usable storage for this package.",
            "Free device storage and retry the same package.",
            None,
        )
    } else if normalized.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE") {
        (
            "INSTALL_FAILED_UPDATE_INCOMPATIBLE",
            "The installed app and selected package are signed by different certificates.",
            "Use a build signed by the same publisher, or uninstall the existing app only after accounting for its data.",
            None,
        )
    } else if normalized.contains("INSTALL_FAILED_USER_RESTRICTED") {
        (
            "INSTALL_FAILED_USER_RESTRICTED",
            "Android policy or an OEM security setting blocked package installation.",
            "Allow USB installs for the active user and check device-owner, work-profile, or OEM security restrictions.",
            None,
        )
    } else if normalized.contains("INSTALL_FAILED_INVALID_APK") {
        (
            "INSTALL_FAILED_INVALID_APK",
            "Android could not parse or validate the package payload.",
            "Re-download the package from a trusted source and make sure every split belongs to the same release.",
            None,
        )
    } else {
        (
            extract_install_code(raw).unwrap_or("UNKNOWN_INSTALL_FAILURE"),
            "Android rejected the install for an unclassified reason.",
            "Review the raw ADB output below; no unsafe retry flag was applied.",
            None,
        )
    };
    InstallFailure {
        code: code.to_string(),
        cause: cause.to_string(),
        remedy: remedy.to_string(),
        suggested_override,
        raw_output: bounded(raw),
    }
}

fn extract_install_code(raw: &str) -> Option<&str> {
    let start = raw.find("INSTALL_FAILED_")?;
    let tail = &raw[start..];
    let len = tail
        .bytes()
        .take_while(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
        .count();
    (len > "INSTALL_FAILED_".len()).then_some(&tail[..len])
}

fn parse_session_id(stdout: &str) -> Option<String> {
    let open = stdout.find('[')?;
    let tail = &stdout[open + 1..];
    let close = tail.find(']')?;
    let value = &tail[..close];
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.to_string())
}

fn prepare_package(
    source_path: &Path,
    app_data_dir: &Path,
    operation_id: &str,
    cancelled: impl Fn() -> bool,
) -> Result<PreparedPackage, InstallError> {
    let metadata = fs::symlink_metadata(source_path)?;
    if !metadata.file_type().is_file() {
        return Err(InstallError::InvalidSource(
            "install source must be a regular, non-symlink file".to_string(),
        ));
    }
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(InstallError::InvalidSource(format!(
            "install source is {} bytes; limit is {MAX_ARCHIVE_BYTES}",
            metadata.len()
        )));
    }
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let kind = match extension.as_str() {
        "apk" => InstallSourceKind::Apk,
        "apks" => InstallSourceKind::Apks,
        "xapk" => InstallSourceKind::Xapk,
        "apkm" => InstallSourceKind::Apkm,
        _ => {
            return Err(InstallError::InvalidSource(
                "supported extensions are .apk, .apks, .xapk, and .apkm".to_string(),
            ))
        }
    };
    if kind == InstallSourceKind::Apk {
        validate_apk_size(metadata.len())?;
        return Ok(PreparedPackage {
            kind,
            files: vec![InstallFile {
                local_path: source_path.to_path_buf(),
                install_name: "base.apk".to_string(),
                size: metadata.len(),
            }],
            staging_dir: None,
        });
    }

    let file = File::open(source_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(InstallError::InvalidSource(format!(
            "archive contains {} entries; limit is {MAX_ARCHIVE_ENTRIES}",
            archive.len()
        )));
    }
    let mut candidates = Vec::new();
    let mut has_obb = false;
    for index in 0..archive.len() {
        if cancelled() {
            return Err(operations::OperationError::Cancelled.into());
        }
        let entry = archive.by_index(index)?;
        let Some(path) = entry.enclosed_name() else {
            return Err(InstallError::InvalidSource(format!(
                "archive entry {:?} has an unsafe path",
                entry.name()
            )));
        };
        if entry.is_dir() {
            continue;
        }
        let name = path.to_string_lossy().replace('\\', "/");
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".obb") {
            has_obb = true;
        }
        if !lower.ends_with(".apk") {
            continue;
        }
        validate_apk_size(entry.size())?;
        candidates.push(ArchiveCandidate {
            index,
            name,
            size: entry.size(),
        });
    }
    if has_obb {
        return Err(InstallError::InvalidSource(
            "XAPK expansion-file (.obb) deployment is not supported; no partial install was attempted"
                .to_string(),
        ));
    }
    if candidates.is_empty() {
        return Err(InstallError::InvalidSource(
            "archive does not contain any APK files".to_string(),
        ));
    }
    if candidates.len() > MAX_APK_FILES {
        return Err(InstallError::InvalidSource(format!(
            "archive contains {} APK files; limit is {MAX_APK_FILES}",
            candidates.len()
        )));
    }
    let total = candidates.iter().try_fold(0_u64, |total, candidate| {
        total
            .checked_add(candidate.size)
            .ok_or_else(|| InstallError::InvalidSource("archive APK sizes overflowed".to_string()))
    })?;
    if total > MAX_TOTAL_APK_BYTES {
        return Err(InstallError::InvalidSource(format!(
            "archive expands to {total} APK bytes; limit is {MAX_TOTAL_APK_BYTES}"
        )));
    }
    let base_index = choose_base(&candidates)?;
    candidates.sort_by_key(|candidate| (candidate.index != base_index, candidate.index));

    let staging_root = app_data_dir.join("install-staging");
    fs::create_dir_all(&staging_root)?;
    let staging_dir = staging_root.join(operation_id);
    fs::create_dir(&staging_dir)?;
    let staging_guard = StagingGuard(Some(staging_dir.clone()));
    let mut extracted = Vec::with_capacity(candidates.len());
    for (position, candidate) in candidates.iter().enumerate() {
        if cancelled() {
            return Err(operations::OperationError::Cancelled.into());
        }
        let install_name = if position == 0 {
            "base.apk".to_string()
        } else {
            format!("split-{position:03}.apk")
        };
        let local_path = staging_dir.join(&install_name);
        let mut entry = archive.by_index(candidate.index)?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&local_path)?;
        let copied = std::io::copy(&mut entry.by_ref().take(MAX_APK_BYTES + 1), &mut output)?;
        if copied != candidate.size || copied > MAX_APK_BYTES {
            return Err(InstallError::InvalidSource(format!(
                "archive entry {:?} expanded to an unexpected size",
                candidate.name
            )));
        }
        output.flush()?;
        extracted.push(InstallFile {
            local_path,
            install_name,
            size: copied,
        });
    }
    Ok(PreparedPackage {
        kind,
        files: extracted,
        staging_dir: Some(staging_guard.into_path()),
    })
}

#[derive(Debug)]
struct ArchiveCandidate {
    index: usize,
    name: String,
    size: u64,
}

fn choose_base(candidates: &[ArchiveCandidate]) -> Result<usize, InstallError> {
    let explicit = candidates
        .iter()
        .filter(|candidate| {
            let basename = Path::new(&candidate.name)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            matches!(basename.as_str(), "base.apk" | "base-master.apk")
        })
        .collect::<Vec<_>>();
    if explicit.len() == 1 {
        return Ok(explicit[0].index);
    }
    if explicit.len() > 1 {
        return Err(InstallError::InvalidSource(
            "archive contains multiple base APK candidates".to_string(),
        ));
    }
    let non_split = candidates
        .iter()
        .filter(|candidate| {
            let basename = Path::new(&candidate.name)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            !basename.starts_with("split_")
                && !basename.starts_with("split-")
                && !basename.starts_with("config.")
                && !basename.starts_with("config_")
        })
        .collect::<Vec<_>>();
    if non_split.len() == 1 {
        Ok(non_split[0].index)
    } else {
        Err(InstallError::InvalidSource(
            "archive must contain one identifiable base APK".to_string(),
        ))
    }
}

fn validate_apk_size(size: u64) -> Result<(), InstallError> {
    if size == 0 {
        return Err(InstallError::InvalidSource(
            "APK files must not be empty".to_string(),
        ));
    }
    if size > MAX_APK_BYTES {
        return Err(InstallError::InvalidSource(format!(
            "an APK is {size} bytes; per-file limit is {MAX_APK_BYTES}"
        )));
    }
    Ok(())
}

fn append_audit_record(path: &Path, record: &InstallAuditRecord) -> std::io::Result<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, record).map_err(std::io::Error::other)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_data()
}

fn bounded(value: &str) -> String {
    value
        .chars()
        .filter(|character| *character == '\n' || !character.is_control())
        .take(MAX_RAW_OUTPUT_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "droidsmith-install-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_archive(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, contents) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn archive_preparation_extracts_base_first_and_cleans_staging() {
        let dir = temp_dir("archive");
        let archive_path = dir.join("sample.apks");
        write_archive(
            &archive_path,
            &[
                ("splits/split_config.en.apk", b"split"),
                ("splits/base.apk", b"base"),
            ],
        );
        let staging_path;
        {
            let prepared =
                prepare_package(&archive_path, &dir, "install-fixture-01", || false).unwrap();
            assert_eq!(prepared.kind, InstallSourceKind::Apks);
            assert_eq!(prepared.files.len(), 2);
            assert_eq!(prepared.files[0].install_name, "base.apk");
            assert_eq!(fs::read(&prepared.files[0].local_path).unwrap(), b"base");
            staging_path = prepared.staging_dir.clone().unwrap();
            assert!(staging_path.is_dir());
        }
        assert!(!staging_path.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn archive_rejects_unsafe_paths_and_obb_payloads() {
        let dir = temp_dir("unsafe");
        let unsafe_path = dir.join("unsafe.xapk");
        write_archive(&unsafe_path, &[("../base.apk", b"base")]);
        assert!(prepare_package(&unsafe_path, &dir, "install-fixture-02", || false).is_err());

        let obb_path = dir.join("expansion.xapk");
        write_archive(
            &obb_path,
            &[
                ("base.apk", b"base"),
                ("Android/obb/main.1.example.obb", b"obb"),
            ],
        );
        let error = prepare_package(&obb_path, &dir, "install-fixture-03", || false)
            .unwrap_err()
            .to_string();
        assert!(error.contains(".obb"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parses_install_session_and_rejects_non_numeric_ids() {
        assert_eq!(
            parse_session_id("Success: created install session [482]"),
            Some("482".to_string())
        );
        assert_eq!(parse_session_id("Success [../../1]"), None);
        assert_eq!(parse_session_id("Failure"), None);
    }

    #[test]
    fn classifies_required_install_failure_fixtures() {
        let downgrade = classify_install_failure(
            "Failure [INSTALL_FAILED_VERSION_DOWNGRADE: Downgrade detected]",
        );
        assert_eq!(
            downgrade.suggested_override,
            Some(SuggestedInstallOverride::AllowDowngrade)
        );
        let sdk = classify_install_failure(
            "Failure [INSTALL_FAILED_DEPRECATED_SDK_VERSION: App package must target at least SDK]",
        );
        assert_eq!(
            sdk.suggested_override,
            Some(SuggestedInstallOverride::BypassLowTargetSdkBlock)
        );
        let split = classify_install_failure(
            "Failure [INSTALL_FAILED_MISSING_SPLIT: Missing split for com.example]",
        );
        assert_eq!(split.code, "INSTALL_FAILED_MISSING_SPLIT");
        assert_eq!(split.suggested_override, None);
    }

    #[test]
    fn unknown_failure_preserves_bounded_raw_output_without_an_override() {
        let raw = format!("vendor failure {}", "x".repeat(MAX_RAW_OUTPUT_CHARS + 50));
        let failure = classify_install_failure(&raw);
        assert_eq!(failure.code, "UNKNOWN_INSTALL_FAILURE");
        assert!(failure.raw_output.len() <= MAX_RAW_OUTPUT_CHARS);
        assert_eq!(failure.suggested_override, None);
    }

    #[test]
    fn override_flags_require_explicit_confirmation() {
        let options = InstallOptions {
            allow_downgrade: true,
            ..Default::default()
        };
        assert!(options.validate().is_err());
        let confirmed = InstallOptions {
            override_confirmed: true,
            ..options
        };
        let mut args = Vec::new();
        confirmed.validate().unwrap().append_flags(&mut args);
        assert_eq!(args, ["-d"]);
    }

    #[test]
    fn confirmed_override_is_durable_without_persisting_the_host_path() {
        let dir = temp_dir("audit");
        let path = dir.join("install-operations.jsonl");
        let record = InstallAuditRecord {
            schema_version: 2,
            operation_id: "install-audit-01".to_string(),
            device_serial: "SERIAL-1".to_string(),
            source_kind: InstallSourceKind::Apks,
            file_count: 3,
            total_bytes: 42,
            allow_downgrade: true,
            bypass_low_target_sdk_block: false,
            override_confirmed: true,
            transport_kind: DeviceTransportKind::LegacyTcp,
            untrusted_transport_override: Some(DeviceTransportKind::LegacyTcp),
            outcome: AuditOutcome::Pending,
            started_at: "2026-07-14T18:00:00Z".to_string(),
            completed_at: None,
            failure_code: None,
            failure_summary: None,
        };
        append_audit_record(&path, &record).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(value["allow_downgrade"], true);
        assert_eq!(value["override_confirmed"], true);
        assert_eq!(value["transport_kind"], "legacy_tcp");
        assert_eq!(value["untrusted_transport_override"], "legacy_tcp");
        assert!(value.get("source_path").is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}
