//! Domain-scoped Tauri command boundary.

use super::*;

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
pub(crate) fn parse_fastboot_getvar(
    key: &str,
    out: &ProcessOutput,
) -> Result<String, CommandError> {
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
pub(crate) fn getvar_value(key: &str, text: &str) -> Option<String> {
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

pub(crate) fn first_nonempty<'a>(a: &'a str, b: &'a str) -> &'a str {
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

pub(crate) fn validate_backup_target(local_path: &str) -> Result<PathBuf, CommandError> {
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
    let result = spawn_blocking_operation(move || {
        Ok(backup::export_apks(
            &adb_path,
            &target,
            &output_target,
            preflight,
            &operation_id,
            sink,
        )?)
    })
    .await?;
    grants.record_produced(&result.artifact.local_path)?;
    Ok(result)
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
    let result = spawn_blocking_operation(move || {
        Ok(bugreport::capture(
            &adb_path,
            &target,
            &destination,
            platform_tools_version,
            &operation_id,
            sink,
        )?)
    })
    .await?;
    grants.record_produced(&result.report.local_path)?;
    grants.record_produced(&result.sidecar.local_path)?;
    Ok(result)
}

/// Probe the selected device for the platform Perfetto service and return only
/// fixed backend-owned capture presets.
#[tauri::command]
#[specta::specta]
pub fn perfetto_capabilities(
    target: adb::DeviceTarget,
) -> Result<perfetto::PerfettoCapabilities, CommandError> {
    let transport = validated_transport(&target)?;
    Ok(perfetto::capabilities(&transport, &target)?)
}

/// Capture a bounded local system trace after explicit privacy review. The
/// one-shot destination grant is consumed only after device support is
/// revalidated, and no trace content is uploaded or parsed by Droidsmith.
#[tauri::command]
#[specta::specta]
pub async fn capture_perfetto_trace(
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    #[allow(non_snake_case)] presetId: String,
    privacy_confirmed: bool,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<perfetto::PerfettoCaptureResult, CommandError> {
    if !privacy_confirmed {
        return Err(CommandError {
            code: "perfetto_privacy_confirmation_required",
            message: "review and acknowledge the Perfetto privacy warning before capture"
                .to_string(),
        });
    }
    let (transport, _) = privileged_transport(&target)?;
    if !perfetto::capabilities(&transport, &target)?.supported {
        return Err(perfetto::PerfettoError::Unsupported.into());
    }
    let destination = grants.consume(&path_grant, HostPathPurpose::PerfettoTraceSave)?;
    let adb_path = transport.adb_path;
    let sink = operations::channel_sink(on_event);
    let result = spawn_blocking_operation(move || {
        Ok(perfetto::capture(
            &adb_path,
            &target,
            &destination,
            &presetId,
            &operation_id,
            sink,
        )?)
    })
    .await?;
    grants.record_produced(&result.artifact.local_path)?;
    Ok(result)
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
    let result = spawn_blocking_operation(move || {
        Ok(backup::export_legacy_data(
            &adb_path,
            &target,
            &output_target,
            preflight,
            &operation_id,
            sink,
        )?)
    })
    .await?;
    grants.record_produced(&result.artifact.local_path)?;
    Ok(result)
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
    let identity = DeviceIdentity::from_target(&target);
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
    journal::with_journal(&dir, &identity, |journal| {
        execute_journaled(journal, &transport, plan, None)
    })
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct PermissionInfo {
    pub permission: String,
    pub granted: bool,
}

pub(crate) fn parse_permissions(stdout: &str) -> Vec<PermissionInfo> {
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
    pub density_dpi: Option<u32>,
    pub audit_findings: Vec<LayoutAuditFinding>,
    pub raw_xml: String,
}

/// Capture the current UI hierarchy with `uiautomator dump`. This is a
/// read-only inspection: it prints the hierarchy to `/dev/tty` (no device-side
/// file is written) and the renderer never controls any path.
#[tauri::command]
#[specta::specta]
pub fn capture_layout(target: adb::DeviceTarget) -> Result<LayoutSnapshot, CommandError> {
    let transport = validated_transport(&target)?;
    let density_dpi = transport
        .shell_target(&target, &["wm", "density"])
        .ok()
        .and_then(|output| parse_effective_density(&output));
    let stdout = transport.shell_target(&target, &["uiautomator", "dump", "/dev/tty"])?;
    let xml = extract_hierarchy(&stdout);
    let nodes = parse_uiautomator_dump(&xml);
    let node_count = nodes
        .iter()
        .filter(|node| node.parse_error.is_none())
        .count() as u32;
    let audit_findings = audit_layout_nodes(&nodes, density_dpi);
    Ok(LayoutSnapshot {
        nodes,
        node_count,
        density_dpi,
        audit_findings,
        raw_xml: xml,
    })
}

/// Isolate the `<hierarchy>…</hierarchy>` document from `uiautomator dump`
/// output, which appends a "UI hierarchy dumped to: /dev/tty" status line.
pub(crate) fn extract_hierarchy(stdout: &str) -> String {
    if let (Some(start), Some(end)) = (stdout.find("<hierarchy"), stdout.rfind("</hierarchy>")) {
        if end >= start {
            return stdout[start..end + "</hierarchy>".len()].to_string();
        }
    }
    stdout.trim().to_string()
}

/// Persist a captured UI hierarchy or accessibility-audit report through a
/// one-shot path grant. The size bound keeps the IPC and host write bounded,
/// mirroring the Logcat export.
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
    let result = spawn_blocking_operation(move || {
        let staged = StagedArtifact::new(&path)?;
        std::fs::write(staged.path(), contents.as_bytes())?;
        Ok(staged.commit(ArtifactKind::AnyFile)?.local_path)
    })
    .await?;
    grants.record_produced(&result)?;
    Ok(result)
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
    let artifact = staged.commit(ArtifactKind::Png)?;
    grants.record_produced(&artifact.local_path)?;
    Ok(artifact)
}

/// Build a per-capture unique `/sdcard` path. Uses the process id plus a
/// monotonic counter so two in-flight screenshots cannot collide without
/// depending on wall-clock time.
pub(crate) fn unique_screenshot_remote() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "/sdcard/droidsmith-screenshot-{}-{}.png",
        std::process::id(),
        n
    )
}
