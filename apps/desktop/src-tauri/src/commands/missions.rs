// commands/missions.rs — v2.16 PR-7: Mission-control board Tauri commands.
//
// Mirrors the CLI's missions.rs read + mutation surface for the desktop.
// Out of scope (CLI-side only): dispatch, tick, check, merge, worktree
// create/cleanup. set_state records the state change + event exactly as
// the CLI does; worktree cleanup is noted in a comment and deferred to the
// CLI path as the spec requires.
//
// Parameterized SQL throughout — no string interpolation of user values.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::fs;
use tauri::State;
use uuid::Uuid;

use crate::DbState;

// ── Validation constants — mirror CLI exactly ─────────────────────────

const VALID_CATEGORIES: &[&str] = &["autonomous", "needs_owner", "ignored", "done"];
const VALID_STATES: &[&str] = &["open", "in_progress", "blocked", "complete"];

// ── Types ─────────────────────────────────────────────────────────────

/// Lightweight card data returned by missions_list.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MissionSummary {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub goal: String,
    pub state: String,
    pub category: String,
    pub workspace_strategy: String,
    pub merge_strategy: String,
    pub max_loops: Option<i64>,
    pub token_budget_usd: Option<f64>,
    /// Aggregate cost from dispatched + loop events (mirrors CLI sum_cost_for_mission UNION ALL).
    pub spent_usd: f64,
    /// Count of dispatched + loop_run_completed events.
    pub dispatch_count: i64,
    pub updated_at: String,
}

/// Full mission detail row (mirrors MissionRow in CLI).
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MissionDetail {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub goal: String,
    pub success_criteria: serde_json::Value,
    pub escalation_policy: Option<serde_json::Value>,
    pub workspace_strategy: String,
    pub base_sha: Option<String>,
    pub cleanup_policy: String,
    pub merge_strategy: String,
    pub category: String,
    pub state: String,
    pub max_loops: Option<i64>,
    pub token_budget_usd: Option<f64>,
    pub result_metadata: Option<serde_json::Value>,
    pub narrative_md_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub repo_root: Option<String>,
    pub worker_config: Option<serde_json::Value>,
    /// Computed fields.
    pub spent_usd: f64,
    pub dispatch_count: i64,
    /// Last 50 events newest-first.
    pub events: Vec<MissionEvent>,
    /// Narrative markdown body; None when the sidecar file is missing.
    pub narrative_body: Option<String>,
    /// Pending escalations: 'escalated' events not followed by an
    /// owner_decision or state_changed event for the same mission.
    pub pending_escalations: Vec<MissionEvent>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MissionEvent {
    pub id: String,
    pub mission_id: String,
    pub kind: String,
    pub payload: Option<serde_json::Value>,
    pub occurred_at: String,
}

// ── SELECT constant — matches CLI MISSION_SELECT column order ─────────

const MISSION_COLS: &str =
    "id, slug, name, goal, success_criteria, escalation_policy,
     workspace_strategy, base_sha, cleanup_policy, merge_strategy,
     category, state, max_loops, token_budget_usd, result_metadata,
     narrative_md_path, created_at, updated_at, repo_root, worker_config";

// ── Internal helpers ──────────────────────────────────────────────────

fn id_or_slug_column(input: &str) -> &'static str {
    if Uuid::parse_str(input).is_ok() {
        "id"
    } else {
        "slug"
    }
}

fn parse_json_opt(raw: Option<String>) -> Option<serde_json::Value> {
    raw.and_then(|s| serde_json::from_str(&s).ok())
}

fn validate_enum(name: &str, value: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "invalid {}: '{}' (expected {})",
            name,
            value,
            allowed.join("|")
        ))
    }
}

/// Aggregate cost across dispatched + loop_run_completed events.
/// Mirrors CLI sum_cost_for_mission UNION ALL exactly.
fn sum_cost_for_mission(conn: &rusqlite::Connection, mission_id: &str) -> Result<f64, String> {
    let total: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(cost), 0.0) FROM (
               -- Leg 1: single-dispatch events
               SELECT el.cost_usd_estimated AS cost
                 FROM mission_events me
                 JOIN execution_logs el
                   ON json_extract(me.payload, '$.execution_log_id') = el.id
                WHERE me.mission_id = ?1
                  AND me.kind = 'dispatched'
               UNION ALL
               -- Leg 2: loop events — sum all steps' execution_logs costs
               SELECT el.cost_usd_estimated AS cost
                 FROM mission_events me
                 JOIN loop_run_steps lrs
                   ON lrs.loop_run_id = json_extract(me.payload, '$.loop_run_id')
                 JOIN execution_logs el
                   ON el.id = lrs.execution_log_id
                WHERE me.mission_id = ?1
                  AND me.kind = 'loop_run_completed'
             )",
            params![mission_id],
            |r| r.get::<_, Option<f64>>(0),
        )
        .map_err(|e| e.to_string())?
        .unwrap_or(0.0);
    Ok(total)
}

fn count_dispatches(conn: &rusqlite::Connection, mission_id: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM mission_events
          WHERE mission_id = ?1 AND kind IN ('dispatched', 'loop_run_completed')",
        params![mission_id],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

/// Load events newest-first up to `limit`.
fn load_events(
    conn: &rusqlite::Connection,
    mission_id: &str,
    limit: i64,
) -> Result<Vec<MissionEvent>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, mission_id, kind, payload, occurred_at
               FROM mission_events
              WHERE mission_id = ?1
           ORDER BY occurred_at DESC
              LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![mission_id, limit], |r| {
            let payload_raw: Option<String> = r.get(3)?;
            Ok(MissionEvent {
                id: r.get(0)?,
                mission_id: r.get(1)?,
                kind: r.get(2)?,
                payload: parse_json_opt(payload_raw),
                occurred_at: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Return pending escalations: 'escalated' events that are NOT followed
/// by a 'state_changed' or 'owner_decision' event with a later occurred_at.
fn pending_escalations(
    conn: &rusqlite::Connection,
    mission_id: &str,
) -> Result<Vec<MissionEvent>, String> {
    // Load all events chronologically to compute the "resolved" set without
    // a correlated sub-query (avoids SQLite quirks with correlated EXISTS on text dates).
    let mut stmt = conn
        .prepare(
            "SELECT id, mission_id, kind, payload, occurred_at
               FROM mission_events
              WHERE mission_id = ?1
           ORDER BY occurred_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![mission_id], |r| {
            let payload_raw: Option<String> = r.get(3)?;
            Ok(MissionEvent {
                id: r.get(0)?,
                mission_id: r.get(1)?,
                kind: r.get(2)?,
                payload: parse_json_opt(payload_raw),
                occurred_at: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut events: Vec<MissionEvent> = Vec::new();
    for r in rows {
        events.push(r.map_err(|e| e.to_string())?);
    }

    // PR-6 R1 alignment: an escalated event is pending iff there is NO
    // later 'owner_decision' or 'state_changed' resolution event AT ALL —
    // any single resolution clears every prior brief regardless of count.
    // Mirrors the CLI `ato missions briefs` filter and `escalation_is_pending`
    // semantics so CLI and desktop never disagree on the pending set.
    let last_resolution_ts: Option<&str> = events
        .iter()
        .filter(|ev| ev.kind == "owner_decision" || ev.kind == "state_changed")
        .map(|ev| ev.occurred_at.as_str())
        .max();
    let pending: Vec<MissionEvent> = events
        .iter()
        .filter(|ev| ev.kind == "escalated")
        .filter(|ev| match last_resolution_ts {
            None => true,
            Some(ts) => ev.occurred_at.as_str() > ts,
        })
        .cloned()
        .collect();
    Ok(pending)
}

fn insert_event(
    conn: &rusqlite::Connection,
    mission_id: &str,
    kind: &str,
    payload: Option<serde_json::Value>,
    occurred_at: &str,
) -> Result<(), String> {
    let payload_str = match payload.as_ref() {
        Some(v) => Some(serde_json::to_string(v).map_err(|e| e.to_string())?),
        None => None,
    };
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO mission_events (id, mission_id, kind, payload, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, mission_id, kind, payload_str, occurred_at],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Tauri commands ────────────────────────────────────────────────────

/// List missions with optional state/category filters.
/// Returns MissionSummary cards (lightweight — no events, no narrative).
#[tauri::command]
pub fn missions_list(
    db: State<'_, DbState>,
    state_filter: Option<String>,
    category_filter: Option<String>,
) -> Result<Vec<MissionSummary>, String> {
    if let Some(s) = state_filter.as_deref() {
        validate_enum("state", s, VALID_STATES)?;
    }
    if let Some(c) = category_filter.as_deref() {
        validate_enum("category", c, VALID_CATEGORIES)?;
    }

    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Build WHERE clause dynamically (same pattern as CLI run_list).
    let sql = match (&state_filter, &category_filter) {
        (Some(_), Some(_)) => format!(
            "SELECT {} FROM missions WHERE state = ?1 AND category = ?2 ORDER BY updated_at DESC",
            MISSION_COLS
        ),
        (Some(_), None) => format!(
            "SELECT {} FROM missions WHERE state = ?1 ORDER BY updated_at DESC",
            MISSION_COLS
        ),
        (None, Some(_)) => format!(
            "SELECT {} FROM missions WHERE category = ?1 ORDER BY updated_at DESC",
            MISSION_COLS
        ),
        (None, None) => format!(
            "SELECT {} FROM missions ORDER BY updated_at DESC",
            MISSION_COLS
        ),
    };

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let map_row2 = |r: &rusqlite::Row| -> rusqlite::Result<MissionSummary> {
        let id: String = r.get(0)?;
        let slug: String = r.get(1)?;
        let name: String = r.get(2)?;
        let goal: String = r.get(3)?;
        let category: String = r.get(10)?;
        let state: String = r.get(11)?;
        let workspace_strategy: String = r.get(6)?;
        let merge_strategy: String = r.get(9)?;
        let max_loops: Option<i64> = r.get(12)?;
        let token_budget_usd: Option<f64> = r.get(13)?;
        let updated_at: String = r.get(17)?;
        Ok(MissionSummary {
            id,
            slug,
            name,
            goal,
            state,
            category,
            workspace_strategy,
            merge_strategy,
            max_loops,
            token_budget_usd,
            spent_usd: 0.0,    // filled below
            dispatch_count: 0, // filled below
            updated_at,
        })
    };

    let rows_iter: Box<dyn Iterator<Item = rusqlite::Result<MissionSummary>>> =
        match (&state_filter, &category_filter) {
            (Some(s), Some(c)) => Box::new(stmt.query_map(params![s, c], map_row2).map_err(|e| e.to_string())?),
            (Some(s), None) => Box::new(stmt.query_map(params![s], map_row2).map_err(|e| e.to_string())?),
            (None, Some(c)) => Box::new(stmt.query_map(params![c], map_row2).map_err(|e| e.to_string())?),
            (None, None) => Box::new(stmt.query_map([], map_row2).map_err(|e| e.to_string())?),
        };

    let mut out = Vec::new();
    for r in rows_iter {
        let mut summary = r.map_err(|e| e.to_string())?;
        summary.spent_usd = sum_cost_for_mission(&conn, &summary.id).unwrap_or(0.0);
        summary.dispatch_count = count_dispatches(&conn, &summary.id).unwrap_or(0);
        out.push(summary);
    }
    Ok(out)
}

/// Return full detail for a single mission (slug or UUID id).
#[tauri::command]
pub fn mission_detail(db: State<'_, DbState>, slug_or_id: String) -> Result<MissionDetail, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let col = id_or_slug_column(&slug_or_id);
    let sql = format!(
        "SELECT {} FROM missions WHERE {} = ?1",
        MISSION_COLS, col
    );
    let row = conn
        .query_row(&sql, params![slug_or_id], |r| {
            let sc_str: String = r.get(4)?;
            let success_criteria: serde_json::Value =
                serde_json::from_str(&sc_str).unwrap_or(serde_json::json!([]));
            let escalation_policy = parse_json_opt(r.get(5)?);
            let result_metadata = parse_json_opt(r.get(14)?);
            let worker_config = parse_json_opt(r.get(19).ok().unwrap_or(None));
            Ok(MissionDetail {
                id: r.get(0)?,
                slug: r.get(1)?,
                name: r.get(2)?,
                goal: r.get(3)?,
                success_criteria,
                escalation_policy,
                workspace_strategy: r.get(6)?,
                base_sha: r.get(7)?,
                cleanup_policy: r.get(8)?,
                merge_strategy: r.get(9)?,
                category: r.get(10)?,
                state: r.get(11)?,
                max_loops: r.get(12)?,
                token_budget_usd: r.get(13)?,
                result_metadata,
                narrative_md_path: r.get(15)?,
                created_at: r.get(16)?,
                updated_at: r.get(17)?,
                repo_root: r.get(18).ok().unwrap_or(None),
                worker_config,
                spent_usd: 0.0,
                dispatch_count: 0,
                events: Vec::new(),
                narrative_body: None,
                pending_escalations: Vec::new(),
            })
        })
        .map_err(|e| format!("mission not found: {}", e))?;

    let mission_id = row.id.clone();
    let narrative_path = row.narrative_md_path.clone();

    let spent_usd = sum_cost_for_mission(&conn, &mission_id).unwrap_or(0.0);
    let dispatch_count = count_dispatches(&conn, &mission_id).unwrap_or(0);
    let events = load_events(&conn, &mission_id, 50)?;
    let narrative_body = fs::read_to_string(&narrative_path).ok();
    let pending = pending_escalations(&conn, &mission_id)?;

    Ok(MissionDetail {
        spent_usd,
        dispatch_count,
        events,
        narrative_body,
        pending_escalations: pending,
        ..row
    })
}

/// Change mission category — mirrors CLI run_set_category exactly
/// (same enums, same category_changed event payload shape {from, to}).
#[tauri::command]
pub fn mission_set_category(
    db: State<'_, DbState>,
    slug_or_id: String,
    category: String,
) -> Result<MissionDetail, String> {
    validate_enum("category", &category, VALID_CATEGORIES)?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Load current row to check for no-op and to get current category.
    let col = id_or_slug_column(&slug_or_id);
    let load_sql = format!("SELECT id, slug, category FROM missions WHERE {} = ?1", col);
    let (id, slug, current_category): (String, String, String) = conn
        .query_row(&load_sql, params![slug_or_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(|e| format!("mission not found: {}", e))?;

    if current_category == category {
        drop(conn);
        return mission_detail(db, id);
    }

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE missions SET category = ?1, updated_at = ?2 WHERE id = ?3",
        params![category, now, id],
    )
    .map_err(|e| e.to_string())?;
    insert_event(
        &conn,
        &id,
        "category_changed",
        Some(serde_json::json!({
            "from": current_category,
            "to": category,
        })),
        &now,
    )?;

    let _ = slug; // captured for future human-mode logging
    drop(conn);
    mission_detail(db, id)
}

/// Change mission state — mirrors CLI run_set_state + transition_state exactly,
/// INCLUDING worktree cleanup on transition to 'complete'. Codex PR-7 R1 [HIGH]
/// fix: not porting cleanup created CLI/UI parity divergence — users who only
/// touched the GUI were left with stale worktree dirs. Cleanup uses
/// std::process::Command::new("git") just like the CLI; we tolerate failures
/// (record worktree_cleanup_failed) instead of failing the state transition.
#[tauri::command]
pub fn mission_set_state(
    db: State<'_, DbState>,
    slug_or_id: String,
    state: String,
) -> Result<MissionDetail, String> {
    validate_enum("state", &state, VALID_STATES)?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let col = id_or_slug_column(&slug_or_id);
    let load_sql = format!(
        "SELECT id, slug, state, cleanup_policy, repo_root FROM missions WHERE {} = ?1",
        col
    );
    let (id, slug, current_state, cleanup_policy, repo_root): (
        String,
        String,
        String,
        String,
        Option<String>,
    ) = conn
        .query_row(&load_sql, params![slug_or_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(|e| format!("mission not found: {}", e))?;

    if current_state == state {
        drop(conn);
        return mission_detail(db, id);
    }

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE missions SET state = ?1, updated_at = ?2 WHERE id = ?3",
        params![state, now, id],
    )
    .map_err(|e| e.to_string())?;

    // Event payload mirrors CLI transition_state: {from, to} — no reason on
    // manual GUI changes (reason is supplied by the coordinator tick).
    insert_event(
        &conn,
        &id,
        "state_changed",
        Some(serde_json::json!({
            "from": current_state,
            "to": state,
        })),
        &now,
    )?;

    // Cleanup on transition to 'complete' — mirrors CLI cleanup_mission_worktrees.
    if state == "complete" {
        if let Err(err) = cleanup_mission_worktrees_inline(
            &conn,
            &id,
            &slug,
            &cleanup_policy,
            repo_root.as_deref(),
        ) {
            // Record but don't fail the transition (parity with CLI's
            // transition_state which surfaces the cleanup_warning).
            let _ = insert_event(
                &conn,
                &id,
                "worktree_cleanup_failed",
                Some(serde_json::json!({ "error": err })),
                &chrono::Utc::now().to_rfc3339(),
            );
        }
    }

    drop(conn);
    mission_detail(db, id)
}

/// Inline port of the CLI's cleanup_mission_worktrees for the desktop path.
/// Removes the integration worktree and any agent worktrees that exist on
/// disk, per `cleanup_policy`. Inserts a `worktree_cleaned` event per
/// successful removal. Returns Err(message) on a non-fatal error so the
/// caller can record a `worktree_cleanup_failed` event without dropping the
/// state change.
fn cleanup_mission_worktrees_inline(
    conn: &rusqlite::Connection,
    mission_id: &str,
    mission_slug: &str,
    cleanup_policy: &str,
    repo_root: Option<&str>,
) -> Result<(), String> {
    // Decide whether to act. transition_state always passes new_state="complete"
    // here so retain/delete_on_success/always_delete all behave identically: only
    // 'retain' skips.
    if cleanup_policy == "retain" {
        return Ok(());
    }
    let delete_branches = cleanup_policy == "always_delete";

    let repo_root = match repo_root {
        Some(r) => r.to_string(),
        None => return Ok(()), // single_cwd mission, nothing to clean
    };

    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| "no HOME / USERPROFILE in env".to_string())?;
    let base = std::path::PathBuf::from(&home)
        .join(".ato")
        .join("missions")
        .join(mission_slug);
    let wt_root = base.join("worktrees");
    let integration_wt = base.join("integration");

    let now = chrono::Utc::now().to_rfc3339();

    // Integration worktree first.
    if integration_wt.exists() {
        let int_branch = format!("ato/mission/{}/integration", mission_slug);
        let int_path_str = integration_wt.to_string_lossy().to_string();
        let rm_out = std::process::Command::new("git")
            .args(["-C", &repo_root, "worktree", "remove", "--force"])
            .arg(&integration_wt)
            .output()
            .ok();
        let rm_ok = rm_out
            .map(|o| {
                o.status.success()
                    || String::from_utf8_lossy(&o.stderr).contains("is not a working tree")
            })
            .unwrap_or(false);
        if rm_ok {
            if delete_branches {
                let _ = std::process::Command::new("git")
                    .args(["-C", &repo_root, "branch", "-D", &int_branch])
                    .output();
            }
            let _ = insert_event(
                conn,
                mission_id,
                "worktree_cleaned",
                Some(serde_json::json!({
                    "path": int_path_str,
                    "branch": int_branch,
                    "policy": cleanup_policy,
                    "trigger": "state_transition",
                    "branch_deleted": delete_branches,
                    "integration": true,
                })),
                &now,
            );
        }
    }

    if !wt_root.exists() {
        return Ok(());
    }

    let entries =
        fs::read_dir(&wt_root).map_err(|e| format!("read_dir {}: {}", wt_root.display(), e))?;
    for entry in entries {
        // String matches CLI cleanup_mission_worktrees for byte-for-byte
        // parity on worktree_cleanup_failed event payloads (R2 [LOW] fix).
        let entry = entry.map_err(|e| format!("read worktree dir entry: {}", e))?;
        let wt_path = entry.path();
        if !wt_path.is_dir() {
            continue;
        }
        let path_str = wt_path.to_string_lossy().to_string();
        let dir_name = entry.file_name().to_string_lossy().to_string();
        let branch = format!("ato/mission/{}/{}", mission_slug, dir_name);

        let rm_out = std::process::Command::new("git")
            .args(["-C", &repo_root, "worktree", "remove", "--force"])
            .arg(&wt_path)
            .output()
            .map_err(|e| format!("git worktree remove {}: {}", wt_path.display(), e))?;

        let rm_ok = rm_out.status.success()
            || String::from_utf8_lossy(&rm_out.stderr).contains("is not a working tree");

        if rm_ok {
            if delete_branches {
                let _ = std::process::Command::new("git")
                    .args(["-C", &repo_root, "branch", "-D", &branch])
                    .output();
            }
            let _ = insert_event(
                conn,
                mission_id,
                "worktree_cleaned",
                Some(serde_json::json!({
                    "path": path_str,
                    "branch": branch,
                    "policy": cleanup_policy,
                    "trigger": "state_transition",
                    "branch_deleted": delete_branches,
                })),
                &now,
            );
        }
    }

    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn make_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // Minimal schema matching prod migrations for missions + events.
        conn.execute_batch(
            "CREATE TABLE missions (
                id TEXT PRIMARY KEY,
                slug TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                goal TEXT NOT NULL,
                success_criteria TEXT NOT NULL DEFAULT '[]',
                escalation_policy TEXT,
                workspace_strategy TEXT NOT NULL DEFAULT 'single_cwd',
                base_sha TEXT,
                cleanup_policy TEXT NOT NULL DEFAULT 'delete_on_success',
                merge_strategy TEXT NOT NULL DEFAULT 'human_approves_each',
                category TEXT NOT NULL DEFAULT 'autonomous',
                state TEXT NOT NULL DEFAULT 'open',
                max_loops INTEGER,
                token_budget_usd REAL,
                result_metadata TEXT,
                narrative_md_path TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                repo_root TEXT,
                worker_config TEXT
            );
            CREATE TABLE mission_events (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                payload TEXT,
                occurred_at TEXT NOT NULL
            );
            CREATE TABLE execution_logs (
                id TEXT PRIMARY KEY,
                runtime TEXT,
                cost_usd_estimated REAL,
                created_at TEXT
            );
            CREATE TABLE loop_run_steps (
                id TEXT PRIMARY KEY,
                loop_run_id TEXT,
                execution_log_id TEXT
            );",
        )
        .unwrap();
        conn
    }

    fn insert_mission(conn: &Connection, id: &str, slug: &str, state: &str, category: &str) {
        let now = "2026-01-01T00:00:00Z";
        conn.execute(
            "INSERT INTO missions (id, slug, name, goal, created_at, updated_at, state, category)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7)",
            params![id, slug, slug, "Test goal", now, state, category],
        )
        .unwrap();
    }

    fn insert_event_row(conn: &Connection, mission_id: &str, kind: &str) {
        let id = Uuid::new_v4().to_string();
        let now = "2026-01-01T00:00:00Z";
        conn.execute(
            "INSERT INTO mission_events (id, mission_id, kind, occurred_at) VALUES (?1,?2,?3,?4)",
            params![id, mission_id, kind, now],
        )
        .unwrap();
    }

    fn insert_event_row_at(conn: &Connection, mission_id: &str, kind: &str, ts: &str) {
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO mission_events (id, mission_id, kind, occurred_at) VALUES (?1,?2,?3,?4)",
            params![id, mission_id, kind, ts],
        )
        .unwrap();
    }

    // Wrap Connection in DbState shape for testing helpers directly.
    fn wrap(conn: Connection) -> tauri::State<'static, DbState> {
        // We test the SQL logic directly against Connection, not via Tauri State,
        // to avoid the AppHandle dependency. Tests call the helpers directly.
        let _ = conn; // conn is used through helper fns below
        unreachable!("use helper fns directly in unit tests")
    }

    #[test]
    fn test_id_or_slug_column_uuid() {
        let uuid = Uuid::new_v4().to_string();
        assert_eq!(id_or_slug_column(&uuid), "id");
    }

    #[test]
    fn test_id_or_slug_column_slug() {
        assert_eq!(id_or_slug_column("my-mission"), "slug");
    }

    #[test]
    fn test_validate_enum_valid() {
        assert!(validate_enum("category", "autonomous", VALID_CATEGORIES).is_ok());
        assert!(validate_enum("state", "complete", VALID_STATES).is_ok());
    }

    #[test]
    fn test_validate_enum_invalid() {
        let err = validate_enum("state", "bogus", VALID_STATES).unwrap_err();
        assert!(err.contains("bogus"));
        assert!(err.contains("open|in_progress|blocked|complete"));
    }

    #[test]
    fn test_count_dispatches_empty() {
        let conn = make_db();
        insert_mission(&conn, "m1", "slug-1", "open", "autonomous");
        let count = count_dispatches(&conn, "m1").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_count_dispatches_increments_on_dispatched_and_loop_events() {
        let conn = make_db();
        insert_mission(&conn, "m1", "slug-1", "open", "autonomous");
        insert_event_row(&conn, "m1", "dispatched");
        insert_event_row(&conn, "m1", "dispatched");
        insert_event_row(&conn, "m1", "loop_run_completed");
        insert_event_row(&conn, "m1", "state_changed"); // should NOT count
        let count = count_dispatches(&conn, "m1").unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_sum_cost_empty_returns_zero() {
        let conn = make_db();
        insert_mission(&conn, "m1", "slug-1", "open", "autonomous");
        let cost = sum_cost_for_mission(&conn, "m1").unwrap();
        assert!((cost - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_pending_escalations_no_events() {
        let conn = make_db();
        insert_mission(&conn, "m1", "slug-1", "open", "autonomous");
        let pending = pending_escalations(&conn, "m1").unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn test_pending_escalations_resolved_by_state_changed() {
        let conn = make_db();
        insert_mission(&conn, "m1", "slug-1", "open", "autonomous");
        insert_event_row(&conn, "m1", "escalated");
        insert_event_row(&conn, "m1", "state_changed");
        let pending = pending_escalations(&conn, "m1").unwrap();
        assert!(pending.is_empty(), "state_changed should resolve the escalation");
    }

    #[test]
    fn test_pending_escalations_unresolved_remains() {
        let conn = make_db();
        insert_mission(&conn, "m1", "slug-1", "open", "autonomous");
        insert_event_row(&conn, "m1", "escalated");
        let pending = pending_escalations(&conn, "m1").unwrap();
        assert_eq!(pending.len(), 1);
    }

    /// PR-6 R1 codex-flagged scenario: two escalated events followed by a
    /// single resolution event must yield ZERO pending (any later
    /// owner_decision/state_changed clears ALL prior briefs — matches the
    /// CLI `ato missions briefs` filter so the two surfaces never disagree).
    /// Locks down the alignment fix.
    #[test]
    fn test_pending_escalations_two_briefs_one_resolution_clears_all() {
        let conn = make_db();
        insert_mission(&conn, "m1", "slug-1", "open", "autonomous");
        // Use distinct timestamps so the ORDER BY occurred_at chain is stable.
        insert_event_row_at(&conn, "m1", "escalated", "2026-06-13T00:00:01Z");
        insert_event_row_at(&conn, "m1", "escalated", "2026-06-13T00:00:02Z");
        insert_event_row_at(&conn, "m1", "state_changed", "2026-06-13T00:00:03Z");
        let pending = pending_escalations(&conn, "m1").unwrap();
        assert!(
            pending.is_empty(),
            "any later resolution must clear ALL prior briefs (got {} pending)",
            pending.len(),
        );
    }

    /// And the inverse: a brief AFTER the most recent resolution must
    /// stay pending. Catches a future regression that would over-clear.
    #[test]
    fn test_pending_escalations_brief_after_last_resolution_stays_pending() {
        let conn = make_db();
        insert_mission(&conn, "m1", "slug-1", "open", "autonomous");
        insert_event_row_at(&conn, "m1", "escalated", "2026-06-13T00:00:01Z");
        insert_event_row_at(&conn, "m1", "state_changed", "2026-06-13T00:00:02Z");
        insert_event_row_at(&conn, "m1", "escalated", "2026-06-13T00:00:03Z");
        let pending = pending_escalations(&conn, "m1").unwrap();
        assert_eq!(
            pending.len(),
            1,
            "the brief after the latest resolution must still be pending",
        );
    }

    #[test]
    fn test_load_events_limit() {
        let conn = make_db();
        insert_mission(&conn, "m1", "slug-1", "open", "autonomous");
        for _ in 0..5 {
            insert_event_row(&conn, "m1", "dispatched");
        }
        let events = load_events(&conn, "m1", 3).unwrap();
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn test_insert_event_roundtrip() {
        let conn = make_db();
        insert_mission(&conn, "m1", "slug-1", "open", "autonomous");
        let payload = serde_json::json!({"from": "open", "to": "in_progress"});
        insert_event(&conn, "m1", "state_changed", Some(payload.clone()), "2026-01-01T00:00:00Z")
            .unwrap();
        let events = load_events(&conn, "m1", 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "state_changed");
        assert_eq!(events[0].payload.as_ref().unwrap()["from"], "open");
    }

    // validate_enum rejects bad state — this is the mutation-guard test
    // the spec requires for set_state.
    #[test]
    fn test_set_state_validates_enum() {
        let err = validate_enum("state", "running", VALID_STATES).unwrap_err();
        assert!(err.contains("running"));
    }

    #[test]
    fn test_set_category_validates_enum() {
        let err = validate_enum("category", "wip", VALID_CATEGORIES).unwrap_err();
        assert!(err.contains("wip"));
    }
}
