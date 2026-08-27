mod args;
mod commands;
mod execution;
mod i18n;

use i18n::t;

use clap::{Arg, ArgAction, Command};
use clap_complete::Shell;
use autocli_core::Registry;
use serde_json::Value;
use autocli_discovery::{discover_builtin_adapters, discover_user_adapters};
use autocli_external::{load_external_clis, ExternalCli};
use autocli_output::format::{OutputFormat, RenderOptions};
use autocli_output::render;
use std::collections::HashMap;
use std::str::FromStr;
use tracing_subscriber::EnvFilter;

use crate::args::coerce_and_validate_args;
use crate::commands::{completion, doctor, read};
use crate::execution::execute_command;

fn build_cli(registry: &Registry, external_clis: &[ExternalCli]) -> Command {
    let mut app = Command::new("autocli")
        .version(env!("CARGO_PKG_VERSION"))
        .about("AI-driven CLI tool — turns websites into command-line interfaces")
        .arg(
            Arg::new("format")
                .long("format")
                .short('f')
                .global(true)
                .default_value("table")
                .help("Output format: table, json, yaml, csv, md"),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Enable verbose output"),
        );

    // Add site subcommands from the adapter registry
    for site in registry.list_sites() {
        let mut site_cmd = Command::new(site.to_string());

        for cmd in registry.list_commands(site) {
            let mut sub = Command::new(cmd.name.clone()).about(cmd.description.clone());

            for arg_def in &cmd.args {
                let mut arg = if arg_def.positional {
                    Arg::new(arg_def.name.clone())
                } else {
                    Arg::new(arg_def.name.clone()).long(arg_def.name.clone())
                };
                if let Some(desc) = &arg_def.description {
                    arg = arg.help(desc.clone());
                }
                if arg_def.required {
                    arg = arg.required(true);
                }
                if let Some(default) = &arg_def.default {
                    // Value::String("x").to_string() produces "\"x\"" (JSON-encoded),
                    // but clap needs the raw string value.
                    let default_str = match default {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    arg = arg.default_value(default_str);
                }
                sub = sub.arg(arg);
            }
            site_cmd = site_cmd.subcommand(sub);
        }
        app = app.subcommand(site_cmd);
    }

    // Add external CLI subcommands
    for ext in external_clis {
        app = app.subcommand(
            Command::new(ext.name.clone())
                .about(ext.description.clone())
                .allow_external_subcommands(true),
        );
    }

    // Built-in utility subcommands
    app = app
        .subcommand(Command::new("doctor").about("Run diagnostics checks"))
        .subcommand(
            Command::new("completion")
                .about("Generate shell completions")
                .arg(
                    Arg::new("shell")
                        .required(true)
                        .value_parser(clap::value_parser!(Shell))
                        .help("Target shell: bash, zsh, fish, powershell"),
                ),
        )
        .subcommand(
            Command::new("explore")
                .about("Explore a website's API surface and discover endpoints")
                .arg(Arg::new("url").required(true).help("URL to explore"))
                .arg(Arg::new("site").long("site").help("Override site name"))
                .arg(Arg::new("goal").long("goal").help("Hint for capability naming (e.g. search, hot)"))
                .arg(Arg::new("wait").long("wait").default_value("3").help("Initial wait seconds"))
                .arg(Arg::new("auto").long("auto").action(ArgAction::SetTrue).help("Enable interactive fuzzing (click buttons/tabs to trigger hidden APIs)"))
                .arg(Arg::new("click").long("click").help("Comma-separated labels to click before fuzzing (e.g. 'Comments,CC,字幕')")),
        )
        .subcommand(
            Command::new("cascade")
                .about("Auto-detect authentication strategy for an API endpoint")
                .arg(Arg::new("url").required(true).help("API endpoint URL to probe")),
        )
        .subcommand(
            Command::new("generate")
                .about("One-shot: explore + synthesize + select best adapter")
                .arg(Arg::new("url").required(true).help("URL to generate adapter for"))
                .arg(Arg::new("goal").long("goal").help("What you want (e.g. hot, search, trending)"))
                .arg(Arg::new("site").long("site").help("Override site name"))
                .arg(Arg::new("ai").long("ai").action(ArgAction::SetTrue).help("Use AI (LLM) to analyze and generate adapter"))
                .arg(Arg::new("provider").long("provider").help("LLM provider name or OpenAI-compatible endpoint URL (overrides ~/.autocli/config.json)"))
                .arg(Arg::new("model").long("model").help("LLM model name (overrides ~/.autocli/config.json)"))
                .arg(Arg::new("api-key").long("api-key").help("LLM API key (overrides ~/.autocli/config.json)")),
        )
        .subcommand(
            Command::new("config-llm")
                .about("Configure a local LLM provider for AI generation")
                .arg(Arg::new("provider").long("provider").help("LLM provider name or OpenAI-compatible endpoint URL"))
                .arg(Arg::new("model").long("model").help("LLM model name"))
                .arg(Arg::new("api-key").long("api-key").help("LLM API key"))
                .arg(Arg::new("show").long("show").action(ArgAction::SetTrue).help("Print current LLM config without changing it")),
        )
        .subcommand(
            Command::new("read")
                .about("Extract main article content from a webpage (Readability)")
                .arg(Arg::new("url").required(true).help("URL to read"))
                .arg(
                    Arg::new("format")
                        .long("format")
                        .short('f')
                        .default_value("markdown")
                        .help("Output format: markdown (default), text, html, json"),
                )
                .arg(
                    Arg::new("output")
                        .long("output")
                        .short('o')
                        .help("Write output to file instead of stdout"),
                ),
        );

    app
}

/// Migrate legacy ~/.opencli-rs directory to ~/.autocli
fn migrate_legacy_config() {
    let home = match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        Ok(h) => h,
        Err(_) => return,
    };
    let old_dir = std::path::PathBuf::from(&home).join(".opencli-rs");
    let new_dir = std::path::PathBuf::from(&home).join(".autocli");

    if !old_dir.exists() {
        return;
    }

    // Copy contents to new directory
    if let Err(e) = copy_dir_recursive(&old_dir, &new_dir) {
        eprintln!("{}{}", t("⚠️  配置迁移失败: ", "⚠️  Config migration failed: "), e);
        return;
    }

    // Remove old directory
    if let Err(e) = std::fs::remove_dir_all(&old_dir) {
        eprintln!("{}{}", t("⚠️  无法删除旧配置目录: ", "⚠️  Cannot remove old config dir: "), e);
        return;
    }

    eprintln!("{}", t(
        "✅ 已将配置从 ~/.opencli-rs 迁移到 ~/.autocli",
        "✅ Migrated config from ~/.opencli-rs to ~/.autocli"
    ));
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if !dst_path.exists() {
            // Don't overwrite existing files in new dir
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Kill the process listening on a given port.
fn kill_process_on_port(port: u16) {
    if cfg!(target_os = "windows") {
        // Windows: use netstat to find PID, then taskkill
        let netstat = std::process::Command::new("netstat")
            .args(["-ano"])
            .output();
        if let Ok(output) = netstat {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains(&format!(":{}", port)) && line.contains("LISTENING") {
                    if let Some(pid) = line.split_whitespace().last() {
                        let _ = std::process::Command::new("taskkill")
                            .args(["/PID", pid, "/F"])
                            .output();
                        tracing::debug!(port, pid, "Killed old daemon (Windows)");
                        return;
                    }
                }
            }
        }
    } else {
        // macOS/Linux: lsof + kill
        let output = std::process::Command::new("sh")
            .args(["-c", &format!("lsof -ti tcp:{} | xargs kill -9 2>/dev/null", port)])
            .output();
        match output {
            Ok(o) => tracing::debug!(port, stdout = %String::from_utf8_lossy(&o.stdout), "Killed old daemon"),
            Err(e) => tracing::warn!(port, error = %e, "Failed to kill old daemon"),
        }
    }
}

fn save_adapter(site: &str, name: &str, yaml: &str) {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let dir = std::path::PathBuf::from(&home)
        .join(".autocli")
        .join("adapters")
        .join(&site);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.yaml", name));
    match std::fs::write(&path, yaml) {
        Ok(_) => {
            eprintln!("{} {} {}", t("✅ 已生成配置:", "✅ Generated adapter:"), site, name);
            eprintln!("   {}{}", t("保存到: ", "Saved to: "), path.display());
            eprintln!();
            eprintln!("   {}", t("运行命令:", "Run it now:"));
            eprintln!("   autocli {} {}", site, name);
        }
        Err(e) => {
            eprintln!("{}{}", t("生成成功但保存失败: ", "Generated adapter but failed to save: "), e);
            eprintln!();
            println!("{}", yaml);
        }
    }
}

fn print_error(err: &autocli_core::CliError) {
    eprintln!("{} {}", err.icon(), err);
    let suggestions = err.suggestions();
    if !suggestions.is_empty() {
        eprintln!();
        for s in suggestions {
            eprintln!("  -> {}", s);
        }
    }
}

#[tokio::main]
async fn main() {
    // 0. Migrate from ~/.opencli-rs to ~/.autocli if needed
    migrate_legacy_config();

    // 1. Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| {
                if std::env::var("AUTOCLI_VERBOSE").is_ok() {
                    EnvFilter::new("debug")
                } else {
                    EnvFilter::new("warn")
                }
            }),
        )
        .init();

    // Check for --daemon flag (used by BrowserBridge to spawn daemon as subprocess)
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--daemon") {
        let port: u16 = std::env::var("AUTOCLI_DAEMON_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(19925);
        tracing::info!(port = port, "Starting daemon server");
        match autocli_browser::Daemon::start(port).await {
            Ok(daemon) => {
                // Wait for shutdown signal (ctrl+c)
                tokio::signal::ctrl_c().await.ok();
                tracing::info!("Shutting down daemon");
                let _ = daemon.shutdown().await;
            }
            Err(e) => {
                eprintln!("Failed to start daemon: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // 1.5. Ensure daemon is running with correct version
    {
        let port: u16 = std::env::var("AUTOCLI_DAEMON_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(19925);
        let current_version = env!("CARGO_PKG_VERSION");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(1))
            .build()
            .ok();

        let mut need_start = true;

        if let Some(c) = &client {
            if let Ok(resp) = c.get(&format!("http://127.0.0.1:{}/ping", port)).send().await {
                if resp.status().is_success() {
                    // Daemon is running — check version
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let daemon_version = body.get("version").and_then(|v| v.as_str()).unwrap_or("");
                        if daemon_version == current_version {
                            need_start = false;
                            tracing::debug!(port, version = daemon_version, "Daemon already running with correct version");
                        } else {
                            tracing::info!(daemon_version, current_version, "Daemon version mismatch, restarting");
                            // Kill old daemon by requesting shutdown, or find and kill process on the port
                            kill_process_on_port(port);
                            // Wait briefly for port to free up
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        }
                    }
                }
            }
        }

        if need_start {
            if let Ok(exe) = std::env::current_exe() {
                let child = tokio::process::Command::new(exe)
                    .arg("--daemon")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                if let Ok(c) = child {
                    std::mem::forget(c);
                    tracing::debug!(port, version = current_version, "Spawned daemon in background");
                }
            }
        }
    }

    // 1.6. Check for updates (only show for non-format output)
    {
        let format_arg = std::env::args().any(|a| a == "--format" || a == "-f");
        if !format_arg {
            let port: u16 = std::env::var("AUTOCLI_DAEMON_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(19925);
            if let Ok(client) = reqwest::Client::builder().timeout(std::time::Duration::from_secs(1)).build() {
                if let Ok(resp) = client.get(&format!("http://127.0.0.1:{}/check-update", port)).send().await {
                    if let Ok(data) = resp.json::<serde_json::Value>().await {
                        if data.get("update_available").and_then(|v| v.as_bool()).unwrap_or(false) {
                            let latest = data.get("latest_version").and_then(|v| v.as_str()).unwrap_or("");
                            let current = data.get("current_version").and_then(|v| v.as_str()).unwrap_or("");
                            let dl = data.get("download_url").and_then(|v| v.as_str()).unwrap_or("");
                            eprintln!("{}", t(
                                &format!("💡 新版本可用: {} (当前: {}) → {}", latest, current, dl),
                                &format!("💡 Update available: {} (current: {}) → {}", latest, current, dl),
                            ));
                            eprintln!();
                        }
                    }
                }
            }
        }
    }

    // 2. Create registry and discover adapters
    let mut registry = Registry::new();

    match discover_builtin_adapters(&mut registry) {
        Ok(n) => tracing::debug!(count = n, "Discovered builtin adapters"),
        Err(e) => tracing::warn!(error = %e, "Failed to discover builtin adapters"),
    }

    match discover_user_adapters(&mut registry) {
        Ok(n) => tracing::debug!(count = n, "Discovered user adapters"),
        Err(e) => tracing::warn!(error = %e, "Failed to discover user adapters"),
    }

    // 3. Load external CLIs
    let external_clis = match load_external_clis() {
        Ok(clis) => {
            tracing::debug!(count = clis.len(), "Loaded external CLIs");
            clis
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load external CLIs");
            vec![]
        }
    };

    // 4. Build clap app with dynamic subcommands
    let app = build_cli(&registry, &external_clis);
    let matches = app.get_matches();

    let format_str = matches.get_one::<String>("format").unwrap().clone();
    let verbose = matches.get_flag("verbose");

    if verbose {
        tracing::info!("Verbose mode enabled");
    }

    let output_format = OutputFormat::from_str(&format_str).unwrap_or_default();

    // 5. Route: find matching site+command or external CLI
    if let Some((site_name, site_matches)) = matches.subcommand() {
        // Handle built-in utility subcommands
        match site_name {
            "doctor" => {
                doctor::run_doctor().await;
                return;
            }
            "read" => {
                let raw_url = site_matches.get_one::<String>("url").unwrap();
                let url = if raw_url.starts_with("http://") || raw_url.starts_with("https://") {
                    raw_url.clone()
                } else {
                    format!("https://{}", raw_url)
                };
                let format_str = site_matches.get_one::<String>("format").unwrap();
                let format = match read::Format::from_str(format_str) {
                    Some(f) => f,
                    None => {
                        eprintln!(
                            "Unknown format '{format_str}'. Valid options: markdown, text, html, json"
                        );
                        std::process::exit(2);
                    }
                };
                let output = site_matches.get_one::<String>("output").map(|s| s.as_str());

                if let Err(e) = read::run(&url, format, output).await {
                    eprintln!("{}: {}", t("获取文章失败", "Failed to read article"), e);
                    std::process::exit(1);
                }
                return;
            }
            "completion" => {
                let shell = site_matches
                    .get_one::<Shell>("shell")
                    .copied()
                    .expect("shell argument required");
                let mut app = build_cli(&registry, &external_clis);
                completion::run_completion(&mut app, shell);
                return;
            }
            "config-llm" => {
                let show = site_matches.get_flag("show");
                let mut config = autocli_ai::load_config();

                if show {
                    eprintln!("{}", t("当前 LLM 配置:", "Current LLM config:"));
                    eprintln!("  endpoint: {}", config.llm.endpoint.as_deref().unwrap_or("(unset)"));
                    eprintln!("  model:    {}", config.llm.modelname.as_deref().unwrap_or("(unset)"));
                    eprintln!("  api-key:  {}", if config.llm.apikey.as_deref().map_or(false, |k| !k.is_empty()) { "***" } else { "(unset)" });
                    return;
                }

                let provider = site_matches.get_one::<String>("provider").cloned();
                let model = site_matches.get_one::<String>("model").cloned();
                let api_key = site_matches.get_one::<String>("api-key").cloned();

                if provider.is_none() && model.is_none() && api_key.is_none() {
                    eprintln!("{}", t(
                        "用法: autocli config-llm --provider <name|url> --model <name> [--api-key <key>]",
                        "Usage: autocli config-llm --provider <name|url> --model <name> [--api-key <key>]"
                    ));
                    eprintln!("{}", t(
                        "provider 可以是名称(openai/deepseek/qwen/moonshot/zhipu/groq/mistral/ollama/lmstudio)或完整 endpoint URL",
                        "provider can be a name (openai/deepseek/qwen/moonshot/zhipu/groq/mistral/ollama/lmstudio) or a full endpoint URL"
                    ));
                    std::process::exit(1);
                }

                if let Some(p) = provider {
                    config.llm.endpoint = Some(autocli_ai::provider_endpoint(&p));
                }
                if let Some(m) = model {
                    config.llm.modelname = Some(m);
                }
                if let Some(k) = api_key {
                    config.llm.apikey = Some(k);
                }

                match autocli_ai::save_config(&config) {
                    Ok(_) => {
                        eprintln!("{}", t("✅ LLM 配置已保存到 ", "✅ LLM config saved to "));
                        eprintln!("  endpoint: {}", config.llm.endpoint.as_deref().unwrap_or("(unset)"));
                        eprintln!("  model:    {}", config.llm.modelname.as_deref().unwrap_or("(unset)"));
                        eprintln!("  api-key:  {}", if config.llm.apikey.as_deref().map_or(false, |k| !k.is_empty()) { "***" } else { "(unset)" });
                    }
                    Err(e) => {
                        eprintln!("{}{}", t("❌ 保存失败: ", "❌ Save failed: "), e);
                        std::process::exit(1);
                    }
                }
                return;
            }
            "explore" => {
                let url = site_matches.get_one::<String>("url").unwrap();
                let site = site_matches.get_one::<String>("site").cloned();
                let goal = site_matches.get_one::<String>("goal").cloned();
                let wait: u64 = site_matches.get_one::<String>("wait")
                    .and_then(|s| s.parse().ok()).unwrap_or(3);
                let auto_fuzz = site_matches.get_flag("auto");
                let click_labels: Vec<String> = site_matches.get_one::<String>("click")
                    .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
                    .unwrap_or_default();

                let mut bridge = autocli_browser::BrowserBridge::new(
                    std::env::var("AUTOCLI_DAEMON_PORT").ok()
                        .and_then(|s| s.parse().ok()).unwrap_or(19925),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        let options = autocli_ai::ExploreOptions {
                            timeout: Some(120),
                            max_scrolls: Some(3),
                            capture_network: Some(true),
                            wait_seconds: Some(wait as f64),
                            auto_fuzz: Some(auto_fuzz),
                            click_labels,
                            goal,
                            site_name: site,
                        };
                        let result = autocli_ai::explore(page.as_ref(), url, options).await;
                        let _ = page.close().await;
                        match result {
                            Ok(manifest) => {
                                let output = serde_json::to_string_pretty(&manifest).unwrap_or_default();
                                println!("{}", output);
                            }
                            Err(e) => { print_error(&e); std::process::exit(1); }
                        }
                    }
                    Err(e) => { print_error(&e); std::process::exit(1); }
                }
                return;
            }
            "cascade" => {
                let url = site_matches.get_one::<String>("url").unwrap();

                let mut bridge = autocli_browser::BrowserBridge::new(
                    std::env::var("AUTOCLI_DAEMON_PORT").ok()
                        .and_then(|s| s.parse().ok()).unwrap_or(19925),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        let result = autocli_ai::cascade(page.as_ref(), url).await;
                        let _ = page.close().await;
                        match result {
                            Ok(r) => {
                                let output = serde_json::to_string_pretty(&r).unwrap_or_default();
                                println!("{}", output);
                            }
                            Err(e) => { print_error(&e); std::process::exit(1); }
                        }
                    }
                    Err(e) => { print_error(&e); std::process::exit(1); }
                }
                return;
            }
            "generate" => {
                let url = site_matches.get_one::<String>("url").unwrap();
                let goal = site_matches.get_one::<String>("goal").cloned();
                let use_ai = site_matches.get_flag("ai");

                let mut bridge = autocli_browser::BrowserBridge::new(
                    std::env::var("AUTOCLI_DAEMON_PORT").ok()
                        .and_then(|s| s.parse().ok()).unwrap_or(19925),
                );
                match bridge.connect().await {
                    Ok(page) => {
                        if use_ai {
                            // Resolve LLM config from ~/.autocli/config.json, overridden by CLI args
                            let mut config = autocli_ai::load_config();
                            if let Some(p) = site_matches.get_one::<String>("provider") {
                                config.llm.endpoint = Some(autocli_ai::provider_endpoint(p));
                            }
                            if let Some(m) = site_matches.get_one::<String>("model") {
                                config.llm.modelname = Some(m.to_string());
                            }
                            if let Some(k) = site_matches.get_one::<String>("api-key") {
                                config.llm.apikey = Some(k.to_string());
                            }

                            let has_endpoint = config.llm.endpoint.as_deref().map_or(false, |e| !e.trim().is_empty());
                            let has_model = config.llm.modelname.as_deref().map_or(false, |m| !m.trim().is_empty());
                            if !has_endpoint || !has_model {
                                eprintln!("{}", t(
                                    "❌ 未配置 LLM provider/model",
                                    "❌ LLM provider/model is not configured"
                                ));
                                eprintln!("{}", t(
                                    "   先运行: autocli config-llm --provider <name|url> --model <name> [--api-key <key>]",
                                    "   Run first: autocli config-llm --provider <name|url> --model <name> [--api-key <key>]"
                                ));
                                eprintln!("{}", t(
                                    "   或直接指定: autocli generate <url> --ai --provider ollama --model llama3",
                                    "   Or inline: autocli generate <url> --ai --provider ollama --model llama3"
                                ));
                                let _ = page.close().await;
                                std::process::exit(1);
                            }

                            let ai_result = autocli_ai::generate_with_ai(
                                page.as_ref(), url,
                                goal.as_deref().unwrap_or("hot"),
                                &config.llm,
                            ).await;
                            let _ = page.close().await;

                            match ai_result {
                                Ok((site, name, yaml)) => {
                                    save_adapter(&site, &name, &yaml);
                                }
                                Err(e) => { print_error(&e); std::process::exit(1); }
                            }
                        } else {
                            // Rule-based generation (existing flow)
                            let gen_result = autocli_ai::generate(page.as_ref(), url, goal.as_deref().unwrap_or("")).await;
                            let _ = page.close().await;
                            match gen_result {
                                Ok(candidate) => {
                                    save_adapter(&candidate.site, &candidate.name, &candidate.yaml);
                                }
                                Err(e) => { print_error(&e); std::process::exit(1); }
                            }
                        }
                    }
                    Err(e) => { print_error(&e); std::process::exit(1); }
                }
                return;
            }
            _ => {}
        }

        // Check if it's an external CLI
        if let Some(ext) = external_clis.iter().find(|e| e.name == site_name) {
            // Gather remaining args for the external CLI
            let ext_args: Vec<String> = match site_matches.subcommand() {
                Some((sub, sub_matches)) => {
                    let mut args = vec![sub.to_string()];
                    if let Some(rest) = sub_matches.get_many::<std::ffi::OsString>("") {
                        args.extend(rest.map(|s| s.to_string_lossy().to_string()));
                    }
                    args
                }
                None => vec![],
            };

            match autocli_external::execute_external_cli(&ext.name, &ext.binary, &ext_args)
                .await
            {
                Ok(status) => {
                    std::process::exit(status.code().unwrap_or(1));
                }
                Err(e) => {
                    print_error(&e);
                    std::process::exit(1);
                }
            }
        }

        // Check if it's a registered site
        if let Some((cmd_name, cmd_matches)) = site_matches.subcommand() {
            if let Some(cmd) = registry.get(site_name, cmd_name) {
                // Collect raw args from clap matches
                let mut raw_args: HashMap<String, String> = HashMap::new();
                for arg_def in &cmd.args {
                    if let Some(val) = cmd_matches.get_one::<String>(&arg_def.name) {
                        raw_args.insert(arg_def.name.clone(), val.clone());
                    }
                }

                // Coerce and validate
                let kwargs = match coerce_and_validate_args(&cmd.args, &raw_args) {
                    Ok(kw) => kw,
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                };

                let start = std::time::Instant::now();

                match execute_command(cmd, kwargs).await {
                    Ok(data) => {
                        let opts = RenderOptions {
                            format: output_format,
                            columns: if cmd.columns.is_empty() {
                                None
                            } else {
                                Some(cmd.columns.clone())
                            },
                            title: None,
                            elapsed: Some(start.elapsed()),
                            source: Some(cmd.full_name()),
                            footer_extra: None,
                        };
                        let output = render(&data, &opts);
                        println!("{}", output);
                    }
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("Unknown command: {} {}", site_name, cmd_name);
                std::process::exit(1);
            }
        } else {
            // Site specified but no command — show site help
            // Re-build and print help for just this site subcommand
            let app = build_cli(&registry, &external_clis);
            let app_clone = app;
            // Try to print subcommand help
            let _ = app_clone.try_get_matches_from(vec!["autocli", site_name, "--help"]);
        }
    } else {
        // No subcommand specified
        eprintln!("autocli v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("No command specified. Use --help for usage.");
        std::process::exit(1);
    }
}
