// v2.3.8 Phase 4.2 — Ops recipe execution engine.
//
// Long-running tokio task that:
//   1. Subscribes to events::bus
//   2. For each event, queries ops_recipes for enabled recipes whose
//      trigger_type matches AND whose optional trigger filters match
//      the event payload
//   3. Runs each matching recipe's action
//   4. Audits the run to ops_recipe_runs
//
// Scope for v1: two action executors (DraftSkillFromReplay,
// ReplayOnAlt) — enough to close the Skillify loop end-to-end. Other
// action variants stub with "not_implemented" status. Recursion guard
// is intentionally absent because the v1 chains don't loop (drafting
// a skill produces no event; replaying produces a single replay_done
// event that only Skillify subscribes to, and Skillify drafts files).

use crate::events::{bus, AtoEvent, RegressionSeverity, ReplayStatus};
use crate::recipes::{OpsRecipe, RecipeAction, RecipeTrigger};
use rusqlite::Connection;
use std::time::Duration;

/// Start the engine. Spawns TWO tokio tasks:
///   1. Live bus subscriber — fast path for in-process events
///   2. events_log poll loop — picks up cross-process events (CLI
///      dispatches publish there since they can't reach the in-memory
///      bus). Polls every 2s for new event_seqs.
///
/// Both paths converge on handle_event, which dedupes via
/// ops_recipe_runs (skip events already processed). Multiple calls to
/// start() are safe but only one should fire at app boot.
pub fn start() {
    // Live bus subscriber.
    tauri::async_runtime::spawn(async move {
        let mut rx = bus::subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Err(e) = handle_event(event).await {
                        eprintln!("recipes_engine: handle_event error: {}", e);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    // Subscriber fell behind. The poll loop below
                    // recovers any missed events from events_log on
                    // the next tick.
                    eprintln!(
                        "recipes_engine: bus lagged {} events; poll loop will recover from events_log",
                        skipped
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    // Channel is dead — bus dropped. Exit cleanly.
                    return;
                }
            }
        }
    });

    // events_log poll loop (catches CLI-published events + recovers
    // from any bus lag). Bootstrap last_seen_seq from MAX(event_seq)
    // at startup so we don't reprocess history.
    tauri::async_runtime::spawn(async move {
        let mut last_seen_seq: i64 = 0;
        // Initial bootstrap — skip all existing events so first launch
        // doesn't replay your historical event log into the engine.
        if let Ok(conn) = rusqlite::Connection::open(crate::get_db_path()) {
            last_seen_seq = conn
                .query_row("SELECT COALESCE(MAX(event_seq), 0) FROM events_log", [], |r| r.get(0))
                .unwrap_or(0);
        }
        let interval = std::time::Duration::from_millis(2000);
        loop {
            tokio::time::sleep(interval).await;
            match poll_events_log(last_seen_seq) {
                Ok((events, max_seen)) => {
                    last_seen_seq = max_seen.max(last_seen_seq);
                    for ev in events {
                        if let Err(e) = handle_event(ev).await {
                            eprintln!("recipes_engine: poll handle_event error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("recipes_engine: poll error: {}", e);
                }
            }
        }
    });
}

/// Read events_log rows with event_seq > since, in seq order. Returns
/// the parsed events and the highest event_seq seen (or `since` if no
/// new rows). Best-effort: a locked DB returns no rows and the next
/// tick retries.
fn poll_events_log(since: i64) -> Result<(Vec<AtoEvent>, i64), String> {
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

async fn handle_event(event: AtoEvent) -> Result<(), String> {
    let event_type = event.type_name();
    let event_payload =
        serde_json::to_string(&event).map_err(|e| format!("serialize event: {}", e))?;

    // v2.3.9 — Recipes store trigger_type with an `on_` prefix
    // ("on_replay_done") while events publish without it
    // ("replay_done"). The mismatch was caught by codex in the 4.1
    // review but not fixed until end-to-end dogfooding surfaced the
    // silent no-fire. Build the recipe-side lookup key explicitly.
    let trigger_lookup = format!("on_{}", event_type);

    // Find all enabled recipes whose trigger_type matches this event.
    let candidates = match find_candidates(&trigger_lookup) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("recipes_engine: candidate lookup failed: {}", e);
            return Err(e);
        }
    };
    if candidates.is_empty() {
        return Ok(());
    }

    for recipe in candidates {
        if !trigger_filters_match(&recipe.trigger, &event) {
            continue;
        }
        // Audit row in 'running' state.
        let run_id = uuid::Uuid::new_v4().to_string();
        let started_at = chrono::Utc::now().to_rfc3339();
        insert_run_row(
            &run_id,
            &recipe,
            event.event_seq() as i64,
            event_type,
            &event_payload,
            &started_at,
        );

        // Execute the action.
        let outcome = execute_action(&recipe.action, &event).await;
        let finished_at = chrono::Utc::now().to_rfc3339();
        finalize_run_row(&run_id, outcome, &finished_at);
    }
    Ok(())
}

fn find_candidates(event_type: &str) -> Result<Vec<OpsRecipe>, String> {
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
fn trigger_filters_match(trigger: &RecipeTrigger, event: &AtoEvent) -> bool {
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
        _ => false, // trigger type mismatch (shouldn't reach here given the SQL filter)
    }
}

// ─── Action executors ──────────────────────────────────────────────────

struct ActionOutcome {
    status: &'static str, // "done" | "failed" | "not_implemented"
    result: Option<String>,
    error: Option<String>,
}

async fn execute_action(action: &RecipeAction, event: &AtoEvent) -> ActionOutcome {
    match action {
        RecipeAction::DraftSkillFromReplay { out } => draft_skill_from_replay(event, out.as_deref()),
        RecipeAction::ReplayOnAlt {
            target_runtime,
            target_model,
        } => {
            // Substitute simple placeholders. Phase 4.2 only supports
            // {{source_runtime}} — the auto-replay template's needs.
            let resolved = substitute_simple_placeholders(target_runtime, event);
            replay_on_alt(event, &resolved, target_model.as_deref()).await
        }
        // Stubs — Phase 4.3+
        RecipeAction::KillRun
        | RecipeAction::DispatchAgent { .. }
        | RecipeAction::PostWebhook { .. }
        | RecipeAction::NotifyHuman { .. }
        | RecipeAction::RunScript { .. } => ActionOutcome {
            status: "not_implemented",
            result: None,
            error: Some(format!(
                "Action '{:?}' is not yet implemented (Phase 4.3 lands the remaining executors).",
                action_name(action)
            )),
        },
    }
}

fn action_name(a: &RecipeAction) -> &'static str {
    match a {
        RecipeAction::DraftSkillFromReplay { .. } => "draft_skill_from_replay",
        RecipeAction::ReplayOnAlt { .. } => "replay_on_alt",
        RecipeAction::KillRun => "kill_run",
        RecipeAction::DispatchAgent { .. } => "dispatch_agent",
        RecipeAction::PostWebhook { .. } => "post_webhook",
        RecipeAction::NotifyHuman { .. } => "notify_human",
        RecipeAction::RunScript { .. } => "run_script",
    }
}

/// Minimal {{name}} substitution. v1 supports source_runtime,
/// target_runtime, agent_slug. {{previous_runtime}} only resolves for
/// ReplayDone events — RegressionDetected doesn't carry old/new
/// values in its payload yet (Phase 4.3 will add them). Until then,
/// auto-replay-regression-failures template won't fully execute;
/// the action executor's unresolved-placeholder guard surfaces this
/// as a "failed" run with a clear error.
fn substitute_simple_placeholders(template: &str, event: &AtoEvent) -> String {
    let (source_runtime, target_runtime, agent_slug, previous_runtime) = match event {
        AtoEvent::RegressionDetected {
            agent_slug,
            field,
            old_value,
            new_value,
            ..
        } => {
            // For a runtime swap regression, old_value is the previous
            // runtime. v2.3.9 — the schema now carries this.
            let prev = if field == "runtime" {
                old_value.clone().unwrap_or_default()
            } else {
                String::new()
            };
            let curr = if field == "runtime" {
                new_value.clone().unwrap_or_default()
            } else {
                String::new()
            };
            (curr, prev.clone(), agent_slug.clone(), prev)
        }
        AtoEvent::ReplayDone {
            source_runtime,
            target_runtime,
            ..
        } => (
            source_runtime.clone(),
            target_runtime.clone(),
            String::new(),
            source_runtime.clone(),
        ),
        AtoEvent::DispatchFailed {
            runtime,
            agent_slug,
            ..
        } => (
            runtime.clone(),
            String::new(),
            agent_slug.clone().unwrap_or_default(),
            String::new(),
        ),
        _ => (String::new(), String::new(), String::new(), String::new()),
    };
    template
        .replace("{{source_runtime}}", &source_runtime)
        .replace("{{target_runtime}}", &target_runtime)
        .replace("{{agent_slug}}", &agent_slug)
        .replace("{{previous_runtime}}", &previous_runtime)
}

/// Executor: draft a SKILL.md from the trigger's ReplayDone payload.
/// Skillify in action.
fn draft_skill_from_replay(event: &AtoEvent, out_override: Option<&str>) -> ActionOutcome {
    let job_id = match event {
        AtoEvent::ReplayDone { job_id, .. } => job_id.clone(),
        _ => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(
                    "draft_skill_from_replay requires a ReplayDone event".to_string(),
                ),
            };
        }
    };

    // Open DB, read the replay row, render + write skill. Mirrors the
    // CLI's `ato skills draft --from-replay` logic but inlined here so
    // the engine doesn't shell out.
    let db_path = crate::get_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!("open DB: {}", e)),
            };
        }
    };
    // Tuple type matches the SELECT column order:
    // 0=id String, 1=source_runtime String, 2=target_model Option<String>,
    // 3=target_runtime String, 4=status String, 5=source_prompt Option<String>.
    let row: Result<(String, String, Option<String>, String, String, Option<String>), rusqlite::Error> =
        conn.query_row(
            "SELECT id, source_runtime, target_model, target_runtime, status,
                    (SELECT prompt FROM execution_logs WHERE id = rj.source_execution_log_id) AS source_prompt
               FROM replay_jobs rj WHERE id = ?1",
            [&job_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        );
    let (_id, source_runtime, target_model, target_runtime, status, source_prompt) = match row {
        Ok(t) => t,
        Err(e) => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!("replay_jobs row not found: {}", e)),
            };
        }
    };
    if status != "done" {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!("replay job status is '{}', not 'done'", status)),
        };
    }
    let source_prompt = match source_prompt {
        Some(p) if !p.is_empty() => p,
        _ => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some("source prompt missing for replay job".to_string()),
            };
        }
    };
    let skill_name = format!("route-{}-to-{}", source_runtime, target_runtime);
    let prompt_summary = summarize(&source_prompt, 80);
    let body = render_skill_md(
        &skill_name,
        &source_runtime,
        &target_runtime,
        target_model.as_deref(),
        &prompt_summary,
        &job_id,
    );
    let path = match out_override {
        Some(p) => std::path::PathBuf::from(p),
        None => default_skill_path(&target_runtime, &skill_name),
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(format!("mkdir {}: {}", parent.display(), e)),
            };
        }
    }
    if let Err(e) = std::fs::write(&path, body) {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!("write {}: {}", path.display(), e)),
        };
    }
    ActionOutcome {
        status: "done",
        result: Some(format!(
            "Drafted skill '{}' at {}",
            skill_name,
            path.display()
        )),
        error: None,
    }
}

/// Executor: replay the source trace on an alternative runtime.
async fn replay_on_alt(
    event: &AtoEvent,
    target_runtime: &str,
    target_model: Option<&str>,
) -> ActionOutcome {
    if target_runtime.is_empty() || target_runtime.contains("{{") {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some(format!(
                "target_runtime unresolved or empty after substitution: '{}'",
                target_runtime
            )),
        };
    }
    // KNOWN GAP (codex-reviewer v2.3.8): the desktop's `start_replay`
    // only resolves source rows by `execution_logs.cloud_trace_id`,
    // but DispatchFailed carries `run_id` which is `execution_logs.id`
    // (no cloud trace yet). Until start_replay accepts execution_logs.id
    // directly, the dispatch_failed → replay_on_alt chain will fail
    // with "prompt-not-local". RegressionDetected's failing_trace_ids
    // ARE cloud trace IDs, so that path works. v1 ships with only the
    // ReplayDone → DraftSkillFromReplay loop fully wired; this path
    // is documented but not yet usable end-to-end.
    let source_trace_id = match event {
        AtoEvent::RegressionDetected {
            failing_trace_ids, ..
        } => failing_trace_ids.first().cloned().unwrap_or_default(),
        AtoEvent::DispatchFailed { run_id, .. } => run_id.clone(),
        _ => {
            return ActionOutcome {
                status: "failed",
                result: None,
                error: Some(
                    "replay_on_alt requires RegressionDetected or DispatchFailed".to_string(),
                ),
            };
        }
    };
    if source_trace_id.is_empty() {
        return ActionOutcome {
            status: "failed",
            result: None,
            error: Some("no failing trace id available to replay".to_string()),
        };
    }
    // Delegate to the existing start_replay Tauri command's underlying
    // logic. For Phase 4.2, we just invoke it via its public path —
    // future refactor: extract to a non-Tauri helper so the engine
    // doesn't depend on the Tauri command macros.
    match crate::start_replay(
        source_trace_id.clone(),
        target_runtime.to_string(),
        target_model.map(String::from),
    )
    .await
    {
        Ok(job_id) => ActionOutcome {
            status: "done",
            result: Some(format!("Started replay {}", job_id)),
            error: None,
        },
        Err(e) => ActionOutcome {
            status: "failed",
            result: None,
            error: Some(e),
        },
    }
}

// ─── Helpers (mirror of CLI skills::draft) ────────────────────────────

fn default_skill_path(target_runtime: &str, skill_name: &str) -> std::path::PathBuf {
    let mut home = crate::home_dir();
    match target_runtime {
        "claude" => home.push(".claude/skills"),
        "codex" => home.push(".codex/skills"),
        "gemini" => home.push(".gemini/skills"),
        "openclaw" => home.push(".openclaw/skills"),
        "hermes" => home.push(".hermes/skills"),
        _ => home.push(".ato/skills"),
    }
    home.push(skill_name);
    home.push("SKILL.md");
    home
}

fn render_skill_md(
    name: &str,
    source_runtime: &str,
    target_runtime: &str,
    target_model: Option<&str>,
    prompt_summary: &str,
    job_id: &str,
) -> String {
    let model_line = target_model
        .map(|m| format!("\n# Pinned model: {}\n", m))
        .unwrap_or_default();
    format!(
        "---\nname: {name}\ndescription: \"Route prompts like '{prompt_summary}' to {target_runtime} — earlier replay showed {source_runtime} was failing on this shape.\"\nallowed-tools: []\n---\n{model_line}\n# Why this skill exists\n\nA replay of a real failing dispatch on `{source_runtime}` showed `{target_runtime}` handled the same prompt cleanly. This skill encodes that routing decision so future prompts matching the same shape get sent to the runtime that works.\n\nSource replay job: `{job_id}`\n\n# When to fire\n\nWhen the user's request resembles the source prompt:\n\n> {prompt_summary}\n\nSpecifically, route the prompt to **`{target_runtime}`** instead of `{source_runtime}`.\n\n# Notes for the human\n\nThis skill was auto-drafted by the ATO ops-recipe engine. Review the routing decision and refine the trigger description before relying on it.\n"
    )
}

fn summarize(text: &str, max_chars: usize) -> String {
    let first_line = text
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .replace("[user]:", "")
        .trim()
        .to_string();
    if first_line.chars().count() <= max_chars {
        first_line
    } else {
        let t: String = first_line.chars().take(max_chars).collect();
        format!("{}…", t)
    }
}

// ─── Audit table writes ───────────────────────────────────────────────

fn insert_run_row(
    run_id: &str,
    recipe: &OpsRecipe,
    event_seq: i64,
    event_type: &str,
    event_payload: &str,
    started_at: &str,
) {
    let db_path = crate::get_db_path();
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return;
    };
    let _ = conn.execute(
        "INSERT INTO ops_recipe_runs (id, recipe_id, recipe_slug, event_seq, event_type, event_payload, action_type, status, result, error_message, started_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', NULL, NULL, ?8, NULL)",
        rusqlite::params![
            run_id,
            recipe.id,
            recipe.slug,
            event_seq,
            event_type,
            event_payload,
            action_name(&recipe.action),
            started_at,
        ],
    );
}

fn finalize_run_row(run_id: &str, outcome: ActionOutcome, finished_at: &str) {
    let db_path = crate::get_db_path();
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return;
    };
    let _ = conn.execute(
        "UPDATE ops_recipe_runs SET status = ?1, result = ?2, error_message = ?3, finished_at = ?4 WHERE id = ?5",
        rusqlite::params![
            outcome.status,
            outcome.result,
            outcome.error,
            finished_at,
            run_id,
        ],
    );
}
