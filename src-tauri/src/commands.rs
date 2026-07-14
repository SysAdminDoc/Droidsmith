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

use serde::{Deserialize, Serialize};
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
    let mut devices = transport.list_devices()?;
    adb::observe_connection_generations(&mut devices);
    for device in devices
        .iter_mut()
        .filter(|device| device.state.is_actionable())
    {
        device.build_fingerprint = transport
            .shell_target(&device.target(), &["getprop", "ro.build.fingerprint"])
            .map(|value| value.trim().to_string())
            .ok()
            .filter(|value| !value.is_empty());
    }
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
    target: adb::DeviceTarget,
    filter: adb::PackageFilter,
    #[allow(non_snake_case)] userId: u32,
) -> Result<Vec<adb::AppPackage>, adb::TransportError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    adb::validate_device_target(&transport, &target)?;
    adb::list_packages(&transport, &target, filter, userId)
}

/// Enumerate Android users on a device so the renderer can offer an
/// explicit `--user` target for package workflows.
#[tauri::command]
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

fn execute_journaled(
    journal: &mut Journal,
    transport: &adb::ShellTransport,
    plan: actions::PlannedAction,
    undoes: Option<u64>,
) -> Result<JournalEntry, CommandError> {
    let started_at = iso_now();
    journal
        .execute(plan, undoes, &started_at, |plan| {
            actions::apply(transport, plan, &iso_now())
        })
        .map_err(|error| match error {
            journal::ExecuteError::Journal(error) => CommandError::from(error),
            journal::ExecuteError::Operation(error) => CommandError::from(error),
        })
}

fn validated_transport(target: &adb::DeviceTarget) -> Result<adb::ShellTransport, CommandError> {
    validate_serial_arg(&target.serial)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    adb::validate_device_target(&transport, target)?;
    Ok(transport)
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

/// Validate a device-side (remote) path before it reaches `adb pull`,
/// `adb push`, or `pm`. Argv-scoped calls have no shell-metachar risk,
/// but a leading `-` would be parsed by adb as an option flag, so reject
/// it — and require an absolute device path so callers can't smuggle a
/// flag or relative token across the IPC boundary.
fn validate_remote_path(remote_path: &str) -> Result<String, CommandError> {
    let trimmed = remote_path.trim();
    if trimmed.is_empty() {
        return Err(CommandError {
            code: "invalid_remote_path",
            message: "device path cannot be empty".to_string(),
        });
    }
    if trimmed.starts_with('-') {
        return Err(CommandError {
            code: "invalid_remote_path",
            message: format!("device path must not start with '-': {trimmed}"),
        });
    }
    if !trimmed.starts_with('/') {
        return Err(CommandError {
            code: "invalid_remote_path",
            message: format!("device path must be absolute: {trimmed}"),
        });
    }
    Ok(trimmed.to_string())
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
    // Serialize intent → device mutation → terminal outcome per device. The
    // durable intent is written and synced before `actions::apply` runs.
    let dir = journal_dir(&app)?;
    let entry = journal::with_journal(&dir, &serial, |journal| {
        execute_journaled(journal, &transport, plan, None)
    })?;
    Ok(entry)
}

#[tauri::command]
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
/// (uninstall, clear-data, force-stop).
#[tauri::command]
pub fn journal_undo(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    entry_id: u64,
) -> Result<JournalEntry, CommandError> {
    let serial = target.serial.clone();
    validate_serial_arg(&serial)?;
    let transport = validated_transport(&target)?;

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

        let plan = actions::plan(undo_request);
        execute_journaled(journal, &transport, plan, Some(entry_id))
    })?;
    Ok(entry)
}

#[tauri::command]
pub fn get_device_info(target: adb::DeviceTarget) -> Result<adb::DeviceInfo, CommandError> {
    let transport = validated_transport(&target)?;
    Ok(adb::get_device_info(&transport, &target)?)
}

/// Run a one-shot shell command on a device and return its output.
#[tauri::command]
pub fn shell_run(target: adb::DeviceTarget, argv: Vec<String>) -> Result<String, CommandError> {
    let transport = validated_transport(&target)?;
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    let stdout = transport.shell_target(&target, &refs)?;
    Ok(stdout)
}

/// List files in a remote directory on the device.
#[tauri::command]
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

/// Push a local file to the device.
#[tauri::command]
pub fn push_file(
    target: adb::DeviceTarget,
    local_path: String,
    remote_path: String,
) -> Result<String, CommandError> {
    let transport = validated_transport(&target)?;
    let validated_path = validate_local_path(&local_path)?;
    let remote = validate_remote_path(&remote_path)?;
    let local_arg = validated_path.display().to_string();
    let timeout = std::time::Duration::from_secs(120);
    let mut args = target.adb_selector();
    args.extend(["push".to_string(), local_arg, remote]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_adb_simple(&transport.adb_path, &refs, timeout)?;
    Ok(output)
}

/// Pull a remote file from the device.
#[tauri::command]
pub fn pull_file(
    target: adb::DeviceTarget,
    remote_path: String,
    local_path: String,
) -> Result<String, CommandError> {
    let transport = validated_transport(&target)?;
    let validated_path = validate_local_path(&local_path)?;
    let remote = validate_remote_path(&remote_path)?;
    let local_arg = validated_path.display().to_string();
    let timeout = std::time::Duration::from_secs(120);
    let mut args = target.adb_selector();
    args.extend(["pull".to_string(), remote, local_arg]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_adb_simple(&transport.adb_path, &refs, timeout)?;
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
    use std::io::Read as IoRead;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CommandError {
            code: "spawn_failed",
            message: format!("failed to run {}: {e}", program.display()),
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
    let mut timed_out = false;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => break None,
        }
    };

    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    Ok(ProcessOutput {
        stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
        code: exit_status.and_then(|s| s.code()),
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
pub fn list_network_connections(
    target: adb::DeviceTarget,
) -> Result<Vec<NetworkConnection>, CommandError> {
    let transport = validated_transport(&target)?;
    let stdout = transport
        .shell_target(&target, &["ss", "-tunp"])
        .or_else(|_| transport.shell_target(&target, &["netstat", "-tunp"]))?;
    Ok(parse_ss_output(&stdout))
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupPackageResult {
    pub local_path: String,
    pub stdout: String,
    pub size_bytes: Option<u64>,
    pub empty: bool,
    /// True when the artifact is non-empty but only large enough to hold
    /// the `.ab` header — i.e. `adb backup` excluded the app's data
    /// (targetSDK 31+/Android 12 deprecation). The UI warns on this.
    pub header_only: bool,
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
    target: adb::DeviceTarget,
    package: String,
    local_path: String,
) -> Result<BackupPackageResult, CommandError> {
    let transport = validated_transport(&target)?;
    validate_package_arg(&package)?;
    let output_target = validate_backup_target(&local_path)?;
    let timeout = std::time::Duration::from_secs(300);
    let target_arg = output_target.display().to_string();
    let mut args = target.adb_selector();
    args.extend([
        "backup".to_string(),
        "-f".to_string(),
        target_arg,
        "-apk".to_string(),
        package,
    ]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_adb_simple(&transport.adb_path, &refs, timeout)?;
    let size_bytes = std::fs::metadata(&output_target)
        .ok()
        .map(|metadata| metadata.len());
    let (empty, header_only) = classify_backup_size(size_bytes);

    Ok(BackupPackageResult {
        local_path: output_target.display().to_string(),
        stdout: output,
        size_bytes,
        empty,
        header_only,
    })
}

/// A `.ab` archive of an app that targets SDK 31+ (Android 12) contains
/// only the ~24-byte "ANDROID BACKUP" header with no payload, because
/// `adb backup` is deprecated and excludes such apps' private data. Treat
/// any non-zero artifact at or below this size as header-only so the UI
/// warns instead of claiming a real backup.
const BACKUP_HEADER_ONLY_MAX_BYTES: u64 = 512;

/// Returns `(empty, header_only)` for a produced backup artifact size.
fn classify_backup_size(size_bytes: Option<u64>) -> (bool, bool) {
    match size_bytes {
        None | Some(0) => (true, false),
        Some(size) if size <= BACKUP_HEADER_ONLY_MAX_BYTES => (false, true),
        Some(_) => (false, false),
    }
}

/// List runtime permissions for a package.
#[tauri::command]
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
pub fn set_permission(
    target: adb::DeviceTarget,
    package: String,
    permission: String,
    grant: bool,
) -> Result<String, CommandError> {
    let transport = validated_transport(&target)?;
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
    let action = if grant { "grant" } else { "revoke" };
    let stdout = transport.shell_target(&target, &["pm", action, &package, &permission])?;
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
pub fn list_processes(target: adb::DeviceTarget) -> Result<Vec<ProcessInfo>, CommandError> {
    let transport = validated_transport(&target)?;
    let stdout = transport.shell_target(&target, &["ps", "-A", "-o", "PID,USER,VSZ,RSS,NAME"])?;
    Ok(parse_ps_output(&stdout))
}

/// Take a screenshot on the device and pull it to a local path.
#[tauri::command]
pub fn take_screenshot(
    target: adb::DeviceTarget,
    local_path: String,
) -> Result<String, CommandError> {
    let transport = validated_transport(&target)?;
    let validated_path = validate_local_path(&local_path)?;
    // Unique device-side temp so concurrent captures (multiple devices or
    // rapid clicks) never clobber each other's PNG mid-pull.
    let remote = unique_screenshot_remote();
    let local_arg = validated_path.display().to_string();
    transport.shell_target(&target, &["screencap", "-p", &remote])?;
    let pulled = actions::extract_apk(&transport.adb_path, &target, &remote, &local_arg);
    // Always remove the device temp, even when the pull failed, so a
    // partial capture never leaks onto /sdcard.
    let _ = transport.shell_target(&target, &["rm", "-f", &remote]);
    pulled?;
    Ok(local_arg)
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
    if request.target.serial != request.serial {
        return Err(CommandError {
            code: "target_mismatch",
            message: "scrcpy target does not match the requested serial".to_string(),
        });
    }
    let transport = validated_transport(&request.target)?;
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
pub fn install_apk(target: adb::DeviceTarget, apk_path: String) -> Result<String, CommandError> {
    let transport = validated_transport(&target)?;
    let validated_path = validate_local_path(&apk_path)?;
    let apk_arg = validated_path.display().to_string();
    let stdout = actions::install_apk(&transport.adb_path, &target, &apk_arg)?;
    Ok(stdout)
}

/// Pull an APK from the device to a local path.
#[tauri::command]
pub fn extract_apk(
    target: adb::DeviceTarget,
    remote_path: String,
    local_path: String,
) -> Result<String, CommandError> {
    let transport = validated_transport(&target)?;
    let validated_path = validate_local_path(&local_path)?;
    let remote = validate_remote_path(&remote_path)?;
    let local_arg = validated_path.display().to_string();
    let stdout = actions::extract_apk(&transport.adb_path, &target, &remote, &local_arg)?;
    Ok(stdout)
}

/// List all debloat packs from the app's `packs/` resource directory.
/// A bundled pack file that failed to load, with a stable code and a
/// human-readable message the UI can show and the user can copy.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Serialize)]
pub struct PackListing {
    pub packs: Vec<crate::packs::PackCandidate>,
    pub errors: Vec<PackLoadError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlanPackRequest {
    pub target: adb::DeviceTarget,
    pub user_id: u32,
    pub pack_id: String,
    pub revision: u32,
    pub selected: Vec<String>,
    #[serde(default)]
    pub override_compatibility: bool,
}

#[derive(Debug, Clone, Serialize)]
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
pub fn list_packs(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    #[allow(non_snake_case)] userId: u32,
) -> Result<PackListing, CommandError> {
    let resource_dir = app.path().resource_dir().map_err(|e| CommandError {
        code: "no_resource_dir",
        message: e.to_string(),
    })?;
    let packs_dir = resource_dir.join("packs");

    let (packs, errors) = load_runtime_packs(&packs_dir)?;
    let transport = validated_transport(&target)?;
    let context = pack_context(&transport, &target, userId)?;
    let packs = packs
        .into_iter()
        .map(|pack| crate::packs::PackCandidate {
            assessment: crate::packs::assess(&pack, &context),
            pack,
        })
        .collect();
    Ok(PackListing { packs, errors })
}

#[tauri::command]
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
    let (packs, _) = load_runtime_packs(&resource_dir.join("packs"))?;
    let pack = packs
        .into_iter()
        .find(|pack| pack.id == request.pack_id)
        .ok_or(CommandError {
            code: "pack_not_found",
            message: format!("debloat pack {:?} is not bundled", request.pack_id),
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
        classify_backup_size, load_runtime_packs, pack_error_to_load_error, parse_fastboot_getvar,
        unique_screenshot_remote, validate_backup_target, validate_local_path,
        validate_remote_path, ProcessOutput,
    };

    #[test]
    fn remote_path_rejects_flags_and_relative() {
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
            validate_remote_path("/sdcard/Download/app.apk").unwrap(),
            "/sdcard/Download/app.apk"
        );
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

    #[test]
    fn backup_size_classification() {
        // Missing or zero → empty.
        assert_eq!(classify_backup_size(None), (true, false));
        assert_eq!(classify_backup_size(Some(0)), (true, false));
        // Bare .ab header (targetSDK 31+ apps) → header-only warning.
        assert_eq!(classify_backup_size(Some(24)), (false, true));
        assert_eq!(classify_backup_size(Some(512)), (false, true));
        // Real payload → a genuine backup.
        assert_eq!(classify_backup_size(Some(513)), (false, false));
        assert_eq!(classify_backup_size(Some(4096)), (false, false));
    }

    #[test]
    fn local_path_requires_absolute() {
        assert_eq!(validate_local_path("   ").unwrap_err().code, "invalid_path");
        // Relative paths (the old screenshot/pull bug) are rejected.
        assert_eq!(
            validate_local_path("screenshot-abc.png").unwrap_err().code,
            "invalid_path"
        );
        assert_eq!(
            validate_local_path("./sub/dir/file.png").unwrap_err().code,
            "invalid_path"
        );

        // An absolute path for the current platform is accepted.
        let abs = if cfg!(windows) {
            "C:\\Users\\qa\\shot.png"
        } else {
            "/home/qa/shot.png"
        };
        assert!(validate_local_path(abs).is_ok());
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
