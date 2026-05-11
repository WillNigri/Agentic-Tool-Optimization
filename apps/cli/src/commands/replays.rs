// `ato replays for-trace <trace-id>` — list replay_jobs for a given
// cloud trace ID. Mirrors the existing list_replays_for_trace Tauri
// command but readable directly from SQLite.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize)]
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

pub fn for_trace(conn: &Connection, trace_id: &str, opts: &Opts) -> Result<()> {
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='replay_jobs'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if table_exists == 0 {
        if opts.human {
            emit_human(
                "No replay_jobs table found. Replay infrastructure shipped in v2.1.0; \
                 if this CLI is reading an older DB, upgrade the desktop GUI to populate \
                 the schema.",
            );
        } else {
            emit_json(&Vec::<ReplayJobRow>::new())?;
        }
        return Ok(());
    }

    // v2.3.5 bug fix: also match on source_execution_log_id so replays
    // of CLI-fired dispatches (which lack a cloud_trace_id) are
    // queryable. Found via end-to-end test in the May 11 session.
    let mut stmt = conn.prepare(
        "SELECT id, source_execution_log_id, source_cloud_trace_id, source_runtime,
                source_model, target_runtime, target_model, status, response,
                duration_ms, error_message, input_tokens, output_tokens,
                cost_usd_estimated, started_at, finished_at
           FROM replay_jobs
          WHERE source_cloud_trace_id = ?1
             OR source_execution_log_id = ?1
          ORDER BY started_at DESC
          LIMIT 50",
    ).context("Failed to prepare replays query")?;

    let rows = stmt
        .query_map([trace_id], |r| {
            Ok(ReplayJobRow {
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
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if opts.human {
        if rows.is_empty() {
            emit_human(&format!("No replays for trace {}.", trace_id));
        } else {
            let mut s = format!("{} replays for trace {}:\n\n", rows.len(), trace_id);
            for r in &rows {
                let dur = r
                    .duration_ms
                    .map(|d| format!("{}ms", d))
                    .unwrap_or_else(|| "—".to_string());
                let cost = r
                    .cost_usd_estimated
                    .map(|c| format!("${:.4}", c))
                    .unwrap_or_else(|| "—".to_string());
                s.push_str(&format!(
                    "  [{}] {} → {} ({}, {}, {})\n",
                    r.status,
                    r.source_runtime,
                    r.target_runtime,
                    &r.id[..8.min(r.id.len())],
                    dur,
                    cost
                ));
            }
            emit_human(&s);
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}
