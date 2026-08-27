//! LLM client for AI-powered adapter generation.
//! Routes requests to a user-specified OpenAI-compatible provider (OpenAI, DeepSeek,
//! Ollama, LM Studio, vLLM, ...). The prompt is constructed locally and the raw
//! captured page data is sent so the model can emit a precise YAML adapter.

use autocli_core::CliError;
use serde_json::{json, Value};
use tracing::{debug, info};

use crate::config::LlmConfig;

/// Send captured page data to the configured LLM provider and get back a YAML adapter.
pub async fn generate_with_llm(
    llm: &LlmConfig,
    captured_data: &Value,
    goal: &str,
    site: &str,
) -> Result<String, CliError> {
    let endpoint = llm.endpoint.clone().unwrap_or_default();
    if endpoint.trim().is_empty() {
        return Err(CliError::empty_result(
            "LLM provider endpoint is not configured. Run: autocli config-llm or edit ~/.autocli/config.json (llm.endpoint)",
        ));
    }

    let model = llm
        .modelname
        .clone()
        .unwrap_or_default();

    info!(endpoint = %endpoint, "Calling LLM for adapter generation");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| CliError::Http {
            message: format!("Failed to create HTTP client: {}", e),
            suggestions: vec![],
            source: None,
        })?;

    let prompt = build_prompt(captured_data, goal, site);

    let mut body = json!({
        "model": model,
        "stream": false,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": prompt }
        ],
        "temperature": 0.2,
    });

    if let Some(k) = &llm.apikey {
        if !k.is_empty() {
            body["max_tokens"] = json!(4096);
        }
    }
    let _ = body; // keep temperature/max_tokens simple

    let mut req = client
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .header("User-Agent", crate::config::user_agent())
        .json(&body);

    if let Some(k) = &llm.apikey {
        if !k.is_empty() {
            req = req.bearer_auth(k);
        }
    }

    let resp = req.send().await.map_err(|e| CliError::Http {
        message: format!("LLM request failed: {}", e),
        suggestions: vec!["Check the endpoint/network in ~/.autocli/config.json (llm.endpoint)".into()],
        source: None,
    })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(CliError::Http {
            message: format!("LLM API error {}: {}", status, text.chars().take(500).collect::<String>()),
            suggestions: vec![],
            source: None,
        });
    }

    let resp_json: Value = resp.json().await.map_err(|e| CliError::Http {
        message: format!("Failed to parse LLM response: {}", e),
        suggestions: vec![],
        source: None,
    })?;

    // Extract content from OpenAI-compatible response format
    let content = resp_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    if content.trim().is_empty() {
        return Err(CliError::Http {
            message: "LLM returned empty content".into(),
            suggestions: vec![],
            source: None,
        });
    }

    let yaml = clean_yaml(&content);

    if yaml.is_empty() {
        return Err(CliError::Http {
            message: "LLM returned empty content".into(),
            suggestions: vec![],
            source: None,
        });
    }

    debug!(yaml_len = yaml.len(), "LLM generated adapter YAML");
    Ok(yaml)
}

/// Strip markdown fencing and thinking tags from LLM output.
fn clean_yaml(content: &str) -> String {
    let mut cleaned = content.to_string();

    // Remove reasoning/thinking tags
    for (open, close) in [(" thinking", " response"), ("<thinking>", "</thinking>"), ("<reasoning>", "</reasoning>")] {
        loop {
            match cleaned.find(open) {
                Some(start) => {
                    if let Some(end) = cleaned.find(close) {
                        if end > start {
                            cleaned = format!("{}{}", &cleaned[..start], &cleaned[end + close.len()..]);
                            continue;
                        }
                    }
                    cleaned = cleaned[..start].to_string();
                }
                None => break,
            }
            break;
        }
    }

    // Strip markdown code fence
    let trimmed = cleaned.trim();
    if let Some(rest) = trimmed
        .strip_prefix("```yaml")
        .or_else(|| trimmed.strip_prefix("```"))
    {
        let rest = rest.strip_suffix("```").unwrap_or(rest);
        cleaned = rest.trim().to_string();
    }

    cleaned.trim().to_string()
}

/// Local system prompt describing the adapter pipeline schema.
const SYSTEM_PROMPT: &str = r#"You are an expert at reverse-engineering websites and generating data-scraping adapter definitions in YAML.

Given captured data from a website (real API responses, rendered HTML, metadata, and framework hints), produce a single YAML adapter that follows this exact schema:

```yaml
site: <detected site name>
name: <short command name, e.g. hot, search, feed>
description: <one-line description>
domain: <the site's root domain>
strategy: <public | cookie | header | intercept | ui>
browser: <true | false>

args:
  <arg_name>:
    type: <int | str | bool>
    default: <value>
    description: <short description>

pipeline:
  - <step>
  - <step>
  # ... more steps ...

columns: [col1, col2, ...]
```

Available pipeline steps (in order of common usage):
- `fetch: { url: <url> }` or `fetch: <url>` — perform an HTTP GET. The URL may reference previous items/variables with ${{ ... }}.
- `select: <json path>` — drill into a nested field of the response (e.g. `data.posts`).
- `map: { field: <expr> }` — project fields from each item. `${{ item.x }}` accesses item fields, `${{ index }}` is the 0-based index.
- `filter: <expr>` — keep items where the expression is truthy (e.g. `item.title && !item.deleted`).
- `sort: { by: <field>, order: <asc|desc> }` — sort items.
- `limit: <expr or int>` — truncate the list (may reference `${{ args.limit }}`).
- `navigate: { url: <url> }` or `navigate: <url>` — load a page in the browser (for browser strategies).
- `evaluate: <js>` — run JS in the browser page and use its return value.
- `click: <selector>` — click an element in the browser.
- `type: { selector: <sel>, text: <text> }` — type into an input.
- `wait: <ms>` — wait milliseconds.
- `intercept: { pattern: <glob> }` — capture network responses matching a pattern.

Guidelines:
- Use the provided API response bodies directly whenever possible (prefer real API JSON over scraping HTML).
- Map output columns to the fields the user cares about; pick meaningful names.
- The `${{ args.xxx }}` placeholder lets the CLI pass a user-supplied argument, typically a `limit`.
- Use `strategy: public` only when the API is reachable without authentication. Otherwise infer cookie/header based on the captured data.
- Return ONLY the YAML. No explanation, no code fences, no markdown."#;

/// Build the user prompt embedding the captured page data.
fn build_prompt(captured_data: &Value, goal: &str, site: &str) -> String {
    let captured_str = serde_json::to_string(captured_data)
        .unwrap_or_else(|_| captured_data.to_string());
    format!(
        "Site: {site}\nGoal: {goal}\n\nGenerate an autocli adapter YAML for the \"{goal}\" data of this site.\n\nCaptured page data (JSON):\n{captured_str}"
    )
}
