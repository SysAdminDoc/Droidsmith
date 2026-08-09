//! Domain-scoped Tauri command boundary.

use super::*;

/// Apply a previously-planned action and record it in the per-device
/// journal. Returns the freshly-written journal entry.
#[tauri::command]
#[specta::specta]
pub fn apply_action(
    app: tauri::AppHandle,
    mut plan: actions::PlannedAction,
) -> Result<ApplyActionResult, CommandError> {
    if plan.request.context.batch_id.is_some() {
        return Err(CommandError {
            code: "batch_command_required",
            message: "backend-issued batch plans must use the batch apply command".to_string(),
        });
    }
    if plan.request.kind == actions::ActionKind::RestoreExistingForUser {
        return Err(CommandError {
            code: "journal_undo_required",
            message: "install-existing recovery can only run from a verified journal undo"
                .to_string(),
        });
    }
    let (transport, transport_override) = privileged_transport(&plan.request.target)?;
    plan.request.context.transport_override = transport_override;

    let identity = DeviceIdentity::from_target(&plan.request.target);
    // Serialize intent → device mutation → terminal outcome per device. The
    // durable intent is written and synced before `actions::apply` runs.
    let dir = journal_dir(&app)?;
    let result = journal::with_journal(&dir, &identity, |journal| {
        execute_journaled(journal, &transport, plan, None)
    })?;
    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub fn apply_action_batch(
    app: tauri::AppHandle,
    mut batch: BatchActionPlan,
) -> Result<BatchActionResult, CommandError> {
    validate_action_batch_plan(&batch)?;
    let first = batch.plans.first().expect("validated non-empty batch");
    let target = first.request.target.clone();
    let identity = DeviceIdentity::from_target(&target);
    let (transport, transport_override) = privileged_transport(&target)?;
    let batch_id = next_batch_id();
    for plan in &mut batch.plans {
        plan.request.context.transport_override = transport_override;
        plan.request.context.batch_id = Some(batch_id.clone());
    }

    let dir = journal_dir(&app)?;
    let items = journal::with_journal(&dir, &identity, |journal| {
        execute_batch_plans(journal, &transport, batch.plans, None)
    })?;
    Ok(BatchActionResult { batch_id, items })
}

pub(crate) fn validate_action_batch_plan(batch: &BatchActionPlan) -> Result<(), CommandError> {
    if !(2..=MAX_ACTION_BATCH_ITEMS).contains(&batch.plans.len()) {
        return Err(CommandError {
            code: "invalid_action_batch",
            message: format!(
                "a package batch must contain between 2 and {MAX_ACTION_BATCH_ITEMS} items"
            ),
        });
    }
    let first = batch.plans.first().expect("length checked");
    let target = &first.request.target;
    let serial = &first.request.serial;
    let user_id = first.request.user_id;
    let kind = first.request.kind;
    if !matches!(
        kind,
        actions::ActionKind::Suspend
            | actions::ActionKind::Unsuspend
            | actions::ActionKind::Disable
            | actions::ActionKind::Enable
            | actions::ActionKind::Archive
            | actions::ActionKind::RequestUnarchive
    ) || batch.description != batch_action_description(kind, batch.plans.len(), user_id)
    {
        return Err(CommandError {
            code: "invalid_action_batch",
            message: "batch metadata does not match its canonical action plans".to_string(),
        });
    }
    let mut packages = HashSet::with_capacity(batch.plans.len());
    for plan in &batch.plans {
        actions::validate_plan(plan)?;
        if &plan.request.serial != serial
            || &plan.request.target != target
            || plan.request.user_id != user_id
            || plan.request.kind != kind
            || plan.request.pack_context.is_some()
            || plan.request.context.confirmation_source != actions::ConfirmationSource::AppsPreview
            || plan.request.context.batch_id.is_some()
            || !plan.before_state.is_empty()
            || !packages.insert(plan.request.package.clone())
        {
            return Err(CommandError {
                code: "mixed_action_batch",
                message: "batch plans must be unique, renderer-reviewed, and bound to one target/user/action"
                    .to_string(),
            });
        }
    }
    Ok(())
}

pub(crate) fn execute_batch_plans(
    journal: &mut Journal,
    transport: &dyn AdbTransport,
    plans: Vec<actions::PlannedAction>,
    undo_ids: Option<Vec<u64>>,
) -> Result<Vec<BatchActionItemResult>, CommandError> {
    if let Some(ids) = undo_ids.as_ref() {
        if ids.len() != plans.len() {
            return Err(CommandError {
                code: "invalid_action_batch",
                message: "batch undo ids do not match their inverse plans".to_string(),
            });
        }
    }
    let mut items = Vec::with_capacity(plans.len());
    for (index, mut plan) in plans.into_iter().enumerate() {
        let package = plan.request.package.clone();
        let before_state = actions::capture_state(transport, &plan.request);
        if !actions::reversible_batch_before_state(plan.request.kind, &before_state) {
            items.push(BatchActionItemResult {
                package,
                entry: None,
                stdout: String::new(),
                error: Some(format!(
                    "verified package state {before_state} is not a reversible starting state for {:?}",
                    plan.request.kind
                )),
            });
            continue;
        }
        plan.before_state = before_state;
        let incident_id = plan.incident_id.clone();
        let undoes = undo_ids.as_ref().map(|ids| ids[index]);
        match execute_journaled(journal, transport, plan, undoes) {
            Ok(result) => items.push(BatchActionItemResult {
                package,
                entry: Some(result.entry),
                stdout: result.stdout,
                error: None,
            }),
            Err(error) if error.code == "package_action_failed" => {
                let entry = journal
                    .entries()
                    .iter()
                    .rev()
                    .find(|entry| entry.applied.plan.incident_id == incident_id)
                    .cloned();
                items.push(BatchActionItemResult {
                    package,
                    entry,
                    stdout: String::new(),
                    error: Some(error.message),
                });
            }
            Err(error) => return Err(error),
        }
    }
    Ok(items)
}

pub(crate) fn next_batch_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("batch-{nanos:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Export a redacted package snapshot before a reviewed destructive batch.
/// The renderer supplies requested intents, while the backend captures all
/// device/package state and writes through a one-shot native path grant.
#[tauri::command]
#[specta::specta]
pub fn export_recovery_baseline(
    target: adb::DeviceTarget,
    #[allow(non_snake_case)] userId: u32,
    actions: Vec<BaselineActionInput>,
    pack: Option<BaselinePack>,
    path_grant: String,
    grants: tauri::State<'_, PathGrantStore>,
) -> Result<HostArtifact, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let path = grants.consume(&path_grant, HostPathPurpose::RecoveryBaselineSave)?;
    let users = adb::list_users(&transport, &target)?;
    if !users.iter().any(|user| user.id == userId) {
        return Err(CommandError {
            code: "recovery_baseline_user_missing",
            message: format!("Android user {userId} is not available"),
        });
    }
    let packages = adb::list_packages(&transport, &target, adb::PackageFilter::All, userId)?;
    let baseline = recovery_baseline::build(&target, userId, pack, &packages, actions, iso_now())?;
    let artifact = recovery_baseline::save(&path, &baseline)?;
    grants.record_produced(&artifact.local_path)?;
    Ok(artifact)
}

/// Load and compare a baseline without mutating the device. Returned plans are
/// canonical but remain inert until the renderer shows the diff and explicitly
/// submits individual plans through `apply_action`.
#[tauri::command]
#[specta::specta]
pub fn inspect_recovery_baseline(
    target: adb::DeviceTarget,
    path_grant: String,
    // `round_trip` selects which half of the OTA round trip to plan: restore
    // to the baseline before updating, or re-apply the recorded actions after.
    round_trip: recovery_baseline::BaselineRoundTrip,
    grants: tauri::State<'_, PathGrantStore>,
) -> Result<RecoveryBaselineDiff, CommandError> {
    let (transport, _) = privileged_transport(&target)?;
    let path = grants.consume(&path_grant, HostPathPurpose::RecoveryBaselineOpen)?;
    let baseline = recovery_baseline::load(&path)?;
    let users = adb::list_users(&transport, &target)?;
    let packages = if users.iter().any(|user| user.id == baseline.android_user) {
        adb::list_packages(
            &transport,
            &target,
            adb::PackageFilter::All,
            baseline.android_user,
        )?
    } else {
        Vec::new()
    };
    Ok(recovery_baseline::inspect_round_trip(
        baseline, &target, &users, &packages, round_trip,
    )?)
}

#[tauri::command]
#[specta::specta]
pub fn journal_list(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
) -> Result<Vec<JournalEntry>, CommandError> {
    validate_serial_arg(&target.serial)?;
    // A journal is keyed on serial *and* build fingerprint, so reading one
    // without a fingerprint could only answer for a device identity no
    // mutation path can produce. Say so instead of rendering an empty history
    // that looks like lost data.
    let identity = journaled_identity(&target)?;
    let dir = journal_dir(&app)?;
    journal::with_journal(&dir, &identity, |journal| {
        Ok::<_, CommandError>(journal.entries().to_vec())
    })
}

/// Resolve the persistence identity for a device whose journal is about to be
/// opened. Fails closed when the build fingerprint is unknown — the same
/// precondition `adb::validate_device_target` enforces before any mutation.
fn journaled_identity(target: &adb::DeviceTarget) -> Result<DeviceIdentity, CommandError> {
    let identity = DeviceIdentity::from_target(target);
    if identity.fingerprint().is_none() {
        return Err(CommandError {
            code: "device_identity_unverified",
            message:
                "this device has not reported a build fingerprint yet; reconnect and authorize it before opening its history"
                    .to_string(),
        });
    }
    Ok(identity)
}

/// Undo entry `entry_id` in `serial`'s journal. Returns the new
/// undo-entry. Fails if the original action is irreversible
/// (unverified uninstall, clear-data, force-stop).
#[tauri::command]
#[specta::specta]
pub fn journal_undo(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    entry_id: u64,
) -> Result<JournalEntry, CommandError> {
    let serial = target.serial.clone();
    validate_serial_arg(&serial)?;
    let identity = journaled_identity(&target)?;
    let (transport, transport_override) = privileged_transport(&target)?;

    // Hold the per-device lock across the reversibility check, the inverse
    // ADB call, and the undo record so two undos of the same entry cannot
    // both pass the check and double-apply.
    let dir = journal_dir(&app)?;
    let entry = journal::with_journal(&dir, &identity, |journal| {
        let mut undo_request = journal::undo_request_for(journal, entry_id).ok_or(CommandError {
            code: "not_reversible",
            message: format!(
                "journal entry {entry_id} either doesn't exist, is already undone, or its action kind cannot be reversed"
            ),
        })?;

        undo_request.target = target.clone();
        undo_request.context.transport_override = transport_override;

        let plan = actions::plan(undo_request);
        execute_journaled(journal, &transport, plan, Some(entry_id)).map(|result| result.entry)
    })?;
    Ok(entry)
}

/// Undo every still-active successful item from one backend-issued batch.
/// Reversibility is proven for the complete remaining set before the first
/// inverse runs; device-level failures are then reported per package without
/// hiding successful inverses.
#[tauri::command]
#[specta::specta]
pub fn journal_undo_batch(
    app: tauri::AppHandle,
    target: adb::DeviceTarget,
    batch_id: String,
) -> Result<BatchActionResult, CommandError> {
    if !actions::valid_batch_id(&batch_id) {
        return Err(CommandError {
            code: "invalid_batch_id",
            message: "batch id is malformed".to_string(),
        });
    }
    let serial = target.serial.clone();
    validate_serial_arg(&serial)?;
    let identity = journaled_identity(&target)?;
    let (transport, transport_override) = privileged_transport(&target)?;
    let dir = journal_dir(&app)?;
    let items = journal::with_journal(&dir, &identity, |journal| {
        let originals = journal
            .entries()
            .iter()
            .filter(|entry| {
                entry.undoes.is_none()
                    && entry.outcome == journal::JournalOutcome::Succeeded
                    && entry.applied.plan.request.context.batch_id.as_deref()
                        == Some(batch_id.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        if originals.is_empty() {
            return Err(CommandError {
                code: "batch_not_found",
                message: format!("no successful journal entries belong to {batch_id}"),
            });
        }

        let remaining = originals
            .into_iter()
            .filter(|entry| entry.undone_by.is_none())
            .collect::<Vec<_>>();
        if remaining.is_empty() {
            return Err(CommandError {
                code: "batch_already_undone",
                message: format!("every successful item in {batch_id} is already undone"),
            });
        }

        let mut plans = Vec::with_capacity(remaining.len());
        let mut ids = Vec::with_capacity(remaining.len());
        for entry in remaining {
            let mut request = journal::undo_request_for(journal, entry.id).ok_or(CommandError {
                code: "batch_not_reversible",
                message: format!(
                    "journal entry {} in {batch_id} cannot be safely reversed as part of the batch",
                    entry.id
                ),
            })?;
            request.serial = serial.clone();
            request.target = target.clone();
            request.context.transport_override = transport_override;
            request.context.batch_id = Some(batch_id.clone());
            plans.push(actions::plan(request));
            ids.push(entry.id);
        }
        execute_batch_plans(journal, &transport, plans, Some(ids))
    })?;
    Ok(BatchActionResult { batch_id, items })
}

#[tauri::command]
#[specta::specta]
pub fn get_device_info(target: adb::DeviceTarget) -> Result<adb::DeviceInfo, CommandError> {
    let transport = validated_transport(&target)?;
    Ok(adb::get_device_info(&transport, &target)?)
}

/// R-082: read the curated system-settings allow-list (`settings get`). Read
/// only; safe over any authorized transport.
#[tauri::command]
#[specta::specta]
pub fn list_device_settings(
    target: adb::DeviceTarget,
) -> Result<Vec<adb::DeviceSetting>, CommandError> {
    let transport = validated_transport(&target)?;
    Ok(adb::read_device_settings(&transport, &target)?)
}

/// R-082: write one allow-listed setting (`settings put`). The `setting_id` and
/// `value` are validated against the catalog before anything is shelled out, so
/// arbitrary keys or out-of-range values are rejected. Runs over the privileged
/// transport boundary because it mutates device state; the previous value is
/// returned so the renderer can offer a one-click revert.
#[tauri::command]
#[specta::specta]
pub fn put_device_setting(
    target: adb::DeviceTarget,
    setting_id: String,
    value: String,
) -> Result<adb::DeviceSettingChange, CommandError> {
    let spec = adb::validate_write(&setting_id, &value).map_err(|message| CommandError {
        code: "invalid_setting",
        message,
    })?;
    let normalized = value.trim().to_string();
    let (transport, _override) = privileged_transport(&target)?;

    let previous = adb::read_device_settings(&transport, &target)
        .ok()
        .and_then(|settings| {
            settings
                .into_iter()
                .find(|setting| setting.id == setting_id)
                .and_then(|setting| setting.value)
        });

    let argv = adb::put_argv(spec, &normalized);
    transport.shell_target(&target, &argv)?;

    Ok(adb::DeviceSettingChange {
        id: setting_id,
        namespace: adb::spec_namespace(spec),
        key: adb::spec_key(spec).to_string(),
        previous_value: previous,
        new_value: normalized.clone(),
        command: adb::command_preview(spec, &normalized),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::transport::MockTransport;

    #[test]
    fn batch_validation_rejects_empty_plans_before_transport_access() {
        let error = validate_action_batch_plan(&BatchActionPlan {
            plans: Vec::new(),
            description: String::new(),
        })
        .unwrap_err();
        assert_eq!(error.code, "invalid_action_batch");
    }

    #[test]
    fn batch_execution_rejects_mismatched_undo_ids_with_fake_transport() {
        let path = std::env::temp_dir().join(format!(
            "droidsmith-command-batch-{}-{}",
            std::process::id(),
            next_batch_id()
        ));
        let identity = DeviceIdentity::new("command-test", Some("build/test"));
        let mut journal = Journal::open(&path, &identity).unwrap();
        let error = execute_batch_plans(
            &mut journal,
            &MockTransport::new(),
            Vec::new(),
            Some(vec![1]),
        )
        .unwrap_err();
        assert_eq!(error.code, "invalid_action_batch");
    }

    #[test]
    fn batch_ids_are_nonempty_and_unique() {
        let first = next_batch_id();
        let second = next_batch_id();
        assert_ne!(first, second);
        assert!(first.starts_with("batch-"));
    }
}
