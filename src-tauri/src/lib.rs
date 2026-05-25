mod adb;
mod commands;
mod diagnostics;
mod journal;
/// `packs` is `pub` so the `droidsmith-pack-lint` binary (which links
/// against this crate as `droidsmith_lib`) can reach the loader + lint
/// types. Tauri-internal callers go through `crate::packs` as usual.
pub mod packs;
mod quirks;

use commands::{
    apply_action, explain_failure, heartbeat, journal_list, journal_undo, list_devices,
    list_packages, plan_action,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Hook panics into a file log before we touch the Tauri runtime, so a
    // panic during Tauri init still leaves a forensic trail.
    let log_dir = diagnostics::fallback_log_dir();
    diagnostics::install_panic_hook(log_dir.clone());

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            heartbeat,
            list_devices,
            list_packages,
            plan_action,
            apply_action,
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
