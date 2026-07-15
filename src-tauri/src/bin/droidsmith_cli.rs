//! `droidsmith-cli` — headless companion to the Tauri GUI.
//!
//! Designed for CI / refurbish-fleet workflows. Two subcommands today:
//!
//! ```text
//! droidsmith-cli devices                        # list ADB devices
//! droidsmith-cli run <profile.yaml> --device <serial> [--dry-run|--apply]
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

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() {
        print_help();
        return ExitCode::from(2);
    }

    match argv[0].as_str() {
        "devices" => cmd_devices(),
        "run" => cmd_run(&argv[1..]),
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
         droidsmith-cli run <profile.yaml> --device <serial> [--dry-run|--apply]\n\n\
         EXIT CODES\n  \
         0 success, 1 apply failure, 2 usage/parse, 3 adb not found"
    );
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
