//! `droidsmith-cli` — headless companion to the Tauri GUI.
//!
//! Designed for CI / refurbish-fleet workflows:
//!
//! ```text
//! droidsmith-cli devices                        # list ADB devices
//! droidsmith-cli run <profile.yaml> --device <serial> [--dry-run|--apply]
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
        "devices" => cmd_devices(),
        "run" => cmd_run(&argv[1..]),
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
         droidsmith-cli devices\n  \
         droidsmith-cli run <profile.yaml> --device <serial> [--dry-run|--apply]\n  \
         droidsmith-cli baseline-export <profile.yaml> --device <serial> --output <file.json> [--allow-unsafe-transport]\n  \
         droidsmith-cli baseline-inspect <file.json> --device <serial> [--json] [--allow-unsafe-transport]\n\n\
         EXIT CODES\n  \
         0 success, 1 apply failure, 2 usage/parse, 3 adb not found"
    );
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
    let serial_issues = profile::serial_match_issues(&profile, &args.serial);
    if !serial_issues.is_empty() {
        for issue in serial_issues {
            eprintln!("[droidsmith-cli] {issue}");
        }
        return ExitCode::from(2);
    }
    let mut users = profile.actions.iter().map(|action| action.user);
    let user_id = users.next().unwrap_or_default();
    if users.any(|candidate| candidate != user_id) {
        eprintln!(
            "[droidsmith-cli] one recovery baseline covers exactly one Android user; split this profile by user"
        );
        return ExitCode::from(2);
    }
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
    if !profile.device.require_manufacturer.trim().is_empty() {
        let manufacturer =
            match transport.shell_target(&target, &["getprop", "ro.product.manufacturer"]) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!(
                        "[droidsmith-cli] could not verify required device manufacturer: {error}"
                    );
                    return ExitCode::from(1);
                }
            };
        let issues = profile::manufacturer_match_issues(&profile, Some(manufacturer.trim()));
        if !issues.is_empty() {
            for issue in issues {
                eprintln!("[droidsmith-cli] {issue}");
            }
            return ExitCode::from(1);
        }
    }
    let available_users = match adb::list_users(&transport, &target) {
        Ok(users) => users,
        Err(error) => {
            eprintln!("[droidsmith-cli] Android user discovery failed: {error}");
            return ExitCode::from(1);
        }
    };
    if !available_users.iter().any(|user| user.id == user_id) {
        eprintln!("[droidsmith-cli] Android user {user_id} is not available");
        return ExitCode::from(1);
    }
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

fn cmd_devices() -> ExitCode {
    let Some(t) = resolve_or_fail() else {
        return ExitCode::from(3);
    };
    match t.list_devices() {
        Ok(devs) => {
            if devs.is_empty() {
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
}

fn parse_run_args(argv: &[String]) -> Result<RunArgs, String> {
    if argv.is_empty() {
        return Err("missing <profile.yaml>".to_string());
    }
    let profile_path = PathBuf::from(&argv[0]);
    let mut serial: Option<String> = None;
    let mut apply: Option<bool> = None;

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
            "--dry-run" => apply = Some(false),
            "--apply" => apply = Some(true),
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
    })
}

fn cmd_run(argv: &[String]) -> ExitCode {
    let args = match parse_run_args(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[droidsmith-cli] {e}");
            print_help();
            return ExitCode::from(2);
        }
    };

    let profile = match profile::load(&args.profile_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[droidsmith-cli] {e}");
            return ExitCode::from(2);
        }
    };

    let serial_issues = profile::serial_match_issues(&profile, &args.serial);
    if !serial_issues.is_empty() {
        for issue in serial_issues {
            eprintln!("[droidsmith-cli] {issue}");
        }
        return ExitCode::from(2);
    }

    println!("Profile: {} (version {})", profile.name, profile.version);
    if !profile.description.is_empty() {
        println!("  {}", profile.description);
    }
    println!("Target device: {}", args.serial);
    println!("Actions: {} step(s)", profile.actions.len());

    let transport = if args.apply {
        let Some(transport) = resolve_or_fail() else {
            return ExitCode::from(3);
        };
        Some(transport)
    } else {
        None
    };
    let target = match &transport {
        Some(transport) => match target_for_serial(transport, &args.serial) {
            Ok(target) => target,
            Err(error) => {
                eprintln!("[droidsmith-cli] {error}");
                return ExitCode::from(1);
            }
        },
        None => DeviceTarget {
            serial: args.serial.clone(),
            transport_id: None,
            // Dry runs never execute this target; a non-zero value keeps the
            // plan schema complete without pretending a live transport was
            // verified.
            connection_generation: 1,
            model: None,
            product: None,
            device: None,
            build_fingerprint: Some("dry-run-unverified".to_string()),
            transport_kind: droidsmith_lib::adb::DeviceTransportKind::UnknownTcp,
            untrusted_transport_override: false,
        },
    };
    let requests = profile::requests_for(&profile, &target);

    // Always render plans for the dry-run-or-apply preview.
    let plans: Vec<_> = requests.into_iter().map(actions::plan).collect();
    for (i, plan) in plans.iter().enumerate() {
        println!(
            "  [{:>2}] {}  →  adb {} shell {}",
            i + 1,
            plan.description,
            plan.request.target.adb_selector().join(" "),
            plan.args.join(" ")
        );
    }

    if !args.apply {
        if !profile.device.require_manufacturer.trim().is_empty() {
            println!(
                "\n(dry-run; manufacturer constraint {:?} will be verified during --apply)",
                profile.device.require_manufacturer
            );
        }
        println!("\n(dry-run; nothing was changed)");
        return ExitCode::SUCCESS;
    }

    let transport = transport.expect("apply mode resolves a transport before planning");
    let manufacturer =
        match transport.shell_target(&target, &["getprop", "ro.product.manufacturer"]) {
            Ok(value) => Some(value.trim().to_string()),
            Err(e) if profile.device.require_manufacturer.trim().is_empty() => {
                eprintln!("[droidsmith-cli] warning: could not read device manufacturer: {e}");
                None
            }
            Err(e) => {
                eprintln!("[droidsmith-cli] could not verify required device manufacturer: {e}");
                return ExitCode::from(1);
            }
        };
    let manufacturer_issues = profile::manufacturer_match_issues(&profile, manufacturer.as_deref());
    if !manufacturer_issues.is_empty() {
        for issue in manufacturer_issues {
            eprintln!("[droidsmith-cli] {issue}");
        }
        return ExitCode::from(1);
    }

    let mut failures = 0;
    let journal_dir = match journal::default_journal_dir() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("[droidsmith-cli] cannot resolve operation journal: {error}");
            return ExitCode::from(1);
        }
    };
    for (i, mut plan) in plans.into_iter().enumerate() {
        plan.before_state = actions::capture_state(&transport, &plan.request);
        let now = iso_now();
        let result = journal::with_journal(&journal_dir, &target.serial, |journal| {
            journal.execute(plan, None, &now, |plan| {
                actions::apply(&transport, plan, &iso_now())
            })
        });
        match result {
            Ok(_) => println!("  [{:>2}] ok", i + 1),
            Err(journal::ExecuteError::Operation(e)) => {
                eprintln!("  [{:>2}] FAILED: {e}", i + 1);
                failures += 1;
            }
            Err(journal::ExecuteError::Journal(e)) => {
                eprintln!("  [{:>2}] JOURNAL FAILED: {e}", i + 1);
                eprintln!("\nStopped before any later actions to preserve auditability.");
                return ExitCode::from(1);
            }
        }
    }

    if failures > 0 {
        eprintln!("\n{failures} action(s) failed");
        ExitCode::from(1)
    } else {
        println!("\nAll actions applied successfully.");
        ExitCode::SUCCESS
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
}
