mod adb;
mod commands;
mod diagnostics;

use commands::heartbeat;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Hook panics into a file log before we touch the Tauri runtime, so a
    // panic during Tauri init still leaves a forensic trail.
    if let Some(dir) = diagnostics::fallback_log_dir() {
        diagnostics::install_panic_hook(dir);
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![heartbeat]);

    let context = tauri::generate_context!();
    if let Err(e) = builder.run(context) {
        diagnostics::fatal_dialog(
            "Droidsmith failed to start",
            &format!("{e}\n\nA crash log was written to the Droidsmith app-data folder."),
        );
        std::process::exit(1);
    }
}
