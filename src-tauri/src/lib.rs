/// `adb` is `pub` so the `droidsmith-cli` binary (which links against
/// this crate as `droidsmith_lib`) can reach the transport and action
/// types. The Tauri runtime still goes through `crate::adb`.
pub mod adb;
mod commands;
mod diagnostics;
mod fs_util;
pub mod journal;
mod operations;
/// `packs` is `pub` so the `droidsmith-pack-lint` binary (which links
/// against this crate as `droidsmith_lib`) can reach the loader + lint
/// types. Tauri-internal callers go through `crate::packs` as usual.
pub mod packs;
pub mod profile;
mod quirks;
mod scrcpy;
pub mod time;

use commands::{
    apply_action, apply_device_control, backup_package, cancel_operation, connect_wireless,
    explain_failure, extract_apk, fastboot_getvar, get_device_info, heartbeat, install_apk,
    journal_list, journal_undo, launch_scrcpy, list_devices, list_fastboot_devices,
    list_network_connections, list_packages, list_packs, list_permissions, list_processes,
    list_remote_files, list_users, list_wireless_services, locate_fastboot, locate_scrcpy,
    pair_wireless, plan_action, plan_pack, plan_shell_action, pull_file, push_file, recover_adb,
    save_logcat_export, scrcpy_session_status, set_permission, shell_run, stop_scrcpy,
    stream_logcat, take_screenshot, watch_devices,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Hook panics into a file log before we touch the Tauri runtime, so a
    // panic during Tauri init still leaves a forensic trail.
    let log_dir = diagnostics::fallback_log_dir();
    diagnostics::install_panic_hook(log_dir.clone());

    let builder = tauri::Builder::default()
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
            list_devices,
            watch_devices,
            recover_adb,
            get_device_info,
            list_wireless_services,
            pair_wireless,
            connect_wireless,
            list_packages,
            list_users,
            list_packs,
            plan_pack,
            plan_action,
            plan_shell_action,
            apply_action,
            apply_device_control,
            shell_run,
            stream_logcat,
            cancel_operation,
            save_logcat_export,
            list_remote_files,
            push_file,
            pull_file,
            backup_package,
            list_network_connections,
            list_permissions,
            set_permission,
            list_processes,
            take_screenshot,
            locate_scrcpy,
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
