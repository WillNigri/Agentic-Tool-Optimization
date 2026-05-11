// v2.3.2 Phase 2 — Local-mode regressions + cost recommendations.
//
// Ports the cloud `/agent-traces/regressions` and
// `/agent-traces/cost-recommendations` endpoints to run over the local
// SQLite (`execution_logs` + `agent_config_changes`). Same algorithm,
// no sign-in required. The cloud endpoints stay as the source of truth
// for cross-device aggregation; this module is the offline-first
// surface.
//
// Two algorithm notes vs the cloud SQL:
//   1. SQLite has no percentile_cont. We pull the duration_ms values
//      for each window in Rust and compute p95 manually (sort + index).
//   2. Local has no agent_evaluations table (evaluators are cloud-tier).
//      We return eval_score / eval_delta_pp as None for every local
//      row. The dashboard renders "—" for those cases already.

use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::Connection;
use serde::Serialize;

// The rest of the desktop crate uses Result<T, String> as the error
// shape (Tauri command convention). We follow the same here so the
// Tauri wrappers in commands.rs don't have to convert at the boundary.
type Result<T> = std::result::Result<T, String>;

fn to_string_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ─── Regressions ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct LocalRegressionRow {
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
    pub before_eval_score: Option<f64>, // always None locally
    pub before_eval_count: i64,         // always 0 locally

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

    pub severity: String, // "regression" | "improvement" | "neutral"
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalRegressionsResult {
    pub regressions: Vec<LocalRegressionRow>,
    pub window_hours: i64,
    pub min_samples: i64,
    pub days: i64,
    pub source: String, // "local" — distinguishes from cloud response
}

#[derive(Debug, Clone)]
struct ConfigChangeRow {
    id: String,
    agent_slug: String,
    field: String,
    old_value: Option<String>,
    new_value: Option<String>,
    changed_at: String,
}

pub fn compute_regressions_local(
    conn: &Connection,
    days: i64,
    window_hours: i64,
    min_samples: i64,
) -> Result<LocalRegressionsResult> {
    let days = days.clamp(1, 365);
    let window_hours = window_hours.clamp(1, 720);
    let min_samples = min_samples.max(5);

    let cutoff = (Utc::now() - ChronoDuration::days(days)).to_rfc3339();

    let mut stmt = conn
        .prepare(
            "SELECT id, agent_slug, field, old_value, new_value, changed_at
               FROM agent_config_changes
              WHERE field IN ('model', 'role_models', 'system_prompt', 'runtime')
                AND changed_at > ?1
              ORDER BY changed_at DESC",
        )
        .map_err(to_string_err)?;

    let candidate_iter = stmt
        .query_map([&cutoff], |r| {
            Ok(ConfigChangeRow {
                id: r.get(0)?,
                agent_slug: r.get(1)?,
                field: r.get(2)?,
                old_value: r.get(3)?,
                new_value: r.get(4)?,
                changed_at: r.get(5)?,
            })
        })
        .map_err(to_string_err)?;

    let candidates: Vec<ConfigChangeRow> = candidate_iter
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(to_string_err)?;

    let mut rows: Vec<LocalRegressionRow> = Vec::new();

    for cc in candidates {
        let changed_at = match chrono::DateTime::parse_from_rfc3339(&cc.changed_at) {
            Ok(t) => t.with_timezone(&Utc),
            Err(_) => continue,
        };
        let window = ChronoDuration::hours(window_hours);
        let before_start = (changed_at - window).to_rfc3339();
        let before_end = changed_at.to_rfc3339();
        let after_start = changed_at.to_rfc3339();
        let after_end = (changed_at + window).to_rfc3339();

        let before = window_stats(conn, &cc.agent_slug, &before_start, &before_end, false)?;
        let after = window_stats(conn, &cc.agent_slug, &after_start, &after_end, true)?;

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
        let eval_delta_pp: Option<f64> = None;

        let severity = if ok_delta_pp <= -10.0 || p95_delta_pct >= 50.0 || cost_delta_pct >= 25.0 {
            "regression"
        } else if ok_delta_pp >= 10.0 || p95_delta_pct <= -25.0 || cost_delta_pct <= -25.0 {
            "improvement"
        } else {
            "neutral"
        }
        .to_string();

        rows.push(LocalRegressionRow {
            change_id: cc.id,
            agent_slug: cc.agent_slug,
            field: cc.field,
            old_value: cc.old_value,
            new_value: cc.new_value,
            changed_at: cc.changed_at,
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
            eval_delta_pp,
            severity,
        });
    }

    Ok(LocalRegressionsResult {
        regressions: rows,
        window_hours,
        min_samples,
        days,
        source: "local".to_string(),
    })
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
    let mut stmt = conn
        .prepare(
            "SELECT duration_ms, status, COALESCE(cost_usd_estimated, 0)
               FROM execution_logs
              WHERE agent_slug = ?1
                AND created_at >= ?2
                AND created_at <  ?3",
        )
        .map_err(to_string_err)?;
    let rows: Vec<(Option<i64>, String, f64)> = stmt
        .query_map([agent_slug, start, end], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })
        .map_err(to_string_err)?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(to_string_err)?;

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
        // Nearest-rank percentile (close enough for human-readable diff).
        let idx = ((durations.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
        *durations.get(idx).unwrap_or(&0)
    };

    let total_cost: f64 = rows.iter().map(|(_, _, c)| *c).sum();
    let cost_per_run = total_cost / (runs as f64);

    let failing_trace_ids = if collect_failing {
        // Bind the statement + the iterator separately so both live
        // long enough for the collect to drain rows. Chaining
        // .query_map().collect() directly off the prepare result
        // dropped the Statement too early.
        let mut s = conn
            .prepare(
                "SELECT COALESCE(cloud_trace_id, id)
                   FROM execution_logs
                  WHERE agent_slug = ?1
                    AND status != 'success'
                    AND created_at >= ?2
                    AND created_at <  ?3
                  ORDER BY created_at DESC
                  LIMIT 10",
            )
            .map_err(to_string_err)?;
        let iter = s
            .query_map([agent_slug, start, end], |r| r.get::<_, String>(0))
            .map_err(to_string_err)?;
        let v: Vec<String> = iter
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
            .map_err(to_string_err)?;
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

// ─── Cost recommendations ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct LocalCostRecRow {
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
pub struct LocalCostRecsResult {
    pub recommendations: Vec<LocalCostRecRow>,
    pub days: i64,
    pub min_runs: i64,
    pub source: String, // "local"
}

#[derive(Clone)]
struct Combo {
    agent_slug: String,
    runtime: String,
    runs: i64,
    cost_per_run: f64,
    ok_rate: f64,
}

pub fn compute_cost_recommendations_local(
    conn: &Connection,
    days: i64,
    min_runs: i64,
) -> Result<LocalCostRecsResult> {
    let days = days.clamp(1, 365);
    let min_runs = min_runs.max(5);

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
        .map_err(to_string_err)?;
    let combos: Vec<Combo> = stmt
        .query_map(rusqlite::params![&cutoff, min_runs], |r| {
            Ok(Combo {
                agent_slug: r.get(0)?,
                runtime: r.get(1)?,
                runs: r.get(2)?,
                cost_per_run: r.get(3)?,
                ok_rate: r.get(4)?,
            })
        })
        .map_err(to_string_err)?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(to_string_err)?;

    let mut by_slug: std::collections::HashMap<String, Vec<Combo>> =
        std::collections::HashMap::new();
    for c in combos {
        by_slug.entry(c.agent_slug.clone()).or_default().push(c);
    }

    let mut recommendations: Vec<LocalCostRecRow> = Vec::new();
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
                recommendations.push(LocalCostRecRow {
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

    Ok(LocalCostRecsResult {
        recommendations,
        days,
        min_runs,
        source: "local".to_string(),
    })
}
