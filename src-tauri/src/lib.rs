mod adb;
mod commands;

use commands::heartbeat;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![heartbeat])
        .run(tauri::generate_context!())
        .expect("error while running droidsmith");
}
