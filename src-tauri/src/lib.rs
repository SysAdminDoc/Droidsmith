/// `adb` is `pub` so the `droidsmith-cli` binary (which links against
/// this crate as `droidsmith_lib`) can reach the transport and action
/// types. The Tauri runtime still goes through `crate::adb`.
pub mod adb;
mod apk_metadata;
mod backup;
mod bugreport;
mod captured_tail;
mod commands;
pub mod contribution_schema;
mod diagnostics;
mod fs_util;
mod gnirehtet;
mod host_diagnostics;
mod host_path;
mod install;
pub mod journal;
pub mod operations;
/// `packs` is `pub` so the `droidsmith-pack-lint` binary (which links
/// against this crate as `droidsmith_lib`) can reach the loader + lint
/// types. Tauri-internal callers go through `crate::packs` as usual.
pub mod packs;
mod process_tree;
pub mod profile;
/// Public for the read-only schema-lint binary; runtime callers still use the
/// same bounded loader and validation path.
pub mod quirks;
pub mod recovery_baseline;
mod remote_files;
mod scrcpy;
pub mod settings;
mod support_bundle;
pub mod time;

use commands::{
    apply_action, apply_action_batch, apply_device_control, apply_remote_file_mutation,
    backup_package, cancel_operation, capture_bugreport, capture_layout, connect_wireless,
    disconnect_device, explain_failure, export_package_apks, export_recovery_baseline,
    export_settings, extract_apk, fastboot_getvar, forget_wireless_endpoint, get_device_info,
    get_package_metadata, get_settings_mirror_preset, gnirehtet_session_status, grant_dropped_path,
    heartbeat, initialize_settings, inspect_profile, inspect_recovery_baseline, install_apk,
    journal_list, journal_undo, journal_undo_batch, launch_scrcpy, list_device_settings,
    list_devices, list_fastboot_devices, list_logcat_queries, list_network_connections,
    list_packages, list_packs, list_permissions, list_processes, list_remote_files,
    list_running_services, list_users, list_wireless_history, list_wireless_services,
    locate_fastboot, locate_gnirehtet, locate_scrcpy, pair_wireless, plan_action,
    plan_action_batch, plan_pack, plan_remote_file_mutation, plan_shell_action,
    preflight_package_backup, preview_diagnostics, pull_file, push_file, put_device_setting,
    recover_adb, reset_settings, reset_settings_mirror_preset, reveal_in_folder, run_host_doctor,
    save_diagnostics, save_layout_export, save_logcat_export, save_logcat_queries, save_profile,
    scrcpy_capabilities, scrcpy_session_status, select_host_path, set_permission,
    set_settings_language, set_settings_mirror_preset, set_wireless_auto_reconnect, shell_run,
    start_gnirehtet, stop_gnirehtet, stop_scrcpy, stream_logcat, take_screenshot, watch_devices,
    wipe_diagnostics,
};

fn ipc_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
        .commands(tauri_specta::collect_commands![
            heartbeat,
            initialize_settings,
            set_settings_language,
            get_settings_mirror_preset,
            set_settings_mirror_preset,
            reset_settings_mirror_preset,
            reset_settings,
            export_settings,
            list_logcat_queries,
            save_logcat_queries,
            capture_layout,
            save_layout_export,
            run_host_doctor,
            list_devices,
            watch_devices,
            recover_adb,
            select_host_path,
            grant_dropped_path,
            disconnect_device,
            reveal_in_folder,
            preview_diagnostics,
            save_diagnostics,
            wipe_diagnostics,
            get_device_info,
            list_device_settings,
            put_device_setting,
            list_wireless_services,
            pair_wireless,
            connect_wireless,
            list_wireless_history,
            forget_wireless_endpoint,
            set_wireless_auto_reconnect,
            list_packages,
            get_package_metadata,
            list_users,
            inspect_profile,
            save_profile,
            list_packs,
            plan_pack,
            plan_action,
            plan_action_batch,
            plan_shell_action,
            apply_action,
            apply_action_batch,
            export_recovery_baseline,
            inspect_recovery_baseline,
            apply_device_control,
            shell_run,
            stream_logcat,
            cancel_operation,
            save_logcat_export,
            list_remote_files,
            plan_remote_file_mutation,
            apply_remote_file_mutation,
            push_file,
            pull_file,
            preflight_package_backup,
            export_package_apks,
            backup_package,
            capture_bugreport,
            list_network_connections,
            list_permissions,
            set_permission,
            list_processes,
            list_running_services,
            take_screenshot,
            locate_scrcpy,
            scrcpy_capabilities,
            launch_scrcpy,
            scrcpy_session_status,
            stop_scrcpy,
            locate_gnirehtet,
            start_gnirehtet,
            gnirehtet_session_status,
            stop_gnirehtet,
            install_apk,
            extract_apk,
            journal_list,
            journal_undo,
            journal_undo_batch,
            explain_failure,
            locate_fastboot,
            list_fastboot_devices,
            fastboot_getvar,
        ])
}

fn typescript_exporter() -> specta_typescript::Typescript {
    // Droidsmith's u64 wire values are bounded counters, timestamps, and
    // artifact sizes well below JavaScript's safe-integer ceiling. Preserve
    // the existing JSON/TypeScript number contract explicitly.
    specta_typescript::Typescript::default().bigint(specta_typescript::BigIntExportBehavior::Number)
}

pub fn export_typescript_bindings(path: &std::path::Path) -> Result<(), String> {
    let raw = ipc_builder()
        .export_str(typescript_exporter())
        .map_err(|error| error.to_string())?;
    let bindings = normalize_compatible_specta_output(raw)?;
    std::fs::write(path, bindings).map_err(|error| error.to_string())
}

fn normalize_compatible_specta_output(raw: String) -> Result<String, String> {
    // rc.21 is the final Tauri Specta v2 release compatible with Droidsmith's
    // Rust 1.81 floor. Normalize two exporter bugs fixed on the Rust-2024 line:
    // reserved command parameters and unused event scaffolding for zero events.
    let types_marker = "/** user-defined types **/";
    let globals_marker = "/** tauri-specta globals **/";
    let types_at = raw
        .find(types_marker)
        .ok_or_else(|| "generated bindings are missing the types marker".to_string())?;
    let globals_at = raw
        .find(globals_marker)
        .ok_or_else(|| "generated bindings are missing the globals marker".to_string())?;
    let mut commands = raw[..types_at]
        .replace(" package: string", " packageName: string")
        .replace(", package, ", ", packageName, ")
        .replace(", package }", ", packageName }");
    // Tauri accepts camelCase argument keys, but Droidsmith's IPC isolation
    // policy deliberately validates the Rust wire names. Keep generated
    // function parameters idiomatic while preserving that audited boundary.
    for (camel, wire) in [
        ("suggestedName", "suggested_name"),
        ("operationId", "operation_id"),
        ("onEvent", "on_event"),
        ("pathGrant", "path_grant"),
        ("privacyConfirmed", "privacy_confirmed"),
        ("remotePath", "remote_path"),
        ("sessionId", "session_id"),
        ("entryId", "entry_id"),
        ("packageName", "package"),
    ] {
        commands = commands
            .replace(&format!("{{ {camel},"), &format!("{{ {wire}: {camel},"))
            .replace(&format!(", {camel},"), &format!(", {wire}: {camel},"))
            .replace(&format!(", {camel} }}"), &format!(", {wire}: {camel} }}"))
            .replace(&format!("{{ {camel} }}"), &format!("{{ {wire}: {camel} }}"));
    }
    let types = raw[types_at..globals_at].replace("export type TAURI_CHANNEL<TSend> = null\n", "");
    Ok(format!(
        "{commands}{types}{globals_marker}\n\nimport {{ invoke as TAURI_INVOKE, Channel as TAURI_CHANNEL }} from \"@tauri-apps/api/core\";\n"
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Hook panics into a file log before we touch the Tauri runtime, so a
    // panic during Tauri init still leaves a forensic trail.
    let log_dir = diagnostics::fallback_log_dir();
    diagnostics::install_panic_hook(log_dir.clone());

    let ipc = ipc_builder();
    let builder = tauri::Builder::default()
        .manage(host_path::PathGrantStore::default())
        // Single-instance must be the FIRST plugin registered (Tauri
        // requirement). A second launch focuses the existing window and
        // exits instead of spawning a rival adb server.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(ipc.invoke_handler());

    let context = tauri::generate_context!();
    if let Err(e) = builder.run(context) {
        // Builder errors do NOT trigger the panic hook (it returns Err
        // not panics), so we must explicitly record them — otherwise the
        // dialog's "a crash log was written" claim is a lie.
        let message = format!("{e}");
        diagnostics::log_fatal(&log_dir, "tauri-builder", &message);
        diagnostics::fatal_dialog(
            "Droidsmith failed to start",
            &format!(
                "{message}\n\nA crash log was written to:\n{}",
                log_dir.display()
            ),
        );
        std::process::exit(1);
    }
}

#[cfg(test)]
mod binding_tests {
    use super::*;

    #[test]
    fn compatible_export_normalization_preserves_wire_keys() {
        let raw = r#"export const commands = {
  async getPackageMetadata(target: DeviceTarget, package: string, operationId: string) {
    return await TAURI_INVOKE("get_package_metadata", { target, package, operationId });
  }
};
/** user-defined types **/
export type TAURI_CHANNEL<TSend> = null
/** tauri-specta globals **/
unused
"#
        .to_string();
        let actual = normalize_compatible_specta_output(raw).unwrap();
        assert!(!actual.contains("package: string"));
        assert!(actual.contains("packageName: string"));
        assert!(actual.contains("package: packageName"));
        assert!(actual.contains("operation_id: operationId"));
        assert!(!actual.contains("export type TAURI_CHANNEL"));
    }
}
