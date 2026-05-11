// `ato runs live` — currently in-flight dispatches.
// `ato runs get <id>` — fetch a single run by ID.
//
// Live runs are tracked in-memory by the desktop process (the
// active_runs module). The CLI can't read in-memory state of another
// process, so live-runs requires either:
//   (a) the desktop GUI to expose a local HTTP/IPC endpoint
//   (b) the CLI to query a shared SQLite-backed live-runs table
//
// For Phase 1, we ship a minimal version that reads any rows that
// exist in a (future) live_runs table; if the table doesn't exist yet,
// we return an empty list with a clear note in --human mode rather
// than crashing. The desktop side adding a SQLite-mirrored live_runs
// table is a follow-up in Phase 1.x.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct LiveRun {
    pub run_id: String,
    pub agent_slug: Option<String>,
    pub runtime: String,
    pub workspace: Option<String>,
    pub source: Option<String>,
    pub started_at: String,
    pub status: String,
}

pub fn live(conn: &Connection, opts: &Opts) -> Result<()> {
    // Check whether the live_runs table exists. If not, the desktop
    // hasn't been updated to mirror its in-memory registry yet, and
    // we return an empty result rather than failing.
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='live_runs'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if table_exists == 0 {
        if opts.human {
            emit_human(
                "No live_runs table found. The desktop GUI tracks live runs in-memory \
                 today; SQLite mirroring is on the Phase 1.x roadmap. Until then, \
                 view live runs from the desktop Insights → Live tab.",
            );
        } else {
            // JSON: return empty array so agents see consistent shape.
            emit_json(&Vec::<LiveRun>::new())?;
        }
        return Ok(());
    }

    let mut stmt = conn.prepare(
        "SELECT run_id, agent_slug, runtime, workspace, source, started_at, status
           FROM live_runs
           ORDER BY started_at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(LiveRun {
                run_id: r.get(0)?,
                agent_slug: r.get(1)?,
                runtime: r.get(2)?,
                workspace: r.get(3)?,
                source: r.get(4)?,
                started_at: r.get(5)?,
                status: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if opts.human {
        if rows.is_empty() {
            emit_human("No active runs.");
        } else {
            let mut s = format!("{} active runs:\n\n", rows.len());
            for r in &rows {
                let agent = r.agent_slug.as_deref().unwrap_or("(no agent)");
                s.push_str(&format!(
                    "  [{}] {} @{} on {} (started {})\n",
                    r.status, &r.run_id[..8.min(r.run_id.len())], agent, r.runtime, r.started_at
                ));
            }
            emit_human(&s);
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct RunDetail {
    pub id: String,
    pub runtime: String,
    pub prompt: Option<String>,
    pub response: Option<String>,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub cost_usd_estimated: Option<f64>,
    pub duration_ms: Option<i64>,
    pub status: String,
    pub error_message: Option<String>,
    pub cloud_trace_id: Option<String>,
    pub created_at: String,
}

pub fn get(conn: &Connection, id: &str, opts: &Opts) -> Result<()> {
    // Two lookups: the canonical execution_logs.id, or the
    // cloud_trace_id link. We try the primary key first.
    let row = conn
        .query_row(
            "SELECT id, runtime, prompt, response, tokens_in, tokens_out,
                    cost_usd_estimated, duration_ms, status, error_message,
                    cloud_trace_id, created_at
               FROM execution_logs
              WHERE id = ?1 OR cloud_trace_id = ?1
              LIMIT 1",
            [id],
            |r| {
                Ok(RunDetail {
                    id: r.get(0)?,
                    runtime: r.get(1)?,
                    prompt: r.get(2)?,
                    response: r.get(3)?,
                    tokens_in: r.get(4)?,
                    tokens_out: r.get(5)?,
                    cost_usd_estimated: r.get(6)?,
                    duration_ms: r.get(7)?,
                    status: r.get(8)?,
                    error_message: r.get(9)?,
                    cloud_trace_id: r.get(10)?,
                    created_at: r.get(11)?,
                })
            },
        )
        .context("Run not found. Try `ato dispatches recent` to see available IDs.")?;

    if opts.human {
        let cost = row
            .cost_usd_estimated
            .map(|c| format!("${:.4}", c))
            .unwrap_or_else(|| "—".to_string());
        let dur = row
            .duration_ms
            .map(|d| format!("{}ms", d))
            .unwrap_or_else(|| "—".to_string());
        emit_human(&format!(
            "Run {}\nRuntime: {}\nStatus: {}\nCreated: {}\nDuration: {} | Cost: {}\nTokens in/out: {:?}/{:?}\nCloud trace ID: {}\n\nPrompt:\n{}\n\nResponse:\n{}\n{}",
            row.id,
            row.runtime,
            row.status,
            row.created_at,
            dur,
            cost,
            row.tokens_in,
            row.tokens_out,
            row.cloud_trace_id.as_deref().unwrap_or("(none)"),
            row.prompt.as_deref().unwrap_or("(none)"),
            row.response.as_deref().unwrap_or("(none)"),
            row.error_message
                .as_ref()
                .map(|e| format!("\nError: {}", e))
                .unwrap_or_default()
        ));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}
