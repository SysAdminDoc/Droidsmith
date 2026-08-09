//! Local Model Context Protocol (MCP) adapter for the headless CLI.
//!
//! The transport is intentionally the smallest MCP transport: one UTF-8 JSON
//! message per stdin line and one response per request on stdout. The server
//! never opens a listener, emits telemetry, or keeps device state. Device
//! operations are delegated to the sibling droidsmith-cli executable so the
//! MCP and CLI surfaces retain one validation and journaling boundary.

use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::adb::device::valid_serial;
use crate::fleet_report;

pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

const JSON_RPC_VERSION: &str = "2.0";
const JSON_RPC_PARSE_ERROR: i32 = -32_700;
const JSON_RPC_INVALID_REQUEST: i32 = -32_600;
const JSON_RPC_METHOD_NOT_FOUND: i32 = -32_601;
const JSON_RPC_INVALID_PARAMS: i32 = -32_602;
const MCP_NOT_INITIALIZED: i32 = -32_002;
const MAX_ARGUMENT_TEXT: usize = 4_096;
const MAX_CLI_ERROR_TEXT: usize = 8_192;

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Default)]
struct ServerState {
    initialized: bool,
}

#[derive(Debug)]
struct CliRun {
    value: Value,
    success: bool,
    exit_code: i32,
}

/// Run the MCP stdio loop. stdout is reserved for JSON-RPC messages; all
/// operational errors are represented in the response and only transport
/// failures reaching this loop are returned to the binary entry point.
pub fn run_stdio() -> io::Result<()> {
    let stdin = io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    let mut line = String::new();
    let mut state = ServerState::default();

    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => dispatch(&mut state, request),
            Err(error) => Some(rpc_error(
                Value::Null,
                JSON_RPC_PARSE_ERROR,
                format!("invalid JSON-RPC message: {error}"),
            )),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response).map_err(io::Error::other)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}

fn dispatch(state: &mut ServerState, request: JsonRpcRequest) -> Option<Value> {
    let is_notification = request.id.is_none();
    let id = request.id.unwrap_or(Value::Null);
    if request.jsonrpc != JSON_RPC_VERSION {
        return Some(rpc_error(
            id,
            JSON_RPC_INVALID_REQUEST,
            "jsonrpc must be \"2.0\"",
        ));
    }

    let response = match request.method.as_str() {
        "initialize" => {
            state.initialized = true;
            rpc_result(id, initialize_result())
        }
        "notifications/initialized" => {
            state.initialized = true;
            rpc_result(id, json!({}))
        }
        "notifications/cancelled" => return None,
        "ping" => {
            if !state.initialized {
                rpc_error(id, MCP_NOT_INITIALIZED, "initialize must complete first")
            } else {
                rpc_result(id, json!({}))
            }
        }
        "tools/list" => {
            if !state.initialized {
                rpc_error(id, MCP_NOT_INITIALIZED, "initialize must complete first")
            } else {
                rpc_result(id, json!({ "tools": tool_definitions() }))
            }
        }
        "tools/call" => {
            if !state.initialized {
                rpc_error(id, MCP_NOT_INITIALIZED, "initialize must complete first")
            } else {
                match call_tool(&request.params) {
                    Ok(result) => rpc_result(id, result),
                    Err(message) => rpc_error(id, JSON_RPC_INVALID_PARAMS, message),
                }
            }
        }
        _ => rpc_error(
            id,
            JSON_RPC_METHOD_NOT_FOUND,
            format!("method {:?} is not supported", request.method),
        ),
    };

    if is_notification {
        None
    } else {
        Some(response)
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "droidsmith-mcp",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Droidsmith is a local stdio server. Read-only tools inspect the local ADB fleet and local report files. Mutating tools require confirmed=true and reuse droidsmith-cli validation and journaling.",
    })
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": JSON_RPC_VERSION,
        "id": id,
        "result": result,
    })
}

fn rpc_error(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": JSON_RPC_VERSION,
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        },
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "devices_list",
            "List connected ADB devices using the local Droidsmith CLI. Read-only; no network listener or telemetry is used.",
            true,
            object_schema(json!({}), json!([])),
        ),
        tool(
            "packages_list",
            "List packages for one connected device and Android user. Read-only; package metadata comes from the device's PackageManager.",
            true,
            object_schema(
                json!({
                    "device": serial_schema(),
                    "user": { "type": "integer", "minimum": 0, "default": 0 },
                    "filter": filter_schema(),
                    "allow_unsafe_transport": unsafe_transport_schema(),
                }),
                json!(["device"]),
            ),
        ),
        tool(
            "profile_plan",
            "Create a read-only profile plan for one device or the connected fleet. No action is applied.",
            true,
            profile_schema(false),
        ),
        tool(
            "baseline_inspect",
            "Inspect a recovery baseline against one device or the connected fleet. Read-only and rediscovers live state.",
            true,
            baseline_schema(false),
        ),
        tool(
            "fleet_report_read",
            "Read a local fleet report through Droidsmith's redacted read-only report view. Device serials are returned as identity digests.",
            true,
            object_schema(
                json!({
                    "report_path": path_schema("Path to a local run or pack fleet report JSON."),
                }),
                json!(["report_path"]),
            ),
        ),
        tool(
            "pack_plan",
            "Create a read-only debloat-pack plan for one device or the connected fleet. No package action is applied.",
            true,
            pack_schema(false),
        ),
        tool(
            "profile_apply",
            "MUTATING: apply a reviewed profile to one device or the connected fleet. Requires confirmed=true; the CLI normal validation and journal remain in force.",
            false,
            profile_schema(true),
        ),
        tool(
            "baseline_apply",
            "MUTATING: apply a recovery-baseline restore or reapply plan to one device. Requires confirmed=true and the CLI live diff and journal checks.",
            false,
            baseline_schema(true),
        ),
        tool(
            "pack_apply",
            "MUTATING: apply a reviewed debloat pack to one device or the connected fleet. Requires confirmed=true plus any explicit compatibility or unsafe-tier acknowledgement.",
            false,
            pack_schema(true),
        ),
    ]
}

fn tool(name: &str, description: &str, read_only: bool, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": !read_only,
            "idempotentHint": read_only,
            "openWorldHint": false,
        },
    })
}

fn object_schema(properties: Value, required: Value) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn serial_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": 256,
        "description": "Validated ADB serial or host:port device selector.",
    })
}

fn path_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": MAX_ARGUMENT_TEXT,
        "description": description,
    })
}

fn unsafe_transport_schema() -> Value {
    json!({
        "type": "boolean",
        "default": false,
        "description": "Acknowledge an unauthenticated legacy/unknown TCP transport after reviewing the risk.",
    })
}

fn filter_schema() -> Value {
    json!({
        "type": "string",
        "enum": ["all", "user", "system", "enabled", "disabled", "archived", "retained"],
        "default": "all",
    })
}

fn selector_properties() -> Value {
    json!({
        "device": serial_schema(),
        "all_devices": {
            "type": "boolean",
            "default": false,
            "description": "Select every connected device instead of supplying device.",
        },
        "allow_unsafe_transport": unsafe_transport_schema(),
    })
}

fn profile_schema(mutating: bool) -> Value {
    let mut properties = selector_properties()
        .as_object()
        .expect("selector properties is an object")
        .clone();
    properties.insert(
        "profile_path".to_string(),
        path_schema("Path to a schema-v2/v3 profile YAML file."),
    );
    if mutating {
        properties.insert(
            "confirmed".to_string(),
            json!({
                "type": "boolean",
                "default": false,
                "description": "Required explicit confirmation for this mutating tool.",
            }),
        );
    }
    let required = if mutating {
        json!(["profile_path", "confirmed"])
    } else {
        json!(["profile_path"])
    };
    object_schema(Value::Object(properties), required)
}

fn baseline_schema(mutating: bool) -> Value {
    let mut properties = json!({
        "baseline_path": path_schema("Path to a recovery baseline JSON file."),
        "device": serial_schema(),
        "all_devices": {
            "type": "boolean",
            "default": false,
            "description": "Inspect every connected device. Not valid for baseline_apply.",
        },
        "direction": {
            "type": "string",
            "enum": ["restore", "reapply"],
            "default": "restore",
        },
        "allow_unsafe_transport": unsafe_transport_schema(),
    })
    .as_object()
    .expect("baseline properties is an object")
    .clone();
    if mutating {
        properties.insert(
            "confirmed".to_string(),
            json!({
                "type": "boolean",
                "default": false,
                "description": "Required explicit confirmation for this mutating tool.",
            }),
        );
    }
    let required = if mutating {
        json!(["baseline_path", "device", "confirmed"])
    } else {
        json!(["baseline_path"])
    };
    object_schema(Value::Object(properties), required)
}

fn pack_schema(mutating: bool) -> Value {
    let mut properties = selector_properties()
        .as_object()
        .expect("selector properties is an object")
        .clone();
    properties.insert(
        "pack".to_string(),
        path_schema("Pack id or path to a local pack YAML file."),
    );
    properties.insert(
        "user".to_string(),
        json!({ "type": "integer", "minimum": 0 }),
    );
    properties.insert(
        "allow_unsafe_tier".to_string(),
        json!({
            "type": "boolean",
            "default": false,
            "description": "Acknowledge entries classified as unsafe after reviewing the plan.",
        }),
    );
    properties.insert(
        "accept_compatibility".to_string(),
        json!({
            "type": "boolean",
            "default": false,
            "description": "Acknowledge a pack/device compatibility override after reviewing the plan.",
        }),
    );
    if mutating {
        properties.insert(
            "confirmed".to_string(),
            json!({
                "type": "boolean",
                "default": false,
                "description": "Required explicit confirmation for this mutating tool.",
            }),
        );
    }
    let required = if mutating {
        json!(["pack", "confirmed"])
    } else {
        json!(["pack"])
    };
    object_schema(Value::Object(properties), required)
}

fn call_tool(params: &Value) -> Result<Value, String> {
    let params = params
        .as_object()
        .ok_or_else(|| "tools/call params must be an object".to_string())?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tools/call requires a string name".to_string())?;
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(arguments)) => arguments.clone(),
        Some(_) => return Err("tools/call arguments must be an object".to_string()),
    };
    Ok(execute_tool(name, &arguments))
}

fn execute_tool(name: &str, arguments: &Map<String, Value>) -> Value {
    match name {
        "devices_list" => run_cli_tool(vec!["devices".to_string(), "--json".to_string()]),
        "packages_list" => packages_list(arguments),
        "profile_plan" => profile_operation(arguments, false),
        "baseline_inspect" => baseline_inspect(arguments),
        "fleet_report_read" => fleet_report_read(arguments),
        "pack_plan" => pack_operation(arguments, false),
        "profile_apply" => profile_operation(arguments, true),
        "baseline_apply" => baseline_apply(arguments),
        "pack_apply" => pack_operation(arguments, true),
        _ => failure_result(
            "unknown_tool",
            format!("tool {name:?} is not supported"),
            None,
        ),
    }
}

fn packages_list(arguments: &Map<String, Value>) -> Value {
    let device = match required_device(arguments) {
        Ok(device) => device,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let user = match optional_u32(arguments, "user") {
        Ok(user) => user.unwrap_or(0),
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let filter = match optional_filter(arguments) {
        Ok(filter) => filter,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let mut command = vec![
        "packages".to_string(),
        "--device".to_string(),
        device,
        "--user".to_string(),
        user.to_string(),
        "--filter".to_string(),
        filter,
        "--json".to_string(),
    ];
    append_bool_flag(
        &mut command,
        arguments,
        "allow_unsafe_transport",
        "--allow-unsafe-transport",
    );
    run_cli_tool(command)
}

fn profile_operation(arguments: &Map<String, Value>, mutating: bool) -> Value {
    if mutating && !is_confirmed(arguments) {
        return confirmation_required("profile_apply");
    }
    let profile_path = match required_string(arguments, "profile_path") {
        Ok(path) => path,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let selector = match selector(arguments) {
        Ok(selector) => selector,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let mut command = vec!["run".to_string(), profile_path];
    command.extend(selector);
    command.push(if mutating {
        "--apply".to_string()
    } else {
        "--dry-run".to_string()
    });
    command.push("--json".to_string());
    append_bool_flag(
        &mut command,
        arguments,
        "allow_unsafe_transport",
        "--allow-unsafe-transport",
    );
    run_cli_tool(command)
}

fn baseline_inspect(arguments: &Map<String, Value>) -> Value {
    let baseline_path = match required_string(arguments, "baseline_path") {
        Ok(path) => path,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let selector = match selector(arguments) {
        Ok(selector) => selector,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let direction = match optional_direction(arguments) {
        Ok(direction) => direction,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let mut command = vec!["baseline-inspect".to_string(), baseline_path];
    command.extend(selector);
    command.extend(["--direction".to_string(), direction, "--json".to_string()]);
    append_bool_flag(
        &mut command,
        arguments,
        "allow_unsafe_transport",
        "--allow-unsafe-transport",
    );
    run_cli_tool(command)
}

fn baseline_apply(arguments: &Map<String, Value>) -> Value {
    if !is_confirmed(arguments) {
        return confirmation_required("baseline_apply");
    }
    let baseline_path = match required_string(arguments, "baseline_path") {
        Ok(path) => path,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let device = match required_device(arguments) {
        Ok(device) => device,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let direction = match optional_direction(arguments) {
        Ok(direction) => direction,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let mut command = vec![
        "baseline-apply".to_string(),
        baseline_path,
        "--device".to_string(),
        device,
        "--direction".to_string(),
        direction,
        "--apply".to_string(),
        "--json".to_string(),
    ];
    append_bool_flag(
        &mut command,
        arguments,
        "allow_unsafe_transport",
        "--allow-unsafe-transport",
    );
    run_cli_tool(command)
}

fn pack_operation(arguments: &Map<String, Value>, mutating: bool) -> Value {
    if mutating && !is_confirmed(arguments) {
        return confirmation_required("pack_apply");
    }
    let pack = match required_string(arguments, "pack") {
        Ok(pack) => pack,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let selector = match selector(arguments) {
        Ok(selector) => selector,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let mut command = vec![
        "pack".to_string(),
        if mutating {
            "apply".to_string()
        } else {
            "plan".to_string()
        },
        pack,
    ];
    command.extend(selector);
    match optional_u32(arguments, "user") {
        Ok(Some(user)) => command.extend(["--user".to_string(), user.to_string()]),
        Ok(None) => {}
        Err(error) => return failure_result("invalid_params", error, None),
    }
    command.push("--json".to_string());
    if mutating {
        command.push("--apply".to_string());
    }
    append_bool_flag(
        &mut command,
        arguments,
        "allow_unsafe_transport",
        "--allow-unsafe-transport",
    );
    append_bool_flag(
        &mut command,
        arguments,
        "allow_unsafe_tier",
        "--allow-unsafe-tier",
    );
    append_bool_flag(
        &mut command,
        arguments,
        "accept_compatibility",
        "--accept-compatibility",
    );
    run_cli_tool(command)
}

fn fleet_report_read(arguments: &Map<String, Value>) -> Value {
    let path = match required_string(arguments, "report_path") {
        Ok(path) => path,
        Err(error) => return failure_result("invalid_params", error, None),
    };
    let loaded = match fleet_report::load_for_view(Path::new(&path)) {
        Ok(loaded) => loaded,
        Err(error) => return failure_result(error.code(), error.to_string(), None),
    };
    success_result(json!({
        "schema_version": 1,
        "command": "fleet-report-read",
        "source_sha256": loaded.source_sha256,
        "report": fleet_report::view(&loaded.report),
    }))
}

fn selector(arguments: &Map<String, Value>) -> Result<Vec<String>, String> {
    let device = optional_string(arguments, "device")?;
    let all_devices = optional_bool(arguments, "all_devices")?.unwrap_or(false);
    match (device, all_devices) {
        (Some(device), false) => {
            if !valid_serial(&device) {
                Err(format!("invalid device serial {device:?}"))
            } else {
                Ok(vec!["--device".to_string(), device])
            }
        }
        (None, true) => Ok(vec!["--all-devices".to_string()]),
        (Some(_), true) => Err("pass either device or all_devices, not both".to_string()),
        (None, false) => Err("pass device or set all_devices=true".to_string()),
    }
}

fn required_device(arguments: &Map<String, Value>) -> Result<String, String> {
    let device = required_string(arguments, "device")?;
    if !valid_serial(&device) {
        return Err(format!("invalid device serial {device:?}"));
    }
    Ok(device)
}

fn required_string(arguments: &Map<String, Value>, name: &str) -> Result<String, String> {
    let value = arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} must be a string"))?;
    if value.is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    if value.len() > MAX_ARGUMENT_TEXT {
        return Err(format!("{name} exceeds the {MAX_ARGUMENT_TEXT}-byte limit"));
    }
    if value.contains('\0') {
        return Err(format!("{name} contains a NUL byte"));
    }
    Ok(value.to_string())
}

fn optional_string(arguments: &Map<String, Value>, name: &str) -> Result<Option<String>, String> {
    match arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(_)) => required_string(arguments, name).map(Some),
        Some(_) => Err(format!("{name} must be a string")),
    }
}

fn optional_bool(arguments: &Map<String, Value>, name: &str) -> Result<Option<bool>, String> {
    match arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("{name} must be a boolean")),
    }
}

fn optional_u32(arguments: &Map<String, Value>, name: &str) -> Result<Option<u32>, String> {
    match arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| format!("{name} must be a non-negative 32-bit integer")),
        Some(_) => Err(format!("{name} must be a non-negative integer")),
    }
}

fn optional_filter(arguments: &Map<String, Value>) -> Result<String, String> {
    let filter = optional_string(arguments, "filter")?.unwrap_or_else(|| "all".to_string());
    if matches!(
        filter.as_str(),
        "all" | "user" | "system" | "enabled" | "disabled" | "archived" | "retained"
    ) {
        Ok(filter)
    } else {
        Err(format!("unknown filter {filter:?}"))
    }
}

fn optional_direction(arguments: &Map<String, Value>) -> Result<String, String> {
    let direction =
        optional_string(arguments, "direction")?.unwrap_or_else(|| "restore".to_string());
    if matches!(direction.as_str(), "restore" | "reapply") {
        Ok(direction)
    } else {
        Err(format!(
            "unknown direction {direction:?}; pass restore or reapply"
        ))
    }
}

fn is_confirmed(arguments: &Map<String, Value>) -> bool {
    matches!(arguments.get("confirmed"), Some(Value::Bool(true)))
}

fn append_bool_flag(
    command: &mut Vec<String>,
    arguments: &Map<String, Value>,
    argument: &str,
    flag: &str,
) {
    if is_true(arguments, argument) {
        command.push(flag.to_string());
    }
}

fn is_true(arguments: &Map<String, Value>, name: &str) -> bool {
    matches!(arguments.get(name), Some(Value::Bool(true)))
}

fn confirmation_required(tool_name: &str) -> Value {
    failure_result(
        "confirmation_required",
        format!("{tool_name} is mutating and requires confirmed=true after reviewing its plan"),
        None,
    )
}

fn run_cli_tool(command: Vec<String>) -> Value {
    match run_cli(&command) {
        Ok(run) if run.success => success_result(run.value),
        Ok(run) => {
            let CliRun {
                value, exit_code, ..
            } = run;
            let code = value
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("cli_failed")
                .to_string();
            failure_result(
                code,
                format!("droidsmith-cli exited with code {exit_code}"),
                Some(value),
            )
        }
        Err((code, message)) => failure_result(code, message, None),
    }
}

fn run_cli(command: &[String]) -> Result<CliRun, (String, String)> {
    let executable = cli_path()?;
    let output = Command::new(&executable)
        .args(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            (
                "cli_spawn_failed".to_string(),
                format!("could not run {}: {error}", executable.display()),
            )
        })?;
    let exit_code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let value = parse_json_output(&stdout)
        .or_else(|| parse_json_output(&stderr))
        .unwrap_or_else(|| {
            json!({
                "command": "droidsmith-cli",
                "exit_code": exit_code,
                "message": trim_error_text(if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() }),
            })
        });
    Ok(CliRun {
        value,
        success: output.status.success(),
        exit_code,
    })
}

fn cli_path() -> Result<std::path::PathBuf, (String, String)> {
    if let Ok(path) = std::env::var("DROIDSMITH_CLI") {
        if !path.is_empty() {
            return Ok(path.into());
        }
    }
    let current = std::env::current_exe().map_err(|error| {
        (
            "cli_path_failed".to_string(),
            format!("could not locate droidsmith-mcp executable: {error}"),
        )
    })?;
    let parent = current.parent().ok_or_else(|| {
        (
            "cli_path_failed".to_string(),
            "droidsmith-mcp executable has no parent directory".to_string(),
        )
    })?;
    let name = if cfg!(windows) {
        "droidsmith-cli.exe"
    } else {
        "droidsmith-cli"
    };
    Ok(parent.join(name))
}

fn parse_json_output(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        serde_json::from_str(trimmed).ok()
    }
}

fn trim_error_text(text: &str) -> String {
    if text.chars().count() <= MAX_CLI_ERROR_TEXT {
        return text.to_string();
    }
    let mut trimmed = text.chars().take(MAX_CLI_ERROR_TEXT).collect::<String>();
    trimmed.push('…');
    trimmed
}

fn success_result(value: Value) -> Value {
    result_with_content(value, false)
}

fn failure_result(
    code: impl Into<String>,
    message: impl Into<String>,
    data: Option<Value>,
) -> Value {
    let mut structured = Map::new();
    structured.insert(
        "error".to_string(),
        json!({
            "code": code.into(),
            "message": message.into(),
        }),
    );
    if let Some(data) = data {
        structured.insert("data".to_string(), data);
    }
    result_with_content(Value::Object(structured), true)
}

fn result_with_content(value: Value, is_error: bool) -> Value {
    let structured = if value.is_object() {
        value
    } else {
        json!({ "value": value })
    };
    let text = serde_json::to_string_pretty(&structured).expect("MCP result is serializable");
    let mut result = Map::new();
    result.insert(
        "content".to_string(),
        json!([{ "type": "text", "text": text }]),
    );
    result.insert("structuredContent".to_string(), structured);
    if is_error {
        result.insert("isError".to_string(), Value::Bool(true));
    }
    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: Option<u64>, method: &str, params: Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: id.map(Value::from),
            method: method.to_string(),
            params,
        }
    }

    #[test]
    fn initialize_and_tools_list_use_mcp_json_rpc_shapes() {
        let mut state = ServerState::default();
        let initialized = dispatch(&mut state, request(Some(1), "initialize", json!({})))
            .expect("initialize response");
        assert_eq!(
            initialized["result"]["protocolVersion"],
            MCP_PROTOCOL_VERSION
        );
        assert_eq!(
            initialized["result"]["serverInfo"]["name"],
            "droidsmith-mcp"
        );

        let listed = dispatch(&mut state, request(Some(2), "tools/list", json!({})))
            .expect("tools/list response");
        let tools = listed["result"]["tools"].as_array().expect("tool list");
        assert!(tools.iter().any(|tool| tool["name"] == "packages_list"));
        let apply = tools
            .iter()
            .find(|tool| tool["name"] == "profile_apply")
            .expect("profile_apply tool");
        assert_eq!(apply["annotations"]["readOnlyHint"], false);
        assert_eq!(apply["annotations"]["destructiveHint"], true);
        assert!(apply["description"]
            .as_str()
            .expect("description")
            .contains("confirmed=true"));
    }

    #[test]
    fn initialized_notification_has_no_response() {
        let mut state = ServerState::default();
        assert!(dispatch(
            &mut state,
            request(None, "notifications/initialized", json!({})),
        )
        .is_none());
        assert!(state.initialized);
    }

    #[test]
    fn mutating_tools_refuse_without_confirmation_before_spawn() {
        let cases = [
            (
                "profile_apply",
                Map::from_iter([
                    (
                        "profile_path".to_string(),
                        Value::String("profile.yaml".to_string()),
                    ),
                    ("device".to_string(), Value::String("QA1".to_string())),
                ]),
            ),
            (
                "baseline_apply",
                Map::from_iter([
                    (
                        "baseline_path".to_string(),
                        Value::String("baseline.json".to_string()),
                    ),
                    ("device".to_string(), Value::String("QA1".to_string())),
                ]),
            ),
            (
                "pack_apply",
                Map::from_iter([
                    ("pack".to_string(), Value::String("pixel-stock".to_string())),
                    ("device".to_string(), Value::String("QA1".to_string())),
                ]),
            ),
        ];
        for (name, arguments) in cases {
            let response = execute_tool(name, &arguments);
            assert_eq!(response["isError"], true, "tool {name}");
            assert_eq!(
                response["structuredContent"]["error"]["code"], "confirmation_required",
                "tool {name}"
            );
        }
    }

    #[test]
    fn selectors_reject_ambiguous_or_invalid_devices() {
        let both = Map::from_iter([
            ("device".to_string(), Value::String("QA1".to_string())),
            ("all_devices".to_string(), Value::Bool(true)),
        ]);
        assert!(selector(&both).is_err());
        let invalid = Map::from_iter([(
            "device".to_string(),
            Value::String("../not-a-device".to_string()),
        )]);
        assert!(selector(&invalid).is_err());
    }
}
