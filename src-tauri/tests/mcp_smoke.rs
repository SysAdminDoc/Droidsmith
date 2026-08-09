use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn mcp_binary() -> PathBuf {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let binary = if cfg!(windows) {
        "droidsmith-mcp.exe"
    } else {
        "droidsmith-mcp"
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(profile)
        .join(binary)
}

#[test]
fn stdio_protocol_keeps_stdout_json_only_and_gates_mutations() {
    let mut child = Command::new(mcp_binary())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to execute droidsmith-mcp");
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"profile_apply\",\"arguments\":{\"profile_path\":\"profile.yaml\",\"device\":\"QA1\"}}}\n",
    );
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(input.as_bytes())
        .expect("write MCP requests");
    let output = child.wait_with_output().expect("wait for droidsmith-mcp");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let lines = String::from_utf8(output.stdout)
        .expect("MCP stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("JSON-RPC line"))
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "droidsmith-mcp");
    assert!(lines[1]["result"]["tools"]
        .as_array()
        .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == "packages_list")));
    assert_eq!(lines[2]["result"]["isError"], true);
    assert_eq!(
        lines[2]["result"]["structuredContent"]["error"]["code"],
        "confirmation_required"
    );
}
