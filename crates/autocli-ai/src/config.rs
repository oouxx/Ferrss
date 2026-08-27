//! Configuration file handling for autocli.
//! Reads ~/.autocli/config.json for LLM settings and other configuration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub llm: LlmConfig,
}

/// Map a provider name to a default OpenAI-compatible chat completions endpoint.
/// Any value that looks like a URL (starts with "http") is passed through as-is.
pub fn provider_endpoint(provider: &str) -> String {
    match provider.trim().to_lowercase().as_str() {
        "openai" => "https://api.openai.com/v1/chat/completions".to_string(),
        "deepseek" => "https://api.deepseek.com/v1/chat/completions".to_string(),
        "qwen" | "dashscope" => "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions".to_string(),
        "moonshot" | "kimi" => "https://api.moonshot.cn/v1/chat/completions".to_string(),
        "zhipu" | "glm" => "https://open.bigmodel.cn/api/paas/v4/chat/completions".to_string(),
        "groq" => "https://api.groq.com/openai/v1/chat/completions".to_string(),
        "mistral" => "https://api.mistral.ai/v1/chat/completions".to_string(),
        "ollama" => "http://localhost:11434/v1/chat/completions".to_string(),
        "lmstudio" => "http://localhost:1234/v1/chat/completions".to_string(),
        other => other.trim().to_string(),
    }
}

/// Build User-Agent string: autocli/{version} ({os}; {arch}; {lang})
pub fn user_agent() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let os = if cfg!(target_os = "macos") { "macOS" }
        else if cfg!(target_os = "windows") { "Windows" }
        else if cfg!(target_os = "linux") { "Linux" }
        else { "Unknown" };
    let arch = std::env::consts::ARCH;
    let lang = if std::env::var("LANG").unwrap_or_default().to_lowercase().starts_with("zh") { "zh" } else { "en" };
    format!("autocli/{} ({}; {}; {})", version, os, arch, lang)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmConfig {
    /// API endpoint URL (e.g., "https://api.anthropic.com/v1/messages", "https://api.openai.com/v1/chat/completions")
    pub endpoint: Option<String>,
    /// API key
    pub apikey: Option<String>,
    /// Model name (e.g., "claude-sonnet-4-20250514", "gpt-4o")
    pub modelname: Option<String>,
}

impl LlmConfig {
    pub fn is_configured(&self) -> bool {
        self.endpoint.is_some() && self.apikey.is_some() && self.modelname.is_some()
    }
}

/// Get the config file path: ~/.autocli/config.json
pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".autocli").join("config.json")
}

/// Load config from ~/.autocli/config.json
/// Returns default config if file doesn't exist or can't be parsed.
pub fn load_config() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Save config to ~/.autocli/config.json
pub fn save_config(config: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {}", e))?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&path, content).map_err(|e| format!("Failed to write config: {}", e))?;
    Ok(())
}
