use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use droidsmith_lib::adb::device::{observe_connection_generations, Device, DeviceState};
use droidsmith_lib::adb::{
    validate_device_target, AdbTransport, OutputStream, ShellTransport, TransportError,
};
use droidsmith_lib::journal::with_journal;
use droidsmith_lib::operations::{cancel, run_process, EventSink, OperationError};

fn fake_tool() -> PathBuf {
    std::env::current_exe().unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
