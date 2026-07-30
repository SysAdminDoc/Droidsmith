//! Domain-scoped Tauri command boundary.

use super::*;

/// Install an APK or split-package archive on a device. Single APKs retain the
/// direct `adb install -r` path; APKS/XAPK/APKM archives are committed through
/// an atomic PackageInstaller session.
#[tauri::command]
#[specta::specta]
pub async fn install_apk(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    options: install::InstallOptions,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<install::InstallPackageResult, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let validated_path = grants.consume(&path_grant, HostPathPurpose::InstallOpen)?;
    let retry_path = validated_path.clone();
    let app_data_dir = app.path().app_data_dir().map_err(|error| CommandError {
        code: "no_app_data_dir",
        message: error.to_string(),
    })?;
    let sink = operations::channel_sink(on_event);
    let mut result = spawn_blocking_operation(move || {
        Ok(install::install_package(
            &transport,
            &target,
            &validated_path,
            &app_data_dir,
            &operation_id,
            options,
            sink,
        )?)
    })
    .await?;
    if result
        .failure
        .as_ref()
        .and_then(|failure| failure.suggested_override)
        .is_some()
        && !options.override_confirmed
    {
        result.retry_path_grant = grants
            .issue(&retry_path, HostPathPurpose::InstallOpen)
            .ok()
            .map(|grant| grant.id);
    }
    Ok(result)
}

/// Pull an APK from the device to a local path.
#[tauri::command]
#[specta::specta]
pub async fn extract_apk(
    target: adb::DeviceTarget,
    grants: tauri::State<'_, PathGrantStore>,
    remote_path: String,
    path_grant: String,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<HostArtifact, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let output_target = grants.consume(&path_grant, HostPathPurpose::ExtractApkSave)?;
    let remote = validate_remote_path(&remote_path)?;
    let selector = target.adb_selector();
    let adb_path = transport.adb_path.clone();
    let sink = operations::channel_sink(on_event);
    let artifact = spawn_blocking_operation(move || {
        let staged = StagedArtifact::new(&output_target)?;
        let mut args = selector;
        args.extend([
            "pull".to_string(),
            remote,
            staged.path().display().to_string(),
        ]);
        let output = operations::run_process(
            &adb_path,
            &args,
            std::time::Duration::from_secs(120),
            &operation_id,
            "Extracting APK",
            sink,
        )?;
        completed_adb_output(output, "adb pull")?;
        Ok(staged.commit(ArtifactKind::Apk)?)
    })
    .await?;
    grants.record_produced(&artifact.local_path)?;
    Ok(artifact)
}
