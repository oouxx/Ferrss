//! HTTP server exposing adapter commands as a REST API (JSON) and as RSS feeds.
//!
//! Routes:
//!   GET /                                  -> HTML index of available endpoints
//!   GET /health                            -> {"status":"ok", ...}
//!   GET /api/sites                         -> [{site, commands:[...]}]
//!   GET /api/commands                      -> every command with its argument schema
//!   GET /api/run/{site}/{command}?a=b      -> execute, JSON (or ?format=md|csv|...)
//!   GET /rss/{site}/{command}?a=b          -> execute, RSS 2.0 feed
//!
//! Query parameters are passed through as command arguments using the same
//! coercion/validation path as the CLI, so `?limit=10` works for an `int` arg.

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use autocli_core::{CliCommand, CliError, Registry};
use autocli_output::format::{OutputFormat, RenderOptions};
use autocli_output::render;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::args::coerce_and_validate_args;
use crate::execution::execute_command;

#[derive(Clone)]
struct AppState {
    registry: Arc<Registry>,
}

/// Run the REST/RSS server. Blocks until the process is terminated.
pub async fn run(registry: Registry, host: String, port: u16) -> Result<(), CliError> {
    let state = AppState {
        registry: Arc::new(registry),
    };

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/health", get(health_handler))
        .route("/api/sites", get(sites_handler))
        .route("/api/commands", get(commands_handler))
        .route("/api/sites/{site}", get(site_commands_handler))
        .route("/api/run/{site}/{command}", get(run_handler))
        .route("/rss/{site}/{command}", get(rss_handler))
        .route("/feed/{site}/{command}", get(rss_handler))
        .layer(cors)
        .with_state(state.clone());

    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| CliError::command_execution(format!("Failed to bind {addr}: {e}")))?;

    let sites = state.registry.site_count();
    let commands = state.registry.command_count();
    eprintln!("ferrss serve listening on http://{addr}");
    eprintln!("  {sites} sites, {commands} commands");
    eprintln!("  JSON: http://{addr}/api/run/<site>/<command>");
    eprintln!("  RSS:  http://{addr}/rss/<site>/<command>");

    axum::serve(listener, app)
        .await
        .map_err(|e| CliError::command_execution(format!("Server error: {e}")))?;
    Ok(())
}

// ── Handlers ────────────────────────────────────────────────────

async fn health_handler() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "name": "ferrss",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn index_handler(State(state): State<AppState>) -> Html<String> {
    let mut rows = String::new();
    for site in state.registry.list_sites() {
        for cmd in state.registry.list_commands(site) {
            rows.push_str(&format!(
                "<tr><td><code>{site}</code></td><td><code>{name}</code></td>\
                 <td>{desc}</td>\
                 <td><a href=\"/api/run/{site}/{name}\">json</a> · \
                 <a href=\"/rss/{site}/{name}\">rss</a></td></tr>",
                site = site,
                name = cmd.name,
                desc = escape_html(&cmd.description),
            ));
        }
    }
    Html(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>ferrss API</title>\
         <style>body{{font-family:system-ui,sans-serif;margin:2rem;max-width:1000px}}\
         table{{border-collapse:collapse;width:100%}}td,th{{border:1px solid #ddd;padding:6px 10px;text-align:left}}\
         code{{background:#f4f4f4;padding:1px 4px;border-radius:3px}}</style></head>\
         <body><h1>ferrss API</h1>\
         <p>Version {version} · {sites} sites · {commands} commands</p>\
         <p><a href=\"/api/commands\">/api/commands</a> · <a href=\"/health\">/health</a></p>\
         <table><thead><tr><th>Site</th><th>Command</th><th>Description</th><th>Links</th></tr></thead>\
         <tbody>{rows}</tbody></table></body></html>",
        version = env!("CARGO_PKG_VERSION"),
        sites = state.registry.site_count(),
        commands = state.registry.command_count(),
    ))
}

async fn sites_handler(State(state): State<AppState>) -> impl IntoResponse {
    let sites: Vec<Value> = state
        .registry
        .list_sites()
        .iter()
        .map(|site| {
            let cmds: Vec<Value> = state
                .registry
                .list_commands(site)
                .iter()
                .map(|c| json!({ "name": c.name, "description": c.description }))
                .collect();
            json!({ "site": site, "commands": cmds })
        })
        .collect();
    Json(Value::Array(sites))
}

async fn commands_handler(State(state): State<AppState>) -> impl IntoResponse {
    let cmds: Vec<Value> = state
        .registry
        .all_commands()
        .iter()
        .map(|c| command_schema(c))
        .collect();
    Json(Value::Array(cmds))
}

async fn site_commands_handler(
    State(state): State<AppState>,
    Path(site): Path<String>,
) -> Response {
    if !state.registry.list_sites().contains(&site.as_str()) {
        return error_response(StatusCode::NOT_FOUND, &format!("Unknown site: {site}"));
    }
    let cmds: Vec<Value> = state
        .registry
        .list_commands(&site)
        .iter()
        .map(|c| command_schema(c))
        .collect();
    Json(Value::Array(cmds)).into_response()
}

async fn run_handler(
    State(state): State<AppState>,
    Path((site, command)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let cmd = match state.registry.get(&site, &command) {
        Some(c) => c,
        None => {
            return error_response(
                StatusCode::NOT_FOUND,
                &format!("Unknown command: {site} {command}"),
            )
        }
    };

    let data = match execute_with_params(cmd, &params).await {
        Ok(d) => d,
        Err(e) => return execute_error_response(e),
    };

    let format = params
        .get("format")
        .and_then(|f| OutputFormat::from_str(f).ok())
        .unwrap_or(OutputFormat::Json);

    let opts = RenderOptions {
        format,
        columns: if cmd.columns.is_empty() {
            None
        } else {
            Some(cmd.columns.clone())
        },
        title: None,
        elapsed: None,
        source: Some(cmd.full_name()),
        footer_extra: None,
    };
    let body = render(&data, &opts);

    match format {
        OutputFormat::Json => Json(data).into_response(),
        OutputFormat::Yaml => (
            [(header::CONTENT_TYPE, "application/yaml; charset=utf-8")],
            body,
        )
            .into_response(),
        OutputFormat::Csv => (
            [(header::CONTENT_TYPE, "text/csv; charset=utf-8")],
            body,
        )
            .into_response(),
        OutputFormat::Markdown => (
            [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
            body,
        )
            .into_response(),
        OutputFormat::Table => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body)
            .into_response(),
    }
}

async fn rss_handler(
    State(state): State<AppState>,
    Path((site, command)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let cmd = match state.registry.get(&site, &command) {
        Some(c) => c,
        None => {
            return error_response(
                StatusCode::NOT_FOUND,
                &format!("Unknown command: {site} {command}"),
            )
        }
    };

    let data = match execute_with_params(cmd, &params).await {
        Ok(d) => d,
        Err(e) => return execute_error_response(e),
    };

    let xml = build_rss(cmd, &data);
    (
        [
            (
                header::CONTENT_TYPE,
                "application/rss+xml; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        xml,
    )
        .into_response()
}

// ── Shared helpers ──────────────────────────────────────────────

async fn execute_with_params(
    cmd: &CliCommand,
    params: &HashMap<String, String>,
) -> Result<Value, CliError> {
    // Only pass through query params that the command actually declares.
    let mut raw: HashMap<String, String> = HashMap::new();
    for arg in &cmd.args {
        if let Some(v) = params.get(&arg.name) {
            raw.insert(arg.name.clone(), v.clone());
        }
    }
    let kwargs = coerce_and_validate_args(&cmd.args, &raw)?;
    execute_command(cmd, kwargs).await
}

fn command_schema(cmd: &CliCommand) -> Value {
    let args: Vec<Value> = cmd
        .args
        .iter()
        .map(|a| {
            json!({
                "name": a.name,
                "type": format!("{:?}", a.arg_type).to_lowercase(),
                "required": a.required,
                "description": a.description,
                "default": a.default,
                "choices": a.choices,
            })
        })
        .collect();
    json!({
        "site": cmd.site,
        "command": cmd.name,
        "description": cmd.description,
        "domain": cmd.domain,
        "columns": cmd.columns,
        "args": args,
        "json_url": format!("/api/run/{}/{}", cmd.site, cmd.name),
        "rss_url": format!("/rss/{}/{}", cmd.site, cmd.name),
    })
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "ok": false, "error": message }))).into_response()
}

fn execute_error_response(err: CliError) -> Response {
    error_response(StatusCode::BAD_GATEWAY, &err.to_string())
}

// ── RSS rendering ───────────────────────────────────────────────

fn build_rss(cmd: &CliCommand, data: &Value) -> String {
    let items = extract_items(data);
    let link = cmd
        .domain
        .as_deref()
        .map(|d| format!("https://{d}"))
        .unwrap_or_default();

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">\n<channel>\n");
    out.push_str(&format!(
        "  <title>{}</title>\n",
        escape_xml(&format!("{} · {}", cmd.full_name(), cmd.description))
    ));
    out.push_str(&format!("  <link>{}</link>\n", escape_xml(&link)));
    out.push_str(&format!(
        "  <description>{}</description>\n",
        escape_xml(&cmd.description)
    ));
    out.push_str(&format!(
        "  <lastBuildDate>{}</lastBuildDate>\n",
        now_rfc822()
    ));
    out.push_str(&format!(
        "  <generator>ferrss {}</generator>\n",
        env!("CARGO_PKG_VERSION")
    ));

    for (i, item) in items.iter().enumerate() {
        out.push_str("  <item>\n");
        let (title, body) = item_title_and_body(item, i);
        out.push_str(&format!("    <title>{}</title>\n", escape_xml(&title)));

        let link_val = first_of(item, LINK_KEYS)
            .and_then(value_to_string)
            .unwrap_or_else(|| link.clone());
        if !link_val.is_empty() {
            out.push_str(&format!("    <link>{}</link>\n", escape_xml(&link_val)));
        }

        let guid = first_of(item, GUID_KEYS)
            .and_then(value_to_string)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if link_val.is_empty() {
                    format!("ferrss:{}:{}:{}", cmd.site, cmd.name, i)
                } else {
                    link_val.clone()
                }
            });
        out.push_str(&format!(
            "    <guid isPermaLink=\"false\">{}</guid>\n",
            escape_xml(&guid)
        ));

        out.push_str(&format!(
            "    <description>{}</description>\n",
            escape_xml(&body)
        ));

        if let Some(ts) = first_of(item, DATE_KEYS).and_then(value_to_unix) {
            out.push_str(&format!(
                "    <pubDate>{}</pubDate>\n",
                unix_to_rfc822(ts)
            ));
        }
        out.push_str("  </item>\n");
    }

    out.push_str("</channel>\n</rss>\n");
    out
}

/// Normalize arbitrary command output into a list of RSS items (JSON objects).
fn extract_items(data: &Value) -> Vec<Map<String, Value>> {
    match data {
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_object().cloned())
            .collect(),
        Value::Object(obj) => {
            // Prefer an array-valued field (e.g. {"items": [...]})
            for key in ["items", "data", "results", "list", "entries", "rows"] {
                if let Some(Value::Array(arr)) = obj.get(key) {
                    let items: Vec<Map<String, Value>> =
                        arr.iter().filter_map(|v| v.as_object().cloned()).collect();
                    if !items.is_empty() {
                        return items;
                    }
                }
            }
            vec![obj.clone()]
        }
        _ => vec![],
    }
}

/// Title and description text for one item.
fn item_title_and_body(item: &Map<String, Value>, index: usize) -> (String, String) {
    let title = first_of(item, TITLE_KEYS)
        .and_then(value_to_string)
        .filter(|s| !s.trim().is_empty());

    let body = first_of(item, BODY_KEYS)
        .and_then(value_to_string)
        .filter(|s| !s.trim().is_empty());

    let title = match (title, &body) {
        (Some(t), _) => t,
        (None, Some(b)) => truncate(b, 120),
        (None, None) => format!("Item {}", index + 1),
    };

    // Fall back to a flat "key: value" summary of the remaining fields.
    let body = body.unwrap_or_else(|| summarize(item));
    (title, body)
}

/// Build a human-readable summary from all non-title fields.
fn summarize(item: &Map<String, Value>) -> String {
    let mut parts = Vec::new();
    for (k, v) in item {
        if TITLE_KEYS.contains(&k.as_str()) {
            continue;
        }
        let text = match v {
            Value::Null => continue,
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if text.is_empty() {
            continue;
        }
        parts.push(format!("{k}: {text}"));
        if parts.len() >= 10 {
            break;
        }
    }
    parts.join(" | ")
}

fn first_of<'a>(item: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| item.get(*k))
}

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{truncated}…")
}

const TITLE_KEYS: &[&str] = &[
    "title", "Title", "name", "Name", "headline", "question", "text", "Text", "desc",
    "description",
];
const LINK_KEYS: &[&str] = &[
    "url", "Url", "URL", "link", "Link", "href", "web_url", "permalink", "source_url",
];
const GUID_KEYS: &[&str] = &["id", "Id", "ID", "guid", "uuid"];
const BODY_KEYS: &[&str] = &[
    "description", "Description", "desc", "summary", "Summary", "content", "Content", "text",
    "Text", "detail",
];
const DATE_KEYS: &[&str] = &[
    "pubDate", "pubdate", "pub_date", "published", "published_at", "publishedAt", "created_at",
    "createdAt", "date", "Date", "time", "Time", "timestamp", "updated", "updated_at",
];

// ── XML / HTML escaping ─────────────────────────────────────────

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Strip control chars that are illegal in XML 1.0.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Date handling (no external date crate) ──────────────────────

fn now_rfc822() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64;
    unix_to_rfc822(secs)
}

/// Convert a JSON value (unix seconds/millis, or a date string) to unix seconds.
fn value_to_unix(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => {
            let raw = n.as_f64()?;
            // Heuristic: anything past year ~33658 in seconds is really millis.
            Some(if raw > 1.0e12 {
                (raw / 1000.0) as i64
            } else {
                raw as i64
            })
        }
        Value::String(s) => parse_datetime(s.trim()),
        _ => None,
    }
}

fn parse_datetime(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    // Pure unix timestamp.
    if let Ok(n) = s.parse::<i64>() {
        return Some(if n > 1_000_000_000_000 { n / 1000 } else { n });
    }

    let bytes = s.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let mut secs = days_from_civil(year, month, day) * 86_400;

    // Optional time component.
    if bytes.len() >= 16 && (bytes[10] == b'T' || bytes[10] == b' ') {
        let hh: i64 = s.get(11..13)?.parse().unwrap_or(0);
        let mm: i64 = s.get(14..16)?.parse().unwrap_or(0);
        let ss: i64 = s.get(17..19).and_then(|x| x.parse().ok()).unwrap_or(0);
        secs += hh * 3600 + mm * 60 + ss;

        // Apply a trailing timezone offset if present (e.g. +08:00 / -0500 / Z).
        if let Some(off) = parse_tz_offset(s) {
            secs -= off;
        }
    }
    Some(secs)
}

/// Parse a trailing RFC3339-ish timezone offset in seconds, if any.
fn parse_tz_offset(s: &str) -> Option<i64> {
    let idx = s.rfind(['+', '-'])?;
    // Ignore '-' characters that are part of the date (positions < 10).
    if idx < 10 {
        return None;
    }
    let tz = &s[idx..];
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let digits: String = tz[1..].chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 2 {
        return None;
    }
    let hours: i64 = digits.get(0..2)?.parse().ok()?;
    let minutes: i64 = digits.get(2..4).and_then(|m| m.parse().ok()).unwrap_or(0);
    Some(sign * (hours * 3600 + minutes * 60))
}

fn unix_to_rfc822(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let weekday = ((days + 4).rem_euclid(7)) as usize;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        WEEKDAYS[weekday], day, MONTHS[(month - 1) as usize], year, hh, mm, ss
    )
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of `days_from_civil`.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (y + i64::from(m <= 2), m, d)
}
