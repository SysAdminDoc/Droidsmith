use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use droidsmith_lib::adb::device::{observe_connection_generations, Device, DeviceState};
use droidsmith_lib::adb::parsers::parse_running_services;
use droidsmith_lib::adb::{
    get_device_info, list_mdns_services, list_packages_with_capability, list_users,
    validate_device_target, AdbTransport, OutputStream, PackageFilter, ShellTransport,
    TransportError,
};
use droidsmith_lib::journal::with_journal;
use droidsmith_lib::operations::{cancel, run_process, EventSink, OperationError};
use serde::Deserialize;

fn fake_tool() -> PathBuf {
    std::env::current_exe().unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if std::env::var_os("DROIDSMITH_ADB_CORPUS_CASE").is_some() {
        run_fake_adb(&args);
        return;
    }
    if args.first().is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "emit"
                | "fail"
                | "flood"
                | "capture-stdout"
                | "capture-stderr"
                | "capture-both"
                | "tree"
                | "grandchild"
        )
    }) {
        run_fake_tool(&args);
        return;
    }

    run_contract(
        "shell_transport_preserves_argv_stdin_stdout_and_stderr",
        shell_transport_preserves_argv_stdin_stdout_and_stderr,
    );
    run_contract(
        "streaming_capture_is_bounded_under_pipe_backpressure",
        streaming_capture_is_bounded_under_pipe_backpressure,
    );
    run_contract(
        "short_lived_capture_limits_stdout_stderr_and_both",
        short_lived_capture_limits_stdout_stderr_and_both,
    );
    run_contract(
        "cancellation_terminates_the_full_descendant_tree",
        cancellation_terminates_the_full_descendant_tree,
    );
    run_contract(
        "target_drift_and_disk_failures_stop_before_mutation",
        target_drift_and_disk_failures_stop_before_mutation,
    );
    run_contract(
        "adb_transcript_corpus_exercises_complete_workflows",
        adb_transcript_corpus_exercises_complete_workflows,
    );
}

fn run_contract(name: &str, contract: fn()) {
    contract();
    println!("contract {name} ... ok");
}

fn test_dir(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "droidsmith-fake-tool-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn no_events() -> EventSink {
    Arc::new(|_| {})
}

fn shell_transport_preserves_argv_stdin_stdout_and_stderr() {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let dir = test_dir("argv");
    let record = dir.join("invocation.json");
    std::env::set_var("DROIDSMITH_FAKE_TOOL_RECORD", &record);
    let transport = ShellTransport::new(fake_tool()).with_timeout(Duration::from_secs(3));

    let stdout = transport.adb(&["emit", "arg with spaces", "雪"]).unwrap();
    assert_eq!(stdout, "stdout-雪\n");
    let recorded: serde_json::Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    assert_eq!(
        recorded["args"],
        serde_json::json!(["emit", "arg with spaces", "雪"])
    );
    assert_eq!(recorded["stdinBytes"], 0);

    let error = transport.adb(&["fail"]).unwrap_err();
    match error {
        TransportError::Exit { code, stderr } => {
            assert_eq!(code, 23);
            assert_eq!(stderr, "exact failure text\n");
        }
        other => panic!("unexpected fake-tool failure: {other}"),
    }
    std::env::remove_var("DROIDSMITH_FAKE_TOOL_RECORD");
}

fn streaming_capture_is_bounded_under_pipe_backpressure() {
    let output = run_process(
        &fake_tool(),
        &["flood".to_string()],
        Duration::from_secs(5),
        "fake-flood-test",
        "fake flood",
        no_events(),
    )
    .unwrap();
    assert!(output.success());
    assert_eq!(output.stdout.len(), 1024 * 1024);
    assert_eq!(output.stderr.len(), 1024 * 1024);
    assert!(output.stdout.ends_with("STDOUT-END"));
    assert!(output.stderr.ends_with("STDERR-END"));
}

fn short_lived_capture_limits_stdout_stderr_and_both() {
    let transport = ShellTransport::new(fake_tool()).with_timeout(Duration::from_secs(5));
    for (command, expected_stream) in [
        ("capture-stdout", OutputStream::Stdout),
        ("capture-stderr", OutputStream::Stderr),
        ("capture-both", OutputStream::Both),
    ] {
        let started = Instant::now();
        let error = transport.adb(&[command]).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(4));
        match error {
            TransportError::OutputLimit {
                stream,
                limit_bytes,
            } => {
                assert_eq!(stream, expected_stream);
                assert!(limit_bytes > 0);
            }
            other => panic!("unexpected capture result for {command}: {other}"),
        }
    }
}

fn cancellation_terminates_the_full_descendant_tree() {
    let dir = test_dir("tree");
    let pid_path = dir.join("grandchild.pid");
    let run_pid_path = pid_path.clone();
    let thread = std::thread::spawn(move || {
        run_process(
            &fake_tool(),
            &["tree".to_string(), run_pid_path.display().to_string()],
            Duration::from_secs(20),
            "fake-tree-test",
            "fake tree",
            no_events(),
        )
    });

    wait_until(Duration::from_secs(3), || pid_path.is_file());
    let grandchild_pid: u32 = fs::read_to_string(&pid_path).unwrap().parse().unwrap();
    wait_until(Duration::from_secs(2), || cancel("fake-tree-test"));
    assert!(matches!(
        thread.join().unwrap(),
        Err(OperationError::Cancelled)
    ));
    wait_until(Duration::from_secs(3), || !process_is_alive(grandchild_pid));
    assert!(!process_is_alive(grandchild_pid));
}

fn target_drift_and_disk_failures_stop_before_mutation() {
    let serial = format!("PROPERTY-{}", std::process::id());
    let mut initial = vec![device(&serial, "build/one")];
    observe_connection_generations(&mut initial);
    let target = initial[0].target();
    let transport = FixedTransport {
        devices: vec![device(&serial, "build/two")],
    };
    assert!(matches!(
        validate_device_target(&transport, &target),
        Err(TransportError::Parse(message)) if message.contains("changed")
    ));

    let dir = test_dir("disk-failure");
    let not_a_directory = dir.join("journal-parent");
    fs::write(&not_a_directory, b"occupied by a file").unwrap();
    let ran = AtomicBool::new(false);
    let result: Result<(), std::io::Error> = with_journal(&not_a_directory, "device", |_| {
        ran.store(true, Ordering::Release);
        Ok(())
    });
    assert!(result.is_err());
    assert!(!ran.load(Ordering::Acquire));
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TranscriptCorpus {
    schema_version: u32,
    cases: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TranscriptCase {
    schema_version: u32,
    id: String,
    platform_tools_version: String,
    device_family: String,
    sanitized: bool,
    commands: Vec<TranscriptCommand>,
    expectations: TranscriptExpectations,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TranscriptCommand {
    argv: Vec<String>,
    exit_code: i32,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TranscriptExpectations {
    serial: String,
    model: String,
    transport_id: u32,
    manufacturer: String,
    package_count: usize,
    enriched_package_metadata: bool,
    storage_partition_count: usize,
    service_count: usize,
    user_count: usize,
    current_user: u32,
    mdns_service_count: usize,
    unknown_device_count: usize,
    enriched_fallback_used: bool,
}

fn adb_transcript_corpus_exercises_complete_workflows() {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let corpus_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("adb-transcripts")
        .join("v1");
    let corpus: TranscriptCorpus =
        serde_json::from_slice(&fs::read(corpus_root.join("corpus.json")).unwrap()).unwrap();
    assert_eq!(corpus.schema_version, 1);
    assert_eq!(corpus.cases.len(), 3);

    for relative_case in corpus.cases {
        let case_path = corpus_root.join(relative_case);
        let case: TranscriptCase = serde_json::from_slice(&fs::read(&case_path).unwrap()).unwrap();
        assert_eq!(case.schema_version, 1, "{}", case.id);
        assert!(case.sanitized, "{}", case.id);
        assert!(
            ["36.0.2", "37.0.0", "37.0.1"].contains(&case.platform_tools_version.as_str()),
            "{}",
            case.id
        );
        assert!(
            ["aosp", "samsung-oneui", "xiaomi-hyperos"].contains(&case.device_family.as_str()),
            "{}",
            case.id
        );

        let run_dir = test_dir(&format!("corpus-{}", case.id));
        let record_path = run_dir.join("invocations.jsonl");
        std::env::set_var("DROIDSMITH_ADB_CORPUS_CASE", &case_path);
        std::env::set_var("DROIDSMITH_ADB_CORPUS_RECORD", &record_path);
        let transport = ShellTransport::new(fake_tool()).with_timeout(Duration::from_secs(3));

        let mut devices = transport.list_devices().unwrap();
        observe_connection_generations(&mut devices);
        assert_eq!(
            devices
                .iter()
                .filter(|device| matches!(device.state, DeviceState::Other(_)))
                .count(),
            case.expectations.unknown_device_count,
            "{}",
            case.id
        );
        assert!(
            devices
                .iter()
                .filter(|device| matches!(device.state, DeviceState::Other(_)))
                .all(|device| !device.state.is_actionable()),
            "{}",
            case.id
        );
        let device = devices
            .iter()
            .find(|device| device.serial == case.expectations.serial)
            .unwrap();
        assert_eq!(
            device.model.as_deref(),
            Some(case.expectations.model.as_str())
        );
        assert_eq!(device.transport_id, Some(case.expectations.transport_id));
        let target = device.target();

        let info = get_device_info(&transport, &target).unwrap();
        assert_eq!(
            info.manufacturer.as_deref(),
            Some(case.expectations.manufacturer.as_str()),
            "{}",
            case.id
        );
        assert_eq!(
            info.storage_partitions.len(),
            case.expectations.storage_partition_count,
            "{}",
            case.id
        );

        let packages =
            list_packages_with_capability(&transport, &target, PackageFilter::All, 0).unwrap();
        assert_eq!(
            packages.packages.len(),
            case.expectations.package_count,
            "{}",
            case.id
        );
        assert_eq!(
            packages
                .packages
                .iter()
                .all(|package| package.uid.is_some() && package.installer.is_some()),
            case.expectations.enriched_package_metadata,
            "{}",
            case.id
        );

        let users = list_users(&transport, &target).unwrap();
        assert_eq!(users.len(), case.expectations.user_count, "{}", case.id);
        assert_eq!(
            users.iter().find(|user| user.current).map(|user| user.id),
            Some(case.expectations.current_user),
            "{}",
            case.id
        );

        let service_output = transport
            .shell_target(&target, &["dumpsys", "activity", "services"])
            .unwrap();
        assert_eq!(
            parse_running_services(&service_output).len(),
            case.expectations.service_count,
            "{}",
            case.id
        );
        assert_eq!(
            list_mdns_services(&transport).unwrap().len(),
            case.expectations.mdns_service_count,
            "{}",
            case.id
        );

        let invocations = fs::read_to_string(&record_path).unwrap();
        let used_enriched = invocations
            .lines()
            .any(|line| line.contains(r#""-U","-i""#));
        let used_core_fallback = invocations.lines().any(|line| {
            line.contains(r#""packages","--user","0","-e","-f"]"#)
                || line.contains(r#""packages","--user","0","-d","-f"]"#)
        });
        assert!(used_enriched, "{}", case.id);
        assert_eq!(
            used_core_fallback, case.expectations.enriched_fallback_used,
            "{}",
            case.id
        );

        std::env::remove_var("DROIDSMITH_ADB_CORPUS_CASE");
        std::env::remove_var("DROIDSMITH_ADB_CORPUS_RECORD");
        fs::remove_dir_all(run_dir).unwrap();
    }
}

struct FixedTransport {
    devices: Vec<Device>,
}

impl AdbTransport for FixedTransport {
    fn list_devices(&self) -> Result<Vec<Device>, TransportError> {
        Ok(self.devices.clone())
    }

    fn shell(&self, _serial: &str, _args: &[&str]) -> Result<String, TransportError> {
        Err(TransportError::Parse("unexpected shell probe".to_string()))
    }
}

fn device(serial: &str, build: &str) -> Device {
    Device {
        serial: serial.to_string(),
        state: DeviceState::Device,
        model: Some("Pixel".to_string()),
        product: Some("panther".to_string()),
        device: Some("panther".to_string()),
        build_fingerprint: Some(build.to_string()),
        transport_id: Some(77),
        connection_generation: 0,
        transport_kind: droidsmith_lib::adb::DeviceTransportKind::Usb,
        wireless: false,
    }
}

fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("condition did not become true within {timeout:?}");
}

fn run_fake_tool(args: &[String]) {
    match args.first().map(String::as_str) {
        Some("emit") => {
            record_invocation(args);
            println!("stdout-雪");
            eprintln!("stderr-qa");
        }
        Some("fail") => {
            record_invocation(args);
            println!("partial-output");
            eprintln!("exact failure text");
            std::process::exit(23);
        }
        Some("flood") => {
            let mut stdout = std::io::stdout().lock();
            let mut stderr = std::io::stderr().lock();
            stdout.write_all(&vec![b'a'; 2 * 1024 * 1024]).unwrap();
            stdout.write_all(b"STDOUT-END").unwrap();
            stderr.write_all(&vec![b'b'; 2 * 1024 * 1024]).unwrap();
            stderr.write_all(b"STDERR-END").unwrap();
        }
        Some("capture-stdout") => write_flood(true, false),
        Some("capture-stderr") => write_flood(false, true),
        Some("capture-both") => write_flood(true, true),
        Some("tree") => spawn_descendant_and_wait(args),
        Some("grandchild") => {
            let pid_path = args.get(1).expect("grandchild pid path");
            fs::write(pid_path, std::process::id().to_string()).unwrap();
            std::thread::sleep(Duration::from_secs(30));
        }
        _ => unreachable!("fake tool dispatch checks the command"),
    }
}

fn run_fake_adb(args: &[String]) {
    let case_path = std::env::var_os("DROIDSMITH_ADB_CORPUS_CASE").unwrap();
    let case: TranscriptCase =
        serde_json::from_slice(&fs::read(PathBuf::from(case_path)).unwrap()).unwrap();
    if let Some(record_path) = std::env::var_os("DROIDSMITH_ADB_CORPUS_RECORD") {
        let mut record = OpenOptions::new()
            .create(true)
            .append(true)
            .open(record_path)
            .unwrap();
        serde_json::to_writer(&mut record, args).unwrap();
        record.write_all(b"\n").unwrap();
    }
    let Some(command) = case.commands.iter().find(|command| command.argv == args) else {
        eprintln!(
            "unmatched sanitized ADB transcript command in {}: {}",
            case.id,
            serde_json::to_string(args).unwrap()
        );
        std::process::exit(64);
    };
    std::io::stdout()
        .lock()
        .write_all(command.stdout.as_bytes())
        .unwrap();
    std::io::stderr()
        .lock()
        .write_all(command.stderr.as_bytes())
        .unwrap();
    if command.exit_code != 0 {
        std::process::exit(command.exit_code);
    }
}

fn write_flood(write_stdout: bool, write_stderr: bool) {
    const FLOOD_BYTES: usize = 8 * 1024 * 1024;
    let barrier = Arc::new(std::sync::Barrier::new(
        usize::from(write_stdout) + usize::from(write_stderr),
    ));
    let mut writers = Vec::new();
    if write_stdout {
        let barrier = Arc::clone(&barrier);
        writers.push(std::thread::spawn(move || {
            let mut stdout = std::io::stdout().lock();
            barrier.wait();
            let _ = stdout.write_all(&vec![b'o'; FLOOD_BYTES]);
        }));
    }
    if write_stderr {
        let barrier = Arc::clone(&barrier);
        writers.push(std::thread::spawn(move || {
            let mut stderr = std::io::stderr().lock();
            barrier.wait();
            let _ = stderr.write_all(&vec![b'e'; FLOOD_BYTES]);
        }));
    }
    for writer in writers {
        let _ = writer.join();
    }
}

fn record_invocation(args: &[String]) {
    let mut stdin = Vec::new();
    std::io::stdin().read_to_end(&mut stdin).unwrap();
    if let Some(path) = std::env::var_os("DROIDSMITH_FAKE_TOOL_RECORD") {
        let value = serde_json::json!({
            "args": args,
            "stdinBytes": stdin.len(),
        });
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
}

fn spawn_descendant_and_wait(args: &[String]) {
    let pid_path = args.get(1).expect("tree pid path");
    let executable = std::env::current_exe().unwrap();
    let mut command = Command::new(executable);
    command
        .arg("grandchild")
        .arg(pid_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().unwrap();
    let _ = child.wait();
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use std::os::windows::process::CommandExt;

    let output = Command::new("tasklist.exe")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
