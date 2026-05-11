// `ato regressions list [--days 30] [--window-hours 168] [--min-samples 20]`
//
// Local-mode regression detection. Mirrors the cloud `/agent-traces/
// regressions` endpoint but runs over the local SQLite tables. No
// sign-in required. Same algorithm; same output shape; same severity
// classification.
//
// What's not yet local: agent_evaluations (eval scores live cloud-side).
// We surface eval_delta_pp as null; the human can compare ok-rate +
// p95 + cost deltas to spot regressions even without a quality score.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::Connection;
use serde::Serialize;

// Same shape as ato-desktop's LocalRegressionsResult so MCP tool
// equivalents (Phase 3) can hand off without translation.
#[derive(Debug, Clone, Serialize)]
pub struct RegressionRow {
    pub change_id: String,
    pub agent_slug: String,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub changed_at: String,

    pub before_runs: i64,
    pub before_ok_rate: f64,
    pub before_p95_ms: i64,
    pub before_cost_per_run: f64,
    pub before_eval_score: Option<f64>,
    pub before_eval_count: i64,

    pub after_runs: i64,
    pub after_ok_rate: f64,
    pub after_p95_ms: i64,
    pub after_cost_per_run: f64,
    pub after_eval_score: Option<f64>,
    pub after_eval_count: i64,

    pub failing_trace_ids: Vec<String>,

    pub ok_delta_pp: f64,
    pub p95_delta_pct: f64,
    pub cost_delta_pct: f64,
    pub eval_delta_pp: Option<f64>,

    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegressionsResult {
    pub regressions: Vec<RegressionRow>,
    pub window_hours: i64,
    pub min_samples: i64,
    pub days: i64,
    pub source: String,
}

pub fn list(
    conn: &Connection,
    days: i64,
    window_hours: i64,
    min_samples: i64,
    opts: &Opts,
) -> Result<()> {
    // Sanity-check the inputs (matching desktop clamps).
    let days = days.clamp(1, 365);
    let window_hours = window_hours.clamp(1, 720);
    let min_samples = min_samples.max(5);

    // Same algorithm as ato-desktop's local_insights::compute_regressions_local.
    // We duplicate here in the CLI rather than IPC into the desktop because
    // (a) the CLI is meant to work without the desktop running, and
    // (b) the queries are cheap — sub-100ms typical.
    // Schema check: agent_config_changes table + execution_logs.agent_slug
    // column both land in v2.3.2 desktop migration. Without them the
    // regression algorithm can't run. Honest empty result with a
    // clear source string so the calling agent knows to flag the
    // version mismatch.
    let agent_config_changes_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='agent_config_changes'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let has_agent_slug = has_column(conn, "execution_logs", "agent_slug");
    if agent_config_changes_exists == 0 || !has_agent_slug {
        // Schema not migrated yet (older desktop install). Honest empty
        // result rather than a confusing SQL error.
        return emit(
            RegressionsResult {
                regressions: vec![],
                window_hours,
                min_samples,
                days,
                source: "local-no-schema".to_string(),
            },
            opts,
        );
    }

    let cutoff = (Utc::now() - ChronoDuration::days(days)).to_rfc3339();

    let mut stmt = conn
        .prepare(
            "SELECT id, agent_slug, field, old_value, new_value, changed_at
               FROM agent_config_changes
              WHERE field IN ('model', 'role_models', 'system_prompt', 'runtime')
                AND changed_at > ?1
              ORDER BY changed_at DESC",
        )
        .context("Failed to prepare regressions query")?;
    let candidates: Vec<(String, String, String, Option<String>, Option<String>, String)> = stmt
        .query_map([&cutoff], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut rows: Vec<RegressionRow> = Vec::new();
    for (id, agent_slug, field, old_value, new_value, changed_at_str) in candidates {
        let changed_at = match chrono::DateTime::parse_from_rfc3339(&changed_at_str) {
            Ok(t) => t.with_timezone(&Utc),
            Err(_) => continue,
        };
        let window = ChronoDuration::hours(window_hours);
        let before_start = (changed_at - window).to_rfc3339();
        let before_end = changed_at.to_rfc3339();
        let after_start = changed_at.to_rfc3339();
        let after_end = (changed_at + window).to_rfc3339();

        let before = window_stats(conn, &agent_slug, &before_start, &before_end, false)?;
        let after = window_stats(conn, &agent_slug, &after_start, &after_end, true)?;

        if before.runs < min_samples || after.runs < min_samples {
            continue;
        }

        let ok_delta_pp = (after.ok_rate - before.ok_rate) * 100.0;
        let p95_delta_pct = if before.p95_ms > 0 {
            (after.p95_ms - before.p95_ms) as f64 / before.p95_ms as f64 * 100.0
        } else {
            0.0
        };
        let cost_delta_pct = if before.cost_per_run > 0.0 {
            (after.cost_per_run - before.cost_per_run) / before.cost_per_run * 100.0
        } else {
            0.0
        };

        let severity = if ok_delta_pp <= -10.0 || p95_delta_pct >= 50.0 || cost_delta_pct >= 25.0 {
            "regression"
        } else if ok_delta_pp >= 10.0 || p95_delta_pct <= -25.0 || cost_delta_pct <= -25.0 {
            "improvement"
        } else {
            "neutral"
        }
        .to_string();

        rows.push(RegressionRow {
            change_id: id,
            agent_slug,
            field,
            old_value,
            new_value,
            changed_at: changed_at_str,
            before_runs: before.runs,
            before_ok_rate: before.ok_rate,
            before_p95_ms: before.p95_ms,
            before_cost_per_run: before.cost_per_run,
            before_eval_score: None,
            before_eval_count: 0,
            after_runs: after.runs,
            after_ok_rate: after.ok_rate,
            after_p95_ms: after.p95_ms,
            after_cost_per_run: after.cost_per_run,
            after_eval_score: None,
            after_eval_count: 0,
            failing_trace_ids: after.failing_trace_ids,
            ok_delta_pp,
            p95_delta_pct,
            cost_delta_pct,
            eval_delta_pp: None,
            severity,
        });
    }

    emit(
        RegressionsResult {
            regressions: rows,
            window_hours,
            min_samples,
            days,
            source: "local".to_string(),
        },
        opts,
    )
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

fn emit(result: RegressionsResult, opts: &Opts) -> Result<()> {
    if opts.human {
        if result.source == "local-no-schema" {
            emit_human(
                "Schema not migrated for regression detection. Launch the ATO desktop (v2.3.2+) once to apply the migration, then retry.",
            );
            return Ok(());
        }
        let regressions_only: Vec<&RegressionRow> = result
            .regressions
            .iter()
            .filter(|r| r.severity == "regression")
            .collect();
        if result.regressions.is_empty() {
            emit_human(&format!(
                "No regressions detected (days={}, window={}h, min-samples={}).",
                result.days, result.window_hours, result.min_samples
            ));
        } else {
            emit_human(&format!(
                "{} regressions, {} improvements, {} neutral (window {}h, min {} samples)",
                regressions_only.len(),
                result.regressions.iter().filter(|r| r.severity == "improvement").count(),
                result.regressions.iter().filter(|r| r.severity == "neutral").count(),
                result.window_hours,
                result.min_samples
            ));
            for r in &result.regressions {
                emit_human(&format!(
                    "  [{}] @{} {} {}→{}: ok {:+.1}pp · p95 {:+.0}% · cost {:+.0}%",
                    r.severity,
                    r.agent_slug,
                    r.field,
                    r.old_value.as_deref().unwrap_or("?"),
                    r.new_value.as_deref().unwrap_or("?"),
                    r.ok_delta_pp,
                    r.p95_delta_pct,
                    r.cost_delta_pct
                ));
            }
        }
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

struct WindowStats {
    runs: i64,
    ok_rate: f64,
    p95_ms: i64,
    cost_per_run: f64,
    failing_trace_ids: Vec<String>,
}

fn window_stats(
    conn: &Connection,
    agent_slug: &str,
    start: &str,
    end: &str,
    collect_failing: bool,
) -> Result<WindowStats> {
    let mut stmt = conn.prepare(
        "SELECT duration_ms, status, COALESCE(cost_usd_estimated, 0)
           FROM execution_logs
          WHERE agent_slug = ?1
            AND created_at >= ?2
            AND created_at <  ?3",
    )?;
    let rows: Vec<(Option<i64>, String, f64)> = stmt
        .query_map([agent_slug, start, end], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let runs = rows.len() as i64;
    if runs == 0 {
        return Ok(WindowStats {
            runs: 0,
            ok_rate: 0.0,
            p95_ms: 0,
            cost_per_run: 0.0,
            failing_trace_ids: vec![],
        });
    }

    let oks = rows.iter().filter(|(_, s, _)| s == "success").count() as f64;
    let ok_rate = oks / (runs as f64);

    let mut durations: Vec<i64> = rows.iter().filter_map(|(d, _, _)| *d).collect();
    durations.sort_unstable();
    let p95_ms = if durations.is_empty() {
        0
    } else {
        let idx = ((durations.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
        *durations.get(idx).unwrap_or(&0)
    };

    let total_cost: f64 = rows.iter().map(|(_, _, c)| *c).sum();
    let cost_per_run = total_cost / (runs as f64);

    let failing_trace_ids = if collect_failing {
        let mut s = conn.prepare(
            "SELECT COALESCE(cloud_trace_id, id)
               FROM execution_logs
              WHERE agent_slug = ?1
                AND status != 'success'
                AND created_at >= ?2
                AND created_at <  ?3
              ORDER BY created_at DESC
              LIMIT 10",
        )?;
        let iter = s.query_map([agent_slug, start, end], |r| r.get::<_, String>(0))?;
        let v: Vec<String> = iter.collect::<Result<Vec<_>, _>>()?;
        v
    } else {
        vec![]
    };

    Ok(WindowStats {
        runs,
        ok_rate,
        p95_ms,
        cost_per_run,
        failing_trace_ids,
    })
}
