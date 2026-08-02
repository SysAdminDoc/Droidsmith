//! `droidsmith-cli` — headless companion to the Tauri GUI.
//!
//! Designed for CI / refurbish-fleet workflows:
//!
//! ```text
//! droidsmith-cli devices                        # list ADB devices
//! droidsmith-cli run <profile.yaml> --device <serial> [--dry-run|--apply] [--json]
//! droidsmith-cli run <profile.yaml> --retry-from <report.json> [--dry-run|--apply]
//! droidsmith-cli migrate-v1 <profile.yaml> --output <profile-v2.yaml> [--json]
//! droidsmith-cli baseline-export <profile.yaml> --device <serial> --output <file.json>
//! droidsmith-cli baseline-inspect <file.json> --device <serial> [--json]
//! ```
//!
//! Exit codes:
//!   0  — success / dry-run plan rendered
//!   1  — apply failure (one or more actions errored)
//!   2  — usage / parse error
//!   3  — adb not found
//!   4  — resume refused: the report's inputs drifted (see `--accept-drift`)
//!
//! No flags are positional after the subcommand to keep the parser
//! tiny; the dependency surface is `clap`-free on purpose.

use std::path::PathBuf;
use std::process::ExitCode;

use serde::Serialize;

use droidsmith_lib::adb::{
    self, actions,
    device::{valid_serial, Device},
    AdbTransport, DeviceTarget, ShellTransport,
};
use droidsmith_lib::device_identity::DeviceIdentity;
use droidsmith_lib::fleet_report::{
    self, ActionStatus, DeviceErrorOutput, DeviceRunResult, FleetRunReport, ReportLineage,
    RetryTarget, RunApplyOutput, RunOutput, RunPlanOutput, FLEET_REPORT_SCHEMA_VERSION,
};
use droidsmith_lib::journal;
use droidsmith_lib::profile;
use droidsmith_lib::recovery_baseline::{self, BaselineActionInput, BaselineRoundTrip};

/// Exit code for a resume the operator has to review before it can proceed.
/// Distinct from `2` so a batch runner can tell "you passed the wrong flags"
/// from "the world moved under the report".
const EXIT_RETRY_DRIFT: u8 = 4;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() {
        print_help();
        return ExitCode::from(2);
    }

    match argv[0].as_str() {
        "devices" => cmd_devices(&argv[1..]),
        "run" => cmd_run(&argv[1..]),
        "migrate-v1" => cmd_migrate(&argv[1..], MigrateFrom::V1),
        "migrate-v2" => cmd_migrate(&argv[1..], MigrateFrom::V2),
        "baseline-export" => cmd_baseline_export(&argv[1..]),
        "baseline-inspect" => cmd_baseline_inspect(&argv[1..]),
        "baseline-apply" => cmd_baseline_apply(&argv[1..]),
        "-h" | "--help" | "help" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown subcommand: {other}");
            print_help();
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    eprintln!(
        "droidsmith-cli — headless ADB action runner\n\n\
         USAGE\n  \
         droidsmith-cli devices [--json]\n  \
         droidsmith-cli run <profile.yaml> (--device <serial> | --all-devices | --retry-from <report.json>) [--dry-run|--apply] [--json] [--allow-unsafe-transport] [--accept-drift]\n  \
         droidsmith-cli migrate-v1 <profile-v1.yaml> --output <profile-v3.yaml> [--json]\n  \
         droidsmith-cli migrate-v2 <profile-v2.yaml> --output <profile-v3.yaml> [--json]\n  \
         droidsmith-cli baseline-export <profile.yaml> (--device <serial> --output <file.json> | --all-devices --output <dir>) [--allow-unsafe-transport]\n  \
         droidsmith-cli baseline-inspect <file.json> (--device <serial> | --all-devices) [--direction restore|reapply] [--json] [--allow-unsafe-transport]\n  \
         droidsmith-cli baseline-apply <file.json> --device <serial> --direction restore|reapply (--dry-run|--apply) [--json] [--allow-unsafe-transport]\n\n\
         --all-devices fans the operation over every connected, authorized device.\n  \
         Unauthorized/offline devices and unauthenticated TCP transports (without\n  \
         --allow-unsafe-transport) are skipped, not aborted; the exit code is 1 if any\n  \
         device was skipped or failed.\n\n\
         The OTA round trip is two directions over one baseline: --direction restore\n  \
         walks every recoverable package back to its baseline state before you update,\n  \
         and --direction reapply re-applies the recorded actions to the packages the\n  \
         update reverted. baseline-apply always recomputes the diff live and executes\n  \
         only what that diff marks ready, so a package already in the wanted state is\n  \
         never acted on twice. Packages the portable baseline cannot restore are named\n  \
         explicitly in both directions.\n\n\
         --retry-from resumes an interrupted `run --all-devices --json` report: only\n  \
         devices the report left failed or skipped are selected, and actions the report\n  \
         proves applied are never replayed. Drift in the profile, the action set, the\n  \
         device identity, the Android user, or the transport blocks --apply with exit 4\n  \
         until it is reviewed with --dry-run and accepted with --accept-drift.\n\n\
         EXIT CODES\n  \
         0 success, 1 apply/fleet failure, 2 usage/parse, 3 adb not found, 4 resume drift"
    );
}

#[derive(Serialize)]
struct MigrationOutput {
    schema_version: u32,
    command: &'static str,
    from_version: String,
    to_version: String,
    output_path: String,
    warnings: Vec<String>,
    action_count: usize,
}

/// Which legacy schema a `migrate-*` invocation is upgrading from.
///
/// v1 must be migrated before it can run at all; v2 runs as-is and this is
/// purely an offered upgrade. The two share one implementation because the
/// output contract — a reviewed document written to an explicit path — is the
/// same either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MigrateFrom {
    V1,
    V2,
}

impl MigrateFrom {
    fn command(self) -> &'static str {
        match self {
            Self::V1 => "migrate-v1",
            Self::V2 => "migrate-v2",
        }
    }

    fn missing_input(self) -> &'static str {
        match self {
            Self::V1 => "missing <profile-v1.yaml>",
            Self::V2 => "missing <profile-v2.yaml>",
        }
    }
}

fn cmd_migrate(argv: &[String], from: MigrateFrom) -> ExitCode {
    if argv.is_empty() {
        eprintln!("[droidsmith-cli] {}", from.missing_input());
        return ExitCode::from(2);
    }
    let input = PathBuf::from(&argv[0]);
    let mut output = None;
    let mut json = false;
    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--output" => {
                index += 1;
                if index >= argv.len() {
                    eprintln!("[droidsmith-cli] --output requires an argument");
                    return ExitCode::from(2);
                }
                output = Some(PathBuf::from(&argv[index]));
            }
            "--json" => json = true,
            other => {
                eprintln!("[droidsmith-cli] unknown flag: {other}");
                return ExitCode::from(2);
            }
        }
        index += 1;
    }
    let Some(output) = output else {
        eprintln!("[droidsmith-cli] --output <profile-v2.yaml> is required");
        return ExitCode::from(2);
    };
    let migration = match match from {
        MigrateFrom::V1 => profile::migrate_v1(&input),
        MigrateFrom::V2 => profile::migrate_v2(&input),
    } {
        Ok(migration) => migration,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(2);
        }
    };
    let output = match absolute_path(&output) {
        Ok(output) => output,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = profile::save(&output, &migration.profile) {
        eprintln!("[droidsmith-cli] {error}");
        return ExitCode::from(1);
    }
    let result = MigrationOutput {
        schema_version: 1,
        command: from.command(),
        from_version: migration.from_version,
        to_version: migration.to_version,
        output_path: output.display().to_string(),
        warnings: migration.warnings,
        action_count: migration.profile.actions.len(),
    };
    if json {
        println!(
            "{}",
            serde_json::to_string(&result).expect("serializable result")
        );
    } else {
        println!(
            "Migrated profile v{} to v{}: {} ({} actions)",
            result.from_version, result.to_version, result.output_path, result.action_count
        );
        for warning in result.warnings {
            println!("  warning: {warning}");
        }
    }
    ExitCode::SUCCESS
}

/// Which baseline subcommand a shared argument parse belongs to. The three
/// differ only in which flags they accept, so they share one parser rather
/// than three near-identical ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaselineCommand {
    Export,
    Inspect,
    Apply,
}

#[derive(Debug)]
struct BaselineArgs {
    input_path: PathBuf,
    /// `Some(serial)` for a single target; `None` when `all_devices` is set.
    serial: Option<String>,
    all_devices: bool,
    output_path: Option<PathBuf>,
    json: bool,
    allow_unsafe_transport: bool,
    /// Which half of the OTA round trip to plan. Meaningless for export.
    round_trip: BaselineRoundTrip,
    /// `Some(true)` to execute, `Some(false)` for a dry-run diff. Only
    /// `baseline-apply` carries it.
    apply: Option<bool>,
}

fn parse_round_trip(value: &str) -> Result<BaselineRoundTrip, String> {
    match value {
        "restore" => Ok(BaselineRoundTrip::Restore),
        "reapply" => Ok(BaselineRoundTrip::Reapply),
        other => Err(format!(
            "unknown --direction {other:?}; pass restore (pre-OTA) or reapply (post-OTA)"
        )),
    }
}

fn parse_baseline_args(argv: &[String], command: BaselineCommand) -> Result<BaselineArgs, String> {
    let export = command == BaselineCommand::Export;
    if argv.is_empty() {
        return Err("missing input file".to_string());
    }
    let input_path = PathBuf::from(&argv[0]);
    let mut serial = None;
    let mut all_devices = false;
    let mut output_path = None;
    let mut json = false;
    let mut allow_unsafe_transport = false;
    let mut round_trip = BaselineRoundTrip::Restore;
    let mut apply: Option<bool> = None;
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--device" => {
                i += 1;
                if i >= argv.len() {
                    return Err("--device requires an argument".to_string());
                }
                serial = Some(argv[i].clone());
            }
            "--all-devices" => all_devices = true,
            "--output" if export => {
                i += 1;
                if i >= argv.len() {
                    return Err("--output requires an argument".to_string());
                }
                output_path = Some(PathBuf::from(&argv[i]));
            }
            "--direction" if !export => {
                i += 1;
                if i >= argv.len() {
                    return Err("--direction requires restore or reapply".to_string());
                }
                round_trip = parse_round_trip(&argv[i])?;
            }
            "--dry-run" if command == BaselineCommand::Apply => {
                if apply.is_some() {
                    return Err("pass exactly one of --dry-run or --apply".to_string());
                }
                apply = Some(false);
            }
            "--apply" if command == BaselineCommand::Apply => {
                if apply.is_some() {
                    return Err("pass exactly one of --dry-run or --apply".to_string());
                }
                apply = Some(true);
            }
            "--json" if !export => json = true,
            "--allow-unsafe-transport" => allow_unsafe_transport = true,
            other => return Err(format!("unknown flag: {other}")),
        }
        i += 1;
    }
    if command == BaselineCommand::Apply {
        // A baseline is bound to one device identity, so fanning an apply over
        // a fleet would mean every device but one refusing on identity. Say so
        // instead of pretending it is supported.
        if all_devices {
            return Err(
                "baseline-apply targets one device; a baseline is bound to a single device identity"
                    .to_string(),
            );
        }
        if apply.is_none() {
            return Err("pass exactly one of --dry-run or --apply".to_string());
        }
    }
    let serial = resolve_device_selector(serial, all_devices)?;
    if export && output_path.is_none() {
        return Err(if all_devices {
            "--output <directory> is required with --all-devices".to_string()
        } else {
            "--output <file.json> is required".to_string()
        });
    }
    Ok(BaselineArgs {
        input_path,
        serial,
        all_devices,
        output_path,
        json,
        allow_unsafe_transport,
        round_trip,
        apply,
    })
}

#[derive(Serialize)]
struct BaselineExportSummary {
    device_serial: String,
    output_path: String,
    packages: usize,
    size_bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum BaselineExportResult {
    Exported(BaselineExportSummary),
    Error(DeviceErrorOutput),
    Skipped {
        device_serial: String,
        reason: String,
    },
}

#[derive(Serialize)]
struct FleetBaselineExportOutput {
    schema_version: u32,
    command: &'static str,
    mode: &'static str,
    devices: Vec<BaselineExportResult>,
    success: bool,
}

/// Assemble the recovery baseline for one already-authorized target. Shared by
/// the single and `--all-devices` export paths.
fn build_baseline_for_target(
    transport: &ShellTransport,
    profile: &profile::Profile,
    target: &DeviceTarget,
    serial: &str,
) -> Result<recovery_baseline::RecoveryBaseline, DeviceErrorOutput> {
    let err = |code: &str, message: String| DeviceErrorOutput {
        device_serial: serial.to_string(),
        code: code.to_string(),
        message,
    };
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
    let available_users = adb::list_users(transport, target)
        .map_err(|error| err("user_probe_failed", error.to_string()))?;
    let user_id = profile::resolve_user(profile, &available_users)
        .map_err(|issues| err("profile_user_unavailable", issues.join("; ")))?;
    let packages = adb::list_packages(transport, target, adb::PackageFilter::All, user_id)
        .map_err(|error| err("package_probe_failed", error.to_string()))?;
    let requested = profile
        .actions
        .iter()
        .map(|action| BaselineActionInput {
            package: action.package.clone(),
            kind: action.kind,
        })
        .collect();
    recovery_baseline::build(target, user_id, None, &packages, requested, iso_now())
        .map_err(|error| err("baseline_invalid", error.to_string()))
}

fn cmd_baseline_export(argv: &[String]) -> ExitCode {
    let args = match parse_baseline_args(argv, BaselineCommand::Export) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            print_help();
            return ExitCode::from(2);
        }
    };
    let profile = match profile::load(&args.input_path) {
        Ok(profile) => profile,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(2);
        }
    };
    let Some(transport) = resolve_or_fail() else {
        return ExitCode::from(3);
    };
    if args.all_devices {
        baseline_export_fleet(&transport, &profile, &args)
    } else {
        baseline_export_single(&transport, &profile, &args)
    }
}

fn baseline_export_single(
    transport: &ShellTransport,
    profile: &profile::Profile,
    args: &BaselineArgs,
) -> ExitCode {
    let serial = args
        .serial
        .as_deref()
        .expect("single-device mode always carries a serial");
    let mut target = match target_for_serial(transport, serial) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(1);
        }
    };
    if let Err(error) = authorize_cli_transport(&mut target, args.allow_unsafe_transport) {
        eprintln!("[droidsmith-cli] {error}");
        return ExitCode::from(2);
    }
    let baseline = match build_baseline_for_target(transport, profile, &target, serial) {
        Ok(baseline) => baseline,
        Err(error) => {
            eprintln!("[droidsmith-cli] {}", error.message);
            return ExitCode::from(1);
        }
    };
    let output = absolute_path(args.output_path.as_deref().expect("required by parser"));
    match output.and_then(|path| {
        recovery_baseline::save(&path, &baseline)
            .map(|artifact| (path, artifact))
            .map_err(|error| error.to_string())
    }) {
        Ok((path, artifact)) => {
            println!(
                "Recovery baseline: {} ({} packages, {} bytes, sha256 {})",
                path.display(),
                baseline.packages.len(),
                artifact.size_bytes,
                artifact.sha256
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("[droidsmith-cli] could not export recovery baseline: {error}");
            ExitCode::from(1)
        }
    }
}

/// Export one recovery baseline per discovered device into an output directory
/// (`<serial>.json`). One device's skip/error never aborts the fleet.
fn baseline_export_fleet(
    transport: &ShellTransport,
    profile: &profile::Profile,
    args: &BaselineArgs,
) -> ExitCode {
    let dir = match absolute_path(args.output_path.as_deref().expect("required by parser")) {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("[droidsmith-cli] could not create output directory {dir:?}: {error}");
        return ExitCode::from(1);
    }
    let devices = match list_fleet(transport) {
        Ok(devices) => devices,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(1);
        }
    };
    let mut results: Vec<BaselineExportResult> = Vec::new();
    let mut success = true;
    for screen in screen_fleet_devices(devices, args.allow_unsafe_transport) {
        match screen {
            FleetScreen::Skipped { serial, reason } => {
                success = false;
                if !args.json {
                    eprintln!("[droidsmith-cli] skipped {serial}: {reason}");
                }
                results.push(BaselineExportResult::Skipped {
                    device_serial: serial,
                    reason,
                });
            }
            FleetScreen::Eligible(device) => {
                let serial = device.serial.clone();
                let mut target = match finalize_target(transport, device) {
                    Ok(target) => target,
                    Err(message) => {
                        success = false;
                        results.push(BaselineExportResult::Error(DeviceErrorOutput {
                            device_serial: serial,
                            code: "device_unavailable".to_string(),
                            message,
                        }));
                        continue;
                    }
                };
                if let Err(message) =
                    authorize_cli_transport(&mut target, args.allow_unsafe_transport)
                {
                    success = false;
                    results.push(BaselineExportResult::Error(DeviceErrorOutput {
                        device_serial: serial,
                        code: "transport_confirmation_required".to_string(),
                        message,
                    }));
                    continue;
                }
                match build_baseline_for_target(transport, profile, &target, &serial) {
                    Ok(baseline) => {
                        let path = dir.join(format!("{}.json", serial_file_stem(&serial)));
                        match recovery_baseline::save(&path, &baseline) {
                            Ok(artifact) => {
                                if !args.json {
                                    println!(
                                        "  {serial} → {} ({} packages, {} bytes)",
                                        path.display(),
                                        baseline.packages.len(),
                                        artifact.size_bytes
                                    );
                                }
                                results.push(BaselineExportResult::Exported(
                                    BaselineExportSummary {
                                        device_serial: serial,
                                        output_path: path.display().to_string(),
                                        packages: baseline.packages.len(),
                                        size_bytes: artifact.size_bytes,
                                        sha256: artifact.sha256,
                                    },
                                ));
                            }
                            Err(error) => {
                                success = false;
                                results.push(BaselineExportResult::Error(DeviceErrorOutput {
                                    device_serial: serial,
                                    code: "baseline_write_failed".to_string(),
                                    message: error.to_string(),
                                }));
                            }
                        }
                    }
                    Err(error) => {
                        success = false;
                        if !args.json {
                            eprintln!("[droidsmith-cli] {serial}: {}", error.message);
                        }
                        results.push(BaselineExportResult::Error(error));
                    }
                }
            }
        }
    }
    if results.is_empty() {
        success = false;
        if !args.json {
            eprintln!("[droidsmith-cli] no devices connected");
        }
    }
    let output = FleetBaselineExportOutput {
        schema_version: 1,
        command: "baseline-export",
        mode: "all_devices",
        devices: results,
        success,
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&output).expect("serializable result")
        );
    }
    if output.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[derive(Serialize)]
struct BaselineInspectSummary {
    device_serial: String,
    diff: recovery_baseline::RecoveryBaselineDiff,
}

#[derive(Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum BaselineInspectResult {
    // Boxed: the diff payload dwarfs the other variants.
    Inspected(Box<BaselineInspectSummary>),
    Error(DeviceErrorOutput),
    Skipped {
        device_serial: String,
        reason: String,
    },
}

#[derive(Serialize)]
struct FleetBaselineInspectOutput {
    schema_version: u32,
    command: &'static str,
    mode: &'static str,
    devices: Vec<BaselineInspectResult>,
    success: bool,
}

/// Diff one baseline against one already-authorized target (read-only). Shared
/// by the single and `--all-devices` inspect paths.
fn inspect_baseline_for_target(
    transport: &ShellTransport,
    baseline: recovery_baseline::RecoveryBaseline,
    target: &DeviceTarget,
    serial: &str,
    round_trip: BaselineRoundTrip,
) -> Result<recovery_baseline::RecoveryBaselineDiff, DeviceErrorOutput> {
    let err = |code: &str, message: String| DeviceErrorOutput {
        device_serial: serial.to_string(),
        code: code.to_string(),
        message,
    };
    let users = adb::list_users(transport, target)
        .map_err(|error| err("user_probe_failed", error.to_string()))?;
    let packages = if users.iter().any(|user| user.id == baseline.android_user) {
        adb::list_packages(
            transport,
            target,
            adb::PackageFilter::All,
            baseline.android_user,
        )
        .map_err(|error| err("package_probe_failed", error.to_string()))?
    } else {
        Vec::new()
    };
    recovery_baseline::inspect_round_trip(baseline, target, &users, &packages, round_trip)
        .map_err(|error| err("baseline_invalid", error.to_string()))
}

fn print_inspect_diff(diff: &recovery_baseline::RecoveryBaselineDiff) {
    println!(
        "Direction: {}\nDevice identity: {}\nBuild fingerprint: {}\nAndroid user {}: {}\nActions ready: {}",
        diff.round_trip.label(),
        if diff.compatibility.device_identity_matches { "match" } else { "MISMATCH" },
        if diff.compatibility.build_fingerprint_matches { "match" } else { "changed" },
        diff.baseline.android_user,
        if diff.compatibility.android_user_available { "available" } else { "missing" },
        diff.plans.len()
    );
    for row in &diff.rows {
        println!("  {:?}\t{}\t{}", row.status, row.package, row.reason);
    }
    if !diff.irreversible.is_empty() {
        // Named at both ends of the round trip: before the update so the
        // operator knows what will not come back, and after it so the same
        // list is not silently absent from the re-apply.
        println!(
            "\nNot recoverable from this baseline ({}):",
            diff.irreversible.len()
        );
        for entry in &diff.irreversible {
            println!(
                "  {}\t{:?}\t{}",
                entry.package, entry.requested_action, entry.reason
            );
        }
    }
}

fn cmd_baseline_inspect(argv: &[String]) -> ExitCode {
    let args = match parse_baseline_args(argv, BaselineCommand::Inspect) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            print_help();
            return ExitCode::from(2);
        }
    };
    let baseline = match recovery_baseline::load(&args.input_path) {
        Ok(baseline) => baseline,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(2);
        }
    };
    let Some(transport) = resolve_or_fail() else {
        return ExitCode::from(3);
    };
    if args.all_devices {
        baseline_inspect_fleet(&transport, baseline, &args)
    } else {
        baseline_inspect_single(&transport, baseline, &args)
    }
}

#[derive(Serialize)]
struct BaselineApplyOutput {
    schema_version: u32,
    command: &'static str,
    mode: &'static str,
    direction: BaselineRoundTrip,
    device_serial: String,
    android_user: u32,
    compatible: bool,
    /// Every package the live diff refused, with the reason it gave.
    skipped: Vec<BaselineSkippedOutput>,
    irreversible: Vec<recovery_baseline::BaselineIrreversible>,
    plans: Vec<RunPlanOutput>,
    results: Vec<RunApplyOutput>,
    success: bool,
}

#[derive(Serialize)]
struct BaselineSkippedOutput {
    package: String,
    reason_code: Option<String>,
    reason: String,
}

/// Plan or execute one half of the OTA round trip against a single device.
///
/// The diff is always recomputed against the live device inside this command,
/// and only the plans that diff produced are ever executed. That is what makes
/// "requires a dry-run diff" structural rather than procedural: there is no
/// path from a stale plan to a mutation, and a package already in the wanted
/// state produces no plan, so it cannot be acted on twice.
fn cmd_baseline_apply(argv: &[String]) -> ExitCode {
    let args = match parse_baseline_args(argv, BaselineCommand::Apply) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            print_help();
            return ExitCode::from(2);
        }
    };
    let apply = args.apply.expect("required by the parser");
    let baseline = match recovery_baseline::load(&args.input_path) {
        Ok(baseline) => baseline,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(2);
        }
    };
    let Some(transport) = resolve_or_fail() else {
        return ExitCode::from(3);
    };
    let serial = args
        .serial
        .as_deref()
        .expect("baseline-apply always carries a serial");
    let mut target = match target_for_serial(&transport, serial) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(1);
        }
    };
    if let Err(error) = authorize_cli_transport(&mut target, args.allow_unsafe_transport) {
        eprintln!("[droidsmith-cli] {error}");
        return ExitCode::from(2);
    }
    let android_user = baseline.android_user;
    let diff =
        match inspect_baseline_for_target(&transport, baseline, &target, serial, args.round_trip) {
            Ok(diff) => diff,
            Err(error) => {
                eprintln!("[droidsmith-cli] {}", error.message);
                return ExitCode::from(1);
            }
        };

    let compatible =
        diff.compatibility.device_identity_matches && diff.compatibility.android_user_available;
    if !compatible {
        // Refuse loudly rather than executing an empty plan and reporting
        // success: an identity mismatch means this baseline is not this
        // device's, which is the one mistake this surface must never make
        // quietly.
        let message = if diff.compatibility.device_identity_matches {
            format!("baseline Android user {android_user} is not available on this device")
        } else {
            "this baseline belongs to a different device identity".to_string()
        };
        eprintln!("[droidsmith-cli] {message}");
        return ExitCode::from(1);
    }

    // The diff plans enable-state changes without probing each package's
    // current state; `run` does probe, and the JSON shape is shared, so fill
    // it here rather than emitting an empty field that reads as "unknown".
    let mut diff = diff;
    for plan in &mut diff.plans {
        plan.before_state = actions::capture_state(&transport, &plan.request);
    }
    let plan_output = diff
        .plans
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
    let skipped = diff
        .rows
        .iter()
        .filter(|row| row.status == recovery_baseline::BaselineDiffStatus::Skipped)
        .map(|row| BaselineSkippedOutput {
            package: row.package.clone(),
            reason_code: row.reason_code.map(str::to_string),
            reason: row.reason.clone(),
        })
        .collect::<Vec<_>>();

    if !args.json {
        print_inspect_diff(&diff);
    }

    let mut results = Vec::new();
    let mut success = true;
    if apply {
        let journal_dir = match journal::default_journal_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!("[droidsmith-cli] {error}");
                return ExitCode::from(1);
            }
        };
        let identity = DeviceIdentity::from_target(&target);
        for (index, plan) in diff.plans.into_iter().enumerate() {
            let package = plan.request.package.clone();
            let kind = plan.request.kind;
            let now = iso_now();
            let outcome = journal::with_journal(&journal_dir, &identity, |journal| {
                journal.execute(plan, None, &now, |plan| {
                    actions::apply(&transport, plan, &iso_now())
                })
            });
            match outcome {
                Ok(_) => {
                    if !args.json {
                        println!("  [{:>2}] ok", index + 1);
                    }
                    results.push(RunApplyOutput {
                        index: index + 1,
                        package,
                        action: kind,
                        status: ActionStatus::Applied,
                        error: None,
                    });
                }
                Err(journal::ExecuteError::Operation(error)) => {
                    success = false;
                    if !args.json {
                        eprintln!("  [{:>2}] FAILED: {error}", index + 1);
                    }
                    results.push(RunApplyOutput {
                        index: index + 1,
                        package,
                        action: kind,
                        status: ActionStatus::Failed,
                        error: Some(error.to_string()),
                    });
                }
                Err(journal::ExecuteError::Journal(error)) => {
                    eprintln!("[droidsmith-cli] journal write failed: {error}");
                    return ExitCode::from(1);
                }
            }
        }
    } else if !args.json {
        println!("\n(dry-run; read-only diff captured; nothing was changed)");
    }

    let output = BaselineApplyOutput {
        schema_version: 1,
        command: "baseline-apply",
        mode: if apply { "apply" } else { "dry_run" },
        direction: args.round_trip,
        device_serial: serial.to_string(),
        android_user,
        compatible,
        skipped,
        irreversible: diff.irreversible,
        plans: plan_output,
        results,
        success,
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&output).expect("serializable result")
        );
    } else if apply && success {
        println!(
            "\nAll {} action(s) applied successfully.",
            output.plans.len()
        );
    }
    if output.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn baseline_inspect_single(
    transport: &ShellTransport,
    baseline: recovery_baseline::RecoveryBaseline,
    args: &BaselineArgs,
) -> ExitCode {
    let serial = args
        .serial
        .as_deref()
        .expect("single-device mode always carries a serial");
    let mut target = match target_for_serial(transport, serial) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(1);
        }
    };
    if let Err(error) = authorize_cli_transport(&mut target, args.allow_unsafe_transport) {
        eprintln!("[droidsmith-cli] {error}");
        return ExitCode::from(2);
    }
    let diff =
        match inspect_baseline_for_target(transport, baseline, &target, serial, args.round_trip) {
            Ok(diff) => diff,
            Err(error) => {
                eprintln!("[droidsmith-cli] {}", error.message);
                return ExitCode::from(1);
            }
        };
    if args.json {
        match serde_json::to_string_pretty(&diff) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("[droidsmith-cli] could not encode diff: {error}");
                return ExitCode::from(1);
            }
        }
    } else {
        print_inspect_diff(&diff);
        println!("\n(read-only inspection; nothing was changed)");
    }
    ExitCode::SUCCESS
}

/// Diff one baseline against every discovered device (read-only). One device's
/// skip/error never aborts the fleet.
fn baseline_inspect_fleet(
    transport: &ShellTransport,
    baseline: recovery_baseline::RecoveryBaseline,
    args: &BaselineArgs,
) -> ExitCode {
    let devices = match list_fleet(transport) {
        Ok(devices) => devices,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(1);
        }
    };
    let mut results: Vec<BaselineInspectResult> = Vec::new();
    let mut success = true;
    for screen in screen_fleet_devices(devices, args.allow_unsafe_transport) {
        match screen {
            FleetScreen::Skipped { serial, reason } => {
                success = false;
                if !args.json {
                    eprintln!("[droidsmith-cli] skipped {serial}: {reason}");
                }
                results.push(BaselineInspectResult::Skipped {
                    device_serial: serial,
                    reason,
                });
            }
            FleetScreen::Eligible(device) => {
                let serial = device.serial.clone();
                let mut target = match finalize_target(transport, device) {
                    Ok(target) => target,
                    Err(message) => {
                        success = false;
                        results.push(BaselineInspectResult::Error(DeviceErrorOutput {
                            device_serial: serial,
                            code: "device_unavailable".to_string(),
                            message,
                        }));
                        continue;
                    }
                };
                if let Err(message) =
                    authorize_cli_transport(&mut target, args.allow_unsafe_transport)
                {
                    success = false;
                    results.push(BaselineInspectResult::Error(DeviceErrorOutput {
                        device_serial: serial,
                        code: "transport_confirmation_required".to_string(),
                        message,
                    }));
                    continue;
                }
                match inspect_baseline_for_target(
                    transport,
                    baseline.clone(),
                    &target,
                    &serial,
                    args.round_trip,
                ) {
                    Ok(diff) => {
                        if !args.json {
                            println!("=== {serial} ===");
                            print_inspect_diff(&diff);
                            println!();
                        }
                        results.push(BaselineInspectResult::Inspected(Box::new(
                            BaselineInspectSummary {
                                device_serial: serial,
                                diff,
                            },
                        )));
                    }
                    Err(error) => {
                        success = false;
                        if !args.json {
                            eprintln!("[droidsmith-cli] {serial}: {}", error.message);
                        }
                        results.push(BaselineInspectResult::Error(error));
                    }
                }
            }
        }
    }
    if results.is_empty() {
        success = false;
        if !args.json {
            eprintln!("[droidsmith-cli] no devices connected");
        }
    }
    let output = FleetBaselineInspectOutput {
        schema_version: 1,
        command: "baseline-inspect",
        mode: "all_devices",
        devices: results,
        success,
    };
    if args.json {
        match serde_json::to_string_pretty(&output) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("[droidsmith-cli] could not encode diff: {error}");
                return ExitCode::from(1);
            }
        }
    } else {
        println!("(read-only inspection; nothing was changed)");
    }
    if output.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn authorize_cli_transport(
    target: &mut DeviceTarget,
    allow_unsafe_transport: bool,
) -> Result<(), String> {
    if target.transport_kind.requires_override() && !allow_unsafe_transport {
        return Err(format!(
            "{} uses an unauthenticated {} transport; reconnect with USB/TLS Wi-Fi or pass --allow-unsafe-transport after reviewing the risk",
            target.serial,
            target.transport_kind.label()
        ));
    }
    target.untrusted_transport_override =
        target.transport_kind.requires_override() && allow_unsafe_transport;
    Ok(())
}

fn absolute_path(path: &std::path::Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(|error| format!("could not resolve current directory: {error}"))
    }
}

fn resolve_or_fail() -> Option<ShellTransport> {
    let res = adb::locate_adb();
    let Some(path) = res.path.as_ref() else {
        eprintln!(
            "[droidsmith-cli] adb binary not found. Install Android platform-tools, set \
             $ANDROID_HOME, or place adb on PATH."
        );
        return None;
    };
    Some(ShellTransport::new(path))
}

#[derive(Serialize)]
struct DevicesOutput {
    schema_version: u32,
    command: &'static str,
    devices: Vec<adb::Device>,
}

fn cmd_devices(argv: &[String]) -> ExitCode {
    let json = match argv {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => {
            eprintln!("[droidsmith-cli] devices accepts only --json");
            return ExitCode::from(2);
        }
    };
    let Some(t) = resolve_or_fail() else {
        return ExitCode::from(3);
    };
    match t.list_devices() {
        Ok(devs) => {
            if json {
                let output = DevicesOutput {
                    schema_version: 1,
                    command: "devices",
                    devices: devs,
                };
                println!(
                    "{}",
                    serde_json::to_string(&output).expect("serializable result")
                );
            } else if devs.is_empty() {
                println!("(no devices)");
            } else {
                println!("SERIAL\t\tSTATE\t\tMODEL");
                for d in devs {
                    println!(
                        "{}\t{:?}\t{}",
                        d.serial,
                        d.state,
                        d.model.unwrap_or_else(|| "-".to_string())
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[droidsmith-cli] adb devices failed: {e}");
            ExitCode::from(1)
        }
    }
}

#[derive(Debug)]
struct RunArgs {
    profile_path: PathBuf,
    /// `Some(serial)` for a single target; `None` for `--all-devices` and for
    /// `--retry-from`, where the report defines the device set.
    serial: Option<String>,
    all_devices: bool,
    /// Source fleet report for a resume. Mutually exclusive with the other two
    /// selectors.
    retry_from: Option<PathBuf>,
    /// Proceed with a resume whose inputs drifted from the source report.
    accept_drift: bool,
    apply: bool,
    json: bool,
    allow_unsafe_transport: bool,
}

fn parse_run_args(argv: &[String]) -> Result<RunArgs, String> {
    if argv.is_empty() {
        return Err("missing <profile.yaml>".to_string());
    }
    let profile_path = PathBuf::from(&argv[0]);
    let mut serial: Option<String> = None;
    let mut all_devices = false;
    let mut retry_from: Option<PathBuf> = None;
    let mut accept_drift = false;
    let mut apply: Option<bool> = None;
    let mut json = false;
    let mut allow_unsafe_transport = false;

    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--device" => {
                i += 1;
                if i >= argv.len() {
                    return Err("--device requires an argument".into());
                }
                serial = Some(argv[i].clone());
            }
            "--all-devices" => all_devices = true,
            "--retry-from" => {
                i += 1;
                if i >= argv.len() {
                    return Err("--retry-from requires an argument".into());
                }
                retry_from = Some(PathBuf::from(&argv[i]));
            }
            "--accept-drift" => accept_drift = true,
            "--dry-run" => {
                if apply.is_some() {
                    return Err("pass exactly one of --dry-run or --apply".to_string());
                }
                apply = Some(false);
            }
            "--apply" => {
                if apply.is_some() {
                    return Err("pass exactly one of --dry-run or --apply".to_string());
                }
                apply = Some(true);
            }
            "--json" => json = true,
            "--allow-unsafe-transport" => allow_unsafe_transport = true,
            other => return Err(format!("unknown flag: {other}")),
        }
        i += 1;
    }
    let serial = resolve_run_selector(serial, all_devices, retry_from.is_some())?;
    let apply = apply.ok_or("pass exactly one of --dry-run or --apply")?;
    if accept_drift && retry_from.is_none() {
        return Err("--accept-drift only applies to --retry-from".to_string());
    }
    Ok(RunArgs {
        profile_path,
        serial,
        all_devices,
        retry_from,
        accept_drift,
        apply,
        json,
        allow_unsafe_transport,
    })
}

/// Enforce that exactly one of `--device <serial>` / `--all-devices` was given
/// and that any explicit serial is well-formed. Returns the validated serial
/// (or `None` for the fleet case).
fn resolve_device_selector(
    serial: Option<String>,
    all_devices: bool,
) -> Result<Option<String>, String> {
    match (all_devices, serial) {
        (true, Some(_)) => Err("pass either --device <serial> or --all-devices, not both".into()),
        (true, None) => Ok(None),
        (false, Some(serial)) => {
            if !valid_serial(&serial) {
                return Err(format!("invalid --device serial: {serial:?}"));
            }
            Ok(Some(serial))
        }
        (false, None) => Err("pass --device <serial> or --all-devices".into()),
    }
}

/// `run` adds a third selector: `--retry-from <report.json>` takes its device
/// set from the report, so it excludes both of the others.
fn resolve_run_selector(
    serial: Option<String>,
    all_devices: bool,
    retry_from: bool,
) -> Result<Option<String>, String> {
    if retry_from {
        if all_devices || serial.is_some() {
            return Err(
                "--retry-from selects devices from the report; do not also pass --device or --all-devices"
                    .to_string(),
            );
        }
        return Ok(None);
    }
    resolve_device_selector(serial, all_devices)
}

#[derive(Serialize)]
struct ErrorOutput<'a> {
    schema_version: u32,
    command: &'static str,
    code: &'a str,
    message: String,
    exit_code: u8,
}

fn run_error(json: bool, exit_code: u8, code: &str, message: impl Into<String>) -> ExitCode {
    let message = message.into();
    if json {
        eprintln!(
            "{}",
            serde_json::to_string(&ErrorOutput {
                schema_version: 1,
                command: "run",
                code,
                message,
                exit_code,
            })
            .expect("serializable error")
        );
    } else {
        eprintln!("[droidsmith-cli] {message}");
    }
    ExitCode::from(exit_code)
}

fn cmd_run(argv: &[String]) -> ExitCode {
    let json_requested = argv.iter().any(|value| value == "--json");
    let args = match parse_run_args(argv) {
        Ok(args) => args,
        Err(error) => return run_error(json_requested, 2, "usage", error),
    };
    let profile = match profile::load(&args.profile_path) {
        Ok(profile) => profile,
        Err(error) => return run_error(args.json, 2, "profile_invalid", error.to_string()),
    };
    let Some(transport) = resolve_or_fail() else {
        return run_error(args.json, 3, "adb_not_found", "adb binary not found");
    };
    if args.retry_from.is_some() {
        cmd_run_retry(&transport, &profile, &args)
    } else if args.all_devices {
        cmd_run_fleet(&transport, &profile, &args)
    } else {
        cmd_run_single(&transport, &profile, &args)
    }
}

/// Assemble the fleet report envelope. Shared by the fan-out and resume paths
/// so both emit exactly one schema.
fn fleet_report_envelope(
    profile: &profile::Profile,
    apply: bool,
    devices: Vec<DeviceRunResult>,
    success: bool,
    lineage: Option<ReportLineage>,
) -> FleetRunReport {
    FleetRunReport {
        schema_version: FLEET_REPORT_SCHEMA_VERSION,
        command: "run".to_string(),
        mode: "all_devices".to_string(),
        apply,
        generated_at: iso_now(),
        profile: fleet_report::describe_profile(profile),
        lineage,
        devices,
        success,
    }
}

fn cmd_run_single(
    transport: &ShellTransport,
    profile: &profile::Profile,
    args: &RunArgs,
) -> ExitCode {
    let serial = args
        .serial
        .as_deref()
        .expect("single-device mode always carries a serial");
    let mut target = match target_for_serial(transport, serial) {
        Ok(target) => target,
        Err(error) => return run_error(args.json, 1, "device_unavailable", error),
    };
    if let Err(error) = authorize_cli_transport(&mut target, args.allow_unsafe_transport) {
        return run_error(args.json, 2, "transport_confirmation_required", error);
    }
    match run_profile_on_target(
        transport, profile, &target, serial, args.apply, !args.json, None,
    ) {
        Ok(output) => {
            let success = output.success;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&output).expect("serializable result")
                );
            }
            if success {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => run_error(args.json, 1, &error.code, error.message),
    }
}

/// Fan `run` out over every discovered device. One device's skip/error never
/// aborts the fleet, but the overall exit code is `1` unless every connected
/// device was processed and every action succeeded.
fn cmd_run_fleet(
    transport: &ShellTransport,
    profile: &profile::Profile,
    args: &RunArgs,
) -> ExitCode {
    let devices = match list_fleet(transport) {
        Ok(devices) => devices,
        Err(error) => return run_error(args.json, 1, "device_list_failed", error),
    };
    let screened = screen_fleet_devices(devices, args.allow_unsafe_transport);
    let mut results: Vec<DeviceRunResult> = Vec::new();
    let mut success = true;

    for screen in screened {
        match screen {
            FleetScreen::Skipped { serial, reason } => {
                success = false;
                if !args.json {
                    eprintln!("[droidsmith-cli] skipped {serial}: {reason}");
                }
                results.push(DeviceRunResult::Skipped {
                    device_serial: serial,
                    reason,
                });
            }
            FleetScreen::Eligible(device) => {
                let serial = device.serial.clone();
                let mut target = match finalize_target(transport, device) {
                    Ok(target) => target,
                    Err(error) => {
                        success = false;
                        if !args.json {
                            eprintln!("[droidsmith-cli] {serial}: {error}");
                        }
                        results.push(DeviceRunResult::Error(DeviceErrorOutput {
                            device_serial: serial,
                            code: "device_unavailable".to_string(),
                            message: error,
                        }));
                        continue;
                    }
                };
                if let Err(error) =
                    authorize_cli_transport(&mut target, args.allow_unsafe_transport)
                {
                    success = false;
                    results.push(DeviceRunResult::Error(DeviceErrorOutput {
                        device_serial: serial,
                        code: "transport_confirmation_required".to_string(),
                        message: error,
                    }));
                    continue;
                }
                if !args.json {
                    println!("\n=== {serial} ===");
                }
                match run_profile_on_target(
                    transport, profile, &target, &serial, args.apply, !args.json, None,
                ) {
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
        // No devices at all: an empty fleet is not a successful apply/dry-run.
        success = false;
        if !args.json {
            eprintln!("[droidsmith-cli] no devices connected");
        }
    }

    let ran = results
        .iter()
        .filter(|result| matches!(result, DeviceRunResult::Ran(_)))
        .count();
    let output = fleet_report_envelope(profile, args.apply, results, success, None);
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&output).expect("serializable result")
        );
    } else if !output.devices.is_empty() {
        println!(
            "\nFleet {}: {ran} device(s) processed{}.",
            if args.apply { "apply" } else { "dry-run" },
            if output.success {
                ""
            } else {
                " — review skips/errors above"
            }
        );
    }
    if output.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// One device's slot in a resume: the recorded expectations from the source
/// report plus whether the operator has accepted drift from them.
struct RetryContext<'a> {
    target: &'a RetryTarget,
    accept_drift: bool,
}

/// Compare a live target against what the source report recorded for it.
/// Returns every mismatch; an empty vector means the resume is addressing the
/// same device, on the same transport, as the run it continues.
///
/// The Android user is resolved later (it needs a live user list), so it is
/// checked separately by the caller rather than folded in here.
fn retry_target_drift(recorded: &RetryTarget, live: &DeviceTarget) -> Vec<String> {
    let mut drift = Vec::new();
    if let Some(expected) = &recorded.identity_sha256 {
        let actual = fleet_report::hashed_identity(&DeviceIdentity::from_target(live));
        if expected != &actual {
            drift.push(
                "the device answering this serial is not the one in the report (its serial was reused, or its build changed)"
                    .to_string(),
            );
        }
    }
    if let Some(expected) = recorded.transport_kind {
        if expected != live.transport_kind {
            drift.push(format!(
                "the transport changed from {} to {}",
                expected.label(),
                live.transport_kind.label()
            ));
        }
    }
    drift
}

/// Resume an interrupted fleet run from its report.
///
/// The report defines the device set; the live fleet decides which of those
/// devices can be reached now. A device the report proved complete is never
/// re-run, and an action the report proved applied is never replayed — see
/// [`run_profile_on_target`], which enforces the per-action half.
///
/// Drift between the report and the current inputs (profile document, action
/// set, device identity, Android user, transport) blocks `--apply` with
/// [`EXIT_RETRY_DRIFT`] until it is reviewed with `--dry-run` and accepted with
/// `--accept-drift`. Dry-run always proceeds: reviewing drift is exactly what
/// it is for.
fn cmd_run_retry(
    transport: &ShellTransport,
    profile: &profile::Profile,
    args: &RunArgs,
) -> ExitCode {
    let report_path = args
        .retry_from
        .as_deref()
        .expect("resume mode always carries a report path");
    let loaded = match fleet_report::load(report_path) {
        Ok(loaded) => loaded,
        Err(error) => {
            let code = error.code();
            return run_error(args.json, 2, code, error.to_string());
        }
    };
    let plan = fleet_report::plan_retry(&loaded, profile);

    if plan.has_drift() && args.apply && !args.accept_drift {
        let detail = plan
            .drift
            .iter()
            .map(|item| format!("{}: {}", item.code, item.message))
            .collect::<Vec<_>>()
            .join("; ");
        return run_error(
            args.json,
            EXIT_RETRY_DRIFT,
            "retry_drift",
            format!(
                "the inputs drifted from {}: {detail}. Review with --dry-run, then re-run with --accept-drift to proceed anyway",
                report_path.display()
            ),
        );
    }
    if plan.targets.is_empty() {
        // Nothing left to do is a success, not a failure: the source run
        // finished every device.
        if args.json {
            let output = fleet_report_envelope(
                profile,
                args.apply,
                Vec::new(),
                true,
                Some(ReportLineage {
                    source_sha256: loaded.source_sha256.clone(),
                    source_generated_at: loaded.report.generated_at.clone(),
                    retry_generation: plan.retry_generation,
                    retried_devices: Vec::new(),
                    excluded_devices: plan.excluded.clone(),
                    accepted_drift: Vec::new(),
                }),
            );
            println!(
                "{}",
                serde_json::to_string(&output).expect("serializable result")
            );
        } else {
            println!(
                "Nothing to resume: every device in {} completed successfully.",
                report_path.display()
            );
        }
        return ExitCode::SUCCESS;
    }

    if !args.json {
        println!(
            "Resuming {} of {} device(s) from {} (generation {}).",
            plan.targets.len(),
            plan.targets.len() + plan.excluded.len(),
            report_path.display(),
            plan.retry_generation
        );
        for excluded in &plan.excluded {
            println!("  skipping {}: {}", excluded.serial, excluded.reason);
        }
        for item in &plan.drift {
            println!(
                "  {} — {}: {}",
                if args.accept_drift {
                    "accepted drift"
                } else {
                    "drift (blocks --apply)"
                },
                item.code,
                item.message
            );
        }
    }

    let devices = match list_fleet(transport) {
        Ok(devices) => devices,
        Err(error) => return run_error(args.json, 1, "device_list_failed", error),
    };
    let mut screened = screen_fleet_devices(devices, args.allow_unsafe_transport)
        .into_iter()
        .map(|screen| {
            let serial = match &screen {
                FleetScreen::Eligible(device) => device.serial.clone(),
                FleetScreen::Skipped { serial, .. } => serial.clone(),
            };
            (serial, Some(screen))
        })
        .collect::<Vec<_>>();

    let mut results: Vec<DeviceRunResult> = Vec::new();
    let mut success = true;
    let mut drift_blocked = false;

    for retry_target in &plan.targets {
        // Take the live screen for this serial so a duplicated serial in the
        // report cannot consume the same device twice.
        let screen = screened
            .iter_mut()
            .find(|(serial, screen)| serial == &retry_target.serial && screen.is_some())
            .and_then(|(_, screen)| screen.take());
        let serial = retry_target.serial.clone();
        let Some(screen) = screen else {
            success = false;
            let reason = "the device is not connected now; reconnect it and resume again";
            if !args.json {
                eprintln!("[droidsmith-cli] skipped {serial}: {reason}");
            }
            results.push(DeviceRunResult::Skipped {
                device_serial: serial,
                reason: reason.to_string(),
            });
            continue;
        };
        let device = match screen {
            FleetScreen::Skipped { serial, reason } => {
                success = false;
                if !args.json {
                    eprintln!("[droidsmith-cli] skipped {serial}: {reason}");
                }
                results.push(DeviceRunResult::Skipped {
                    device_serial: serial,
                    reason,
                });
                continue;
            }
            FleetScreen::Eligible(device) => device,
        };
        let mut target = match finalize_target(transport, device) {
            Ok(target) => target,
            Err(error) => {
                success = false;
                if !args.json {
                    eprintln!("[droidsmith-cli] {serial}: {error}");
                }
                results.push(DeviceRunResult::Error(DeviceErrorOutput {
                    device_serial: serial,
                    code: "device_unavailable".to_string(),
                    message: error,
                }));
                continue;
            }
        };
        if let Err(error) = authorize_cli_transport(&mut target, args.allow_unsafe_transport) {
            success = false;
            results.push(DeviceRunResult::Error(DeviceErrorOutput {
                device_serial: serial,
                code: "transport_confirmation_required".to_string(),
                message: error,
            }));
            continue;
        }
        if !args.json {
            println!("\n=== {serial} ===");
        }
        let context = RetryContext {
            target: retry_target,
            accept_drift: args.accept_drift,
        };
        match run_profile_on_target(
            transport,
            profile,
            &target,
            &serial,
            args.apply,
            !args.json,
            Some(&context),
        ) {
            Ok(output) => {
                if !output.success {
                    success = false;
                }
                results.push(DeviceRunResult::Ran(Box::new(output)));
            }
            Err(error) => {
                success = false;
                if error.code == "retry_drift" {
                    drift_blocked = true;
                }
                if !args.json {
                    eprintln!("[droidsmith-cli] {serial}: {}", error.message);
                }
                results.push(DeviceRunResult::Error(error));
            }
        }
    }

    let ran = results
        .iter()
        .filter(|result| matches!(result, DeviceRunResult::Ran(_)))
        .count();
    let output = fleet_report_envelope(
        profile,
        args.apply,
        results,
        success,
        Some(ReportLineage {
            source_sha256: loaded.source_sha256.clone(),
            source_generated_at: loaded.report.generated_at.clone(),
            retry_generation: plan.retry_generation,
            retried_devices: plan
                .targets
                .iter()
                .map(|target| target.serial.clone())
                .collect(),
            excluded_devices: plan.excluded.clone(),
            accepted_drift: if args.accept_drift {
                plan.drift.clone()
            } else {
                Vec::new()
            },
        }),
    );
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&output).expect("serializable result")
        );
    } else {
        println!(
            "\nResume {}: {ran} device(s) processed{}.",
            if args.apply { "apply" } else { "dry-run" },
            if output.success {
                ""
            } else {
                " — review skips/errors above"
            }
        );
    }
    if output.success {
        ExitCode::SUCCESS
    } else if drift_blocked {
        // Per-device drift is the same class of refusal as report-level drift:
        // reviewable, and distinct from an action that genuinely failed.
        ExitCode::from(EXIT_RETRY_DRIFT)
    } else {
        ExitCode::from(1)
    }
}

/// Run one profile against one already-authorized target. Shared by the single,
/// fleet, and resume paths; `print_human` drives the inline progress output for
/// non-JSON runs.
fn run_profile_on_target(
    transport: &ShellTransport,
    profile: &profile::Profile,
    target: &DeviceTarget,
    serial: &str,
    apply: bool,
    print_human: bool,
    retry: Option<&RetryContext<'_>>,
) -> Result<RunOutput, DeviceErrorOutput> {
    let err = |code: &str, message: String| DeviceErrorOutput {
        device_serial: serial.to_string(),
        code: code.to_string(),
        message,
    };

    let mut drift = retry
        .map(|retry| retry_target_drift(retry.target, target))
        .unwrap_or_default();

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
    let users = adb::list_users(transport, target)
        .map_err(|error| err("user_probe_failed", error.to_string()))?;
    let user_id = profile::resolve_user(profile, &users)
        .map_err(|issues| err("profile_user_unavailable", issues.join("; ")))?;
    if let Some(retry) = retry {
        if let Some(expected) = retry.target.android_user {
            if expected != user_id {
                drift.push(format!(
                    "the profile now resolves to Android user {user_id}, not user {expected} as in the report"
                ));
            }
        }
        if !drift.is_empty() && !retry.accept_drift {
            return Err(err("retry_drift", drift.join("; ")));
        }
    }
    // Schema-v3 filter steps resolve against the live inventory, so the
    // inventory is now part of planning rather than only of review.
    let inventory = adb::list_packages(transport, target, adb::PackageFilter::All, user_id)
        .map_err(|error| err("package_probe_failed", error.to_string()))?;
    let resolved = profile::resolve(
        profile,
        target,
        user_id,
        &inventory,
        actions::ConfirmationSource::CliApply,
    );
    let resolved_matches = resolved.matches;
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

    // A resume never replays an action the source report proved applied. The
    // exclusion is decided here, before anything is executed, and is reported
    // in dry-run as well so the review shows exactly what will and will not
    // run.
    let already_applied = |package: &str, kind: actions::ActionKind| -> bool {
        retry.is_some_and(|retry| {
            retry
                .target
                .completed
                .iter()
                .any(|done| done.package == package && done.kind == kind)
        })
    };

    if print_human {
        println!("Profile: {} (version {})", profile.name, profile.version);
        println!("Target device: {serial} / Android user {user_id}");
        if let Some(retry) = retry {
            println!("Resuming: {}", retry.target.reason.label());
            if !drift.is_empty() {
                println!(
                    "{}: {}",
                    if retry.accept_drift {
                        "Accepted device drift"
                    } else {
                        "Device drift (blocks --apply)"
                    },
                    drift.join("; ")
                );
            }
        }
        for matched in &resolved_matches {
            println!(
                "  filter [{}] {:?} matched {} package(s): {}",
                matched.action_index,
                matched.filter,
                matched.packages.len(),
                if matched.packages.is_empty() {
                    "(none)".to_string()
                } else {
                    matched.packages.join(", ")
                }
            );
        }
        for plan in &plan_output {
            println!(
                "  [{:>2}] {}{} [{}] → adb {}",
                plan.index,
                if already_applied(&plan.package, plan.action) {
                    "(already applied) "
                } else {
                    ""
                },
                plan.description,
                plan.before_state,
                plan.adb_args.join(" ")
            );
        }
        if !resolved_exclusions.is_empty() {
            // Reported, never silently dropped: a predicate that could not be
            // decided must not look like a predicate that did not match.
            println!(
                "\nExcluded because a filter could not be resolved ({}):",
                resolved_exclusions.len()
            );
            for exclusion in &resolved_exclusions {
                println!(
                    "  {}\tfilter [{}] needs `{}`, which this device did not report",
                    exclusion.package, exclusion.action_index, exclusion.attribute
                );
            }
        }
    }

    let mut results = Vec::new();
    let mut success = true;
    if apply {
        let journal_dir = journal::default_journal_dir()
            .map_err(|error| err("journal_unavailable", error.to_string()))?;
        // `validate_device_target` already proved the fingerprint, so this
        // identity is the same one the GUI writes under for this device.
        let identity = DeviceIdentity::from_target(target);
        for (index, plan) in plans.into_iter().enumerate() {
            let package = plan.request.package.clone();
            let kind = plan.request.kind;
            if already_applied(&package, kind) {
                if print_human {
                    println!("  [{:>2}] skipped (already applied)", index + 1);
                }
                results.push(RunApplyOutput {
                    index: index + 1,
                    package,
                    action: kind,
                    status: ActionStatus::Skipped,
                    error: None,
                });
                continue;
            }
            let now = iso_now();
            let result = journal::with_journal(&journal_dir, &identity, |journal| {
                journal.execute(plan, None, &now, |plan| {
                    actions::apply(transport, plan, &iso_now())
                })
            });
            match result {
                Ok(_) => {
                    if print_human {
                        println!("  [{:>2}] ok", index + 1);
                    }
                    results.push(RunApplyOutput {
                        index: index + 1,
                        package,
                        action: kind,
                        status: ActionStatus::Applied,
                        error: None,
                    });
                }
                Err(journal::ExecuteError::Operation(error)) => {
                    success = false;
                    if print_human {
                        eprintln!("  [{:>2}] FAILED: {error}", index + 1);
                    }
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
    } else if retry.is_some() {
        // Dry-run resume: nothing is executed, but the exclusions are the
        // point of the review, so record them explicitly.
        for plan in &plan_output {
            if already_applied(&plan.package, plan.action) {
                results.push(RunApplyOutput {
                    index: plan.index,
                    package: plan.package.clone(),
                    action: plan.action,
                    status: ActionStatus::Skipped,
                    error: None,
                });
            }
        }
    }

    if print_human {
        if apply && success {
            println!("\nAll actions applied successfully.");
        } else if !apply {
            println!("\n(dry-run; read-only state captured; nothing was changed)");
        }
    }

    Ok(RunOutput {
        schema_version: FLEET_REPORT_SCHEMA_VERSION,
        command: "run".to_string(),
        mode: if apply { "apply" } else { "dry_run" }.to_string(),
        profile_name: profile.name.clone(),
        profile_version: profile.version.clone(),
        device_serial: serial.to_string(),
        device_identity_sha256: fleet_report::hashed_identity(&DeviceIdentity::from_target(target)),
        transport_kind: target.transport_kind,
        android_user: user_id,
        compatible: true,
        plans: plan_output,
        results,
        filter_exclusions: resolved_exclusions,
        success,
    })
}

fn list_fleet(transport: &ShellTransport) -> Result<Vec<Device>, String> {
    let mut devices = transport
        .list_devices()
        .map_err(|error| format!("could not refresh devices: {error}"))?;
    adb::observe_connection_generations(&mut devices);
    Ok(devices)
}

/// Bind a discovered device to an operable, fingerprinted target. Shared by the
/// single-serial and `--all-devices` paths so both enforce the same
/// actionable-state and build-identity checks.
fn finalize_target(transport: &ShellTransport, mut device: Device) -> Result<DeviceTarget, String> {
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

fn target_for_serial(transport: &ShellTransport, serial: &str) -> Result<DeviceTarget, String> {
    let devices = list_fleet(transport)?;
    let mut matches = devices
        .into_iter()
        .filter(|device| device.serial == serial)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!(
            "device serial {serial:?} is missing or ambiguous; reconnect it and run `devices` again"
        ));
    }
    finalize_target(transport, matches.remove(0))
}

/// Screening verdict for one discovered device in a `--all-devices` fleet run.
enum FleetScreen {
    /// Actionable and transport-authorized; ready for fingerprint binding.
    Eligible(Device),
    /// Excluded before any device I/O, with a user-facing reason.
    Skipped { serial: String, reason: String },
}

/// Pure fleet screen: partition discovered devices into eligible vs skipped
/// without touching the device. Unauthorized/offline devices and
/// override-required transports (legacy/unknown TCP without
/// `--allow-unsafe-transport`) are skipped rather than aborting the fleet.
fn screen_fleet_devices(devices: Vec<Device>, allow_unsafe_transport: bool) -> Vec<FleetScreen> {
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

/// Sanitize a device serial (which may be `host:port`) into a filesystem-safe
/// stem for per-device fleet artifacts. `valid_serial` already blocks path
/// separators; this only neutralizes the remaining reserved characters (`:`).
fn serial_file_stem(serial: &str) -> String {
    serial
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn iso_now() -> String {
    droidsmith_lib::time::iso_utc_now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use droidsmith_lib::adb::device::{DeviceState, DeviceTransportKind};

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn device(serial: &str, state: DeviceState, kind: DeviceTransportKind) -> Device {
        Device {
            serial: serial.to_string(),
            state,
            model: None,
            product: None,
            device: None,
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
    fn device_selector_requires_exactly_one_of_device_or_all() {
        assert_eq!(
            resolve_device_selector(Some("QA1".to_string()), false).unwrap(),
            Some("QA1".to_string())
        );
        assert_eq!(resolve_device_selector(None, true).unwrap(), None);
        assert!(resolve_device_selector(None, false)
            .unwrap_err()
            .contains("--device"));
        assert!(resolve_device_selector(Some("QA1".to_string()), true)
            .unwrap_err()
            .contains("not both"));
        assert!(
            resolve_device_selector(Some("bad serial".to_string()), false)
                .unwrap_err()
                .contains("invalid")
        );
    }

    #[test]
    fn run_parser_accepts_all_devices_and_rejects_combining_selectors() {
        let args =
            parse_run_args(&strings(&["profile.yaml", "--all-devices", "--dry-run"])).unwrap();
        assert!(args.all_devices);
        assert!(args.serial.is_none());
        assert!(parse_run_args(&strings(&[
            "profile.yaml",
            "--all-devices",
            "--device",
            "QA1",
            "--apply",
        ]))
        .is_err());
    }

    #[test]
    fn baseline_export_all_devices_requires_output_directory() {
        // Missing --output with --all-devices reports the directory guidance.
        let error = parse_baseline_args(
            &strings(&["profile.yaml", "--all-devices"]),
            BaselineCommand::Export,
        )
        .unwrap_err();
        assert!(error.contains("directory"), "got: {error}");
        let args = parse_baseline_args(
            &strings(&["profile.yaml", "--all-devices", "--output", "out"]),
            BaselineCommand::Export,
        )
        .unwrap();
        assert!(args.all_devices);
        assert!(args.serial.is_none());
        assert_eq!(args.output_path, Some(PathBuf::from("out")));
    }

    #[test]
    fn fleet_screen_partitions_by_state_and_transport() {
        let devices = vec![
            device("USB1", DeviceState::Device, DeviceTransportKind::Usb),
            device(
                "wifi:5555",
                DeviceState::Device,
                DeviceTransportKind::TlsWifi,
            ),
            device("OFFL1", DeviceState::Offline, DeviceTransportKind::Usb),
            device(
                "UNAUTH1",
                DeviceState::Unauthorized,
                DeviceTransportKind::Usb,
            ),
            device(
                "tcp:5555",
                DeviceState::Device,
                DeviceTransportKind::UnknownTcp,
            ),
        ];

        // Without the unsafe override, USB + paired TLS Wi-Fi are eligible;
        // offline/unauthorized/unknown-TCP are skipped with reasons.
        let screened = screen_fleet_devices(devices.clone(), false);
        let eligible: Vec<&str> = screened
            .iter()
            .filter_map(|screen| match screen {
                FleetScreen::Eligible(device) => Some(device.serial.as_str()),
                FleetScreen::Skipped { .. } => None,
            })
            .collect();
        assert_eq!(eligible, vec!["USB1", "wifi:5555"]);
        let unknown_tcp_skip = screened.iter().find_map(|screen| match screen {
            FleetScreen::Skipped { serial, reason } if serial == "tcp:5555" => Some(reason.clone()),
            _ => None,
        });
        assert!(unknown_tcp_skip
            .expect("unknown tcp skipped")
            .contains("--allow-unsafe-transport"));

        // With the override, the unknown-TCP device becomes eligible too.
        let with_override = screen_fleet_devices(devices, true);
        let eligible: Vec<&str> = with_override
            .iter()
            .filter_map(|screen| match screen {
                FleetScreen::Eligible(device) => Some(device.serial.as_str()),
                FleetScreen::Skipped { .. } => None,
            })
            .collect();
        assert_eq!(eligible, vec!["USB1", "wifi:5555", "tcp:5555"]);
    }

    #[test]
    fn serial_file_stem_neutralizes_reserved_characters() {
        assert_eq!(serial_file_stem("192.168.1.5:5555"), "192.168.1.5_5555");
        assert_eq!(serial_file_stem("R5CT60ZQR4M"), "R5CT60ZQR4M");
    }

    fn sample_profile() -> profile::Profile {
        profile::Profile {
            name: "p".into(),
            version: "2".into(),
            description: String::new(),
            device: Default::default(),
            user: Default::default(),
            actions: vec![profile::ProfileAction {
                kind: actions::ActionKind::Disable,
                package: "com.a".into(),
                filter: String::new(),
                note: String::new(),
            }],
        }
    }

    fn sample_run_output(serial: &str, success: bool) -> RunOutput {
        RunOutput {
            schema_version: FLEET_REPORT_SCHEMA_VERSION,
            command: "run".into(),
            mode: "dry_run".into(),
            profile_name: "p".into(),
            profile_version: "2".into(),
            device_serial: serial.into(),
            device_identity_sha256: "a".repeat(64),
            transport_kind: DeviceTransportKind::Usb,
            android_user: 0,
            compatible: true,
            plans: vec![],
            results: vec![],
            filter_exclusions: vec![],
            success,
        }
    }

    #[test]
    fn fleet_run_output_serializes_all_variants() {
        // Internally-tagged enums panic at serialize time if a newtype variant
        // wraps a non-map; assert the fleet envelope round-trips as JSON.
        let output = fleet_report_envelope(
            &sample_profile(),
            false,
            vec![
                DeviceRunResult::Ran(Box::new(sample_run_output("USB1", true))),
                DeviceRunResult::Error(DeviceErrorOutput {
                    device_serial: "USB2".into(),
                    code: "device_probe_failed".to_string(),
                    message: "boom".into(),
                }),
                DeviceRunResult::Skipped {
                    device_serial: "tcp:5555".into(),
                    reason: "unsafe".into(),
                },
            ],
            false,
            None,
        );
        let json = serde_json::to_string(&output).expect("fleet run output serializes");
        assert!(json.contains("\"outcome\":\"ran\""));
        assert!(json.contains("\"outcome\":\"error\""));
        assert!(json.contains("\"outcome\":\"skipped\""));
        // The envelope a resume reads back must be the envelope the fan-out
        // writes: parse its own output rather than a hand-built fixture.
        let reparsed = fleet_report::parse(std::path::Path::new("r.json"), json.as_bytes())
            .expect("the emitted report is loadable");
        assert_eq!(
            reparsed.report.profile.action_set_sha256,
            fleet_report::action_set_fingerprint(&sample_profile())
        );

        // ...and the GUI renders that same emitted document, with the serials
        // the CLI necessarily wrote into it redacted away. Asserting this here
        // rather than only in `fleet_report`'s own tests keeps the writer and
        // the renderer pinned to one document.
        let rendered =
            serde_json::to_string(&fleet_report::view(&reparsed.report)).expect("view serializes");
        for serial in ["USB1", "USB2", "tcp:5555"] {
            assert!(
                !rendered.contains(serial),
                "serial {serial} survived into the rendered view"
            );
        }
    }

    #[test]
    fn a_resume_selector_excludes_the_other_two() {
        let args = parse_run_args(&strings(&[
            "profile.yaml",
            "--retry-from",
            "report.json",
            "--apply",
        ]))
        .unwrap();
        assert_eq!(args.retry_from, Some(PathBuf::from("report.json")));
        assert!(args.serial.is_none());
        assert!(!args.all_devices);
        assert!(!args.accept_drift);

        for combined in [
            vec![
                "profile.yaml",
                "--retry-from",
                "r.json",
                "--all-devices",
                "--apply",
            ],
            vec![
                "profile.yaml",
                "--retry-from",
                "r.json",
                "--device",
                "QA1",
                "--apply",
            ],
        ] {
            assert!(parse_run_args(&strings(&combined))
                .unwrap_err()
                .contains("--retry-from selects devices from the report"));
        }

        // --accept-drift is meaningless without a report to drift from.
        assert!(parse_run_args(&strings(&[
            "profile.yaml",
            "--all-devices",
            "--apply",
            "--accept-drift"
        ]))
        .unwrap_err()
        .contains("--accept-drift only applies"));
        assert!(parse_run_args(&strings(&["profile.yaml", "--retry-from"]))
            .unwrap_err()
            .contains("requires an argument"));
    }

    #[test]
    fn a_resumed_device_must_still_be_the_device_the_report_recorded() {
        let recorded_target = DeviceTarget {
            serial: "USB1".into(),
            build_fingerprint: Some("brand/x:16/X/1:user/release-keys".into()),
            transport_kind: DeviceTransportKind::Usb,
            ..DeviceTarget::default()
        };
        let recorded = RetryTarget {
            serial: "USB1".into(),
            identity_sha256: Some(fleet_report::hashed_identity(&DeviceIdentity::from_target(
                &recorded_target,
            ))),
            android_user: Some(0),
            transport_kind: Some(DeviceTransportKind::Usb),
            reason: fleet_report::RetryReason::PartiallyApplied,
            completed: vec![],
        };
        assert!(retry_target_drift(&recorded, &recorded_target).is_empty());

        // Same serial, different build: a reused serial or an OTA between runs.
        let updated = DeviceTarget {
            build_fingerprint: Some("brand/x:17/Y/2:user/release-keys".into()),
            ..recorded_target.clone()
        };
        let drift = retry_target_drift(&recorded, &updated);
        assert_eq!(drift.len(), 1);
        assert!(drift[0].contains("not the one in the report"), "{drift:?}");

        // Same device, demoted transport.
        let downgraded = DeviceTarget {
            transport_kind: DeviceTransportKind::UnknownTcp,
            ..recorded_target.clone()
        };
        let drift = retry_target_drift(&recorded, &downgraded);
        assert_eq!(drift.len(), 1);
        assert!(drift[0].contains("transport changed"), "{drift:?}");

        // A device the source never bound records no expectations, so nothing
        // can drift — the live run validates it from scratch instead.
        let unbound = RetryTarget {
            identity_sha256: None,
            transport_kind: None,
            android_user: None,
            ..recorded.clone()
        };
        assert!(retry_target_drift(&unbound, &downgraded).is_empty());
    }

    #[test]
    fn baseline_export_requires_device_and_output() {
        assert!(parse_baseline_args(
            &strings(&["profile.yaml", "--device", "abc"]),
            BaselineCommand::Export
        )
        .unwrap_err()
        .contains("--output"));
        let args = parse_baseline_args(
            &strings(&[
                "profile.yaml",
                "--device",
                "abc",
                "--output",
                "baseline.json",
            ]),
            BaselineCommand::Export,
        )
        .unwrap();
        assert_eq!(args.output_path, Some(PathBuf::from("baseline.json")));
        assert!(!args.json);
    }

    #[test]
    fn baseline_inspect_is_read_only_and_supports_json() {
        let args = parse_baseline_args(
            &strings(&[
                "baseline.json",
                "--device",
                "abc",
                "--json",
                "--allow-unsafe-transport",
            ]),
            BaselineCommand::Inspect,
        )
        .unwrap();
        assert!(args.json);
        assert!(args.allow_unsafe_transport);
        assert!(args.output_path.is_none());
        assert!(parse_baseline_args(
            &strings(&["baseline.json", "--device", "abc", "--output", "x"]),
            BaselineCommand::Inspect,
        )
        .is_err());
    }

    #[test]
    fn run_parser_supports_machine_output_and_rejects_ambiguous_modes() {
        let args = parse_run_args(&strings(&[
            "profile.yaml",
            "--device",
            "QA123",
            "--dry-run",
            "--json",
            "--allow-unsafe-transport",
        ]))
        .unwrap();
        assert!(!args.apply);
        assert!(args.json);
        assert!(args.allow_unsafe_transport);
        assert!(parse_run_args(&strings(&[
            "profile.yaml",
            "--device",
            "QA123",
            "--dry-run",
            "--apply",
        ]))
        .is_err());
    }
}
