//! MCP (Model Context Protocol) server over stdio.
//!
//! Exposes every autocli adapter command as an MCP tool, so MCP clients
//! (Claude, Cursor, etc.) can call `autocli` directly. Implements the
//! JSON-RPC 2.0 method set: initialize, tools/list, tools/call, ping,
//! shutdown. Line-delimited JSON on stdin/stdout.

use autocli_core::{ArgType, CliError, Registry};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "autocli";

/// Map an ArgType to a JSON Schema type string.
fn schema_type(t: ArgType) -> &'static str {
    match t {
        ArgType::Str => "string",
        ArgType::Int => "integer",
        ArgType::Number => "number",
        ArgType::Bool | ArgType::Boolean => "boolean",
    }
}

/// Build the list of MCP tool definitions from the adapter registry.
fn build_tools(registry: &Registry) -> Vec<Value> {
    let mut tools = Vec::new();
    for site in registry.list_sites() {
        for cmd in registry.list_commands(site) {
            let name = format!("{}_{}", site, cmd.name);
            let mut properties = serde_json::Map::new();
            let mut required: Vec<Value> = Vec::new();

            for arg in &cmd.args {
                let mut prop = json!({
                    "type": schema_type(arg.arg_type),
                });
                if let Some(desc) = &arg.description {
                    prop["description"] = Value::String(desc.clone());
                }
                if let Some(def) = &arg.default {
                    prop["default"] = def.clone();
                }
                if let Some(choices) = &arg.choices {
                    prop["enum"] = Value::Array(choices.iter().map(|c| Value::String(c.clone())).collect());
                }
                properties.insert(arg.name.clone(), prop);
                if arg.required {
                    required.push(Value::String(arg.name.clone()));
                }
            }

            tools.push(json!({
                "name": name,
                "description": cmd.description,
                "inputSchema": {
                    "type": "object",
                    "properties": properties,
                    "required": required,
                },
            }));
        }
    }
    tools
}

/// Coerce a client-provided JSON value to the command's declared arg type.
fn coerce_value(name: &str, v: Value, t: ArgType) -> Result<Value, String> {
    match t {
        ArgType::Int => match &v {
            Value::Number(n) => Ok(Value::Number(n.as_i64().map(|i| i.into()).unwrap_or_else(|| n.clone()))),
            Value::String(s) => s
                .trim()
                .parse::<i64>()
                .map(|i| Value::Number(i.into()))
                .map_err(|_| format!("argument '{}' must be an integer, got '{}'", name, s)),
            _ => Err(format!("argument '{}' must be an integer", name)),
        },
        ArgType::Number => match &v {
            Value::Number(_) => Ok(v),
            Value::String(s) => s
                .trim()
                .parse::<f64>()
                .map(|f| Value::from(f))
                .map_err(|_| format!("argument '{}' must be a number, got '{}'", name, s)),
            _ => Err(format!("argument '{}' must be a number", name)),
        },
        ArgType::Bool | ArgType::Boolean => match &v {
            Value::Bool(_) => Ok(v),
            Value::String(s) => match s.trim().to_lowercase().as_str() {
                "true" | "1" | "yes" | "y" => Ok(Value::Bool(true)),
                "false" | "0" | "no" | "n" => Ok(Value::Bool(false)),
                _ => Err(format!("argument '{}' must be a boolean, got '{}'", name, s)),
            },
            _ => Err(format!("argument '{}' must be a boolean", name)),
        },
        ArgType::Str => Ok(match v {
            Value::String(s) => Value::String(s),
            other => other,
        }),
    }
}

/// Execute an MCP tools/call against a command in the registry.
async fn call_tool(registry: &Registry, name: &str, arguments: &Value) -> Result<Value, Value> {
    // Find the command whose tool name matches.
    let mut found: Option<(&str, &autocli_core::CliCommand)> = None;
    'outer: for site in registry.list_sites() {
        for cmd in registry.list_commands(site) {
            if format!("{}_{}", site, cmd.name) == name {
                found = Some((site, cmd));
                break 'outer;
            }
        }
    }

    let (site, cmd) = found.ok_or_else(|| {
        json!({ "content": [{ "type": "text", "text": format!("Unknown tool: {}", name) }], "isError": true })
    })?;

    // Build kwargs from arguments, validating required args and coercing types.
    let args_obj = arguments.as_object().cloned().unwrap_or_default();
    let mut kwargs: HashMap<String, Value> = HashMap::new();
    for arg in &cmd.args {
        match args_obj.get(&arg.name) {
            Some(v) => {
                let coerced = coerce_value(&arg.name, v.clone(), arg.arg_type).map_err(|e| {
                    json!({ "content": [{ "type": "text", "text": e }], "isError": true })
                })?;
                kwargs.insert(arg.name.clone(), coerced);
            }
            None => {
                if arg.required {
                    return Err(json!({
                        "content": [{ "type": "text", "text": format!("Missing required argument '{}'", arg.name) }],
                        "isError": true,
                    }));
                }
            }
        }
    }

    match crate::execution::execute_command(cmd, kwargs).await {
        Ok(data) => {
            let text = serde_json::to_string(&data).unwrap_or_else(|_| "null".into());
            Ok(json!({
                "content": [{ "type": "text", "text": text }],
                "isError": false,
            }))
        }
        Err(e) => Err(json!({
            "content": [{ "type": "text", "text": format!("Command '{} {}' failed: {}", site, cmd.name, e) }],
            "isError": true,
        })),
    }
}

/// Handle a single JSON-RPC message, returning an optional response.
/// Returns None for notifications (no response expected).
async fn handle_message(msg: &Value, registry: &Registry) -> Result<Option<Value>, CliError> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    // Notifications never carry an id and get no response.
    let is_notification = id.is_none() || method.starts_with("notifications/");

    let result = match method {
        "initialize" => json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": build_tools(registry) }),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call_tool(registry, name, &arguments).await {
                Ok(res) => res,
                Err(err) => return Ok(Some(err)),
            }
        }
        "shutdown" => json!({}),
        _ => {
            // Unknown method: respond with method-not-found if not a notification.
            if is_notification {
                return Ok(None);
            }
            return Ok(Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Method not found: {}", method) },
            })));
        }
    };

    if is_notification {
        return Ok(None);
    }

    Ok(Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })))
}

/// Run the MCP stdio server loop until EOF.
pub async fn run_mcp(registry: &Registry) -> Result<(), CliError> {
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|e| CliError::command_execution(format!("Failed to read stdin: {}", e)))?;
        if n == 0 {
            break; // EOF — client closed
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("").to_string();
        let should_exit = method == "shutdown";

        if let Some(resp) = handle_message(&msg, registry).await? {
            let out = serde_json::to_string(&resp).map_err(|e| CliError::command_execution(e.to_string()))?;
            stdout.write_all(out.as_bytes()).await.map_err(|e| CliError::command_execution(e.to_string()))?;
            stdout.write_all(b"\n").await.map_err(|e| CliError::command_execution(e.to_string()))?;
            stdout.flush().await.map_err(|e| CliError::command_execution(e.to_string()))?;
        }

        if should_exit {
            break;
        }
    }

    Ok(())
}
