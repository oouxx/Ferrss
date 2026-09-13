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
//!
//! Commands that drive the browser extension are serialised behind a queue:
//! the extension owns a single Chrome automation window, so two concurrent
//! browser commands would clobber each other's tab. Plain-HTTP commands are
//! never gated and keep running in parallel.

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
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
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;

use crate::args::coerce_and_validate_args;
use crate::execution::{env_compat, execute_command_in};

#[derive(Clone)]
struct AppState {
    registry: Arc<Registry>,
    browser_gate: Arc<BrowserGate>,
}

/// How many requests may wait for a browser slot before load is shed.
const BROWSER_QUEUE_CAPACITY: usize = 8;

/// Concurrent browser commands used when nothing else is configured.
const BROWSER_SLOTS_DEFAULT: usize = 2;

/// Commands per site used when nothing else is configured.
const PER_SITE_CONCURRENCY_DEFAULT: usize = 1;

/// How long a request may wait for a browser slot. Kept below the timeout of
/// any reverse proxy in front so callers get a clean 503 instead of a cut
/// connection.
const BROWSER_WAIT_TIMEOUT: Duration = Duration::from_secs(240);

/// Hands out named browser slots.
///
/// Every browser command runs in its own slot, and every slot is a distinct
/// extension workspace: the extension keeps one automation window (and tab) per
/// workspace, so slots execute in parallel instead of fighting over a single
/// tab. Requests beyond the slot count wait in a bounded queue; commands that
/// only use plain HTTP are never gated.
struct BrowserGate {
    /// One permit per slot.
    slots: Semaphore,
    /// Bounds how many requests may be waiting for a slot.
    queue: Semaphore,
    /// Idle slot names, one per permit.
    free_names: Mutex<Vec<String>>,
    /// Per-site limit, so one site is not hammered in parallel.
    per_site: Mutex<HashMap<String, Arc<Semaphore>>>,
    per_site_limit: usize,
    total_slots: usize,
}

impl BrowserGate {
    fn new(slots: usize, per_site_limit: usize) -> Self {
        let slots = slots.max(1);
        Self {
            slots: Semaphore::new(slots),
            queue: Semaphore::new(BROWSER_QUEUE_CAPACITY),
            free_names: Mutex::new((0..slots).map(|i| format!("serve-{i}")).collect()),
            per_site: Mutex::new(HashMap::new()),
            per_site_limit: per_site_limit.max(1),
            total_slots: slots,
        }
    }

    /// Resolve the slot count from the arguments, then the environment, then
    /// the defaults.
    fn from_env(slots: Option<usize>, per_site_limit: Option<usize>) -> Self {
        let slots = slots
            .or_else(|| env_compat("FERRSS_SERVE_BROWSER_SLOTS").and_then(|v| v.parse().ok()))
            .unwrap_or(BROWSER_SLOTS_DEFAULT);
        let per_site_limit = per_site_limit
            .or_else(|| {
                env_compat("FERRSS_SERVE_PER_SITE_CONCURRENCY").and_then(|v| v.parse().ok())
            })
            .unwrap_or(PER_SITE_CONCURRENCY_DEFAULT);
        Self::new(slots, per_site_limit)
    }

    fn total_slots(&self) -> usize {
        self.total_slots
    }

    fn slots_in_use(&self) -> usize {
        self.total_slots - self.slots.available_permits()
    }

    fn queued(&self) -> usize {
        BROWSER_QUEUE_CAPACITY - self.queue.available_permits()
    }

    async fn acquire(&self, site: &str) -> Result<BrowserSlot<'_>, GateError> {
        // Join the bounded wait queue first so a burst cannot pile up.
        let queued = self.queue.try_acquire().map_err(|_| GateError::QueueFull)?;

        let running = match tokio::time::timeout(BROWSER_WAIT_TIMEOUT, self.slots.acquire()).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => return Err(GateError::Closed),
            Err(_) => return Err(GateError::Timeout),
        };

        // Serialise per site: many readers can request the same feed at once,
        // and upstreams react badly to parallel hits from one session.
        let site_semaphore = {
            let mut map = self.per_site.lock().expect("per-site map poisoned");
            map.entry(site.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(self.per_site_limit)))
                .clone()
        };
        let site_permit = site_semaphore
            .acquire_owned()
            .await
            .map_err(|_| GateError::Closed)?;

        // Holding a slot guarantees a free name; take it last so an early
        // return cannot leak the name.
        let name = self
            .free_names
            .lock()
            .expect("slot name list poisoned")
            .pop()
            .unwrap_or_else(|| "serve-?".to_string());

        Ok(BrowserSlot {
            gate: self,
            _queued: queued,
            _running: running,
            _site: site_permit,
            name,
        })
    }
}

/// Holds a slot's permits and its workspace name; everything is released and
/// the name returned to the pool when dropped.
struct BrowserSlot<'a> {
    gate: &'a BrowserGate,
    _queued: tokio::sync::SemaphorePermit<'a>,
    _running: tokio::sync::SemaphorePermit<'a>,
    _site: tokio::sync::OwnedSemaphorePermit,
    name: String,
}

impl BrowserSlot<'_> {
    /// Workspace name for this slot (`serve-0`, `serve-1`, ...).
    fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for BrowserSlot<'_> {
    fn drop(&mut self) {
        if let Ok(mut names) = self.gate.free_names.lock() {
            names.push(std::mem::take(&mut self.name));
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum GateError {
    QueueFull,
    Timeout,
    Closed,
}

/// Result of running a command through `execute_with_params`.
enum ExecuteError {
    Command(CliError),
    BrowserBusy(GateError),
}

/// Run the REST/RSS server. Blocks until the process is terminated.
pub async fn run(
    registry: Registry,
    host: String,
    port: u16,
    browser_slots: Option<usize>,
    per_site_concurrency: Option<usize>,
) -> Result<(), CliError> {
    let browser_gate = BrowserGate::from_env(browser_slots, per_site_concurrency);
    let state = AppState {
        registry: Arc::new(registry),
        browser_gate: Arc::new(browser_gate),
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
    eprintln!(
        "  browser: {} concurrent slot(s), {} command(s) per site, {} queue",
        state.browser_gate.total_slots(),
        state.browser_gate.per_site_limit,
        BROWSER_QUEUE_CAPACITY
    );
    eprintln!("  JSON: http://{addr}/api/run/<site>/<command>");
    eprintln!("  RSS:  http://{addr}/rss/<site>/<command>");

    axum::serve(listener, app)
        .await
        .map_err(|e| CliError::command_execution(format!("Server error: {e}")))?;
    Ok(())
}

// ── Handlers ────────────────────────────────────────────────────

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let gate = &state.browser_gate;
    Json(json!({
        "status": "ok",
        "name": "ferrss",
        "version": env!("CARGO_PKG_VERSION"),
        "browser_slots": {
            "total": gate.total_slots(),
            "in_use": gate.slots_in_use(),
            "queued": gate.queued(),
            "per_site": gate.per_site_limit,
        },
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

    let data = match execute_with_params(&state, cmd, &params).await {
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

    let data = match execute_with_params(&state, cmd, &params).await {
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
    state: &AppState,
    cmd: &CliCommand,
    params: &HashMap<String, String>,
) -> Result<Value, ExecuteError> {
    // Browser-driven commands must not overlap: the Chrome extension owns a
    // single automation window, so concurrent requests clobber each other's
    // tab. Wait for the slot instead of failing; nothing is held for
    // HTTP-only commands.
    let browser_slot = if cmd.needs_browser() {
        match state.browser_gate.acquire(&cmd.site).await {
            Ok(slot) => {
                tracing::debug!(
                    command = %cmd.full_name(),
                    slot = slot.name(),
                    "acquired browser slot"
                );
                Some(slot)
            }
            Err(e) => {
                tracing::warn!(command = %cmd.full_name(), reason = ?e, "browser slot unavailable");
                return Err(ExecuteError::BrowserBusy(e));
            }
        }
    } else {
        None
    };

    // Only pass through query params that the command actually declares.
    let mut raw: HashMap<String, String> = HashMap::new();
    for arg in &cmd.args {
        if let Some(v) = params.get(&arg.name) {
            raw.insert(arg.name.clone(), v.clone());
        }
    }
    let kwargs = coerce_and_validate_args(&cmd.args, &raw).map_err(ExecuteError::Command)?;

    // Each slot is its own extension workspace, i.e. its own automation window
    // and tab, which is what lets browser commands run in parallel.
    let workspace = browser_slot.as_ref().map(|slot| slot.name().to_string());
    execute_command_in(cmd, kwargs, workspace)
        .await
        .map_err(ExecuteError::Command)
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

fn execute_error_response(err: ExecuteError) -> Response {
    match err {
        ExecuteError::Command(err) => error_response(StatusCode::BAD_GATEWAY, &err.to_string()),
        ExecuteError::BrowserBusy(reason) => {
            let message = match reason {
                GateError::QueueFull => {
                    "Too many browser commands are already queued; retry shortly"
                }
                GateError::Timeout => {
                    "Timed out waiting for the browser: another browser command is still running"
                }
                GateError::Closed => "Browser queue is unavailable",
            };
            let mut response = error_response(StatusCode::SERVICE_UNAVAILABLE, message);
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("10"));
            response
        }
    }
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
            .filter(|s| !s.trim().is_empty());

        // An item's link must be its own URL. Reusing the channel link here
        // makes every entry look like the same story to a reader, so when the
        // payload has no URL we emit no item link at all.
        if let Some(url) = &link_val {
            out.push_str(&format!("    <link>{}</link>\n", escape_xml(url)));
        }

        let guid = first_of(item, GUID_KEYS)
            .and_then(value_to_string)
            .filter(|s| !s.trim().is_empty())
            .or_else(|| link_val.clone())
            .unwrap_or_else(|| {
                // Payloads with neither id nor URL (e.g. wikipedia trending)
                // still need a stable identity: derive it from the content
                // rather than the position, so re-ordering a feed does not
                // make readers show every entry as new.
                format!(
                    "ferrss:{}:{}:{:016x}",
                    cmd.site,
                    cmd.name,
                    stable_hash(&format!("{title}\u{1f}{body}"))
                )
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

/// Deterministic 64-bit FNV-1a hash.
///
/// Used to derive stable item ids from content. `std::collections::hash_map`
/// seeds its hasher per process, and the standard library makes no stability
/// guarantee for `DefaultHasher`, so a tiny fixed hash keeps feed ids stable
/// across restarts (and avoids a new dependency).
fn stable_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

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

#[cfg(test)]
mod browser_gate_tests {
    use super::*;

    #[tokio::test]
    async fn slots_run_in_parallel() {
        let gate = BrowserGate::new(2, 1);

        let first = gate.acquire("a").await.expect("slot 1");
        let second = gate.acquire("b").await.expect("slot 2");
        assert_eq!(gate.slots_in_use(), 2);
        assert_ne!(first.name(), second.name(), "each slot needs its own workspace");

        // Both slots are taken: the next request waits instead of failing.
        let waiting = tokio::time::timeout(Duration::from_millis(50), gate.acquire("c")).await;
        assert!(waiting.is_err(), "third acquire must wait for a free slot");

        drop(first);
        assert_eq!(gate.slots_in_use(), 1);
        let third = tokio::time::timeout(Duration::from_secs(1), gate.acquire("c")).await;
        assert!(third.is_ok(), "slot must be reusable after release");
    }

    #[tokio::test]
    async fn slot_names_are_reused_not_leaked() {
        let gate = BrowserGate::new(2, 1);
        for _ in 0..5 {
            let a = gate.acquire("a").await.expect("slot");
            let b = gate.acquire("b").await.expect("slot");
            assert_eq!(gate.free_names.lock().unwrap().len(), 0);
            drop(a);
            drop(b);
            assert_eq!(gate.free_names.lock().unwrap().len(), 2, "names must return to the pool");
        }
    }

    #[tokio::test]
    async fn same_site_is_serialised() {
        let gate = BrowserGate::new(2, 1);

        let held = gate.acquire("bilibili").await.expect("slot");
        // A second slot is free, but the same site must still wait.
        let waiting =
            tokio::time::timeout(Duration::from_millis(50), gate.acquire("bilibili")).await;
        assert!(waiting.is_err(), "same site must not run twice at once");

        // A different site is unaffected.
        let other = tokio::time::timeout(Duration::from_millis(200), gate.acquire("zhihu")).await;
        assert!(other.is_ok(), "other sites must keep running in parallel");

        drop(held);
        let after =
            tokio::time::timeout(Duration::from_secs(1), gate.acquire("bilibili")).await;
        assert!(after.is_ok(), "site must be usable again once the holder finishes");
    }

    #[tokio::test]
    async fn per_site_limit_is_configurable() {
        let gate = BrowserGate::new(3, 2);
        let a = gate.acquire("v2ex").await.expect("slot");
        let b = gate.acquire("v2ex").await.expect("second slot for the same site");
        let waiting = tokio::time::timeout(Duration::from_millis(50), gate.acquire("v2ex")).await;
        assert!(waiting.is_err(), "limit of 2 must block the third");
        drop(a);
        drop(b);
    }

    #[tokio::test]
    async fn shed_load_when_wait_queue_is_full() {
        let gate = Arc::new(BrowserGate::new(1, 1));
        let held = gate.acquire("a").await.expect("holds the only slot");

        let mut waiters = Vec::new();
        for _ in 0..BROWSER_QUEUE_CAPACITY {
            let gate = gate.clone();
            waiters.push(tokio::spawn(async move {
                let slot = gate.acquire("site").await;
                std::future::pending::<()>().await;
                drop(slot);
            }));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;

        match gate.acquire("b").await {
            Err(GateError::QueueFull) => {}
            Err(other) => panic!("expected QueueFull, got {other:?}"),
            Ok(_) => panic!("expected QueueFull, got a slot"),
        }

        drop(held);
        for waiter in waiters {
            waiter.abort();
        }
    }

    #[tokio::test]
    async fn defaults_are_sane() {
        let gate = BrowserGate::new(0, 0);
        assert_eq!(gate.total_slots(), 1, "slot count must be at least 1");
        assert_eq!(gate.per_site_limit, 1, "per-site limit must be at least 1");
        assert_eq!(gate.slots_in_use(), 0);
        assert_eq!(gate.queued(), 0);
    }
}

#[cfg(test)]
mod rss_item_identity_tests {
    use super::*;
    use autocli_core::Strategy;

    fn cmd(site: &str, name: &str) -> CliCommand {
        CliCommand {
            site: site.to_string(),
            name: name.to_string(),
            description: "test feed".to_string(),
            domain: Some("example.com".to_string()),
            strategy: Strategy::default(),
            browser: false,
            args: Vec::new(),
            columns: Vec::new(),
            pipeline: None,
            func: None,
            timeout_seconds: None,
            navigate_before: Default::default(),
        }
    }

    /// Guids, in document order.
    fn guids(rss: &str) -> Vec<String> {
        rss.split("<guid isPermaLink=\"false\">")
            .skip(1)
            .filter_map(|chunk| chunk.split("</guid>").next())
            .map(str::to_string)
            .collect()
    }

    /// Per-item links, in document order (the channel link is not included).
    fn item_links(rss: &str) -> Vec<String> {
        rss.split("<item>")
            .skip(1)
            .filter_map(|chunk| {
                chunk
                    .split("<link>")
                    .nth(1)
                    .and_then(|rest| rest.split("</link>").next())
                    .map(str::to_string)
            })
            .collect()
    }

    #[test]
    fn items_without_urls_do_not_reuse_the_channel_link() {
        let data = json!([
            {"title": "A", "views": 1},
            {"title": "B", "views": 2},
        ]);
        let rss = build_rss(&cmd("wikipedia", "trending"), &data);

        // Only the channel carries a <link>; no item repeats it.
        assert_eq!(rss.matches("<link>").count(), 1, "rss was: {rss}");
        assert!(rss.contains("<link>https://example.com</link>"));
        assert!(item_links(&rss).is_empty(), "rss was: {rss}");
    }

    #[test]
    fn items_without_ids_get_unique_stable_guids() {
        let c = cmd("wikipedia", "trending");
        let data = json!([
            {"title": "A", "views": 1},
            {"title": "B", "views": 2},
        ]);
        let ids = guids(&build_rss(&c, &data));
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);

        // Refetching the same items in a different order must keep the same
        // ids, otherwise readers show everything as new again.
        let reordered = json!([
            {"title": "B", "views": 2},
            {"title": "A", "views": 1},
        ]);
        let ids_again = guids(&build_rss(&c, &reordered));
        assert_eq!(ids[0], ids_again[1]);
        assert_eq!(ids[1], ids_again[0]);
    }

    #[test]
    fn items_use_their_own_url() {
        let data = json!([
            {"title": "A", "url": "https://example.com/a"},
            {"title": "B", "url": "https://example.com/b"},
        ]);
        let rss = build_rss(&cmd("hackernews", "best"), &data);
        assert_eq!(
            item_links(&rss),
            vec!["https://example.com/a", "https://example.com/b"]
        );
    }

    #[test]
    fn payload_id_wins_over_derived_id() {
        let data = json!([{"title": "A", "id": "2608.12564"}]);
        let rss = build_rss(&cmd("hf", "top"), &data);
        assert!(rss.contains(">2608.12564<"), "rss was: {rss}");
    }
}
