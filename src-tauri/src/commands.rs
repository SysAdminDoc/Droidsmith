//! Tauri `#[command]` glue.
//!
//! Keep this file *thin* — types live in the relevant domain modules,
//! and IO work is delegated. The shape rules so future contributors
//! don't grow this file into a god-module:
//!
//! - Every `#[tauri::command]` takes either no args, or `tauri::AppHandle`
//!   plus serializable args.
//! - Return types are `Serialize` and live in a domain module.
//! - No business logic inline — it goes to `adb`, `diagnostics`, etc.

use std::sync::OnceLock;

use serde::Serialize;
use tauri::Manager;

use crate::adb;

#[derive(Serialize)]
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

#[derive(Serialize, Clone)]
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
