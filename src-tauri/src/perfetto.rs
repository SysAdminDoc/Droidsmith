//! Bounded, local-only Perfetto system-trace capture.
//!
//! Droidsmith supplies a fixed text-proto config through stdin, captures to a
//! backend-owned remote path, checks the remote size before pulling, installs
//! the host artifact atomically, and attempts device cleanup on every exit.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::adb::transport::{AdbTransport, TransportError};
use crate::adb::DeviceTarget;
use crate::fs_util::{ArtifactError, ArtifactKind, HostArtifact, StagedArtifact};
use crate::operations::{EventSink, OperationError, ProcessOutput, RegisteredOperation};

const MIN_PERFETTO_SDK: u32 = 29;
const STAT_TIMEOUT: Duration = Duration::from_secs(10);
const PULL_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PerfettoUnavailableReason {
    AndroidVersion,
    ToolUnavailable,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PerfettoPreset {
    pub id: String,
    pub duration_secs: u32,
    pub buffer_size_mb: u32,
    pub max_output_bytes: u64,
    pub data_sources: Vec<String>,
    pub atrace_categories: Vec<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PerfettoCapabilities {
    pub supported: bool,
    pub sdk_level: Option<u32>,
    pub unavailable_reason: Option<PerfettoUnavailableReason>,
    pub presets: Vec<PerfettoPreset>,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PerfettoCaptureResult {
    pub artifact: HostArtifact,
    pub preset_id: String,
    pub duration_secs: u32,
    pub buffer_size_mb: u32,
    pub max_output_bytes: u64,
    pub captured_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PerfettoError {
    #[error("Perfetto is not supported by this device")]
    Unsupported,
    #[error("unknown Perfetto preset {0:?}")]
    InvalidPreset(String),
    #[error("trace destination must use the .perfetto-trace extension")]
    InvalidExtension,
    #[error("Perfetto capture failed: {0}")]
    Capture(String),
    #[error("Perfetto produced an invalid trace: {0}")]
    InvalidTrace(String),
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
    #[error(transparent)]
    Operation(#[from] OperationError),
    #[error("could not inspect the trace artifact: {0}")]
    Io(#[from] std::io::Error),
}

impl PerfettoError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unsupported => "perfetto_unsupported",
            Self::InvalidPreset(_) => "perfetto_invalid_preset",
            Self::InvalidExtension => "perfetto_invalid_extension",
            Self::Capture(_) => "perfetto_capture_failed",
            Self::InvalidTrace(_) => "perfetto_invalid_trace",
            Self::Artifact(error) => error.code(),
            Self::Operation(OperationError::Cancelled) => "operation_cancelled",
            Self::Operation(OperationError::Timeout(_)) => "operation_timeout",
            Self::Operation(OperationError::OutputTooLarge(_)) => "operation_output_too_large",
            Self::Operation(_) => "perfetto_operation_failed",
            Self::Io(_) => "perfetto_io_failed",
        }
    }
}

struct PresetDefinition {
    id: &'static str,
    duration_secs: u32,
    buffer_size_mb: u32,
    max_output_bytes: u64,
    data_sources: &'static [&'static str],
    atrace_categories: &'static [&'static str],
    ftrace_events: &'static [&'static str],
}

const MIB: u64 = 1024 * 1024;
const PRESETS: &[PresetDefinition] = &[
    PresetDefinition {
        id: "ui_rendering",
        duration_secs: 10,
        buffer_size_mb: 32,
        max_output_bytes: 64 * MIB,
        data_sources: &[
            "linux.ftrace",
            "linux.process_stats",
            "android.packages_list",
        ],
        atrace_categories: &["gfx", "view", "wm", "am", "input", "binder_driver"],
        ftrace_events: &[
            "sched/sched_switch",
            "sched/sched_waking",
            "power/cpu_frequency",
            "power/cpu_idle",
        ],
    },
    PresetDefinition {
        id: "app_startup",
        duration_secs: 15,
        buffer_size_mb: 32,
        max_output_bytes: 64 * MIB,
        data_sources: &[
            "linux.ftrace",
            "linux.process_stats",
            "android.packages_list",
        ],
        atrace_categories: &["am", "wm", "gfx", "view", "binder_driver", "dalvik"],
        ftrace_events: &[
            "sched/sched_switch",
            "sched/sched_waking",
            "power/cpu_frequency",
        ],
    },
    PresetDefinition {
        id: "system_health",
        duration_secs: 30,
        buffer_size_mb: 48,
        max_output_bytes: 64 * MIB,
        data_sources: &["linux.ftrace", "linux.process_stats", "linux.sys_stats"],
        atrace_categories: &["am", "wm", "binder_driver", "power", "memory"],
        ftrace_events: &[
            "sched/sched_switch",
            "sched/sched_waking",
            "power/cpu_frequency",
            "power/cpu_idle",
            "lowmemorykiller/lowmemory_kill",
        ],
    },
];

pub fn capabilities(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
) -> Result<PerfettoCapabilities, TransportError> {
    let sdk_level = transport
        .shell_target(target, &["getprop", "ro.build.version.sdk"])?
        .trim()
        .parse::<u32>()
        .ok();
    if sdk_level.map_or(true, |sdk| sdk < MIN_PERFETTO_SDK) {
        return Ok(PerfettoCapabilities {
            supported: false,
            sdk_level,
            unavailable_reason: Some(PerfettoUnavailableReason::AndroidVersion),
            presets: presets(),
        });
    }

    match transport.shell_target(target, &["perfetto", "--help"]) {
        Ok(_) => Ok(PerfettoCapabilities {
            supported: true,
            sdk_level,
            unavailable_reason: None,
            presets: presets(),
        }),
        Err(TransportError::Exit { .. } | TransportError::Signaled { .. }) => {
            Ok(PerfettoCapabilities {
                supported: false,
                sdk_level,
                unavailable_reason: Some(PerfettoUnavailableReason::ToolUnavailable),
                presets: presets(),
            })
        }
        Err(error) => Err(error),
    }
}

pub fn presets() -> Vec<PerfettoPreset> {
    PRESETS.iter().map(PresetDefinition::to_wire).collect()
}

impl PresetDefinition {
    fn to_wire(&self) -> PerfettoPreset {
        PerfettoPreset {
            id: self.id.to_string(),
            duration_secs: self.duration_secs,
            buffer_size_mb: self.buffer_size_mb,
            max_output_bytes: self.max_output_bytes,
            data_sources: self
                .data_sources
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            atrace_categories: self
                .atrace_categories
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
        }
    }
}

pub fn capture(
    adb_path: &Path,
    target: &DeviceTarget,
    destination: &Path,
    preset_id: &str,
    operation_id: &str,
    sink: EventSink,
) -> Result<PerfettoCaptureResult, PerfettoError> {
    let preset = preset_definition(preset_id)?;
    let remote_path = format!(
        "/data/misc/perfetto-traces/droidsmith-{}.perfetto-trace",
        uuid::Uuid::new_v4().simple()
    );
    let mut operation = RegisteredOperation::new(
        operation_id,
        "Capturing sensitive local Perfetto trace",
        sink,
    )?;
    let mut runner = OperationRunner {
        operation: &mut operation,
        adb_path,
        target,
    };
    let result = capture_with_runner(destination, preset, &remote_path, &mut runner);
    match &result {
        Ok(_) => operation.finish("Perfetto trace captured and device temporary removed"),
        Err(PerfettoError::Operation(OperationError::Cancelled)) => {
            operation.cancelled("Perfetto capture cancelled; device cleanup attempted")
        }
        Err(_) => {}
    }
    result
}

fn preset_definition(id: &str) -> Result<&'static PresetDefinition, PerfettoError> {
    PRESETS
        .iter()
        .find(|preset| preset.id == id)
        .ok_or_else(|| PerfettoError::InvalidPreset(id.to_string()))
}

trait TraceRunner {
    fn capture(
        &mut self,
        remote_path: &str,
        config: &[u8],
        timeout: Duration,
    ) -> Result<ProcessOutput, OperationError>;
    fn stat(&mut self, remote_path: &str) -> Result<ProcessOutput, OperationError>;
    fn pull(
        &mut self,
        remote_path: &str,
        host_path: &Path,
    ) -> Result<ProcessOutput, OperationError>;
    fn cleanup(&mut self, remote_path: &str) -> Result<ProcessOutput, OperationError>;
}

struct OperationRunner<'a> {
    operation: &'a mut RegisteredOperation,
    adb_path: &'a Path,
    target: &'a DeviceTarget,
}

impl OperationRunner<'_> {
    fn args(&self, command: &[&str]) -> Vec<String> {
        let mut args = self.target.adb_selector();
        args.extend(command.iter().map(|value| (*value).to_string()));
        args
    }
}

impl TraceRunner for OperationRunner<'_> {
    fn capture(
        &mut self,
        remote_path: &str,
        config: &[u8],
        timeout: Duration,
    ) -> Result<ProcessOutput, OperationError> {
        let args = self.args(&["shell", "perfetto", "--txt", "-c", "-", "-o", remote_path]);
        self.operation.run_stage_with_input(
            self.adb_path,
            &args,
            config,
            timeout,
            "Recording Perfetto sources on the device",
        )
    }

    fn stat(&mut self, remote_path: &str) -> Result<ProcessOutput, OperationError> {
        let args = self.args(&["shell", "stat", "-c", "%s", remote_path]);
        self.operation.run_stage(
            self.adb_path,
            &args,
            STAT_TIMEOUT,
            "Checking trace size before transfer",
        )
    }

    fn pull(
        &mut self,
        remote_path: &str,
        host_path: &Path,
    ) -> Result<ProcessOutput, OperationError> {
        let host = host_path.display().to_string();
        let args = self.args(&["pull", remote_path, &host]);
        self.operation.run_stage(
            self.adb_path,
            &args,
            PULL_TIMEOUT,
            "Pulling the bounded trace to its atomic host stage",
        )
    }

    fn cleanup(&mut self, remote_path: &str) -> Result<ProcessOutput, OperationError> {
        let args = self.args(&["shell", "rm", "-f", remote_path]);
        self.operation.run_cleanup_stage(
            self.adb_path,
            &args,
            CLEANUP_TIMEOUT,
            "Removing the device trace temporary",
        )
    }
}

fn capture_with_runner(
    destination: &Path,
    preset: &PresetDefinition,
    remote_path: &str,
    runner: &mut dyn TraceRunner,
) -> Result<PerfettoCaptureResult, PerfettoError> {
    if !destination
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.to_ascii_lowercase().ends_with(".perfetto-trace"))
    {
        return Err(PerfettoError::InvalidExtension);
    }

    let staged = StagedArtifact::new(destination)?;
    let config = build_config(preset);
    let capture_timeout = Duration::from_secs(u64::from(preset.duration_secs) + 30);
    let workflow = (|| {
        ensure_success(
            runner.capture(remote_path, config.as_bytes(), capture_timeout)?,
            "perfetto",
        )?;
        let size_output = runner.stat(remote_path)?;
        let size_text = ensure_success(size_output, "adb shell stat")?;
        let remote_size = size_text
            .trim()
            .parse::<u64>()
            .map_err(|_| PerfettoError::InvalidTrace("device size was not numeric".to_string()))?;
        if remote_size == 0 {
            return Err(PerfettoError::InvalidTrace(
                "device trace was empty".to_string(),
            ));
        }
        if remote_size > preset.max_output_bytes {
            return Err(OperationError::OutputTooLarge(preset.max_output_bytes).into());
        }
        ensure_success(
            runner.pull(remote_path, staged.path())?,
            "adb pull Perfetto trace",
        )?;
        let local_size = fs::metadata(staged.path())?.len();
        if local_size != remote_size {
            return Err(PerfettoError::InvalidTrace(format!(
                "pulled size {local_size} did not match device size {remote_size}"
            )));
        }
        Ok(())
    })();

    let workflow_error = workflow.err();
    let cleanup = runner.cleanup(remote_path);
    if let Some(error) = workflow_error {
        return Err(error);
    }
    ensure_success(cleanup?, "adb shell rm Perfetto temporary")?;

    let artifact = staged.commit(ArtifactKind::AnyFile)?;
    Ok(PerfettoCaptureResult {
        artifact,
        preset_id: preset.id.to_string(),
        duration_secs: preset.duration_secs,
        buffer_size_mb: preset.buffer_size_mb,
        max_output_bytes: preset.max_output_bytes,
        captured_at: crate::time::iso_utc_now(),
    })
}

fn ensure_success(output: ProcessOutput, label: &str) -> Result<String, PerfettoError> {
    if output.success() {
        return Ok(output.stdout);
    }
    let detail = if output.stderr.trim().is_empty() {
        output.stdout.trim()
    } else {
        output.stderr.trim()
    };
    Err(PerfettoError::Capture(format!(
        "{label} exited with code {}: {}",
        output
            .code
            .map_or_else(|| "signal".to_string(), |code| code.to_string()),
        detail.chars().take(4096).collect::<String>()
    )))
}

fn build_config(preset: &PresetDefinition) -> String {
    let mut config = format!(
        "buffers: {{ size_kb: {} fill_policy: RING_BUFFER }}\n\
         duration_ms: {}\n\
         write_into_file: true\n\
         file_write_period_ms: 1000\n\
         max_file_size_bytes: {}\n\
         flush_period_ms: 1000\n",
        preset.buffer_size_mb * 1024,
        preset.duration_secs * 1000,
        preset.max_output_bytes
    );
    for source in preset.data_sources {
        if *source == "linux.ftrace" {
            config.push_str("data_sources: { config { name: \"linux.ftrace\" ftrace_config {\n");
            for event in preset.ftrace_events {
                config.push_str(&format!("ftrace_events: \"{event}\"\n"));
            }
            for category in preset.atrace_categories {
                config.push_str(&format!("atrace_categories: \"{category}\"\n"));
            }
            config.push_str("buffer_size_kb: 2048 drain_period_ms: 250\n} } }\n");
        } else {
            config.push_str(&format!(
                "data_sources: {{ config {{ name: \"{source}\" }} }}\n"
            ));
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::device::{Device, DeviceState, DeviceTransportKind};
    use crate::adb::transport::MockTransport;
    use serde::Deserialize;
    use std::io;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Deserialize)]
    struct CaptureCase {
        name: String,
        outcome: String,
        remote_size: Option<u64>,
        expect_cleanup: bool,
    }

    struct FixtureRunner {
        case: CaptureCase,
        cleanup_attempted: bool,
        pulled: bool,
    }

    impl FixtureRunner {
        fn output(stdout: impl Into<String>) -> ProcessOutput {
            ProcessOutput {
                stdout: stdout.into(),
                stderr: String::new(),
                code: Some(0),
                timed_out: false,
                cancelled: false,
            }
        }
    }

    impl TraceRunner for FixtureRunner {
        fn capture(
            &mut self,
            _remote_path: &str,
            config: &[u8],
            _timeout: Duration,
        ) -> Result<ProcessOutput, OperationError> {
            assert!(String::from_utf8_lossy(config).contains("max_file_size_bytes"));
            match self.case.outcome.as_str() {
                "cancel" => Err(OperationError::Cancelled),
                "disconnect" => Err(OperationError::Wait(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "device disconnected",
                ))),
                _ => Ok(Self::output("")),
            }
        }

        fn stat(&mut self, _remote_path: &str) -> Result<ProcessOutput, OperationError> {
            Ok(Self::output(
                self.case.remote_size.unwrap_or(4096).to_string(),
            ))
        }

        fn pull(
            &mut self,
            _remote_path: &str,
            host_path: &Path,
        ) -> Result<ProcessOutput, OperationError> {
            self.pulled = true;
            fs::write(
                host_path,
                vec![0x5a; self.case.remote_size.unwrap_or(4096) as usize],
            )
            .unwrap();
            Ok(Self::output("pulled"))
        }

        fn cleanup(&mut self, _remote_path: &str) -> Result<ProcessOutput, OperationError> {
            self.cleanup_attempted = true;
            Ok(Self::output(""))
        }
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "droidsmith-perfetto-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn capture_fixtures_cover_success_disconnect_cancel_and_oversize_cleanup() {
        let cases: Vec<CaptureCase> =
            serde_json::from_str(include_str!("../fixtures/perfetto/capture-cases.json")).unwrap();
        for case in cases {
            let dir = test_dir(&case.name);
            let destination = dir.join("capture.perfetto-trace");
            let expect_success = case.outcome == "success";
            let mut runner = FixtureRunner {
                case,
                cleanup_attempted: false,
                pulled: false,
            };
            let result = capture_with_runner(
                &destination,
                &PRESETS[0],
                "/data/misc/perfetto-traces/test.perfetto-trace",
                &mut runner,
            );
            assert_eq!(result.is_ok(), expect_success, "{}", runner.case.name);
            assert_eq!(
                runner.cleanup_attempted, runner.case.expect_cleanup,
                "{}",
                runner.case.name
            );
            assert_eq!(destination.exists(), expect_success);
            if runner.case.outcome == "oversize" {
                assert!(!runner.pulled, "oversize traces must not be pulled");
            }
        }
    }

    #[test]
    fn capability_fixture_gates_android_9_before_probing_the_tool() {
        let target = Device {
            serial: "SDK28".to_string(),
            state: DeviceState::Device,
            model: None,
            product: None,
            device: None,
            build_fingerprint: Some("fixture/sdk28".to_string()),
            transport_id: Some(1),
            connection_generation: 1,
            transport_kind: DeviceTransportKind::Usb,
            wireless: false,
        }
        .target();
        let transport = MockTransport::default();
        transport.expect_shell(
            "SDK28",
            &["getprop", "ro.build.version.sdk"],
            Ok("28\n".to_string()),
        );
        let capability = capabilities(&transport, &target).unwrap();
        assert!(!capability.supported);
        assert_eq!(
            capability.unavailable_reason,
            Some(PerfettoUnavailableReason::AndroidVersion)
        );
    }

    #[test]
    fn configs_are_bounded_and_contain_only_backend_presets() {
        for preset in PRESETS {
            let config = build_config(preset);
            assert!(config.len() < 16 * 1024);
            assert!(config.contains(&format!("max_file_size_bytes: {}", preset.max_output_bytes)));
            assert!(config.contains("duration_ms:"));
            assert!(config.contains("linux.ftrace"));
        }
    }
}
