/// `adb` is `pub` so the `droidsmith-cli` binary (which links against
/// this crate as `droidsmith_lib`) can reach the transport and action
/// types. The Tauri runtime still goes through `crate::adb`.
pub mod adb;
mod commands;
mod diagnostics;
mod fs_util;
mod journal;
/// `packs` is `pub` so the `droidsmith-pack-lint` binary (which links
/// against this crate as `droidsmith_lib`) can reach the loader + lint
/// types. Tauri-internal callers go through `crate::packs` as usual.
pub mod packs;
pub mod profile;
mod quirks;
pub mod time;

use commands::{
    apply_action, connect_wireless, explain_failure, extract_apk, get_device_info, heartbeat,
    install_apk, journal_list, journal_undo, launch_scrcpy, list_devices, list_packages,
    list_packs, list_permissions, list_processes, list_wireless_services, locate_scrcpy,
    pair_wireless, plan_action, set_permission, shell_run, take_screenshot,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Hook panics into a file log before we touch the Tauri runtime, so a
    // panic during Tauri init still leaves a forensic trail.
    let log_dir = diagnostics::fallback_log_dir();
    diagnostics::install_panic_hook(log_dir.clone());

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            heartbeat,
            list_devices,
            get_device_info,
            list_wireless_services,
            pair_wireless,
            connect_wireless,
            list_packages,
            list_packs,
            plan_action,
            apply_action,
            shell_run,
            list_permissions,
            set_permission,
            list_processes,
            take_screenshot,
            locate_scrcpy,
            launch_scrcpy,
            install_apk,
            extract_apk,
            journal_list,
            journal_undo,
            explain_failure,
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
