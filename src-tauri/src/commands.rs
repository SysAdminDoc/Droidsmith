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
use crate::adb::packages::valid_package_name;
use crate::adb::parsers::{
    parse_fastboot_devices, parse_ls_output, parse_ps_output, parse_ss_output, FastbootDevice,
    NetworkConnection, ProcessInfo, RemoteFileEntry,
};
use crate::adb::transport::AdbTransport;
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

fn validate_local_path(local_path: &str) -> Result<PathBuf, CommandError> {
    let trimmed = local_path.trim();
    if trimmed.is_empty() {
        return Err(CommandError {
            code: "invalid_path",
            message: "file path cannot be empty".to_string(),
        });
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(CommandError {
            code: "invalid_path",
            message: format!("file path must be absolute: {trimmed}"),
        });
    }
    Ok(path)
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

/// List files in a remote directory on the device.
#[tauri::command]
pub fn list_remote_files(
    serial: String,
    remote_path: String,
) -> Result<RemoteListing, CommandError> {
    validate_serial_arg(&serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    let stdout = transport.shell(&serial, &["ls", "-la", &remote_path])?;
    let entries = parse_ls_output(&stdout);
    let free_space = transport
        .shell(&serial, &["df", &remote_path])
        .ok()
        .and_then(|s| parse_df_free(&s));
    Ok(RemoteListing {
        path: remote_path,
        entries,
        free_space_kb: free_space,
    })
}

/// Push a local file to the device.
#[tauri::command]
pub fn push_file(
    serial: String,
    local_path: String,
    remote_path: String,
) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    let validated_path = validate_local_path(&local_path)?;
    let resolution = adb::locate_adb();
    let adb_path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;

    let local_arg = validated_path.display().to_string();
    let timeout = std::time::Duration::from_secs(120);
    let output = run_adb_simple(
        std::path::Path::new(adb_path),
        &["-s", &serial, "push", &local_arg, &remote_path],
        timeout,
    )?;
    Ok(output)
}

/// Pull a remote file from the device.
#[tauri::command]
pub fn pull_file(
    serial: String,
    remote_path: String,
    local_path: String,
) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    let validated_path = validate_local_path(&local_path)?;
    let resolution = adb::locate_adb();
    let adb_path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;

    let local_arg = validated_path.display().to_string();
    let timeout = std::time::Duration::from_secs(120);
    let output = run_adb_simple(
        std::path::Path::new(adb_path),
        &["-s", &serial, "pull", &remote_path, &local_arg],
        timeout,
    )?;
    Ok(output)
}

#[derive(Debug, Clone, Serialize)]
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

fn run_adb_simple(
    adb_path: &std::path::Path,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<String, CommandError> {
    use std::io::Read as IoRead;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new(adb_path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CommandError {
            code: "spawn_failed",
            message: format!("failed to run adb: {e}"),
        })?;

    let mut stdout_pipe = child.stdout.take().unwrap();
    let mut stderr_pipe = child.stderr.take().unwrap();

    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::with_capacity(4096);
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::with_capacity(1024);
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => break None,
        }
    };

    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();

    match exit_status {
        None => Err(CommandError {
            code: "adb_timeout",
            message: format!("adb timed out after {timeout:?}"),
        }),
        Some(status) if status.success() => Ok(stdout),
        Some(status) => Err(CommandError {
            code: "adb_exit",
            message: format!(
                "adb exited with code {}: {}",
                status.code().unwrap_or(-1),
                stderr
            ),
        }),
    }
}

/// Locate the fastboot binary on the system.
#[tauri::command]
pub fn locate_fastboot() -> Option<String> {
    which::which("fastboot")
        .ok()
        .map(|p| p.display().to_string())
}

/// List devices visible to fastboot.
#[tauri::command]
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
pub fn fastboot_getvar(serial: String, key: String) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    validate_fastboot_key(&key)?;
    let fastboot_path = which::which("fastboot").map_err(|_| CommandError {
        code: "fastboot_not_found",
        message: "fastboot binary not found on PATH".to_string(),
    })?;

    let timeout = std::time::Duration::from_secs(10);
    let stdout = run_adb_simple(&fastboot_path, &["-s", &serial, "getvar", &key], timeout)
        .or_else(|_| {
            // fastboot getvar outputs to stderr — retry capturing stderr
            // via the same timeout-guarded helper
            let stderr_attempt =
                run_adb_simple(&fastboot_path, &["-s", &serial, "getvar", &key], timeout);
            match stderr_attempt {
                Ok(output) => Ok(output),
                Err(_) => Err(CommandError {
                    code: "fastboot_timeout",
                    message: format!("fastboot getvar {key:?} timed out"),
                }),
            }
        })?;
    Ok(stdout)
}

/// Get network connections from the device using `ss -tunp`.
#[tauri::command]
pub fn list_network_connections(serial: String) -> Result<Vec<NetworkConnection>, CommandError> {
    validate_serial_arg(&serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    let stdout = transport
        .shell(&serial, &["ss", "-tunp"])
        .or_else(|_| transport.shell(&serial, &["netstat", "-tunp"]))?;
    Ok(parse_ss_output(&stdout))
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupPackageResult {
    pub local_path: String,
    pub stdout: String,
    pub size_bytes: Option<u64>,
    pub empty: bool,
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

/// Backup a package's data using `adb backup`. The user must confirm
/// on the device screen. Returns the adb output and local artifact metadata.
#[tauri::command]
pub fn backup_package(
    serial: String,
    package: String,
    local_path: String,
) -> Result<BackupPackageResult, CommandError> {
    validate_serial_arg(&serial)?;
    validate_package_arg(&package)?;
    let target = validate_backup_target(&local_path)?;
    let resolution = adb::locate_adb();
    let adb_path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;

    let timeout = std::time::Duration::from_secs(300);
    let target_arg = target.display().to_string();
    let output = run_adb_simple(
        std::path::Path::new(adb_path),
        &["-s", &serial, "backup", "-f", &target_arg, "-apk", &package],
        timeout,
    )?;
    let size_bytes = std::fs::metadata(&target)
        .ok()
        .map(|metadata| metadata.len());
    let empty = size_bytes.map_or(true, |size| size == 0);

    Ok(BackupPackageResult {
        local_path: target.display().to_string(),
        stdout: output,
        size_bytes,
        empty,
    })
}

/// List runtime permissions for a package.
#[tauri::command]
pub fn list_permissions(
    serial: String,
    package: String,
) -> Result<Vec<PermissionInfo>, CommandError> {
    validate_serial_arg(&serial)?;
    validate_package_arg(&package)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    let stdout = transport.shell(&serial, &["dumpsys", "package", &package])?;
    Ok(parse_permissions(&stdout))
}

/// Grant or revoke a runtime permission.
#[tauri::command]
pub fn set_permission(
    serial: String,
    package: String,
    permission: String,
    grant: bool,
) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    validate_package_arg(&package)?;
    if permission.is_empty()
        || !permission
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_'))
    {
        return Err(CommandError {
            code: "invalid_permission",
            message: format!("invalid permission identifier {permission:?}"),
        });
    }
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    let action = if grant { "grant" } else { "revoke" };
    let stdout = transport.shell(&serial, &["pm", action, &package, &permission])?;
    Ok(stdout)
}

#[derive(Debug, Clone, Serialize)]
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

/// Take a screenshot on the device and pull it to a local path.
#[tauri::command]
pub fn take_screenshot(serial: String, local_path: String) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    let validated_path = validate_local_path(&local_path)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);

    let remote = "/sdcard/droidsmith-screenshot.png";
    let local_arg = validated_path.display().to_string();
    transport.shell(&serial, &["screencap", "-p", remote])?;
    actions::extract_apk(std::path::Path::new(path), &serial, remote, &local_arg)?;
    let _ = transport.shell(&serial, &["rm", remote]);
    Ok(local_arg)
}

/// Locate the scrcpy binary on the system. Returns the path if found.
#[tauri::command]
pub fn locate_scrcpy() -> Option<String> {
    which::which("scrcpy").ok().map(|p| p.display().to_string())
}

/// Launch scrcpy for a device. Fire-and-forget: we spawn the process
/// and track it so the renderer can poll or stop the session.
#[tauri::command]
pub fn launch_scrcpy(
    request: crate::scrcpy::LaunchScrcpyRequest,
) -> Result<crate::scrcpy::ScrcpySession, CommandError> {
    validate_serial_arg(&request.serial)?;
    let scrcpy_path = which::which("scrcpy").map_err(|_| CommandError {
        code: "scrcpy_not_found",
        message: "scrcpy binary not found on PATH".to_string(),
    })?;
    crate::scrcpy::launch(&scrcpy_path, request, iso_now()).map_err(|e| CommandError {
        code: "scrcpy_spawn_failed",
        message: e,
    })
}

#[tauri::command]
pub fn scrcpy_session_status(
    session_id: u64,
) -> Result<crate::scrcpy::ScrcpySession, CommandError> {
    crate::scrcpy::status(session_id).map_err(|e| CommandError {
        code: "scrcpy_session_not_found",
        message: e,
    })
}

#[tauri::command]
pub fn stop_scrcpy(session_id: u64) -> Result<crate::scrcpy::ScrcpySession, CommandError> {
    crate::scrcpy::stop(session_id).map_err(|e| CommandError {
        code: "scrcpy_stop_failed",
        message: e,
    })
}

/// Install an APK file on a device. The `apk_path` is a local filesystem
/// path to the `.apk` file. Uses `adb install -r` for replace-on-conflict.
#[tauri::command]
pub fn install_apk(serial: String, apk_path: String) -> Result<String, CommandError> {
    validate_serial_arg(&serial)?;
    let validated_path = validate_local_path(&apk_path)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let apk_arg = validated_path.display().to_string();
    let stdout = actions::install_apk(std::path::Path::new(path), &serial, &apk_arg)?;
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
    let validated_path = validate_local_path(&local_path)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let local_arg = validated_path.display().to_string();
    let stdout = actions::extract_apk(
        std::path::Path::new(path),
        &serial,
        &remote_path,
        &local_arg,
    )?;
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
        if path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml")
        {
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

#[cfg(test)]
mod tests {
    use super::validate_backup_target;

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
}

/// Request shape for [`explain_failure`].
#[derive(Debug, serde::Deserialize)]
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
