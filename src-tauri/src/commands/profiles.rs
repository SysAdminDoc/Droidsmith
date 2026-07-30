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
    let rows = if let Some(user_id) = android_user {
        let packages = adb::list_packages(&transport, &target, adb::PackageFilter::All, user_id)?;
        profile_preview_rows(&profile, &target, user_id, &packages)
    } else {
        Vec::new()
    };
    Ok(ProfilePreview {
        source_version,
        profile,
        migration,
        compatible: compatibility_issues.is_empty(),
        compatibility_issues,
        android_user,
        rows,
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
    let requests = profile::requests_for(
        profile,
        target,
        user_id,
        actions::ConfirmationSource::ProfilePreview,
    );
    profile
        .actions
        .iter()
        .cloned()
        .zip(requests.into_iter().map(actions::plan))
        .map(|(action, plan)| {
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
