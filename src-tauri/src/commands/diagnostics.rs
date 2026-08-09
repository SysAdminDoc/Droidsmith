//! Domain-scoped Tauri command boundary.

use super::*;

#[derive(specta::Type, Serialize)]
pub struct Heartbeat {
    /// Droidsmith app version (`CARGO_PKG_VERSION`).
    pub version: String,
    /// Operating system family + version + arch.
    pub os: OsInfo,
    /// Tauri framework version this build links against.
    pub tauri_version: &'static str,
    /// Rust MSRV declared in `Cargo.toml`. Useful for bug reports.
    pub rust_version: &'static str,
    /// Where the user's persisted state lives (journal, settings, logs).
    pub app_data_dir: Option<String>,
    /// ADB binary resolution + source + version.
    pub adb: adb::AdbResolution,
}

#[derive(specta::Type, Serialize, Clone)]
pub struct OsInfo {
    pub family: String,
    pub version: String,
    pub arch: String,
}

/// Cache `os_info::get()` for the process lifetime. The probe reads
/// `/etc/os-release` on Linux and the registry on Windows; cheap once,
/// noisy if called on every heartbeat refresh.
pub(crate) fn cached_os_info() -> &'static OsInfo {
    static CACHE: OnceLock<OsInfo> = OnceLock::new();
    CACHE.get_or_init(|| {
        let info = os_info::get();
        OsInfo {
            family: info.os_type().to_string(),
            version: info.version().to_string(),
            arch: info.architecture().unwrap_or("unknown").to_string(),
        }
    })
}

#[tauri::command]
#[specta::specta]
pub fn heartbeat(app: tauri::AppHandle) -> Heartbeat {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .ok()
        .map(|p| p.display().to_string());

    Heartbeat {
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: cached_os_info().clone(),
        tauri_version: tauri::VERSION,
        rust_version: env!("CARGO_PKG_RUST_VERSION"),
        app_data_dir,
        adb: adb::locate_adb(),
    }
}

/// Run a bounded, non-elevated, read-only host connection scan. The report
/// contains state counts and redacted configuration presence only; it never
/// persists device identifiers, USB instance IDs, environment values, or keys.
#[tauri::command]
#[specta::specta]
pub async fn run_host_doctor() -> Result<crate::host_diagnostics::HostDoctorReport, CommandError> {
    spawn_blocking_operation(|| Ok(crate::host_diagnostics::scan())).await
}

/// Build a bounded, redacted support snapshot entirely on the local machine.
/// The payload deliberately excludes resolver paths and raw device targets.
#[tauri::command]
#[specta::specta]
pub async fn preview_diagnostics(
    app: tauri::AppHandle,
) -> Result<support_bundle::SupportPreview, CommandError> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| CommandError {
        code: "no_app_data_dir",
        message: error.to_string(),
    })?;
    spawn_blocking_operation(move || build_support_preview(&app_data_dir)).await
}

/// Generate a fresh redacted snapshot and persist it to the path retained by
/// the backend-owned native save dialog. No renderer-supplied bundle content is
/// accepted, so the backend remains the sole redaction boundary.
#[tauri::command]
#[specta::specta]
pub async fn save_diagnostics(
    app: tauri::AppHandle,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
) -> Result<support_bundle::SavedResult, CommandError> {
    let path = grants.consume(&path_grant, HostPathPurpose::DiagnosticsSave)?;
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err(CommandError {
            code: "invalid_diagnostics_extension",
            message: "support bundles must use a .json extension".to_string(),
        });
    }
    if path.parent().is_none_or(|parent| !parent.is_dir()) {
        return Err(CommandError {
            code: "invalid_path",
            message: "support bundle parent directory does not exist".to_string(),
        });
    }
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(CommandError {
            code: "invalid_path",
            message: "support bundle target must not be a symbolic link".to_string(),
        });
    }
    let app_data_dir = app.path().app_data_dir().map_err(|error| CommandError {
        code: "no_app_data_dir",
        message: error.to_string(),
    })?;
    let result = spawn_blocking_operation(move || {
        let preview = build_support_preview(&app_data_dir)?;
        let staged = StagedArtifact::new(&path)?;
        let mut file = OpenOptions::new().write(true).open(staged.path())?;
        file.write_all(preview.content.as_bytes())?;
        file.flush()?;
        file.sync_data()?;
        drop(file);
        let artifact = staged.commit(ArtifactKind::AnyFile)?;
        Ok(support_bundle::SavedResult {
            path: artifact.local_path,
            byte_size: preview.byte_size,
            generated_at: preview.generated_at,
        })
    })
    .await?;
    grants.record_produced(&result.path)?;
    Ok(result)
}

/// Remove only erasable diagnostic history: rotating crash logs and host-wide
/// recovery records. Per-device journals are intentionally preserved because
/// they back undo/recovery and are not disposable telemetry.
#[tauri::command]
#[specta::specta]
pub async fn wipe_diagnostics(
    app: tauri::AppHandle,
    confirmed: bool,
) -> Result<support_bundle::WipeResult, CommandError> {
    if !confirmed {
        return Err(CommandError {
            code: "confirmation_required",
            message: "wiping diagnostic history requires explicit confirmation".to_string(),
        });
    }
    let app_data_dir = app.path().app_data_dir().map_err(|error| CommandError {
        code: "no_app_data_dir",
        message: error.to_string(),
    })?;
    spawn_blocking_operation(move || {
        Ok(support_bundle::wipe_local_data(
            &app_data_dir,
            &crate::diagnostics::fallback_log_dir(),
        )?)
    })
    .await
}

pub(crate) fn build_support_preview(
    app_data_dir: &Path,
) -> Result<support_bundle::SupportPreview, CommandError> {
    let resolution = adb::locate_adb();
    let mut warnings = Vec::new();
    let mut devices = Vec::new();
    let mut health = None;
    if let Some(path) = resolution.path.as_ref() {
        let transport = adb::ShellTransport::new(path);
        match transport.list_devices() {
            Ok(mut found) => {
                adb::observe_connection_generations(&mut found);
                devices = found;
            }
            Err(error) => warnings.push(error.to_string()),
        }
        health = Some(adb::health::probe(&transport, resolution.version.clone()));
    }
    let source = serde_json::to_value(resolution.source)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    let os = cached_os_info();
    Ok(support_bundle::build_preview(
        app_data_dir,
        &crate::diagnostics::fallback_log_dir(),
        support_bundle::EnvironmentInput {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            tauri_version: tauri::VERSION.to_string(),
            rust_version: env!("CARGO_PKG_RUST_VERSION").to_string(),
            os_family: os.family.clone(),
            os_version: os.version.clone(),
            os_arch: os.arch.clone(),
            adb_available: resolution.path.is_some(),
            adb_source: source,
            adb_version: resolution.version,
            adb_compatibility: resolution.compatibility,
            adb_health: health,
            devices,
            collection_warnings: warnings,
        },
    )?)
}

pub(crate) fn recovery_operation_failure(error: &operations::OperationError) -> String {
    match error {
        operations::OperationError::InvalidId(_) => "invalid recovery operation id".to_string(),
        operations::OperationError::DuplicateId(_) => {
            "a recovery operation with this id is already running".to_string()
        }
        operations::OperationError::Spawn { source, .. } => {
            format!("failed to spawn adb: {source}")
        }
        operations::OperationError::Wait(source) => {
            format!("failed while waiting for adb: {source}")
        }
        operations::OperationError::Input(source) => {
            format!("failed while writing adb input: {source}")
        }
        operations::OperationError::Terminate(source) => {
            format!("failed to terminate the adb process tree: {source}")
        }
        operations::OperationError::OutputRead { stream, source } => {
            format!("failed while reading adb {stream}: {source}")
        }
        operations::OperationError::ReaderPanicked(stream) => {
            format!("the adb {stream} reader stopped unexpectedly")
        }
        operations::OperationError::UnexpectedExit(code) => {
            format!("the adb stream exited unexpectedly with code {code:?}")
        }
        operations::OperationError::Cancelled => "operation was cancelled".to_string(),
        operations::OperationError::Timeout(duration) => {
            format!("adb recovery step timed out after {duration:?}")
        }
        operations::OperationError::OutputTooLarge(limit) => {
            format!("adb recovery output exceeded {limit} bytes")
        }
    }
}

/// Request shape for [`explain_failure`].
#[derive(specta::Type, Debug, serde::Deserialize)]
pub struct ExplainFailureRequest {
    pub manufacturer: Option<String>,
    pub rom: Option<String>,
    pub package_id: Option<String>,
    pub raw_error: Option<String>,
}

/// Bounded request for predictive package hazards shown before a debloat apply.
#[derive(specta::Type, Debug, serde::Deserialize)]
pub struct ExplainPackageHazardsRequest {
    pub manufacturer: Option<String>,
    pub rom: Option<String>,
    pub package_ids: Vec<String>,
}

pub(crate) fn validate_package_hazard_request(
    req: &ExplainPackageHazardsRequest,
) -> Result<(), CommandError> {
    const MAX_PACKAGES: usize = 256;
    if req.package_ids.len() > MAX_PACKAGES {
        return Err(CommandError {
            code: "too_many_packages",
            message: format!("package hazard review accepts at most {MAX_PACKAGES} packages"),
        });
    }
    if let Some(package_id) = req
        .package_ids
        .iter()
        .find(|package_id| !crate::adb::packages::valid_package_name(package_id))
    {
        return Err(CommandError {
            code: "invalid_package",
            message: format!("invalid package id {package_id:?}"),
        });
    }
    Ok(())
}

/// Load quirks from the bundled resource directory and match against the
/// failure context.
/// Returns `Some(quirk)` if a rule applies, `None` if the raw error
/// should be shown as-is.
#[tauri::command]
#[specta::specta]
pub fn explain_failure(
    app: tauri::AppHandle,
    req: ExplainFailureRequest,
) -> Result<Option<Quirk>, CommandError> {
    let resource_dir = app.path().resource_dir().map_err(|e| CommandError {
        code: "no_resource_dir",
        message: e.to_string(),
    })?;
    let quirks_list = quirks::load_dir(&resource_dir.join("quirks")).map_err(|e| CommandError {
        code: "quirks_load_failed",
        message: e.to_string(),
    })?;

    let ctx = DeviceContext {
        manufacturer: req.manufacturer.as_deref(),
        rom: req.rom.as_deref(),
        package_id: req.package_id.as_deref(),
        raw_error: req.raw_error.as_deref(),
    };
    Ok(quirks::explain(&quirks_list, &ctx).cloned())
}

/// Match selected packages against evidence-backed, pre-apply quirk rules.
/// Loading and matching are batched so opening a large review never performs
/// one resource-directory scan per selected package.
#[tauri::command]
#[specta::specta]
pub fn explain_package_hazards(
    app: tauri::AppHandle,
    req: ExplainPackageHazardsRequest,
) -> Result<Vec<Quirk>, CommandError> {
    validate_package_hazard_request(&req)?;

    let resource_dir = app.path().resource_dir().map_err(|error| CommandError {
        code: "no_resource_dir",
        message: error.to_string(),
    })?;
    let quirks_list =
        quirks::load_dir(&resource_dir.join("quirks")).map_err(|error| CommandError {
            code: "quirks_load_failed",
            message: error.to_string(),
        })?;
    Ok(quirks::package_hazards(
        &quirks_list,
        req.manufacturer.as_deref(),
        req.rom.as_deref(),
        &req.package_ids,
    )
    .into_iter()
    .cloned()
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_hazard_validation_rejects_invalid_ids_before_resource_access() {
        let request = ExplainPackageHazardsRequest {
            manufacturer: None,
            rom: None,
            package_ids: vec!["com.example.good".to_string(), "-bad".to_string()],
        };
        let error = validate_package_hazard_request(&request).unwrap_err();
        assert_eq!(error.code, "invalid_package");
    }

    #[test]
    fn package_hazard_validation_bounds_batch_size() {
        let request = ExplainPackageHazardsRequest {
            manufacturer: None,
            rom: None,
            package_ids: (0..257).map(|i| format!("com.example.p{i}")).collect(),
        };
        let error = validate_package_hazard_request(&request).unwrap_err();
        assert_eq!(error.code, "too_many_packages");
    }

    #[test]
    fn recovery_failure_text_keeps_cancellation_and_output_limit_distinct() {
        assert_eq!(
            recovery_operation_failure(&operations::OperationError::Cancelled),
            "operation was cancelled"
        );
        assert_eq!(
            recovery_operation_failure(&operations::OperationError::OutputTooLarge(128)),
            "adb recovery output exceeded 128 bytes"
        );
    }
}
