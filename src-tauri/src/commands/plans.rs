//! Domain-scoped Tauri command boundary.

use super::*;

/// Synthesise an ADB action without running it. Pure: this is the
/// preview surface the confirmation dialog renders before the user
/// commits.
#[tauri::command]
#[specta::specta]
pub fn plan_action(
    mut request: actions::ActionRequest,
) -> Result<actions::PlannedAction, CommandError> {
    if !matches!(
        request.kind,
        actions::ActionKind::Suspend
            | actions::ActionKind::Unsuspend
            | actions::ActionKind::Disable
            | actions::ActionKind::Enable
            | actions::ActionKind::Archive
            | actions::ActionKind::RequestUnarchive
            | actions::ActionKind::UninstallForUser
            | actions::ActionKind::ClearData
            | actions::ActionKind::ForceStop
    ) {
        return Err(CommandError {
            code: "invalid_action_kind",
            message: "use the dedicated audited planner for this operation kind".to_string(),
        });
    }
    request.pack_context = None;
    request.context = actions::ActionContext {
        confirmation_source: actions::ConfirmationSource::AppsPreview,
        ..Default::default()
    };
    Ok(actions::plan(request))
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct UninstallRecoveryAssessment {
    pub package: String,
    pub evidence: adb::packages::UninstallRecoveryEvidence,
}

/// Answer "can I get this back?" for every package in a proposed
/// uninstall-for-user set, before any of them is touched.
///
/// The whole set is enumerated here rather than one call per package from the
/// renderer: a fan-out of N target-bound calls is the stale-completion shape
/// the target lifecycle contract exists to prevent, and a partial answer to
/// this question is worse than none.
#[tauri::command]
#[specta::specta]
pub fn assess_uninstall_recovery(
    target: adb::DeviceTarget,
    #[allow(non_snake_case)] userId: u32,
    packages: Vec<String>,
) -> Result<Vec<UninstallRecoveryAssessment>, CommandError> {
    if packages.is_empty() || packages.len() > MAX_ACTION_BATCH_ITEMS {
        return Err(CommandError {
            code: "invalid_recovery_assessment",
            message: format!("assess between 1 and {MAX_ACTION_BATCH_ITEMS} packages at a time"),
        });
    }
    let transport = validated_transport(&target)?;
    Ok(packages
        .into_iter()
        .map(|package| UninstallRecoveryAssessment {
            evidence: adb::packages::assess_uninstall_recovery(
                &transport, &target, userId, &package,
            ),
            package,
        })
        .collect())
}

/// Build one reviewed, reversible package-action plan for multiple packages.
/// Every item is bound to the same immutable device target, Android user, and
/// action kind; destructive or conditionally-reversible kinds stay on the
/// single-item path.
#[tauri::command]
#[specta::specta]
pub fn plan_action_batch(
    requests: Vec<actions::ActionRequest>,
) -> Result<BatchActionPlan, CommandError> {
    if !(2..=MAX_ACTION_BATCH_ITEMS).contains(&requests.len()) {
        return Err(CommandError {
            code: "invalid_action_batch",
            message: format!(
                "a package batch must contain between 2 and {MAX_ACTION_BATCH_ITEMS} items"
            ),
        });
    }
    let first = requests.first().expect("length checked");
    if !matches!(
        first.kind,
        actions::ActionKind::Suspend
            | actions::ActionKind::Unsuspend
            | actions::ActionKind::Disable
            | actions::ActionKind::Enable
            | actions::ActionKind::Archive
            | actions::ActionKind::RequestUnarchive
    ) {
        return Err(CommandError {
            code: "invalid_action_kind",
            message: "batch actions must have a losslessly reversible inverse".to_string(),
        });
    }
    let target = first.target.clone();
    let serial = first.serial.clone();
    let user_id = first.user_id;
    let kind = first.kind;
    let mut packages = HashSet::with_capacity(requests.len());
    let mut plans = Vec::with_capacity(requests.len());
    for mut request in requests {
        if request.serial != serial
            || request.target != target
            || request.user_id != user_id
            || request.kind != kind
        {
            return Err(CommandError {
                code: "mixed_action_batch",
                message: "every batch item must use the same device target, Android user, and action kind"
                    .to_string(),
            });
        }
        validate_package_arg(&request.package)?;
        if !packages.insert(request.package.clone()) {
            return Err(CommandError {
                code: "duplicate_batch_package",
                message: format!(
                    "package {} appears more than once in the batch",
                    request.package
                ),
            });
        }
        request.pack_context = None;
        request.context = actions::ActionContext {
            confirmation_source: actions::ConfirmationSource::AppsPreview,
            ..Default::default()
        };
        plans.push(actions::plan(request));
    }
    let description = batch_action_description(kind, plans.len(), user_id);
    Ok(BatchActionPlan { plans, description })
}

pub(crate) fn batch_action_description(
    kind: actions::ActionKind,
    count: usize,
    user_id: u32,
) -> String {
    let action = match kind {
        actions::ActionKind::Suspend => "Suspend",
        actions::ActionKind::Unsuspend => "Unsuspend",
        actions::ActionKind::Disable => "Disable",
        actions::ActionKind::Enable => "Enable",
        actions::ActionKind::Archive => "Archive",
        actions::ActionKind::RequestUnarchive => "Request unarchive for",
        _ => "Apply action to",
    };
    format!("{action} {count} packages for Android user {user_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> adb::DeviceTarget {
        adb::DeviceTarget {
            serial: "plans-test".to_string(),
            transport_id: Some(3),
            connection_generation: 2,
            model: Some("Test".to_string()),
            product: Some("test".to_string()),
            device: Some("test".to_string()),
            build_fingerprint: Some("build/test".to_string()),
            transport_kind: adb::DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        }
    }

    fn request(kind: actions::ActionKind, package: &str) -> actions::ActionRequest {
        actions::ActionRequest {
            serial: "plans-test".to_string(),
            target: target(),
            package: package.to_string(),
            kind,
            user_id: 10,
            pack_context: None,
            context: Default::default(),
        }
    }

    #[test]
    fn plan_action_rejects_unreviewed_shell_operations() {
        let error = plan_action(request(actions::ActionKind::Shell, "ignored")).unwrap_err();
        assert_eq!(error.code, "invalid_action_kind");
    }

    #[test]
    fn batch_description_is_canonical_and_user_scoped() {
        assert_eq!(
            batch_action_description(actions::ActionKind::Disable, 3, 10),
            "Disable 3 packages for Android user 10"
        );
        let batch = plan_action_batch(vec![
            request(actions::ActionKind::Disable, "com.example.one"),
            request(actions::ActionKind::Disable, "com.example.two"),
        ])
        .unwrap();
        assert_eq!(batch.description, "Disable 2 packages for Android user 10");
    }
}
