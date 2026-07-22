//! Tauri `#[command]` glue.
//!
//! Keep this file *thin* — types live in the relevant domain modules,
//! and IO work is delegated. Conventions:
//!
//! - Every `#[tauri::command]` takes either no args, or `tauri::AppHandle`
//!   plus serializable args.
//! - Return types are `Serialize` and live in a domain module.
//! - No business logic inline — it goes to `adb`, `diagnostics`, etc.

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::adb::device::valid_serial;
use crate::adb::packages::valid_package_name;
use crate::adb::parsers::{
    parse_fastboot_devices, parse_ls_output, parse_ps_output, parse_running_services,
    parse_ss_output, parse_uiautomator_dump, FastbootDevice, LayoutNode, NetworkConnection,
    ProcessInfo, RemoteFileEntry, RunningService,
};
use crate::adb::transport::AdbTransport;
use crate::adb::{self, actions};
use crate::apk_metadata;
use crate::backup;
use crate::bugreport;
use crate::fs_util::{ArtifactError, ArtifactKind, HostArtifact, StagedArtifact};
use crate::host_path::{
    open_directory_command, reveal_command, validate_suggested_file_name, HostPathGrant,
    HostPathPurpose, PathGrantError, PathGrantStore,
};
use crate::install;
use crate::journal::{self, Journal, JournalEntry};
use crate::operations::{self, OperationEvent};
use crate::profile;
use crate::quirks::{self, DeviceContext, Quirk};
use crate::recovery_baseline::{self, BaselineActionInput, BaselinePack, RecoveryBaselineDiff};
use crate::remote_files;
use crate::settings;
use crate::support_bundle;

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
fn cached_os_info() -> &'static OsInfo {
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

/// Outcome envelope for `list_devices`. We surface adb-not-found as a
/// structured success-with-zero-devices + an `adb_resolved=false` flag
/// rather than an Err, because "no adb installed" is a normal first-run
/// state, not a runtime fault.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListDevicesResult {
    pub adb_resolved: bool,
    pub adb_path: Option<String>,
    pub devices: Vec<adb::Device>,
}

#[tauri::command]
#[specta::specta]
pub fn list_devices() -> Result<ListDevicesResult, adb::TransportError> {
    let resolution = adb::locate_adb();
    let Some(path) = resolution.path.as_ref() else {
        return Ok(ListDevicesResult {
            adb_resolved: false,
            adb_path: None,
            devices: Vec::new(),
        });
    };

    let transport = adb::ShellTransport::new(path);
    let mut fingerprints = HashMap::new();
    collect_devices(&transport, &mut fingerprints)
}

fn collect_devices(
    transport: &adb::ShellTransport,
    fingerprint_cache: &mut HashMap<String, String>,
) -> Result<ListDevicesResult, adb::TransportError> {
    let mut devices = transport.list_devices()?;
    adb::observe_connection_generations(&mut devices);
    for device in devices
        .iter_mut()
        .filter(|device| device.state.is_actionable())
    {
        let cache_key = format!(
            "{}|{}|{}|{}|{}",
            device.serial,
            device.transport_id.unwrap_or_default(),
            device.model.as_deref().unwrap_or_default(),
            device.product.as_deref().unwrap_or_default(),
            device.device.as_deref().unwrap_or_default()
        );
        let fingerprint = if let Some(value) = fingerprint_cache.get(&cache_key) {
            Some(value.clone())
        } else {
            transport
                .shell_target(&device.target(), &["getprop", "ro.build.fingerprint"])
                .map(|value| value.trim().to_string())
                .ok()
                .filter(|value| !value.is_empty())
                .inspect(|value| {
                    fingerprint_cache.insert(cache_key, value.clone());
                })
        };
        device.build_fingerprint = fingerprint;
    }
    let live_keys: std::collections::HashSet<String> = devices
        .iter()
        .map(|device| {
            format!(
                "{}|{}|{}|{}|{}",
                device.serial,
                device.transport_id.unwrap_or_default(),
                device.model.as_deref().unwrap_or_default(),
                device.product.as_deref().unwrap_or_default(),
                device.device.as_deref().unwrap_or_default()
            )
        })
        .collect();
    fingerprint_cache.retain(|key, _| live_keys.contains(key));
    Ok(ListDevicesResult {
        adb_resolved: true,
        adb_path: Some(transport.adb_path.display().to_string()),
        devices,
    })
}

#[derive(specta::Type, Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeviceLifecycleEvent {
    Snapshot {
        result: ListDevicesResult,
        health: Option<Box<adb::health::AdbHealth>>,
        observed_at: String,
    },
    Error {
        message: String,
        observed_at: String,
    },
}

/// Maintain one app-wide live device snapshot. The frontend starts this once
/// and all routes subscribe to the renderer's shared external store instead
/// of polling ADB independently.
#[tauri::command]
#[specta::specta]
pub async fn watch_devices(
    operation_id: String,
    on_event: tauri::ipc::Channel<DeviceLifecycleEvent>,
) -> Result<(), CommandError> {
    spawn_blocking_operation(move || {
        let cancellation = operations::register_cancellable(&operation_id)?;
        let mut fingerprints = HashMap::new();
        let mut last_snapshot = String::new();
        let mut last_health = None;
        let mut health_checked_at: Option<std::time::Instant> = None;

        while !cancellation.is_cancelled() {
            let resolution = adb::locate_adb();
            let result = if let Some(path) = resolution.path.as_ref() {
                let transport = adb::ShellTransport::new(path);
                collect_devices(&transport, &mut fingerprints).map_err(CommandError::from)
            } else {
                Ok(ListDevicesResult {
                    adb_resolved: false,
                    adb_path: None,
                    devices: Vec::new(),
                })
            };

            match result {
                Ok(result) => {
                    let encoded = serde_json::to_string(&result).unwrap_or_default();
                    if encoded != last_snapshot {
                        last_snapshot = encoded;
                        let _ = on_event.send(DeviceLifecycleEvent::Snapshot {
                            result: result.clone(),
                            health: last_health.clone().map(Box::new),
                            observed_at: iso_now(),
                        });
                    }

                    let health_due = health_checked_at.map_or(true, |checked| {
                        checked.elapsed() >= std::time::Duration::from_secs(10)
                    });
                    if health_due {
                        health_checked_at = Some(std::time::Instant::now());
                        let health = resolution.path.as_ref().map(|path| {
                            let transport = adb::ShellTransport::new(path);
                            adb::health::probe(&transport, resolution.version.clone())
                        });
                        if health != last_health {
                            last_health = health.clone();
                            let _ = on_event.send(DeviceLifecycleEvent::Snapshot {
                                result,
                                health: health.map(Box::new),
                                observed_at: iso_now(),
                            });
                        }
                    }
                }
                Err(error) => {
                    let message = error.to_string();
                    if message != last_snapshot {
                        last_snapshot = message.clone();
                        let _ = on_event.send(DeviceLifecycleEvent::Error {
                            message,
                            observed_at: iso_now(),
                        });
                    }
                }
            }

            for _ in 0..20 {
                if cancellation.is_cancelled() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        Ok(())
    })
    .await
}

const ADB_RECOVERY_STEPS: [&[&str]; 3] = [
    &["kill-server"],
    &["start-server"],
    &["reconnect", "offline"],
];

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdbRecoveryOutcome {
    Pending,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct AdbRecoveryRecord {
    pub schema_version: u32,
    pub operation_id: String,
    pub operation: &'static str,
    pub confirmation_source: &'static str,
    pub outcome: AdbRecoveryOutcome,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub commands: Vec<Vec<String>>,
    pub health_before: Option<adb::health::AdbHealth>,
    pub health_after: Option<adb::health::AdbHealth>,
    pub failure: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct AdbRecoveryResult {
    pub record: AdbRecoveryRecord,
    pub record_path: String,
}

/// Restart the local ADB server and reconnect only offline transports. This is
/// a host-wide mutation, so the renderer must review the exact argv and send
/// an explicit confirmation. A synced pending record lands before `kill-server`.
#[tauri::command]
#[specta::specta]
pub async fn recover_adb(
    app: tauri::AppHandle,
    confirmed: bool,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<AdbRecoveryResult, CommandError> {
    if !confirmed {
        return Err(CommandError {
            code: "confirmation_required",
            message: "ADB recovery requires explicit confirmation".to_string(),
        });
    }

    let resolution = adb::locate_adb();
    let adb_path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)
        .map(PathBuf::from)?;
    let record_path = app
        .path()
        .app_data_dir()
        .map_err(|error| CommandError {
            code: "no_app_data_dir",
            message: error.to_string(),
        })?
        .join("host-operations.jsonl");
    let sink = operations::channel_sink(on_event);

    spawn_blocking_operation(move || {
        // Register before health probing or durable-intent IO so an immediate
        // Cancel click cannot miss the operation and orphan the later child.
        let cancellation = operations::register_cancellable(&operation_id)?;
        let transport = adb::ShellTransport::new(&adb_path);
        let started_at = iso_now();
        let commands = ADB_RECOVERY_STEPS
            .iter()
            .map(|args| args.iter().map(|arg| (*arg).to_string()).collect())
            .collect::<Vec<Vec<String>>>();
        let mut record = AdbRecoveryRecord {
            schema_version: 1,
            operation_id: operation_id.clone(),
            operation: "adb_server_recovery",
            confirmation_source: "devices_health_review",
            outcome: AdbRecoveryOutcome::Pending,
            started_at,
            completed_at: None,
            commands: commands.clone(),
            health_before: None,
            health_after: None,
            failure: None,
        };
        append_host_operation(&record_path, &record)?;
        if !cancellation.is_cancelled() {
            record.health_before = Some(adb::health::probe(&transport, resolution.version));
        }

        let stages = commands
            .iter()
            .map(|args| (format!("adb {}", args.join(" ")), args.clone()))
            .collect::<Vec<_>>();
        let sequence = operations::run_registered_sequence(
            &adb_path,
            &stages,
            std::time::Duration::from_secs(30),
            &operation_id,
            sink,
            &cancellation,
        );

        match sequence {
            Ok(outputs) => {
                if let Some((index, output)) = outputs
                    .iter()
                    .enumerate()
                    .find(|(_, output)| !output.success())
                {
                    record.outcome = AdbRecoveryOutcome::Failed;
                    record.failure = Some(format!(
                        "adb {} exited with code {}: {}",
                        commands[index].join(" "),
                        output.code.unwrap_or(-1),
                        diagnostic_text(if output.stderr.trim().is_empty() {
                            &output.stdout
                        } else {
                            &output.stderr
                        })
                    ));
                } else {
                    record.outcome = AdbRecoveryOutcome::Succeeded;
                }
            }
            Err(error) => {
                record.outcome = if matches!(error, operations::OperationError::Cancelled) {
                    AdbRecoveryOutcome::Cancelled
                } else {
                    AdbRecoveryOutcome::Failed
                };
                record.failure = Some(recovery_operation_failure(&error));
            }
        }

        record.completed_at = Some(iso_now());
        if record.outcome != AdbRecoveryOutcome::Cancelled {
            record.health_after = Some(adb::health::probe(&transport, None));
        }
        append_host_operation(&record_path, &record)?;

        Ok(AdbRecoveryResult {
            record,
            record_path: record_path.display().to_string(),
        })
    })
    .await
}

fn host_operation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn append_host_operation(path: &Path, record: &AdbRecoveryRecord) -> Result<(), CommandError> {
    let _guard = host_operation_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, record).map_err(|error| CommandError {
        code: "host_operation_serialize_failed",
        message: error.to_string(),
    })?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_data()?;
    Ok(())
}

fn diagnostic_text(value: &str) -> String {
    let normalized = value
        .chars()
        .filter(|character| *character == '\n' || !character.is_control())
        .collect::<String>();
    normalized.trim().chars().take(1_024).collect()
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
    if path.parent().map_or(true, |parent| !parent.is_dir()) {
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
    spawn_blocking_operation(move || {
        let preview = build_support_preview(&app_data_dir)?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)?;
        file.write_all(preview.content.as_bytes())?;
        file.flush()?;
        file.sync_data()?;
        Ok(support_bundle::SavedResult {
            path: path.display().to_string(),
            byte_size: preview.byte_size,
            generated_at: preview.generated_at,
        })
    })
    .await
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

fn build_support_preview(
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

fn recovery_operation_failure(error: &operations::OperationError) -> String {
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
        operations::OperationError::Cancelled => "operation was cancelled".to_string(),
        operations::OperationError::Timeout(duration) => {
            format!("adb recovery step timed out after {duration:?}")
        }
        operations::OperationError::OutputTooLarge(limit) => {
            format!("adb recovery output exceeded {limit} bytes")
        }
    }
}

#[derive(specta::Type, Serialize)]
pub struct ListWirelessServicesResult {
    pub adb_resolved: bool,
    pub adb_path: Option<String>,
    pub services: Vec<adb::WirelessAdbService>,
}

#[tauri::command]
#[specta::specta]
pub fn list_wireless_services() -> Result<ListWirelessServicesResult, adb::TransportError> {
    let resolution = adb::locate_adb();
    let Some(path) = resolution.path.as_ref() else {
        return Ok(ListWirelessServicesResult {
            adb_resolved: false,
            adb_path: None,
            services: Vec::new(),
        });
    };

    let transport = adb::ShellTransport::new(path);
    let services = adb::list_mdns_services(&transport)?;
    Ok(ListWirelessServicesResult {
        adb_resolved: true,
        adb_path: Some(path.clone()),
        services,
    })
}

#[tauri::command]
#[specta::specta]
pub fn pair_wireless(
    request: adb::WirelessPairRequest,
) -> Result<adb::WirelessCommandResult, adb::WirelessCommandError> {
    let resolution = adb::locate_adb();
    let path = resolution.path.as_ref().ok_or_else(|| {
        adb::WirelessCommandError::unavailable(
            adb::TransportError::AdbNotFound,
            &request.host,
            resolution.version.clone(),
        )
    })?;
    let transport = adb::ShellTransport::new(path);
    adb::pair_wireless(&transport, &request, resolution.version)
}

#[tauri::command]
#[specta::specta]
pub fn connect_wireless(
    app: tauri::AppHandle,
    request: adb::WirelessConnectRequest,
) -> Result<adb::WirelessCommandResult, adb::WirelessCommandError> {
    let resolution = adb::locate_adb();
    let path = resolution.path.as_ref().ok_or_else(|| {
        adb::WirelessCommandError::unavailable(
            adb::TransportError::AdbNotFound,
            &request.host,
            resolution.version.clone(),
        )
    })?;
    let transport = adb::ShellTransport::new(path);
    let result = adb::connect_wireless(&transport, &request, resolution.version)?;
    // Best-effort: record the endpoint so it appears in reconnect history. A
    // settings write failure must never mask a successful connect.
    if let Ok(app_data_dir) = settings_app_data_dir(&app) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0);
        let _ =
            settings::record_wireless_endpoint(&app_data_dir, &request.host, request.port, now_ms);
    }
    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub fn list_packages(
    target: adb::DeviceTarget,
    filter: adb::PackageFilter,
    #[allow(non_snake_case)] userId: u32,
) -> Result<adb::PackageListing, adb::TransportError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    adb::validate_device_target(&transport, &target)?;
    adb::list_packages_with_capability(&transport, &target, filter, userId)
}

/// Lazily enrich one package row after it approaches the renderer viewport.
/// The domain service bounds concurrent pulls and validates a fresh APK
/// size/timestamp before consulting its process-local cache.
#[tauri::command]
#[specta::specta]
pub async fn get_package_metadata(
    target: adb::DeviceTarget,
    package: String,
    #[allow(non_snake_case)] userId: u32,
) -> Result<apk_metadata::AppPackageMetadata, CommandError> {
    if !valid_package_name(&package) {
        return Err(CommandError {
            code: "invalid_package",
            message: "invalid package id".to_string(),
        });
    }
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)
        .map(PathBuf::from)?;
    spawn_blocking_operation(move || {
        let transport = adb::ShellTransport::new(path);
        adb::validate_device_target(&transport, &target)?;
        Ok(apk_metadata::load_package_metadata(
            &transport, &target, userId, &package,
        )?)
    })
    .await
}

/// Enumerate Android users on a device so the renderer can offer an
/// explicit `--user` target for package workflows.
#[tauri::command]
#[specta::specta]
pub fn list_users(target: adb::DeviceTarget) -> Result<Vec<adb::AndroidUser>, adb::TransportError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    adb::validate_device_target(&transport, &target)?;
    adb::list_users(&transport, &target)
}

#[derive(specta::Type, Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfilePreviewStatus {
    Ready,
    AlreadyMatches,
    Missing,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ProfilePreviewRow {
    pub action: profile::ProfileAction,
    pub plan: actions::PlannedAction,
    pub current_state: String,
    pub expected_state: String,
    pub status: ProfilePreviewStatus,
    pub reason: String,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ProfilePreview {
    pub source_version: String,
    pub profile: profile::Profile,
    pub migration: Option<profile::ProfileMigration>,
    pub compatible: bool,
    pub compatibility_issues: Vec<String>,
    pub android_user: Option<u32>,
    pub rows: Vec<ProfilePreviewRow>,
}

/// Import a profile through a one-shot native read grant and build a complete,
/// read-only device/user/package diff. Legacy v1 input is returned only as an
/// explicit migration candidate and cannot be applied as-is.
#[tauri::command]
#[specta::specta]
pub fn inspect_profile(
    grants: tauri::State<'_, PathGrantStore>,
    target: adb::DeviceTarget,
    path_grant: String,
) -> Result<ProfilePreview, CommandError> {
    let path = grants.consume(&path_grant, HostPathPurpose::ProfileOpen)?;
    let document = profile::inspect(&path)?;
    let (source_version, profile, migration) = match document {
        profile::ProfileDocument::Current { profile } => (profile.version.clone(), profile, None),
        profile::ProfileDocument::MigrationAvailable { migration } => (
            migration.from_version.clone(),
            migration.profile.clone(),
            Some(migration),
        ),
    };

    let resolution = adb::locate_adb();
    let adb_path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(adb_path);
    adb::validate_device_target(&transport, &target)?;
    let info = adb::get_device_info(&transport, &target)?;
    let users = adb::list_users(&transport, &target)?;
    let mut compatibility_issues = profile::device_match_issues(
        &profile,
        &target.serial,
        info.manufacturer.as_deref(),
        info.model.as_deref(),
        info.sdk_level
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok()),
    );
    let android_user = match profile::resolve_user(&profile, &users) {
        Ok(user_id) => Some(user_id),
        Err(mut issues) => {
            compatibility_issues.append(&mut issues);
            None
        }
    };
    let rows = if let Some(user_id) = android_user {
        let packages = adb::list_packages(&transport, &target, adb::PackageFilter::All, user_id)?;
        profile_preview_rows(&profile, &target, user_id, &packages)
    } else {
        Vec::new()
    };
    Ok(ProfilePreview {
        source_version,
        profile,
        migration,
        compatible: compatibility_issues.is_empty(),
        compatibility_issues,
        android_user,
        rows,
    })
}

/// Validate and atomically export a current v2 profile through a purpose-
/// scoped native save grant. This is also the only GUI path that finalizes a
/// reviewed v1 migration.
#[tauri::command]
#[specta::specta]
pub fn save_profile(
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    profile: profile::Profile,
) -> Result<HostArtifact, CommandError> {
    let path = grants.consume(&path_grant, HostPathPurpose::ProfileSave)?;
    Ok(profile::save(&path, &profile)?)
}

fn profile_preview_rows(
    profile: &profile::Profile,
    target: &adb::DeviceTarget,
    user_id: u32,
    packages: &[adb::AppPackage],
) -> Vec<ProfilePreviewRow> {
    let requests = profile::requests_for(
        profile,
        target,
        user_id,
        actions::ConfirmationSource::ProfilePreview,
    );
    profile
        .actions
        .iter()
        .cloned()
        .zip(requests.into_iter().map(actions::plan))
        .map(|(action, plan)| {
            let package = packages
                .iter()
                .find(|candidate| candidate.package == action.package);
            let current_state = match package {
                Some(package) if package.archived => "archived",
                Some(package) if package.enabled => "enabled",
                Some(_) => "disabled",
                None => "missing",
            }
            .to_string();
            let expected_state = match action.kind {
                actions::ActionKind::Disable => "disabled",
                actions::ActionKind::Enable | actions::ActionKind::RestoreExistingForUser => {
                    "enabled"
                }
                actions::ActionKind::UninstallForUser => "uninstalled_for_user",
                actions::ActionKind::ClearData => "data_cleared",
                actions::ActionKind::ForceStop => "stopped",
                _ => "reviewed_action",
            }
            .to_string();
            let (status, reason) = match (package, action.kind) {
                (Some(package), _) if package.archived => (
                    ProfilePreviewStatus::Missing,
                    "package is archived; restore it from Apps before applying profile actions"
                        .to_string(),
                ),
                (None, actions::ActionKind::UninstallForUser) => (
                    ProfilePreviewStatus::AlreadyMatches,
                    "package is already absent for this user".to_string(),
                ),
                (None, actions::ActionKind::RestoreExistingForUser) => (
                    ProfilePreviewStatus::Ready,
                    "restore will ask Android to install the retained system package".to_string(),
                ),
                (None, _) => (
                    ProfilePreviewStatus::Missing,
                    "package is not installed for this user".to_string(),
                ),
                (Some(package), actions::ActionKind::Disable) if !package.enabled => (
                    ProfilePreviewStatus::AlreadyMatches,
                    "package is already disabled".to_string(),
                ),
                (Some(package), actions::ActionKind::Enable) if package.enabled => (
                    ProfilePreviewStatus::AlreadyMatches,
                    "package is already enabled".to_string(),
                ),
                _ => (
                    ProfilePreviewStatus::Ready,
                    "canonical action is ready for explicit review".to_string(),
                ),
            };
            ProfilePreviewRow {
                action,
                plan,
                current_state,
                expected_state,
                status,
                reason,
            }
        })
        .collect()
}

/// Synthesise an ADB action without running it. Pure: this is the
/// preview surface the confirmation dialog renders before the user
/// commits.
#[tauri::command]
#[specta::specta]
pub fn plan_action(
    mut request: actions::ActionRequest,
) -> Result<actions::PlannedAction, CommandError> {
    if !matches!(
        request.kind,
        actions::ActionKind::Disable
            | actions::ActionKind::Enable
            | actions::ActionKind::Archive
            | actions::ActionKind::RequestUnarchive
            | actions::ActionKind::UninstallForUser
            | actions::ActionKind::ClearData
            | actions::ActionKind::ForceStop
    ) {
        return Err(CommandError {
            code: "invalid_action_kind",
            message: "use the dedicated audited planner for this operation kind".to_string(),
        });
    }
    request.pack_context = None;
    request.context = actions::ActionContext {
        confirmation_source: actions::ConfirmationSource::AppsPreview,
        ..Default::default()
    };
    Ok(actions::plan(request))
}

/// Build one reviewed, reversible package-action plan for multiple packages.
/// Every item is bound to the same immutable device target, Android user, and
/// action kind; destructive or conditionally-reversible kinds stay on the
/// single-item path.
#[tauri::command]
#[specta::specta]
pub fn plan_action_batch(
    requests: Vec<actions::ActionRequest>,
) -> Result<BatchActionPlan, CommandError> {
    if !(2..=MAX_ACTION_BATCH_ITEMS).contains(&requests.len()) {
        return Err(CommandError {
            code: "invalid_action_batch",
            message: format!(
                "a package batch must contain between 2 and {MAX_ACTION_BATCH_ITEMS} items"
            ),
        });
    }
    let first = requests.first().expect("length checked");
    if !matches!(
        first.kind,
        actions::ActionKind::Disable
            | actions::ActionKind::Enable
            | actions::ActionKind::Archive
            | actions::ActionKind::RequestUnarchive
    ) {
        return Err(CommandError {
            code: "invalid_action_kind",
            message: "batch actions must have a losslessly reversible inverse".to_string(),
        });
    }
    let target = first.target.clone();
    let serial = first.serial.clone();
    let user_id = first.user_id;
    let kind = first.kind;
    let mut packages = HashSet::with_capacity(requests.len());
    let mut plans = Vec::with_capacity(requests.len());
    for mut request in requests {
        if request.serial != serial
            || request.target != target
            || request.user_id != user_id
            || request.kind != kind
        {
            return Err(CommandError {
                code: "mixed_action_batch",
                message: "every batch item must use the same device target, Android user, and action kind"
                    .to_string(),
            });
        }
        validate_package_arg(&request.package)?;
        if !packages.insert(request.package.clone()) {
            return Err(CommandError {
                code: "duplicate_batch_package",
                message: format!(
                    "package {} appears more than once in the batch",
                    request.package
                ),
            });
        }
        request.pack_context = None;
        request.context = actions::ActionContext {
            confirmation_source: actions::ConfirmationSource::AppsPreview,
            ..Default::default()
        };
        plans.push(actions::plan(request));
    }
    let description = batch_action_description(kind, plans.len(), user_id);
    Ok(BatchActionPlan { plans, description })
}

fn batch_action_description(kind: actions::ActionKind, count: usize, user_id: u32) -> String {
    let action = match kind {
        actions::ActionKind::Disable => "Disable",
        actions::ActionKind::Enable => "Enable",
        actions::ActionKind::Archive => "Archive",
        actions::ActionKind::RequestUnarchive => "Request unarchive for",
        _ => "Apply action to",
    };
    format!("{action} {count} packages for Android user {user_id}")
}

/// Generic Tauri-command error envelope so the JS side gets the same
/// shape regardless of whether the underlying failure was a transport
/// error or a filesystem error from the journal.
#[derive(specta::Type, Debug, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct CommandError {
    /// Stable string code for client-side branching (e.g. "adb_not_found").
    pub code: &'static str,
    pub message: String,
}

impl From<adb::TransportError> for CommandError {
    fn from(e: adb::TransportError) -> Self {
        let code: &'static str = match &e {
            adb::TransportError::AdbNotFound => "adb_not_found",
            adb::TransportError::Spawn(_) => "spawn_failed",
            adb::TransportError::Exit { .. } => "adb_exit",
            adb::TransportError::Signaled { .. } => "adb_signaled",
            adb::TransportError::Timeout(_) => "adb_timeout",
            adb::TransportError::OutputLimit { .. } => "subprocess_output_limit",
            adb::TransportError::Parse(_) => "parse_error",
        };
        Self {
            code,
            message: e.to_string(),
        }
    }
}

impl From<std::io::Error> for CommandError {
    fn from(e: std::io::Error) -> Self {
        Self {
            code: "io_error",
            message: e.to_string(),
        }
    }
}

impl From<ArtifactError> for CommandError {
    fn from(error: ArtifactError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<PathGrantError> for CommandError {
    fn from(error: PathGrantError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<recovery_baseline::RecoveryBaselineError> for CommandError {
    fn from(error: recovery_baseline::RecoveryBaselineError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<profile::ProfileError> for CommandError {
    fn from(error: profile::ProfileError) -> Self {
        let code = match &error {
            profile::ProfileError::Read { .. } => "profile_read_failed",
            profile::ProfileError::Parse { .. } => "profile_parse_failed",
            profile::ProfileError::Validate { .. } => "profile_invalid",
            profile::ProfileError::Serialize(_) => "profile_serialize_failed",
            profile::ProfileError::Save(_) => "profile_save_failed",
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}

impl From<remote_files::RemoteFileError> for CommandError {
    fn from(error: remote_files::RemoteFileError) -> Self {
        Self {
            code: "invalid_remote_file_operation",
            message: error.to_string(),
        }
    }
}

impl From<operations::OperationError> for CommandError {
    fn from(error: operations::OperationError) -> Self {
        let code = match &error {
            operations::OperationError::InvalidId(_) => "invalid_operation_id",
            operations::OperationError::DuplicateId(_) => "operation_already_running",
            operations::OperationError::Spawn { .. } => "spawn_failed",
            operations::OperationError::Wait(_) => "process_wait_failed",
            operations::OperationError::Cancelled => "operation_cancelled",
            operations::OperationError::Timeout(_) => "operation_timeout",
            operations::OperationError::OutputTooLarge(_) => "operation_output_too_large",
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}

impl From<install::InstallError> for CommandError {
    fn from(error: install::InstallError) -> Self {
        match error {
            install::InstallError::InvalidSource(message) => Self {
                code: "invalid_install_source",
                message,
            },
            install::InstallError::Archive(error) => Self {
                code: "invalid_install_archive",
                message: error.to_string(),
            },
            install::InstallError::Io(error) => Self::from(error),
            install::InstallError::Operation(error) => Self::from(error),
        }
    }
}

impl From<backup::BackupError> for CommandError {
    fn from(error: backup::BackupError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<bugreport::BugreportError> for CommandError {
    fn from(error: bugreport::BugreportError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<apk_metadata::MetadataError> for CommandError {
    fn from(error: apk_metadata::MetadataError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<settings::SettingsError> for CommandError {
    fn from(error: settings::SettingsError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

async fn spawn_blocking_operation<T, F>(operation: F) -> Result<T, CommandError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CommandError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| CommandError {
            code: "operation_join_failed",
            message: format!("background operation task failed: {error}"),
        })?
}

fn settings_app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, CommandError> {
    app.path().app_data_dir().map_err(|error| CommandError {
        code: "no_app_data_dir",
        message: error.to_string(),
    })
}

/// Load the fixed backend-owned settings document and perform the one-time
/// import of bounded legacy renderer values when needed.
#[tauri::command]
#[specta::specta]
pub async fn initialize_settings(
    app: tauri::AppHandle,
    legacy: settings::LegacySettingsImport,
) -> Result<settings::SettingsLoadResult, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::initialize(&app_data_dir, legacy)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_settings_language(
    app: tauri::AppHandle,
    language: settings::SettingsLanguage,
) -> Result<settings::SettingsSnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::set_language(&app_data_dir, language)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings_mirror_preset(
    app: tauri::AppHandle,
    device_identity: String,
) -> Result<Option<settings::MirrorPreset>, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::get_mirror_preset(
            &app_data_dir,
            &device_identity,
        )?)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_settings_mirror_preset(
    app: tauri::AppHandle,
    device_identity: String,
    preset: settings::MirrorPreset,
) -> Result<settings::SettingsSnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::set_mirror_preset(
            &app_data_dir,
            &device_identity,
            preset,
        )?)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn reset_settings_mirror_preset(
    app: tauri::AppHandle,
    device_identity: String,
) -> Result<settings::SettingsSnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::reset_mirror_preset(
            &app_data_dir,
            &device_identity,
        )?)
    })
    .await
}

/// Record the selected device's current build fingerprint and report whether it
/// differs from the last one Droidsmith saw for it (R-087). A changed
/// fingerprint means the device was updated (OTA) since it was last used, so the
/// renderer can prompt a debloat-drift review. Devices without a verified
/// fingerprint are treated as unchanged.
#[tauri::command]
#[specta::specta]
pub async fn observe_device_fingerprint(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
) -> Result<settings::FingerprintObservation, CommandError> {
    let Some(fingerprint) = target.build_fingerprint.clone() else {
        return Ok(settings::FingerprintObservation {
            changed: false,
            previous: None,
        });
    };
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::record_device_fingerprint(
            &app_data_dir,
            &target.serial,
            &fingerprint,
        )?)
    })
    .await
}

/// Return the persisted wireless-endpoint history and the opt-in
/// reconnect-on-launch flag so the renderer can offer one-click reconnect.
#[tauri::command]
#[specta::specta]
pub async fn list_wireless_history(
    app: tauri::AppHandle,
) -> Result<settings::WirelessHistorySnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::list_wireless_history(&app_data_dir)?)).await
}

/// Remove one endpoint from the wireless history.
#[tauri::command]
#[specta::specta]
pub async fn forget_wireless_endpoint(
    app: tauri::AppHandle,
    host: String,
    port: u16,
) -> Result<settings::WirelessHistorySnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::forget_wireless_endpoint(
            &app_data_dir,
            &host,
            port,
        )?)
    })
    .await
}

/// Persist the opt-in "reconnect known wireless devices on launch" preference.
#[tauri::command]
#[specta::specta]
pub async fn set_wireless_auto_reconnect(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<settings::WirelessHistorySnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::set_wireless_auto_reconnect(
            &app_data_dir,
            enabled,
        )?)
    })
    .await
}

/// List the saved Logcat query presets for the global scope and, when a device
/// identity is supplied, that device's scope. Only query definitions are
/// stored; captured log lines never enter the settings document.
#[tauri::command]
#[specta::specta]
pub async fn list_logcat_queries(
    app: tauri::AppHandle,
    device_identity: Option<String>,
) -> Result<settings::LogcatQueryLibrary, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::list_logcat_queries(
            &app_data_dir,
            device_identity.as_deref(),
        )?)
    })
    .await
}

/// Persist the full ordered list of Logcat query presets for one scope. This
/// single write covers create, rename, duplicate, reorder, and delete; an empty
/// list clears the scope. Each preset is validated (including a linear-time
/// regex guard) before it is written.
#[tauri::command]
#[specta::specta]
pub async fn save_logcat_queries(
    app: tauri::AppHandle,
    scope: settings::LogcatQueryScope,
    device_identity: Option<String>,
    queries: Vec<settings::LogcatQuery>,
) -> Result<settings::LogcatQueryLibrary, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::save_logcat_queries(
            &app_data_dir,
            scope,
            device_identity.as_deref(),
            queries,
        )?)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn reset_settings(
    app: tauri::AppHandle,
    scope: settings::SettingsScope,
) -> Result<settings::SettingsSnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::reset(&app_data_dir, scope)?)).await
}

/// Export only a named settings scope through a backend-issued save grant.
/// The internal settings path never crosses IPC.
#[tauri::command]
#[specta::specta]
pub async fn export_settings(
    app: tauri::AppHandle,
    grants: tauri::State<'_, PathGrantStore>,
    scope: settings::SettingsScope,
    path_grant: String,
) -> Result<settings::SettingsExportResult, CommandError> {
    let destination = grants.consume(&path_grant, HostPathPurpose::SettingsExport)?;
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::export(&app_data_dir, scope, &destination)?))
        .await
}

/// Parse and validate a portable settings document, then return only a
/// redacted change summary plus an opaque, short-lived import id.
#[tauri::command]
#[specta::specta]
pub async fn preview_settings_import(
    app: tauri::AppHandle,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
) -> Result<settings::SettingsImportPreview, CommandError> {
    let source = grants.consume(&path_grant, HostPathPurpose::SettingsImport)?;
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::preview_import(&app_data_dir, &source)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn apply_settings_import(
    app: tauri::AppHandle,
    import_id: String,
    mode: settings::SettingsImportMode,
) -> Result<settings::SettingsImportResult, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::apply_import(&app_data_dir, &import_id, mode)?))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_settings_import_backup(
    app: tauri::AppHandle,
) -> Result<settings::SettingsSnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::restore_import_backup(&app_data_dir)?)).await
}

#[tauri::command]
#[specta::specta]
pub fn has_settings_import_backup(app: tauri::AppHandle) -> Result<bool, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    Ok(settings::import_backup_available(&app_data_dir))
}

/// Open a backend-owned native file dialog and retain its result as a short-
/// lived, purpose-scoped, one-shot grant. Privileged commands accept only the
/// opaque grant id, never a renderer-authored host path.
#[tauri::command]
#[specta::specta]
pub async fn select_host_path(
    app: tauri::AppHandle,
    grants: tauri::State<'_, PathGrantStore>,
    purpose: HostPathPurpose,
    suggested_name: Option<String>,
) -> Result<Option<HostPathGrant>, CommandError> {
    use tauri_plugin_dialog::DialogExt;

    let suggested_name = validate_suggested_file_name(suggested_name)?;
    let mut dialog = app.dialog().file().set_title(purpose.dialog_title());
    if let Some(name) = suggested_name {
        dialog = dialog.set_file_name(name);
    }
    if let Some((name, extensions)) = purpose.filter() {
        dialog = dialog.add_filter(name, extensions);
    }
    let selected = if purpose.is_write() {
        dialog.blocking_save_file()
    } else {
        dialog.blocking_pick_file()
    };
    let Some(selected) = selected else {
        return Ok(None);
    };
    let selected_path = selected
        .simplified()
        .into_path()
        .map_err(|error| CommandError {
            code: "path_grant_invalid_path",
            message: error.to_string(),
        })?;
    Ok(Some(grants.issue(&selected_path, purpose)?))
}

/// Issue a one-shot path grant for a file dropped onto the window by the OS.
/// Only `InstallOpen` purpose is accepted, and the path must exist, be absolute,
/// and carry a supported extension (.apk, .apks, .xapk, .apkm). This does not
/// bypass the grant model: the install command still consumes the grant normally.
#[tauri::command]
#[specta::specta]
pub fn grant_dropped_path(
    grants: tauri::State<'_, PathGrantStore>,
    path: String,
) -> Result<HostPathGrant, CommandError> {
    let path = std::path::PathBuf::from(&path);
    if !path.is_absolute() {
        return Err(CommandError {
            code: "dropped_path_relative",
            message: "dropped path must be absolute".to_string(),
        });
    }
    if !path.is_file() {
        return Err(CommandError {
            code: "dropped_path_not_file",
            message: "dropped path does not exist or is not a regular file".to_string(),
        });
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "apk" | "apks" | "xapk" | "apkm") {
        return Err(CommandError {
            code: "dropped_path_wrong_type",
            message: format!(
                "dropped file must be an Android package (.apk, .apks, .xapk, .apkm), got .{extension}"
            ),
        });
    }
    Ok(grants.issue(&path, HostPathPurpose::InstallOpen)?)
}

#[derive(specta::Type, Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectResult {
    pub serial: String,
    pub disconnected: bool,
    pub message: String,
}

/// Disconnect a device safely. Wireless devices are disconnected via
/// `adb disconnect`; USB devices cannot be disconnected programmatically
/// but the user is advised that it is safe to unplug.
#[tauri::command]
#[specta::specta]
pub fn disconnect_device(target: adb::DeviceTarget) -> Result<DisconnectResult, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    if target.transport_kind == adb::DeviceTransportKind::Usb {
        return Ok(DisconnectResult {
            serial: target.serial,
            disconnected: false,
            message: "USB device cannot be disconnected via ADB. It is safe to unplug the cable."
                .to_string(),
        });
    }
    let result = transport.adb(&["disconnect", &target.serial]);
    match result {
        Ok(stdout) => Ok(DisconnectResult {
            serial: target.serial,
            disconnected: true,
            message: stdout.trim().to_string(),
        }),
        Err(error) => Ok(DisconnectResult {
            serial: target.serial,
            disconnected: false,
            message: format!("disconnect failed: {error}"),
        }),
    }
}

/// Open the OS file manager at an artifact Droidsmith produced this session.
/// `path` must equal a save-dialog destination the backend itself issued; any
/// other renderer-supplied path is rejected, so the renderer can never drive an
/// open of an arbitrary location. The file manager is spawned detached.
#[tauri::command]
#[specta::specta]
pub fn reveal_in_folder(
    grants: tauri::State<'_, PathGrantStore>,
    path: String,
) -> Result<(), CommandError> {
    if !grants.is_revealable(&path) {
        return Err(CommandError {
            code: "reveal_path_not_produced",
            message: "only artifacts Droidsmith produced this session can be revealed".to_string(),
        });
    }
    let target = Path::new(&path);
    if !target.exists() {
        return Err(CommandError {
            code: "reveal_path_missing",
            message: "the artifact is no longer at that location".to_string(),
        });
    }
    let (program, args) = reveal_command(target);
    std::process::Command::new(&program)
        .args(&args)
        .spawn()
        .map_err(|error| CommandError {
            code: "reveal_failed",
            message: format!("could not open the file manager: {error}"),
        })?;
    Ok(())
}

/// Open Droidsmith's backend-resolved crash-log directory. The command accepts
/// no path or grant from the renderer, so an error surface cannot be repurposed
/// to open an arbitrary host location.
#[tauri::command]
#[specta::specta]
pub fn reveal_diagnostics_directory() -> Result<(), CommandError> {
    let directory = crate::diagnostics::fallback_log_dir();
    std::fs::create_dir_all(&directory).map_err(|error| CommandError {
        code: "diagnostics_directory_unavailable",
        message: format!("could not prepare the diagnostics directory: {error}"),
    })?;
    let (program, args) = open_directory_command(&directory);
    std::process::Command::new(&program)
        .args(&args)
        .spawn()
        .map_err(|error| CommandError {
            code: "reveal_failed",
            message: format!("could not open the diagnostics directory: {error}"),
        })?;
    Ok(())
}

fn completed_adb_output(
    output: operations::ProcessOutput,
    program_name: &str,
) -> Result<String, CommandError> {
    if output.success() {
        Ok(output.stdout)
    } else {
        Err(CommandError {
            code: "adb_exit",
            message: format!(
                "{program_name} exited with code {}: {}",
                output.code.unwrap_or(-1),
                if output.stderr.trim().is_empty() {
                    output.stdout.trim()
                } else {
                    output.stderr.trim()
                }
            ),
        })
    }
}

fn journal_dir(app: &tauri::AppHandle) -> Result<PathBuf, CommandError> {
    let base = app.path().app_data_dir().map_err(|e| CommandError {
        code: "no_app_data_dir",
        message: e.to_string(),
    })?;
    Ok(base.join("journal"))
}

fn validate_serial_arg(serial: &str) -> Result<(), CommandError> {
    if valid_serial(serial) {
        Ok(())
    } else {
        Err(CommandError {
            code: "invalid_serial",
            message: format!("invalid device serial {serial:?}"),
        })
    }
}

fn execute_journaled(
    journal: &mut Journal,
    transport: &dyn AdbTransport,
    mut plan: actions::PlannedAction,
    undoes: Option<u64>,
) -> Result<ApplyActionResult, CommandError> {
    actions::validate_plan(&plan)?;
    adb::validate_device_target(transport, &plan.request.target)?;
    if plan.before_state.is_empty() {
        plan.before_state = actions::capture_state(transport, &plan.request);
    }
    let started_at = iso_now();
    let entry = journal
        .execute(plan, undoes, &started_at, |plan| {
            actions::apply(transport, plan, &iso_now())
        })
        .map_err(|error| match error {
            journal::ExecuteError::Journal(error) => CommandError::from(error),
            journal::ExecuteError::Operation(error) => CommandError {
                code: "package_action_failed",
                message: actions::package_action_failure_message(&error.to_string()),
            },
        })?;
    Ok(ApplyActionResult {
        stdout: entry.applied.display_stdout.clone(),
        entry,
    })
}

fn execute_remote_file_journaled(
    journal: &mut Journal,
    transport: &adb::ShellTransport,
    mut plan: actions::PlannedAction,
) -> Result<ApplyActionResult, CommandError> {
    actions::validate_plan(&plan)?;
    adb::validate_device_target(transport, &plan.request.target)?;
    plan.before_state = actions::capture_state(transport, &plan.request);
    let started_at = iso_now();
    let entry = journal
        .execute(
            plan,
            None,
            &started_at,
            |plan| -> Result<_, adb::TransportError> {
                let applied = actions::apply(transport, plan, &iso_now())?;
                remote_files::verify_transition(
                    transport,
                    &applied.plan.request.target,
                    &applied.plan.args,
                )?;
                Ok(applied)
            },
        )
        .map_err(map_remote_file_execute_error)?;
    Ok(ApplyActionResult {
        stdout: entry.applied.display_stdout.clone(),
        entry,
    })
}

fn map_remote_file_execute_error<E: std::fmt::Display>(
    error: journal::ExecuteError<E>,
) -> CommandError {
    match error {
        journal::ExecuteError::Journal(error) => CommandError::from(error),
        journal::ExecuteError::Operation(error) => CommandError {
            code: "remote_file_operation_failed",
            message: error.to_string(),
        },
    }
}

fn current_android_user(
    transport: &adb::ShellTransport,
    target: &adb::DeviceTarget,
) -> Result<u32, CommandError> {
    adb::list_users(transport, target)?
        .into_iter()
        .find(|user| user.current)
        .map(|user| user.id)
        .ok_or(CommandError {
            code: "current_user_missing",
            message: "could not bind the remote file operation to the current Android user"
                .to_string(),
        })
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ApplyActionResult {
    pub entry: JournalEntry,
    /// Raw output is returned only to the initiating view and is excluded from
    /// the persisted journal, which carries the redacted/bounded copy.
    pub stdout: String,
}

const MAX_ACTION_BATCH_ITEMS: usize = 100;

#[derive(specta::Type, Debug, Clone, Serialize, Deserialize)]
pub struct BatchActionPlan {
    pub plans: Vec<actions::PlannedAction>,
    pub description: String,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct BatchActionItemResult {
    pub package: String,
    pub entry: Option<JournalEntry>,
    pub stdout: String,
    pub error: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct BatchActionResult {
    pub batch_id: String,
    pub items: Vec<BatchActionItemResult>,
}

fn validated_transport_with_device(
    target: &adb::DeviceTarget,
) -> Result<(adb::ShellTransport, adb::Device), CommandError> {
    validate_serial_arg(&target.serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    let device = adb::validate_device_target(&transport, target)?;
    Ok((transport, device))
}

fn validated_transport(target: &adb::DeviceTarget) -> Result<adb::ShellTransport, CommandError> {
    validated_transport_with_device(target).map(|(transport, _)| transport)
}

fn accepted_transport_override(
    kind: adb::DeviceTransportKind,
    acknowledged: bool,
) -> Result<Option<adb::DeviceTransportKind>, ()> {
    if kind.requires_override() {
        acknowledged.then_some(Some(kind)).ok_or(())
    } else {
        Ok(None)
    }
}

/// Revalidate transport provenance at the privileged boundary. Renderer
/// acknowledgement is authorization metadata, never evidence that a TCP
/// endpoint is authenticated.
fn privileged_transport(
    target: &adb::DeviceTarget,
) -> Result<(adb::ShellTransport, Option<adb::DeviceTransportKind>), CommandError> {
    let (transport, device) = validated_transport_with_device(target)?;
    let override_kind = accepted_transport_override(
        device.transport_kind,
        target.untrusted_transport_override,
    )
    .map_err(|()| CommandError {
        code: "untrusted_transport_override_required",
        message: format!(
            "{} is connected over an unauthenticated {} transport; explicitly acknowledge the warning before running this operation",
            target.serial,
            device.transport_kind.label()
        ),
    })?;
    Ok((transport, override_kind))
}

fn validate_package_arg(package: &str) -> Result<(), CommandError> {
    if valid_package_name(package) {
        Ok(())
    } else {
        Err(CommandError {
            code: "invalid_package",
            message: format!("invalid package name {package:?}"),
        })
    }
}

/// Validate a device-side (remote) path before it reaches `adb pull`,
/// `adb push`, or `pm`. Argv-scoped calls have no shell-metachar risk,
/// but a leading `-` would be parsed by adb as an option flag, so reject
/// it — and require an absolute device path so callers can't smuggle a
/// flag or relative token across the IPC boundary.
fn validate_remote_path(remote_path: &str) -> Result<String, CommandError> {
    remote_files::validate_path(remote_path).map_err(|error| CommandError {
        code: "invalid_remote_path",
        message: error.to_string(),
    })
}

fn validate_fastboot_key(key: &str) -> Result<(), CommandError> {
    if key.is_empty() || key.len() > 128 {
        return Err(CommandError {
            code: "invalid_key",
            message: "fastboot variable key is empty or too long".to_string(),
        });
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(CommandError {
            code: "invalid_key",
            message: format!("fastboot variable key contains invalid characters: {key:?}"),
        });
    }
    Ok(())
}

/// Apply a previously-planned action and record it in the per-device
/// journal. Returns the freshly-written journal entry.
#[tauri::command]
#[specta::specta]
pub fn apply_action(
    app: tauri::AppHandle,
    mut plan: actions::PlannedAction,
) -> Result<ApplyActionResult, CommandError> {
    if plan.request.context.batch_id.is_some() {
        return Err(CommandError {
            code: "batch_command_required",
            message: "backend-issued batch plans must use the batch apply command".to_string(),
        });
    }
    if plan.request.kind == actions::ActionKind::RestoreExistingForUser {
        return Err(CommandError {
            code: "journal_undo_required",
            message: "install-existing recovery can only run from a verified journal undo"
                .to_string(),
        });
    }
    let (transport, transport_override) = privileged_transport(&plan.request.target)?;
    plan.request.context.transport_override = transport_override;

    let serial = plan.request.serial.clone();
    // Serialize intent → device mutation → terminal outcome per device. The
    // durable intent is written and synced before `actions::apply` runs.
    let dir = journal_dir(&app)?;
    let result = journal::with_journal(&dir, &serial, |journal| {
        execute_journaled(journal, &transport, plan, None)
    })?;
    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub fn apply_action_batch(
    app: tauri::AppHandle,
    mut batch: BatchActionPlan,
) -> Result<BatchActionResult, CommandError> {
    validate_action_batch_plan(&batch)?;
    let first = batch.plans.first().expect("validated non-empty batch");
    let target = first.request.target.clone();
    let serial = first.request.serial.clone();
    let (transport, transport_override) = privileged_transport(&target)?;
    let batch_id = next_batch_id();
    for plan in &mut batch.plans {
        plan.request.context.transport_override = transport_override;
        plan.request.context.batch_id = Some(batch_id.clone());
    }

    let dir = journal_dir(&app)?;
    let items = journal::with_journal(&dir, &serial, |journal| {
        execute_batch_plans(journal, &transport, batch.plans, None)
    })?;
    Ok(BatchActionResult { batch_id, items })
}

fn validate_action_batch_plan(batch: &BatchActionPlan) -> Result<(), CommandError> {
    if !(2..=MAX_ACTION_BATCH_ITEMS).contains(&batch.plans.len()) {
        return Err(CommandError {
            code: "invalid_action_batch",
            message: format!(
                "a package batch must contain between 2 and {MAX_ACTION_BATCH_ITEMS} items"
            ),
        });
    }
    let first = batch.plans.first().expect("length checked");
    let target = &first.request.target;
    let serial = &first.request.serial;
    let user_id = first.request.user_id;
    let kind = first.request.kind;
    if !matches!(
        kind,
        actions::ActionKind::Disable
            | actions::ActionKind::Enable
            | actions::ActionKind::Archive
            | actions::ActionKind::RequestUnarchive
    ) || batch.description != batch_action_description(kind, batch.plans.len(), user_id)
    {
        return Err(CommandError {
            code: "invalid_action_batch",
            message: "batch metadata does not match its canonical action plans".to_string(),
        });
    }
    let mut packages = HashSet::with_capacity(batch.plans.len());
    for plan in &batch.plans {
        actions::validate_plan(plan)?;
        if &plan.request.serial != serial
            || &plan.request.target != target
            || plan.request.user_id != user_id
            || plan.request.kind != kind
            || plan.request.pack_context.is_some()
            || plan.request.context.confirmation_source != actions::ConfirmationSource::AppsPreview
            || plan.request.context.batch_id.is_some()
            || !plan.before_state.is_empty()
            || !packages.insert(plan.request.package.clone())
        {
            return Err(CommandError {
                code: "mixed_action_batch",
                message: "batch plans must be unique, renderer-reviewed, and bound to one target/user/action"
                    .to_string(),
            });
        }
    }
    Ok(())
}

fn execute_batch_plans(
    journal: &mut Journal,
    transport: &dyn AdbTransport,
    plans: Vec<actions::PlannedAction>,
    undo_ids: Option<Vec<u64>>,
) -> Result<Vec<BatchActionItemResult>, CommandError> {
    if let Some(ids) = undo_ids.as_ref() {
        if ids.len() != plans.len() {
            return Err(CommandError {
                code: "invalid_action_batch",
                message: "batch undo ids do not match their inverse plans".to_string(),
            });
        }
    }
    let mut items = Vec::with_capacity(plans.len());
    for (index, mut plan) in plans.into_iter().enumerate() {
        let package = plan.request.package.clone();
        let before_state = actions::capture_state(transport, &plan.request);
        if !actions::reversible_batch_before_state(plan.request.kind, &before_state) {
            items.push(BatchActionItemResult {
                package,
                entry: None,
                stdout: String::new(),
                error: Some(format!(
                    "verified package state {before_state} is not a reversible starting state for {:?}",
                    plan.request.kind
                )),
            });
            continue;
        }
        plan.before_state = before_state;
        let incident_id = plan.incident_id.clone();
        let undoes = undo_ids.as_ref().map(|ids| ids[index]);
        match execute_journaled(journal, transport, plan, undoes) {
            Ok(result) => items.push(BatchActionItemResult {
                package,
                entry: Some(result.entry),
                stdout: result.stdout,
                error: None,
            }),
            Err(error) if error.code == "package_action_failed" => {
                let entry = journal
                    .entries()
                    .iter()
                    .rev()
                    .find(|entry| entry.applied.plan.incident_id == incident_id)
                    .cloned();
                items.push(BatchActionItemResult {
                    package,
                    entry,
                    stdout: String::new(),
                    error: Some(error.message),
                });
            }
            Err(error) => return Err(error),
        }
    }
    Ok(items)
}

fn next_batch_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("batch-{nanos:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Export a redacted package snapshot before a reviewed destructive batch.
/// The renderer supplies requested intents, while the backend captures all
/// device/package state and writes through a one-shot native path grant.
#[tauri::command]
#[specta::specta]
pub fn export_recovery_baseline(
    target: adb::DeviceTarget,
    #[allow(non_snake_case)] userId: u32,
    actions: Vec<BaselineActionInput>,
    pack: Option<BaselinePack>,
    path_grant: String,
    grants: tauri::State<'_, PathGrantStore>,
) -> Result<HostArtifact, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let path = grants.consume(&path_grant, HostPathPurpose::RecoveryBaselineSave)?;
    let users = adb::list_users(&transport, &target)?;
    if !users.iter().any(|user| user.id == userId) {
        return Err(CommandError {
            code: "recovery_baseline_user_missing",
            message: format!("Android user {userId} is not available"),
        });
    }
    let packages = adb::list_packages(&transport, &target, adb::PackageFilter::All, userId)?;
    let baseline = recovery_baseline::build(&target, userId, pack, &packages, actions, iso_now())?;
    Ok(recovery_baseline::save(&path, &baseline)?)
}

/// Load and compare a baseline without mutating the device. Returned plans are
/// canonical but remain inert until the renderer shows the diff and explicitly
/// submits individual plans through `apply_action`.
#[tauri::command]
#[specta::specta]
pub fn inspect_recovery_baseline(
    target: adb::DeviceTarget,
    path_grant: String,
    grants: tauri::State<'_, PathGrantStore>,
) -> Result<RecoveryBaselineDiff, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let path = grants.consume(&path_grant, HostPathPurpose::RecoveryBaselineOpen)?;
    let baseline = recovery_baseline::load(&path)?;
    let users = adb::list_users(&transport, &target)?;
    let packages = if users.iter().any(|user| user.id == baseline.android_user) {
        adb::list_packages(
            &transport,
            &target,
            adb::PackageFilter::All,
            baseline.android_user,
        )?
    } else {
        Vec::new()
    };
    Ok(recovery_baseline::inspect(
        baseline, &target, &users, &packages,
    )?)
}

#[tauri::command]
#[specta::specta]
pub fn journal_list(
    app: tauri::AppHandle,
    serial: String,
) -> Result<Vec<JournalEntry>, CommandError> {
    validate_serial_arg(&serial)?;
    let dir = journal_dir(&app)?;
    journal::with_journal(&dir, &serial, |journal| {
        Ok::<_, CommandError>(journal.entries().to_vec())
    })
}

/// Undo entry `entry_id` in `serial`'s journal. Returns the new
/// undo-entry. Fails if the original action is irreversible
/// (unverified uninstall, clear-data, force-stop).
#[tauri::command]
#[specta::specta]
pub fn journal_undo(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    entry_id: u64,
) -> Result<JournalEntry, CommandError> {
    let serial = target.serial.clone();
    validate_serial_arg(&serial)?;
    let (transport, transport_override) = privileged_transport(&target)?;

    // Hold the per-device lock across the reversibility check, the inverse
    // ADB call, and the undo record so two undos of the same entry cannot
    // both pass the check and double-apply.
    let dir = journal_dir(&app)?;
    let entry = journal::with_journal(&dir, &serial, |journal| {
        let mut undo_request = journal::undo_request_for(journal, entry_id).ok_or(CommandError {
            code: "not_reversible",
            message: format!(
                "journal entry {entry_id} either doesn't exist, is already undone, or its action kind cannot be reversed"
            ),
        })?;

        undo_request.target = target.clone();
        undo_request.context.transport_override = transport_override;

        let plan = actions::plan(undo_request);
        execute_journaled(journal, &transport, plan, Some(entry_id)).map(|result| result.entry)
    })?;
    Ok(entry)
}

/// Undo every still-active successful item from one backend-issued batch.
/// Reversibility is proven for the complete remaining set before the first
/// inverse runs; device-level failures are then reported per package without
/// hiding successful inverses.
#[tauri::command]
#[specta::specta]
pub fn journal_undo_batch(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    batch_id: String,
) -> Result<BatchActionResult, CommandError> {
    if !actions::valid_batch_id(&batch_id) {
        return Err(CommandError {
            code: "invalid_batch_id",
            message: "batch id is malformed".to_string(),
        });
    }
    let serial = target.serial.clone();
    validate_serial_arg(&serial)?;
    let (transport, transport_override) = privileged_transport(&target)?;
    let dir = journal_dir(&app)?;
    let items = journal::with_journal(&dir, &serial, |journal| {
        let originals = journal
            .entries()
            .iter()
            .filter(|entry| {
                entry.undoes.is_none()
                    && entry.outcome == journal::JournalOutcome::Succeeded
                    && entry.applied.plan.request.context.batch_id.as_deref()
                        == Some(batch_id.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        if originals.is_empty() {
            return Err(CommandError {
                code: "batch_not_found",
                message: format!("no successful journal entries belong to {batch_id}"),
            });
        }

        let remaining = originals
            .into_iter()
            .filter(|entry| entry.undone_by.is_none())
            .collect::<Vec<_>>();
        if remaining.is_empty() {
            return Err(CommandError {
                code: "batch_already_undone",
                message: format!("every successful item in {batch_id} is already undone"),
            });
        }

        let mut plans = Vec::with_capacity(remaining.len());
        let mut ids = Vec::with_capacity(remaining.len());
        for entry in remaining {
            let mut request = journal::undo_request_for(journal, entry.id).ok_or(CommandError {
                code: "batch_not_reversible",
                message: format!(
                    "journal entry {} in {batch_id} cannot be safely reversed as part of the batch",
                    entry.id
                ),
            })?;
            request.serial = serial.clone();
            request.target = target.clone();
            request.context.transport_override = transport_override;
            request.context.batch_id = Some(batch_id.clone());
            plans.push(actions::plan(request));
            ids.push(entry.id);
        }
        execute_batch_plans(journal, &transport, plans, Some(ids))
    })?;
    Ok(BatchActionResult { batch_id, items })
}

#[tauri::command]
#[specta::specta]
pub fn get_device_info(target: adb::DeviceTarget) -> Result<adb::DeviceInfo, CommandError> {
    let transport = validated_transport(&target)?;
    Ok(adb::get_device_info(&transport, &target)?)
}

/// R-082: read the curated system-settings allow-list (`settings get`). Read
/// only; safe over any authorized transport.
#[tauri::command]
#[specta::specta]
pub fn list_device_settings(
    target: adb::DeviceTarget,
) -> Result<Vec<adb::DeviceSetting>, CommandError> {
    let transport = validated_transport(&target)?;
    Ok(adb::read_device_settings(&transport, &target)?)
}

/// R-082: write one allow-listed setting (`settings put`). The `setting_id` and
/// `value` are validated against the catalog before anything is shelled out, so
/// arbitrary keys or out-of-range values are rejected. Runs over the privileged
/// transport boundary because it mutates device state; the previous value is
/// returned so the renderer can offer a one-click revert.
#[tauri::command]
#[specta::specta]
pub fn put_device_setting(
    target: adb::DeviceTarget,
    setting_id: String,
    value: String,
) -> Result<adb::DeviceSettingChange, CommandError> {
    let spec = adb::validate_write(&setting_id, &value).map_err(|message| CommandError {
        code: "invalid_setting",
        message,
    })?;
    let normalized = value.trim().to_string();
    let (transport, _override) = privileged_transport(&target)?;

    let previous = adb::read_device_settings(&transport, &target)
        .ok()
        .and_then(|settings| {
            settings
                .into_iter()
                .find(|setting| setting.id == setting_id)
                .and_then(|setting| setting.value)
        });

    let argv = adb::put_argv(spec, &normalized);
    transport.shell_target(&target, &argv)?;

    Ok(adb::DeviceSettingChange {
        id: setting_id,
        namespace: adb::spec_namespace(spec),
        key: adb::spec_key(spec).to_string(),
        previous_value: previous,
        new_value: normalized.clone(),
        command: adb::command_preview(spec, &normalized),
    })
}

/// Run a one-shot read-only shell command outside the webview thread and
/// stream progress/output through a Tauri channel. Mutations continue to use
/// the reviewed audited executor.
#[tauri::command]
#[specta::specta]
pub async fn shell_run(
    target: adb::DeviceTarget,
    argv: Vec<String>,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<String, CommandError> {
    if !actions::valid_shell_argv(&argv) {
        return Err(CommandError {
            code: "invalid_shell_argv",
            message: "shell argv is empty, oversized, or contains control characters".to_string(),
        });
    }
    if classify_shell(&argv) != ShellClassification::ReadOnly {
        return Err(CommandError {
            code: "shell_mutation_requires_review",
            message: "mutating shell commands must be reviewed and executed through the audited operation planner".to_string(),
        });
    }
    let (transport, _) = privileged_transport(&target)?;
    let adb_path = transport.adb_path.clone();
    let mut args = target.adb_selector();
    args.push("shell".to_string());
    args.extend(argv);
    let sink = operations::channel_sink(on_event);
    spawn_blocking_operation(move || {
        let output = operations::run_process(
            &adb_path,
            &args,
            std::time::Duration::from_secs(300),
            &operation_id,
            "Running ADB shell command",
            sink,
        )?;
        completed_adb_output(output, "adb shell")
    })
    .await
}

/// Cancel a registered background operation. The runner observes this flag,
/// kills and reaps its child, and then emits a terminal cancellation event.
#[tauri::command]
#[specta::specta]
pub fn cancel_operation(operation_id: String) -> bool {
    operations::cancel(&operation_id)
}

/// Start one incremental Logcat process. Unexpected exits are retried by the
/// backend and surfaced as reconnect markers; the call completes only after
/// cancellation or an unrecoverable spawn/wait failure.
#[tauri::command]
#[specta::specta]
pub async fn stream_logcat(
    target: adb::DeviceTarget,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<(), CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let adb_path = transport.adb_path.clone();
    let mut args = target.adb_selector();
    args.extend([
        "shell".to_string(),
        "logcat".to_string(),
        "-v".to_string(),
        // threadtime carries the timestamp and pid the query presets filter on;
        // brief format omitted both.
        "threadtime".to_string(),
    ]);
    let sink = operations::channel_sink(on_event);
    spawn_blocking_operation(move || {
        operations::stream_logcat(&adb_path, &args, &operation_id, sink)?;
        Ok(())
    })
    .await
}

/// Persist the renderer's bounded Logcat buffer through a one-shot path grant.
/// The size limit keeps the IPC and host write bounded.
#[tauri::command]
#[specta::specta]
pub async fn save_logcat_export(
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    contents: String,
) -> Result<String, CommandError> {
    const MAX_LOGCAT_EXPORT_BYTES: usize = 4 * 1024 * 1024;
    if contents.len() > MAX_LOGCAT_EXPORT_BYTES {
        return Err(CommandError {
            code: "logcat_export_too_large",
            message: format!("Logcat export exceeds the {MAX_LOGCAT_EXPORT_BYTES}-byte limit"),
        });
    }
    let path = grants.consume(&path_grant, HostPathPurpose::LogcatSave)?;
    if !path.parent().is_some_and(std::path::Path::is_dir) {
        return Err(CommandError {
            code: "invalid_path",
            message: "Logcat export parent directory does not exist".to_string(),
        });
    }
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(CommandError {
            code: "invalid_path",
            message: "Logcat export target must not be a symbolic link".to_string(),
        });
    }
    let display_path = path.display().to_string();
    spawn_blocking_operation(move || {
        std::fs::write(&path, contents.as_bytes())?;
        Ok(display_path)
    })
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellClassification {
    ReadOnly,
    Mutation,
    Dangerous,
}

/// Characters that let the on-device shell chain a second command, substitute
/// a subshell, or redirect output. `adb shell` joins argv with spaces and runs
/// the result through the device's `sh -c`, so any of these inside *any* token
/// (e.g. a `getprop; pm uninstall …` typed into the console and split on
/// whitespace into `getprop;`) would execute a hidden mutation while the head
/// token still looked read-only. Such argv can never be classified read-only.
fn argv_has_shell_control_metacharacter(argv: &[String]) -> bool {
    argv.iter().any(|token| {
        token.chars().any(|character| {
            matches!(
                character,
                ';' | '|' | '&' | '$' | '`' | '(' | ')' | '<' | '>'
            )
        })
    })
}

fn classify_shell(argv: &[String]) -> ShellClassification {
    // A token carrying a shell control metacharacter can smuggle a mutation past
    // the head-token classifier, so refuse to treat it as anything but dangerous
    // — that routes it through the reviewed/journaled executor (or is rejected
    // outright by `shell_run`, which only runs read-only commands).
    if argv_has_shell_control_metacharacter(argv) {
        return ShellClassification::Dangerous;
    }
    let head = argv.first().map(String::as_str).unwrap_or_default();
    let subcommand = argv.get(1).map(String::as_str).unwrap_or_default();
    match head {
        "logcat" | "getprop" | "dumpsys" | "ps" | "ss" | "netstat" | "ls" | "df" | "stat"
        | "cat" | "id" | "uname" => ShellClassification::ReadOnly,
        "wm" if argv.len() <= 2 && matches!(subcommand, "size" | "density") => {
            ShellClassification::ReadOnly
        }
        "settings" if matches!(subcommand, "get" | "list") => ShellClassification::ReadOnly,
        "pm" if matches!(subcommand, "list" | "path" | "dump") => ShellClassification::ReadOnly,
        "cmd"
            if argv.get(1).map(String::as_str) == Some("package")
                && matches!(argv.get(2).map(String::as_str), Some("list") | Some("path")) =>
        {
            ShellClassification::ReadOnly
        }
        "input" | "wm" | "settings" => ShellClassification::Mutation,
        _ => ShellClassification::Dangerous,
    }
}

#[derive(specta::Type, Debug, Clone, Deserialize)]
pub struct PlanShellActionRequest {
    pub target: adb::DeviceTarget,
    pub argv: Vec<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ShellActionPlan {
    pub mutating: bool,
    pub dangerous: bool,
    pub plan: Option<actions::PlannedAction>,
}

#[tauri::command]
#[specta::specta]
pub fn plan_shell_action(request: PlanShellActionRequest) -> Result<ShellActionPlan, CommandError> {
    if !actions::valid_shell_argv(&request.argv) {
        return Err(CommandError {
            code: "invalid_shell_argv",
            message: "shell argv is empty, oversized, or contains control characters".to_string(),
        });
    }
    let classification = classify_shell(&request.argv);
    if classification == ShellClassification::ReadOnly {
        return Ok(ShellActionPlan {
            mutating: false,
            dangerous: false,
            plan: None,
        });
    }
    let (transport, transport_override) = privileged_transport(&request.target)?;
    let users = adb::list_users(&transport, &request.target)?;
    let user_id = users
        .iter()
        .find(|user| user.current)
        .map(|user| user.id)
        .ok_or(CommandError {
            code: "current_user_missing",
            message: "could not bind the shell mutation to the current Android user".to_string(),
        })?;
    let plan = actions::plan(actions::ActionRequest {
        serial: request.target.serial.clone(),
        target: request.target,
        package: String::new(),
        kind: actions::ActionKind::Shell,
        user_id,
        pack_context: None,
        context: actions::ActionContext {
            confirmation_source: actions::ConfirmationSource::ConsoleReview,
            permission: None,
            shell_argv: request.argv,
            device_control_restore_argv: Vec::new(),
            device_control_expected_before: None,
            transport_override,
            restore_enabled_state: None,
            batch_id: None,
        },
    });
    Ok(ShellActionPlan {
        mutating: true,
        dangerous: classification == ShellClassification::Dangerous,
        plan: Some(plan),
    })
}

fn is_allowed_device_control(argv: &[String]) -> bool {
    matches!(
        argv,
        [input, keyevent, code]
            if input == "input" && keyevent == "keyevent" && code.parse::<u32>().is_ok()
    ) || matches!(
        argv,
        [wm, density, value]
            if wm == "wm"
                && density == "density"
                && (value == "reset" || value.parse::<u16>().is_ok_and(|value| (72..=1000).contains(&value)))
    ) || matches!(
        argv,
        [settings, put, secure, key, value]
            if settings == "settings"
                && put == "put"
                && secure == "secure"
                && key == "ui_night_mode"
                && matches!(value.as_str(), "1" | "2")
    )
}

#[tauri::command]
#[specta::specta]
pub fn apply_device_control(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    argv: Vec<String>,
) -> Result<ApplyActionResult, CommandError> {
    if !is_allowed_device_control(&argv) {
        return Err(CommandError {
            code: "device_control_not_allowed",
            message: "command is not an allowlisted Droidsmith device control".to_string(),
        });
    }
    let (transport, transport_override) = privileged_transport(&target)?;
    let users = adb::list_users(&transport, &target)?;
    let user_id = users
        .iter()
        .find(|user| user.current)
        .map(|user| user.id)
        .ok_or(CommandError {
            code: "current_user_missing",
            message: "could not bind the device control to the current Android user".to_string(),
        })?;
    let prepared = actions::prepare_device_control(&transport, &target, user_id, &argv)?;
    let serial = target.serial.clone();
    let mut plan = actions::plan(actions::ActionRequest {
        serial: serial.clone(),
        target,
        package: String::new(),
        kind: actions::ActionKind::Shell,
        user_id,
        pack_context: None,
        context: actions::ActionContext {
            confirmation_source: actions::ConfirmationSource::DeviceControl,
            permission: None,
            shell_argv: prepared.argv,
            device_control_restore_argv: prepared.restore_argv,
            device_control_expected_before: None,
            transport_override,
            restore_enabled_state: None,
            batch_id: None,
        },
    });
    plan.before_state = prepared.before_state;
    let dir = journal_dir(&app)?;
    journal::with_journal(&dir, &serial, |journal| {
        execute_journaled(journal, &transport, plan, None)
    })
}

/// List files in a remote directory on the device.
#[tauri::command]
#[specta::specta]
pub fn list_remote_files(
    target: adb::DeviceTarget,
    remote_path: String,
) -> Result<RemoteListing, CommandError> {
    let remote = validate_remote_path(&remote_path)?;
    let transport = validated_transport(&target)?;
    let stdout = transport.shell_target(&target, &["ls", "-la", &remote])?;
    let entries = parse_ls_output(&stdout);
    let free_space = transport
        .shell_target(&target, &["df", "-k", &remote])
        .ok()
        .and_then(|s| parse_df_free(&s));
    Ok(RemoteListing {
        path: remote,
        entries,
        free_space_kb: free_space,
    })
}

/// Validate a structured file mutation and return the exact argv that will be
/// journaled and executed after the renderer presents its confirmation review.
#[tauri::command]
#[specta::specta]
pub fn plan_remote_file_mutation(
    request: remote_files::RemoteFileMutationRequest,
) -> Result<remote_files::RemoteFileMutationPlan, CommandError> {
    Ok(remote_files::plan(&request)?)
}

/// Rebuild and execute a reviewed device-side file mutation. The renderer
/// cannot supply argv: it submits only structured paths and the backend
/// regenerates the canonical mkdir/mv/rm command before writing the intent.
#[tauri::command]
#[specta::specta]
pub fn apply_remote_file_mutation(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    request: remote_files::RemoteFileMutationRequest,
    confirmed: bool,
) -> Result<ApplyActionResult, CommandError> {
    let reviewed = remote_files::plan(&request)?;
    if !confirmed {
        return Err(CommandError {
            code: "confirmation_required",
            message: "remote file mutation requires an explicit confirmation review".to_string(),
        });
    }
    let (transport, transport_override) = privileged_transport(&target)?;
    let user_id = current_android_user(&transport, &target)?;
    let serial = target.serial.clone();
    let plan = remote_files::action_plan(target, user_id, transport_override, &reviewed);
    let dir = journal_dir(&app)?;
    journal::with_journal(&dir, &serial, |journal| {
        execute_remote_file_journaled(journal, &transport, plan)
    })
}

/// Push a local file to the device.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn push_file(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    remote_path: String,
    confirmed: bool,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<ApplyActionResult, CommandError> {
    if !confirmed {
        return Err(CommandError {
            code: "confirmation_required",
            message: "file push requires an explicit source/target confirmation".to_string(),
        });
    }
    let (transport, transport_override) = privileged_transport(&target)?;
    let validated_path = grants.consume(&path_grant, HostPathPurpose::PushOpen)?;
    let remote = validate_remote_path(&remote_path)?;
    let user_id = current_android_user(&transport, &target)?;
    let local_arg = validated_path.display().to_string();
    let timeout = std::time::Duration::from_secs(120);
    let mut args = target.adb_selector();
    args.extend(["push".to_string(), local_arg, remote.clone()]);
    let adb_path = transport.adb_path.clone();
    let sink = operations::channel_sink(on_event);
    let serial = target.serial.clone();
    let journal_path = journal_dir(&app)?;
    let mut plan = actions::plan(actions::ActionRequest {
        serial: serial.clone(),
        target: target.clone(),
        package: String::new(),
        kind: actions::ActionKind::Shell,
        user_id,
        pack_context: None,
        context: actions::ActionContext {
            confirmation_source: actions::ConfirmationSource::FileManagerReview,
            permission: None,
            shell_argv: vec!["droidsmith-file-push".to_string(), remote.clone()],
            device_control_restore_argv: Vec::new(),
            device_control_expected_before: None,
            transport_override,
            restore_enabled_state: None,
            batch_id: None,
        },
    });
    plan.description = format!("Push a native-selected local file to {remote:?}");
    plan.before_state = format!(
        "{remote}={}",
        remote_files::capture_path_state(&transport, &target, &remote)
    );
    spawn_blocking_operation(move || {
        journal::with_journal(&journal_path, &serial, |journal| {
            let started_at = iso_now();
            let entry = journal
                .execute(plan, None, &started_at, |plan| {
                    adb::validate_device_target(&transport, &target)?;
                    let output = operations::run_process(
                        &adb_path,
                        &args,
                        timeout,
                        &operation_id,
                        "Pushing file to device",
                        sink,
                    )?;
                    let stdout = completed_adb_output(output, "adb push")?;
                    let after_state = format!(
                        "{remote}={}",
                        remote_files::capture_path_state(&transport, &target, &remote)
                    );
                    if !after_state.ends_with("=present") {
                        return Err(CommandError {
                            code: "remote_file_operation_failed",
                            message:
                                "adb push exited successfully but the target file was not observed"
                                    .to_string(),
                        });
                    }
                    Ok::<_, CommandError>(actions::AppliedAction {
                        stdout: actions::redact_journal_text(&plan.request, &stdout),
                        display_stdout: stdout,
                        before_state: plan.before_state.clone(),
                        after_state,
                        plan,
                        applied_at: iso_now(),
                    })
                })
                .map_err(map_remote_file_execute_error)?;
            Ok(ApplyActionResult {
                stdout: entry.applied.display_stdout.clone(),
                entry,
            })
        })
    })
    .await
}

/// Pull a remote file from the device.
#[tauri::command]
#[specta::specta]
pub async fn pull_file(
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    remote_path: String,
    path_grant: String,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<HostArtifact, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let output_target = grants.consume(&path_grant, HostPathPurpose::PullSave)?;
    let remote = validate_remote_path(&remote_path)?;
    let timeout = std::time::Duration::from_secs(120);
    let selector = target.adb_selector();
    let adb_path = transport.adb_path.clone();
    let sink = operations::channel_sink(on_event);
    spawn_blocking_operation(move || {
        let staged = StagedArtifact::new(&output_target)?;
        let mut args = selector;
        args.extend([
            "pull".to_string(),
            remote,
            staged.path().display().to_string(),
        ]);
        let output = operations::run_process(
            &adb_path,
            &args,
            timeout,
            &operation_id,
            "Pulling file from device",
            sink,
        )?;
        completed_adb_output(output, "adb pull")?;
        Ok(staged.commit(ArtifactKind::AnyFile)?)
    })
    .await
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct RemoteListing {
    pub path: String,
    pub entries: Vec<RemoteFileEntry>,
    pub free_space_kb: Option<u64>,
}

fn parse_df_free(stdout: &str) -> Option<u64> {
    for line in stdout.lines().skip(1) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() >= 4 {
            return tokens[3].parse().ok();
        }
    }
    None
}

/// Typed result of one captured subprocess execution. Unlike the old
/// stdout-only helper, this keeps both streams plus the exit disposition
/// so callers such as `fastboot getvar` (which prints successful values
/// to stderr) can read the right stream without a blind retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
    /// Process exit code, or `None` when killed by signal.
    pub code: Option<i32>,
    /// True when the child was killed because it exceeded the timeout.
    pub timed_out: bool,
}

impl ProcessOutput {
    fn success(&self) -> bool {
        !self.timed_out && self.code == Some(0)
    }
}

/// Run a subprocess once, capturing stdout, stderr, exit code, and
/// timeout state in a single execution.
fn run_captured(
    program: &std::path::Path,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<ProcessOutput, CommandError> {
    use std::process::Command;

    let mut command = Command::new(program);
    command.args(args);
    let output = crate::process_capture::run(
        &mut command,
        timeout,
        crate::process_capture::CaptureLimits::default(),
    )
    .map_err(|error| CommandError {
        code: match error {
            crate::process_capture::CaptureError::Spawn(_) => "spawn_failed",
            _ => "subprocess_capture_failed",
        },
        message: format!("failed to run {}: {error}", program.display()),
    })?;
    let (code, timed_out) = match output.termination {
        crate::process_capture::CaptureTermination::Exited(status) => (status.code(), false),
        crate::process_capture::CaptureTermination::TimedOut => (None, true),
        crate::process_capture::CaptureTermination::OutputLimitExceeded {
            stream,
            limit_bytes,
        } => {
            return Err(CommandError {
                code: "subprocess_output_limit",
                message: format!(
                    "{} {stream} exceeded the {limit_bytes}-byte capture limit",
                    program.display()
                ),
            });
        }
    };
    Ok(ProcessOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code,
        timed_out,
    })
}

/// Backwards-compatible helper for callers that only care about stdout
/// on success (push/pull/df/ls/fastboot devices). Failures and timeouts
/// still surface stderr in the error message.
fn run_adb_simple(
    adb_path: &std::path::Path,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<String, CommandError> {
    let out = run_captured(adb_path, args, timeout)?;
    if out.timed_out {
        return Err(CommandError {
            code: "adb_timeout",
            message: format!("adb timed out after {timeout:?}"),
        });
    }
    if out.success() {
        Ok(out.stdout)
    } else {
        Err(CommandError {
            code: "adb_exit",
            message: format!(
                "adb exited with code {}: {}",
                out.code.unwrap_or(-1),
                out.stderr
            ),
        })
    }
}

/// Locate the fastboot binary on the system.
#[tauri::command]
#[specta::specta]
pub fn locate_fastboot() -> Option<String> {
    which::which("fastboot")
        .ok()
        .map(|p| p.display().to_string())
}

/// List devices visible to fastboot.
#[tauri::command]
#[specta::specta]
pub fn list_fastboot_devices() -> Result<Vec<FastbootDevice>, CommandError> {
    let fastboot_path = which::which("fastboot").map_err(|_| CommandError {
        code: "fastboot_not_found",
        message: "fastboot binary not found on PATH".to_string(),
    })?;

    let timeout = std::time::Duration::from_secs(10);
    let stdout = run_adb_simple(&fastboot_path, &["devices", "-l"], timeout)?;
    Ok(parse_fastboot_devices(&stdout))
}

/// Query a fastboot variable.
#[tauri::command]
#[specta::specta]
pub fn fastboot_getvar(serial: String, key: String) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    validate_fastboot_key(&key)?;
    let fastboot_path = which::which("fastboot").map_err(|_| CommandError {
        code: "fastboot_not_found",
        message: "fastboot binary not found on PATH".to_string(),
    })?;

    let timeout = std::time::Duration::from_secs(10);
    let out = run_captured(&fastboot_path, &["-s", &serial, "getvar", &key], timeout)?;
    parse_fastboot_getvar(&key, &out)
}

/// Extract a `fastboot getvar <key>` value from a captured execution.
///
/// fastboot writes the value to **stderr** in the shape `key: value`
/// (stdout stays empty on success), so a single execution suffices — no
/// blind retry. Failures preserve both streams; timeouts are explicit.
fn parse_fastboot_getvar(key: &str, out: &ProcessOutput) -> Result<String, CommandError> {
    if out.timed_out {
        return Err(CommandError {
            code: "fastboot_timeout",
            message: format!("fastboot getvar {key:?} timed out"),
        });
    }

    // The value line can arrive on either stream depending on the
    // fastboot build; check stderr first (the common case) then stdout.
    if let Some(value) = getvar_value(key, &out.stderr).or_else(|| getvar_value(key, &out.stdout)) {
        return Ok(value);
    }

    if !out.success() {
        return Err(CommandError {
            code: "fastboot_exit",
            message: format!(
                "fastboot getvar {key:?} failed (code {}): {}",
                out.code.unwrap_or(-1),
                // Prefer stderr, fall back to stdout, so the operator sees
                // whatever diagnostic the tool emitted.
                first_nonempty(&out.stderr, &out.stdout)
            ),
        });
    }

    Err(CommandError {
        code: "fastboot_no_value",
        message: format!("fastboot getvar {key:?} returned no value"),
    })
}

/// Scan `text` for a `key: value` line and return the trimmed value.
/// Ignores fastboot's trailing `finished. total time: ...` line and the
/// `getvar:<key> FAILED` error shape.
fn getvar_value(key: &str, text: &str) -> Option<String> {
    let prefix = format!("{key}:");
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let value = rest.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn first_nonempty<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.trim().is_empty() {
        b
    } else {
        a
    }
}

/// Get network connections from the device using `ss -tunp`.
#[tauri::command]
#[specta::specta]
pub fn list_network_connections(
    target: adb::DeviceTarget,
) -> Result<Vec<NetworkConnection>, CommandError> {
    let transport = validated_transport(&target)?;
    let stdout = transport
        .shell_target(&target, &["ss", "-tunp"])
        .or_else(|_| transport.shell_target(&target, &["netstat", "-tunp"]))?;
    Ok(parse_ss_output(&stdout))
}

fn validate_backup_target(local_path: &str) -> Result<PathBuf, CommandError> {
    let trimmed = local_path.trim();
    if trimmed.is_empty() {
        return Err(CommandError {
            code: "invalid_backup_path",
            message: "backup destination cannot be empty".to_string(),
        });
    }

    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(CommandError {
            code: "invalid_backup_path",
            message: format!("backup destination must be an absolute path: {trimmed}"),
        });
    }
    if path.is_dir() {
        return Err(CommandError {
            code: "invalid_backup_path",
            message: format!("backup destination is a directory: {}", path.display()),
        });
    }
    let Some(parent) = path.parent() else {
        return Err(CommandError {
            code: "invalid_backup_path",
            message: format!(
                "backup destination has no parent directory: {}",
                path.display()
            ),
        });
    };
    if !parent.is_dir() {
        return Err(CommandError {
            code: "invalid_backup_path",
            message: format!(
                "backup destination parent does not exist: {}",
                parent.display()
            ),
        });
    }

    Ok(path)
}

/// Inspect the package's default APK export and deprecated data-backup
/// capabilities. This is read-only and scoped to one Android user.
#[tauri::command]
#[specta::specta]
pub fn preflight_package_backup(
    target: adb::DeviceTarget,
    package: String,
    #[allow(non_snake_case)] userId: u32,
) -> Result<backup::PackageBackupPreflight, CommandError> {
    let transport = validated_transport(&target)?;
    validate_package_arg(&package)?;
    Ok(backup::preflight(&transport, &target, &package, userId)?)
}

/// Export every base/split APK plus a versioned evidence manifest to one
/// atomically-installed ZIP. This is the dependable default package backup.
#[tauri::command]
#[specta::specta]
pub async fn export_package_apks(
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    package: String,
    #[allow(non_snake_case)] userId: u32,
    path_grant: String,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<backup::PackageExportResult, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    validate_package_arg(&package)?;
    let granted_path = grants.consume(&path_grant, HostPathPurpose::PackageExportSave)?;
    let output_target = validate_backup_target(&granted_path.display().to_string())?;
    let preflight = backup::preflight(&transport, &target, &package, userId)?;
    let adb_path = transport.adb_path.clone();
    let sink = operations::channel_sink(on_event);
    spawn_blocking_operation(move || {
        Ok(backup::export_apks(
            &adb_path,
            &target,
            &output_target,
            preflight,
            &operation_id,
            sink,
        )?)
    })
    .await
}

/// Capture an opaque Android bugreport only after a dedicated privacy
/// acknowledgement. The immutable target and one-shot native path grant are
/// revalidated before the long-running ADB process begins.
#[tauri::command]
#[specta::specta]
pub async fn capture_bugreport(
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    privacy_confirmed: bool,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<bugreport::BugreportCaptureResult, CommandError> {
    if !privacy_confirmed {
        return Err(CommandError {
            code: "bugreport_privacy_confirmation_required",
            message: "review and acknowledge the bugreport privacy warning before capture"
                .to_string(),
        });
    }
    let (transport, _) = privileged_transport(&target)?;
    let destination = grants.consume(&path_grant, HostPathPurpose::BugreportSave)?;
    let platform_tools_version = adb::locate_adb().version;
    let adb_path = transport.adb_path;
    let sink = operations::channel_sink(on_event);
    spawn_blocking_operation(move || {
        Ok(bugreport::capture(
            &adb_path,
            &target,
            &destination,
            platform_tools_version,
            &operation_id,
            sink,
        )?)
    })
    .await
}

/// Advanced-only deprecated `adb backup` path. The produced `.ab` is forced
/// uncompressed, structurally inspected, and packaged with a manifest. The
/// result reports detected entries, never verified restorability.
#[tauri::command]
#[specta::specta]
pub async fn backup_package(
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    package: String,
    #[allow(non_snake_case)] userId: u32,
    path_grant: String,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<backup::PackageExportResult, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    validate_package_arg(&package)?;
    let granted_path = grants.consume(&path_grant, HostPathPurpose::BackupSave)?;
    let output_target = validate_backup_target(&granted_path.display().to_string())?;
    let preflight = backup::preflight(&transport, &target, &package, userId)?;
    let adb_path = transport.adb_path.clone();
    let sink = operations::channel_sink(on_event);
    spawn_blocking_operation(move || {
        Ok(backup::export_legacy_data(
            &adb_path,
            &target,
            &output_target,
            preflight,
            &operation_id,
            sink,
        )?)
    })
    .await
}

/// List runtime permissions for a package.
#[tauri::command]
#[specta::specta]
pub fn list_permissions(
    target: adb::DeviceTarget,
    package: String,
) -> Result<Vec<PermissionInfo>, CommandError> {
    let transport = validated_transport(&target)?;
    validate_package_arg(&package)?;
    let stdout = transport.shell_target(&target, &["dumpsys", "package", &package])?;
    Ok(parse_permissions(&stdout))
}

/// Grant or revoke a runtime permission.
#[tauri::command]
#[specta::specta]
pub fn set_permission(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    package: String,
    permission: String,
    grant: bool,
    #[allow(non_snake_case)] userId: u32,
) -> Result<ApplyActionResult, CommandError> {
    let (transport, transport_override) = privileged_transport(&target)?;
    validate_package_arg(&package)?;
    if !actions::valid_permission(&permission) {
        return Err(CommandError {
            code: "invalid_permission",
            message: format!("invalid permission identifier {permission:?}"),
        });
    }
    let users = adb::list_users(&transport, &target)?;
    if !users.iter().any(|user| user.id == userId) {
        return Err(CommandError {
            code: "permission_user_missing",
            message: format!("Android user {userId} is not available"),
        });
    }
    let serial = target.serial.clone();
    let plan = actions::plan(actions::ActionRequest {
        serial: serial.clone(),
        target,
        package,
        kind: if grant {
            actions::ActionKind::GrantPermission
        } else {
            actions::ActionKind::RevokePermission
        },
        user_id: userId,
        pack_context: None,
        context: actions::ActionContext {
            confirmation_source: actions::ConfirmationSource::PermissionToggle,
            permission: Some(permission),
            shell_argv: Vec::new(),
            device_control_restore_argv: Vec::new(),
            device_control_expected_before: None,
            transport_override,
            restore_enabled_state: None,
            batch_id: None,
        },
    });
    let dir = journal_dir(&app)?;
    journal::with_journal(&dir, &serial, |journal| {
        execute_journaled(journal, &transport, plan, None)
    })
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct PermissionInfo {
    pub permission: String,
    pub granted: bool,
}

fn parse_permissions(stdout: &str) -> Vec<PermissionInfo> {
    let mut out = Vec::new();
    let mut in_perms = false;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("runtime permissions:")
            || trimmed.starts_with("install permissions:")
        {
            in_perms = true;
            continue;
        }
        if in_perms {
            if trimmed.is_empty()
                || (!trimmed.contains("android.permission") && !trimmed.contains(':'))
            {
                in_perms = false;
                continue;
            }
            // Format: android.permission.CAMERA: granted=true
            if let Some((perm, rest)) = trimmed.split_once(':') {
                let perm = perm.trim();
                if perm.contains("android.permission") || perm.contains("android.") {
                    let granted = rest.contains("granted=true");
                    out.push(PermissionInfo {
                        permission: perm.to_string(),
                        granted,
                    });
                }
            }
        }
    }
    out
}

/// Get process list from a device. Uses `ps -A -o PID,USER,VSZ,RSS,%CPU,NAME`
/// for a structured snapshot.
#[tauri::command]
#[specta::specta]
pub fn list_processes(target: adb::DeviceTarget) -> Result<Vec<ProcessInfo>, CommandError> {
    let transport = validated_transport(&target)?;
    let stdout = transport.shell_target(&target, &["ps", "-A", "-o", "PID,USER,VSZ,RSS,NAME"])?;
    Ok(parse_ps_output(&stdout))
}

/// Running services for a specific package on the device, parsed from
/// `dumpsys activity services <package>`.
#[tauri::command]
#[specta::specta]
pub fn list_running_services(
    target: adb::DeviceTarget,
    package: String,
) -> Result<Vec<RunningService>, CommandError> {
    if !valid_package_name(&package) {
        return Err(CommandError {
            code: "invalid_package",
            message: "invalid package name".to_string(),
        });
    }
    let transport = validated_transport(&target)?;
    let stdout = transport
        .shell_target(&target, &["dumpsys", "activity", "services", &package])
        .unwrap_or_default();
    Ok(parse_running_services(&stdout))
}

/// A read-only snapshot of the current on-screen UI hierarchy plus the raw
/// dump so the renderer can export it verbatim.
#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct LayoutSnapshot {
    pub nodes: Vec<LayoutNode>,
    pub node_count: u32,
    pub raw_xml: String,
}

/// Capture the current UI hierarchy with `uiautomator dump`. This is a
/// read-only inspection: it prints the hierarchy to `/dev/tty` (no device-side
/// file is written) and the renderer never controls any path.
#[tauri::command]
#[specta::specta]
pub fn capture_layout(target: adb::DeviceTarget) -> Result<LayoutSnapshot, CommandError> {
    let transport = validated_transport(&target)?;
    let stdout = transport.shell_target(&target, &["uiautomator", "dump", "/dev/tty"])?;
    let xml = extract_hierarchy(&stdout);
    let nodes = parse_uiautomator_dump(&xml);
    let node_count = nodes
        .iter()
        .filter(|node| node.parse_error.is_none())
        .count() as u32;
    Ok(LayoutSnapshot {
        nodes,
        node_count,
        raw_xml: xml,
    })
}

/// Isolate the `<hierarchy>…</hierarchy>` document from `uiautomator dump`
/// output, which appends a "UI hierarchy dumped to: /dev/tty" status line.
fn extract_hierarchy(stdout: &str) -> String {
    if let (Some(start), Some(end)) = (stdout.find("<hierarchy"), stdout.rfind("</hierarchy>")) {
        if end >= start {
            return stdout[start..end + "</hierarchy>".len()].to_string();
        }
    }
    stdout.trim().to_string()
}

/// Persist a captured UI hierarchy XML through a one-shot path grant. The size
/// bound keeps the IPC and host write bounded, mirroring the Logcat export.
#[tauri::command]
#[specta::specta]
pub async fn save_layout_export(
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    contents: String,
) -> Result<String, CommandError> {
    const MAX_LAYOUT_EXPORT_BYTES: usize = 8 * 1024 * 1024;
    if contents.len() > MAX_LAYOUT_EXPORT_BYTES {
        return Err(CommandError {
            code: "layout_export_too_large",
            message: format!("Layout export exceeds the {MAX_LAYOUT_EXPORT_BYTES}-byte limit"),
        });
    }
    let path = grants.consume(&path_grant, HostPathPurpose::LayoutExportSave)?;
    if !path.parent().is_some_and(std::path::Path::is_dir) {
        return Err(CommandError {
            code: "invalid_path",
            message: "Layout export parent directory does not exist".to_string(),
        });
    }
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(CommandError {
            code: "invalid_path",
            message: "Layout export target must not be a symbolic link".to_string(),
        });
    }
    let display_path = path.display().to_string();
    spawn_blocking_operation(move || {
        std::fs::write(&path, contents.as_bytes())?;
        Ok(display_path)
    })
    .await
}

/// Take a screenshot on the device and pull it to a local path.
#[tauri::command]
#[specta::specta]
pub fn take_screenshot(
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
) -> Result<HostArtifact, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let output_target = grants.consume(&path_grant, HostPathPurpose::ScreenshotSave)?;
    let staged = StagedArtifact::new(&output_target)?;
    // Unique device-side temp so concurrent captures (multiple devices or
    // rapid clicks) never clobber each other's PNG mid-pull.
    let remote = unique_screenshot_remote();
    if let Err(error) = transport.shell_target(&target, &["screencap", "-p", &remote]) {
        let _ = transport.shell_target(&target, &["rm", "-f", &remote]);
        return Err(error.into());
    }
    let stage_arg = staged.path().display().to_string();
    let pulled = actions::extract_apk(&transport.adb_path, &target, &remote, &stage_arg);
    // Always remove the device temp, even when the pull failed, so a
    // partial capture never leaks onto /sdcard.
    let _ = transport.shell_target(&target, &["rm", "-f", &remote]);
    pulled?;
    Ok(staged.commit(ArtifactKind::Png)?)
}

/// Build a per-capture unique `/sdcard` path. Uses the process id plus a
/// monotonic counter so two in-flight screenshots cannot collide without
/// depending on wall-clock time.
fn unique_screenshot_remote() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "/sdcard/droidsmith-screenshot-{}-{}.png",
        std::process::id(),
        n
    )
}

/// Locate the scrcpy binary on the system. Returns the path if found.
#[tauri::command]
#[specta::specta]
pub fn locate_scrcpy() -> Option<String> {
    which::which("scrcpy").ok().map(|p| p.display().to_string())
}

/// Probe the installed scrcpy build and the selected device's reported video
/// encoders. Results are cached against both the binary fingerprint and the
/// immutable device target, so an upgraded/replaced executable is never
/// trusted through stale capability data.
#[tauri::command]
#[specta::specta]
pub async fn scrcpy_capabilities(
    target: adb::DeviceTarget,
) -> Result<crate::scrcpy::ScrcpyCapabilities, CommandError> {
    let transport = validated_transport(&target)?;
    let duplicate_count = transport
        .list_devices()?
        .into_iter()
        .filter(|device| device.serial == target.serial)
        .count();
    if duplicate_count != 1 {
        return Err(CommandError {
            code: "ambiguous_serial",
            message: "scrcpy cannot safely probe a duplicate device serial".to_string(),
        });
    }
    let scrcpy_path = which::which("scrcpy").map_err(|_| CommandError {
        code: "scrcpy_not_found",
        message: "scrcpy binary not found on PATH".to_string(),
    })?;
    spawn_blocking_operation(move || {
        crate::scrcpy::capabilities(&scrcpy_path, &target).map_err(|message| CommandError {
            code: "scrcpy_capability_probe_failed",
            message,
        })
    })
    .await
}

/// Launch scrcpy for a device. Fire-and-forget: we spawn the process
/// and track it so the renderer can poll or stop the session.
#[tauri::command]
#[specta::specta]
pub fn launch_scrcpy(
    request: crate::scrcpy::LaunchScrcpyRequest,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: Option<String>,
) -> Result<crate::scrcpy::ScrcpySession, CommandError> {
    validate_serial_arg(&request.serial)?;
    if request.target.serial != request.serial {
        return Err(CommandError {
            code: "target_mismatch",
            message: "scrcpy target does not match the requested serial".to_string(),
        });
    }
    let (transport, _) = privileged_transport(&request.target)?;
    let duplicate_count = transport
        .list_devices()?
        .into_iter()
        .filter(|device| device.serial == request.serial)
        .count();
    if duplicate_count != 1 {
        return Err(CommandError {
            code: "ambiguous_serial",
            message: "scrcpy cannot safely select a duplicate device serial".to_string(),
        });
    }
    let scrcpy_path = which::which("scrcpy").map_err(|_| CommandError {
        code: "scrcpy_not_found",
        message: "scrcpy binary not found on PATH".to_string(),
    })?;
    let capabilities =
        crate::scrcpy::capabilities(&scrcpy_path, &request.target).map_err(|message| {
            CommandError {
                code: "scrcpy_capability_probe_failed",
                message,
            }
        })?;
    let record_path = path_grant
        .as_deref()
        .map(|grant| grants.consume(grant, HostPathPurpose::ScrcpyRecordSave))
        .transpose()?;
    crate::scrcpy::launch(
        &scrcpy_path,
        request,
        record_path.as_deref(),
        iso_now(),
        &capabilities,
    )
    .map_err(|message| CommandError {
        code: "scrcpy_spawn_failed",
        message,
    })
}

#[tauri::command]
#[specta::specta]
pub fn scrcpy_session_status(
    session_id: u64,
) -> Result<crate::scrcpy::ScrcpySession, CommandError> {
    crate::scrcpy::status(session_id).map_err(|e| CommandError {
        code: "scrcpy_session_not_found",
        message: e,
    })
}

#[tauri::command]
#[specta::specta]
pub fn stop_scrcpy(session_id: u64) -> Result<crate::scrcpy::ScrcpySession, CommandError> {
    crate::scrcpy::stop(session_id).map_err(|e| CommandError {
        code: "scrcpy_stop_failed",
        message: e,
    })
}

/// Locate the gnirehtet binary on the system. Returns the path if found.
#[tauri::command]
#[specta::specta]
pub fn locate_gnirehtet() -> Option<String> {
    which::which("gnirehtet")
        .ok()
        .map(|p| p.display().to_string())
}

/// Start a gnirehtet reverse-tethering session for a device. Supervised like
/// scrcpy: we spawn `gnirehtet run <serial>` and track it so the renderer can
/// poll or stop the session. Stopping restores the device's default network.
#[tauri::command]
#[specta::specta]
pub fn start_gnirehtet(
    target: adb::DeviceTarget,
) -> Result<crate::gnirehtet::GnirehtetSession, CommandError> {
    validate_serial_arg(&target.serial)?;
    let (transport, _) = privileged_transport(&target)?;
    let duplicate_count = transport
        .list_devices()?
        .into_iter()
        .filter(|device| device.serial == target.serial)
        .count();
    if duplicate_count != 1 {
        return Err(CommandError {
            code: "ambiguous_serial",
            message: "gnirehtet cannot safely select a duplicate device serial".to_string(),
        });
    }
    let gnirehtet_path = which::which("gnirehtet").map_err(|_| CommandError {
        code: "gnirehtet_not_found",
        message: "gnirehtet binary not found on PATH".to_string(),
    })?;
    crate::gnirehtet::start(&gnirehtet_path, target.serial, iso_now()).map_err(|message| {
        CommandError {
            code: "gnirehtet_spawn_failed",
            message,
        }
    })
}

/// Return the supervised gnirehtet session already running for this device, if
/// any, so a renderer remount can re-attach to it instead of showing "start"
/// and spawning a duplicate that would fail on the busy relay port. Persists
/// reverse-tethering across navigation.
#[tauri::command]
#[specta::specta]
pub fn find_gnirehtet_session(
    target: adb::DeviceTarget,
) -> Result<Option<crate::gnirehtet::GnirehtetSession>, CommandError> {
    validate_serial_arg(&target.serial)?;
    crate::gnirehtet::find_running_by_serial(&target.serial).map_err(|message| CommandError {
        code: "gnirehtet_lookup_failed",
        message,
    })
}

#[tauri::command]
#[specta::specta]
pub fn gnirehtet_session_status(
    session_id: u64,
) -> Result<crate::gnirehtet::GnirehtetSession, CommandError> {
    crate::gnirehtet::status(session_id).map_err(|e| CommandError {
        code: "gnirehtet_session_not_found",
        message: e,
    })
}

#[tauri::command]
#[specta::specta]
pub fn stop_gnirehtet(session_id: u64) -> Result<crate::gnirehtet::GnirehtetSession, CommandError> {
    crate::gnirehtet::stop(session_id).map_err(|e| CommandError {
        code: "gnirehtet_stop_failed",
        message: e,
    })
}

/// Install an APK or split-package archive on a device. Single APKs retain the
/// direct `adb install -r` path; APKS/XAPK/APKM archives are committed through
/// an atomic PackageInstaller session.
#[tauri::command]
#[specta::specta]
pub async fn install_apk(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    options: install::InstallOptions,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<install::InstallPackageResult, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let validated_path = grants.consume(&path_grant, HostPathPurpose::InstallOpen)?;
    let retry_path = validated_path.clone();
    let app_data_dir = app.path().app_data_dir().map_err(|error| CommandError {
        code: "no_app_data_dir",
        message: error.to_string(),
    })?;
    let sink = operations::channel_sink(on_event);
    let mut result = spawn_blocking_operation(move || {
        Ok(install::install_package(
            &transport,
            &target,
            &validated_path,
            &app_data_dir,
            &operation_id,
            options,
            sink,
        )?)
    })
    .await?;
    if result
        .failure
        .as_ref()
        .and_then(|failure| failure.suggested_override)
        .is_some()
        && !options.override_confirmed
    {
        result.retry_path_grant = grants
            .issue(&retry_path, HostPathPurpose::InstallOpen)
            .ok()
            .map(|grant| grant.id);
    }
    Ok(result)
}

/// Pull an APK from the device to a local path.
#[tauri::command]
#[specta::specta]
pub async fn extract_apk(
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    remote_path: String,
    path_grant: String,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<HostArtifact, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let output_target = grants.consume(&path_grant, HostPathPurpose::ExtractApkSave)?;
    let remote = validate_remote_path(&remote_path)?;
    let selector = target.adb_selector();
    let adb_path = transport.adb_path.clone();
    let sink = operations::channel_sink(on_event);
    spawn_blocking_operation(move || {
        let staged = StagedArtifact::new(&output_target)?;
        let mut args = selector;
        args.extend([
            "pull".to_string(),
            remote,
            staged.path().display().to_string(),
        ]);
        let output = operations::run_process(
            &adb_path,
            &args,
            std::time::Duration::from_secs(120),
            &operation_id,
            "Extracting APK",
            sink,
        )?;
        completed_adb_output(output, "adb pull")?;
        Ok(staged.commit(ArtifactKind::Apk)?)
    })
    .await
}

/// List all debloat packs from the app's `packs/` resource directory.
/// A bundled pack file that failed to load, with a stable code and a
/// human-readable message the UI can show and the user can copy.
#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PackLoadError {
    /// File name (not full path — no host paths leak to the renderer).
    pub file: String,
    /// Stable code: `pack_read`, `pack_parse`, `pack_validate`, or
    /// `pack_duplicate_id`.
    pub code: &'static str,
    pub message: String,
}

/// Result of enumerating bundled packs: the healthy packs plus per-file
/// errors for any that failed. A broken file no longer disappears
/// silently — it surfaces as an error the user can act on.
#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct PackListing {
    pub packs: Vec<crate::packs::PackCandidate>,
    pub errors: Vec<PackLoadError>,
}

#[derive(specta::Type, Debug, Clone, Deserialize)]
pub struct PlanPackRequest {
    pub target: adb::DeviceTarget,
    pub user_id: u32,
    pub pack_id: String,
    pub revision: u32,
    pub selected: Vec<String>,
    #[serde(default)]
    pub override_compatibility: bool,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct PlannedPack {
    pub pack_id: String,
    pub revision: u32,
    pub assessment: crate::packs::PackAssessment,
    pub selected_ids: Vec<String>,
    pub plans: Vec<actions::PlannedAction>,
    pub skipped: Vec<crate::packs::PackEntryAssessment>,
}

fn pack_error_to_load_error(file: String, err: &crate::packs::PackError) -> PackLoadError {
    use crate::packs::PackError;
    let code = match err {
        PackError::Read { .. } => "pack_read",
        PackError::Parse { .. } => "pack_parse",
        PackError::Validate { .. } => "pack_validate",
    };
    PackLoadError {
        file,
        code,
        message: err.to_string(),
    }
}

/// Returns packs that parse and lint cleanly, plus a per-file error for
/// each broken file so a packaging defect is visible instead of looking
/// like an empty pack list.
fn load_runtime_packs(
    packs_dir: &std::path::Path,
) -> Result<(Vec<crate::packs::Pack>, Vec<PackLoadError>), CommandError> {
    if !packs_dir.is_dir() {
        return Ok((Vec::new(), Vec::new()));
    }
    let entries = std::fs::read_dir(packs_dir).map_err(|e| CommandError {
        code: "io_error",
        message: format!("could not read packs directory: {e}"),
    })?;
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if file.starts_with('_') {
            continue;
        }
        if path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml")
        {
            match crate::packs::load(&path) {
                Ok(pack) => loaded.push((file, pack)),
                Err(err) => errors.push(pack_error_to_load_error(file, &err)),
            }
        }
    }
    let id_counts = loaded.iter().fold(
        std::collections::HashMap::<String, usize>::new(),
        |mut counts, (_, pack)| {
            *counts.entry(pack.id.clone()).or_default() += 1;
            counts
        },
    );
    let mut packs = Vec::new();
    for (file, pack) in loaded {
        if id_counts.get(pack.id.as_str()).copied().unwrap_or_default() > 1 {
            errors.push(PackLoadError {
                file,
                code: "pack_duplicate_id",
                message: format!(
                    "stable pack id {:?} is declared by more than one runtime pack",
                    pack.id
                ),
            });
        } else {
            packs.push(pack);
        }
    }
    packs.sort_by(|a, b| a.id.cmp(&b.id));
    errors.sort_by(|a, b| a.file.cmp(&b.file));
    Ok((packs, errors))
}

/// Absolute path of the app-data directory that holds user-imported packs.
/// Imported pack files are named `<pack-id>.yaml` (the id is validated
/// kebab-case, so the name can never traverse out of this directory).
fn user_packs_dir(app: &tauri::AppHandle) -> Result<PathBuf, CommandError> {
    Ok(settings_app_data_dir(app)?.join("packs"))
}

/// A merged pack set: each pack paired with an `imported` flag (`true` when it
/// came from the user-imported app-data directory), plus per-file load errors.
type MergedPacks = (Vec<(crate::packs::Pack, bool)>, Vec<PackLoadError>);

/// Load bundled packs (from the resource directory) merged with any packs the
/// user has imported into the app-data `packs/` directory. Bundled ids win: an
/// imported pack whose id shadows a bundled one surfaces as a load error rather
/// than silently overriding the shipped pack. The `bool` is `true` for
/// imported packs.
fn load_all_packs(
    bundled_dir: &std::path::Path,
    user_dir: &std::path::Path,
) -> Result<MergedPacks, CommandError> {
    let (bundled, mut errors) = load_runtime_packs(bundled_dir)?;
    let (imported, imported_errors) = load_runtime_packs(user_dir)?;
    errors.extend(imported_errors);

    let bundled_ids: std::collections::HashSet<String> =
        bundled.iter().map(|pack| pack.id.clone()).collect();
    let mut packs: Vec<(crate::packs::Pack, bool)> =
        bundled.into_iter().map(|pack| (pack, false)).collect();
    for pack in imported {
        if bundled_ids.contains(&pack.id) {
            errors.push(PackLoadError {
                file: format!("{}.yaml", pack.id),
                code: "pack_duplicate_id",
                message: format!(
                    "imported pack id {:?} shadows a bundled pack; remove the import",
                    pack.id
                ),
            });
        } else {
            packs.push((pack, true));
        }
    }
    packs.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    errors.sort_by(|a, b| a.file.cmp(&b.file));
    Ok((packs, errors))
}

fn pack_context(
    transport: &adb::ShellTransport,
    target: &adb::DeviceTarget,
    user_id: u32,
) -> Result<crate::packs::DevicePackContext, CommandError> {
    adb::validate_device_target(transport, target)?;
    let users = adb::list_users(transport, target)?;
    let user = users
        .iter()
        .find(|user| user.id == user_id)
        .ok_or(CommandError {
            code: "pack_user_missing",
            message: format!("Android user {user_id} is not available"),
        })?;
    let info = adb::get_device_info(transport, target)?;
    let installed_packages =
        adb::list_packages(transport, target, adb::PackageFilter::All, user_id)?
            .into_iter()
            .filter(|package| !package.archived)
            .map(|package| package.package)
            .collect();
    Ok(crate::packs::DevicePackContext {
        manufacturer: info.manufacturer,
        model: info.model,
        build_fingerprint: info.build_fingerprint,
        api_level: info.sdk_level.and_then(|value| value.parse().ok()),
        user_id,
        user_current: user.current,
        installed_packages,
    })
}

#[tauri::command]
#[specta::specta]
pub fn list_packs(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    #[allow(non_snake_case)] userId: u32,
) -> Result<PackListing, CommandError> {
    let resource_dir = app.path().resource_dir().map_err(|e| CommandError {
        code: "no_resource_dir",
        message: e.to_string(),
    })?;
    let user_dir = user_packs_dir(&app)?;

    let (packs, errors) = load_all_packs(&resource_dir.join("packs"), &user_dir)?;
    let transport = validated_transport(&target)?;
    let context = pack_context(&transport, &target, userId)?;
    let packs = packs
        .into_iter()
        .map(|(pack, imported)| crate::packs::PackCandidate {
            assessment: crate::packs::assess(&pack, &context),
            pack,
            imported,
        })
        .collect();
    Ok(PackListing { packs, errors })
}

/// Metadata returned after a debloat pack is imported from a local file.
#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ImportedPack {
    pub id: String,
    pub name: String,
    pub revision: u32,
    /// SHA-256 of the imported file, computed at import time. Surfaced so the
    /// user can record it and re-import the same bytes with a pin later.
    pub sha256: String,
    /// Number of package entries the pack offers to remove.
    pub packages: usize,
}

/// Import a debloat pack from a user-selected local file through a one-shot
/// native read grant. This is the network-free alternative to remote-pack
/// fetching (R-095): it reuses the audited host-path grant model, optionally
/// verifies a caller-supplied SHA-256 pin, schema-validates and lints the
/// bytes, rejects ids that shadow a bundled pack, and persists the file to the
/// app-data `packs/` directory so it appears in the picker on the next load.
#[tauri::command]
#[specta::specta]
pub fn import_pack(
    app: tauri::AppHandle,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    #[allow(non_snake_case)] expectedSha256: Option<String>,
) -> Result<ImportedPack, CommandError> {
    let source = grants.consume(&path_grant, HostPathPurpose::PackImportOpen)?;

    let actual_sha256 = crate::fs_util::sha256_file(&source).map_err(|error| CommandError {
        code: "pack_read",
        message: format!("could not read the selected pack file: {error}"),
    })?;
    if let Some(expected) = expectedSha256 {
        let expected = expected.trim().to_ascii_lowercase();
        if !expected.is_empty() {
            if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(CommandError {
                    code: "pack_sha256_invalid",
                    message: "expected SHA-256 must be 64 hexadecimal characters".to_string(),
                });
            }
            if expected != actual_sha256 {
                return Err(CommandError {
                    code: "pack_sha256_mismatch",
                    message: format!(
                        "pack SHA-256 does not match: expected {expected}, got {actual_sha256}"
                    ),
                });
            }
        }
    }

    let pack = crate::packs::load(&source).map_err(|error| {
        let load_error = pack_error_to_load_error(String::new(), &error);
        CommandError {
            code: load_error.code,
            message: load_error.message,
        }
    })?;

    // `packs::load` lints the id to lowercase kebab-case, so `<id>.yaml` is a
    // safe filename; guard again for defense in depth before touching the FS.
    if !crate::packs::valid_pack_id(&pack.id) {
        return Err(CommandError {
            code: "pack_validate",
            message: format!("pack id {:?} is not a valid identifier", pack.id),
        });
    }

    let resource_dir = app.path().resource_dir().map_err(|e| CommandError {
        code: "no_resource_dir",
        message: e.to_string(),
    })?;
    let (bundled, _) = load_runtime_packs(&resource_dir.join("packs"))?;
    if bundled
        .iter()
        .any(|bundled_pack| bundled_pack.id == pack.id)
    {
        return Err(CommandError {
            code: "pack_id_conflicts_bundled",
            message: format!(
                "a bundled pack already uses id {:?}; imported packs must have a unique id",
                pack.id
            ),
        });
    }

    let user_dir = user_packs_dir(&app)?;
    std::fs::create_dir_all(&user_dir).map_err(|error| CommandError {
        code: "io_error",
        message: format!("could not create the imported-packs directory: {error}"),
    })?;
    let destination = user_dir.join(format!("{}.yaml", pack.id));
    std::fs::copy(&source, &destination).map_err(|error| CommandError {
        code: "io_error",
        message: format!("could not store the imported pack: {error}"),
    })?;

    Ok(ImportedPack {
        id: pack.id.clone(),
        name: pack.name.clone(),
        revision: pack.revision,
        sha256: actual_sha256,
        packages: pack.packages.len(),
    })
}

/// Remove a previously-imported debloat pack by its stable id. Bundled packs
/// live in the read-only resource directory and are never touched. Returns
/// `true` when a file was deleted, `false` when no import with that id existed.
#[tauri::command]
#[specta::specta]
pub fn remove_imported_pack(
    app: tauri::AppHandle,
    #[allow(non_snake_case)] packId: String,
) -> Result<bool, CommandError> {
    if !crate::packs::valid_pack_id(&packId) {
        return Err(CommandError {
            code: "pack_id_invalid",
            message: "invalid pack id".to_string(),
        });
    }
    let destination = user_packs_dir(&app)?.join(format!("{packId}.yaml"));
    match std::fs::remove_file(&destination) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CommandError {
            code: "io_error",
            message: format!("could not remove the imported pack: {error}"),
        }),
    }
}

/// Result of exporting a device's captured debloat state to a pack file.
#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ExportedDevicePack {
    pub pack_id: String,
    pub packages: usize,
    pub artifact: crate::fs_util::HostArtifact,
}

/// Capture the selected device's currently disabled/archived/uninstalled
/// packages and write them to a schema-valid debloat pack YAML through a
/// one-shot native save grant (R-098). The result round-trips through
/// `import_pack`, so "what I removed on this phone" can be re-applied to another
/// device after an OTA or factory reset.
#[tauri::command]
#[specta::specta]
pub fn export_device_pack(
    grants: tauri::State<'_, PathGrantStore>,
    target: adb::DeviceTarget,
    #[allow(non_snake_case)] userId: u32,
    path_grant: String,
) -> Result<ExportedDevicePack, CommandError> {
    let destination = grants.consume(&path_grant, HostPathPurpose::PackExportSave)?;
    let transport = validated_transport(&target)?;
    adb::validate_device_target(&transport, &target)?;
    let info = adb::get_device_info(&transport, &target)?;
    let packages = adb::list_packages(&transport, &target, adb::PackageFilter::All, userId)?;

    let removed: Vec<crate::packs::RemovedPackage> = packages
        .into_iter()
        .filter_map(|package| {
            let kind = if package.archived {
                crate::packs::RemovedKind::Archived
            } else if package.retained {
                crate::packs::RemovedKind::Uninstalled
            } else if !package.enabled {
                crate::packs::RemovedKind::Disabled
            } else {
                return None;
            };
            Some(crate::packs::RemovedPackage {
                id: package.package,
                kind,
            })
        })
        .collect();

    let context = crate::packs::DeviceExportContext {
        manufacturer: info.manufacturer,
        model: info.model,
        api_level: info.sdk_level.and_then(|value| value.parse().ok()),
        user_id: userId,
        date: crate::time::iso_utc_now().chars().take(10).collect(),
    };
    let pack =
        crate::packs::from_device_state(&removed, &context).map_err(|message| CommandError {
            code: "pack_export_empty",
            message,
        })?;
    let yaml = crate::packs::to_yaml(&pack).map_err(|error| CommandError {
        code: "pack_export_serialize",
        message: error.to_string(),
    })?;

    let staged =
        crate::fs_util::StagedArtifact::new(&destination).map_err(|error| CommandError {
            code: "io_error",
            message: error.to_string(),
        })?;
    std::fs::write(staged.path(), yaml).map_err(|error| CommandError {
        code: "io_error",
        message: format!("could not write the exported pack: {error}"),
    })?;
    let artifact = staged
        .commit(crate::fs_util::ArtifactKind::AnyFile)
        .map_err(|error| CommandError {
            code: "io_error",
            message: error.to_string(),
        })?;

    Ok(ExportedDevicePack {
        pack_id: pack.id,
        packages: pack.packages.len(),
        artifact,
    })
}

/// Statically analyze a local APK file the user selects through a one-shot
/// native read grant (R-097). Fully offline and device-free: parses the binary
/// manifest, DEX headers, signing artifacts, and a per-entry size breakdown.
#[tauri::command]
#[specta::specta]
pub fn analyze_apk(
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
) -> Result<crate::apk_analysis::ApkAnalysis, CommandError> {
    let path = grants.consume(&path_grant, HostPathPurpose::ApkAnalyzeOpen)?;
    crate::apk_analysis::analyze(&path).map_err(|error| CommandError {
        code: error.code(),
        message: error.to_string(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn plan_pack(
    app: tauri::AppHandle,
    request: PlanPackRequest,
) -> Result<PlannedPack, CommandError> {
    if request.selected.is_empty() {
        return Err(CommandError {
            code: "pack_selection_empty",
            message: "select at least one pack entry".to_string(),
        });
    }
    let resource_dir = app.path().resource_dir().map_err(|e| CommandError {
        code: "no_resource_dir",
        message: e.to_string(),
    })?;
    let (packs, _) = load_all_packs(&resource_dir.join("packs"), &user_packs_dir(&app)?)?;
    let pack = packs
        .into_iter()
        .map(|(pack, _)| pack)
        .find(|pack| pack.id == request.pack_id)
        .ok_or(CommandError {
            code: "pack_not_found",
            message: format!("debloat pack {:?} is not available", request.pack_id),
        })?;
    if pack.revision != request.revision {
        return Err(CommandError {
            code: "pack_revision_changed",
            message: format!(
                "pack {} changed from revision {} to {}; review it again",
                pack.id, request.revision, pack.revision
            ),
        });
    }
    let transport = validated_transport(&request.target)?;
    let context = pack_context(&transport, &request.target, request.user_id)?;
    let assessment = crate::packs::assess(&pack, &context);
    if assessment.override_required && !request.override_compatibility {
        return Err(CommandError {
            code: "pack_compatibility_override_required",
            message: format!(
                "pack {} is {:?} for this device/user; review checks and explicitly accept the override",
                pack.id, assessment.status
            ),
        });
    }
    let selected =
        crate::packs::expand_dependencies(&pack, request.selected).map_err(|message| {
            CommandError {
                code: "pack_selection_invalid",
                message,
            }
        })?;
    let selected_ids: Vec<String> = pack
        .packages
        .iter()
        .filter(|entry| selected.contains(&entry.id))
        .map(|entry| entry.id.clone())
        .collect();
    let status_by_id: std::collections::HashMap<&str, &crate::packs::PackEntryAssessment> =
        assessment
            .entries
            .iter()
            .map(|entry| (entry.id.as_str(), entry))
            .collect();
    let mut plans = Vec::new();
    let mut skipped = Vec::new();
    for entry in pack
        .packages
        .iter()
        .filter(|entry| selected.contains(&entry.id))
    {
        let support = status_by_id
            .get(entry.id.as_str())
            .expect("assessment covers every pack entry");
        if support.status != crate::packs::PackEntryStatus::Ready {
            skipped.push((*support).clone());
            continue;
        }
        plans.push(actions::plan(actions::ActionRequest {
            serial: request.target.serial.clone(),
            target: request.target.clone(),
            package: entry.id.clone(),
            kind: actions::ActionKind::Disable,
            user_id: request.user_id,
            pack_context: Some(actions::PackActionContext {
                pack_id: pack.id.clone(),
                revision: pack.revision,
                provenance_source: pack.provenance.source.clone(),
                provenance_license: pack.provenance.license.clone(),
                compatibility_status: format!("{:?}", assessment.status).to_lowercase(),
                override_accepted: request.override_compatibility,
            }),
            context: actions::ActionContext {
                confirmation_source: actions::ConfirmationSource::DebloatPreview,
                ..Default::default()
            },
        }));
    }
    Ok(PlannedPack {
        pack_id: pack.id,
        revision: pack.revision,
        assessment,
        selected_ids,
        plans,
        skipped,
    })
}

fn iso_now() -> String {
    crate::time::iso_utc_now()
}

#[cfg(test)]
mod tests {
    use super::{
        accepted_transport_override, append_host_operation, classify_shell, diagnostic_text,
        execute_batch_plans, is_allowed_device_control, load_all_packs, load_runtime_packs,
        pack_error_to_load_error, parse_fastboot_getvar, plan_action_batch, profile_preview_rows,
        unique_screenshot_remote, validate_action_batch_plan, validate_backup_target,
        validate_remote_path, AdbRecoveryOutcome, AdbRecoveryRecord, ProcessOutput,
        ProfilePreviewStatus, ShellClassification,
    };
    use crate::adb::device::DeviceState;
    use crate::adb::transport::MockTransport;
    use crate::adb::{
        actions::{ActionKind, ActionRequest},
        AppPackage, Device, DeviceTarget, DeviceTransportKind,
    };

    fn batch_device() -> Device {
        Device {
            serial: "batch-device".to_string(),
            state: DeviceState::Device,
            model: Some("Pixel".to_string()),
            product: Some("pixel".to_string()),
            device: Some("husky".to_string()),
            build_fingerprint: Some("google/husky/build".to_string()),
            transport_id: Some(9),
            connection_generation: 10,
            transport_kind: DeviceTransportKind::Usb,
            wireless: false,
        }
    }

    fn batch_request(package: &str, kind: ActionKind) -> ActionRequest {
        let device = batch_device();
        ActionRequest {
            serial: device.serial.clone(),
            target: device.target(),
            package: package.to_string(),
            kind,
            user_id: 0,
            pack_context: None,
            context: Default::default(),
        }
    }

    #[test]
    fn batch_planner_rejects_mixed_or_duplicate_targets() {
        let duplicate = plan_action_batch(vec![
            batch_request("com.example.one", ActionKind::Disable),
            batch_request("com.example.one", ActionKind::Disable),
        ])
        .unwrap_err();
        assert_eq!(duplicate.code, "duplicate_batch_package");

        let mixed = plan_action_batch(vec![
            batch_request("com.example.one", ActionKind::Disable),
            batch_request("com.example.two", ActionKind::Enable),
        ])
        .unwrap_err();
        assert_eq!(mixed.code, "mixed_action_batch");

        let irreversible = plan_action_batch(vec![
            batch_request("com.example.one", ActionKind::ClearData),
            batch_request("com.example.two", ActionKind::ClearData),
        ])
        .unwrap_err();
        assert_eq!(irreversible.code, "invalid_action_kind");
    }

    #[test]
    fn batch_executor_continues_after_a_package_failure() {
        let mut batch = plan_action_batch(vec![
            batch_request("com.example.ok", ActionKind::Disable),
            batch_request("com.example.fail", ActionKind::Disable),
        ])
        .unwrap();
        assert!(validate_action_batch_plan(&batch).is_ok());
        for plan in &mut batch.plans {
            plan.request.context.batch_id = Some("batch-test-1".to_string());
        }

        let device = batch_device();
        let mock = MockTransport::new().with_devices(vec![device]);
        for package in ["com.example.ok", "com.example.fail"] {
            mock.expect_shell(
                "batch-device",
                &["pm", "list", "packages", "--user", "0", "-d", package],
                Ok(String::new()),
            );
            mock.expect_shell(
                "batch-device",
                &["pm", "list", "packages", "--user", "0", package],
                Ok(format!("package:{package}\n")),
            );
            mock.expect_shell(
                "batch-device",
                &["pm", "list", "users"],
                Ok("Users:\n  UserInfo{0:Owner:c13} running (current)\n".to_string()),
            );
            mock.expect_shell(
                "batch-device",
                &["am", "get-current-user"],
                Ok("0\n".to_string()),
            );
        }
        mock.expect_shell(
            "batch-device",
            &["pm", "disable-user", "--user", "0", "com.example.ok"],
            Ok("Package com.example.ok new state: disabled-user\n".to_string()),
        );
        mock.expect_shell(
            "batch-device",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "0",
                "-d",
                "com.example.ok",
            ],
            Ok("package:com.example.ok\n".to_string()),
        );
        mock.expect_shell(
            "batch-device",
            &["pm", "disable-user", "--user", "0", "com.example.fail"],
            Ok("Failure [PACKAGE_NOT_FOUND]\n".to_string()),
        );

        let dir = std::env::temp_dir().join(format!(
            "droidsmith-batch-test-{}-{}",
            std::process::id(),
            crate::time::iso_utc_now().replace([':', '.'], "-")
        ));
        let mut journal = crate::journal::Journal::open(&dir, "batch-device").unwrap();
        let items = execute_batch_plans(&mut journal, &mock, batch.plans, None).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].error.is_none());
        assert!(items[1].error.is_some());
        assert_eq!(journal.entries().len(), 2);
        assert_eq!(
            journal.entries()[0].outcome,
            crate::journal::JournalOutcome::Succeeded
        );
        assert_eq!(
            journal.entries()[1].outcome,
            crate::journal::JournalOutcome::Failed
        );
        assert!(journal.entries().iter().all(|entry| {
            entry.applied.plan.request.context.batch_id.as_deref() == Some("batch-test-1")
        }));
        drop(journal);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn profile_preview_reports_ready_matching_and_missing_rows_without_mutation() {
        let profile = crate::profile::Profile {
            name: "preview-test".to_string(),
            version: crate::profile::PROFILE_SCHEMA_VERSION.to_string(),
            description: String::new(),
            device: Default::default(),
            user: Default::default(),
            actions: vec![
                crate::profile::ProfileAction {
                    kind: ActionKind::Disable,
                    package: "com.example.enabled".to_string(),
                    note: String::new(),
                },
                crate::profile::ProfileAction {
                    kind: ActionKind::Disable,
                    package: "com.example.disabled".to_string(),
                    note: String::new(),
                },
                crate::profile::ProfileAction {
                    kind: ActionKind::Enable,
                    package: "com.example.missing".to_string(),
                    note: String::new(),
                },
            ],
        };
        let target = DeviceTarget {
            serial: "QA123".to_string(),
            transport_id: Some(7),
            connection_generation: 2,
            transport_kind: DeviceTransportKind::Usb,
            untrusted_transport_override: false,
            model: Some("Pixel QA".to_string()),
            product: None,
            device: None,
            build_fingerprint: Some("google/qa/qa:17/test".to_string()),
        };
        let package = |name: &str, enabled: bool| AppPackage {
            package: name.to_string(),
            enabled,
            system: false,
            apk_path: None,
            uid: None,
            installer: None,
            archived: false,
            retained: false,
        };
        let rows = profile_preview_rows(
            &profile,
            &target,
            10,
            &[
                package("com.example.enabled", true),
                package("com.example.disabled", false),
            ],
        );

        assert!(matches!(rows[0].status, ProfilePreviewStatus::Ready));
        assert!(matches!(
            rows[1].status,
            ProfilePreviewStatus::AlreadyMatches
        ));
        assert!(matches!(rows[2].status, ProfilePreviewStatus::Missing));
        assert!(rows.iter().all(|row| row.plan.request.user_id == 10));
        assert!(rows
            .iter()
            .all(|row| row.plan.request.context.confirmation_source
                == crate::adb::actions::ConfirmationSource::ProfilePreview));
    }

    #[test]
    fn host_recovery_records_are_newline_delimited_and_synced() {
        let dir = std::env::temp_dir().join(format!(
            "droidsmith-host-audit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("host-operations.jsonl");
        let record = AdbRecoveryRecord {
            schema_version: 1,
            operation_id: "adb-recovery-test".to_string(),
            operation: "adb_server_recovery",
            confirmation_source: "devices_health_review",
            outcome: AdbRecoveryOutcome::Pending,
            started_at: "2026-07-14T18:00:00Z".to_string(),
            completed_at: None,
            commands: vec![vec!["kill-server".to_string()]],
            health_before: None,
            health_after: None,
            failure: None,
        };
        append_host_operation(&path, &record).unwrap();
        append_host_operation(&path, &record).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn host_diagnostic_text_is_bounded_and_drops_controls() {
        let value = format!("ok\0{}", "x".repeat(2_000));
        let sanitized = diagnostic_text(&value);
        assert!(!sanitized.contains('\0'));
        assert_eq!(sanitized.chars().count(), 1_024);
    }

    #[test]
    fn remote_path_rejects_flags_relative_and_traversal() {
        // A leading '-' would reach adb as an option flag.
        assert_eq!(
            validate_remote_path("-a").unwrap_err().code,
            "invalid_remote_path"
        );
        // Relative and empty are rejected; only absolute device paths pass.
        assert_eq!(
            validate_remote_path("sdcard/x").unwrap_err().code,
            "invalid_remote_path"
        );
        assert_eq!(
            validate_remote_path("   ").unwrap_err().code,
            "invalid_remote_path"
        );
        assert_eq!(
            validate_remote_path("/sdcard/../data/secret")
                .unwrap_err()
                .code,
            "invalid_remote_path"
        );
        assert_eq!(
            validate_remote_path("/sdcard/./Download").unwrap_err().code,
            "invalid_remote_path"
        );
        assert_eq!(
            validate_remote_path("/sdcard/Download\nsecret")
                .unwrap_err()
                .code,
            "invalid_remote_path"
        );
        assert_eq!(
            validate_remote_path("/sdcard/Download/app.apk").unwrap(),
            "/sdcard/Download/app.apk"
        );
    }

    #[test]
    fn shell_classifier_fails_unknown_commands_into_review() {
        let args = |values: &[&str]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            classify_shell(&args(&["getprop", "ro.product.model"])),
            ShellClassification::ReadOnly
        );
        assert_eq!(
            classify_shell(&args(&["wm", "density", "420"])),
            ShellClassification::Mutation
        );
        assert_eq!(
            classify_shell(&args(&["rm", "-rf", "/sdcard/data"])),
            ShellClassification::Dangerous
        );
        // A read-only head must not launder a chained/redirected mutation past
        // the classifier: any shell control metacharacter forces Dangerous so
        // shell_run rejects it and plan_shell_action routes it through review.
        assert_eq!(
            classify_shell(&args(&["getprop", "ro.build;", "pm", "uninstall", "com.x"])),
            ShellClassification::Dangerous
        );
        assert_eq!(
            classify_shell(&args(&["cat", "/proc/version", "&&", "reboot"])),
            ShellClassification::Dangerous
        );
        assert_eq!(
            classify_shell(&args(&["settings", "get", "secure", "$(reboot)"])),
            ShellClassification::Dangerous
        );
        assert_eq!(
            classify_shell(&args(&["getprop", ">", "/sdcard/x"])),
            ShellClassification::Dangerous
        );
        // Plain read-only commands with dotted/underscored operands still pass.
        assert_eq!(
            classify_shell(&args(&["settings", "get", "global", "adb_enabled"])),
            ShellClassification::ReadOnly
        );
    }

    #[test]
    fn device_control_allowlist_is_exact_and_bounded() {
        let args = |values: &[&str]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        };
        assert!(is_allowed_device_control(&args(&[
            "input", "keyevent", "3"
        ])));
        assert!(is_allowed_device_control(&args(&["wm", "density", "420"])));
        assert!(!is_allowed_device_control(&args(&[
            "wm", "density", "5000"
        ])));
        assert!(!is_allowed_device_control(&args(&[
            "settings",
            "delete",
            "secure",
            "adb_enabled"
        ])));
    }

    #[test]
    fn broken_pack_maps_to_stable_error_code() {
        let dir = std::env::temp_dir().join("droidsmith-pack-err-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Invalid YAML → parse error.
        let bad_parse = dir.join("broken.yaml");
        std::fs::write(&bad_parse, "name: [unterminated\n").unwrap();
        let err = crate::packs::load(&bad_parse).unwrap_err();
        let mapped = pack_error_to_load_error("broken.yaml".to_string(), &err);
        assert_eq!(mapped.code, "pack_parse");
        assert_eq!(mapped.file, "broken.yaml");
        assert!(!mapped.message.is_empty());

        // Well-formed YAML (all required fields present) that fails lint
        // on an empty name → validate error.
        let bad_validate = dir.join("empty.yaml");
        std::fs::write(
            &bad_validate,
            "name: \"\"\nversion: \"1\"\ndescription: \"x\"\npackages:\n  - id: com.x\n    removal: recommended\n    description: y\n",
        )
        .unwrap();
        let err = crate::packs::load(&bad_validate).unwrap_err();
        let mapped = pack_error_to_load_error("empty.yaml".to_string(), &err);
        assert_eq!(mapped.code, "pack_validate");
    }

    #[test]
    fn runtime_pack_loader_never_lists_underscore_templates() {
        let dir = std::env::temp_dir().join("droidsmith-runtime-pack-filter-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let yaml = r#"
id: runtime-test
revision: 1
name: Runtime test
version: "1"
description: Runtime loader test.
targets:
  user_scope: any
provenance:
  source: https://example.invalid/test
  license: MIT
packages:
  - id: com.example.runtime
    removal: recommended
    description: Runtime test package.
"#;
        std::fs::write(dir.join("runtime.yaml"), yaml).unwrap();
        std::fs::write(
            dir.join("_template.yaml"),
            yaml.replace("runtime-test", "template-test"),
        )
        .unwrap();

        let (packs, errors) = load_runtime_packs(&dir).unwrap();
        assert!(errors.is_empty());
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "runtime-test");
    }

    #[test]
    fn runtime_pack_loader_rejects_duplicate_stable_ids() {
        let dir = std::env::temp_dir().join("droidsmith-runtime-pack-id-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let yaml = r#"
id: duplicate-test
revision: 1
name: Duplicate test
version: "1"
description: Duplicate stable ID test.
targets:
  user_scope: any
provenance:
  source: https://example.invalid/test
  license: MIT
packages:
  - id: com.example.duplicate
    removal: recommended
    description: Duplicate test package.
"#;
        std::fs::write(dir.join("one.yaml"), yaml).unwrap();
        std::fs::write(dir.join("two.yaml"), yaml).unwrap();

        let (packs, errors) = load_runtime_packs(&dir).unwrap();
        assert!(packs.is_empty());
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|error| error.code == "pack_duplicate_id"));
    }

    fn pack_yaml(id: &str, package: &str) -> String {
        format!(
            r#"
id: {id}
revision: 1
name: Pack {id}
version: "1"
description: Merge loader test pack.
targets:
  user_scope: any
provenance:
  source: https://example.invalid/test
  license: MIT
packages:
  - id: {package}
    removal: recommended
    description: Merge test package.
"#
        )
    }

    fn merge_dirs(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("droidsmith-merge-pack-{name}"));
        let _ = std::fs::remove_dir_all(&base);
        let bundled = base.join("bundled");
        let user = base.join("user");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&user).unwrap();
        (bundled, user)
    }

    #[test]
    fn merged_loader_flags_bundled_and_imported_packs() {
        let (bundled, user) = merge_dirs("merge");
        std::fs::write(
            bundled.join("shipped.yaml"),
            pack_yaml("shipped-pack", "com.example.shipped"),
        )
        .unwrap();
        std::fs::write(
            user.join("imported.yaml"),
            pack_yaml("imported-pack", "com.example.imported"),
        )
        .unwrap();

        let (packs, errors) = load_all_packs(&bundled, &user).unwrap();
        assert!(errors.is_empty());
        assert_eq!(packs.len(), 2);
        // Sorted by id: imported-pack < shipped-pack.
        assert_eq!(packs[0].0.id, "imported-pack");
        assert!(packs[0].1, "imported pack is flagged imported");
        assert_eq!(packs[1].0.id, "shipped-pack");
        assert!(!packs[1].1, "bundled pack is not flagged imported");
    }

    #[test]
    fn merged_loader_rejects_imported_pack_shadowing_bundled_id() {
        let (bundled, user) = merge_dirs("shadow");
        std::fs::write(
            bundled.join("shipped.yaml"),
            pack_yaml("shared-id", "com.example.bundled"),
        )
        .unwrap();
        std::fs::write(
            user.join("shadow.yaml"),
            pack_yaml("shared-id", "com.example.user"),
        )
        .unwrap();

        let (packs, errors) = load_all_packs(&bundled, &user).unwrap();
        // Only the bundled pack survives; the shadowing import is an error.
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].0.id, "shared-id");
        assert!(!packs[0].1);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "pack_duplicate_id");
    }

    #[test]
    fn merged_loader_tolerates_a_missing_user_directory() {
        let (bundled, user) = merge_dirs("no-user");
        std::fs::write(
            bundled.join("shipped.yaml"),
            pack_yaml("only-bundled", "com.example.only"),
        )
        .unwrap();
        std::fs::remove_dir_all(&user).unwrap();

        let (packs, errors) = load_all_packs(&bundled, &user).unwrap();
        assert!(errors.is_empty());
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].0.id, "only-bundled");
    }

    #[test]
    fn screenshot_remote_paths_are_unique() {
        let a = unique_screenshot_remote();
        let b = unique_screenshot_remote();
        assert_ne!(a, b);
        assert!(a.starts_with("/sdcard/droidsmith-screenshot-"));
        assert!(a.ends_with(".png"));
    }

    fn fake(stdout: &str, stderr: &str, code: Option<i32>, timed_out: bool) -> ProcessOutput {
        ProcessOutput {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            code,
            timed_out,
        }
    }

    #[test]
    fn fastboot_getvar_reads_value_from_stderr() {
        // Real fastboot prints the value to stderr and exits 0 with empty
        // stdout — the exact case the old stdout-only path dropped.
        let out = fake(
            "",
            "version-bootloader: SLIDER-1.2\nfinished. total time: 0.001s\n",
            Some(0),
            false,
        );
        let value = parse_fastboot_getvar("version-bootloader", &out).unwrap();
        assert_eq!(value, "SLIDER-1.2");
    }

    #[test]
    fn fastboot_getvar_reads_value_from_stdout_fallback() {
        let out = fake("product: oriole\n", "", Some(0), false);
        assert_eq!(parse_fastboot_getvar("product", &out).unwrap(), "oriole");
    }

    #[test]
    fn fastboot_getvar_surfaces_error_with_both_streams() {
        let out = fake(
            "",
            "getvar:bogus FAILED (remote: 'unknown variable')\n",
            Some(1),
            false,
        );
        let err = parse_fastboot_getvar("bogus", &out).unwrap_err();
        assert_eq!(err.code, "fastboot_exit");
        assert!(err.message.contains("unknown variable"));
    }

    #[test]
    fn fastboot_getvar_reports_timeout() {
        let out = fake("", "", None, true);
        let err = parse_fastboot_getvar("version", &out).unwrap_err();
        assert_eq!(err.code, "fastboot_timeout");
    }

    #[test]
    fn fastboot_getvar_no_value_on_clean_but_empty() {
        let out = fake("", "finished. total time: 0.000s\n", Some(0), false);
        let err = parse_fastboot_getvar("version", &out).unwrap_err();
        assert_eq!(err.code, "fastboot_no_value");
    }

    #[test]
    fn backup_target_rejects_empty_and_relative_paths() {
        let empty = validate_backup_target("   ").unwrap_err();
        assert_eq!(empty.code, "invalid_backup_path");

        let relative = validate_backup_target("package.ab").unwrap_err();
        assert_eq!(relative.code, "invalid_backup_path");
    }

    #[test]
    fn backup_target_requires_existing_parent_and_file_target() {
        let dir = std::env::temp_dir();
        let dir_err = validate_backup_target(&dir.display().to_string()).unwrap_err();
        assert_eq!(dir_err.code, "invalid_backup_path");

        let missing_parent = dir
            .join("droidsmith-missing-backup-parent")
            .join("package.ab");
        let missing_err =
            validate_backup_target(&missing_parent.display().to_string()).unwrap_err();
        assert_eq!(missing_err.code, "invalid_backup_path");

        let valid = dir.join("package.ab");
        assert_eq!(
            validate_backup_target(&valid.display().to_string()).unwrap(),
            valid
        );
    }

    #[test]
    fn unsafe_transport_requires_explicit_acknowledgement() {
        assert_eq!(
            accepted_transport_override(DeviceTransportKind::Usb, false),
            Ok(None)
        );
        assert_eq!(
            accepted_transport_override(DeviceTransportKind::TlsWifi, false),
            Ok(None)
        );
        assert!(accepted_transport_override(DeviceTransportKind::LegacyTcp, false).is_err());
        assert_eq!(
            accepted_transport_override(DeviceTransportKind::LegacyTcp, true),
            Ok(Some(DeviceTransportKind::LegacyTcp))
        );
        assert!(accepted_transport_override(DeviceTransportKind::UnknownTcp, false).is_err());
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
