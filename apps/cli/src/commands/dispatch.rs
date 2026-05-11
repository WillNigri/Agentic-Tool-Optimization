// `ato dispatch <runtime> "<prompt>" [--model M]`
//
// Fires a single-shot dispatch against any supported runtime CLI.
// Captures stdout/stderr, persists to execution_logs with token + cost
// estimates, returns JSON describing the result.
//
// Why this lives in the CLI (rather than calling out to the desktop):
// agents shouldn't depend on the GUI being open. The CLI is self-
// sufficient — same dispatch logic, same execution_logs schema, just
// no live-runs registration or streaming UI. Run from any shell, with
// or without the desktop running.

use crate::db;
use crate::output::{emit_human, emit_json, Opts};
use crate::runtime;
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct DispatchResult {
    pub id: String,
    pub runtime: String,
    pub model: Option<String>,
    pub status: String,
    pub response: Option<String>,
    pub error_message: Option<String>,
    pub duration_ms: i64,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub cost_usd_estimated: Option<f64>,
    pub created_at: String,
}

pub fn run(
    runtime_name: &str,
    prompt: &str,
    model: Option<String>,
    agent_slug_for_event: Option<String>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let cli_path = runtime::resolve_runtime_cli(runtime_name)?;

    let mut cmd = Command::new(&cli_path);
    match runtime_name {
        "claude" => {
            cmd.arg("--print").arg(prompt);
            if let Some(m) = &model {
                cmd.arg("--model").arg(m);
            }
        }
        "codex" => {
            // Codex requires `exec` + skip-git-repo-check (mirrors the
            // desktop's behaviour). Model goes before the prompt arg.
            cmd.arg("exec").arg("--skip-git-repo-check");
            if let Some(m) = &model {
                cmd.arg("--model").arg(m);
            }
            cmd.arg(prompt);
        }
        "gemini" => {
            cmd.arg("-p").arg(prompt);
            if let Some(m) = &model {
                cmd.arg("-m").arg(m);
            }
        }
        "hermes" => {
            cmd.arg("--execute").arg(prompt);
        }
        "openclaw" => {
            // OpenClaw's local CLI just takes `exec <prompt>` directly
            // when invoked without SSH. SSH-style remote dispatch is
            // a desktop-only feature for now (needs the ssh_config the
            // desktop loads from agent records).
            cmd.arg("exec").arg(prompt);
        }
        other => {
            anyhow::bail!("Unsupported runtime: {}", other);
        }
    }

    let started = Instant::now();
    let output = cmd
        .output()
        .with_context(|| format!("Failed to spawn {} CLI", runtime_name))?;
    let duration_ms = started.elapsed().as_millis() as i64;

    let response_text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();
    let (status, response_persisted, error_persisted): (&str, Option<String>, Option<String>) =
        if output.status.success() {
            ("success", Some(truncate(&response_text)), None)
        } else {
            let msg = if stderr_text.is_empty() {
                format!("{} exited with status {}", runtime_name, output.status)
            } else {
                stderr_text
            };
            ("error", None, Some(truncate(&msg)))
        };

    // Compute usage estimates against the effective model (override or
    // runtime default). Mirrors the desktop's persist_execution_log.
    let effective_model = model
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| runtime::default_model_for_runtime(runtime_name).map(String::from));

    // v2.3.6 — Token estimates only. CLI dispatches always go through
    // the runtime CLI (claude --print, codex exec, gemini -p), which
    // means subscription billing. We don't pretend to know the dollar
    // cost; let the cost panels treat NULL as "subscription" cleanly.
    // See persist_execution_log in the desktop crate for the matching
    // rationale. Tokens are char-count based — populated regardless of
    // whether we have an effective_model, so runtimes without a default
    // model (openclaw, hermes) still get token rows.
    let response_for_cost = response_persisted.as_deref().unwrap_or("");
    let tokens_in = Some(runtime::estimate_text_tokens(prompt));
    let tokens_out = Some(runtime::estimate_text_tokens(response_for_cost));
    let cost_usd: Option<f64> = None;

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let conn = db::open_readwrite(db_path)?;
    conn.execute(
        "INSERT INTO execution_logs (id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, cloud_trace_id, created_at, cost_usd_estimated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?11)",
        rusqlite::params![
            id,
            runtime_name,
            truncate(prompt),
            response_persisted,
            tokens_in,
            tokens_out,
            duration_ms,
            status,
            error_persisted,
            now,
            cost_usd,
        ],
    ).context("Failed to write execution_logs row")?;

    // v2.3.9 Phase 4.3 — publish a DispatchFailed event to events_log
    // so the desktop's engine poll loop can pick it up and run matching
    // recipes. CLI dispatches don't go through the in-memory bus
    // (different process); events_log is the cross-process channel.
    if status == "error" {
        crate::events_publisher::publish_dispatch_failed(
            &conn,
            &id,
            agent_slug_for_event.as_deref(),
            runtime_name,
            error_persisted.as_deref().unwrap_or(""),
            duration_ms,
            &now,
        );
    }

    let result = DispatchResult {
        id: id.clone(),
        runtime: runtime_name.to_string(),
        model: effective_model,
        status: status.to_string(),
        response: response_persisted,
        error_message: error_persisted,
        duration_ms,
        tokens_in,
        tokens_out,
        cost_usd_estimated: cost_usd,
        created_at: now,
    };

    if opts.human {
        let cost = result
            .cost_usd_estimated
            .map(|c| format!("${:.4}", c))
            .unwrap_or_else(|| "—".to_string());
        let head = format!(
            "[{}] {} {} ({}ms, {}, {})",
            result.status,
            result.runtime,
            result.model.as_deref().unwrap_or("?"),
            result.duration_ms,
            &result.id[..8.min(result.id.len())],
            cost
        );
        emit_human(&head);
        if let Some(r) = &result.response {
            emit_human("\n--- Response ---");
            emit_human(r);
        }
        if let Some(e) = &result.error_message {
            emit_human("\n--- Error ---");
            emit_human(e);
        }
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

/// 64 KB cap matching the desktop's truncate_for_log.
fn truncate(s: &str) -> String {
    const MAX: usize = 64 * 1024;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…[truncated]", &s[..MAX])
    }
}
