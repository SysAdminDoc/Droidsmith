//! Domain-scoped Tauri command boundary.

use super::*;

/// Run a one-shot read-only shell command outside the webview thread and
/// stream progress/output through a Tauri channel. Mutations continue to use
/// the reviewed audited executor.
#[tauri::command]
#[specta::specta]
pub async fn shell_run(
    target: adb::DeviceTarget,
    argv: Vec<String>,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<String, CommandError> {
    if !actions::valid_shell_argv(&argv) {
        return Err(CommandError {
            code: "invalid_shell_argv",
            message: "shell argv is empty, oversized, or contains control characters".to_string(),
        });
    }
    if classify_shell(&argv) != ShellClassification::ReadOnly {
        return Err(CommandError {
            code: "shell_mutation_requires_review",
            message: "mutating shell commands must be reviewed and executed through the audited operation planner".to_string(),
        });
    }
    let (transport, _) = privileged_transport(&target)?;
    let adb_path = transport.adb_path.clone();
    let mut args = target.adb_selector();
    args.push("shell".to_string());
    args.extend(argv);
    let sink = operations::channel_sink(on_event);
    spawn_blocking_operation(move || {
        let output = operations::run_process(
            &adb_path,
            &args,
            std::time::Duration::from_secs(300),
            &operation_id,
            "Running ADB shell command",
            sink,
        )?;
        completed_adb_output(output, "adb shell")
    })
    .await
}

/// Cancel a background operation. A bounded pending marker closes the race
/// where renderer invalidation arrives just before backend registration; the
/// runner observes the flag before spawn or kills and reaps an active child.
#[tauri::command]
#[specta::specta]
pub fn cancel_operation(operation_id: String) -> bool {
    operations::cancel(&operation_id)
}

/// Start one incremental Logcat process. Unexpected exits are retried by the
/// backend and surfaced as reconnect markers; the call completes only after
/// cancellation or an unrecoverable spawn/wait failure.
#[tauri::command]
#[specta::specta]
pub async fn stream_logcat(
    target: adb::DeviceTarget,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<(), CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let adb_path = transport.adb_path.clone();
    let mut args = target.adb_selector();
    args.extend([
        "shell".to_string(),
        "logcat".to_string(),
        "-v".to_string(),
        // threadtime carries the timestamp and pid the query presets filter on;
        // brief format omitted both.
        "threadtime".to_string(),
    ]);
    let sink = operations::channel_sink(on_event);
    spawn_blocking_operation(move || {
        operations::stream_logcat(&adb_path, &args, &operation_id, sink)?;
        Ok(())
    })
    .await
}

/// Persist the renderer's bounded Logcat buffer through a one-shot path grant.
/// The size limit keeps the IPC and host write bounded.
#[tauri::command]
#[specta::specta]
pub async fn save_logcat_export(
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    contents: String,
) -> Result<String, CommandError> {
    const MAX_LOGCAT_EXPORT_BYTES: usize = 4 * 1024 * 1024;
    if contents.len() > MAX_LOGCAT_EXPORT_BYTES {
        return Err(CommandError {
            code: "logcat_export_too_large",
            message: format!("Logcat export exceeds the {MAX_LOGCAT_EXPORT_BYTES}-byte limit"),
        });
    }
    let path = grants.consume(&path_grant, HostPathPurpose::LogcatSave)?;
    if !path.parent().is_some_and(std::path::Path::is_dir) {
        return Err(CommandError {
            code: "invalid_path",
            message: "Logcat export parent directory does not exist".to_string(),
        });
    }
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(CommandError {
            code: "invalid_path",
            message: "Logcat export target must not be a symbolic link".to_string(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellClassification {
    ReadOnly,
    Mutation,
    Dangerous,
}

/// Characters that let the on-device shell chain a second command, substitute
/// a subshell, or redirect output. `adb shell` joins argv with spaces and runs
/// the result through the device's `sh -c`, so any of these inside *any* token
/// (e.g. a `getprop; pm uninstall …` typed into the console and split on
/// whitespace into `getprop;`) would execute a hidden mutation while the head
/// token still looked read-only. Such argv can never be classified read-only.
pub(crate) fn argv_has_shell_control_metacharacter(argv: &[String]) -> bool {
    actions::argv_has_shell_control_metacharacter(argv)
}

pub(crate) fn logcat_is_read_only(argv: &[String]) -> bool {
    // `logcat` is usually diagnostic, but several options mutate the device or
    // write a persistent device-side file. Keep those behind the reviewed
    // action path instead of letting a diagnostic-looking command bypass it.
    !argv.iter().skip(1).any(|argument| {
        matches!(
            argument.as_str(),
            "-c" | "--clear"
                | "-G"
                | "--buffer-size"
                | "-f"
                | "--file"
                | "-r"
                | "--rotate-kbytes"
                | "-n"
                | "--rotate-count"
        )
    })
}

pub(crate) fn dumpsys_is_read_only(argv: &[String]) -> bool {
    // Service-specific arguments are an open-ended command surface. Some
    // services expose mutations such as `battery set` and `deviceidle
    // force-idle`, so only the top-level listing and a single service/query
    // selector can skip review.
    argv.len() <= 2
}

pub(crate) fn classify_shell(argv: &[String]) -> ShellClassification {
    // A token carrying a shell control metacharacter can smuggle a mutation past
    // the head-token classifier, so refuse to treat it as anything but dangerous
    // — that routes it through the reviewed/journaled executor (or is rejected
    // outright by `shell_run`, which only runs read-only commands).
    if argv_has_shell_control_metacharacter(argv) {
        return ShellClassification::Dangerous;
    }
    let head = argv.first().map(String::as_str).unwrap_or_default();
    let subcommand = argv.get(1).map(String::as_str).unwrap_or_default();
    match head {
        "logcat" if logcat_is_read_only(argv) => ShellClassification::ReadOnly,
        "dumpsys" if dumpsys_is_read_only(argv) => ShellClassification::ReadOnly,
        "getprop" | "ps" | "ss" | "netstat" | "ls" | "df" | "stat" | "cat" | "id" | "uname" => {
            ShellClassification::ReadOnly
        }
        "wm" if argv.len() <= 2 && matches!(subcommand, "size" | "density") => {
            ShellClassification::ReadOnly
        }
        "settings" if matches!(subcommand, "get" | "list") => ShellClassification::ReadOnly,
        "pm" if matches!(subcommand, "list" | "path" | "dump") => ShellClassification::ReadOnly,
        "cmd"
            if argv.get(1).map(String::as_str) == Some("package")
                && matches!(argv.get(2).map(String::as_str), Some("list") | Some("path")) =>
        {
            ShellClassification::ReadOnly
        }
        "input" | "wm" | "settings" => ShellClassification::Mutation,
        _ => ShellClassification::Dangerous,
    }
}

#[derive(specta::Type, Debug, Clone, Deserialize)]
pub struct PlanShellActionRequest {
    pub target: adb::DeviceTarget,
    pub argv: Vec<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ShellActionPlan {
    pub mutating: bool,
    pub dangerous: bool,
    pub plan: Option<actions::PlannedAction>,
}

#[tauri::command]
#[specta::specta]
pub fn plan_shell_action(request: PlanShellActionRequest) -> Result<ShellActionPlan, CommandError> {
    if !actions::valid_shell_argv(&request.argv) {
        return Err(CommandError {
            code: "invalid_shell_argv",
            message: "shell argv is empty, oversized, or contains control characters".to_string(),
        });
    }
    let (transport, transport_override) = privileged_transport(&request.target)?;
    plan_shell_action_with_transport(&transport, transport_override, request)
}

fn plan_shell_action_with_transport(
    transport: &dyn AdbTransport,
    transport_override: Option<adb::DeviceTransportKind>,
    request: PlanShellActionRequest,
) -> Result<ShellActionPlan, CommandError> {
    let classification = classify_shell(&request.argv);
    if classification == ShellClassification::ReadOnly {
        return Ok(ShellActionPlan {
            mutating: false,
            dangerous: false,
            plan: None,
        });
    }
    let users = adb::list_users(transport, &request.target)?;
    let user_id = users
        .iter()
        .find(|user| user.current)
        .map(|user| user.id)
        .ok_or(CommandError {
            code: "current_user_missing",
            message: "could not bind the shell mutation to the current Android user".to_string(),
        })?;
    let plan = actions::plan(actions::ActionRequest {
        serial: request.target.serial.clone(),
        target: request.target,
        package: String::new(),
        kind: actions::ActionKind::Shell,
        user_id,
        pack_context: None,
        context: actions::ActionContext {
            confirmation_source: actions::ConfirmationSource::ConsoleReview,
            permission: None,
            shell_argv: request.argv,
            device_control_restore_argv: Vec::new(),
            device_control_expected_before: None,
            transport_override,
            restore_enabled_state: None,
            batch_id: None,
        },
    });
    Ok(ShellActionPlan {
        mutating: true,
        dangerous: classification == ShellClassification::Dangerous,
        plan: Some(plan),
    })
}

pub(crate) fn is_allowed_device_control(argv: &[String]) -> bool {
    matches!(
        argv,
        [input, keyevent, code]
            if input == "input" && keyevent == "keyevent" && code.parse::<u32>().is_ok()
    ) || matches!(
        argv,
        [wm, density, value]
            if wm == "wm"
                && density == "density"
                && (value == "reset" || value.parse::<u16>().is_ok_and(|value| (72..=1000).contains(&value)))
    ) || matches!(
        argv,
        [settings, put, secure, key, value]
            if settings == "settings"
                && put == "put"
                && secure == "secure"
                && key == "ui_night_mode"
                && matches!(value.as_str(), "1" | "2")
    )
}

#[tauri::command]
#[specta::specta]
pub fn apply_device_control(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    argv: Vec<String>,
) -> Result<ApplyActionResult, CommandError> {
    if !is_allowed_device_control(&argv) {
        return Err(CommandError {
            code: "device_control_not_allowed",
            message: "command is not an allowlisted Droidsmith device control".to_string(),
        });
    }
    let (transport, transport_override) = privileged_transport(&target)?;
    let users = adb::list_users(&transport, &target)?;
    let user_id = users
        .iter()
        .find(|user| user.current)
        .map(|user| user.id)
        .ok_or(CommandError {
            code: "current_user_missing",
            message: "could not bind the device control to the current Android user".to_string(),
        })?;
    let prepared = actions::prepare_device_control(&transport, &target, user_id, &argv)?;
    let serial = target.serial.clone();
    let identity = DeviceIdentity::from_target(&target);
    let mut plan = actions::plan(actions::ActionRequest {
        serial: serial.clone(),
        target,
        package: String::new(),
        kind: actions::ActionKind::Shell,
        user_id,
        pack_context: None,
        context: actions::ActionContext {
            confirmation_source: actions::ConfirmationSource::DeviceControl,
            permission: None,
            shell_argv: prepared.argv,
            device_control_restore_argv: prepared.restore_argv,
            device_control_expected_before: None,
            transport_override,
            restore_enabled_state: None,
            batch_id: None,
        },
    });
    plan.before_state = prepared.before_state;
    let dir = journal_dir(&app)?;
    journal::with_journal(&dir, &identity, |journal| {
        execute_journaled(journal, &transport, plan, None)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::device::DeviceState;
    use crate::adb::transport::MockTransport;

    fn target() -> adb::DeviceTarget {
        adb::DeviceTarget {
            serial: "console-test".to_string(),
            transport_id: Some(7),
            connection_generation: 1,
            model: Some("Pixel".to_string()),
            product: Some("pixel".to_string()),
            device: Some("husky".to_string()),
            build_fingerprint: Some("google/husky/test".to_string()),
            transport_kind: adb::DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        }
    }

    #[test]
    fn plan_shell_action_rejects_invalid_argv_before_transport_access() {
        let error = plan_shell_action(PlanShellActionRequest {
            target: target(),
            argv: Vec::new(),
        })
        .unwrap_err();
        assert_eq!(error.code, "invalid_shell_argv");
    }

    #[test]
    fn plan_shell_action_keeps_read_only_commands_out_of_the_mutation_plan() {
        let result = plan_shell_action_with_transport(
            &MockTransport::new(),
            None,
            PlanShellActionRequest {
                target: target(),
                argv: vec!["getprop".to_string(), "ro.product.model".to_string()],
            },
        )
        .unwrap();
        assert!(!result.mutating);
        assert!(!result.dangerous);
        assert!(result.plan.is_none());
    }

    #[test]
    fn plan_shell_action_binds_mutations_to_the_current_user() {
        let mock = MockTransport::new();
        mock.expect_shell(
            "console-test",
            &["pm", "list", "users"],
            Ok("Users:\n    UserInfo{0:Owner:c13} running (current)\n".to_string()),
        );
        mock.expect_shell(
            "console-test",
            &["am", "get-current-user"],
            Ok("0\n".to_string()),
        );
        let result = plan_shell_action_with_transport(
            &mock,
            None,
            PlanShellActionRequest {
                target: target(),
                argv: vec![
                    "settings".to_string(),
                    "put".to_string(),
                    "secure".to_string(),
                    "ui_night_mode".to_string(),
                    "2".to_string(),
                ],
            },
        )
        .unwrap();
        let plan = result.plan.expect("mutation plan");
        assert!(result.mutating);
        assert_eq!(plan.request.user_id, 0);
        assert_eq!(plan.args[0], "settings");
        assert_eq!(plan.args[4], "2");
    }

    #[test]
    fn target_fixture_retains_device_state_for_command_surface_tests() {
        let device = adb::Device {
            serial: "console-test".to_string(),
            state: DeviceState::Device,
            model: Some("Pixel".to_string()),
            product: Some("pixel".to_string()),
            device: Some("husky".to_string()),
            marketing_name: None,
            bus_address: None,
            connection_type: None,
            negotiated_speed: None,
            max_speed: None,
            build_fingerprint: Some("google/husky/test".to_string()),
            transport_id: Some(7),
            connection_generation: 1,
            transport_kind: adb::DeviceTransportKind::Usb,
            wireless: false,
        };
        assert_eq!(device.target(), target());
    }
}
