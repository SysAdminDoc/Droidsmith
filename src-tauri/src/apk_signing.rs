//! Optional APK signature verification through the official Android SDK tool.
//!
//! Static APK analysis never depends on Java or Build Tools. When a compatible
//! `apksigner` 0.9+ is discoverable, this module runs its verifier under the
//! shared subprocess timeout/output limits and reports only verified signer
//! material. Certificate decoding extracts display metadata from the PEM bytes
//! printed by `apksigner`; Droidsmith does not implement signature verification.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};
use x509_parser::pem::Pem;

use crate::process_capture::{CaptureLimits, CaptureTermination, CapturedOutput};

const MIN_APKSIGNER_VERSION: (u32, u32) = (0, 9);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const LINEAGE_TIMEOUT: Duration = Duration::from_secs(30);
const TOOL_OUTPUT_LIMIT: usize = 512 * 1024;
const MAX_DIAGNOSTICS: usize = 32;
const MAX_DIAGNOSTIC_CHARS: usize = 4096;
const MAX_CERTIFICATES: usize = 32;

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApkVerificationStatus {
    Verified,
    Failed,
    NotVerified,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApkVerifierUnavailableReason {
    NotFound,
    JavaUnavailable,
    IncompatibleVersion,
    ProbeFailed,
    ExecutionFailed,
    OutputUnsupported,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApkSignerSource {
    Path,
    AndroidHome,
    AndroidStudio,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApkSignerToolInfo {
    pub version: String,
    pub build_tools_version: Option<String>,
    pub source: ApkSignerSource,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApkSignerCertificate {
    /// Backend-bounded label emitted by the official tool (for example
    /// `Signer #1` or `V3.0 Signer`).
    pub label: String,
    /// Lower-case SHA-256 digest of the DER certificate. This is checked
    /// against the digest printed by `apksigner` before being returned.
    pub sha256: String,
    pub subject: String,
    pub issuer: String,
    pub valid_from_unix: i64,
    pub valid_until_unix: i64,
}

#[derive(specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ApkSignerCapabilities {
    pub installed_data: bool,
    pub shared_uid: bool,
    pub permission: bool,
    pub rollback: bool,
    pub auth: bool,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApkSigningLineageEntry {
    pub position: u32,
    pub certificate: ApkSignerCertificate,
    pub capabilities: ApkSignerCapabilities,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApkSigningLineage {
    /// Proof-of-rotation order reported by `apksigner lineage`: oldest first,
    /// current signer last.
    pub entries: Vec<ApkSigningLineageEntry>,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApkSignatureVerification {
    pub status: ApkVerificationStatus,
    pub unavailable_reason: Option<ApkVerifierUnavailableReason>,
    pub tool: Option<ApkSignerToolInfo>,
    pub verified_schemes: Vec<String>,
    pub signer_count: u32,
    pub signers: Vec<ApkSignerCertificate>,
    pub source_stamp_verified: bool,
    pub source_stamp: Option<ApkSignerCertificate>,
    pub proof_of_rotation: Option<ApkSigningLineage>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl ApkSignatureVerification {
    fn not_verified(reason: ApkVerifierUnavailableReason, tool: Option<ApkSignerToolInfo>) -> Self {
        Self {
            status: ApkVerificationStatus::NotVerified,
            unavailable_reason: Some(reason),
            tool,
            verified_schemes: Vec::new(),
            signer_count: 0,
            signers: Vec::new(),
            source_stamp_verified: false,
            source_stamp: None,
            proof_of_rotation: None,
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }
}

impl Default for ApkSignatureVerification {
    fn default() -> Self {
        Self::not_verified(ApkVerifierUnavailableReason::NotFound, None)
    }
}

#[derive(Debug, Clone)]
struct ToolInvocation {
    program: PathBuf,
    prefix_args: Vec<OsString>,
}

#[derive(Debug, Clone)]
struct ApkSignerTool {
    invocation: ToolInvocation,
    info: ApkSignerToolInfo,
}

#[derive(Debug, Clone)]
struct ToolCandidate {
    invocation: ToolInvocation,
    source: ApkSignerSource,
    build_tools_version: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct ResolverEnv {
    android_home: Option<PathBuf>,
    android_sdk_root: Option<PathBuf>,
    home: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
    java_home: Option<PathBuf>,
    program_files: Option<PathBuf>,
}

impl ResolverEnv {
    fn from_os() -> Self {
        Self {
            android_home: read_env_path("ANDROID_HOME"),
            android_sdk_root: read_env_path("ANDROID_SDK_ROOT"),
            home: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            local_app_data: read_env_path("LOCALAPPDATA"),
            java_home: read_env_path("JAVA_HOME"),
            program_files: read_env_path("ProgramFiles"),
        }
    }
}

fn read_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Verify `path` only when a compatible official tool can be proven. Any tool
/// discovery/runtime/parser problem is returned as an explicit Not verified
/// state so the Java-free static analysis result remains usable.
pub fn verify(path: &Path) -> ApkSignatureVerification {
    let tool = match locate_tool(&ResolverEnv::from_os()) {
        Ok(tool) => tool,
        Err((reason, info)) => return ApkSignatureVerification::not_verified(reason, info),
    };

    let verify_output = match run_tool(
        &tool.invocation,
        [
            OsStr::new("verify"),
            OsStr::new("--verbose"),
            OsStr::new("--print-certs"),
            OsStr::new("--print-certs-pem"),
            path.as_os_str(),
        ],
        VERIFY_TIMEOUT,
    ) {
        Ok(output) => output,
        Err(()) => {
            return ApkSignatureVerification::not_verified(
                ApkVerifierUnavailableReason::ExecutionFailed,
                Some(tool.info),
            )
        }
    };

    match verify_output.termination {
        CaptureTermination::Exited(status) if status.success() => {
            let stdout = String::from_utf8_lossy(&verify_output.stdout);
            let stderr = String::from_utf8_lossy(&verify_output.stderr);
            let mut verified =
                match parse_verified_output(&stdout, &stderr, path, tool.info.clone()) {
                    Ok(verified) => verified,
                    Err(()) => {
                        return ApkSignatureVerification::not_verified(
                            ApkVerifierUnavailableReason::OutputUnsupported,
                            Some(tool.info),
                        )
                    }
                };
            verified.proof_of_rotation = read_lineage(&tool, path);
            verified
        }
        CaptureTermination::Exited(_) => failed_verification(&verify_output, path, tool.info),
        CaptureTermination::TimedOut | CaptureTermination::OutputLimitExceeded { .. } => {
            ApkSignatureVerification::not_verified(
                ApkVerifierUnavailableReason::ExecutionFailed,
                Some(tool.info),
            )
        }
    }
}

fn failed_verification(
    output: &CapturedOutput,
    path: &Path,
    tool: ApkSignerToolInfo,
) -> ApkSignatureVerification {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut errors = collect_diagnostics(&stdout, path, &["ERROR:", "DOES NOT VERIFY"]);
    errors.extend(collect_diagnostics(
        &stderr,
        path,
        &["ERROR:", "DOES NOT VERIFY"],
    ));
    errors.truncate(MAX_DIAGNOSTICS);
    if errors.is_empty() {
        errors.push("apksigner rejected the selected APK without a diagnostic".to_string());
    }
    ApkSignatureVerification {
        status: ApkVerificationStatus::Failed,
        unavailable_reason: None,
        tool: Some(tool),
        verified_schemes: Vec::new(),
        signer_count: 0,
        signers: Vec::new(),
        source_stamp_verified: false,
        source_stamp: None,
        proof_of_rotation: None,
        warnings: collect_diagnostics(&stdout, path, &["WARNING:"]),
        errors,
    }
}

fn read_lineage(tool: &ApkSignerTool, path: &Path) -> Option<ApkSigningLineage> {
    let output = run_tool(
        &tool.invocation,
        [
            OsStr::new("lineage"),
            OsStr::new("--in"),
            path.as_os_str(),
            OsStr::new("--print-certs"),
            OsStr::new("--print-certs-pem"),
            OsStr::new("--verbose"),
        ],
        LINEAGE_TIMEOUT,
    )
    .ok()?;
    match output.termination {
        CaptureTermination::Exited(status) if status.success() => {
            parse_lineage_output(&String::from_utf8_lossy(&output.stdout)).ok()
        }
        _ => None,
    }
}

fn locate_tool(
    env: &ResolverEnv,
) -> Result<ApkSignerTool, (ApkVerifierUnavailableReason, Option<ApkSignerToolInfo>)> {
    let java = locate_java(env);
    let (candidates, saw_sdk_tool) = tool_candidates(env, java.as_deref());
    let mut incompatible = None;
    let mut probe_failed = false;

    for candidate in candidates {
        let Some(version) = probe_version(&candidate.invocation) else {
            probe_failed = true;
            continue;
        };
        let info = ApkSignerToolInfo {
            version: version.clone(),
            build_tools_version: candidate.build_tools_version,
            source: candidate.source,
        };
        if version_is_compatible(&version) {
            return Ok(ApkSignerTool {
                invocation: candidate.invocation,
                info,
            });
        }
        incompatible = Some(info);
    }

    if let Some(info) = incompatible {
        return Err((
            ApkVerifierUnavailableReason::IncompatibleVersion,
            Some(info),
        ));
    }
    if saw_sdk_tool && java.is_none() {
        return Err((ApkVerifierUnavailableReason::JavaUnavailable, None));
    }
    if probe_failed {
        return Err((ApkVerifierUnavailableReason::ProbeFailed, None));
    }
    Err((ApkVerifierUnavailableReason::NotFound, None))
}

fn tool_candidates(env: &ResolverEnv, java: Option<&Path>) -> (Vec<ToolCandidate>, bool) {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut saw_sdk_tool = false;

    for name in ["apksigner", "apksigner.bat"] {
        if let Ok(path) = which::which(name) {
            if path
                .extension()
                .and_then(OsStr::to_str)
                .is_some_and(|extension| {
                    matches!(extension.to_ascii_lowercase().as_str(), "bat" | "cmd")
                        && path
                            .parent()
                            .is_some_and(|parent| parent.join("lib/apksigner.jar").is_file())
                })
            {
                saw_sdk_tool = true;
            }
            if let Some(candidate) =
                candidate_from_apksigner_path(&path, ApkSignerSource::Path, None, java)
            {
                push_unique(&mut candidates, &mut seen, candidate);
            }
        }
    }

    for (root, source) in sdk_roots(env) {
        for (version, jar) in sdk_apksigner_jars(&root) {
            saw_sdk_tool = true;
            if let Some(java) = java {
                push_unique(
                    &mut candidates,
                    &mut seen,
                    ToolCandidate {
                        invocation: jar_invocation(java, &jar),
                        source,
                        build_tools_version: Some(version),
                    },
                );
            }
        }
    }

    (candidates, saw_sdk_tool)
}

fn push_unique(
    candidates: &mut Vec<ToolCandidate>,
    seen: &mut HashSet<(PathBuf, Vec<OsString>)>,
    candidate: ToolCandidate,
) {
    let key = (
        candidate.invocation.program.clone(),
        candidate.invocation.prefix_args.clone(),
    );
    if seen.insert(key) {
        candidates.push(candidate);
    }
}

fn candidate_from_apksigner_path(
    path: &Path,
    source: ApkSignerSource,
    build_tools_version: Option<String>,
    java: Option<&Path>,
) -> Option<ToolCandidate> {
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "bat" | "cmd") {
        let jar = path.parent()?.join("lib").join("apksigner.jar");
        let java = java?;
        if !jar.is_file() {
            return None;
        }
        return Some(ToolCandidate {
            invocation: jar_invocation(java, &jar),
            source,
            build_tools_version,
        });
    }
    Some(ToolCandidate {
        invocation: ToolInvocation {
            program: path.to_path_buf(),
            prefix_args: Vec::new(),
        },
        source,
        build_tools_version,
    })
}

fn jar_invocation(java: &Path, jar: &Path) -> ToolInvocation {
    ToolInvocation {
        program: java.to_path_buf(),
        prefix_args: vec![OsString::from("-jar"), jar.as_os_str().to_os_string()],
    }
}

fn locate_java(env: &ResolverEnv) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    let mut candidates = Vec::new();
    if let Some(root) = env.java_home.as_ref() {
        candidates.push(root.join("bin").join(exe));
    }
    if let Ok(path) = which::which("java") {
        candidates.push(path);
    }
    if cfg!(windows) {
        if let Some(program_files) = env.program_files.as_ref() {
            candidates.push(
                program_files
                    .join("Android")
                    .join("Android Studio")
                    .join("jbr")
                    .join("bin")
                    .join(exe),
            );
        }
    } else if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from(
            "/Applications/Android Studio.app/Contents/jbr/Contents/Home/bin/java",
        ));
    } else {
        candidates.push(PathBuf::from("/opt/android-studio/jbr/bin/java"));
        if let Some(home) = env.home.as_ref() {
            candidates.push(home.join("android-studio/jbr/bin/java"));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn sdk_roots(env: &ResolverEnv) -> Vec<(PathBuf, ApkSignerSource)> {
    let mut roots = Vec::new();
    for root in [env.android_home.as_ref(), env.android_sdk_root.as_ref()]
        .into_iter()
        .flatten()
    {
        roots.push((root.clone(), ApkSignerSource::AndroidHome));
    }
    if cfg!(windows) {
        if let Some(local_app_data) = env.local_app_data.as_ref() {
            roots.push((
                local_app_data.join("Android").join("Sdk"),
                ApkSignerSource::AndroidStudio,
            ));
        } else if let Some(home) = env.home.as_ref() {
            roots.push((
                home.join("AppData/Local/Android/Sdk"),
                ApkSignerSource::AndroidStudio,
            ));
        }
    } else if let Some(home) = env.home.as_ref() {
        let relative = if cfg!(target_os = "macos") {
            "Library/Android/sdk"
        } else {
            "Android/Sdk"
        };
        roots.push((home.join(relative), ApkSignerSource::AndroidStudio));
    }
    let mut seen = HashSet::new();
    roots.retain(|(root, _)| seen.insert(root.clone()));
    roots
}

fn sdk_apksigner_jars(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root.join("build-tools")) else {
        return Vec::new();
    };
    let mut jars = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let version = entry.file_name().to_string_lossy().into_owned();
            let parsed = numeric_version(&version)?;
            let jar = entry.path().join("lib").join("apksigner.jar");
            jar.is_file().then_some((parsed, version, jar))
        })
        .collect::<Vec<_>>();
    jars.sort_by(|left, right| right.0.cmp(&left.0));
    jars.into_iter()
        .map(|(_, version, jar)| (version, jar))
        .collect()
}

fn numeric_version(value: &str) -> Option<Vec<u32>> {
    let parts = value
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (!parts.is_empty()).then_some(parts)
}

fn version_is_compatible(value: &str) -> bool {
    let mut parts = value.trim().split('.');
    let Some(major) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    let Some(minor) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    (major, minor) >= MIN_APKSIGNER_VERSION
}

fn probe_version(invocation: &ToolInvocation) -> Option<String> {
    let output = run_tool(invocation, [OsStr::new("version")], PROBE_TIMEOUT).ok()?;
    match output.termination {
        CaptureTermination::Exited(status) if status.success() => {}
        _ => return None,
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn run_tool<'a>(
    invocation: &ToolInvocation,
    args: impl IntoIterator<Item = &'a OsStr>,
    timeout: Duration,
) -> Result<CapturedOutput, ()> {
    let mut command = Command::new(&invocation.program);
    command.args(&invocation.prefix_args).args(args);
    crate::process_capture::run(
        &mut command,
        timeout,
        CaptureLimits::symmetric(TOOL_OUTPUT_LIMIT),
    )
    .map_err(|_| ())
}

fn parse_verified_output(
    stdout: &str,
    stderr: &str,
    path: &Path,
    tool: ApkSignerToolInfo,
) -> Result<ApkSignatureVerification, ()> {
    if !stdout.lines().any(|line| line.trim() == "Verifies") {
        return Err(());
    }
    let signer_count = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("Number of signers: "))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|count| *count > 0)
        .ok_or(())?;
    let mut certificates = parse_certificates(stdout)?;
    let source_stamp = certificates
        .iter()
        .position(|certificate| certificate.label.starts_with("Source Stamp Signer"))
        .map(|index| certificates.remove(index));
    if certificates.is_empty() || certificates.len() > MAX_CERTIFICATES {
        return Err(());
    }

    let verified_schemes = stdout
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let scheme = line
                .strip_prefix("Verified using ")?
                .split(" scheme")
                .next()?;
            line.ends_with(": true")
                .then(|| scheme.to_ascii_lowercase())
        })
        .collect::<Vec<_>>();
    if verified_schemes.is_empty() {
        return Err(());
    }
    let source_stamp_verified = stdout
        .lines()
        .any(|line| line.trim() == "Verified for SourceStamp: true");
    let mut warnings = collect_diagnostics(stdout, path, &["WARNING:"]);
    warnings.extend(collect_diagnostics(stderr, path, &["WARNING:"]));
    warnings.truncate(MAX_DIAGNOSTICS);

    Ok(ApkSignatureVerification {
        status: ApkVerificationStatus::Verified,
        unavailable_reason: None,
        tool: Some(tool),
        verified_schemes,
        signer_count,
        signers: certificates,
        source_stamp_verified,
        source_stamp,
        proof_of_rotation: None,
        warnings,
        errors: Vec::new(),
    })
}

fn parse_certificates(output: &str) -> Result<Vec<ApkSignerCertificate>, ()> {
    let descriptors = output
        .lines()
        .filter_map(|line| {
            let (label, digest) = line.trim().split_once(" certificate SHA-256 digest: ")?;
            Some((
                label.trim().trim_end_matches(':').to_string(),
                normalize_digest(digest)?,
            ))
        })
        .collect::<Vec<_>>();
    let pems = Pem::iter_from_buffer(output.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ())?
        .into_iter()
        .filter(|pem| pem.label == "CERTIFICATE")
        .collect::<Vec<_>>();
    if descriptors.is_empty()
        || descriptors.len() != pems.len()
        || descriptors.len() > MAX_CERTIFICATES
    {
        return Err(());
    }

    descriptors
        .into_iter()
        .zip(pems)
        .map(|((label, printed_digest), pem)| {
            let certificate = pem.parse_x509().map_err(|_| ())?;
            let calculated_digest = hex_lower(&Sha256::digest(&pem.contents));
            if calculated_digest != printed_digest {
                return Err(());
            }
            Ok(ApkSignerCertificate {
                label: bounded_clean(&label, 256),
                sha256: calculated_digest,
                subject: bounded_clean(&certificate.subject().to_string(), 4096),
                issuer: bounded_clean(&certificate.issuer().to_string(), 4096),
                valid_from_unix: certificate.validity().not_before.timestamp(),
                valid_until_unix: certificate.validity().not_after.timestamp(),
            })
        })
        .collect()
}

fn parse_lineage_output(output: &str) -> Result<ApkSigningLineage, ()> {
    let certificates = parse_certificates(output)?
        .into_iter()
        .filter(|certificate| certificate.label.contains("in lineage"))
        .collect::<Vec<_>>();
    let capabilities = parse_capabilities(output)?;
    if certificates.len() < 2 || certificates.len() != capabilities.len() {
        return Err(());
    }
    Ok(ApkSigningLineage {
        entries: certificates
            .into_iter()
            .zip(capabilities)
            .enumerate()
            .map(
                |(index, (certificate, capabilities))| ApkSigningLineageEntry {
                    position: u32::try_from(index + 1).unwrap_or(u32::MAX),
                    certificate,
                    capabilities,
                },
            )
            .collect(),
    })
}

fn parse_capabilities(output: &str) -> Result<Vec<ApkSignerCapabilities>, ()> {
    let mut entries = Vec::new();
    let mut current = None;
    for line in output.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("Has installed data capability: ") {
            if current.is_some() {
                return Err(());
            }
            current = Some(ApkSignerCapabilities {
                installed_data: parse_bool(value)?,
                ..ApkSignerCapabilities::default()
            });
        } else if let Some(value) = line.strip_prefix("Has shared UID capability : ") {
            current.as_mut().ok_or(())?.shared_uid = parse_bool(value)?;
        } else if let Some(value) = line.strip_prefix("Has permission capability : ") {
            current.as_mut().ok_or(())?.permission = parse_bool(value)?;
        } else if let Some(value) = line.strip_prefix("Has rollback capability : ") {
            current.as_mut().ok_or(())?.rollback = parse_bool(value)?;
        } else if let Some(value) = line.strip_prefix("Has auth capability : ") {
            let mut completed = current.take().ok_or(())?;
            completed.auth = parse_bool(value)?;
            entries.push(completed);
        }
    }
    if current.is_some() || entries.is_empty() {
        return Err(());
    }
    Ok(entries)
}

fn parse_bool(value: &str) -> Result<bool, ()> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(()),
    }
}

fn normalize_digest(value: &str) -> Option<String> {
    let normalized = value
        .chars()
        .filter(|character| *character != ':')
        .collect::<String>()
        .to_ascii_lowercase();
    (normalized.len() == 64
        && normalized
            .chars()
            .all(|character| character.is_ascii_hexdigit()))
    .then_some(normalized)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn collect_diagnostics(text: &str, path: &Path, prefixes: &[&str]) -> Vec<String> {
    let path_text = path.to_string_lossy();
    text.lines()
        .map(str::trim)
        .filter(|line| prefixes.iter().any(|prefix| line.starts_with(prefix)))
        .take(MAX_DIAGNOSTICS)
        .map(|line| {
            bounded_clean(
                &line.replace(path_text.as_ref(), "<selected APK>"),
                MAX_DIAGNOSTIC_CHARS,
            )
        })
        .collect()
}

fn bounded_clean(value: &str, max_chars: usize) -> String {
    bounded(&crate::captured_tail::sanitize_log(value), max_chars)
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    const CERTIFICATE: &str = include_str!("../fixtures/apksigner/certificate.pem");

    #[derive(Debug, Deserialize)]
    struct VerificationCase {
        name: String,
        status: String,
        schemes: Vec<String>,
        signer_labels: Vec<String>,
        signer_count: u32,
        lineage_labels: Vec<String>,
    }

    fn certificate_output(label: &str) -> String {
        let pem = Pem::iter_from_buffer(CERTIFICATE.as_bytes())
            .next()
            .unwrap()
            .unwrap();
        let digest = hex_lower(&Sha256::digest(&pem.contents));
        format!(
            "{label} certificate DN: fixture\n{label} certificate SHA-256 digest: {digest}\n{CERTIFICATE}"
        )
    }

    fn verified_output(case: &VerificationCase) -> String {
        let mut output = String::from("Verifies\n");
        for scheme in ["v1", "v2", "v3", "v3.1", "v3.2", "v4"] {
            output.push_str(&format!(
                "Verified using {scheme} scheme (fixture): {}\n",
                case.schemes.iter().any(|value| value == scheme)
            ));
        }
        output.push_str("Verified for SourceStamp: false\n");
        output.push_str(&format!("Number of signers: {}\n", case.signer_count));
        for label in &case.signer_labels {
            output.push_str(&certificate_output(label));
        }
        output
    }

    fn lineage_output(labels: &[String]) -> String {
        let mut output = String::new();
        for label in labels {
            output.push_str(&certificate_output(label));
            output.push_str(
                "Has installed data capability: true\n\
                 Has shared UID capability : false\n\
                 Has permission capability : true\n\
                 Has rollback capability : false\n\
                 Has auth capability : true\n",
            );
        }
        output
    }

    fn tool_info() -> ApkSignerToolInfo {
        ApkSignerToolInfo {
            version: "0.9".to_string(),
            build_tools_version: Some("37.0.0".to_string()),
            source: ApkSignerSource::AndroidStudio,
        }
    }

    #[test]
    fn verification_fixtures_cover_valid_tampered_multi_signer_and_rotation() {
        let cases: Vec<VerificationCase> = serde_json::from_str(include_str!(
            "../fixtures/apksigner/verification-cases.json"
        ))
        .unwrap();
        assert_eq!(cases.len(), 4);
        for case in cases {
            if case.status == "failed" {
                let output = CapturedOutput {
                    stdout: Vec::new(),
                    stderr: b"DOES NOT VERIFY\nERROR: APK integrity check failed\n".to_vec(),
                    termination: CaptureTermination::Exited(failed_status()),
                };
                let result = failed_verification(&output, Path::new("fixture.apk"), tool_info());
                assert_eq!(
                    result.status,
                    ApkVerificationStatus::Failed,
                    "{}",
                    case.name
                );
                assert!(!result.errors.is_empty(), "{}", case.name);
                continue;
            }

            let mut result = parse_verified_output(
                &verified_output(&case),
                "",
                Path::new("fixture.apk"),
                tool_info(),
            )
            .unwrap();
            assert_eq!(
                result.status,
                ApkVerificationStatus::Verified,
                "{}",
                case.name
            );
            assert_eq!(result.signer_count, case.signer_count, "{}", case.name);
            assert_eq!(
                result.signers.len(),
                case.signer_labels.len(),
                "{}",
                case.name
            );
            assert_eq!(result.verified_schemes, case.schemes, "{}", case.name);
            if !case.lineage_labels.is_empty() {
                result.proof_of_rotation =
                    Some(parse_lineage_output(&lineage_output(&case.lineage_labels)).unwrap());
                assert_eq!(
                    result.proof_of_rotation.unwrap().entries.len(),
                    case.lineage_labels.len(),
                    "{}",
                    case.name
                );
            }
        }
    }

    #[test]
    fn certificate_digest_mismatch_never_returns_identity() {
        let output = format!(
            "Signer #1 certificate SHA-256 digest: {}\n{CERTIFICATE}",
            "0".repeat(64)
        );
        assert!(parse_certificates(&output).is_err());
    }

    #[test]
    fn build_tools_candidates_are_newest_first_and_ignore_previews() {
        let root = test_dir("resolver");
        for version in ["35.0.0", "37.0.0", "38.0.0-rc1"] {
            let lib = root.join("build-tools").join(version).join("lib");
            fs::create_dir_all(&lib).unwrap();
            fs::write(lib.join("apksigner.jar"), []).unwrap();
        }
        let jars = sdk_apksigner_jars(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(
            jars.into_iter()
                .map(|(version, _)| version)
                .collect::<Vec<_>>(),
            vec!["37.0.0", "35.0.0"]
        );
    }

    #[test]
    fn compatibility_requires_current_structured_output_generation() {
        assert!(version_is_compatible("0.9"));
        assert!(version_is_compatible("1.0"));
        assert!(!version_is_compatible("0.8"));
        assert!(!version_is_compatible("unknown"));
    }

    fn test_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "droidsmith-apksigner-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(unix)]
    fn failed_status() -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(1 << 8)
    }

    #[cfg(windows)]
    fn failed_status() -> std::process::ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(1)
    }
}
