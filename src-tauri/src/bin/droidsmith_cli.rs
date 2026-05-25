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

use droidsmith_lib::adb::{self, actions, device::valid_serial, AdbTransport, ShellTransport};
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

    let requests = profile::requests_for(&profile, &args.serial);

    // Always render plans for the dry-run-or-apply preview.
    let plans: Vec<_> = requests.into_iter().map(actions::plan).collect();
    for (i, plan) in plans.iter().enumerate() {
        println!(
            "  [{:>2}] {}  →  adb -s {} shell {}",
            i + 1,
            plan.description,
            plan.request.serial,
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

    let Some(transport) = resolve_or_fail() else {
        return ExitCode::from(3);
    };
    let manufacturer = match transport.shell(&args.serial, &["getprop", "ro.product.manufacturer"])
    {
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
    for (i, plan) in plans.into_iter().enumerate() {
        let now = iso_now();
        match actions::apply(&transport, plan, &now) {
            Ok(_) => println!("  [{:>2}] ok", i + 1),
            Err(e) => {
                eprintln!("  [{:>2}] FAILED: {e}", i + 1);
                failures += 1;
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

fn iso_now() -> String {
    droidsmith_lib::time::iso_utc_now()
}
