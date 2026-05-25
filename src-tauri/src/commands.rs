//! Tauri `#[command]` glue.
//!
//! Keep this file *thin* — types live in the relevant domain modules,
//! and IO work is delegated. Conventions:
//!
//! - Every `#[tauri::command]` takes either no args, or `tauri::AppHandle`
//!   plus serializable args.
//! - Return types are `Serialize` and live in a domain module.
//! - No business logic inline — it goes to `adb`, `diagnostics`, etc.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Serialize;
use tauri::Manager;

use crate::adb::{self, actions};
use crate::journal::{self, Journal, JournalEntry};

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

/// Outcome envelope for `list_devices`. We surface adb-not-found as a
/// structured success-with-zero-devices + an `adb_resolved=false` flag
/// rather than an Err, because "no adb installed" is a normal first-run
/// state, not a runtime fault.
#[derive(Serialize)]
pub struct ListDevicesResult {
    pub adb_resolved: bool,
    pub adb_path: Option<String>,
    pub devices: Vec<adb::Device>,
}

#[tauri::command]
pub fn list_devices() -> Result<ListDevicesResult, adb::TransportError> {
    let resolution = adb::locate_adb();
    let Some(path) = resolution.path.as_ref() else {
        return Ok(ListDevicesResult {
            adb_resolved: false,
            adb_path: None,
            devices: Vec::new(),
        });
    };

    use adb::AdbTransport;
    let transport = adb::ShellTransport::new(path);
    let devices = transport.list_devices()?;
    Ok(ListDevicesResult {
        adb_resolved: true,
        adb_path: Some(path.clone()),
        devices,
    })
}

#[tauri::command]
pub fn list_packages(
    serial: String,
    filter: adb::PackageFilter,
) -> Result<Vec<adb::AppPackage>, adb::TransportError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    adb::list_packages(&transport, &serial, filter)
}

/// Synthesise an ADB action without running it. Pure: this is the
/// preview surface the confirmation dialog renders before the user
/// commits.
#[tauri::command]
pub fn plan_action(request: actions::ActionRequest) -> actions::PlannedAction {
    actions::plan(request)
}

/// Generic Tauri-command error envelope so the JS side gets the same
/// shape regardless of whether the underlying failure was a transport
/// error or a filesystem error from the journal.
#[derive(Debug, Serialize, thiserror::Error)]
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

fn journal_dir(app: &tauri::AppHandle) -> Result<PathBuf, CommandError> {
    let base = app.path().app_data_dir().map_err(|e| CommandError {
        code: "no_app_data_dir",
        message: e.to_string(),
    })?;
    Ok(base.join("journal"))
}

/// Apply a previously-planned action and record it in the per-device
/// journal. Returns the freshly-written journal entry.
#[tauri::command]
pub fn apply_action(
    app: tauri::AppHandle,
    plan: actions::PlannedAction,
) -> Result<JournalEntry, CommandError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);

    let serial = plan.request.serial.clone();
    let applied = actions::apply(&transport, plan, &iso_now())?;

    let mut journal = Journal::open(&journal_dir(&app)?, &serial)?;
    let entry = journal.record(applied)?.clone();
    Ok(entry)
}

#[tauri::command]
pub fn journal_list(
    app: tauri::AppHandle,
    serial: String,
) -> Result<Vec<JournalEntry>, CommandError> {
    let journal = Journal::open(&journal_dir(&app)?, &serial)?;
    Ok(journal.entries().to_vec())
}

/// Undo entry `entry_id` in `serial`'s journal. Returns the new
/// undo-entry. Fails if the original action is irreversible
/// (uninstall, clear-data, force-stop).
#[tauri::command]
pub fn journal_undo(
    app: tauri::AppHandle,
    serial: String,
    entry_id: u64,
) -> Result<JournalEntry, CommandError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);

    let mut journal = Journal::open(&journal_dir(&app)?, &serial)?;
    let undo_request = journal::undo_request_for(&journal, entry_id).ok_or(CommandError {
        code: "not_reversible",
        message: format!(
            "journal entry {entry_id} either doesn't exist, is already undone, or its action kind cannot be reversed"
        ),
    })?;

    let plan = actions::plan(undo_request);
    let applied = actions::apply(&transport, plan, &iso_now())?;
    let entry = journal.record_undo(entry_id, applied)?.clone();
    Ok(entry)
}

/// Hand-rolled RFC3339 UTC stamp matching the one in `diagnostics`.
/// Centralised here for the journal; the diagnostics module's copy is
/// independent so the two layers don't reach into each other.
fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    crate::diagnostics::format_utc_rfc3339_for_journal(secs)
}
