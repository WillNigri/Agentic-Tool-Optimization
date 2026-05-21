// v2.7.14 — extracted from recipes_engine.rs (v2.8.0 split).
// Owns: "given an event + a recipe trigger, do they match?"
// Plus the event-log poller + recipe-candidate finder that fan
// events out to potential matchers. Pure read-side; no state
// mutation here. Sibling modules use these via pub(super).

use crate::events::{AtoEvent, RegressionSeverity, ReplayStatus};
use crate::recipes::{OpsRecipe, RecipeAction, RecipeTrigger};
use std::time::Duration;
use rusqlite::Connection;

pub(super) fn poll_events_log(since: i64) -> Result<(Vec<AtoEvent>, i64), String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    let _ = conn.busy_timeout(Duration::from_millis(500));
    let mut stmt = conn
        .prepare(
            "SELECT event_seq, payload FROM events_log WHERE event_seq > ?1 ORDER BY event_seq ASC LIMIT 200",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([since], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut events = Vec::new();
    let mut max_seq = since;
    for (seq, payload) in rows {
        if seq > max_seq {
            max_seq = seq;
        }
        match serde_json::from_str::<AtoEvent>(&payload) {
            Ok(ev) => events.push(ev),
            Err(e) => {
                // Skip malformed rows; advance past them so we don't
                // re-hit on every tick.
                eprintln!("recipes_engine: skip malformed event #{}: {}", seq, e);
            }
        }
    }
    Ok((events, max_seq))
}


pub(super) fn find_candidates(event_type: &str) -> Result<Vec<OpsRecipe>, String> {
    let db_path = crate::get_db_path();
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let _ = conn.busy_timeout(Duration::from_secs(5));
    let mut stmt = conn.prepare(
        "SELECT id, slug, name, description, trigger_config, action_config, enabled, created_at, updated_at
           FROM ops_recipes WHERE trigger_type = ?1 AND enabled = 1",
    ).map_err(|e| e.to_string())?;
    let rows: Vec<(String, String, String, Option<String>, String, String, i64, String, String)> = stmt
        .query_map([event_type], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (id, slug, name, description, tj, aj, enabled_int, created_at, updated_at) in rows {
        let trigger: RecipeTrigger = serde_json::from_str(&tj).map_err(|e| e.to_string())?;
        let action: RecipeAction = serde_json::from_str(&aj).map_err(|e| e.to_string())?;
        out.push(OpsRecipe {
            id,
            slug,
            name,
            description,
            trigger,
            action,
            enabled: enabled_int != 0,
            created_at,
            updated_at,
        });
    }
    Ok(out)
}

/// Apply optional trigger filters. None values mean "match any."
pub(super) fn trigger_filters_match(trigger: &RecipeTrigger, event: &AtoEvent) -> bool {
    match (trigger, event) {
        (
            RecipeTrigger::OnRegressionDetected {
                severity: tsev,
                agent_slug: tslug,
            },
            AtoEvent::RegressionDetected {
                severity: esev,
                agent_slug: easlug,
                ..
            },
        ) => {
            if let Some(want) = tsev {
                let got = match esev {
                    RegressionSeverity::Regression => "regression",
                    RegressionSeverity::Improvement => "improvement",
                };
                if want != got {
                    return false;
                }
            }
            if let Some(want) = tslug {
                if want != easlug {
                    return false;
                }
            }
            true
        }
        (
            RecipeTrigger::OnDispatchFailed {
                runtime: trt,
                agent_slug: tslug,
            },
            AtoEvent::DispatchFailed {
                runtime: ert,
                agent_slug: easlug,
                ..
            },
        ) => {
            if let Some(want) = trt {
                if want != ert {
                    return false;
                }
            }
            if let Some(want) = tslug {
                match easlug {
                    Some(got) if want == got => (),
                    _ => return false,
                }
            }
            true
        }
        (
            RecipeTrigger::OnReplayDone {
                status: tstatus,
                target_runtime: trt,
            },
            AtoEvent::ReplayDone {
                status: estatus,
                target_runtime: ert,
                ..
            },
        ) => {
            if let Some(want) = tstatus {
                let got = match estatus {
                    ReplayStatus::Done => "done",
                    ReplayStatus::Failed => "failed",
                };
                if want != got {
                    return false;
                }
            }
            if let Some(want) = trt {
                if want != ert {
                    return false;
                }
            }
            true
        }
        (
            RecipeTrigger::OnCostThresholdExceeded {
                window: twin,
                agent_slug: tslug,
            },
            AtoEvent::CostThresholdExceeded {
                agent_slug: easlug,
                ..
            },
        ) => {
            // window enum-to-string serialization not yet exposed
            // cleanly; for v1 we only filter on agent_slug.
            let _ = twin;
            if let Some(want) = tslug {
                if want != easlug {
                    return false;
                }
            }
            true
        }
        (
            RecipeTrigger::OnSchedule {
                cron: tcron,
                agent_slug: tslug,
            },
            AtoEvent::ScheduleFired {
                cron_id: ecron,
                agent_slug: easlug,
                ..
            },
        ) => {
            if let Some(want) = tcron {
                if want != ecron {
                    return false;
                }
            }
            if let Some(want) = tslug {
                if want != easlug {
                    return false;
                }
            }
            true
        }
        (
            RecipeTrigger::OnDispatchLongRunning {
                runtime: trt,
                agent_slug: tslug,
                threshold_secs: t_threshold,
            },
            AtoEvent::DispatchLongRunning {
                runtime: ert,
                agent_slug: easlug,
                threshold_secs: e_threshold,
                ..
            },
        ) => {
            // The watcher already emits one event per (run_id,
            // threshold) crossing keyed off this recipe's threshold,
            // so the match here is exact: recipe and event must agree
            // on threshold_secs OR the event was scheduled by another
            // recipe at a different tier. Skip the mismatch case so
            // the rate limit doesn't burn on tier-irrelevant events.
            if t_threshold != e_threshold {
                return false;
            }
            if let Some(want) = trt {
                if want != ert {
                    return false;
                }
            }
            if let Some(want) = tslug {
                match easlug {
                    Some(got) if want == got => (),
                    _ => return false,
                }
            }
            true
        }
        _ => false, // trigger type mismatch (shouldn't reach here given the SQL filter)
    }
}

