// commands/usage_billing.rs — local-only usage summaries and cost
// recommendations.
//
// PR 3 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope:
//   - `get_local_usage`                   — today/week/month stub
//   - `get_daily_usage`                   — daily timeline stub
//   - `get_burn_rate`                     — burn-rate stub
//   - `compute_cost_recommendations_local` — real recs from local_insights
//
// The three stub commands return hardcoded zeros today; real usage
// tracking parses Claude/Codex session logs and lives behind v2.6
// PR-A (passive observation). Keeping them registered preserves the
// frontend's IPC surface — UsageAnalytics + the burn-rate widget
// invoke them on mount; flipping behavior to "real numbers" is a
// PR-A concern, not this split's.
//
// The cost-recs command delegates to `crate::local_insights` which
// holds the actual aggregation logic. The split moves the Tauri
// wrapper only; the heavy lifting stays where it is.

use crate::{BurnRate, DailyUsage, UsagePeriod, UsageSummary};

#[tauri::command]
pub fn get_local_usage() -> Result<UsageSummary, String> {
    // Return zeros — real usage tracking would parse Claude's session logs
    Ok(UsageSummary {
        today: UsagePeriod { input_tokens: 0, output_tokens: 0, cost_cents: 0 },
        week: UsagePeriod { input_tokens: 0, output_tokens: 0, cost_cents: 0 },
        month: UsagePeriod { input_tokens: 0, output_tokens: 0, cost_cents: 0 },
    })
}

#[tauri::command]
pub fn get_daily_usage(_days: u32) -> Result<Vec<DailyUsage>, String> {
    Ok(Vec::new())
}

#[tauri::command]
pub fn get_burn_rate() -> Result<BurnRate, String> {
    Ok(BurnRate {
        tokens_per_hour: 0,
        cost_per_hour: 0.0,
        estimated_hours_to_limit: None,
        limit: Some(200000),
    })
}

#[tauri::command]
pub fn compute_cost_recommendations_local(
    days: Option<i64>,
    min_runs: Option<i64>,
) -> Result<crate::local_insights::LocalCostRecsResult, String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    crate::local_insights::compute_cost_recommendations_local(
        &conn,
        days.unwrap_or(30),
        min_runs.unwrap_or(10),
    )
    .map_err(|e| e.to_string())
}
