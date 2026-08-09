//! Domain-scoped Tauri command boundary.

use super::*;

#[derive(specta::Type, Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfilePreviewStatus {
    Ready,
    AlreadyMatches,
    Missing,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ProfilePreviewRow {
    pub action: profile::ProfileAction,
    pub plan: actions::PlannedAction,
    pub current_state: String,
    pub expected_state: String,
    pub status: ProfilePreviewStatus,
    pub reason: String,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ProfilePreview {
    pub source_version: String,
    pub profile: profile::Profile,
    pub migration: Option<profile::ProfileMigration>,
    pub compatible: bool,
    pub compatibility_issues: Vec<String>,
    pub android_user: Option<u32>,
    pub rows: Vec<ProfilePreviewRow>,
    /// What each schema-v3 filter step selected from the live inventory,
    /// including steps that selected nothing.
    pub filter_matches: Vec<profile::FilterMatch>,
    /// Packages a filter could not decide, and therefore excluded. Shown, not
    /// dropped.
    pub filter_exclusions: Vec<profile::FilterExclusion>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct FleetRunResult {
    pub artifact: HostArtifact,
    pub report: fleet_report::FleetReportView,
}

/// Render a saved fleet run report through a one-shot native read grant.
///
/// Deliberately the only command in this file that never constructs a
/// transport: opening a report is an offline, read-only operation, and a
/// report is often reviewed on a machine that has none of its devices
/// attached. Raw serials never cross the boundary — the view names every
/// device by digest (see [`fleet_report::view`]).
///
/// Resuming a report is the CLI's `run --retry-from`; this command renders the
/// same document rather than reimplementing the selection rules.
#[tauri::command]
#[specta::specta]
pub fn inspect_fleet_report(
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
) -> Result<fleet_report::FleetReportView, CommandError> {
    let path = grants.consume(&path_grant, HostPathPurpose::FleetReportOpen)?;
    let loaded = fleet_report::load(&path)?;
    Ok(fleet_report::view(&loaded.report))
}

/// Plan and optionally apply a profile across every connected device. The
/// fleet runner owns screening and report construction; this command only
/// supplies the GUI's explicit apply choice and persists the resulting report
/// through a purpose-scoped native save grant.
#[tauri::command]
#[specta::specta]
pub async fn run_profile_fleet(
    grants: tauri::State<'_, PathGrantStore>,
    profile: profile::Profile,
    apply: bool,
    path_grant: String,
    operation_id: String,
    on_event: tauri::ipc::Channel<OperationEvent>,
) -> Result<FleetRunResult, CommandError> {
    let issues = profile::lint(&profile);
    if !issues.is_empty() {
        return Err(CommandError {
            code: "profile_invalid",
            message: issues.join("; "),
        });
    }
    let output_target = grants.consume(&path_grant, HostPathPurpose::FleetReportSave)?;
    let sink = operations::channel_sink(on_event);
    let result = spawn_blocking_operation(move || {
        let adb_path = adb::locate_adb().path.ok_or_else(|| CommandError {
            code: "adb_not_found",
            message: "adb binary not found".to_string(),
        })?;
        let transport = adb::ShellTransport::new(&adb_path);
        let report = crate::fleet::run_all(&transport, &profile, apply, false, &operation_id, sink)
            .map_err(|error| CommandError {
                code: error.code(),
                message: error.to_string(),
            })?;
        let view = fleet_report::view(&report);
        let bytes = serde_json::to_vec_pretty(&report).map_err(|error| CommandError {
            code: "fleet_report_serialize_failed",
            message: error.to_string(),
        })?;
        let staged = StagedArtifact::new(&output_target)?;
        std::fs::write(staged.path(), bytes)?;
        let artifact = staged.commit(ArtifactKind::AnyFile)?;
        Ok(FleetRunResult {
            artifact,
            report: view,
        })
    })
    .await?;
    grants.record_produced(&result.artifact.local_path)?;
    Ok(result)
}

/// Import a profile through a one-shot native read grant and build a complete,
/// read-only device/user/package diff. Legacy v1 input is returned only as an
/// explicit migration candidate and cannot be applied as-is.
#[tauri::command]
#[specta::specta]
pub fn inspect_profile(
    grants: tauri::State<'_, PathGrantStore>,
    target: adb::DeviceTarget,
    path_grant: String,
) -> Result<ProfilePreview, CommandError> {
    let path = grants.consume(&path_grant, HostPathPurpose::ProfileOpen)?;
    let document = profile::inspect(&path)?;
    let (source_version, profile, migration) = match document {
        profile::ProfileDocument::Current { profile } => (profile.version.clone(), profile, None),
        // v2: runnable as-is, and the upgrade rides along so the reviewer can
        // save a v3 copy without re-opening the file through a fresh grant.
        profile::ProfileDocument::UpgradeAvailable { profile, migration } => {
            (profile.version.clone(), profile, Some(migration))
        }
        profile::ProfileDocument::MigrationAvailable { migration } => (
            migration.from_version.clone(),
            migration.profile.clone(),
            Some(migration),
        ),
    };

    let resolution = adb::locate_adb();
    let adb_path = resolution
        .path
        .as_ref()
        .ok_or(adb::TransportError::AdbNotFound)?;
    let transport = adb::ShellTransport::new(adb_path);
    adb::validate_device_target(&transport, &target)?;
    let info = adb::get_device_info(&transport, &target)?;
    let users = adb::list_users(&transport, &target)?;
    let mut compatibility_issues = profile::device_match_issues(
        &profile,
        &target.serial,
        info.manufacturer.as_deref(),
        info.model.as_deref(),
        info.sdk_level
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok()),
    );
    let android_user = match profile::resolve_user(&profile, &users) {
        Ok(user_id) => Some(user_id),
        Err(mut issues) => {
            compatibility_issues.append(&mut issues);
            None
        }
    };
    let (rows, filter_matches, filter_exclusions) = if let Some(user_id) = android_user {
        let packages = adb::list_packages(&transport, &target, adb::PackageFilter::All, user_id)?;
        // Resolved twice on purpose: `profile_preview_rows` needs the plans,
        // this needs the selection report, and resolution is pure and cheap
        // against an inventory already in memory.
        let resolved = profile::resolve(
            &profile,
            &target,
            user_id,
            &packages,
            actions::ConfirmationSource::ProfilePreview,
        );
        (
            profile_preview_rows(&profile, &target, user_id, &packages),
            resolved.matches,
            resolved.exclusions,
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    Ok(ProfilePreview {
        source_version,
        profile,
        migration,
        compatible: compatibility_issues.is_empty(),
        compatibility_issues,
        android_user,
        rows,
        filter_matches,
        filter_exclusions,
    })
}

/// Validate and atomically export a current v2 profile through a purpose-
/// scoped native save grant. This is also the only GUI path that finalizes a
/// reviewed v1 migration.
#[tauri::command]
#[specta::specta]
pub fn save_profile(
    grants: tauri::State<'_, PathGrantStore>,
    path_grant: String,
    profile: profile::Profile,
) -> Result<HostArtifact, CommandError> {
    let path = grants.consume(&path_grant, HostPathPurpose::ProfileSave)?;
    let artifact = profile::save(&path, &profile)?;
    grants.record_produced(&artifact.local_path)?;
    Ok(artifact)
}

pub(crate) fn profile_preview_rows(
    profile: &profile::Profile,
    target: &adb::DeviceTarget,
    user_id: u32,
    packages: &[adb::AppPackage],
) -> Vec<ProfilePreviewRow> {
    // Resolve first: a v3 filter step expands into one row per matched
    // package, so rows are no longer 1:1 with `profile.actions`.
    let resolved = profile::resolve(
        profile,
        target,
        user_id,
        packages,
        actions::ConfirmationSource::ProfilePreview,
    );
    resolved
        .requests
        .into_iter()
        .map(|resolved| {
            let source = &profile.actions[resolved.action_index - 1];
            // Report the action as resolved for this package: the concrete
            // package it will run against, with the predicate that chose it
            // still attached so the review shows why.
            let action = profile::ProfileAction {
                kind: source.kind,
                package: resolved.request.package.clone(),
                filter: resolved.filter.clone(),
                note: source.note.clone(),
            };
            let plan = actions::plan(resolved.request);
            let package = packages
                .iter()
                .find(|candidate| candidate.package == action.package);
            let current_state = match package {
                Some(package) if package.archived => "archived",
                Some(package) if package.enabled => "enabled",
                Some(_) => "disabled",
                None => "missing",
            }
            .to_string();
            let expected_state = match action.kind {
                actions::ActionKind::Disable => "disabled",
                actions::ActionKind::Enable | actions::ActionKind::RestoreExistingForUser => {
                    "enabled"
                }
                actions::ActionKind::UninstallForUser => "uninstalled_for_user",
                actions::ActionKind::ClearData => "data_cleared",
                actions::ActionKind::ForceStop => "stopped",
                _ => "reviewed_action",
            }
            .to_string();
            let (status, reason) = match (package, action.kind) {
                (Some(package), _) if package.archived => (
                    ProfilePreviewStatus::Missing,
                    "package is archived; restore it from Apps before applying profile actions"
                        .to_string(),
                ),
                (None, actions::ActionKind::UninstallForUser) => (
                    ProfilePreviewStatus::AlreadyMatches,
                    "package is already absent for this user".to_string(),
                ),
                (None, actions::ActionKind::RestoreExistingForUser) => (
                    ProfilePreviewStatus::Ready,
                    "restore will ask Android to install the retained system package".to_string(),
                ),
                (None, _) => (
                    ProfilePreviewStatus::Missing,
                    "package is not installed for this user".to_string(),
                ),
                (Some(package), actions::ActionKind::Disable) if !package.enabled => (
                    ProfilePreviewStatus::AlreadyMatches,
                    "package is already disabled".to_string(),
                ),
                (Some(package), actions::ActionKind::Enable) if package.enabled => (
                    ProfilePreviewStatus::AlreadyMatches,
                    "package is already enabled".to_string(),
                ),
                _ => (
                    ProfilePreviewStatus::Ready,
                    "canonical action is ready for explicit review".to_string(),
                ),
            };
            ProfilePreviewRow {
                action,
                plan,
                current_state,
                expected_state,
                status,
                reason,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> adb::DeviceTarget {
        adb::DeviceTarget {
            serial: "profiles-test".to_string(),
            transport_id: Some(4),
            connection_generation: 1,
            model: Some("Test".to_string()),
            product: Some("test".to_string()),
            device: Some("test".to_string()),
            build_fingerprint: Some("build/test".to_string()),
            transport_kind: adb::DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        }
    }

    #[test]
    fn profile_preview_rows_report_ready_and_already_matching_states() {
        let profile = profile::Profile {
            name: "test".to_string(),
            version: profile::PROFILE_SCHEMA_VERSION.to_string(),
            description: String::new(),
            device: Default::default(),
            user: Default::default(),
            actions: vec![
                profile::ProfileAction {
                    kind: actions::ActionKind::Disable,
                    package: "com.example.ready".to_string(),
                    filter: String::new(),
                    note: String::new(),
                },
                profile::ProfileAction {
                    kind: actions::ActionKind::Disable,
                    package: "com.example.disabled".to_string(),
                    filter: String::new(),
                    note: String::new(),
                },
            ],
        };
        let packages = vec![
            adb::AppPackage {
                package: "com.example.ready".to_string(),
                enabled: true,
                system: false,
                apk_path: None,
                uid: None,
                installer: None,
                archived: false,
                retained: false,
            },
            adb::AppPackage {
                package: "com.example.disabled".to_string(),
                enabled: false,
                system: true,
                apk_path: Some("/system/app/Test.apk".to_string()),
                uid: Some(1_000),
                installer: None,
                archived: false,
                retained: false,
            },
        ];
        let rows = profile_preview_rows(&profile, &target(), 0, &packages);
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0].status, ProfilePreviewStatus::Ready));
        assert!(matches!(
            rows[1].status,
            ProfilePreviewStatus::AlreadyMatches
        ));
        assert_eq!(rows[0].expected_state, "disabled");
    }
}
