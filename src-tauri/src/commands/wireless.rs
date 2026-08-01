//! Domain-scoped Tauri command boundary.

use super::*;

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

/// CVE-2026-0073 advisory for every connected, authorized device.
///
/// Enumerating in Rust keeps this to one round trip and avoids fanning
/// per-device target-bound calls out of the renderer, where a device that
/// disappears mid-sweep would produce a stale completion. A device that cannot
/// be read is simply omitted; the renderer treats absence as "no verdict".
#[tauri::command]
#[specta::specta]
pub fn list_wireless_debugging_risks() -> Result<Vec<adb::WirelessDeviceRisk>, adb::TransportError>
{
    let resolution = adb::locate_adb();
    let Some(path) = resolution.path.as_ref() else {
        return Ok(Vec::new());
    };
    let transport = adb::ShellTransport::new(path);
    let mut risks = Vec::new();
    for device in transport.list_devices()? {
        if device.state != adb::DeviceState::Device {
            continue;
        }
        if let Ok(risk) = adb::get_wireless_debugging_risk(&transport, &device.target()) {
            risks.push(risk);
        }
    }
    Ok(risks)
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
