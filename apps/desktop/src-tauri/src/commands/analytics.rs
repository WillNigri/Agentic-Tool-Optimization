// commands/analytics.rs — analytics aggregation queries.
//
// PR 6 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope:
//   - `compute_regressions_local`        — v2.3.2 fallback when cloud 401s
//   - `compute_billing_surface_summary`  — v2.6 PR-A "Last 7 days" header
//   - `get_analytics_summary`            — header counts for the dashboard
//   - `get_token_timeline`               — hourly token usage per runtime
//
// The two `compute_*_local` commands are thin Tauri wrappers around
// `crate::local_insights::*` — same shape as the cloud routes return so
// the RegressionsPanel + CostBenchmarksPanel components don't fork.
//
// `get_analytics_summary` + `get_token_timeline` are local SQL aggregations
// that the GUI's analytics dashboard hits on mount; no cross-cutting
// helpers beyond `serde_json::json!`.

use rusqlite::Connection;
use serde_json::json;
use tauri::State;

use crate::DbState;

// ── v2.3.2 Phase 2 — Local-mode regressions + billing summary ─────────
//
// Thin Tauri wrappers around the algorithm in `local_insights.rs`. The
// GUI's RegressionsPanel + CostBenchmarksPanel call these as a fallback
// when the cloud routes 401 (signed-out or expired token). Same result
// shape so the existing UI components don't need to fork.

#[tauri::command]
pub fn compute_regressions_local(
    days: Option<i64>,
    window_hours: Option<i64>,
    min_samples: Option<i64>,
) -> Result<crate::local_insights::LocalRegressionsResult, String> {
    let db_path = crate::get_db_path();
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    crate::local_insights::compute_regressions_local(
        &conn,
        days.unwrap_or(30),
        window_hours.unwrap_or(168),
        min_samples.unwrap_or(20),
    )
    .map_err(|e| e.to_string())
}

// v2.6 PR-A — billing-surface summary feeding both the "Last 7 days
// at a glance" header card and the by-surface group-by toggle in
// CostBenchmarksPanel. Local-only (passive observations + ATO's own
// dispatches both land in execution_logs).
#[tauri::command]
pub fn compute_billing_surface_summary(
    days: Option<i64>,
) -> Result<crate::local_insights::BillingSurfaceSummary, String> {
    let db_path = crate::get_db_path();
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    crate::local_insights::compute_billing_surface_summary(&conn, days.unwrap_or(7))
        .map_err(|e| e.to_string())
}

/// Get aggregated usage statistics for analytics dashboard
#[tauri::command]
pub fn get_analytics_summary(
    db: State<'_, DbState>,
) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Get skill counts
    let skill_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM skills",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    // Get workflow counts
    let workflow_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workflows",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    // Get notification channel counts
    let channel_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM notification_channels WHERE enabled = 1",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    // Get cron job counts
    let cron_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cron_jobs WHERE enabled = 1",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    // Get recent execution counts (last 7 days)
    let recent_executions: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cron_executions WHERE executed_at > datetime('now', '-7 days')",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    Ok(json!({
        "skills": skill_count,
        "workflows": workflow_count,
        "notificationChannels": channel_count,
        "cronJobs": cron_count,
        "recentExecutions": recent_executions,
        "sessionId": uuid::Uuid::new_v4().to_string(),
        "generatedAt": chrono::Utc::now().to_rfc3339()
    }))
}

#[tauri::command]
pub fn get_token_timeline(
    db: State<'_, DbState>,
    hours: Option<u32>,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let hours = hours.unwrap_or(24);

    let mut stmt = conn.prepare(&format!(
        "SELECT strftime('%Y-%m-%dT%H:00:00Z', created_at) as hour,
                runtime,
                COALESCE(SUM(tokens_in), 0) as total_in,
                COALESCE(SUM(tokens_out), 0) as total_out,
                COUNT(*) as session_count
         FROM execution_logs
         WHERE created_at > datetime('now', '-{} hours')
         GROUP BY hour, runtime
         ORDER BY hour ASC",
        hours
    )).map_err(|e| e.to_string())?;

    let rows: Vec<serde_json::Value> = stmt.query_map([], |row| {
        Ok(json!({
            "hour": row.get::<_, String>(0)?,
            "runtime": row.get::<_, String>(1)?,
            "tokensIn": row.get::<_, i64>(2).unwrap_or(0),
            "tokensOut": row.get::<_, i64>(3).unwrap_or(0),
            "sessions": row.get::<_, i64>(4).unwrap_or(0)
        }))
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    Ok(rows)
}
