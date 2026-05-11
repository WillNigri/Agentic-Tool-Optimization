// `ato dispatches recent` — list recent execution_logs rows.
//
// Maps to the History tab in the desktop GUI. Reads execution_logs
// directly; the schema lives in apps/desktop/src-tauri/src/lib.rs
// (init_database).

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DispatchRow {
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

pub fn recent(
    conn: &Connection,
    limit: usize,
    runtime: Option<String>,
    status: Option<String>,
    opts: &Opts,
) -> Result<()> {
    // Build the query incrementally based on filters.
    // Why named conditions rather than a single big SQL: makes it
    // obvious which filters were applied; easier to grep for.
    let mut where_parts: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(rt) = &runtime {
        where_parts.push("runtime = ?".to_string());
        params.push(Box::new(rt.clone()));
    }
    if let Some(st) = &status {
        where_parts.push("status = ?".to_string());
        params.push(Box::new(st.clone()));
    }

    let where_clause = if where_parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_parts.join(" AND "))
    };

    // Limit is a regular bind value but only valid for non-negative
    // integers; clamp defensively.
    let safe_limit: i64 = limit.min(10_000) as i64;

    let sql = format!(
        "SELECT id, runtime, prompt, response, tokens_in, tokens_out,
                cost_usd_estimated, duration_ms, status, error_message,
                cloud_trace_id, created_at
           FROM execution_logs
           {where_clause}
           ORDER BY created_at DESC
           LIMIT ?",
        where_clause = where_clause
    );

    let mut stmt = conn.prepare(&sql).context("Failed to prepare query")?;

    // Combine the filter params with the limit at the tail.
    params.push(Box::new(safe_limit));
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

    let rows = stmt
        .query_map(rusqlite::params_from_iter(param_refs.iter()), |r| {
            Ok(DispatchRow {
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
        })
        .context("Failed to execute query")?;

    let mut out: Vec<DispatchRow> = Vec::new();
    for r in rows {
        out.push(r?);
    }

    if opts.human {
        emit_human(&format_human(&out));
    } else {
        emit_json(&out)?;
    }
    Ok(())
}

fn format_human(rows: &[DispatchRow]) -> String {
    if rows.is_empty() {
        return "No dispatches found.".to_string();
    }
    let mut s = String::new();
    s.push_str(&format!("{} dispatches:\n\n", rows.len()));
    for r in rows {
        let cost = r
            .cost_usd_estimated
            .map(|c| format!("${:.4}", c))
            .unwrap_or_else(|| "—".to_string());
        let dur = r
            .duration_ms
            .map(|d| format!("{}ms", d))
            .unwrap_or_else(|| "—".to_string());
        s.push_str(&format!(
            "  [{}] {} {} ({}, {}, {})\n",
            r.status, r.created_at, r.runtime, &r.id[..8.min(r.id.len())], dur, cost
        ));
    }
    s
}
