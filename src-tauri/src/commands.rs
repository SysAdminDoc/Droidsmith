use serde::Serialize;

use crate::adb;

#[derive(Serialize)]
pub struct Heartbeat {
    pub version: String,
    pub adb_resolved: Option<String>,
}

#[tauri::command]
pub fn heartbeat() -> Heartbeat {
    Heartbeat {
        version: env!("CARGO_PKG_VERSION").to_string(),
        adb_resolved: adb::locate_adb().map(|p| p.display().to_string()),
    }
}
