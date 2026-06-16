//! Android 11+ wireless debugging helpers.
//!
//! The actual pairing protocol is handled by the platform-tools `adb`
//! binary. This module owns the validated request/response types, mDNS
//! output parsing, and the narrow raw `adb` invocations needed by the
//! renderer wizard.

use serde::{Deserialize, Serialize};

use crate::adb::transport::{ShellTransport, TransportError};

const PAIRING_SERVICE: &str = "_adb-tls-pairing._tcp";
const CONNECT_SERVICE: &str = "_adb-tls-connect._tcp";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WirelessAdbService {
    pub name: String,
    pub service_type: String,
    pub kind: WirelessServiceKind,
    pub host: String,
    pub port: u16,
    pub endpoint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WirelessServiceKind {
    Pairing,
    Connect,
    Other,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WirelessPairRequest {
    pub host: String,
    pub port: u16,
    pub pairing_code: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WirelessConnectRequest {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct WirelessCommandResult {
    pub endpoint: String,
    pub stdout: String,
}

pub fn list_mdns_services(
    transport: &ShellTransport,
) -> Result<Vec<WirelessAdbService>, TransportError> {
    let stdout = transport.adb(&["mdns", "services"])?;
    parse_mdns_services(&stdout)
}

pub fn pair(
    transport: &ShellTransport,
    req: &WirelessPairRequest,
) -> Result<WirelessCommandResult, TransportError> {
    validate_pairing_code(&req.pairing_code)?;
    let endpoint = validated_endpoint(&req.host, req.port)?;
    let stdout = transport.adb(&["pair", &endpoint, &req.pairing_code])?;
    Ok(WirelessCommandResult { endpoint, stdout })
}

pub fn connect(
    transport: &ShellTransport,
    req: &WirelessConnectRequest,
) -> Result<WirelessCommandResult, TransportError> {
    let endpoint = validated_endpoint(&req.host, req.port)?;
    let stdout = transport.adb(&["connect", &endpoint])?;
    Ok(WirelessCommandResult { endpoint, stdout })
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
    Ok(format!("{host}:{port}"))
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
        && host.len() <= 255
        && host
            .chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && !matches!(c, '/' | '\\'))
}

fn parse_endpoint(endpoint: &str) -> Option<(String, u16)> {
    let (host, raw_port) = endpoint.rsplit_once(':')?;
    let host = host.trim_matches(|c| c == '[' || c == ']');
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
        assert!(validated_endpoint("", 37099).is_err());
        assert!(validated_endpoint("bad host", 37099).is_err());
        assert!(validated_endpoint("bad/host", 37099).is_err());
        assert!(validated_endpoint("192.168.1.42", 0).is_err());
    }

    #[test]
    fn pairing_code_validation_is_six_digits() {
        assert!(validate_pairing_code("123456").is_ok());
        assert!(validate_pairing_code("12345").is_err());
        assert!(validate_pairing_code("abcdef").is_err());
        assert!(validate_pairing_code("1234567").is_err());
    }
}
