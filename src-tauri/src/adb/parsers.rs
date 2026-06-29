use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RemoteFileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub permissions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FastbootDevice {
    pub serial: String,
    pub mode: String,
    pub product: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NetworkConnection {
    pub state: String,
    pub protocol: String,
    pub local_addr: String,
    pub remote_addr: String,
    pub process: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub user: String,
    pub vsz_kb: u64,
    pub rss_kb: u64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

pub fn parse_ls_output(stdout: &str) -> Vec<RemoteFileEntry> {
    let mut out = Vec::new();
    for raw in stdout.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("total") {
            continue;
        }
        out.push(parse_ls_line(line).unwrap_or_else(|| degraded_file(line)));
    }
    out.sort_by(|a, b| {
        a.parse_error
            .is_some()
            .cmp(&b.parse_error.is_some())
            .then(b.is_dir.cmp(&a.is_dir))
            .then(a.name.cmp(&b.name))
    });
    out
}

fn parse_ls_line(line: &str) -> Option<RemoteFileEntry> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let permissions = *tokens.first()?;
    if permissions.len() < 2
        || !matches!(
            permissions.as_bytes()[0] as char,
            '-' | 'd' | 'l' | 'c' | 'b' | 's' | 'p'
        )
    {
        return None;
    }

    let date_idx = tokens
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(idx, token)| (is_iso_date(token) || is_month_name(token)).then_some(idx))?;
    let size_idx = date_idx.checked_sub(1)?;
    let date_tokens = if tokens.get(date_idx).is_some_and(|token| is_iso_date(token)) {
        2
    } else {
        3
    };
    let name_idx = date_idx + date_tokens;
    if name_idx >= tokens.len() {
        return None;
    }

    let is_dir = permissions.starts_with('d');
    let size = if is_dir {
        None
    } else {
        tokens[size_idx].parse::<u64>().ok()
    };

    Some(RemoteFileEntry {
        name: tokens[name_idx..].join(" "),
        is_dir,
        size,
        permissions: permissions.to_string(),
        parse_error: None,
    })
}

fn is_iso_date(token: &str) -> bool {
    token.len() >= 8 && token.contains('-')
}

fn is_month_name(token: &str) -> bool {
    matches!(
        token,
        "Jan"
            | "Feb"
            | "Mar"
            | "Apr"
            | "May"
            | "Jun"
            | "Jul"
            | "Aug"
            | "Sep"
            | "Oct"
            | "Nov"
            | "Dec"
    )
}

fn degraded_file(line: &str) -> RemoteFileEntry {
    RemoteFileEntry {
        name: line.to_string(),
        is_dir: false,
        size: None,
        permissions: "?".to_string(),
        parse_error: Some("unrecognized ls row".to_string()),
    }
}

pub fn parse_fastboot_devices(stdout: &str) -> Vec<FastbootDevice> {
    let mut out = Vec::new();
    for raw in stdout.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if line.starts_with('<') || line.to_ascii_lowercase().contains("no permissions") {
            out.push(FastbootDevice {
                serial: tokens.first().copied().unwrap_or(line).to_string(),
                mode: "no_permissions".to_string(),
                product: None,
                parse_error: Some(line.to_string()),
            });
            continue;
        }
        let Some(serial) = tokens.first() else {
            continue;
        };
        if tokens.len() == 1 {
            out.push(FastbootDevice {
                serial: (*serial).to_string(),
                mode: "fastboot".to_string(),
                product: None,
                parse_error: None,
            });
            continue;
        }

        let mode = tokens.get(1).copied().unwrap_or("fastboot").to_string();
        let product = tokens
            .iter()
            .find_map(|token| token.strip_prefix("product:"))
            .map(str::to_string);
        out.push(FastbootDevice {
            serial: (*serial).to_string(),
            mode,
            product,
            parse_error: None,
        });
    }
    out
}

pub fn parse_ss_output(stdout: &str) -> Vec<NetworkConnection> {
    let mut out = Vec::new();
    for raw in stdout.lines() {
        let line = raw.trim();
        if line.is_empty() || is_network_header(line) {
            continue;
        }
        out.push(parse_network_line(line).unwrap_or_else(|| degraded_network(line)));
    }
    out
}

fn is_network_header(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with("netid")
        || lower.starts_with("proto")
        || lower.starts_with("active internet")
        || lower.starts_with("recv-q")
}

fn parse_network_line(line: &str) -> Option<NetworkConnection> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let first = *tokens.first()?;
    if is_network_protocol(first) {
        parse_protocol_first_network(&tokens)
    } else {
        parse_state_first_network(line, &tokens)
    }
}

fn parse_protocol_first_network(tokens: &[&str]) -> Option<NetworkConnection> {
    let protocol = tokens[0].trim_end_matches('6').to_string();
    if tokens.len() >= 6 && tokens[1].parse::<u64>().is_err() {
        return Some(NetworkConnection {
            protocol,
            state: tokens[1].to_string(),
            local_addr: tokens[4].to_string(),
            remote_addr: tokens[5].to_string(),
            process: process_tail(tokens, 6),
            parse_error: None,
        });
    }
    if tokens.len() >= 5 {
        let state_idx = if tokens.get(5).is_some_and(|token| is_tcp_state(token)) {
            5
        } else {
            1
        };
        let process_idx = if state_idx == 5 { 6 } else { 5 };
        return Some(NetworkConnection {
            protocol,
            state: tokens
                .get(state_idx)
                .copied()
                .unwrap_or("UNCONN")
                .to_string(),
            local_addr: tokens[3].to_string(),
            remote_addr: tokens[4].to_string(),
            process: process_tail(tokens, process_idx),
            parse_error: None,
        });
    }
    None
}

fn parse_state_first_network(line: &str, tokens: &[&str]) -> Option<NetworkConnection> {
    if tokens.len() < 5 {
        return None;
    }
    Some(NetworkConnection {
        state: tokens[0].to_string(),
        protocol: if line.contains("tcp") {
            "tcp".to_string()
        } else if line.contains("udp") {
            "udp".to_string()
        } else {
            "?".to_string()
        },
        local_addr: tokens[3].to_string(),
        remote_addr: tokens[4].to_string(),
        process: process_tail(tokens, 5),
        parse_error: None,
    })
}

fn process_tail(tokens: &[&str], start: usize) -> Option<String> {
    (tokens.len() > start).then(|| tokens[start..].join(" "))
}

fn is_network_protocol(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    lower.starts_with("tcp") || lower.starts_with("udp")
}

fn is_tcp_state(token: &str) -> bool {
    matches!(
        token.to_ascii_uppercase().as_str(),
        "LISTEN"
            | "ESTABLISHED"
            | "ESTAB"
            | "SYN-SENT"
            | "SYN-RECV"
            | "FIN-WAIT-1"
            | "FIN-WAIT-2"
            | "TIME-WAIT"
            | "CLOSE"
            | "CLOSE-WAIT"
            | "LAST-ACK"
            | "CLOSING"
            | "UNKNOWN"
            | "UNCONN"
    )
}

fn degraded_network(line: &str) -> NetworkConnection {
    NetworkConnection {
        state: "unparsed".to_string(),
        protocol: "?".to_string(),
        local_addr: line.to_string(),
        remote_addr: "?".to_string(),
        process: None,
        parse_error: Some("unrecognized network row".to_string()),
    }
}

pub fn parse_ps_output(stdout: &str) -> Vec<ProcessInfo> {
    let mut out = Vec::new();
    let mut header: Option<Vec<String>> = None;

    for raw in stdout.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if header.is_none() && is_process_header(line) {
            header = Some(
                line.split_whitespace()
                    .map(|token| token.to_ascii_uppercase())
                    .collect(),
            );
            continue;
        }

        let tokens: Vec<&str> = line.split_whitespace().collect();
        let parsed = header
            .as_deref()
            .and_then(|columns| parse_process_with_header(columns, &tokens))
            .or_else(|| parse_process_fallback(&tokens));
        out.push(parsed.unwrap_or_else(|| degraded_process(line)));
    }

    out
}

fn is_process_header(line: &str) -> bool {
    let columns: Vec<String> = line
        .split_whitespace()
        .map(|token| token.to_ascii_uppercase())
        .collect();
    columns.iter().any(|token| token == "PID")
        && columns
            .iter()
            .any(|token| matches!(token.as_str(), "NAME" | "CMD" | "COMMAND" | "ARGS"))
}

fn parse_process_with_header(columns: &[String], tokens: &[&str]) -> Option<ProcessInfo> {
    let pid_idx = column_index(columns, &["PID"])?;
    let name_idx = column_index(columns, &["NAME", "CMD", "COMMAND", "ARGS"])?;
    let pid = tokens.get(pid_idx)?.parse::<u32>().ok()?;
    Some(ProcessInfo {
        pid,
        user: column_index(columns, &["USER"])
            .and_then(|idx| tokens.get(idx).copied())
            .unwrap_or("?")
            .to_string(),
        vsz_kb: column_index(columns, &["VSZ", "VSIZE"])
            .and_then(|idx| tokens.get(idx))
            .and_then(|token| token.parse::<u64>().ok())
            .unwrap_or(0),
        rss_kb: column_index(columns, &["RSS", "RSS_KB"])
            .and_then(|idx| tokens.get(idx))
            .and_then(|token| token.parse::<u64>().ok())
            .unwrap_or(0),
        name: if name_idx < tokens.len() {
            tokens[name_idx..].join(" ")
        } else {
            "?".to_string()
        },
        parse_error: None,
    })
}

fn column_index(columns: &[String], names: &[&str]) -> Option<usize> {
    columns
        .iter()
        .position(|column| names.iter().any(|name| column == name))
}

fn parse_process_fallback(tokens: &[&str]) -> Option<ProcessInfo> {
    if tokens.len() < 5 {
        return None;
    }
    let pid = tokens[0].parse::<u32>().ok()?;
    Some(ProcessInfo {
        pid,
        user: tokens[1].to_string(),
        vsz_kb: tokens[2].parse::<u64>().unwrap_or(0),
        rss_kb: tokens[3].parse::<u64>().unwrap_or(0),
        name: tokens[4..].join(" "),
        parse_error: None,
    })
}

fn degraded_process(line: &str) -> ProcessInfo {
    ProcessInfo {
        pid: 0,
        user: "?".to_string(),
        vsz_kb: 0,
        rss_kb: 0,
        name: line.to_string(),
        parse_error: Some("unrecognized process row".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_rows_and_keeps_malformed_oem_rows_visible() {
        let transcript = r#"
total 8
drwxrwx--- 2 root sdcard_rw 4096 2026-06-28 12:30 Download
-rw-rw---- 1 u0_a123 media_rw 1536 2026-06-28 12:31 Pixel.txt
drwxr-xr-x    2 root     root          4096 Jan  1  1970 Samsung
lrwxrwxrwx 1 root root 21 2024-02-03 04:05 FireOS -> /storage/emulated/0
bad-coloros-row-without-columns
"#;
        let rows = parse_ls_output(transcript);
        assert!(rows.iter().any(|row| row.name == "Download" && row.is_dir));
        assert!(rows
            .iter()
            .any(|row| row.name == "Pixel.txt" && row.size == Some(1536)));
        assert!(rows.iter().any(|row| row.name == "Samsung" && row.is_dir));
        assert!(rows
            .iter()
            .any(|row| row.name == "FireOS -> /storage/emulated/0"));
        assert!(rows
            .iter()
            .any(|row| row.name.contains("bad-coloros") && row.parse_error.is_some()));
        assert!(parse_ls_output("").is_empty());
    }

    #[test]
    fn parses_fastboot_devices_and_no_permission_rows() {
        let rows = parse_fastboot_devices(
            r#"
ZY22 fastboot product:oriole variant:pixel
R58M fastboot product:dm1q
emulator-5554 fastboot
???????? no permissions (udev rules); see [http://developer.android.com/tools/device.html]
"#,
        );
        assert_eq!(rows[0].product.as_deref(), Some("oriole"));
        assert_eq!(rows[1].product.as_deref(), Some("dm1q"));
        assert_eq!(rows[2].serial, "emulator-5554");
        assert_eq!(rows[3].mode, "no_permissions");
        assert!(rows[3].parse_error.is_some());
        assert!(parse_fastboot_devices("").is_empty());
    }

    #[test]
    fn parses_network_rows_from_ss_netstat_and_degrades_malformed_rows() {
        let rows = parse_ss_output(
            r#"
Netid State Recv-Q Send-Q Local Address:Port Peer Address:Port Process
tcp ESTAB 0 0 192.168.1.10:43210 142.250.190.14:443 users:(("chrome",pid=231,fd=8))
udp UNCONN 0 0 0.0.0.0:5353 0.0.0.0:* users:(("mdnsd",pid=44,fd=12))
Proto Recv-Q Send-Q Local Address Foreign Address State PID/Program name
tcp 0 0 127.0.0.1:5037 0.0.0.0:* LISTEN 1234/adb
hyperos-short-row
"#,
        );
        assert!(rows.iter().any(|row| row.protocol == "tcp"
            && row.state == "ESTAB"
            && row.local_addr == "192.168.1.10:43210"));
        assert!(rows
            .iter()
            .any(|row| row.protocol == "udp" && row.state == "UNCONN"));
        assert!(rows.iter().any(
            |row| row.remote_addr == "0.0.0.0:*" && row.process.as_deref() == Some("1234/adb")
        ));
        assert!(rows.iter().any(|row| row.parse_error.is_some()));
        assert!(parse_ss_output("").is_empty());
    }

    #[test]
    fn parses_process_rows_from_requested_and_oem_headers() {
        let requested = parse_ps_output(
            r#"
PID USER VSZ RSS NAME
123 u0_a123 204800 12345 com.pixel.app
bad-oppo-row
"#,
        );
        assert_eq!(requested[0].pid, 123);
        assert_eq!(requested[0].name, "com.pixel.app");
        assert!(requested[1].parse_error.is_some());

        let samsung = parse_ps_output(
            r#"
USER PID PPID VSZ RSS WCHAN ADDR S NAME
system 456 1 102400 2048 0 0 S system_server
u0_a55 789 1 51200 4096 0 0 S com.samsung.android.app
"#,
        );
        assert_eq!(samsung[0].pid, 456);
        assert_eq!(samsung[1].name, "com.samsung.android.app");

        let emulator = parse_ps_output("u0_a1 901 1 1000 200 0 0 S com.android.shell\n");
        assert_eq!(emulator[0].pid, 0);
        assert!(emulator[0].parse_error.is_some());
        assert!(parse_ps_output("").is_empty());
    }
}
