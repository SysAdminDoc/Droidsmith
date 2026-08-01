//! Domain-scoped Tauri command boundary.

use super::*;

/// List all debloat packs from the app's `packs/` resource directory.
/// A bundled pack file that failed to load, with a stable code and a
/// human-readable message the UI can show and the user can copy.
#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PackLoadError {
    /// File name (not full path — no host paths leak to the renderer).
    pub file: String,
    /// Stable code: `pack_read`, `pack_parse`, `pack_validate`, or
    /// `pack_duplicate_id`.
    pub code: &'static str,
    pub message: String,
}

/// Result of enumerating bundled packs: the healthy packs plus per-file
/// errors for any that failed. A broken file no longer disappears
/// silently — it surfaces as an error the user can act on.
#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct PackListing {
    pub packs: Vec<crate::packs::PackCandidate>,
    pub errors: Vec<PackLoadError>,
}

#[derive(specta::Type, Debug, Clone, Deserialize)]
pub struct PlanPackRequest {
    pub target: adb::DeviceTarget,
    pub user_id: u32,
    pub pack_id: String,
    pub revision: u32,
    pub selected: Vec<String>,
    #[serde(default)]
    pub override_compatibility: bool,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct PlannedPack {
    pub pack_id: String,
    pub revision: u32,
    pub assessment: crate::packs::PackAssessment,
    pub selected_ids: Vec<String>,
    pub plans: Vec<actions::PlannedAction>,
    pub skipped: Vec<crate::packs::PackEntryAssessment>,
}

pub(crate) fn pack_error_to_load_error(
    file: String,
    err: &crate::packs::PackError,
) -> PackLoadError {
    use crate::packs::PackError;
    let code = match err {
        PackError::Read { .. } => "pack_read",
        PackError::Parse { .. } => "pack_parse",
        PackError::Validate { .. } => "pack_validate",
    };
    PackLoadError {
        file,
        code,
        message: err.to_string(),
    }
}

/// Returns packs that parse and lint cleanly, plus a per-file error for
/// each broken file so a packaging defect is visible instead of looking
/// like an empty pack list.
pub(crate) fn load_runtime_packs(
    packs_dir: &std::path::Path,
) -> Result<(Vec<crate::packs::Pack>, Vec<PackLoadError>), CommandError> {
    if !packs_dir.is_dir() {
        return Ok((Vec::new(), Vec::new()));
    }
    let entries = std::fs::read_dir(packs_dir).map_err(|e| CommandError {
        code: "io_error",
        message: format!("could not read packs directory: {e}"),
    })?;
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if file.starts_with('_') {
            continue;
        }
        if path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml")
        {
            match crate::packs::load(&path) {
                Ok(pack) => loaded.push((file, pack)),
                Err(err) => errors.push(pack_error_to_load_error(file, &err)),
            }
        }
    }
    let id_counts = loaded.iter().fold(
        std::collections::HashMap::<String, usize>::new(),
        |mut counts, (_, pack)| {
            *counts.entry(pack.id.clone()).or_default() += 1;
            counts
        },
    );
    let mut packs = Vec::new();
    for (file, pack) in loaded {
        if id_counts.get(pack.id.as_str()).copied().unwrap_or_default() > 1 {
            errors.push(PackLoadError {
                file,
                code: "pack_duplicate_id",
                message: format!(
                    "stable pack id {:?} is declared by more than one runtime pack",
                    pack.id
                ),
            });
        } else {
            packs.push(pack);
        }
    }
    packs.sort_by(|a, b| a.id.cmp(&b.id));
    errors.sort_by(|a, b| a.file.cmp(&b.file));
    Ok((packs, errors))
}

/// Absolute path of the app-data directory that holds user-imported packs.
/// Imported pack files are named `<pack-id>.yaml` (the id is validated
/// kebab-case, so the name can never traverse out of this directory).
pub(crate) fn user_packs_dir(app: &tauri::AppHandle) -> Result<PathBuf, CommandError> {
    Ok(settings_app_data_dir(app)?.join("packs"))
}

/// A merged pack set: each pack paired with an `imported` flag (`true` when it
/// came from the user-imported app-data directory), plus per-file load errors.
type MergedPacks = (Vec<(crate::packs::Pack, bool)>, Vec<PackLoadError>);

/// Load bundled packs (from the resource directory) merged with any packs the
/// user has imported into the app-data `packs/` directory. Bundled ids win: an
/// imported pack whose id shadows a bundled one surfaces as a load error rather
/// than silently overriding the shipped pack. The `bool` is `true` for
/// imported packs.
pub(crate) fn load_all_packs(
    bundled_dir: &std::path::Path,
    user_dir: &std::path::Path,
) -> Result<MergedPacks, CommandError> {
    let (bundled, mut errors) = load_runtime_packs(bundled_dir)?;
    let (imported, imported_errors) = load_runtime_packs(user_dir)?;
    errors.extend(imported_errors);

    let bundled_ids: std::collections::HashSet<String> =
        bundled.iter().map(|pack| pack.id.clone()).collect();
    let mut packs: Vec<(crate::packs::Pack, bool)> =
        bundled.into_iter().map(|pack| (pack, false)).collect();
    for pack in imported {
        if bundled_ids.contains(&pack.id) {
            errors.push(PackLoadError {
                file: format!("{}.yaml", pack.id),
                code: "pack_duplicate_id",
                message: format!(
                    "imported pack id {:?} shadows a bundled pack; remove the import",
                    pack.id
                ),
            });
        } else {
            packs.push((pack, true));
        }
    }
    packs.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    errors.sort_by(|a, b| a.file.cmp(&b.file));
    Ok((packs, errors))
}

/// Package ids that are valid debloat targets: actually installed for the user.
/// `PackageFilter::All` also surfaces archived apps and uninstalled-for-user
/// "retained" remnants (via the `pm list packages -u` pass); neither can be
/// disabled. Counting them made the pack assessment mark them Ready, plan a
/// disable, and then fail verification with "package disappeared after apply"
/// because the post-state query (which excludes `-u`) correctly reports them as
/// not installed.
pub(crate) fn debloat_target_ids(
    packages: Vec<adb::AppPackage>,
) -> std::collections::HashSet<String> {
    packages
        .into_iter()
        .filter(|package| !package.archived && !package.retained)
        .map(|package| package.package)
        .collect()
}

pub(crate) fn pack_context(
    transport: &adb::ShellTransport,
    target: &adb::DeviceTarget,
    user_id: u32,
) -> Result<crate::packs::DevicePackContext, CommandError> {
    adb::validate_device_target(transport, target)?;
    let users = adb::list_users(transport, target)?;
    let user = users
        .iter()
        .find(|user| user.id == user_id)
        .ok_or(CommandError {
            code: "pack_user_missing",
            message: format!("Android user {user_id} is not available"),
        })?;
    let info = adb::get_device_info(transport, target)?;
    let packages = adb::list_packages(transport, target, adb::PackageFilter::All, user_id)?;
    let system_uid_packages = packages
        .iter()
        .filter(|package| {
            !package.archived && !package.retained && package.uses_android_system_uid()
        })
        .map(|package| package.package.clone())
        .collect();
    let installed_packages = debloat_target_ids(packages);
    Ok(crate::packs::DevicePackContext {
        manufacturer: info.manufacturer,
        model: info.model,
        build_fingerprint: info.build_fingerprint,
        api_level: info.sdk_level.and_then(|value| value.parse().ok()),
        user_id,
        user_current: user.current,
        installed_packages,
        system_uid_packages,
    })
}

#[tauri::command]
#[specta::specta]
pub fn list_packs(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    #[allow(non_snake_case)] userId: u32,
) -> Result<PackListing, CommandError> {
    let resource_dir = app.path().resource_dir().map_err(|e| CommandError {
        code: "no_resource_dir",
        message: e.to_string(),
    })?;
    let user_dir = user_packs_dir(&app)?;

    let (packs, errors) = load_all_packs(&resource_dir.join("packs"), &user_dir)?;
    let transport = validated_transport(&target)?;
    let context = pack_context(&transport, &target, userId)?;
    let packs = packs
        .into_iter()
        .map(|(pack, imported)| crate::packs::PackCandidate {
            assessment: crate::packs::assess(&pack, &context),
            pack,
            imported,
        })
        .collect();
    Ok(PackListing { packs, errors })
}

/// Metadata returned after a debloat pack is imported from a local file.
#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ImportedPack {
    pub id: String,
    pub name: String,
    pub revision: u32,
    /// SHA-256 of the imported file, computed at import time. Surfaced so the
    /// user can record it and re-import the same bytes with a pin later.
    pub sha256: String,
    /// Number of package entries the pack offers to remove.
    pub packages: usize,
}

/// Import a debloat pack from a user-selected local file through a one-shot
/// native read grant. This is the network-free alternative to remote-pack
/// fetching (R-095): it reuses the audited host-path grant model, optionally
/// verifies a caller-supplied SHA-256 pin, schema-validates and lints the
/// bytes, rejects ids that shadow a bundled pack, and persists the file to the
/// app-data `packs/` directory so it appears in the picker on the next load.
#[tauri::command]
#[specta::specta]
pub fn import_pack(
    app: tauri::AppHandle,
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    #[allow(non_snake_case)] expectedSha256: Option<String>,
) -> Result<ImportedPack, CommandError> {
    let source = grants.consume(&path_grant, HostPathPurpose::PackImportOpen)?;

    let actual_sha256 = crate::fs_util::sha256_file(&source).map_err(|error| CommandError {
        code: "pack_read",
        message: format!("could not read the selected pack file: {error}"),
    })?;
    if let Some(expected) = expectedSha256 {
        let expected = expected.trim().to_ascii_lowercase();
        if !expected.is_empty() {
            if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(CommandError {
                    code: "pack_sha256_invalid",
                    message: "expected SHA-256 must be 64 hexadecimal characters".to_string(),
                });
            }
            if expected != actual_sha256 {
                return Err(CommandError {
                    code: "pack_sha256_mismatch",
                    message: format!(
                        "pack SHA-256 does not match: expected {expected}, got {actual_sha256}"
                    ),
                });
            }
        }
    }

    let pack = crate::packs::load(&source).map_err(|error| {
        let load_error = pack_error_to_load_error(String::new(), &error);
        CommandError {
            code: load_error.code,
            message: load_error.message,
        }
    })?;

    // `packs::load` lints the id to lowercase kebab-case, so `<id>.yaml` is a
    // safe filename; guard again for defense in depth before touching the FS.
    if !crate::packs::valid_pack_id(&pack.id) {
        return Err(CommandError {
            code: "pack_validate",
            message: format!("pack id {:?} is not a valid identifier", pack.id),
        });
    }

    let resource_dir = app.path().resource_dir().map_err(|e| CommandError {
        code: "no_resource_dir",
        message: e.to_string(),
    })?;
    let (bundled, _) = load_runtime_packs(&resource_dir.join("packs"))?;
    if bundled
        .iter()
        .any(|bundled_pack| bundled_pack.id == pack.id)
    {
        return Err(CommandError {
            code: "pack_id_conflicts_bundled",
            message: format!(
                "a bundled pack already uses id {:?}; imported packs must have a unique id",
                pack.id
            ),
        });
    }

    let user_dir = user_packs_dir(&app)?;
    std::fs::create_dir_all(&user_dir).map_err(|error| CommandError {
        code: "io_error",
        message: format!("could not create the imported-packs directory: {error}"),
    })?;
    let destination = user_dir.join(format!("{}.yaml", pack.id));
    std::fs::copy(&source, &destination).map_err(|error| CommandError {
        code: "io_error",
        message: format!("could not store the imported pack: {error}"),
    })?;

    Ok(ImportedPack {
        id: pack.id.clone(),
        name: pack.name.clone(),
        revision: pack.revision,
        sha256: actual_sha256,
        packages: pack.packages.len(),
    })
}

/// Remove a previously-imported debloat pack by its stable id. Bundled packs
/// live in the read-only resource directory and are never touched. Returns
/// `true` when a file was deleted, `false` when no import with that id existed.
#[tauri::command]
#[specta::specta]
pub fn remove_imported_pack(
    app: tauri::AppHandle,
    #[allow(non_snake_case)] packId: String,
) -> Result<bool, CommandError> {
    if !crate::packs::valid_pack_id(&packId) {
        return Err(CommandError {
            code: "pack_id_invalid",
            message: "invalid pack id".to_string(),
        });
    }
    let destination = user_packs_dir(&app)?.join(format!("{packId}.yaml"));
    match std::fs::remove_file(&destination) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CommandError {
            code: "io_error",
            message: format!("could not remove the imported pack: {error}"),
        }),
    }
}

/// Result of exporting a device's captured debloat state to a pack file.
#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ExportedDevicePack {
    pub pack_id: String,
    pub packages: usize,
    pub artifact: crate::fs_util::HostArtifact,
}

/// Capture the selected device's currently disabled/archived/uninstalled
/// packages and write them to a schema-valid debloat pack YAML through a
/// one-shot native save grant (R-098). The result round-trips through
/// `import_pack`, so "what I removed on this phone" can be re-applied to another
/// device after an OTA or factory reset.
#[tauri::command]
#[specta::specta]
pub fn export_device_pack(
    grants: tauri::State<'_, PathGrantStore>,
    target: adb::DeviceTarget,
    #[allow(non_snake_case)] userId: u32,
    path_grant: String,
) -> Result<ExportedDevicePack, CommandError> {
    let destination = grants.consume(&path_grant, HostPathPurpose::PackExportSave)?;
    let transport = validated_transport(&target)?;
    adb::validate_device_target(&transport, &target)?;
    let info = adb::get_device_info(&transport, &target)?;
    let packages = adb::list_packages(&transport, &target, adb::PackageFilter::All, userId)?;

    let removed: Vec<crate::packs::RemovedPackage> = packages
        .into_iter()
        .filter_map(|package| {
            let kind = if package.archived {
                crate::packs::RemovedKind::Archived
            } else if package.retained {
                crate::packs::RemovedKind::Uninstalled
            } else if !package.enabled {
                crate::packs::RemovedKind::Disabled
            } else {
                return None;
            };
            Some(crate::packs::RemovedPackage {
                id: package.package,
                kind,
            })
        })
        .collect();

    let context = crate::packs::DeviceExportContext {
        manufacturer: info.manufacturer,
        model: info.model,
        api_level: info.sdk_level.and_then(|value| value.parse().ok()),
        user_id: userId,
        date: crate::time::iso_utc_now().chars().take(10).collect(),
    };
    let pack =
        crate::packs::from_device_state(&removed, &context).map_err(|message| CommandError {
            code: "pack_export_empty",
            message,
        })?;
    let yaml = crate::packs::to_yaml(&pack).map_err(|error| CommandError {
        code: "pack_export_serialize",
        message: error.to_string(),
    })?;

    let staged =
        crate::fs_util::StagedArtifact::new(&destination).map_err(|error| CommandError {
            code: "io_error",
            message: error.to_string(),
        })?;
    std::fs::write(staged.path(), yaml).map_err(|error| CommandError {
        code: "io_error",
        message: format!("could not write the exported pack: {error}"),
    })?;
    let artifact = staged
        .commit(crate::fs_util::ArtifactKind::AnyFile)
        .map_err(|error| CommandError {
            code: "io_error",
            message: error.to_string(),
        })?;
    grants.record_produced(&artifact.local_path)?;

    Ok(ExportedDevicePack {
        pack_id: pack.id,
        packages: pack.packages.len(),
        artifact,
    })
}

/// Statically analyze a local APK file the user selects through a one-shot
/// native read grant (R-097). Fully offline and device-free: parses the binary
/// manifest, DEX headers, signing artifacts, and a per-entry size breakdown.
#[tauri::command]
#[specta::specta]
pub async fn analyze_apk(
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
) -> Result<crate::apk_analysis::ApkAnalysis, CommandError> {
    let path = grants.consume(&path_grant, HostPathPurpose::ApkAnalyzeOpen)?;
    spawn_blocking_operation(move || {
        crate::apk_analysis::analyze(&path).map_err(|error| CommandError {
            code: error.code(),
            message: error.to_string(),
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub fn plan_pack(
    app: tauri::AppHandle,
    request: PlanPackRequest,
) -> Result<PlannedPack, CommandError> {
    if request.selected.is_empty() {
        return Err(CommandError {
            code: "pack_selection_empty",
            message: "select at least one pack entry".to_string(),
        });
    }
    let resource_dir = app.path().resource_dir().map_err(|e| CommandError {
        code: "no_resource_dir",
        message: e.to_string(),
    })?;
    let (packs, _) = load_all_packs(&resource_dir.join("packs"), &user_packs_dir(&app)?)?;
    let pack = packs
        .into_iter()
        .map(|(pack, _)| pack)
        .find(|pack| pack.id == request.pack_id)
        .ok_or(CommandError {
            code: "pack_not_found",
            message: format!("debloat pack {:?} is not available", request.pack_id),
        })?;
    if pack.revision != request.revision {
        return Err(CommandError {
            code: "pack_revision_changed",
            message: format!(
                "pack {} changed from revision {} to {}; review it again",
                pack.id, request.revision, pack.revision
            ),
        });
    }
    let transport = validated_transport(&request.target)?;
    let context = pack_context(&transport, &request.target, request.user_id)?;
    let assessment = crate::packs::assess(&pack, &context);
    if assessment.override_required && !request.override_compatibility {
        return Err(CommandError {
            code: "pack_compatibility_override_required",
            message: format!(
                "pack {} is {:?} for this device/user; review checks and explicitly accept the override",
                pack.id, assessment.status
            ),
        });
    }
    let selected =
        crate::packs::expand_dependencies(&pack, request.selected).map_err(|message| {
            CommandError {
                code: "pack_selection_invalid",
                message,
            }
        })?;
    let selected_ids: Vec<String> = pack
        .packages
        .iter()
        .filter(|entry| selected.contains(&entry.id))
        .map(|entry| entry.id.clone())
        .collect();
    let status_by_id: std::collections::HashMap<&str, &crate::packs::PackEntryAssessment> =
        assessment
            .entries
            .iter()
            .map(|entry| (entry.id.as_str(), entry))
            .collect();
    let mut plans = Vec::new();
    let mut skipped = Vec::new();
    for entry in pack
        .packages
        .iter()
        .filter(|entry| selected.contains(&entry.id))
    {
        let Some(support) = status_by_id.get(entry.id.as_str()) else {
            skipped.push(missing_pack_assessment(entry));
            continue;
        };
        if support.status != crate::packs::PackEntryStatus::Ready {
            skipped.push((*support).clone());
            continue;
        }
        plans.push(actions::plan(actions::ActionRequest {
            serial: request.target.serial.clone(),
            target: request.target.clone(),
            package: entry.id.clone(),
            kind: actions::ActionKind::Disable,
            user_id: request.user_id,
            pack_context: Some(actions::PackActionContext {
                pack_id: pack.id.clone(),
                revision: pack.revision,
                provenance_source: pack.provenance.source.clone(),
                provenance_license: pack.provenance.license.clone(),
                compatibility_status: format!("{:?}", assessment.status).to_lowercase(),
                override_accepted: request.override_compatibility,
            }),
            context: actions::ActionContext {
                confirmation_source: actions::ConfirmationSource::DebloatPreview,
                ..Default::default()
            },
        }));
    }
    Ok(PlannedPack {
        pack_id: pack.id,
        revision: pack.revision,
        assessment,
        selected_ids,
        plans,
        skipped,
    })
}

fn missing_pack_assessment(entry: &crate::packs::PackEntry) -> crate::packs::PackEntryAssessment {
    crate::packs::PackEntryAssessment {
        id: entry.id.clone(),
        status: crate::packs::PackEntryStatus::Unsupported,
        detail: Some(
            "pack assessment omitted this entry; the action was skipped safely".to_string(),
        ),
        effective_removal: entry.removal,
        shared_system_uid: false,
    }
}

#[cfg(test)]
mod invariant_tests {
    use super::*;

    #[test]
    fn a_missing_pack_assessment_becomes_an_explicit_skip() {
        let entry = crate::packs::PackEntry {
            id: "com.example.missing".to_string(),
            description: "fixture".to_string(),
            removal: crate::packs::RemovalLevel::Recommended,
            labels: Vec::new(),
            depends_on: Vec::new(),
            needed_by: Vec::new(),
        };
        let skipped = missing_pack_assessment(&entry);
        assert_eq!(skipped.id, entry.id);
        assert_eq!(skipped.status, crate::packs::PackEntryStatus::Unsupported);
        assert!(skipped.detail.unwrap().contains("skipped safely"));
    }
}
