//! Transport abstraction over `adb`.
//!
//! Three responsibilities:
//! 1. Define the [`AdbTransport`] trait every consumer (device list,
//!    package enumeration, action runner) talks to.
//! 2. Provide a [`ShellTransport`] that shells out to the resolved
//!    `adb` binary. This is the production implementation.
//! 3. Provide a [`MockTransport`] for tests: scripted device list +
//!    scripted shell responses, no child processes.
//!
//! The trait is **synchronous** by design for v0.1: Tauri commands
//! that want to keep the UI responsive can wrap calls in
//! `tauri::async_runtime::spawn_blocking`. We may revisit if we hit a
//! call site that genuinely needs streaming output (logcat — R-051).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::adb::device::{
    attach_transport_provenance, looks_wireless, observe_connection_generations, valid_serial,
    Device, DeviceState, DeviceTarget, DeviceTransportKind,
};

/// Default timeout for non-streaming `adb` calls. Two seconds is enough
/// for `devices`, `shell`, and most metadata reads; longer-running flows
/// (install, logcat, scrcpy) take per-call overrides.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
    Both,
}

impl std::fmt::Display for OutputStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Both => "stdout and stderr",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("adb binary not available")]
    AdbNotFound,
    #[error("failed to spawn adb: {0}")]
    Spawn(std::io::Error),
    #[error("adb exited with code {code}: {stderr}")]
    Exit { code: i32, stderr: String },
    #[error("adb killed by signal; stderr: {stderr}")]
    Signaled { stderr: String },
    #[error("adb timed out after {0:?}")]
    Timeout(Duration),
    #[error("adb {stream} exceeded the {limit_bytes}-byte capture limit")]
    OutputLimit {
        stream: OutputStream,
        limit_bytes: usize,
    },
    #[error("could not parse adb output: {0}")]
    Parse(String),
}

impl serde::Serialize for TransportError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

// Transport errors serialize as a single user-facing string at the IPC
// boundary; mirror that exact wire shape in generated TypeScript bindings.
impl specta::Type for TransportError {
    fn inline(
        type_map: &mut specta::TypeCollection,
        generics: specta::Generics,
    ) -> specta::datatype::DataType {
        <String as specta::Type>::inline(type_map, generics)
    }
}

pub trait AdbTransport: Send + Sync {
    fn list_devices(&self) -> Result<Vec<Device>, TransportError>;

    /// Run `adb -s <serial> shell <args>` and return stdout. Trailing
    /// newline is preserved; callers strip if they want a one-liner.
    fn shell(&self, serial: &str, args: &[&str]) -> Result<String, TransportError>;

    /// Run a device shell through a previously validated immutable target.
    /// Test transports can continue matching by serial; the production
    /// transport overrides this to use `adb -t` when available.
    fn shell_target(&self, target: &DeviceTarget, args: &[&str]) -> Result<String, TransportError> {
        self.shell(&target.serial, args)
    }
}

// ---- ShellTransport -----------------------------------------------------

#[derive(Debug, Clone)]
pub struct ShellTransport {
    pub adb_path: PathBuf,
    pub timeout: Duration,
}

impl ShellTransport {
    pub fn new(adb_path: impl Into<PathBuf>) -> Self {
        Self {
            adb_path: adb_path.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Override the default timeout. Reserved for the streaming
    /// logcat / install flows in R-051 / R-023 where the global
    /// `DEFAULT_TIMEOUT` is too short.
    #[allow(dead_code)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn run(&self, args: &[&str]) -> Result<String, TransportError> {
        run_capture(&self.adb_path, args, self.timeout)
    }

    /// Run a raw `adb` command that is not device-shell scoped. Kept on
    /// the concrete shell transport so the trait only exposes
    /// device-level operations.
    pub fn adb(&self, args: &[&str]) -> Result<String, TransportError> {
        self.run(args)
    }

    /// Run a non-shell command (`push`, `pull`, `install`, ...) through a
    /// validated target selector.
    pub fn adb_target(
        &self,
        target: &DeviceTarget,
        args: &[&str],
    ) -> Result<String, TransportError> {
        let selector = target.adb_selector();
        let mut full: Vec<&str> = selector.iter().map(String::as_str).collect();
        full.extend_from_slice(args);
        self.run(&full)
    }
}

impl AdbTransport for ShellTransport {
    fn list_devices(&self) -> Result<Vec<Device>, TransportError> {
        let stdout = self.run(&["devices", "-l"])?;
        let mut devices = parse_devices_long(&stdout)?;
        attach_transport_provenance(&mut devices);
        Ok(devices)
    }

    fn shell(&self, serial: &str, args: &[&str]) -> Result<String, TransportError> {
        let mut full = Vec::with_capacity(args.len() + 3);
        full.push("-s");
        full.push(serial);
        full.push("shell");
        full.extend_from_slice(args);
        self.run(&full)
    }

    fn shell_target(&self, target: &DeviceTarget, args: &[&str]) -> Result<String, TransportError> {
        let selector = target.adb_selector();
        let mut full: Vec<&str> = selector.iter().map(String::as_str).collect();
        full.push("shell");
        full.extend_from_slice(args);
        self.run(&full)
    }
}

/// Re-resolve a captured target immediately before an operation. This is the
/// central fail-closed guard against duplicate serials, reconnects, stale UI
/// state, and non-actionable ADB states.
pub fn validate_device_target(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
) -> Result<Device, TransportError> {
    if !valid_serial(&target.serial) || target.connection_generation == 0 {
        return Err(TransportError::Parse(
            "device target is missing a valid serial or connection generation".to_string(),
        ));
    }
    if target
        .build_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(TransportError::Parse(
            "device target is missing a verified build fingerprint".to_string(),
        ));
    }

    let mut devices = transport.list_devices()?;
    observe_connection_generations(&mut devices);
    let mut matches: Vec<Device> = match target.transport_id {
        Some(id) => devices
            .into_iter()
            .filter(|device| device.transport_id == Some(id))
            .collect(),
        None => devices
            .into_iter()
            .filter(|device| device.serial == target.serial)
            .collect(),
    };
    if matches.len() != 1 {
        return Err(TransportError::Parse(format!(
            "device target is missing or ambiguous (serial {:?}, transport {:?})",
            target.serial, target.transport_id
        )));
    }
    let mut actual = matches.remove(0);
    if actual.build_fingerprint.is_none() {
        let probe = actual.target();
        let fingerprint = transport
            .shell_target(&probe, &["getprop", "ro.build.fingerprint"])?
            .trim()
            .to_string();
        if fingerprint.is_empty() {
            return Err(TransportError::Parse(
                "device did not report a build fingerprint".to_string(),
            ));
        }
        actual.build_fingerprint = Some(fingerprint);
    }
    if actual.serial != target.serial
        || actual.connection_generation != target.connection_generation
        || actual.model != target.model
        || actual.product != target.product
        || actual.device != target.device
        || actual.build_fingerprint != target.build_fingerprint
        || actual.transport_kind != target.transport_kind
    {
        return Err(TransportError::Parse(format!(
            "device target changed; refresh the device list before continuing (serial {:?})",
            target.serial
        )));
    }
    if !actual.state.is_actionable() {
        return Err(TransportError::Parse(format!(
            "device target {:?} is not authorized/actionable ({:?})",
            target.serial, actual.state
        )));
    }
    Ok(actual)
}

// ---- Parsing ------------------------------------------------------------

/// Parse the output of `adb devices -l`. Format (from
/// platform-tools/services/adbd/services.cpp):
///
/// ```text
/// List of devices attached
/// emulator-5554          device product:sdk_gphone64_x86_64 model:sdk_gphone_x86_64 device:emu64x transport_id:1
/// R5CT60ZQR4M            unauthorized usb:1-2 transport_id:2
/// 192.168.1.42:5555      device product:redfin model:Pixel_5 device:redfin transport_id:3
/// ```
///
/// The first line is a header. Subsequent lines are tab/space-separated
/// `serial <state> [k:v]...`. We tolerate either separator and skip
/// blank lines.
pub fn parse_devices_long(stdout: &str) -> Result<Vec<Device>, TransportError> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        // `adb` can emit the header at idx 0 _or_ later when daemon
        // startup messages precede it. Match unconditionally.
        if line.starts_with("List of devices") {
            continue;
        }
        // Daemon startup chatter — `* daemon not running ...` etc.
        if line.starts_with("* ") || line.starts_with("adb server") {
            continue;
        }

        // Split into tokens, preserving order. The state token can
        // contain spaces (`no permissions ...`) — handled below.
        let mut tokens = line.split_whitespace();
        let serial = match tokens.next() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let state_token = match tokens.next() {
            Some(s) => s,
            None => continue,
        };

        // Reassemble "no permissions ..." into a single state.
        let (state, rest_tokens) = if state_token == "no" {
            // Consume until we hit a k:v token; everything up to that
            // is the state text.
            let mut state_buf = String::from("no");
            let mut kv_tokens: Vec<&str> = Vec::new();
            for tok in tokens {
                if tok.contains(':') && !tok.contains('/') {
                    kv_tokens.push(tok);
                } else if kv_tokens.is_empty() {
                    state_buf.push(' ');
                    state_buf.push_str(tok);
                } else {
                    // After we've started collecting k:v tokens, an
                    // unparsable token is a parse failure — but we'd
                    // rather degrade gracefully than refuse the entire
                    // device list.
                    kv_tokens.push(tok);
                }
            }
            (DeviceState::parse(&state_buf), kv_tokens)
        } else {
            (DeviceState::parse(state_token), tokens.collect::<Vec<_>>())
        };

        let mut device = Device {
            wireless: looks_wireless(&serial),
            transport_kind: if looks_wireless(&serial) {
                DeviceTransportKind::UnknownTcp
            } else {
                DeviceTransportKind::Usb
            },
            serial,
            state,
            model: None,
            product: None,
            device: None,
            build_fingerprint: None,
            transport_id: None,
            connection_generation: 0,
        };

        for tok in rest_tokens {
            if let Some((k, v)) = tok.split_once(':') {
                match k {
                    "product" => device.product = Some(v.to_string()),
                    "model" => device.model = Some(v.to_string()),
                    "device" => device.device = Some(v.to_string()),
                    "transport_id" => device.transport_id = v.parse().ok(),
                    _ => { /* ignore unknown keys; new adb versions add them */ }
                }
            }
        }

        out.push(device);
    }
    Ok(out)
}

// ---- run_capture --------------------------------------------------------

/// Run a child with stdin closed, stdout+stderr piped, and a hard wall
/// clock. Reads stdout AND stderr on worker threads to avoid the pipe-
/// buffer deadlock fixed in the audit pass.
fn run_capture(program: &Path, args: &[&str], timeout: Duration) -> Result<String, TransportError> {
    let mut command = Command::new(program);
    command.args(args);
    let output = crate::process_capture::run(
        &mut command,
        timeout,
        crate::process_capture::CaptureLimits::default(),
    )
    .map_err(capture_error)?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    match output.termination {
        crate::process_capture::CaptureTermination::TimedOut => {
            Err(TransportError::Timeout(timeout))
        }
        crate::process_capture::CaptureTermination::OutputLimitExceeded {
            stream,
            limit_bytes,
        } => Err(TransportError::OutputLimit {
            stream: output_stream(stream),
            limit_bytes,
        }),
        crate::process_capture::CaptureTermination::Exited(status) if status.success() => {
            Ok(stdout)
        }
        crate::process_capture::CaptureTermination::Exited(status) => match status.code() {
            Some(code) => Err(TransportError::Exit { code, stderr }),
            None => Err(TransportError::Signaled { stderr }),
        },
    }
}

pub(crate) fn capture_error(error: crate::process_capture::CaptureError) -> TransportError {
    match error {
        crate::process_capture::CaptureError::Spawn(error) => TransportError::Spawn(error),
        error => TransportError::Parse(format!("subprocess capture failed: {error}")),
    }
}

pub(crate) const fn output_stream(stream: crate::process_capture::CaptureStream) -> OutputStream {
    match stream {
        crate::process_capture::CaptureStream::Stdout => OutputStream::Stdout,
        crate::process_capture::CaptureStream::Stderr => OutputStream::Stderr,
        crate::process_capture::CaptureStream::Both => OutputStream::Both,
    }
}

// ---- MockTransport (test-only) -----------------------------------------

#[cfg(test)]
pub use mock::MockTransport;

#[cfg(test)]
mod mock {
    use std::sync::Mutex;

    use super::*;

    /// In-memory transport for tests. Scripted device list + per-serial
    /// scripted shell response map.
    pub struct MockTransport {
        pub devices: Mutex<Vec<Device>>,
        shell_responses: Mutex<Vec<MockShellResponse>>,
    }

    struct MockShellResponse {
        serial: String,
        args: Vec<String>,
        result: Result<String, TransportError>,
    }

    impl MockTransport {
        pub fn new() -> Self {
            Self {
                devices: Mutex::new(Vec::new()),
                shell_responses: Mutex::new(Vec::new()),
            }
        }

        pub fn with_devices(self, ds: Vec<Device>) -> Self {
            *self.devices.lock().unwrap() = ds;
            self
        }

        /// Register a canned shell response. Match is exact on `(serial, args)`.
        pub fn expect_shell(
            &self,
            serial: &str,
            args: &[&str],
            result: Result<String, TransportError>,
        ) {
            self.shell_responses
                .lock()
                .unwrap()
                .push(MockShellResponse {
                    serial: serial.to_string(),
                    args: args.iter().map(|s| (*s).to_string()).collect(),
                    result,
                });
        }
    }

    impl Default for MockTransport {
        fn default() -> Self {
            Self::new()
        }
    }

    impl AdbTransport for MockTransport {
        fn list_devices(&self) -> Result<Vec<Device>, TransportError> {
            Ok(self.devices.lock().unwrap().clone())
        }

        fn shell(&self, serial: &str, args: &[&str]) -> Result<String, TransportError> {
            let mut responses = self.shell_responses.lock().unwrap();
            if let Some(idx) = responses.iter().position(|r| {
                r.serial == serial && r.args.iter().map(String::as_str).eq(args.iter().copied())
            }) {
                let response = responses.remove(idx);
                // Re-create the result since TransportError isn't Clone.
                return match response.result {
                    Ok(s) => Ok(s),
                    Err(e) => Err(remake_error(&e)),
                };
            }
            Err(TransportError::Parse(format!(
                "MockTransport: no scripted response for serial={serial:?} args={args:?}"
            )))
        }
    }

    fn remake_error(e: &TransportError) -> TransportError {
        match e {
            TransportError::AdbNotFound => TransportError::AdbNotFound,
            TransportError::Spawn(io) => {
                TransportError::Spawn(std::io::Error::new(io.kind(), e.to_string()))
            }
            TransportError::Exit { code, stderr } => TransportError::Exit {
                code: *code,
                stderr: stderr.clone(),
            },
            TransportError::Signaled { stderr } => TransportError::Signaled {
                stderr: stderr.clone(),
            },
            TransportError::Timeout(d) => TransportError::Timeout(*d),
            TransportError::OutputLimit {
                stream,
                limit_bytes,
            } => TransportError::OutputLimit {
                stream: *stream,
                limit_bytes: *limit_bytes,
            },
            TransportError::Parse(s) => TransportError::Parse(s.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(serial: &str, transport_id: Option<u32>, build: &str) -> Device {
        Device {
            serial: serial.into(),
            state: DeviceState::Device,
            model: Some("Pixel".into()),
            product: Some("panther".into()),
            device: Some("panther".into()),
            build_fingerprint: Some(build.into()),
            transport_id,
            connection_generation: 0,
            transport_kind: DeviceTransportKind::Usb,
            wireless: false,
        }
    }

    #[test]
    fn parse_devices_long_real_output() {
        let s = "\
List of devices attached
emulator-5554          device product:sdk_gphone64_x86_64 model:sdk_gphone_x86_64 device:emu64x transport_id:1
R5CT60ZQR4M            unauthorized usb:1-2 transport_id:2
192.168.1.42:5555      device product:redfin model:Pixel_5 device:redfin transport_id:3
";
        let devices = parse_devices_long(s).unwrap();
        assert_eq!(devices.len(), 3);

        assert_eq!(devices[0].serial, "emulator-5554");
        assert_eq!(devices[0].state, DeviceState::Device);
        assert_eq!(devices[0].model.as_deref(), Some("sdk_gphone_x86_64"));
        assert_eq!(devices[0].transport_id, Some(1));
        assert!(!devices[0].wireless);

        assert_eq!(devices[1].state, DeviceState::Unauthorized);
        assert_eq!(devices[1].model, None);

        assert_eq!(devices[2].serial, "192.168.1.42:5555");
        assert!(devices[2].wireless);
        assert_eq!(devices[2].model.as_deref(), Some("Pixel_5"));
    }

    #[test]
    fn parse_devices_long_handles_empty_list() {
        let s = "List of devices attached\n";
        let devices = parse_devices_long(s).unwrap();
        assert!(devices.is_empty());
    }

    #[test]
    fn parse_devices_long_handles_no_permissions() {
        let s = "\
List of devices attached
0123456789ABCDEF       no permissions (user in plugdev group; missing udev) transport_id:1
";
        let devices = parse_devices_long(s).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].state, DeviceState::NoPermissions);
        assert_eq!(devices[0].transport_id, Some(1));
    }

    #[test]
    fn parse_devices_long_skips_daemon_chatter() {
        let s = "\
* daemon not running; starting now at tcp:5037
* daemon started successfully
List of devices attached
emulator-5554          device transport_id:1
";
        let devices = parse_devices_long(s).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].serial, "emulator-5554");
    }

    #[test]
    fn mock_returns_scripted_devices() {
        let mock = MockTransport::new().with_devices(vec![Device {
            serial: "abc".into(),
            state: DeviceState::Device,
            model: Some("Pixel".into()),
            product: None,
            device: None,
            build_fingerprint: Some("build/test".into()),
            transport_id: Some(1),
            connection_generation: 0,
            transport_kind: DeviceTransportKind::Usb,
            wireless: false,
        }]);
        let devs = mock.list_devices().unwrap();
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].serial, "abc");
    }

    #[test]
    fn mock_returns_scripted_shell_response() {
        let mock = MockTransport::new();
        mock.expect_shell("abc", &["echo", "hello"], Ok("hello\n".into()));
        let out = mock.shell("abc", &["echo", "hello"]).unwrap();
        assert_eq!(out, "hello\n");
    }

    #[test]
    fn mock_consumes_each_response_once() {
        let mock = MockTransport::new();
        mock.expect_shell("abc", &["x"], Ok("first".into()));
        mock.expect_shell("abc", &["x"], Ok("second".into()));
        assert_eq!(mock.shell("abc", &["x"]).unwrap(), "first");
        assert_eq!(mock.shell("abc", &["x"]).unwrap(), "second");
        // Third call has no script left.
        assert!(mock.shell("abc", &["x"]).is_err());
    }

    #[test]
    fn mock_returns_error_for_unknown_serial() {
        let mock = MockTransport::new();
        assert!(matches!(
            mock.shell("nope", &["x"]),
            Err(TransportError::Parse(_))
        ));
    }

    #[test]
    fn transport_id_disambiguates_duplicate_serials() {
        let mock = MockTransport::new().with_devices(vec![
            device("duplicate", Some(4), "build/a"),
            device("duplicate", Some(9), "build/b"),
        ]);
        let target = DeviceTarget {
            serial: "duplicate".into(),
            transport_id: Some(9),
            connection_generation: 10,
            model: Some("Pixel".into()),
            product: Some("panther".into()),
            device: Some("panther".into()),
            build_fingerprint: Some("build/b".into()),
            transport_kind: DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        };
        let validated = validate_device_target(&mock, &target).unwrap();
        assert_eq!(validated.transport_id, Some(9));
        assert_eq!(target.adb_selector(), vec!["-t", "9"]);
    }

    #[test]
    fn serial_fallback_rejects_ambiguous_targets() {
        let mock = MockTransport::new().with_devices(vec![
            device("duplicate", None, "build/a"),
            device("duplicate", None, "build/a"),
        ]);
        let target = DeviceTarget {
            serial: "duplicate".into(),
            transport_id: None,
            connection_generation: 1,
            model: Some("Pixel".into()),
            product: Some("panther".into()),
            device: Some("panther".into()),
            build_fingerprint: Some("build/a".into()),
            transport_kind: DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        };
        assert!(validate_device_target(&mock, &target).is_err());
    }

    #[test]
    fn target_revalidation_rejects_build_or_generation_changes() {
        let mock = MockTransport::new().with_devices(vec![device("abc", Some(2), "build/new")]);
        let target = DeviceTarget {
            serial: "abc".into(),
            transport_id: Some(2),
            connection_generation: 2,
            model: Some("Pixel".into()),
            product: Some("panther".into()),
            device: Some("panther".into()),
            build_fingerprint: Some("build/old".into()),
            transport_kind: DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        };
        assert!(validate_device_target(&mock, &target).is_err());
    }
}
