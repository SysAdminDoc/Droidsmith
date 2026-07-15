/// `adb` is `pub` so the `droidsmith-cli` binary (which links against
/// this crate as `droidsmith_lib`) can reach the transport and action
/// types. The Tauri runtime still goes through `crate::adb`.
pub mod adb;
mod apk_metadata;
mod backup;
mod bugreport;
mod commands;
mod diagnostics;
mod fs_util;
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
mod scrcpy;
mod support_bundle;
pub mod time;

use commands::{
    apply_action, apply_device_control, backup_package, cancel_operation, capture_bugreport,
    connect_wireless, explain_failure, export_package_apks, export_recovery_baseline, extract_apk,
    fastboot_getvar, get_device_info, get_package_metadata, heartbeat, inspect_recovery_baseline,
    install_apk, journal_list, journal_undo, launch_scrcpy, list_devices, list_fastboot_devices,
    list_network_connections, list_packages, list_packs, list_permissions, list_processes,
    list_remote_files, list_users, list_wireless_services, locate_fastboot, locate_scrcpy,
    pair_wireless, plan_action, plan_pack, plan_shell_action, preflight_package_backup,
    preview_diagnostics, pull_file, push_file, recover_adb, run_host_doctor, save_diagnostics,
    save_logcat_export, scrcpy_capabilities, scrcpy_session_status, select_host_path,
    set_permission, shell_run, stop_scrcpy, stream_logcat, take_screenshot, watch_devices,
    wipe_diagnostics,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Hook panics into a file log before we touch the Tauri runtime, so a
    // panic during Tauri init still leaves a forensic trail.
    let log_dir = diagnostics::fallback_log_dir();
    diagnostics::install_panic_hook(log_dir.clone());

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
        .invoke_handler(tauri::generate_handler![
            heartbeat,
            run_host_doctor,
            list_devices,
            watch_devices,
            recover_adb,
            select_host_path,
            preview_diagnostics,
            save_diagnostics,
            wipe_diagnostics,
            get_device_info,
            list_wireless_services,
            pair_wireless,
            connect_wireless,
            list_packages,
            get_package_metadata,
            list_users,
            list_packs,
            plan_pack,
            plan_action,
            plan_shell_action,
            apply_action,
            export_recovery_baseline,
            inspect_recovery_baseline,
            apply_device_control,
            shell_run,
            stream_logcat,
            cancel_operation,
            save_logcat_export,
            list_remote_files,
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
            take_screenshot,
            locate_scrcpy,
            scrcpy_capabilities,
            launch_scrcpy,
            scrcpy_session_status,
            stop_scrcpy,
            install_apk,
            extract_apk,
            journal_list,
            journal_undo,
            explain_failure,
            locate_fastboot,
            list_fastboot_devices,
            fastboot_getvar,
        ]);

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
