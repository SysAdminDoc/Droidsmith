//! `droidsmith-pack-lint <files...>` — CI gate + local-dev tool.
//!
//! Exits 0 if every input pack parses and lints clean. Exits 1 on any
//! issue. Output is one block per file, deliberately grep-friendly so
//! contributors can pipe it through `| grep ERROR` to see only failures.
//!
//! This binary is intentionally narrow: it does not network, it does
//! not write files, it does not call adb. Safe to invoke from CI on
//! any platform.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use droidsmith_lib::packs::{lint, Pack};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: droidsmith-pack-lint <pack.yaml> [<pack.yaml>...]");
        return ExitCode::from(2);
    }

    let mut had_errors = false;
    let mut checked = 0usize;

    for raw in args {
        let path = PathBuf::from(&raw);
        if !path.exists() {
            println!("[ERROR] {raw}: file does not exist");
            had_errors = true;
            continue;
        }
        match lint_file(&path) {
            Ok(n_entries) => {
                println!("[ok]    {raw} ({n_entries} entries)");
                checked += 1;
            }
            Err(e) => {
                println!("[ERROR] {raw}: {e}");
                had_errors = true;
            }
        }
    }

    println!();
    println!(
        "{checked} pack(s) clean, {} with errors",
        if had_errors { "≥1" } else { "0" }
    );
    if had_errors {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn lint_file(path: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read failed: {e}"))?;
    let pack: Pack = serde_yml::from_str(&text).map_err(|e| format!("parse failed: {e}"))?;
    let issues = lint(&pack);
    if issues.is_empty() {
        Ok(pack.packages.len())
    } else {
        Err(issues.join("; "))
    }
}
