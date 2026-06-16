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

use crate::adb::device::valid_serial;
use crate::adb::{self, actions};
use crate::journal::{self, Journal, JournalEntry};
use crate::quirks::{self, DeviceContext, Quirk};

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

#[derive(Serialize)]
pub struct ListWirelessServicesResult {
    pub adb_resolved: bool,
    pub adb_path: Option<String>,
    pub services: Vec<adb::WirelessAdbService>,
}

#[tauri::command]
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
pub fn pair_wireless(
    request: adb::WirelessPairRequest,
) -> Result<adb::WirelessCommandResult, CommandError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    Ok(adb::pair_wireless(&transport, &request)?)
}

#[tauri::command]
pub fn connect_wireless(
    request: adb::WirelessConnectRequest,
) -> Result<adb::WirelessCommandResult, CommandError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    Ok(adb::connect_wireless(&transport, &request)?)
}

#[tauri::command]
pub fn list_packages(
    serial: String,
    filter: adb::PackageFilter,
) -> Result<Vec<adb::AppPackage>, adb::TransportError> {
    if !valid_serial(&serial) {
        return Err(adb::TransportError::Parse(format!(
            "invalid device serial {serial:?}"
        )));
    }
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
    validate_serial_arg(&serial)?;
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
    validate_serial_arg(&serial)?;
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

#[tauri::command]
pub fn get_device_info(serial: String) -> Result<adb::DeviceInfo, CommandError> {
    validate_serial_arg(&serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    Ok(adb::get_device_info(&transport, &serial)?)
}

/// Run a one-shot shell command on a device and return its output.
#[tauri::command]
pub fn shell_run(serial: String, argv: Vec<String>) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    let stdout = transport.shell(&serial, &refs)?;
    Ok(stdout)
}

/// Get process list from a device. Uses `ps -A -o PID,USER,VSZ,RSS,%CPU,NAME`
/// for a structured snapshot.
#[tauri::command]
pub fn list_processes(serial: String) -> Result<Vec<ProcessInfo>, CommandError> {
    validate_serial_arg(&serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    let stdout = transport.shell(&serial, &["ps", "-A", "-o", "PID,USER,VSZ,RSS,NAME"])?;
    Ok(parse_ps_output(&stdout))
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub user: String,
    pub vsz_kb: u64,
    pub rss_kb: u64,
    pub name: String,
}

fn parse_ps_output(stdout: &str) -> Vec<ProcessInfo> {
    let mut out = Vec::new();
    for line in stdout.lines().skip(1) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 5 {
            continue;
        }
        let Some(pid) = tokens[0].parse::<u32>().ok() else {
            continue;
        };
        let user = tokens[1].to_string();
        let vsz_kb = tokens[2].parse::<u64>().unwrap_or(0);
        let rss_kb = tokens[3].parse::<u64>().unwrap_or(0);
        let name = tokens[4..].join(" ");
        out.push(ProcessInfo {
            pid,
            user,
            vsz_kb,
            rss_kb,
            name,
        });
    }
    out
}

/// Take a screenshot on the device and pull it to a local path.
#[tauri::command]
pub fn take_screenshot(
    serial: String,
    local_path: String,
) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);

    let remote = "/sdcard/droidsmith-screenshot.png";
    transport.shell(&serial, &["screencap", "-p", remote])?;
    actions::extract_apk(std::path::Path::new(path), &serial, remote, &local_path)?;
    let _ = transport.shell(&serial, &["rm", remote]);
    Ok(local_path)
}

/// Locate the scrcpy binary on the system. Returns the path if found.
#[tauri::command]
pub fn locate_scrcpy() -> Option<String> {
    which::which("scrcpy")
        .ok()
        .map(|p| p.display().to_string())
}

/// Launch scrcpy for a device. Fire-and-forget: we spawn the process
/// detached so it outlives the IPC call. Returns the PID.
#[tauri::command]
pub fn launch_scrcpy(
    serial: String,
    max_size: Option<u32>,
    bit_rate: Option<String>,
    no_audio: bool,
    record_path: Option<String>,
) -> Result<u32, CommandError> {
    validate_serial_arg(&serial)?;
    let scrcpy_path = which::which("scrcpy").map_err(|_| CommandError {
        code: "scrcpy_not_found",
        message: "scrcpy binary not found on PATH".to_string(),
    })?;

    let mut cmd = std::process::Command::new(scrcpy_path);
    cmd.arg("-s").arg(&serial);
    if let Some(ms) = max_size {
        cmd.arg("--max-size").arg(ms.to_string());
    }
    if let Some(br) = &bit_rate {
        cmd.arg("--video-bit-rate").arg(br);
    }
    if no_audio {
        cmd.arg("--no-audio");
    }
    if let Some(rp) = &record_path {
        cmd.arg("--record").arg(rp);
    }

    let child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| CommandError {
            code: "scrcpy_spawn_failed",
            message: format!("failed to launch scrcpy: {e}"),
        })?;

    Ok(child.id())
}

/// Install an APK file on a device. The `apk_path` is a local filesystem
/// path to the `.apk` file. Uses `adb install -r` for replace-on-conflict.
#[tauri::command]
pub fn install_apk(serial: String, apk_path: String) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let stdout = actions::install_apk(std::path::Path::new(path), &serial, &apk_path)?;
    Ok(stdout)
}

/// Pull an APK from the device to a local path.
#[tauri::command]
pub fn extract_apk(
    serial: String,
    remote_path: String,
    local_path: String,
) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let stdout =
        actions::extract_apk(std::path::Path::new(path), &serial, &remote_path, &local_path)?;
    Ok(stdout)
}

/// List all debloat packs from the app's `packs/` resource directory.
/// Returns packs that parse and lint cleanly; silently skips broken files.
#[tauri::command]
pub fn list_packs(app: tauri::AppHandle) -> Result<Vec<crate::packs::Pack>, CommandError> {
    let resource_dir = app.path().resource_dir().map_err(|e| CommandError {
        code: "no_resource_dir",
        message: e.to_string(),
    })?;
    let packs_dir = resource_dir.join("packs");

    if !packs_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut packs = Vec::new();
    let entries = std::fs::read_dir(&packs_dir).map_err(|e| CommandError {
        code: "io_error",
        message: format!("could not read packs directory: {e}"),
    })?;

    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "yaml" || ext == "yml") {
            if let Ok(pack) = crate::packs::load(&path) {
                packs.push(pack);
            }
        }
    }

    packs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packs)
}

fn iso_now() -> String {
    crate::time::iso_utc_now()
}

/// Request shape for [`explain_failure`].
#[derive(Debug, serde::Deserialize)]
pub struct ExplainFailureRequest {
    pub manufacturer: Option<String>,
    pub rom: Option<String>,
    pub package_id: Option<String>,
    pub raw_error: Option<String>,
    /// Optional path to a `quirks/*.yaml` file. If `None`, we look in
    /// the app-resource folder for the bundled quirks.
    pub quirks_path: Option<String>,
}

/// Load quirks from the given YAML file (or the bundled default once
/// resource shipping lands) and match against the failure context.
/// Returns `Some(quirk)` if a rule applies, `None` if the raw error
/// should be shown as-is.
#[tauri::command]
pub fn explain_failure(req: ExplainFailureRequest) -> Result<Option<Quirk>, CommandError> {
    // Until R-006 ships bundled resources, the caller passes an
    // explicit path. We refuse to silently load nothing — that would
    // make false-negatives look like "no quirk applies".
    let path = req.quirks_path.ok_or(CommandError {
        code: "quirks_path_required",
        message: "explain_failure currently requires an explicit quirks_path; bundled \
                  resources land with R-006."
            .to_string(),
    })?;

    let quirks_list = quirks::load_file(std::path::Path::new(&path)).map_err(|e| CommandError {
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
