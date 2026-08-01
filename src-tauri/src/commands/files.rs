//! Domain-scoped Tauri command boundary.

use super::*;

/// Open a backend-owned native file dialog and retain its result as a short-
/// lived, purpose-scoped, one-shot grant. Privileged commands accept only the
/// opaque grant id, never a renderer-authored host path.
#[tauri::command]
#[specta::specta]
pub async fn select_host_path(
    app: tauri::AppHandle,
    grants: tauri::State<'_, PathGrantStore>,
    purpose: HostPathPurpose,
    suggested_name: Option<String>,
) -> Result<Option<HostPathGrant>, CommandError> {
    use tauri_plugin_dialog::DialogExt;

    let suggested_name = validate_suggested_file_name(suggested_name)?;
    let mut dialog = app.dialog().file().set_title(purpose.dialog_title());
    if let Some(name) = suggested_name {
        dialog = dialog.set_file_name(name);
    }
    if let Some((name, extensions)) = purpose.filter() {
        dialog = dialog.add_filter(name, extensions);
    }
    let selected = if purpose.is_write() {
        dialog.blocking_save_file()
    } else {
        dialog.blocking_pick_file()
    };
    let Some(selected) = selected else {
        return Ok(None);
    };
    let selected_path = selected
        .simplified()
        .into_path()
        .map_err(|error| CommandError {
            code: "path_grant_invalid_path",
            message: error.to_string(),
        })?;
    Ok(Some(grants.issue(&selected_path, purpose)?))
}

/// Issue a one-shot path grant for a file dropped onto the window by the OS.
/// Only `InstallOpen` purpose is accepted, and the path must exist, be absolute,
/// and carry a supported extension (.apk, .apks, .xapk, .apkm). This does not
/// bypass the grant model: the install command still consumes the grant normally.
#[tauri::command]
#[specta::specta]
pub fn grant_dropped_path(
    grants: tauri::State<'_, PathGrantStore>,
    path: String,
) -> Result<HostPathGrant, CommandError> {
    let path = std::path::PathBuf::from(&path);
    if !path.is_absolute() {
        return Err(CommandError {
            code: "dropped_path_relative",
            message: "dropped path must be absolute".to_string(),
        });
    }
    if !path.is_file() {
        return Err(CommandError {
            code: "dropped_path_not_file",
            message: "dropped path does not exist or is not a regular file".to_string(),
        });
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "apk" | "apks" | "xapk" | "apkm") {
        return Err(CommandError {
            code: "dropped_path_wrong_type",
            message: format!(
                "dropped file must be an Android package (.apk, .apks, .xapk, .apkm), got .{extension}"
            ),
        });
    }
    Ok(grants.issue(&path, HostPathPurpose::InstallOpen)?)
}

#[derive(specta::Type, Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectResult {
    pub serial: String,
    pub disconnected: bool,
    pub message: String,
}

/// Disconnect a device safely. Wireless devices are disconnected via
/// `adb disconnect`; USB devices cannot be disconnected programmatically
/// but the user is advised that it is safe to unplug.
#[tauri::command]
#[specta::specta]
pub fn disconnect_device(target: adb::DeviceTarget) -> Result<DisconnectResult, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    if target.transport_kind == adb::DeviceTransportKind::Usb {
        return Ok(DisconnectResult {
            serial: target.serial,
            disconnected: false,
            message: "USB device cannot be disconnected via ADB. It is safe to unplug the cable."
                .to_string(),
        });
    }
    let result = transport.adb(&["disconnect", &target.serial]);
    match result {
        Ok(stdout) => Ok(DisconnectResult {
            serial: target.serial,
            disconnected: true,
            message: stdout.trim().to_string(),
        }),
        Err(error) => Ok(DisconnectResult {
            serial: target.serial,
            disconnected: false,
            message: format!("disconnect failed: {error}"),
        }),
    }
}

/// Open the OS file manager at an artifact Droidsmith produced this session.
/// `path` must equal a save-dialog destination the backend itself issued; any
/// other renderer-supplied path is rejected, so the renderer can never drive an
/// open of an arbitrary location. The file manager is spawned detached.
#[tauri::command]
#[specta::specta]
pub fn reveal_in_folder(
    grants: tauri::State<'_, PathGrantStore>,
    path: String,
) -> Result<(), CommandError> {
    let target = grants.resolve_produced(&path).ok_or_else(|| CommandError {
        code: "reveal_path_not_produced",
        message: "only intact artifacts Droidsmith produced this session can be revealed"
            .to_string(),
    })?;
    let (program, args) = reveal_command(&target);
    std::process::Command::new(&program)
        .args(&args)
        .spawn()
        .map_err(|error| CommandError {
            code: "reveal_failed",
            message: format!("could not open the file manager: {error}"),
        })?;
    Ok(())
}

/// Open a Droidsmith-produced artifact with the platform chooser/association.
/// The renderer cannot use this command for an arbitrary host path.
#[tauri::command]
#[specta::specta]
pub fn open_artifact_with(
    grants: tauri::State<'_, PathGrantStore>,
    path: String,
) -> Result<(), CommandError> {
    let target = grants.resolve_produced(&path).ok_or_else(|| CommandError {
        code: "open_path_not_produced",
        message: "only intact artifacts Droidsmith produced this session can be opened".to_string(),
    })?;
    let (program, args) = open_with_command(&target);
    std::process::Command::new(&program)
        .args(&args)
        .spawn()
        .map_err(|error| CommandError {
            code: "open_with_failed",
            message: format!("could not open the platform file chooser: {error}"),
        })?;
    Ok(())
}

/// Open Droidsmith's backend-resolved crash-log directory. The command accepts
/// no path or grant from the renderer, so an error surface cannot be repurposed
/// to open an arbitrary host location.
#[tauri::command]
#[specta::specta]
pub fn reveal_diagnostics_directory() -> Result<(), CommandError> {
    let directory = crate::diagnostics::fallback_log_dir();
    std::fs::create_dir_all(&directory).map_err(|error| CommandError {
        code: "diagnostics_directory_unavailable",
        message: format!("could not prepare the diagnostics directory: {error}"),
    })?;
    let (program, args) = open_directory_command(&directory);
    std::process::Command::new(&program)
        .args(&args)
        .spawn()
        .map_err(|error| CommandError {
            code: "reveal_failed",
            message: format!("could not open the diagnostics directory: {error}"),
        })?;
    Ok(())
}

/// List files in a remote directory on the device.
#[tauri::command]
#[specta::specta]
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

/// Validate a structured file mutation and return the exact argv that will be
/// journaled and executed after the renderer presents its confirmation review.
#[tauri::command]
#[specta::specta]
pub fn plan_remote_file_mutation(
    request: remote_files::RemoteFileMutationRequest,
) -> Result<remote_files::RemoteFileMutationPlan, CommandError> {
    Ok(remote_files::plan(&request)?)
}

/// Rebuild and execute a reviewed device-side file mutation. The renderer
/// cannot supply argv: it submits only structured paths and the backend
/// regenerates the canonical mkdir/mv/rm command before writing the intent.
#[tauri::command]
#[specta::specta]
pub fn apply_remote_file_mutation(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    request: remote_files::RemoteFileMutationRequest,
    confirmed: bool,
) -> Result<ApplyActionResult, CommandError> {
    let reviewed = remote_files::plan(&request)?;
    if !confirmed {
        return Err(CommandError {
            code: "confirmation_required",
            message: "remote file mutation requires an explicit confirmation review".to_string(),
        });
    }
    let (transport, transport_override) = privileged_transport(&target)?;
    let user_id = current_android_user(&transport, &target)?;
    let identity = DeviceIdentity::from_target(&target);
    let plan = remote_files::action_plan(target, user_id, transport_override, &reviewed);
    let dir = journal_dir(&app)?;
    journal::with_journal(&dir, &identity, |journal| {
        execute_remote_file_journaled(journal, &transport, plan)
    })
}

/// Push a local file to the device.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn push_file(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    remote_path: String,
    confirmed: bool,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<ApplyActionResult, CommandError> {
    if !confirmed {
        return Err(CommandError {
            code: "confirmation_required",
            message: "file push requires an explicit source/target confirmation".to_string(),
        });
    }
    let (transport, transport_override) = privileged_transport(&target)?;
    let validated_path = grants.consume(&path_grant, HostPathPurpose::PushOpen)?;
    let remote = validate_remote_path(&remote_path)?;
    let user_id = current_android_user(&transport, &target)?;
    let local_arg = validated_path.display().to_string();
    let timeout = std::time::Duration::from_secs(120);
    let mut args = target.adb_selector();
    args.extend(["push".to_string(), local_arg, remote.clone()]);
    let adb_path = transport.adb_path.clone();
    let sink = operations::channel_sink(on_event);
    let serial = target.serial.clone();
    let identity = DeviceIdentity::from_target(&target);
    let journal_path = journal_dir(&app)?;
    let mut plan = actions::plan(actions::ActionRequest {
        serial: serial.clone(),
        target: target.clone(),
        package: String::new(),
        kind: actions::ActionKind::Shell,
        user_id,
        pack_context: None,
        context: actions::ActionContext {
            confirmation_source: actions::ConfirmationSource::FileManagerReview,
            permission: None,
            shell_argv: vec!["droidsmith-file-push".to_string(), remote.clone()],
            device_control_restore_argv: Vec::new(),
            device_control_expected_before: None,
            transport_override,
            restore_enabled_state: None,
            batch_id: None,
        },
    });
    plan.description = format!("Push a native-selected local file to {remote:?}");
    plan.before_state = format!(
        "{remote}={}",
        remote_files::capture_path_state(&transport, &target, &remote)
    );
    spawn_blocking_operation(move || {
        journal::with_journal(&journal_path, &identity, |journal| {
            let started_at = iso_now();
            let entry = journal
                .execute(plan, None, &started_at, |plan| {
                    adb::validate_device_target(&transport, &target)?;
                    let output = operations::run_process(
                        &adb_path,
                        &args,
                        timeout,
                        &operation_id,
                        "Pushing file to device",
                        sink,
                    )?;
                    let stdout = completed_adb_output(output, "adb push")?;
                    let after_state = format!(
                        "{remote}={}",
                        remote_files::capture_path_state(&transport, &target, &remote)
                    );
                    if !after_state.ends_with("=present") {
                        return Err(CommandError {
                            code: "remote_file_operation_failed",
                            message:
                                "adb push exited successfully but the target file was not observed"
                                    .to_string(),
                        });
                    }
                    Ok::<_, CommandError>(actions::AppliedAction {
                        stdout: actions::redact_journal_text(&plan.request, &stdout),
                        display_stdout: stdout,
                        before_state: plan.before_state.clone(),
                        after_state,
                        plan,
                        applied_at: iso_now(),
                    })
                })
                .map_err(map_remote_file_execute_error)?;
            Ok(ApplyActionResult {
                stdout: entry.applied.display_stdout.clone(),
                entry,
            })
        })
    })
    .await
}

/// Pull a remote file from the device.
#[tauri::command]
#[specta::specta]
pub async fn pull_file(
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    remote_path: String,
    path_grant: String,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<HostArtifact, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let output_target = grants.consume(&path_grant, HostPathPurpose::PullSave)?;
    let remote = validate_remote_path(&remote_path)?;
    let timeout = std::time::Duration::from_secs(120);
    let selector = target.adb_selector();
    let adb_path = transport.adb_path.clone();
    let sink = operations::channel_sink(on_event);
    let artifact = spawn_blocking_operation(move || {
        let staged = StagedArtifact::new(&output_target)?;
        let mut args = selector;
        args.extend([
            "pull".to_string(),
            remote,
            staged.path().display().to_string(),
        ]);
        let output = operations::run_process(
            &adb_path,
            &args,
            timeout,
            &operation_id,
            "Pulling file from device",
            sink,
        )?;
        completed_adb_output(output, "adb pull")?;
        Ok(staged.commit(ArtifactKind::AnyFile)?)
    })
    .await?;
    grants.record_produced(&artifact.local_path)?;
    Ok(artifact)
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct RemoteListing {
    pub path: String,
    pub entries: Vec<RemoteFileEntry>,
    pub free_space_kb: Option<u64>,
}

pub(crate) fn parse_df_free(stdout: &str) -> Option<u64> {
    for line in stdout.lines().skip(1) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() >= 4 {
            return tokens[3].parse().ok();
        }
    }
    None
}
