use std::path::PathBuf;
use std::process::Command;

fn cli_binary() -> PathBuf {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let binary = if cfg!(windows) {
        "droidsmith-cli.exe"
    } else {
        "droidsmith-cli"
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(profile)
        .join(binary)
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(cli_binary())
        .args(args)
        .output()
        .expect("failed to execute droidsmith-cli");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stdout, stderr)
}

#[test]
fn help_exits_zero() {
    let (code, _, stderr) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(stderr.contains("droidsmith-cli"));
}

#[test]
fn unknown_command_exits_two() {
    let (code, _, stderr) = run(&["nonexistent-subcommand"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("unknown subcommand"));
}

#[test]
fn no_args_exits_two() {
    let (code, _, _) = run(&[]);
    assert_eq!(code, 2);
}

#[test]
fn migrate_v1_succeeds_with_fixture() {
    let input = fixtures().join("profiles").join("v1-valid.yaml");
    let dir = std::env::temp_dir().join(format!("droidsmith-cli-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let output = dir.join("migrated.yaml");
    let (code, stdout, stderr) = run(&[
        "migrate-v1",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON output");
    assert_eq!(parsed["command"], "migrate-v1");
    assert_eq!(parsed["to_version"], "2");
    assert!(output.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn migrate_v1_rejects_missing_output() {
    let input = fixtures().join("profiles").join("v1-valid.yaml");
    let (code, _, stderr) = run(&["migrate-v1", input.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("--output"));
}

#[test]
fn run_rejects_combining_device_and_all_devices() {
    let input = fixtures().join("profiles").join("v1-valid.yaml");
    let (code, _, stderr) = run(&[
        "run",
        input.to_str().unwrap(),
        "--all-devices",
        "--device",
        "QA1",
        "--dry-run",
    ]);
    assert_eq!(code, 2, "stderr: {stderr}");
    assert!(stderr.contains("not both"), "stderr: {stderr}");
}

#[test]
fn baseline_export_all_devices_requires_output_directory() {
    let input = fixtures().join("profiles").join("v1-valid.yaml");
    let (code, _, stderr) = run(&["baseline-export", input.to_str().unwrap(), "--all-devices"]);
    assert_eq!(code, 2, "stderr: {stderr}");
    assert!(stderr.contains("directory"), "stderr: {stderr}");
}
