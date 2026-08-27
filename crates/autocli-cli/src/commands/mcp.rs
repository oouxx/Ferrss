//! MCP (Model Context Protocol) server over stdio, using the "three meta-tool"
//! progressive-discovery pattern.
//!
//! Instead of dumping all ~300 adapter commands into `tools/list` (which would
//! burn tens of thousands of tokens in context), we expose only three meta-tools:
//!
//!   searchTools(query)          -> BM25 keyword search; returns names + short
//!                                  descriptions only (no full schema)
//!   getToolDefinition(name)     -> lazily fetch the full JSON schema for one tool
//!   useTool(name, arguments)    -> execute a tool
//!
//! An agent discovers tools progressively: search -> inspect schema -> call.
//! Implements JSON-RPC 2.0 (initialize, tools/list, tools/call, ping, shutdown)
//! over newline-delimited stdio.

use autocli_core::{ArgType, CliError, CliCommand, Registry};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "autocli";

// ── Tool index ───────────────────────────────────────────────────

/// A searchable entry for one adapter command.
struct ToolEntry {
    tool_name: String, // "site_cmd"
    site: String,
    cmd_name: String,
    description: String,
    doc: String, // lowercased searchable text
}

fn tool_entries(registry: &Registry) -> Vec<ToolEntry> {
    let mut entries = Vec::new();
    for site in registry.list_sites() {
        for cmd in registry.list_commands(site) {
            let tool_name = format!("{}_{}", site, cmd.name);
            let doc = format!(
                "{} {} {} {}",
                site,
                cmd.name,
                tool_name,
                cmd.description
            )
            .to_lowercase();
            entries.push(ToolEntry {
                tool_name,
                site: site.to_string(),
                cmd_name: cmd.name.clone(),
                description: cmd.description.clone(),
                doc,
            });
        }
    }
    entries
}

fn find_tool<'a>(registry: &'a Registry, name: &str) -> Option<(&'a str, &'a CliCommand)> {
    for site in registry.list_sites() {
        for cmd in registry.list_commands(site) {
            if format!("{}_{}", site, cmd.name) == name {
                return Some((site, cmd));
            }
        }
    }
    None
}

// ── JSON Schema helpers ─────────────────────────────────────────

fn schema_type(t: ArgType) -> &'static str {
    match t {
        ArgType::Str => "string",
        ArgType::Int => "integer",
        ArgType::Number => "number",
        ArgType::Bool | ArgType::Boolean => "boolean",
    }
}

/// Full tool definition (description + inputSchema) for one command.
fn tool_definition(site: &str, cmd: &CliCommand) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required: Vec<Value> = Vec::new();
    for arg in &cmd.args {
        let mut prop = json!({ "type": schema_type(arg.arg_type) });
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
    json!({
        "name": format!("{}_{}", site, cmd.name),
        "description": cmd.description,
        "site": site,
        "command": cmd.name,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        },
    })
}

/// The three meta-tools exposed in tools/list.
fn build_meta_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "searchTools",
            "description": "Search the available autocli tools by keyword. Returns matching tool names and short descriptions (NOT full schemas). Use this first to discover what tools exist.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search keywords, e.g. 'bilibili hot' or 'stock finance'" },
                    "limit": { "type": "integer", "description": "Max results (default 20)", "default": 20 }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "getToolDefinition",
            "description": "Get the full JSON schema / definition for a specific tool by name (e.g. 'jiuyangongshe_hot'). Call this after searchTools to see a tool's required arguments.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Tool name, e.g. 'jiuyangongshe_hot'" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "useTool",
            "description": "Execute an autocli tool by name with arguments. Call getToolDefinition first to learn the argument schema.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Tool name, e.g. 'jiuyangongshe_hot'" },
                    "arguments": { "type": "object", "description": "Arguments for the tool (see getToolDefinition)", "default": {} }
                },
                "required": ["name"]
            }
        }),
    ]
}

// ── BM25 search ─────────────────────────────────────────────────

fn tokenize(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// BM25 keyword search over tool names/descriptions. Returns [{name, description}].
fn bm25_search(entries: &[ToolEntry], query: &str, top_n: usize) -> Vec<Value> {
    if entries.is_empty() {
        return vec![];
    }
    let n = entries.len() as f64;
    let avgdl = entries.iter().map(|e| e.doc.split_whitespace().count() as f64).sum::<f64>() / n;
    let terms = tokenize(query);
    if terms.is_empty() {
        return vec![];
    }

    let k1 = 1.2f64;
    let b = 0.75f64;

    let mut scored: Vec<(f64, &ToolEntry)> = Vec::new();
    for e in entries {
        let mut score = 0.0f64;
        for t in &terms {
            let f = e.doc.split_whitespace().filter(|w| *w == t.as_str()).count() as f64;
            if f == 0.0 {
                continue;
            }
            let n_t = entries
                .iter()
                .filter(|x| x.doc.split_whitespace().any(|w| w == t.as_str()))
                .count()
                .max(1) as f64;
            let idf = (1.0 + (n - n_t + 0.5) / (n_t + 0.5)).ln();
            let dl = e.doc.split_whitespace().count() as f64;
            let tf = (f * (k1 + 1.0)) / (f + k1 * (1.0 - b + b * dl / avgdl));
            score += idf * tf;
        }
        if score > 0.0 {
            scored.push((score, e));
        }
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_n);
    scored
        .into_iter()
        .map(|(_, e)| json!({ "name": e.tool_name, "description": e.description }))
        .collect()
}

// ── Tool execution ──────────────────────────────────────────────

/// Coerce a client-provided JSON value to the command's declared arg type.
fn coerce_value(name: &str, v: Value, t: ArgType) -> Result<Value, String> {
    match t {
        ArgType::Int => match &v {
            Value::Number(_) => Ok(v),
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
        ArgType::Str => Ok(v),
    }
}

async fn call_tool(registry: &Registry, name: &str, arguments: &Value) -> Result<Value, Value> {
    let (site, cmd) = find_tool(registry, name).ok_or_else(|| {
        json!({ "content": [{ "type": "text", "text": format!("Unknown tool: {}", name) }], "isError": true })
    })?;

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

// ── JSON-RPC dispatch ───────────────────────────────────────────

async fn handle_message(msg: &Value, registry: &Registry) -> Result<Option<Value>, CliError> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let is_notification = id.is_none() || method.starts_with("notifications/");

    let result = match method {
        "initialize" => json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": build_meta_tools() }),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match name {
                "searchTools" => {
                    let query = arguments.get("query").and_then(|q| q.as_str()).unwrap_or("").to_string();
                    let limit = arguments.get("limit").and_then(|l| l.as_u64()).unwrap_or(20) as usize;
                    let entries = tool_entries(registry);
                    let results = bm25_search(&entries, &query, limit);
                    let text = serde_json::to_string(&results).map_err(|e| CliError::command_execution(e.to_string()))?;
                    json!({ "content": [{ "type": "text", "text": text }], "isError": false })
                }
                "getToolDefinition" => {
                    let tool = arguments.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    match find_tool(registry, tool) {
                        Some((site, cmd)) => {
                            let def = tool_definition(site, cmd);
                            let text = serde_json::to_string(&def).map_err(|e| CliError::command_execution(e.to_string()))?;
                            json!({ "content": [{ "type": "text", "text": text }], "isError": false })
                        }
                        None => {
                            return Ok(Some(json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": { "content": [{ "type": "text", "text": format!("Unknown tool: {}", tool) }], "isError": true },
                            })));
                        }
                    }
                }
                "useTool" => {
                    let tool = arguments.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let tool_args = arguments.get("arguments").cloned().unwrap_or_else(|| json!({}));
                    match call_tool(registry, tool, &tool_args).await {
                        Ok(res) => res,
                        Err(err) => return Ok(Some(err)),
                    }
                }
                _ => {
                    return Ok(Some(json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "content": [{ "type": "text", "text": format!("Unknown tool: {}", name) }], "isError": true },
                    })));
                }
            }
        }
        "shutdown" => json!({}),
        _ => {
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

// ── stdio loop ──────────────────────────────────────────────────

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
