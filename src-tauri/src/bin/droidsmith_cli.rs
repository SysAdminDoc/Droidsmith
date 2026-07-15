//! `droidsmith-cli` — headless companion to the Tauri GUI.
//!
//! Designed for CI / refurbish-fleet workflows:
//!
//! ```text
//! droidsmith-cli devices                        # list ADB devices
//! droidsmith-cli run <profile.yaml> --device <serial> [--dry-run|--apply] [--json]
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
//!
//! No flags are positional after the subcommand to keep the parser
//! tiny; the dependency surface is `clap`-free on purpose.

use std::path::PathBuf;
use std::process::ExitCode;

use serde::Serialize;

use droidsmith_lib::adb::{
    self, actions, device::valid_serial, AdbTransport, DeviceTarget, ShellTransport,
};
use droidsmith_lib::journal;
use droidsmith_lib::profile;
use droidsmith_lib::recovery_baseline::{self, BaselineActionInput};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() {
        print_help();
        return ExitCode::from(2);
    }

    match argv[0].as_str() {
        "devices" => cmd_devices(&argv[1..]),
        "run" => cmd_run(&argv[1..]),
        "migrate-v1" => cmd_migrate_v1(&argv[1..]),
        "baseline-export" => cmd_baseline_export(&argv[1..]),
        "baseline-inspect" => cmd_baseline_inspect(&argv[1..]),
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
         droidsmith-cli run <profile.yaml> --device <serial> [--dry-run|--apply] [--json] [--allow-unsafe-transport]\n  \
         droidsmith-cli migrate-v1 <profile-v1.yaml> --output <profile-v2.yaml> [--json]\n  \
         droidsmith-cli baseline-export <profile.yaml> --device <serial> --output <file.json> [--allow-unsafe-transport]\n  \
         droidsmith-cli baseline-inspect <file.json> --device <serial> [--json] [--allow-unsafe-transport]\n\n\
         EXIT CODES\n  \
         0 success, 1 apply failure, 2 usage/parse, 3 adb not found"
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

fn cmd_migrate_v1(argv: &[String]) -> ExitCode {
    if argv.is_empty() {
        eprintln!("[droidsmith-cli] missing <profile-v1.yaml>");
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
    let migration = match profile::migrate_v1(&input) {
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
        command: "migrate-v1",
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

#[derive(Debug)]
struct BaselineArgs {
    input_path: PathBuf,
    serial: String,
    output_path: Option<PathBuf>,
    json: bool,
    allow_unsafe_transport: bool,
}

fn parse_baseline_args(argv: &[String], export: bool) -> Result<BaselineArgs, String> {
    if argv.is_empty() {
        return Err("missing input file".to_string());
    }
    let input_path = PathBuf::from(&argv[0]);
    let mut serial = None;
    let mut output_path = None;
    let mut json = false;
    let mut allow_unsafe_transport = false;
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
            "--output" if export => {
                i += 1;
                if i >= argv.len() {
                    return Err("--output requires an argument".to_string());
                }
                output_path = Some(PathBuf::from(&argv[i]));
            }
            "--json" if !export => json = true,
            "--allow-unsafe-transport" => allow_unsafe_transport = true,
            other => return Err(format!("unknown flag: {other}")),
        }
        i += 1;
    }
    let serial = serial.ok_or("--device <serial> is required")?;
    if !valid_serial(&serial) {
        return Err(format!("invalid --device serial: {serial:?}"));
    }
    if export && output_path.is_none() {
        return Err("--output <file.json> is required".to_string());
    }
    Ok(BaselineArgs {
        input_path,
        serial,
        output_path,
        json,
        allow_unsafe_transport,
    })
}

fn cmd_baseline_export(argv: &[String]) -> ExitCode {
    let args = match parse_baseline_args(argv, true) {
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
    let mut target = match target_for_serial(&transport, &args.serial) {
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
    let info = match adb::get_device_info(&transport, &target) {
        Ok(info) => info,
        Err(error) => {
            eprintln!("[droidsmith-cli] device constraint probe failed: {error}");
            return ExitCode::from(1);
        }
    };
    let compatibility = profile::device_match_issues(
        &profile,
        &args.serial,
        info.manufacturer.as_deref(),
        info.model.as_deref(),
        info.sdk_level
            .as_deref()
            .and_then(|value| value.parse().ok()),
    );
    if !compatibility.is_empty() {
        for issue in compatibility {
            eprintln!("[droidsmith-cli] {issue}");
        }
        return ExitCode::from(1);
    }
    let available_users = match adb::list_users(&transport, &target) {
        Ok(users) => users,
        Err(error) => {
            eprintln!("[droidsmith-cli] Android user discovery failed: {error}");
            return ExitCode::from(1);
        }
    };
    let user_id = match profile::resolve_user(&profile, &available_users) {
        Ok(user_id) => user_id,
        Err(issues) => {
            for issue in issues {
                eprintln!("[droidsmith-cli] {issue}");
            }
            return ExitCode::from(1);
        }
    };
    let packages = match adb::list_packages(&transport, &target, adb::PackageFilter::All, user_id) {
        Ok(packages) => packages,
        Err(error) => {
            eprintln!("[droidsmith-cli] package inventory failed: {error}");
            return ExitCode::from(1);
        }
    };
    let requested = profile
        .actions
        .iter()
        .map(|action| BaselineActionInput {
            package: action.package.clone(),
            kind: action.kind,
        })
        .collect();
    let baseline =
        match recovery_baseline::build(&target, user_id, None, &packages, requested, iso_now()) {
            Ok(baseline) => baseline,
            Err(error) => {
                eprintln!("[droidsmith-cli] {error}");
                return ExitCode::from(2);
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

fn cmd_baseline_inspect(argv: &[String]) -> ExitCode {
    let args = match parse_baseline_args(argv, false) {
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
    let mut target = match target_for_serial(&transport, &args.serial) {
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
    let users = match adb::list_users(&transport, &target) {
        Ok(users) => users,
        Err(error) => {
            eprintln!("[droidsmith-cli] Android user discovery failed: {error}");
            return ExitCode::from(1);
        }
    };
    let packages = if users.iter().any(|user| user.id == baseline.android_user) {
        match adb::list_packages(
            &transport,
            &target,
            adb::PackageFilter::All,
            baseline.android_user,
        ) {
            Ok(packages) => packages,
            Err(error) => {
                eprintln!("[droidsmith-cli] package inventory failed: {error}");
                return ExitCode::from(1);
            }
        }
    } else {
        Vec::new()
    };
    let diff = match recovery_baseline::inspect(baseline, &target, &users, &packages) {
        Ok(diff) => diff,
        Err(error) => {
            eprintln!("[droidsmith-cli] {error}");
            return ExitCode::from(2);
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
        println!(
            "Device identity: {}\nBuild fingerprint: {}\nAndroid user {}: {}\nRecovery actions ready: {}",
            if diff.compatibility.device_identity_matches { "match" } else { "MISMATCH" },
            if diff.compatibility.build_fingerprint_matches { "match" } else { "changed" },
            diff.baseline.android_user,
            if diff.compatibility.android_user_available { "available" } else { "missing" },
            diff.plans.len()
        );
        for row in &diff.rows {
            println!("  {:?}\t{}\t{}", row.status, row.package, row.reason);
        }
        println!("\n(read-only inspection; nothing was changed)");
    }
    ExitCode::SUCCESS
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

struct RunArgs {
    profile_path: PathBuf,
    serial: String,
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
    let serial = serial.ok_or("--device <serial> is required")?;
    if !valid_serial(&serial) {
        return Err(format!("invalid --device serial: {serial:?}"));
    }
    let apply = apply.ok_or("pass exactly one of --dry-run or --apply")?;
    Ok(RunArgs {
        profile_path,
        serial,
        apply,
        json,
        allow_unsafe_transport,
    })
}

#[derive(Serialize)]
struct RunPlanOutput {
    index: usize,
    package: String,
    action: actions::ActionKind,
    user_id: u32,
    before_state: String,
    description: String,
    adb_args: Vec<String>,
}

#[derive(Serialize)]
struct RunApplyOutput {
    index: usize,
    package: String,
    status: &'static str,
    error: Option<String>,
}

#[derive(Serialize)]
struct RunOutput {
    schema_version: u32,
    command: &'static str,
    mode: &'static str,
    profile_name: String,
    profile_version: String,
    device_serial: String,
    android_user: u32,
    compatible: bool,
    plans: Vec<RunPlanOutput>,
    results: Vec<RunApplyOutput>,
    success: bool,
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
    let mut target = match target_for_serial(&transport, &args.serial) {
        Ok(target) => target,
        Err(error) => return run_error(args.json, 1, "device_unavailable", error),
    };
    if let Err(error) = authorize_cli_transport(&mut target, args.allow_unsafe_transport) {
        return run_error(args.json, 2, "transport_confirmation_required", error);
    }
    let info = match adb::get_device_info(&transport, &target) {
        Ok(info) => info,
        Err(error) => return run_error(args.json, 1, "device_probe_failed", error.to_string()),
    };
    let compatibility = profile::device_match_issues(
        &profile,
        &args.serial,
        info.manufacturer.as_deref(),
        info.model.as_deref(),
        info.sdk_level
            .as_deref()
            .and_then(|value| value.parse().ok()),
    );
    if !compatibility.is_empty() {
        return run_error(
            args.json,
            1,
            "profile_incompatible",
            compatibility.join("; "),
        );
    }
    let users = match adb::list_users(&transport, &target) {
        Ok(users) => users,
        Err(error) => return run_error(args.json, 1, "user_probe_failed", error.to_string()),
    };
    let user_id = match profile::resolve_user(&profile, &users) {
        Ok(user_id) => user_id,
        Err(issues) => {
            return run_error(args.json, 1, "profile_user_unavailable", issues.join("; "))
        }
    };
    let requests = profile::requests_for(
        &profile,
        &target,
        user_id,
        actions::ConfirmationSource::CliApply,
    );
    let mut plans = requests.into_iter().map(actions::plan).collect::<Vec<_>>();
    for plan in &mut plans {
        plan.before_state = actions::capture_state(&transport, &plan.request);
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

    if !args.json {
        println!("Profile: {} (version {})", profile.name, profile.version);
        println!("Target device: {} / Android user {user_id}", args.serial);
        for plan in &plan_output {
            println!(
                "  [{:>2}] {} [{}] → adb {}",
                plan.index,
                plan.description,
                plan.before_state,
                plan.adb_args.join(" ")
            );
        }
    }

    let mut results = Vec::new();
    let mut success = true;
    if args.apply {
        let journal_dir = match journal::default_journal_dir() {
            Ok(path) => path,
            Err(error) => return run_error(args.json, 1, "journal_unavailable", error.to_string()),
        };
        for (index, plan) in plans.into_iter().enumerate() {
            let package = plan.request.package.clone();
            let now = iso_now();
            let result = journal::with_journal(&journal_dir, &target.serial, |journal| {
                journal.execute(plan, None, &now, |plan| {
                    actions::apply(&transport, plan, &iso_now())
                })
            });
            match result {
                Ok(_) => {
                    if !args.json {
                        println!("  [{:>2}] ok", index + 1);
                    }
                    results.push(RunApplyOutput {
                        index: index + 1,
                        package,
                        status: "applied",
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
                        status: "failed",
                        error: Some(error.to_string()),
                    });
                }
                Err(journal::ExecuteError::Journal(error)) => {
                    return run_error(args.json, 1, "journal_failed", error.to_string())
                }
            }
        }
    }

    let output = RunOutput {
        schema_version: 1,
        command: "run",
        mode: if args.apply { "apply" } else { "dry_run" },
        profile_name: profile.name,
        profile_version: profile.version,
        device_serial: args.serial,
        android_user: user_id,
        compatible: true,
        plans: plan_output,
        results,
        success,
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&output).expect("serializable result")
        );
    } else if args.apply && success {
        println!("\nAll actions applied successfully.");
    } else if !args.apply {
        println!("\n(dry-run; read-only state captured; nothing was changed)");
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn target_for_serial(transport: &ShellTransport, serial: &str) -> Result<DeviceTarget, String> {
    let mut devices = transport
        .list_devices()
        .map_err(|error| format!("could not refresh devices: {error}"))?;
    adb::observe_connection_generations(&mut devices);
    let mut matches = devices
        .into_iter()
        .filter(|device| device.serial == serial)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!(
            "device serial {serial:?} is missing or ambiguous; reconnect it and run `devices` again"
        ));
    }
    let mut device = matches.remove(0);
    if !device.state.is_actionable() {
        return Err(format!(
            "device serial {serial:?} is not actionable ({:?})",
            device.state
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

fn iso_now() -> String {
    droidsmith_lib::time::iso_utc_now()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn baseline_export_requires_device_and_output() {
        assert!(
            parse_baseline_args(&strings(&["profile.yaml", "--device", "abc"]), true)
                .unwrap_err()
                .contains("--output")
        );
        let args = parse_baseline_args(
            &strings(&[
                "profile.yaml",
                "--device",
                "abc",
                "--output",
                "baseline.json",
            ]),
            true,
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
            false,
        )
        .unwrap();
        assert!(args.json);
        assert!(args.allow_unsafe_transport);
        assert!(args.output_path.is_none());
        assert!(parse_baseline_args(
            &strings(&["baseline.json", "--device", "abc", "--output", "x"]),
            false,
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
