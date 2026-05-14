// `ato replay start <trace-id> --runtime X [--model M]`
// `ato replay get <job-id> [--wait]`
//
// Replay an existing dispatch against a different runtime/model. Reads
// the source prompt from execution_logs (matched by id OR cloud_trace_id),
// inserts a replay_jobs row, dispatches synchronously, updates the row
// with the result, returns the final job state.
//
// Why synchronous: agent shells out, gets the answer back in one call,
// stays simple. If the user wants async, they can `&` the command.

use crate::commands::dispatch;
use crate::db;
use crate::output::{emit_human, emit_json, Opts};
use crate::runtime;
use anyhow::{anyhow, Context, Result};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Default)]
pub struct ReplayJobRow {
    pub id: String,
    pub source_execution_log_id: String,
    pub source_cloud_trace_id: Option<String>,
    pub source_runtime: String,
    pub source_model: Option<String>,
    pub target_runtime: String,
    pub target_model: Option<String>,
    pub status: String,
    pub response: Option<String>,
    pub duration_ms: Option<i64>,
    pub error_message: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost_usd_estimated: Option<f64>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

pub fn start(
    trace_or_log_id: &str,
    target_runtime: &str,
    target_model: Option<String>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;

    // Look up the source. Try cloud_trace_id first (that's what the
    // GUI's Compare panel hands the user); fall back to execution_logs.id
    // so the CLI accepts either shape.
    let source: Option<(String, Option<String>, String, String)> = conn
        .query_row(
            "SELECT id, cloud_trace_id, runtime, COALESCE(prompt, '') AS prompt
               FROM execution_logs
              WHERE cloud_trace_id = ?1 OR id = ?1
              LIMIT 1",
            [trace_or_log_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;

    let (source_log_id, source_cloud_id, source_runtime, prompt) = source
        .ok_or_else(|| anyhow!("Source prompt not found locally for id '{}'. Replay requires the original dispatch to have happened on this machine.", trace_or_log_id))?;

    if prompt.is_empty() {
        return Err(anyhow!(
            "Found the source execution_logs row but its prompt column is empty — can't replay without a prompt."
        ));
    }

    let job_id = uuid::Uuid::new_v4().to_string();
    let started_at = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO replay_jobs (id, source_execution_log_id, source_cloud_trace_id, source_runtime, source_model, target_runtime, target_model, status, started_at) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, 'running', ?7)",
        rusqlite::params![
            job_id,
            source_log_id,
            source_cloud_id,
            source_runtime,
            target_runtime,
            target_model,
            started_at,
        ],
    )?;

    // Run the replay synchronously, reusing the dispatch binary path so
    // model resolution + token estimation + persistence stay consistent.
    // Note: we shell out to the runtime's CLI here directly rather than
    // calling dispatch::run() because dispatch::run writes its own
    // execution_logs row, and we want the replay's run to also write
    // one (so it's queryable like any other dispatch) — that means
    // we actually DO want to reuse dispatch::run + then UPDATE the
    // replay_jobs row with the resulting trace data.
    let dispatch_result = run_replay_dispatch(target_runtime, &prompt, target_model.clone())?;

    let finished_at = chrono::Utc::now().to_rfc3339();
    let final_status = if dispatch_result.error_message.is_some() {
        "failed"
    } else {
        "done"
    };
    conn.execute(
        "UPDATE replay_jobs SET status = ?1, response = ?2, duration_ms = ?3, error_message = ?4, finished_at = ?5, input_tokens = ?6, output_tokens = ?7, cost_usd_estimated = ?8 WHERE id = ?9",
        rusqlite::params![
            final_status,
            dispatch_result.response,
            dispatch_result.duration_ms,
            dispatch_result.error_message,
            finished_at,
            dispatch_result.tokens_in,
            dispatch_result.tokens_out,
            dispatch_result.cost_usd_estimated,
            job_id,
        ],
    )?;

    // v2.3.9 Phase 4.3 — publish replay_done to events_log so the
    // desktop engine's poll loop picks it up. Without this, CLI
    // replays never trigger Skillify; the loop only closes when the
    // GUI fires the replay. Source trace ID can be either the cloud
    // trace or the execution log id — pass whichever the row has.
    let source_for_event = source_cloud_id
        .clone()
        .unwrap_or_else(|| source_log_id.clone());
    crate::events_publisher::publish_replay_done(
        &conn,
        &job_id,
        &source_for_event,
        &source_runtime,
        target_runtime,
        target_model.as_deref(),
        final_status,
        dispatch_result.duration_ms.into(),
        dispatch_result.cost_usd_estimated,
        dispatch_result.error_message.as_deref(),
        &finished_at,
    );

    // Read back the final row to emit it consistently.
    let row = fetch_replay_row(&conn, &job_id)?;

    if opts.human {
        let cost = row
            .cost_usd_estimated
            .map(|c| format!("${:.4}", c))
            .unwrap_or_else(|| "—".to_string());
        emit_human(&format!(
            "[{}] {} → {} ({}, {})\n",
            row.status,
            row.source_runtime,
            row.target_runtime,
            row.duration_ms.map(|d| format!("{}ms", d)).unwrap_or_else(|| "—".into()),
            cost
        ));
        if let Some(r) = &row.response {
            emit_human("--- Replay response ---");
            emit_human(r);
        }
        if let Some(e) = &row.error_message {
            emit_human("--- Replay error ---");
            emit_human(e);
        }
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

pub fn get(job_id: &str, wait: bool, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;

    let poll_interval = Duration::from_millis(800);
    let start = Instant::now();
    let max_wait = Duration::from_secs(5 * 60); // 5 min cap on --wait

    loop {
        let row = fetch_replay_row(&conn, job_id)?;
        let terminal = matches!(row.status.as_str(), "done" | "failed" | "cancelled");
        if terminal || !wait {
            if opts.human {
                let cost = row
                    .cost_usd_estimated
                    .map(|c| format!("${:.4}", c))
                    .unwrap_or_else(|| "—".to_string());
                emit_human(&format!(
                    "[{}] {} → {} ({}, {})",
                    row.status,
                    row.source_runtime,
                    row.target_runtime,
                    row.duration_ms.map(|d| format!("{}ms", d)).unwrap_or_else(|| "—".into()),
                    cost
                ));
                if let Some(r) = &row.response {
                    emit_human("\n--- Replay response ---");
                    emit_human(r);
                }
            } else {
                emit_json(&row)?;
            }
            return Ok(());
        }
        if start.elapsed() > max_wait {
            return Err(anyhow!("--wait timed out after 5 minutes; replay {} still pending", job_id));
        }
        thread::sleep(poll_interval);
    }
}

fn fetch_replay_row(conn: &rusqlite::Connection, job_id: &str) -> Result<ReplayJobRow> {
    conn.query_row(
        "SELECT id, source_execution_log_id, source_cloud_trace_id, source_runtime,
                source_model, target_runtime, target_model, status, response,
                duration_ms, error_message, input_tokens, output_tokens,
                cost_usd_estimated, started_at, finished_at
           FROM replay_jobs WHERE id = ?1",
        [job_id],
        |r| Ok(ReplayJobRow {
            id: r.get(0)?,
            source_execution_log_id: r.get(1)?,
            source_cloud_trace_id: r.get(2)?,
            source_runtime: r.get(3)?,
            source_model: r.get(4)?,
            target_runtime: r.get(5)?,
            target_model: r.get(6)?,
            status: r.get(7)?,
            response: r.get(8)?,
            duration_ms: r.get(9)?,
            error_message: r.get(10)?,
            input_tokens: r.get(11)?,
            output_tokens: r.get(12)?,
            cost_usd_estimated: r.get(13)?,
            started_at: r.get(14)?,
            finished_at: r.get(15)?,
        }),
    ).context("Replay job not found")
}

/// Internal — same dispatch shape as `commands::dispatch::run` but
/// returns the result struct directly instead of writing it to stdout.
/// The dispatch's execution_logs row IS written, so the replay's output
/// also shows up in `ato dispatches recent` just like any other run.
fn run_replay_dispatch(
    runtime_name: &str,
    prompt: &str,
    model: Option<String>,
) -> Result<dispatch::DispatchResult> {
    let cli_path = runtime::resolve_runtime_cli(runtime_name)?;
    let mut cmd = Command::new(&cli_path);
    // BYOK: forward stored API key as the runtime's env var if configured.
    crate::byok::apply_byok_env(&mut cmd, &crate::db::default_db_path(), runtime_name);
    match runtime_name {
        "claude" => {
            cmd.arg("--print").arg(prompt);
            if let Some(m) = &model {
                cmd.arg("--model").arg(m);
            }
        }
        "codex" => {
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
            cmd.arg("exec").arg(prompt);
        }
        other => {
            anyhow::bail!("Unsupported runtime: {}", other);
        }
    }

    let started = Instant::now();
    let output = cmd.output().context("Failed to spawn runtime CLI")?;
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

    let effective_model = model
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| runtime::default_model_for_runtime(runtime_name).map(String::from));

    // v2.3.6 — Token estimates only. Same reasoning as dispatch::run
    // and persist_execution_log: the runtime CLI uses the user's
    // subscription, so an "API-equivalent" cost would mislead the
    // cost panels into treating subscription rows as billed. Tokens
    // decoupled from model availability.
    let response_for_cost = response_persisted.as_deref().unwrap_or("");
    let tokens_in = Some(runtime::estimate_text_tokens(prompt));
    let tokens_out = Some(runtime::estimate_text_tokens(response_for_cost));
    let cost_usd: Option<f64> = None;

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // Persist the dispatch row too — the replay's output is itself a
    // first-class dispatch in execution_logs (closes the loop).
    let conn = db::open_readwrite(&db::default_db_path())?;
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
    )?;

    Ok(dispatch::DispatchResult {
        id,
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
    })
}

fn truncate(s: &str) -> String {
    const MAX: usize = 64 * 1024;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…[truncated]", &s[..MAX])
    }
}
