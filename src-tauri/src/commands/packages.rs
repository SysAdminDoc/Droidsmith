//! Domain-scoped Tauri command boundary.

use super::*;

#[tauri::command]
#[specta::specta]
pub fn list_packages(
    target: adb::DeviceTarget,
    filter: adb::PackageFilter,
    #[allow(non_snake_case)] userId: u32,
) -> Result<adb::PackageListing, adb::TransportError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    adb::validate_device_target(&transport, &target)?;
    adb::list_packages_with_capability(&transport, &target, filter, userId)
}

/// Probe optional package-manager actions from this exact device. Unsupported
/// commands are returned as capabilities, not errors, so the renderer can hide
/// them instead of offering a broken action.
#[tauri::command]
#[specta::specta]
pub fn get_package_action_capabilities(
    target: adb::DeviceTarget,
) -> Result<adb::PackageActionCapabilities, adb::TransportError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    adb::validate_device_target(&transport, &target)?;
    Ok(adb::package_action_capabilities(&transport, &target))
}

/// Lazily enrich one package row after it approaches the renderer viewport.
/// The domain service bounds concurrent pulls and validates a fresh APK
/// size/timestamp before consulting its process-local cache.
#[tauri::command]
#[specta::specta]
pub async fn get_package_metadata(
    target: adb::DeviceTarget,
    package: String,
    #[allow(non_snake_case)] userId: u32,
) -> Result<apk_metadata::AppPackageMetadata, CommandError> {
    validate_metadata_package(&package)?;
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)
        .map(PathBuf::from)?;
    spawn_blocking_operation(move || {
        let transport = adb::ShellTransport::new(path);
        adb::validate_device_target(&transport, &target)?;
        Ok(apk_metadata::load_package_metadata(
            &transport, &target, userId, &package,
        )?)
    })
    .await
}

/// Enumerate Android users on a device so the renderer can offer an
/// explicit `--user` target for package workflows.
#[tauri::command]
#[specta::specta]
pub fn list_users(target: adb::DeviceTarget) -> Result<Vec<adb::AndroidUser>, adb::TransportError> {
    let resolution = adb::locate_adb();
    let path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(path);
    adb::validate_device_target(&transport, &target)?;
    adb::list_users(&transport, &target)
}

pub(crate) fn validate_metadata_package(package: &str) -> Result<(), CommandError> {
    if valid_package_name(package) {
        Ok(())
    } else {
        Err(CommandError {
            code: "invalid_package",
            message: "invalid package id".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_validation_accepts_hyphenated_package_names() {
        assert!(validate_metadata_package("com.example.my-app").is_ok());
    }

    #[test]
    fn metadata_validation_rejects_flag_like_package_names() {
        let error = validate_metadata_package("--user").unwrap_err();
        assert_eq!(error.code, "invalid_package");
    }
}
