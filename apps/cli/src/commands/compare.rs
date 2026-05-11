// `ato compare <id-a> <id-b>` — side-by-side diff of two runs.
//
// Both IDs match either execution_logs.id or cloud_trace_id. Output:
// the two run rows plus a computed diff (duration delta, cost delta,
// success/failure, token delta if both estimates exist).

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ComparePane {
    pub id: String,
    pub runtime: String,
    pub prompt: Option<String>,
    pub response: Option<String>,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub cost_usd_estimated: Option<f64>,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct CompareDiff {
    pub duration_delta_ms: Option<i64>,
    pub cost_delta_usd: Option<f64>,
    pub same_status: bool,
    pub response_lines_a: Option<usize>,
    pub response_lines_b: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct CompareResult {
    pub a: ComparePane,
    pub b: ComparePane,
    pub diff: CompareDiff,
}

pub fn run(conn: &Connection, id_a: &str, id_b: &str, opts: &Opts) -> Result<()> {
    let a = fetch(conn, id_a)?.with_context(|| format!("Run not found: {}", id_a))?;
    let b = fetch(conn, id_b)?.with_context(|| format!("Run not found: {}", id_b))?;

    let duration_delta_ms = match (a.duration_ms, b.duration_ms) {
        (Some(da), Some(db)) => Some(db - da),
        _ => None,
    };
    let cost_delta_usd = match (a.cost_usd_estimated, b.cost_usd_estimated) {
        (Some(ca), Some(cb)) => Some((cb - ca * 10000.0).round() / 10000.0).map(|_| {
            // Round each side independently before subtracting to avoid
            // misleading precision from FP arithmetic.
            ((cb * 10000.0).round() - (ca * 10000.0).round()) / 10000.0
        }),
        _ => None,
    };
    let response_lines_a = a.response.as_ref().map(|r| r.lines().count());
    let response_lines_b = b.response.as_ref().map(|r| r.lines().count());

    let diff = CompareDiff {
        duration_delta_ms,
        cost_delta_usd,
        same_status: a.status == b.status,
        response_lines_a,
        response_lines_b,
    };

    let result = CompareResult { a, b, diff };

    if opts.human {
        let r = &result;
        emit_human(&format!(
            "Compare {} vs {}",
            &r.a.id[..8.min(r.a.id.len())],
            &r.b.id[..8.min(r.b.id.len())]
        ));
        emit_human(&format!(
            "  A: {} {} ({})",
            r.a.runtime,
            r.a.status,
            r.a.duration_ms.map(|d| format!("{}ms", d)).unwrap_or_else(|| "—".into()),
        ));
        emit_human(&format!(
            "  B: {} {} ({})",
            r.b.runtime,
            r.b.status,
            r.b.duration_ms.map(|d| format!("{}ms", d)).unwrap_or_else(|| "—".into()),
        ));
        emit_human(&format!(
            "  Δduration: {}",
            r.diff
                .duration_delta_ms
                .map(|d| format!("{:+}ms", d))
                .unwrap_or_else(|| "—".into()),
        ));
        emit_human(&format!(
            "  Δcost:     {}",
            r.diff
                .cost_delta_usd
                .map(|c| format!("{:+.4}", c))
                .unwrap_or_else(|| "—".into()),
        ));
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

fn fetch(conn: &Connection, id: &str) -> Result<Option<ComparePane>> {
    Ok(conn
        .query_row(
            "SELECT id, runtime, prompt, response, status, duration_ms,
                    cost_usd_estimated, tokens_in, tokens_out, created_at
               FROM execution_logs
              WHERE id = ?1 OR cloud_trace_id = ?1
              LIMIT 1",
            [id],
            |r| {
                Ok(ComparePane {
                    id: r.get(0)?,
                    runtime: r.get(1)?,
                    prompt: r.get(2)?,
                    response: r.get(3)?,
                    status: r.get(4)?,
                    duration_ms: r.get(5)?,
                    cost_usd_estimated: r.get(6)?,
                    tokens_in: r.get(7)?,
                    tokens_out: r.get(8)?,
                    created_at: r.get(9)?,
                })
            },
        )
        .optional()?)
}
