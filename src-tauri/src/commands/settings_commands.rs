//! Domain-scoped Tauri command boundary.

use super::*;

pub(crate) fn settings_app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, CommandError> {
    app.path().app_data_dir().map_err(|error| CommandError {
        code: "no_app_data_dir",
        message: error.to_string(),
    })
}

/// Load the fixed backend-owned settings document and perform the one-time
/// import of bounded legacy renderer values when needed.
#[tauri::command]
#[specta::specta]
pub async fn initialize_settings(
    app: tauri::AppHandle,
    legacy: settings::LegacySettingsImport,
) -> Result<settings::SettingsLoadResult, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::initialize(&app_data_dir, legacy)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_settings_language(
    app: tauri::AppHandle,
    language: settings::SettingsLanguage,
) -> Result<settings::SettingsSnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::set_language(&app_data_dir, language)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings_mirror_preset(
    app: tauri::AppHandle,
    device_identity: String,
) -> Result<Option<settings::MirrorPreset>, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::get_mirror_preset(
            &app_data_dir,
            &device_identity,
        )?)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_settings_mirror_preset(
    app: tauri::AppHandle,
    device_identity: String,
    preset: settings::MirrorPreset,
) -> Result<settings::SettingsSnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::set_mirror_preset(
            &app_data_dir,
            &device_identity,
            preset,
        )?)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn reset_settings_mirror_preset(
    app: tauri::AppHandle,
    device_identity: String,
) -> Result<settings::SettingsSnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::reset_mirror_preset(
            &app_data_dir,
            &device_identity,
        )?)
    })
    .await
}

/// Record the selected device's current build fingerprint and report whether it
/// differs from the last one Droidsmith saw for it (R-087). A changed
/// fingerprint means the device was updated (OTA) since it was last used, so the
/// renderer can prompt a debloat-drift review. Devices without a verified
/// fingerprint are treated as unchanged.
#[tauri::command]
#[specta::specta]
pub async fn observe_device_fingerprint(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
) -> Result<settings::FingerprintObservation, CommandError> {
    let Some(fingerprint) = target.build_fingerprint.clone() else {
        return Ok(settings::FingerprintObservation {
            changed: false,
            previous: None,
        });
    };
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::record_device_fingerprint(
            &app_data_dir,
            &target.serial,
            &fingerprint,
        )?)
    })
    .await
}

/// Return the persisted wireless-endpoint history and the opt-in
/// reconnect-on-launch flag so the renderer can offer one-click reconnect.
#[tauri::command]
#[specta::specta]
pub async fn list_wireless_history(
    app: tauri::AppHandle,
) -> Result<settings::WirelessHistorySnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::list_wireless_history(&app_data_dir)?)).await
}

/// Remove one endpoint from the wireless history.
#[tauri::command]
#[specta::specta]
pub async fn forget_wireless_endpoint(
    app: tauri::AppHandle,
    host: String,
    port: u16,
) -> Result<settings::WirelessHistorySnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::forget_wireless_endpoint(
            &app_data_dir,
            &host,
            port,
        )?)
    })
    .await
}

/// Persist the opt-in "reconnect known wireless devices on launch" preference.
#[tauri::command]
#[specta::specta]
pub async fn set_wireless_auto_reconnect(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<settings::WirelessHistorySnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::set_wireless_auto_reconnect(
            &app_data_dir,
            enabled,
        )?)
    })
    .await
}

/// List the saved Logcat query presets for the global scope and, when a device
/// identity is supplied, that device's scope. Only query definitions are
/// stored; captured log lines never enter the settings document.
#[tauri::command]
#[specta::specta]
pub async fn list_logcat_queries(
    app: tauri::AppHandle,
    device_identity: Option<String>,
) -> Result<settings::LogcatQueryLibrary, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::list_logcat_queries(
            &app_data_dir,
            device_identity.as_deref(),
        )?)
    })
    .await
}

/// Persist the full ordered list of Logcat query presets for one scope. This
/// single write covers create, rename, duplicate, reorder, and delete; an empty
/// list clears the scope. Each preset is validated (including a linear-time
/// regex guard) before it is written.
#[tauri::command]
#[specta::specta]
pub async fn save_logcat_queries(
    app: tauri::AppHandle,
    scope: settings::LogcatQueryScope,
    device_identity: Option<String>,
    queries: Vec<settings::LogcatQuery>,
) -> Result<settings::LogcatQueryLibrary, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || {
        Ok(settings::save_logcat_queries(
            &app_data_dir,
            scope,
            device_identity.as_deref(),
            queries,
        )?)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn reset_settings(
    app: tauri::AppHandle,
    scope: settings::SettingsScope,
) -> Result<settings::SettingsSnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::reset(&app_data_dir, scope)?)).await
}

/// Export only a named settings scope through a backend-issued save grant.
/// The internal settings path never crosses IPC.
#[tauri::command]
#[specta::specta]
pub async fn export_settings(
    app: tauri::AppHandle,
    grants: tauri::State<'_, PathGrantStore>,
    scope: settings::SettingsScope,
    path_grant: String,
) -> Result<settings::SettingsExportResult, CommandError> {
    let destination = grants.consume(&path_grant, HostPathPurpose::SettingsExport)?;
    let app_data_dir = settings_app_data_dir(&app)?;
    let result =
        spawn_blocking_operation(move || Ok(settings::export(&app_data_dir, scope, &destination)?))
            .await?;
    grants.record_produced(&result.path)?;
    Ok(result)
}

/// Parse and validate a portable settings document, then return only a
/// redacted change summary plus an opaque, short-lived import id.
#[tauri::command]
#[specta::specta]
pub async fn preview_settings_import(
    app: tauri::AppHandle,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
) -> Result<settings::SettingsImportPreview, CommandError> {
    let source = grants.consume(&path_grant, HostPathPurpose::SettingsImport)?;
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::preview_import(&app_data_dir, &source)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn apply_settings_import(
    app: tauri::AppHandle,
    import_id: String,
    mode: settings::SettingsImportMode,
) -> Result<settings::SettingsImportResult, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::apply_import(&app_data_dir, &import_id, mode)?))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_settings_import_backup(
    app: tauri::AppHandle,
) -> Result<settings::SettingsSnapshot, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    spawn_blocking_operation(move || Ok(settings::restore_import_backup(&app_data_dir)?)).await
}

#[tauri::command]
#[specta::specta]
pub fn has_settings_import_backup(app: tauri::AppHandle) -> Result<bool, CommandError> {
    let app_data_dir = settings_app_data_dir(&app)?;
    Ok(settings::import_backup_available(&app_data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_command_state_round_trip_keeps_the_selected_language() {
        let dir = std::env::temp_dir().join(format!(
            "droidsmith-settings-command-{}-{}",
            std::process::id(),
            crate::time::iso_utc_now().replace([':', '.'], "-")
        ));
        let initial = settings::initialize(&dir, Default::default()).unwrap();
        assert_eq!(initial.settings.language, None);
        let updated = settings::set_language(&dir, settings::SettingsLanguage::Ru).unwrap();
        assert_eq!(updated.language, Some(settings::SettingsLanguage::Ru));
        let loaded = settings::initialize(&dir, Default::default()).unwrap();
        assert_eq!(
            loaded.settings.language,
            Some(settings::SettingsLanguage::Ru)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
