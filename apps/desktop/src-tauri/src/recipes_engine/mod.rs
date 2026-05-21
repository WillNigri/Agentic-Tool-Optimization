// v2.3.8 Phase 4.2 — Ops recipe execution engine.
// v2.3.11 Phase 4.5 — PostWebhook executor added.
// v2.3.12 Phase 4.6 — RunScript executor added.
// v2.3.13 Phase 4.7 — DispatchLongRunning watcher + KillRun re-enable.
// v2.3.16 Phase 5.1 — NotifyHuman executor (writes to activity feed).
// v2.3.19 Phase 5.4 — RequestApproval executor + approval-resume watcher.
//
// Long-running tokio task that:
//   1. Subscribes to events::bus
//   2. For each event, queries ops_recipes for enabled recipes whose
//      trigger_type matches AND whose optional trigger filters match
//      the event payload
//   3. Runs each matching recipe's action
//   4. Audits the run to ops_recipe_runs
//
// Watchers spawned at start():
//   - long-running-dispatch watcher (Phase 4.7)
//   - approval-resume watcher (Phase 5.4) — parks runs go from
//     status='awaiting_approval' to 'approved'/'denied' when a
//     matching ApprovalDecision post lands.
//
// All eight action executors are implemented: DraftSkillFromReplay,
// ReplayOnAlt, DispatchAgent, PostWebhook, RunScript, KillRun,
// NotifyHuman, RequestApproval. No stubs remain.


// v2.7.14 (v2.8.0 ROADMAP) — formerly recipes_engine.rs (2245
// lines). Split into engine (this file) + triggers + placeholders
// + actions + audit per the dogfood war-room 726F8702-… (claude +
// minimax). Behavior unchanged; sibling-visibility achieved via
// pub(super) re-exports.
//
// mod.rs owns the runtime: spawned tokio tasks, the event loop,
// shared rate-limit + dedup state. Everything other than the
// entry-point + state lives in a sibling module.

mod triggers;
mod placeholders;
mod actions;
mod audit;

use crate::events::{bus, AtoEvent};
// Recipe types are consumed by triggers/actions sibling modules — not mod.rs.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use triggers::{find_candidates, poll_events_log, trigger_filters_match};
use actions::{execute_action, ActionOutcome};
use audit::{insert_run_row, finalize_run_row};


/// v2.3.10 — per-recipe rate-limit mutexes. Codex flagged the
/// previous check-then-insert as non-atomic: two concurrent
/// handle_event invocations could both observe count=9 and both fire.
/// Mutex<()> here serializes the "count + decide + insert" sequence
/// per recipe slug. Different recipes lock independently.
fn rate_limit_locks() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_for_slug(slug: &str) -> Arc<Mutex<()>> {
    let mut map = rate_limit_locks().lock().expect("rate-limit map poisoned");
    map.entry(slug.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// v2.3.13 Phase 4.7 — per-recipe set of recently-claimed event_seqs.
///
/// Codex round-1 caught that in-process bus::publish events hit BOTH
/// the live broadcast AND the events_log poll loop, so the same
/// (recipe, event) pair was processed twice. Round-2 caught that a
/// monotonic watermark would also drop legitimate OUT-OF-ORDER events
/// (lagged subscriber recovering older seqs from the ledger;
/// cross-process CLI events arriving after a higher-seq in-process
/// event).
///
/// Solution: per-slug bounded BTreeSet<u64>. claim_recipe_event
/// inserts the seq; returns true on first claim, false on duplicate.
/// The set is capped at SEEN_CAP entries — when over cap, the
/// smallest seq is evicted. At ~10 events/min and cap=256, that's
/// ~25min of retention — far longer than any plausible bus/poll
/// reorder window.
const SEEN_CAP: usize = 256;

fn recipe_seen_seqs() -> &'static Mutex<HashMap<String, std::collections::BTreeSet<u64>>> {
    static SEEN: OnceLock<Mutex<HashMap<String, std::collections::BTreeSet<u64>>>> =
        OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Claim (slug, seq) for processing. Returns true if newly claimed
/// (proceed); false if already in the recent-seen set (duplicate,
/// skip).
fn claim_recipe_event(slug: &str, seq: u64) -> bool {
    let mut map = recipe_seen_seqs().lock().expect("seen-seqs map poisoned");
    let set = map.entry(slug.to_string()).or_default();
    let newly = set.insert(seq);
    while set.len() > SEEN_CAP {
        // BTreeSet iter is sorted ascending — first() = smallest seq.
        let min = match set.iter().next() {
            Some(&m) => m,
            None => break,
        };
        set.remove(&min);
    }
    newly
}

/// Start the engine. Spawns THREE tokio tasks:
///   1. Live bus subscriber — fast path for in-process events
///   2. events_log poll loop — picks up cross-process events (CLI
///      dispatches publish there since they can't reach the in-memory
///      bus). Polls every 2s for new event_seqs.
///   3. Long-running watcher (v2.3.13 Phase 4.7) — scans active_runs
///      every 30s, emits DispatchLongRunning when an active run first
///      crosses each threshold any enabled recipe asked about. Lets
///      `kill_run` finally have an event variant it can act on.
///
/// The first two paths converge on handle_event, which dedupes via
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

    // v2.3.13 Phase 4.7 — long-running watcher. Scans active_runs
    // every 30s, computes the set of (runtime, threshold) pairs any
    // enabled recipe is subscribed to, and emits DispatchLongRunning
    // for each new (run_id, threshold) crossing. State lives in a
    // local HashMap; tracking is dropped when a run leaves the
    // registry (active_runs::finish_run removes the entry on
    // completion — no tombstone window).
    tauri::async_runtime::spawn(async move {
        let interval = std::time::Duration::from_secs(30);
        let mut emitted: std::collections::HashMap<String, std::collections::HashSet<u32>> =
            std::collections::HashMap::new();
        loop {
            tokio::time::sleep(interval).await;
            run_long_running_watcher_tick(&mut emitted).await;
        }
    });

    // v2.3.19 Phase 5.4 — approval resume watcher. Scans recipe runs
    // parked in status='awaiting_approval' every 5s. For each, looks
    // up the matching ApprovalDecision post via the request_post_id;
    // if found, updates ops_recipe_runs.status (approved|denied),
    // decision_post_id, finished_at. No retry / chained-action logic
    // for v1 — the post + audit row are the data, the GUI / future
    // recipes can act on them.
    tauri::async_runtime::spawn(async move {
        let interval = std::time::Duration::from_secs(5);
        loop {
            tokio::time::sleep(interval).await;
            run_approval_resume_tick();
        }
    });
}

/// One tick of the approval resume watcher. Cheap: a single SELECT
/// joining ops_recipe_runs to activity_posts, then per-row UPDATEs.
/// All read/write through a single short-lived connection.
fn run_approval_resume_tick() {
    let db_path = crate::get_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let _ = conn.busy_timeout(Duration::from_secs(2));

    let mut stmt = match conn.prepare(
        "SELECT r.id, r.awaiting_approval_request_post_id,
                d.id, json_extract(d.payload, '$.decision')
           FROM ops_recipe_runs r
           JOIN activity_posts d
             ON d.kind = 'approval_decision'
            AND json_extract(d.payload, '$.request_post_id') = r.awaiting_approval_request_post_id
          WHERE r.status = 'awaiting_approval'
            AND r.awaiting_approval_request_post_id IS NOT NULL",
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("recipes_engine: approval resume prepare failed: {}", e);
            return;
        }
    };
    let resolved: Vec<(String, String, String, String)> = match stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .and_then(|iter| iter.collect::<rusqlite::Result<Vec<_>>>())
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("recipes_engine: approval resume query failed: {}", e);
            return;
        }
    };

    for (run_id, _request_id, decision_post_id, decision) in resolved {
        // decision is the SQL-extracted value: "approved" or
        // "denied". Map it to the final run status.
        let final_status = match decision.as_str() {
            "approved" => "approved",
            "denied" => "denied",
            other => {
                eprintln!(
                    "recipes_engine: approval decision for run {} has unknown value '{}' — leaving parked",
                    run_id, other
                );
                continue;
            }
        };
        let finished_at = chrono::Utc::now().to_rfc3339();
        if let Err(e) = conn.execute(
            "UPDATE ops_recipe_runs
                SET status = ?1, decision = ?2, decision_post_id = ?3, finished_at = ?4
              WHERE id = ?5
                AND status = 'awaiting_approval'",
            rusqlite::params![final_status, final_status, decision_post_id, finished_at, run_id],
        ) {
            eprintln!(
                "recipes_engine: failed to resume run {}: {}",
                run_id, e
            );
        }
    }
}

/// v2.3.13 Phase 4.7 — One tick of the long-running watcher.
///
/// 1. Snapshot active_runs (cheap, single Mutex lock).
/// 2. Pull the set of distinct threshold_secs values referenced by
///    ENABLED OnDispatchLongRunning recipes. If nothing subscribes,
///    skip the tick — no point computing crossings nobody will react
///    to.
/// 3. For each (running run, threshold) pair where elapsed >=
///    threshold AND we haven't emitted yet, publish DispatchLongRunning.
/// 4. Garbage-collect emission state for runs no longer in the registry.
///
/// The `emitted` map persists across ticks: key = run_id, value = set
/// of thresholds already emitted for that run. Resets when a run drops
/// from active_runs.
async fn run_long_running_watcher_tick(
    emitted: &mut std::collections::HashMap<String, std::collections::HashSet<u32>>,
) {
    let runs = crate::active_runs::list_runs();
    let active_run_ids: std::collections::HashSet<String> = runs
        .iter()
        .filter(|r| r.status == "running")
        .map(|r| r.run_id.clone())
        .collect();
    // GC tracking for runs no longer in the registry.
    emitted.retain(|run_id, _| active_run_ids.contains(run_id));

    let thresholds = match active_thresholds_from_db() {
        Ok(ts) if !ts.is_empty() => ts,
        _ => return,
    };

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    for run in runs.into_iter().filter(|r| r.status == "running") {
        let elapsed = now_unix.saturating_sub(run.started_at_unix);
        for &t in &thresholds {
            if elapsed < t as u64 {
                continue;
            }
            let set = emitted.entry(run.run_id.clone()).or_default();
            if !set.insert(t) {
                continue; // already emitted for this (run, threshold)
            }
            // Codex round-1 important: re-check the run is still
            // active right before publishing. active_runs has no
            // tombstone — `finish_run` removes immediately — so a
            // run that finished between snapshot and now would
            // otherwise produce a phantom event that notify/webhook
            // recipes would still act on.
            let still_running = crate::active_runs::list_runs()
                .iter()
                .any(|r| r.run_id == run.run_id && r.status == "running");
            if !still_running {
                continue;
            }
            let seq = crate::events::bus::next_seq();
            let event = crate::events::AtoEvent::DispatchLongRunning {
                event_seq: seq,
                run_id: run.run_id.clone(),
                runtime: run.runtime.clone(),
                agent_slug: run.agent_slug.clone(),
                started_at_unix: run.started_at_unix,
                elapsed_secs: elapsed,
                threshold_secs: t,
            };
            crate::events::bus::publish(event);
        }
    }
}

/// Read distinct threshold_secs values from enabled
/// OnDispatchLongRunning recipes. Codex round-1 nit: deserialize
/// through the typed RecipeTrigger so schema changes (added fields,
/// nesting) fail loudly instead of silently making the watcher stop
/// emitting.
fn active_thresholds_from_db() -> Result<Vec<u32>, String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    let _ = conn.busy_timeout(Duration::from_millis(500));
    let mut stmt = conn
        .prepare(
            "SELECT slug, trigger_config FROM ops_recipes
              WHERE enabled = 1 AND trigger_type = 'on_dispatch_long_running'",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut out: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for (slug, tj) in rows {
        match serde_json::from_str::<crate::recipes::RecipeTrigger>(&tj) {
            Ok(crate::recipes::RecipeTrigger::OnDispatchLongRunning {
                threshold_secs,
                ..
            }) => {
                if threshold_secs > 0 {
                    out.insert(threshold_secs);
                }
            }
            Ok(_) => {
                // Wrong trigger variant for this row — shouldn't be
                // reachable given the SQL filter.
            }
            Err(e) => {
                eprintln!(
                    "recipes_engine: malformed trigger_config for enabled recipe @{}: {}",
                    slug, e
                );
            }
        }
    }
    let mut sorted: Vec<u32> = out.into_iter().collect();
    sorted.sort();
    Ok(sorted)
}

/// Read events_log rows with event_seq > since, in seq order. Returns
/// the parsed events and the highest event_seq seen (or `since` if no
/// new rows). Best-effort: a locked DB returns no rows and the next
/// tick retries.

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

        // v2.3.10 Phase 4.4 — per-recipe rate limit. Catches infinite
        // recursion (action→event→same recipe) and general runaway
        // recipes. 10 successful/failed runs in any 60s window per
        // recipe. Rate-limited rows are NOT counted toward the next
        // window's quota (codex #3 from the 4.4 review).
        //
        // Atomicity: the check-then-insert is serialized per recipe
        // via a slug-keyed Mutex (codex #2). Different recipes lock
        // independently so unrelated triggers stay parallel.
        let slug_lock = lock_for_slug(&recipe.slug);
        let _serialize = slug_lock.lock().expect("recipe rate-limit lock poisoned");

        // v2.3.13 Phase 4.7 — dedupe across the live-bus and
        // events_log-poll paths. Both call handle_event for the same
        // in-process event; without this, every recipe would fire
        // twice. The watermark check inside the per-slug lock means
        // the two arrivals race for who gets to claim — exactly one
        // wins and proceeds, the other returns immediately.
        if !claim_recipe_event(&recipe.slug, event.event_seq()) {
            continue;
        }

        if let Some(count) = runs_in_window_executed_only(&recipe.slug, 60) {
            if count >= 10 {
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
                finalize_run_row(
                    &run_id,
                    ActionOutcome {
                        status: "rate_limited",
                        result: None,
                        error: Some(format!(
                            "recipe @{} hit the rate limit ({} executed runs in the last 60s)",
                            recipe.slug, count
                        )),
                    },
                    &started_at,
                );
                // Drop the lock before continuing.
                drop(_serialize);
                continue;
            }
        }

        // Insert the audit row in 'running' state BEFORE releasing the
        // lock — that way concurrent invocations see this run in
        // runs_in_window_executed_only and back off correctly.
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
        drop(_serialize);

        // Execute the action.
        let outcome = execute_action(&recipe.action, &event, &recipe.slug, &run_id).await;
        let finished_at = chrono::Utc::now().to_rfc3339();
        finalize_run_row(&run_id, outcome, &finished_at);
    }
    Ok(())
}

/// Count ops_recipe_runs rows for `slug` in the last `window_secs`
/// seconds, EXCLUDING rate_limited rows (so blocked attempts don't
/// extend the block window — codex #3 from the 4.4 review).
fn runs_in_window_executed_only(slug: &str, window_secs: i64) -> Option<i64> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path).ok()?;
    let _ = conn.busy_timeout(Duration::from_millis(500));
    let cutoff =
        (chrono::Utc::now() - chrono::Duration::seconds(window_secs)).to_rfc3339();
    conn.query_row(
        "SELECT COUNT(*) FROM ops_recipe_runs WHERE recipe_slug = ?1 AND started_at > ?2 AND status != 'rate_limited'",
        rusqlite::params![slug, cutoff],
        |r| r.get::<_, i64>(0),
    )
    .ok()
}


