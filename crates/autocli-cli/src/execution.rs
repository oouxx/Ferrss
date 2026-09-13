use autocli_core::{CliCommand, CliError, IPage};
use autocli_pipeline::{execute_pipeline, steps::register_all_steps, StepRegistry};
use autocli_browser::BrowserBridge;
use serde_json::Value;
use std::sync::Arc;
use std::collections::HashMap;

/// Read a `FERRSS_*` environment variable, falling back to its legacy
/// `AUTOCLI_*` name so existing setups keep working.
pub fn env_compat(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| std::env::var(name.replacen("FERRSS_", "AUTOCLI_", 1)).ok())
}

/// Get daemon port from env or default
fn daemon_port() -> u16 {
    env_compat("FERRSS_DAEMON_PORT")
        .and_then(|s| s.parse().ok())
        .unwrap_or(19925)
}

/// Get command timeout from env or command config or default (60s)
fn command_timeout(cmd: &CliCommand) -> u64 {
    env_compat("FERRSS_BROWSER_COMMAND_TIMEOUT")
        .and_then(|s| s.parse().ok())
        .or(cmd.timeout_seconds)
        .unwrap_or(60)
}

pub async fn execute_command(
    cmd: &CliCommand,
    kwargs: HashMap<String, Value>,
) -> Result<Value, CliError> {
    execute_command_in(cmd, kwargs, None).await
}

/// Like [`execute_command`], but pins the browser work to a named workspace.
///
/// The Chrome extension keeps one automation window per workspace, so handing
/// every concurrent caller its own workspace is what makes browser commands
/// safe to run in parallel. `None` keeps the shared `default` workspace, which
/// is what the interactive CLI wants.
pub async fn execute_command_in(
    cmd: &CliCommand,
    kwargs: HashMap<String, Value>,
    workspace: Option<String>,
) -> Result<Value, CliError> {
    tracing::info!(
        site = %cmd.site,
        name = %cmd.name,
        workspace = workspace.as_deref().unwrap_or("default"),
        "Executing command"
    );

    let timeout_secs = command_timeout(cmd);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        execute_command_inner(cmd, kwargs, workspace),
    )
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(CliError::timeout(format!(
            "Command '{}' timed out after {}s",
            cmd.full_name(),
            timeout_secs
        ))),
    }
}

async fn execute_command_inner(
    cmd: &CliCommand,
    kwargs: HashMap<String, Value>,
    workspace: Option<String>,
) -> Result<Value, CliError> {
    // Build step registry
    let mut registry = StepRegistry::new();
    register_all_steps(&mut registry);

    if cmd.needs_browser() {
        // Browser session
        let mut bridge = BrowserBridge::new(daemon_port());
        if let Some(ref ws) = workspace {
            bridge = bridge.with_workspace(ws.clone());
        }
        let page = bridge.connect().await?;

        // Pre-navigate to domain if set, but ONLY if the pipeline doesn't
        // start with its own navigate step (to avoid double navigation).
        let pipeline_starts_with_navigate = cmd.pipeline.as_ref()
            .and_then(|steps| steps.first())
            .and_then(|step| step.as_object())
            .map_or(false, |obj| obj.contains_key("navigate"));

        if !pipeline_starts_with_navigate {
            if let Some(domain) = &cmd.domain {
                let url = format!("https://{}", domain);
                tracing::debug!(url = %url, "Pre-navigating to domain");
                page.goto(&url, None).await?;
            }
        }

        // Execute
        let result = if let Some(ref steps) = cmd.pipeline {
            execute_pipeline(Some(page.clone()), steps, &kwargs, &registry).await
        } else if cmd.func.is_some() {
            run_command(cmd, Some(page.clone()), &kwargs, &registry).await
        } else {
            Err(CliError::command_execution(format!(
                "Command '{}' has no pipeline or func",
                cmd.full_name()
            )))
        };

        // Close the automation window after a one-shot CLI command. Callers
        // that hold a pooled workspace keep it: the window is reused by the
        // next request on that slot and reclaimed by the extension's idle
        // timer once the slot goes quiet.
        if workspace.is_none() {
            let _ = page.close().await;
        }

        result
    } else {
        run_command(cmd, None, &kwargs, &registry).await
    }
}


async fn run_command(
    cmd: &CliCommand,
    page: Option<Arc<dyn IPage>>,
    kwargs: &HashMap<String, Value>,
    registry: &StepRegistry,
) -> Result<Value, CliError> {
    if let Some(pipeline) = &cmd.pipeline {
        execute_pipeline(page, pipeline, kwargs, registry).await
    } else if let Some(func) = &cmd.func {
        func(page, kwargs.clone()).await
    } else {
        Err(CliError::command_execution(format!(
            "Command '{}' has no pipeline or func",
            cmd.full_name()
        )))
    }
}
