// v2.10 PR-8 — Methodology UI read APIs.
//
// Three read-only Tauri commands that back the Methodologies tab in the
// Insights section. Returns rows verbatim from the local SQLite database;
// no cloud calls, no admin endpoints, no internal pricing data beyond
// what's already published in packages/ato-pricing/pricing.json.
//
// Architecture: the React panel calls these via `invoke()`, groups the
// dispatch rows by variant_cell client-side, and computes the same per-
// cell statistics (mean / SD / 95% CI) the CLI's `runs show` already
// prints. Keeping the math in TypeScript means the Rust side stays a
// dumb data pipe — easy to audit, easy to keep aligned with the CLI's
// canonical composer.
//
// What we explicitly DON'T expose here:
// - Admin margin queries (CLI has them; UI doesn't need them in v1).
// - Internal compute rate calibration data (lives in pricing.json,
//   not in this file).
// - Any cloud-backed endpoint (those belong in ato-cloud, separate repo).

use rusqlite::params;
use tauri::State;

use crate::DbState;

#[derive(serde::Serialize)]
pub struct MethodologyView {
    pub id: String,
    pub slug: String,
    pub description: Option<String>,
    pub archetype: String,
    pub variant_matrix: String,
    pub rubric: String,
    pub created_at: String,
    pub run_count: i64,
}

#[tauri::command]
pub fn list_methodology_definitions(
    db: State<'_, DbState>,
) -> Result<Vec<MethodologyView>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT m.id, m.slug, m.description, m.archetype, m.variant_matrix,
                    m.rubric, m.created_at,
                    (SELECT COUNT(*) FROM methodology_runs r WHERE r.methodology_id = m.id) AS run_count
             FROM methodologies m
             ORDER BY m.created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MethodologyView {
                id: r.get(0)?,
                slug: r.get(1)?,
                description: r.get(2)?,
                archetype: r.get(3)?,
                variant_matrix: r.get(4)?,
                rubric: r.get(5)?,
                created_at: r.get(6)?,
                run_count: r.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

#[derive(serde::Serialize)]
pub struct MethodologyRunView {
    pub id: String,
    pub methodology_slug: String,
    pub methodology_archetype: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub planned: i64,
    pub completed: i64,
    pub customer_cost_usd: f64,
    pub customer_tokens_in: i64,
    pub customer_tokens_out: i64,
    pub provider_total_cost_usd: f64,
    pub provider_judge_cost_usd: f64,
    pub margin_usd: f64,
    pub billing_mode: String,
}

#[tauri::command]
pub fn list_methodology_runs(
    db: State<'_, DbState>,
    methodology_slug: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<MethodologyRunView>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let cap = limit.unwrap_or(100) as i64;
    let (sql, has_filter) = match &methodology_slug {
        Some(_) => (
            "SELECT r.id, m.slug, m.archetype, r.started_at, r.ended_at, r.status,
                    r.total_dispatches_planned, r.total_dispatches_completed,
                    r.customer_cost_usd, r.customer_tokens_in, r.customer_tokens_out,
                    r.provider_total_cost_usd, r.provider_judge_cost_usd,
                    r.margin_usd, r.customer_billing_mode
             FROM methodology_runs r
             JOIN methodologies m ON m.id = r.methodology_id
             WHERE m.slug = ?1
             ORDER BY r.started_at DESC
             LIMIT ?2",
            true,
        ),
        None => (
            "SELECT r.id, m.slug, m.archetype, r.started_at, r.ended_at, r.status,
                    r.total_dispatches_planned, r.total_dispatches_completed,
                    r.customer_cost_usd, r.customer_tokens_in, r.customer_tokens_out,
                    r.provider_total_cost_usd, r.provider_judge_cost_usd,
                    r.margin_usd, r.customer_billing_mode
             FROM methodology_runs r
             JOIN methodologies m ON m.id = r.methodology_id
             ORDER BY r.started_at DESC
             LIMIT ?1",
            false,
        ),
    };
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let map_row = |r: &rusqlite::Row| -> rusqlite::Result<MethodologyRunView> {
        Ok(MethodologyRunView {
            id: r.get(0)?,
            methodology_slug: r.get(1)?,
            methodology_archetype: r.get(2)?,
            started_at: r.get(3)?,
            ended_at: r.get(4)?,
            status: r.get(5)?,
            planned: r.get(6)?,
            completed: r.get(7)?,
            customer_cost_usd: r.get(8)?,
            customer_tokens_in: r.get(9)?,
            customer_tokens_out: r.get(10)?,
            provider_total_cost_usd: r.get(11)?,
            provider_judge_cost_usd: r.get(12)?,
            margin_usd: r.get(13)?,
            billing_mode: r.get(14)?,
        })
    };
    let rows: Vec<MethodologyRunView> = if has_filter {
        let slug = methodology_slug.unwrap();
        stmt.query_map(params![&slug, cap], map_row)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map(params![cap], map_row)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect()
    };
    Ok(rows)
}

#[derive(serde::Serialize)]
pub struct MethodologyDispatchView {
    pub execution_log_id: String,
    pub variant_cell: String,
    pub score: Option<f64>,
    pub cost_usd: Option<f64>,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub duration_ms: Option<i64>,
    pub status: Option<String>,
    pub grounding_verdict: Option<String>,
    pub runtime: Option<String>,
    pub model: Option<String>,
    pub created_at: Option<String>,
}

#[derive(serde::Serialize)]
pub struct MethodologyRunDetail {
    pub run: MethodologyRunView,
    pub dispatches: Vec<MethodologyDispatchView>,
}

#[tauri::command]
pub fn get_methodology_run_detail(
    db: State<'_, DbState>,
    run_id: String,
) -> Result<MethodologyRunDetail, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let run: MethodologyRunView = conn
        .query_row(
            "SELECT r.id, m.slug, m.archetype, r.started_at, r.ended_at, r.status,
                    r.total_dispatches_planned, r.total_dispatches_completed,
                    r.customer_cost_usd, r.customer_tokens_in, r.customer_tokens_out,
                    r.provider_total_cost_usd, r.provider_judge_cost_usd,
                    r.margin_usd, r.customer_billing_mode
             FROM methodology_runs r
             JOIN methodologies m ON m.id = r.methodology_id
             WHERE r.id = ?1",
            params![&run_id],
            |r| {
                Ok(MethodologyRunView {
                    id: r.get(0)?,
                    methodology_slug: r.get(1)?,
                    methodology_archetype: r.get(2)?,
                    started_at: r.get(3)?,
                    ended_at: r.get(4)?,
                    status: r.get(5)?,
                    planned: r.get(6)?,
                    completed: r.get(7)?,
                    customer_cost_usd: r.get(8)?,
                    customer_tokens_in: r.get(9)?,
                    customer_tokens_out: r.get(10)?,
                    provider_total_cost_usd: r.get(11)?,
                    provider_judge_cost_usd: r.get(12)?,
                    margin_usd: r.get(13)?,
                    billing_mode: r.get(14)?,
                })
            },
        )
        .map_err(|e| format!("methodology run '{}' not found: {}", run_id, e))?;

    let mut stmt = conn
        .prepare(
            "SELECT mrd.execution_log_id, mrd.variant_cell, mrd.score,
                    e.cost_usd_estimated, e.tokens_in, e.tokens_out, e.duration_ms,
                    e.status, e.grounding_verdict, e.runtime, e.model, e.created_at
             FROM methodology_run_dispatches mrd
             JOIN execution_logs e ON e.id = mrd.execution_log_id
             WHERE mrd.methodology_run_id = ?1
             ORDER BY e.created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let dispatches: Vec<MethodologyDispatchView> = stmt
        .query_map(params![&run_id], |r| {
            Ok(MethodologyDispatchView {
                execution_log_id: r.get(0)?,
                variant_cell: r.get(1)?,
                score: r.get(2)?,
                cost_usd: r.get(3)?,
                tokens_in: r.get(4)?,
                tokens_out: r.get(5)?,
                duration_ms: r.get(6)?,
                status: r.get(7)?,
                grounding_verdict: r.get(8)?,
                runtime: r.get(9)?,
                model: r.get(10)?,
                created_at: r.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(MethodologyRunDetail { run, dispatches })
}
