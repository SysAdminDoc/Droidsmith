//! Domain-scoped Tauri command boundary.

use super::*;

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
pub async fn launch_scrcpy(
    request: crate::scrcpy::LaunchScrcpyRequest,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: Option<String>,
    retry_session_id: Option<u64>,
) -> Result<crate::scrcpy::ScrcpySession, CommandError> {
    validate_serial_arg(&request.serial)?;
    if request.target.serial != request.serial {
        return Err(CommandError {
            code: "target_mismatch",
            message: "scrcpy target does not match the requested serial".to_string(),
        });
    }
    // The adb list_devices round-trip and the capability probe (up to ~23 s on
    // a cache miss) must not run on the IPC dispatch thread; mirror the
    // sibling scrcpy_capabilities command.
    let (scrcpy_path, capabilities, request) = spawn_blocking_operation(move || {
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
        Ok((scrcpy_path, capabilities, request))
    })
    .await?;
    // Grant consumption stays on the command task (managed State cannot move
    // into the 'static blocking closure) and still happens only after a
    // successful probe, so a failed probe does not burn the one-shot record
    // grant. The remaining launch is a fast local process spawn.
    let record_path = path_grant
        .as_deref()
        .map(|grant| grants.consume(grant, HostPathPurpose::ScrcpyRecordSave))
        .transpose()?;
    crate::scrcpy::launch(
        &scrcpy_path,
        request,
        record_path.as_deref(),
        retry_session_id,
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
    grants: tauri::State<'_, PathGrantStore>,
    session_id: u64,
) -> Result<crate::scrcpy::ScrcpySession, CommandError> {
    let session = crate::scrcpy::status(session_id).map_err(|e| CommandError {
        code: "scrcpy_session_not_found",
        message: e,
    })?;
    if let Some(path) =
        crate::scrcpy::finished_recording(session_id).map_err(|message| CommandError {
            code: "scrcpy_session_not_found",
            message,
        })?
    {
        grants.record_produced(&path)?;
    }
    Ok(session)
}

#[tauri::command]
#[specta::specta]
pub fn stop_scrcpy(
    grants: tauri::State<'_, PathGrantStore>,
    session_id: u64,
) -> Result<crate::scrcpy::ScrcpySession, CommandError> {
    let session = crate::scrcpy::stop(session_id).map_err(|e| CommandError {
        code: "scrcpy_stop_failed",
        message: e,
    })?;
    if let Some(path) =
        crate::scrcpy::finished_recording(session_id).map_err(|message| CommandError {
            code: "scrcpy_stop_failed",
            message,
        })?
    {
        grants.record_produced(&path)?;
    }
    Ok(session)
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
pub async fn start_gnirehtet(
    target: adb::DeviceTarget,
) -> Result<crate::gnirehtet::GnirehtetSession, CommandError> {
    validate_serial_arg(&target.serial)?;
    // The adb list_devices round-trip must not run on the IPC dispatch
    // thread; mirror the sibling scrcpy_capabilities command.
    spawn_blocking_operation(move || {
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
    })
    .await
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
