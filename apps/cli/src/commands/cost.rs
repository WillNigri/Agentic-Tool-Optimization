// `ato cost recommendations [--days 30] [--min-runs 10]`
//
// Local-mode cost recommendations. Mirrors the cloud
// `/agent-traces/cost-recommendations` endpoint but runs over the local
// execution_logs table.
//
// Quality guards (a recommendation must satisfy ALL):
//   - alt has ≥ min_runs traces in the window
//   - alt is at least 30% cheaper per run than baseline
//   - alt's ok_rate is within 10pp of baseline
//
// No local agent_evaluations table yet, so eval-score guard is skipped
// (the cloud version requires eval within 5pp when both sides have it).
// Ok-rate gates alone are still useful because they catch obvious
// regressions (Haiku failing where Sonnet succeeded).

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct CostRecRow {
    pub agent_slug: String,
    pub current_runtime: String,
    pub current_runs: i64,
    pub current_cost_per_run: f64,
    pub current_ok_rate: f64,
    pub current_eval_score: Option<f64>,
    pub suggested_runtime: String,
    pub suggested_runs: i64,
    pub suggested_cost_per_run: f64,
    pub suggested_ok_rate: f64,
    pub suggested_eval_score: Option<f64>,
    pub savings_per_run_usd: f64,
    pub savings_window_usd: f64,
    pub savings_pct: f64,
    pub projected_monthly_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CostRecsResult {
    pub recommendations: Vec<CostRecRow>,
    pub days: i64,
    pub min_runs: i64,
    pub source: String,
}

#[derive(Clone)]
struct Combo {
    agent_slug: String,
    runtime: String,
    runs: i64,
    cost_per_run: f64,
    ok_rate: f64,
}

pub fn recommendations(conn: &Connection, days: i64, min_runs: i64, opts: &Opts) -> Result<()> {
    let days = days.clamp(1, 365);
    let min_runs = min_runs.max(5);

    // Schema check: agent_slug column lands in v2.3.2 desktop migration.
    // Older installs don't have it. Honest empty rather than confusing
    // SQL error so the agent calling this command knows to flag the
    // version mismatch to the human.
    if !has_column(conn, "execution_logs", "agent_slug") {
        let result = CostRecsResult {
            recommendations: vec![],
            days,
            min_runs,
            source: "local-no-schema".to_string(),
        };
        return emit(result, opts);
    }

    let cutoff = (Utc::now() - ChronoDuration::days(days)).to_rfc3339();

    let mut stmt = conn
        .prepare(
            "SELECT agent_slug, runtime,
                    COUNT(*) AS runs,
                    AVG(cost_usd_estimated) AS cost_per_run,
                    AVG(CASE WHEN status = 'success' THEN 1.0 ELSE 0.0 END) AS ok_rate
               FROM execution_logs
              WHERE agent_slug IS NOT NULL
                AND created_at > ?1
                AND cost_usd_estimated IS NOT NULL
              GROUP BY agent_slug, runtime
             HAVING COUNT(*) >= ?2",
        )
        .context("Failed to prepare cost-recommendations query")?;
    let combos: Vec<Combo> = stmt
        .query_map(rusqlite::params![&cutoff, min_runs], |r| {
            Ok(Combo {
                agent_slug: r.get(0)?,
                runtime: r.get(1)?,
                runs: r.get(2)?,
                cost_per_run: r.get(3)?,
                ok_rate: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut by_slug: HashMap<String, Vec<Combo>> = HashMap::new();
    for c in combos {
        by_slug.entry(c.agent_slug.clone()).or_default().push(c);
    }

    let mut recommendations: Vec<CostRecRow> = Vec::new();
    for combos in by_slug.values() {
        for baseline in combos {
            if baseline.cost_per_run <= 0.0 {
                continue;
            }
            for alt in combos {
                if alt.runtime == baseline.runtime {
                    continue;
                }
                let cheaper_enough = alt.cost_per_run < baseline.cost_per_run * 0.7;
                let quality_close = alt.ok_rate >= baseline.ok_rate - 0.10;
                if !cheaper_enough || !quality_close {
                    continue;
                }
                let savings_per_run = baseline.cost_per_run - alt.cost_per_run;
                let savings_window = savings_per_run * (baseline.runs as f64);
                let savings_pct = (savings_per_run / baseline.cost_per_run) * 100.0;
                let projected_monthly = (savings_window / (days as f64)) * 30.0;
                recommendations.push(CostRecRow {
                    agent_slug: baseline.agent_slug.clone(),
                    current_runtime: baseline.runtime.clone(),
                    current_runs: baseline.runs,
                    current_cost_per_run: baseline.cost_per_run,
                    current_ok_rate: baseline.ok_rate,
                    current_eval_score: None,
                    suggested_runtime: alt.runtime.clone(),
                    suggested_runs: alt.runs,
                    suggested_cost_per_run: alt.cost_per_run,
                    suggested_ok_rate: alt.ok_rate,
                    suggested_eval_score: None,
                    savings_per_run_usd: savings_per_run,
                    savings_window_usd: savings_window,
                    savings_pct,
                    projected_monthly_usd: projected_monthly,
                });
            }
        }
    }

    recommendations.sort_by(|a, b| {
        b.projected_monthly_usd
            .partial_cmp(&a.projected_monthly_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    recommendations.truncate(25);

    let result = CostRecsResult {
        recommendations,
        days,
        min_runs,
        source: "local".to_string(),
    };

    emit(result, opts)
}

fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .ok()
        .map(|iter| iter.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();
    cols.iter().any(|c| c == column)
}

fn emit(result: CostRecsResult, opts: &Opts) -> Result<()> {
    if opts.human {
        if result.source == "local-no-schema" {
            emit_human(
                "Schema not migrated for cost recommendations. Launch the ATO desktop (v2.3.2+) once to apply the migration, then retry.",
            );
            return Ok(());
        }
        if result.recommendations.is_empty() {
            emit_human(&format!(
                "No cost-swap recommendations in the last {} days. Either your usage is already optimal, or there isn't enough cross-runtime data on the same agents yet.",
                result.days
            ));
        } else {
            emit_human(&format!(
                "{} cost recommendations (last {}d, min {} runs):",
                result.recommendations.len(),
                result.days,
                result.min_runs
            ));
            for r in &result.recommendations {
                emit_human(&format!(
                    "  @{}: {} → {} (-{:.0}%, ${:.2}/mo est.)",
                    r.agent_slug,
                    r.current_runtime,
                    r.suggested_runtime,
                    r.savings_pct,
                    r.projected_monthly_usd
                ));
            }
        }
    } else {
        emit_json(&result)?;
    }
    Ok(())
}
