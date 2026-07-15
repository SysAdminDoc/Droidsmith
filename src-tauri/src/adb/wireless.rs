//! Android 11+ wireless debugging helpers.
//!
//! The actual pairing protocol is handled by the platform-tools `adb`
//! binary. This module owns the validated request/response types, mDNS
//! output parsing, and the narrow raw `adb` invocations needed by the
//! renderer wizard.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::adb::device::{remember_wireless_transport, DeviceTransportKind};
use crate::adb::transport::{ShellTransport, TransportError};

const PAIRING_SERVICE: &str = "_adb-tls-pairing._tcp";
const CONNECT_SERVICE: &str = "_adb-tls-connect._tcp";

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WirelessAdbService {
    pub name: String,
    pub service_type: String,
    pub kind: WirelessServiceKind,
    pub host: String,
    pub port: u16,
    pub endpoint: String,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WirelessServiceKind {
    Pairing,
    Connect,
    Other,
}

#[derive(specta::Type, Debug, Clone, Deserialize)]
pub struct WirelessPairRequest {
    pub host: String,
    pub port: u16,
    pub pairing_code: String,
}

#[derive(specta::Type, Debug, Clone, Deserialize)]
pub struct WirelessConnectRequest {
    pub host: String,
    pub port: u16,
    /// Explicitly labels a manual endpoint as legacy `adb tcpip`. This can
    /// never upgrade an endpoint to TLS; exact current mDNS discovery is the
    /// only source of `tls_wifi` provenance.
    #[serde(default)]
    pub legacy_tcp: bool,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct WirelessCommandResult {
    pub endpoint: String,
    pub stdout: String,
    pub transport_kind: Option<DeviceTransportKind>,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WirelessFailureHintCode {
    VpnInterferenceLikely,
    MdnsInterferenceLikely,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WirelessEndpointKind {
    IpAddress,
    LocalName,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WirelessFailureDiagnostics {
    pub platform_tools_version: Option<String>,
    pub mdns_enabled: Option<bool>,
    pub mdns_backend: Option<String>,
    pub mdns_check_succeeded: bool,
    pub active_vpn_interfaces: u32,
    pub endpoint_kind: WirelessEndpointKind,
    pub adb_error_kind: &'static str,
}

#[derive(specta::Type, Debug, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct WirelessCommandError {
    pub code: &'static str,
    pub message: String,
    pub hint_code: Option<WirelessFailureHintCode>,
    pub diagnostics: WirelessFailureDiagnostics,
}

impl WirelessCommandError {
    pub fn unavailable(
        error: TransportError,
        host: &str,
        platform_tools_version: Option<String>,
    ) -> Self {
        let diagnostics = WirelessFailureDiagnostics {
            platform_tools_version,
            mdns_enabled: None,
            mdns_backend: None,
            mdns_check_succeeded: false,
            active_vpn_interfaces: 0,
            endpoint_kind: endpoint_kind(host),
            adb_error_kind: transport_error_kind(&error),
        };
        Self {
            code: "wireless_adb_failed",
            message: bounded_error_message(&error),
            hint_code: None,
            diagnostics,
        }
    }

    fn validation(
        error: TransportError,
        host: &str,
        platform_tools_version: Option<String>,
    ) -> Self {
        Self::unavailable(error, host, platform_tools_version)
    }
}

fn tls_connect_endpoints() -> &'static Mutex<HashSet<String>> {
    static ENDPOINTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ENDPOINTS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn list_mdns_services(
    transport: &ShellTransport,
) -> Result<Vec<WirelessAdbService>, TransportError> {
    let stdout = transport.adb(&["mdns", "services"])?;
    let services = parse_mdns_services(&stdout)?;
    remember_tls_connect_services(&services);
    Ok(services)
}

fn remember_tls_connect_services(services: &[WirelessAdbService]) {
    if let Ok(mut known) = tls_connect_endpoints().lock() {
        known.clear();
        known.extend(
            services
                .iter()
                .filter(|service| service.kind == WirelessServiceKind::Connect)
                .map(|service| service.endpoint.clone()),
        );
    }
}

pub fn pair(
    transport: &ShellTransport,
    req: &WirelessPairRequest,
    platform_tools_version: Option<String>,
) -> Result<WirelessCommandResult, WirelessCommandError> {
    validate_pairing_code(&req.pairing_code).map_err(|error| {
        WirelessCommandError::validation(error, &req.host, platform_tools_version.clone())
    })?;
    let endpoint = validated_endpoint(&req.host, req.port).map_err(|error| {
        WirelessCommandError::validation(error, &req.host, platform_tools_version.clone())
    })?;
    let stdout = transport
        .adb(&["pair", &endpoint, &req.pairing_code])
        .map_err(|error| {
            diagnose_wireless_failure(transport, &req.host, platform_tools_version, error)
        })?;
    Ok(WirelessCommandResult {
        endpoint,
        stdout,
        transport_kind: None,
    })
}

pub fn connect(
    transport: &ShellTransport,
    req: &WirelessConnectRequest,
    platform_tools_version: Option<String>,
) -> Result<WirelessCommandResult, WirelessCommandError> {
    let endpoint = validated_endpoint(&req.host, req.port).map_err(|error| {
        WirelessCommandError::validation(error, &req.host, platform_tools_version.clone())
    })?;
    let stdout = transport.adb(&["connect", &endpoint]).map_err(|error| {
        diagnose_wireless_failure(transport, &req.host, platform_tools_version, error)
    })?;
    let transport_kind = classify_connect_transport(&endpoint, req.legacy_tcp);
    if connect_succeeded(&stdout) {
        remember_wireless_transport(&endpoint, transport_kind);
    }
    Ok(WirelessCommandResult {
        endpoint,
        stdout,
        transport_kind: Some(transport_kind),
    })
}

fn diagnose_wireless_failure(
    transport: &ShellTransport,
    host: &str,
    platform_tools_version: Option<String>,
    error: TransportError,
) -> WirelessCommandError {
    let message = bounded_error_message(&error);
    let health = crate::adb::health::probe(transport, platform_tools_version.clone());
    let active_vpn_interfaces = crate::host_diagnostics::active_vpn_interface_count();
    let endpoint_kind = endpoint_kind(host);
    let mdns_unhealthy = health.mdns_enabled == Some(false) || health.mdns_check.is_none();
    let hint_code = select_failure_hint(
        active_vpn_interfaces,
        endpoint_kind,
        mdns_unhealthy,
        &message,
    );
    WirelessCommandError {
        code: "wireless_adb_failed",
        message,
        hint_code,
        diagnostics: WirelessFailureDiagnostics {
            platform_tools_version,
            mdns_enabled: health.mdns_enabled,
            mdns_backend: health.mdns_backend,
            mdns_check_succeeded: health.mdns_check.is_some(),
            active_vpn_interfaces,
            endpoint_kind,
            adb_error_kind: transport_error_kind(&error),
        },
    }
}

fn select_failure_hint(
    active_vpn_interfaces: u32,
    endpoint_kind: WirelessEndpointKind,
    mdns_unhealthy: bool,
    error_message: &str,
) -> Option<WirelessFailureHintCode> {
    if active_vpn_interfaces > 0 {
        return Some(WirelessFailureHintCode::VpnInterferenceLikely);
    }
    let lower = error_message.to_ascii_lowercase();
    let name_resolution_failed = [
        "cannot resolve",
        "failed to resolve",
        "unknown host",
        "no such host",
        "name or service not known",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    if endpoint_kind == WirelessEndpointKind::LocalName
        && (mdns_unhealthy || name_resolution_failed)
    {
        return Some(WirelessFailureHintCode::MdnsInterferenceLikely);
    }
    None
}

fn endpoint_kind(host: &str) -> WirelessEndpointKind {
    let bare = host
        .trim()
        .trim_matches(|character| character == '[' || character == ']');
    if bare.parse::<IpAddr>().is_ok() {
        WirelessEndpointKind::IpAddress
    } else {
        WirelessEndpointKind::LocalName
    }
}

fn transport_error_kind(error: &TransportError) -> &'static str {
    match error {
        TransportError::AdbNotFound => "adb_not_found",
        TransportError::Spawn(_) => "spawn_failed",
        TransportError::Exit { .. } => "adb_exit",
        TransportError::Signaled { .. } => "adb_signaled",
        TransportError::Timeout(_) => "adb_timeout",
        TransportError::Parse(_) => "parse_error",
    }
}

fn bounded_error_message(error: &TransportError) -> String {
    error.to_string().chars().take(4_096).collect()
}

fn classify_connect_transport(endpoint: &str, legacy_tcp: bool) -> DeviceTransportKind {
    if tls_connect_endpoints()
        .lock()
        .is_ok_and(|known| known.contains(endpoint))
    {
        DeviceTransportKind::TlsWifi
    } else if legacy_tcp {
        DeviceTransportKind::LegacyTcp
    } else {
        DeviceTransportKind::UnknownTcp
    }
}

fn connect_succeeded(stdout: &str) -> bool {
    let normalized = stdout.trim().to_ascii_lowercase();
    normalized.starts_with("connected to ") || normalized.starts_with("already connected to ")
}

pub fn parse_mdns_services(stdout: &str) -> Result<Vec<WirelessAdbService>, TransportError> {
    let mut services = Vec::new();

    for raw in stdout.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("List of discovered") {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        let Some(service_type) = parts.next() else {
            continue;
        };
        let Some(endpoint) = parts.next() else {
            continue;
        };
        let Some((host, port)) = parse_endpoint(endpoint) else {
            continue;
        };

        services.push(WirelessAdbService {
            name: name.to_string(),
            service_type: service_type.to_string(),
            kind: classify_service(service_type),
            host,
            port,
            endpoint: endpoint.to_string(),
        });
    }

    Ok(services)
}

fn validated_endpoint(host: &str, port: u16) -> Result<String, TransportError> {
    let host = host.trim();
    if !valid_host(host) {
        return Err(TransportError::Parse(format!(
            "invalid wireless adb host {host:?}"
        )));
    }
    if port == 0 {
        return Err(TransportError::Parse(
            "wireless adb port must be between 1 and 65535".to_string(),
        ));
    }
    if host.contains(':') {
        Ok(format!(
            "[{}]:{port}",
            host.trim_matches(|c| c == '[' || c == ']')
        ))
    } else {
        Ok(format!("{host}:{port}"))
    }
}

fn validate_pairing_code(code: &str) -> Result<(), TransportError> {
    if code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err(TransportError::Parse(
            "wireless adb pairing code must be exactly six digits".to_string(),
        ))
    }
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        // A leading '-' would let the host reach `adb connect`/`adb pair`
        // as an option flag rather than a positional endpoint (the serial
        // path already blocks this via `valid_serial`).
        && !host.starts_with('-')
        && host.len() <= 255
        && host
            .chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && !matches!(c, '/' | '\\'))
}

fn parse_endpoint(endpoint: &str) -> Option<(String, u16)> {
    // A bracketed IPv6 endpoint (`[fe80::1]:5555`) is the only unambiguous way
    // to carry an IPv6 literal with a port, so parse the bracket form directly.
    let (host, raw_port) = if let Some(rest) = endpoint.strip_prefix('[') {
        let (host, raw_port) = rest.split_once("]:")?;
        if host.contains('[') || host.contains(']') {
            return None;
        }
        (host, raw_port)
    } else if endpoint.matches(':').count() > 1 {
        // A bare (unbracketed) IPv6 literal has no unambiguous host/port split;
        // `rsplit_once(':')` would treat a trailing address group as the port.
        // Reject it rather than surface a corrupted host and port for display.
        return None;
    } else {
        endpoint.rsplit_once(':')?
    };
    if !valid_host(host) {
        return None;
    }
    let port = raw_port.parse::<u16>().ok()?;
    if port == 0 {
        return None;
    }
    Some((host.to_string(), port))
}

fn classify_service(service_type: &str) -> WirelessServiceKind {
    let bare = service_type.trim_end_matches(".local.");
    if bare == PAIRING_SERVICE {
        WirelessServiceKind::Pairing
    } else if bare == CONNECT_SERVICE {
        WirelessServiceKind::Connect
    } else {
        WirelessServiceKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mdns_services_finds_pairing_and_connect_targets() {
        let stdout = "\
List of discovered mdns services
adb-123456abcd     _adb-tls-pairing._tcp   192.168.1.42:41789
adb-987654fedc     _adb-tls-connect._tcp   pixel.local:38899
";

        let services = parse_mdns_services(stdout).unwrap();

        assert_eq!(services.len(), 2);
        assert_eq!(services[0].kind, WirelessServiceKind::Pairing);
        assert_eq!(services[0].host, "192.168.1.42");
        assert_eq!(services[0].port, 41789);
        assert_eq!(services[1].kind, WirelessServiceKind::Connect);
        assert_eq!(services[1].endpoint, "pixel.local:38899");
    }

    #[test]
    fn parse_mdns_services_tolerates_noise_and_unknown_services() {
        let stdout = "\
List of discovered mdns services
garbage
adb-other _printer._tcp 192.168.1.2:9100
adb-bad _adb-tls-connect._tcp no-port
";

        let services = parse_mdns_services(stdout).unwrap();

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].kind, WirelessServiceKind::Other);
    }

    #[test]
    fn endpoint_validation_rejects_bad_hosts_and_ports() {
        assert!(validated_endpoint("192.168.1.42", 37099).is_ok());
        assert!(validated_endpoint("device.local", 37099).is_ok());
        assert_eq!(
            validated_endpoint("fe80::1", 37099).unwrap(),
            "[fe80::1]:37099"
        );
        assert!(validated_endpoint("", 37099).is_err());
        assert!(validated_endpoint("bad host", 37099).is_err());
        assert!(validated_endpoint("bad/host", 37099).is_err());
        assert!(validated_endpoint("192.168.1.42", 0).is_err());
        // A leading '-' host would reach `adb connect`/`adb pair` as a flag.
        assert!(validated_endpoint("-oProxyCommand", 37099).is_err());
    }

    #[test]
    fn parse_endpoint_handles_ipv4_hostname_and_bracketed_ipv6() {
        assert_eq!(
            parse_endpoint("192.168.1.42:5555"),
            Some(("192.168.1.42".to_string(), 5555))
        );
        assert_eq!(
            parse_endpoint("device.local:38899"),
            Some(("device.local".to_string(), 38899))
        );
        assert_eq!(
            parse_endpoint("[fe80::1]:5555"),
            Some(("fe80::1".to_string(), 5555))
        );
        // A bare IPv6 literal is ambiguous and must not have an address group
        // mis-read as the port.
        assert_eq!(parse_endpoint("fe80::1:5555"), None);
        assert_eq!(parse_endpoint("fe80::1"), None);
        // Malformed bracket forms and zero ports are rejected.
        assert_eq!(parse_endpoint("[fe80::1]:0"), None);
        assert_eq!(parse_endpoint("[fe80::1]"), None);
        assert_eq!(parse_endpoint("[fe80::1:5555"), None);
    }

    #[test]
    fn pairing_code_validation_is_six_digits() {
        assert!(validate_pairing_code("123456").is_ok());
        assert!(validate_pairing_code("12345").is_err());
        assert!(validate_pairing_code("abcdef").is_err());
        assert!(validate_pairing_code("1234567").is_err());
    }

    #[test]
    fn connect_trust_requires_exact_mdns_provenance_or_explicit_legacy_mode() {
        let services =
            parse_mdns_services("adb-imp44 _adb-tls-connect._tcp imp44-mdns.local:38899\n")
                .unwrap();
        remember_tls_connect_services(&services);

        assert_eq!(
            classify_connect_transport("imp44-mdns.local:38899", false),
            DeviceTransportKind::TlsWifi
        );
        assert_eq!(
            classify_connect_transport("imp44-mdns.local:5555", false),
            DeviceTransportKind::UnknownTcp
        );
        assert_eq!(
            classify_connect_transport("imp44-mdns.local:5555", true),
            DeviceTransportKind::LegacyTcp
        );
        assert!(connect_succeeded("connected to imp44-mdns.local:38899"));
        assert!(connect_succeeded(
            "already connected to imp44-mdns.local:38899"
        ));
        assert!(!connect_succeeded("failed to connect"));

        remember_tls_connect_services(&[]);
    }

    #[test]
    fn failure_hints_require_observed_vpn_or_mdns_evidence() {
        assert_eq!(
            select_failure_hint(
                1,
                WirelessEndpointKind::IpAddress,
                false,
                "failed to connect"
            ),
            Some(WirelessFailureHintCode::VpnInterferenceLikely)
        );
        assert_eq!(
            select_failure_hint(
                0,
                WirelessEndpointKind::LocalName,
                true,
                "failed to connect"
            ),
            Some(WirelessFailureHintCode::MdnsInterferenceLikely)
        );
        assert_eq!(
            select_failure_hint(
                0,
                WirelessEndpointKind::IpAddress,
                true,
                "failed to connect"
            ),
            None
        );
    }
}
