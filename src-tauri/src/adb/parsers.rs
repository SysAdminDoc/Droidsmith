use serde::Serialize;

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RemoteFileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub permissions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FastbootDevice {
    pub serial: String,
    pub mode: String,
    pub product: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NetworkConnection {
    pub state: String,
    pub protocol: String,
    pub local_addr: String,
    pub remote_addr: String,
    pub process: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub user: String,
    pub vsz_kb: u64,
    pub rss_kb: u64,
    /// Cumulative CPU usage as reported by `ps -o %CPU`, when the column is
    /// present. A single-snapshot value (not a live rate) — `None` on OEM `ps`
    /// builds that omit the column.
    pub cpu_percent: Option<f32>,
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

    let date_idx = tokens.iter().enumerate().skip(1).find_map(|(idx, token)| {
        let is_date =
            is_iso_date(token) || (is_month_name(token) && is_month_date_sequence(&tokens, idx));
        is_date.then_some(idx)
    })?;
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
    token.len() >= 10
        && token.as_bytes()[4] == b'-'
        && token.as_bytes()[7] == b'-'
        && token.as_bytes()[0..4].iter().all(u8::is_ascii_digit)
        && token.as_bytes()[5..7].iter().all(u8::is_ascii_digit)
        && token.as_bytes()[8..10].iter().all(u8::is_ascii_digit)
}

/// A bare month name is only a date column when it heads a `Mon DD YYYY|HH:MM`
/// sequence. Without this, an owner/group literally named e.g. `May` would be
/// mistaken for the date and shift the size/name columns silently.
fn is_month_date_sequence(tokens: &[&str], idx: usize) -> bool {
    let day_looks_valid = tokens
        .get(idx + 1)
        .and_then(|day| day.parse::<u8>().ok())
        .is_some_and(|day| (1..=31).contains(&day));
    let time_or_year = tokens
        .get(idx + 2)
        .is_some_and(|token| is_clock(token) || is_year(token));
    day_looks_valid && time_or_year
}

fn is_clock(token: &str) -> bool {
    let bytes = token.as_bytes();
    token.len() == 5
        && bytes[2] == b':'
        && bytes[0..2].iter().all(u8::is_ascii_digit)
        && bytes[3..5].iter().all(u8::is_ascii_digit)
}

fn is_year(token: &str) -> bool {
    token.len() == 4 && token.as_bytes().iter().all(u8::is_ascii_digit)
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
        // netstat-style `proto recv-q send-q local foreign [state]`. The
        // state column is optional (UDP rows omit it), so use tokens[5]
        // only when it is a real TCP state; otherwise the socket is
        // stateless — never fall back to the numeric recv-q column, which
        // would surface a bogus `state = "0"`.
        let (state, process_idx) = match tokens.get(5) {
            Some(token) if is_tcp_state(token) => ((*token).to_string(), 6),
            _ => ("UNCONN".to_string(), 5),
        };
        return Some(NetworkConnection {
            protocol,
            state,
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
        cpu_percent: column_index(columns, &["%CPU", "PCPU", "CPU"])
            .and_then(|idx| tokens.get(idx))
            .and_then(|token| token.parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0),
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
        cpu_percent: None,
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
        cpu_percent: None,
        name: line.to_string(),
        parse_error: Some("unrecognized process row".to_string()),
    }
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RunningService {
    pub component: String,
}

pub fn parse_running_services(stdout: &str) -> Vec<RunningService> {
    let mut services = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("* ServiceRecord{") {
            let tokens: Vec<&str> = rest.split_whitespace().collect();
            let component = tokens
                .iter()
                .find(|t| t.contains('/'))
                .map(|t| t.trim_end_matches('}'));
            if let Some(component) = component {
                services.push(RunningService {
                    component: component.to_string(),
                });
            }
        }
    }
    services
}

/// One node of a `uiautomator dump` UI hierarchy, flattened with its nesting
/// `depth` so the renderer can indent it. Malformed structure yields a node
/// with `parse_error` set instead of being silently dropped.
#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LayoutNode {
    pub depth: u32,
    pub index: String,
    pub class: String,
    pub package: String,
    pub text: String,
    pub content_desc: String,
    pub resource_id: String,
    pub bounds: String,
    /// Exact attribute payload from the source `<node ...>` tag. Kept so an
    /// audit finding can expose inspectable evidence without reparsing XML in
    /// the renderer.
    pub raw_attributes: String,
    pub clickable: bool,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutAuditKind {
    MissingAccessibleLabel,
    DuplicateResourceId,
    SmallClickTarget,
}

#[derive(specta::Type, Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LayoutAuditFinding {
    pub id: String,
    pub kind: LayoutAuditKind,
    pub node_index: u32,
    pub related_node_indices: Vec<u32>,
    pub resource_id: String,
    pub bounds: String,
    pub width_px: Option<u32>,
    pub height_px: Option<u32>,
    pub width_dp_tenths: Option<u32>,
    pub height_dp_tenths: Option<u32>,
}

/// Run deterministic checks that are supportable from a UIAutomator XML dump.
/// Contrast and rendered pixels are intentionally outside this evidence model.
pub fn audit_layout_nodes(
    nodes: &[LayoutNode],
    density_dpi: Option<u32>,
) -> Vec<LayoutAuditFinding> {
    let mut ids = std::collections::BTreeMap::<&str, Vec<u32>>::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.parse_error.is_none() && !node.resource_id.is_empty() {
            ids.entry(&node.resource_id).or_default().push(index as u32);
        }
    }

    let mut findings = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.parse_error.is_some() {
            continue;
        }
        let node_index = index as u32;
        let dimensions = parse_layout_bounds(&node.bounds);
        let (width_px, height_px) = dimensions.unzip();
        let width_dp_tenths = width_px
            .zip(density_dpi)
            .map(|(width, density)| px_to_dp_tenths(width, density));
        let height_dp_tenths = height_px
            .zip(density_dpi)
            .map(|(height, density)| px_to_dp_tenths(height, density));
        let finding = |kind: LayoutAuditKind, related_node_indices: Vec<u32>| {
            let kind_id = match &kind {
                LayoutAuditKind::MissingAccessibleLabel => "missing_accessible_label",
                LayoutAuditKind::DuplicateResourceId => "duplicate_resource_id",
                LayoutAuditKind::SmallClickTarget => "small_click_target",
            };
            LayoutAuditFinding {
                id: format!("{kind_id}:{node_index}"),
                kind,
                node_index,
                related_node_indices,
                resource_id: node.resource_id.clone(),
                bounds: node.bounds.clone(),
                width_px,
                height_px,
                width_dp_tenths,
                height_dp_tenths,
            }
        };

        if node.clickable && node.text.trim().is_empty() && node.content_desc.trim().is_empty() {
            findings.push(finding(
                LayoutAuditKind::MissingAccessibleLabel,
                vec![node_index],
            ));
        }
        if let Some(duplicates) = ids
            .get(node.resource_id.as_str())
            .filter(|matches| matches.len() > 1)
        {
            findings.push(finding(
                LayoutAuditKind::DuplicateResourceId,
                duplicates.clone(),
            ));
        }
        if node.clickable
            && dimensions.is_some_and(|(width, height)| {
                density_dpi.is_some_and(|density| {
                    (width as u64) * 160 < 48 * (density as u64)
                        || (height as u64) * 160 < 48 * (density as u64)
                })
            })
        {
            findings.push(finding(LayoutAuditKind::SmallClickTarget, vec![node_index]));
        }
    }
    findings
}

pub fn parse_effective_density(output: &str) -> Option<u32> {
    let mut physical = None;
    let mut override_density = None;
    for line in output.lines() {
        let line = line.trim();
        let parsed = line
            .split_once(':')
            .and_then(|(_, value)| value.trim().parse::<u32>().ok())
            .filter(|value| (72..=1000).contains(value));
        if line.starts_with("Override density:") {
            override_density = parsed;
        } else if line.starts_with("Physical density:") {
            physical = parsed;
        }
    }
    override_density.or(physical)
}

fn parse_layout_bounds(value: &str) -> Option<(u32, u32)> {
    let coordinates = value
        .split(['[', ']', ','])
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().parse::<i64>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let [left, top, right, bottom] = coordinates.as_slice() else {
        return None;
    };
    if right <= left || bottom <= top {
        return None;
    }
    Some((
        u32::try_from(right - left).ok()?,
        u32::try_from(bottom - top).ok()?,
    ))
}

fn px_to_dp_tenths(px: u32, density_dpi: u32) -> u32 {
    if density_dpi == 0 {
        return 0;
    }
    (((px as u64) * 1600 + (density_dpi as u64 / 2)) / density_dpi as u64).min(u32::MAX as u64)
        as u32
}

/// Parse a `uiautomator dump` XML document into a depth-flattened node list.
/// The uiautomator format escapes `<`, `>`, `&`, and `"` inside attribute
/// values, so a quote-aware scan is sufficient and needs no XML dependency.
pub fn parse_uiautomator_dump(xml: &str) -> Vec<LayoutNode> {
    let mut nodes = Vec::new();
    let mut depth: i64 = 0;
    let mut cursor = 0usize;
    while let Some(rel) = xml[cursor..].find('<') {
        let start = cursor + rel;
        let rest = &xml[start..];
        if rest.starts_with("</node>") {
            depth -= 1;
            if depth < 0 {
                nodes.push(layout_parse_error("unbalanced closing node"));
                depth = 0;
            }
            cursor = start + "</node>".len();
        } else if rest.starts_with("<node")
            && matches!(
                rest.as_bytes().get(5),
                Some(b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>')
            )
        {
            match find_node_tag_end(rest) {
                Some((end, self_closing)) => {
                    let attrs_end = if self_closing { end - 1 } else { end };
                    let attrs = &rest[5..attrs_end];
                    nodes.push(build_layout_node(attrs, depth.max(0) as u32));
                    if !self_closing {
                        depth += 1;
                    }
                    cursor = start + end + 1;
                }
                None => {
                    nodes.push(layout_parse_error("unterminated node tag"));
                    break;
                }
            }
        } else {
            // Non-node markup (<?xml?>, <hierarchy>, </hierarchy>) is skipped.
            cursor = start + 1;
        }
    }
    if depth != 0 {
        nodes.push(layout_parse_error("unbalanced node nesting"));
    }
    if nodes.is_empty() {
        nodes.push(layout_parse_error("no UI nodes found in dump"));
    }
    nodes
}

fn find_node_tag_end(tag: &str) -> Option<(usize, bool)> {
    let bytes = tag.as_bytes();
    let mut in_quote = false;
    let mut last_non_space = b' ';
    let mut i = 5;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'"' {
            in_quote = !in_quote;
        } else if byte == b'>' && !in_quote {
            return Some((i, last_non_space == b'/'));
        }
        if !in_quote && !byte.is_ascii_whitespace() {
            last_non_space = byte;
        }
        i += 1;
    }
    None
}

fn build_layout_node(attrs: &str, depth: u32) -> LayoutNode {
    let map = parse_node_attributes(attrs);
    let take = |key: &str| map.get(key).cloned().unwrap_or_default();
    let flag = |key: &str| map.get(key).map(|value| value == "true").unwrap_or(false);
    LayoutNode {
        depth,
        index: take("index"),
        class: take("class"),
        package: take("package"),
        text: take("text"),
        content_desc: take("content-desc"),
        resource_id: take("resource-id"),
        bounds: take("bounds"),
        raw_attributes: attrs.trim().to_string(),
        clickable: flag("clickable"),
        enabled: flag("enabled"),
        parse_error: None,
    }
}

fn parse_node_attributes(attrs: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    let bytes = attrs.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
        {
            i += 1;
        }
        if name_start == i {
            break;
        }
        let name = &attrs[name_start..i];
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'=') {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'"' {
            continue;
        }
        i += 1;
        let value_start = i;
        while i < bytes.len() && bytes[i] != b'"' {
            i += 1;
        }
        let value = &attrs[value_start..i.min(bytes.len())];
        if i < bytes.len() {
            i += 1;
        }
        map.insert(name.to_string(), unescape_xml(value));
    }
    map
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn layout_parse_error(message: &str) -> LayoutNode {
    LayoutNode {
        depth: 0,
        index: String::new(),
        class: String::new(),
        package: String::new(),
        text: String::new(),
        content_desc: String::new(),
        resource_id: String::new(),
        bounds: String::new(),
        raw_attributes: String::new(),
        clickable: false,
        enabled: false,
        parse_error: Some(message.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_uiautomator_hierarchy_with_depth_and_entities() {
        let xml = r#"<?xml version='1.0' encoding='UTF-8'?>
<hierarchy rotation="0">
  <node index="0" class="android.widget.FrameLayout" package="com.example.app" text="" content-desc="" resource-id="" bounds="[0,0][1080,2400]" clickable="false" enabled="true">
    <node index="1" class="android.widget.TextView" package="com.example.app" text="Tom &amp; Jerry &lt;3" content-desc="Play" resource-id="com.example.app:id/title" bounds="[10,20][300,80]" clickable="true" enabled="true" />
  </node>
</hierarchy>"#;
        let nodes = parse_uiautomator_dump(xml);
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().all(|node| node.parse_error.is_none()));
        assert_eq!(nodes[0].depth, 0);
        assert_eq!(nodes[0].class, "android.widget.FrameLayout");
        assert_eq!(nodes[1].depth, 1);
        assert_eq!(nodes[1].text, "Tom & Jerry <3");
        assert_eq!(nodes[1].content_desc, "Play");
        assert_eq!(nodes[1].resource_id, "com.example.app:id/title");
        assert!(nodes[1].raw_attributes.contains("content-desc=\"Play\""));
        assert!(nodes[1].clickable);
    }

    #[test]
    fn parses_effective_density_and_prefers_an_override() {
        assert_eq!(
            parse_effective_density("Physical density: 420\n"),
            Some(420)
        );
        assert_eq!(
            parse_effective_density("Physical density: 420\nOverride density: 320\n"),
            Some(320)
        );
        assert_eq!(parse_effective_density("Physical density: unknown\n"), None);
        assert_eq!(parse_effective_density("Physical density: 12\n"), None);
    }

    #[test]
    fn audits_labels_duplicate_ids_and_density_aware_target_sizes() {
        let xml = include_str!("../../fixtures/layout/accessibility-audit.xml");
        let nodes = parse_uiautomator_dump(xml);
        let findings = audit_layout_nodes(&nodes, Some(320));

        assert_eq!(findings.len(), 4);
        assert_eq!(
            findings
                .iter()
                .filter(|finding| finding.kind == LayoutAuditKind::MissingAccessibleLabel)
                .count(),
            1
        );
        let duplicates = findings
            .iter()
            .filter(|finding| finding.kind == LayoutAuditKind::DuplicateResourceId)
            .collect::<Vec<_>>();
        assert_eq!(duplicates.len(), 2);
        assert!(duplicates
            .iter()
            .all(|finding| finding.related_node_indices == vec![1, 2]));
        let small = findings
            .iter()
            .find(|finding| finding.kind == LayoutAuditKind::SmallClickTarget)
            .expect("small target finding");
        assert_eq!((small.width_px, small.height_px), (Some(80), Some(60)));
        assert_eq!(
            (small.width_dp_tenths, small.height_dp_tenths),
            (Some(400), Some(300))
        );
    }

    #[test]
    fn malformed_dumps_surface_parse_errors_rather_than_dropping() {
        let empty = parse_uiautomator_dump("<hierarchy></hierarchy>");
        assert_eq!(empty.len(), 1);
        assert_eq!(
            empty[0].parse_error.as_deref(),
            Some("no UI nodes found in dump")
        );

        let unbalanced =
            parse_uiautomator_dump("<hierarchy><node index=\"0\" class=\"X\"></hierarchy>");
        assert!(unbalanced
            .iter()
            .any(|node| node.parse_error.as_deref() == Some("unbalanced node nesting")));

        let garbage = parse_uiautomator_dump("not xml at all");
        assert_eq!(garbage.len(), 1);
        assert!(garbage[0].parse_error.is_some());
    }

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
    fn month_named_owner_does_not_shift_size_and_name_columns() {
        // Owner "May" is a month abbreviation; the real date is the later
        // `May 06 09:12` triple. The parser must not lock onto the owner.
        let rows = parse_ls_output("-rw-r--r-- 1 May staff 4096 May 06 09:12 notes.txt");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "notes.txt");
        assert_eq!(rows[0].size, Some(4096));
        assert!(rows[0].parse_error.is_none());
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
udp 0 0 10.0.0.1:68 0.0.0.0:*
hyperos-short-row
"#,
        );
        assert!(rows.iter().any(|row| row.protocol == "tcp"
            && row.state == "ESTAB"
            && row.local_addr == "192.168.1.10:43210"));
        assert!(rows
            .iter()
            .any(|row| row.protocol == "udp" && row.state == "UNCONN"));
        // Netstat-style stateless UDP row must not surface the recv-q
        // number ("0") as its state.
        let netstat_udp = rows
            .iter()
            .find(|row| row.local_addr == "10.0.0.1:68")
            .expect("netstat udp row parsed");
        assert_eq!(netstat_udp.state, "UNCONN");
        assert_eq!(netstat_udp.protocol, "udp");
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

    #[test]
    fn parses_cpu_percent_column_when_present() {
        let rows = parse_ps_output(
            "PID USER VSZ RSS %CPU NAME\n123 u0_a1 204800 12345 7.5 com.pixel.app\n",
        );
        assert_eq!(rows[0].rss_kb, 12345);
        assert_eq!(rows[0].cpu_percent, Some(7.5));

        // OEM headers without a CPU column leave it None rather than misreading.
        let no_cpu = parse_ps_output("PID USER VSZ RSS NAME\n5 root 10 20 init\n");
        assert_eq!(no_cpu[0].cpu_percent, None);
    }

    #[test]
    fn parses_running_service_records() {
        let output = r#"ACTIVITY MANAGER SERVICES (dumpsys activity services)
  User 0 active services:
  * ServiceRecord{abc1234 u0 com.example.app/.sync.SyncService}
    intent={cmp=com.example.app/.sync.SyncService}
    packageName=com.example.app
    processName=com.example.app
    app=ProcessRecord{def5678 1234:com.example.app/u0a55}
  * ServiceRecord{ghi9012 u0 com.example.app/.push.PushService}
    intent={cmp=com.example.app/.push.PushService}
    packageName=com.example.app
    processName=com.example.app:push
"#;
        let services = parse_running_services(output);
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].component, "com.example.app/.sync.SyncService");
        assert_eq!(services[1].component, "com.example.app/.push.PushService");
        assert!(parse_running_services("").is_empty());
        assert!(parse_running_services("No services\n").is_empty());
    }
}
