//! Shared fleet profile execution used by the CLI and the GUI.
//!
//! Fleet selection is intentionally performed before any device-specific
//! probing. That makes an unauthorized, offline, or untrusted TCP transport a
//! reportable skip instead of a reason to abort the whole batch. The report
//! builder is also shared so the GUI writes the same schema-2 document that
//! `droidsmith-cli run --all-devices --json` produces.

use crate::adb::{self, actions, AdbTransport, Device, DeviceTarget, ShellTransport};
use crate::device_identity::DeviceIdentity;
use crate::fleet_report::{
    ActionStatus, DeviceErrorOutput, DeviceRunResult, FleetRunReport, RunApplyOutput, RunOutput,
    RunPlanOutput, FLEET_REPORT_SCHEMA_VERSION,
};
use crate::journal;
use crate::operations::{self, EventSink, RegisteredOperation};
use crate::profile;

#[derive(Debug, thiserror::Error)]
pub enum FleetRunError {
    #[error("could not refresh devices: {0}")]
    DeviceList(String),
    #[error("{0}")]
    Operation(#[from] operations::OperationError),
}

impl FleetRunError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::DeviceList(_) => "device_list_failed",
            Self::Operation(error) => match error {
                operations::OperationError::InvalidId(_) => "invalid_operation_id",
                operations::OperationError::DuplicateId(_) => "operation_already_running",
                _ => "operation_failed",
            },
        }
    }
}

/// Screening verdict for one discovered device in a fleet run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FleetScreen {
    /// Actionable and transport-authorized; ready for fingerprint binding.
    Eligible(Device),
    /// Excluded before any device I/O, with a user-facing reason.
    Skipped { serial: String, reason: String },
}

/// Pure fleet screen shared by the CLI and GUI. Unauthorized/offline devices
/// and override-required transports are skipped rather than aborting the run.
pub fn screen_fleet_devices(
    devices: Vec<Device>,
    allow_unsafe_transport: bool,
) -> Vec<FleetScreen> {
    devices
        .into_iter()
        .map(|device| {
            if !device.state.is_actionable() {
                return FleetScreen::Skipped {
                    serial: device.serial.clone(),
                    reason: format!("device is not actionable ({:?})", device.state),
                };
            }
            if device.transport_kind.requires_override() && !allow_unsafe_transport {
                return FleetScreen::Skipped {
                    serial: device.serial.clone(),
                    reason: format!(
                        "uses an unauthenticated {} transport; pass --allow-unsafe-transport to include it",
                        device.transport_kind.label()
                    ),
                };
            }
            FleetScreen::Eligible(device)
        })
        .collect()
}

/// Bind a discovered device to an operable, fingerprinted target.
pub fn finalize_target(
    transport: &dyn AdbTransport,
    mut device: Device,
) -> Result<DeviceTarget, String> {
    if !device.state.is_actionable() {
        return Err(format!(
            "device serial {:?} is not actionable ({:?})",
            device.serial, device.state
        ));
    }
    let fingerprint = transport
        .shell_target(&device.target(), &["getprop", "ro.build.fingerprint"])
        .map_err(|error| format!("could not identify the device build: {error}"))?
        .trim()
        .to_string();
    if fingerprint.is_empty() {
        return Err("device did not report a build fingerprint".to_string());
    }
    device.build_fingerprint = Some(fingerprint);
    Ok(device.target())
}

/// Run a profile across every discovered device and return the schema-2
/// report. `allow_unsafe_transport` is false for the GUI; the CLI can opt in
/// explicitly with its existing `--allow-unsafe-transport` flag.
pub fn run_all(
    transport: &ShellTransport,
    profile: &profile::Profile,
    apply: bool,
    allow_unsafe_transport: bool,
    operation_id: &str,
    sink: EventSink,
) -> Result<FleetRunReport, FleetRunError> {
    let operation = RegisteredOperation::new(operation_id, "Running profile across fleet", sink)?;
    let mut devices = transport
        .list_devices()
        .map_err(|error| FleetRunError::DeviceList(error.to_string()))?;
    adb::observe_connection_generations(&mut devices);
    let screened = screen_fleet_devices(devices, allow_unsafe_transport);
    let total = screened.len();
    let mut results = Vec::with_capacity(total);
    let mut success = true;

    for (index, screen) in screened.into_iter().enumerate() {
        let serial = match &screen {
            FleetScreen::Eligible(device) => device.serial.clone(),
            FleetScreen::Skipped { serial, .. } => serial.clone(),
        };
        if operation.is_cancelled() {
            success = false;
            results.push(DeviceRunResult::Skipped {
                device_serial: serial,
                reason: "fleet run cancelled before this device was processed".to_string(),
            });
            continue;
        }

        operation.progress(format!("Device {}/{}: {}", index + 1, total, serial));
        match screen {
            FleetScreen::Skipped { serial, reason } => {
                success = false;
                results.push(DeviceRunResult::Skipped {
                    device_serial: serial,
                    reason,
                });
            }
            FleetScreen::Eligible(device) => {
                let mut target = match finalize_target(transport, device) {
                    Ok(target) => target,
                    Err(error) => {
                        success = false;
                        results.push(DeviceRunResult::Error(DeviceErrorOutput {
                            device_serial: serial,
                            code: "device_unavailable".to_string(),
                            message: error,
                        }));
                        continue;
                    }
                };
                if allow_unsafe_transport && target.transport_kind.requires_override() {
                    target.untrusted_transport_override = true;
                }
                match run_profile_on_target(transport, profile, &target, &serial, apply, &operation)
                {
                    Ok(output) => {
                        if !output.success {
                            success = false;
                        }
                        results.push(DeviceRunResult::Ran(Box::new(output)));
                    }
                    Err(error) => {
                        success = false;
                        results.push(DeviceRunResult::Error(error));
                    }
                }
            }
        }
    }

    if results.is_empty() {
        success = false;
    }
    let report = FleetRunReport {
        schema_version: FLEET_REPORT_SCHEMA_VERSION,
        command: "run".to_string(),
        mode: "all_devices".to_string(),
        apply,
        generated_at: crate::time::iso_utc_now(),
        profile: crate::fleet_report::describe_profile(profile),
        lineage: None,
        devices: results,
        success,
    };
    if operation.is_cancelled() {
        operation.cancelled("Fleet run cancelled; the partial report is ready");
    } else {
        operation.finish(if report.success {
            "Fleet run completed successfully"
        } else {
            "Fleet run completed with skips or errors"
        });
    }
    Ok(report)
}

fn run_profile_on_target(
    transport: &dyn AdbTransport,
    profile: &profile::Profile,
    target: &DeviceTarget,
    serial: &str,
    apply: bool,
    operation: &RegisteredOperation,
) -> Result<RunOutput, DeviceErrorOutput> {
    let err = |code: &str, message: String| DeviceErrorOutput {
        device_serial: serial.to_string(),
        code: code.to_string(),
        message,
    };

    operation.progress(format!("{}: probing device", serial));
    let info = adb::get_device_info(transport, target)
        .map_err(|error| err("device_probe_failed", error.to_string()))?;
    let compatibility = profile::device_match_issues(
        profile,
        serial,
        info.manufacturer.as_deref(),
        info.model.as_deref(),
        info.sdk_level
            .as_deref()
            .and_then(|value| value.parse().ok()),
    );
    if !compatibility.is_empty() {
        return Err(err("profile_incompatible", compatibility.join("; ")));
    }
    operation.progress(format!("{}: resolving Android user", serial));
    let users = adb::list_users(transport, target)
        .map_err(|error| err("user_probe_failed", error.to_string()))?;
    let user_id = profile::resolve_user(profile, &users)
        .map_err(|issues| err("profile_user_unavailable", issues.join("; ")))?;
    operation.progress(format!("{}: loading package inventory", serial));
    let inventory = adb::list_packages(transport, target, adb::PackageFilter::All, user_id)
        .map_err(|error| err("package_probe_failed", error.to_string()))?;
    let resolved = profile::resolve(
        profile,
        target,
        user_id,
        &inventory,
        actions::ConfirmationSource::CliApply,
    );
    let resolved_exclusions = resolved.exclusions;
    let mut plans = resolved
        .requests
        .into_iter()
        .map(|resolved| actions::plan(resolved.request))
        .collect::<Vec<_>>();
    for plan in &mut plans {
        plan.before_state = actions::capture_state(transport, &plan.request);
    }
    let plan_output = plans
        .iter()
        .enumerate()
        .map(|(index, plan)| RunPlanOutput {
            index: index + 1,
            package: plan.request.package.clone(),
            action: plan.request.kind,
            user_id: plan.request.user_id,
            before_state: plan.before_state.clone(),
            description: plan.description.clone(),
            adb_args: plan
                .request
                .target
                .adb_selector()
                .into_iter()
                .chain(["shell".to_string()])
                .chain(plan.args.clone())
                .collect(),
        })
        .collect::<Vec<_>>();

    let mut results = Vec::new();
    let mut success = true;
    if apply {
        let journal_dir = journal::default_journal_dir()
            .map_err(|error| err("journal_unavailable", error.to_string()))?;
        let identity = DeviceIdentity::from_target(target);
        for (index, plan) in plans.into_iter().enumerate() {
            if operation.is_cancelled() {
                success = false;
                break;
            }
            let package = plan.request.package.clone();
            let kind = plan.request.kind;
            operation.progress(format!(
                "{}: applying action {}/{} ({})",
                serial,
                index + 1,
                plan_output.len(),
                package
            ));
            let now = crate::time::iso_utc_now();
            let result = journal::with_journal(&journal_dir, &identity, |journal| {
                journal.execute(plan, None, &now, |plan| {
                    actions::apply(transport, plan, &crate::time::iso_utc_now())
                })
            });
            match result {
                Ok(_) => results.push(RunApplyOutput {
                    index: index + 1,
                    package,
                    action: kind,
                    status: ActionStatus::Applied,
                    error: None,
                }),
                Err(journal::ExecuteError::Operation(error)) => {
                    success = false;
                    results.push(RunApplyOutput {
                        index: index + 1,
                        package,
                        action: kind,
                        status: ActionStatus::Failed,
                        error: Some(error.to_string()),
                    });
                }
                Err(journal::ExecuteError::Journal(error)) => {
                    return Err(err("journal_failed", error.to_string()));
                }
            }
        }
    }
    if operation.is_cancelled() {
        success = false;
    }

    Ok(RunOutput {
        schema_version: FLEET_REPORT_SCHEMA_VERSION,
        command: "run".to_string(),
        mode: if apply { "apply" } else { "dry_run" }.to_string(),
        profile_name: profile.name.clone(),
        profile_version: profile.version.clone(),
        device_serial: serial.to_string(),
        device_identity_sha256: crate::fleet_report::hashed_identity(&DeviceIdentity::from_target(
            target,
        )),
        transport_kind: target.transport_kind,
        android_user: user_id,
        compatible: true,
        plans: plan_output,
        results,
        filter_exclusions: resolved_exclusions,
        success,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::device::{DeviceState, DeviceTransportKind};

    fn device(serial: &str, state: DeviceState, kind: DeviceTransportKind) -> Device {
        Device {
            serial: serial.to_string(),
            state,
            model: None,
            product: None,
            device: None,
            marketing_name: None,
            bus_address: None,
            connection_type: None,
            negotiated_speed: None,
            max_speed: None,
            build_fingerprint: None,
            transport_id: Some(1),
            connection_generation: 1,
            transport_kind: kind,
            wireless: kind != DeviceTransportKind::Usb,
        }
    }

    #[test]
    fn fleet_screen_skips_non_actionable_and_untrusted_transports() {
        let result = screen_fleet_devices(
            vec![
                device("offline", DeviceState::Offline, DeviceTransportKind::Usb),
                device("tcp", DeviceState::Device, DeviceTransportKind::UnknownTcp),
                device("usb", DeviceState::Device, DeviceTransportKind::Usb),
            ],
            false,
        );
        assert!(matches!(result[0], FleetScreen::Skipped { .. }));
        assert!(matches!(result[1], FleetScreen::Skipped { .. }));
        assert!(matches!(result[2], FleetScreen::Eligible(_)));
    }

    #[test]
    fn fleet_screen_allows_explicit_unsafe_transport_opt_in() {
        let result = screen_fleet_devices(
            vec![device(
                "legacy",
                DeviceState::Device,
                DeviceTransportKind::LegacyTcp,
            )],
            true,
        );
        assert!(matches!(result.as_slice(), [FleetScreen::Eligible(_)]));
    }
}
