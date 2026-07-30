//! Domain-scoped Tauri command boundary.

use super::*;

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

pub(crate) fn collect_devices(
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

pub(crate) fn host_operation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn append_host_operation(
    path: &Path,
    record: &AdbRecoveryRecord,
) -> Result<(), CommandError> {
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

pub(crate) fn diagnostic_text(value: &str) -> String {
    let normalized = value
        .chars()
        .filter(|character| *character == '\n' || !character.is_control())
        .collect::<String>();
    normalized.trim().chars().take(1_024).collect()
}
