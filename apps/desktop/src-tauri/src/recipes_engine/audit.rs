// v2.7.14 — extracted from recipes_engine.rs (v2.8.0 split).
// Owns: ops_recipe_runs INSERT/UPDATE writes + action-name lookup
// used by both. Sibling modules call insert_run_row/finalize_run_row
// via pub(super).

use crate::recipes::{RecipeAction, OpsRecipe};
use super::actions::ActionOutcome;

pub(super) fn action_name(a: &RecipeAction) -> &'static str {
    match a {
        RecipeAction::DraftSkillFromReplay { .. } => "draft_skill_from_replay",
        RecipeAction::ReplayOnAlt { .. } => "replay_on_alt",
        RecipeAction::KillRun => "kill_run",
        RecipeAction::RequestApproval { .. } => "request_approval",
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
// ─── Audit table writes ───────────────────────────────────────────────

pub(super) fn insert_run_row(
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

pub(super) fn finalize_run_row(run_id: &str, outcome: ActionOutcome, finished_at: &str) {
    let db_path = crate::get_db_path();
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return;
    };
    // v2.3.19 Phase 5.4: RequestApproval's executor wrote status +
    // result + awaiting_approval_request_post_id atomically inside
    // its own transaction (closes the codex round-2 crash window).
    // Nothing left to do here for parked runs.
    if outcome.status == "awaiting_approval" {
        return;
    }
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
