// v2.3.45 — Ratchet view for the GUI Insights panel.
//
// Phase 6.x-K (v2.3.39) shipped the eval-score ratchet as a CLI-only
// CI gate. v2.3.40 wired breaches into the events bus + activity feed.
// This module exposes both surfaces — locked floors and breach history
// — to the desktop so a user can see "where is my quality floor and
// has it been breached?" without running CLI commands.
//
// All reads. Lock/unlock from the GUI shells out via the existing
// shell pane or is documented via the CLI commands rendered in the
// panel's empty-state copy.

use rusqlite::Connection;
use serde::Serialize;
use std::process::Command;
use tauri::State;

use crate::DbState;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RatchetRow {
    pub target_kind: String,
    pub target_value: String,
    pub metric: String,
    pub baseline_value: f64,
    pub baseline_window_days: i64,
    pub threshold: f64,
    /// Current 7-day success rate for this target, or null when there
    /// are no dispatches in the window.
    pub current_value: Option<f64>,
    pub current_sample_count: i64,
    pub floor_with_tolerance: f64,
    pub verdict: String, // "pass" | "fail" | "insufficient_data"
    pub locked_at: String,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RatchetBreachEvent {
    pub event_seq: i64,
    pub target_kind: String,
    pub target_value: String,
    pub baseline_value: f64,
    pub current_value: f64,
    pub current_sample_count: i64,
    pub occurred_at: String,
}

#[tauri::command]
pub fn list_ratchets(db: State<'_, DbState>) -> Result<Vec<RatchetRow>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_inner(&conn).map_err(|e| e.to_string())
}

const CHECK_WINDOW_DAYS: i64 = 7;

fn list_inner(conn: &Connection) -> rusqlite::Result<Vec<RatchetRow>> {
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='eval_ratchets'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT target_kind, target_value, metric, baseline_value,
                baseline_window_days, threshold, locked_at, notes
           FROM eval_ratchets
          ORDER BY target_kind, target_value",
    )?;
    let bare: Vec<(String, String, String, f64, i64, f64, String, Option<String>)> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, f64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut rows = Vec::with_capacity(bare.len());
    for (kind, value, metric, baseline, window, threshold, locked_at, notes) in bare {
        let (current, samples) = compute_success_rate(conn, &kind, &value, CHECK_WINDOW_DAYS)?;
        let floor_tol = (baseline - threshold).max(0.0);
        let verdict = match current {
            None => "insufficient_data".to_string(),
            Some(c) if c >= floor_tol => "pass".to_string(),
            Some(_) => "fail".to_string(),
        };
        rows.push(RatchetRow {
            target_kind: kind,
            target_value: value,
            metric,
            baseline_value: baseline,
            baseline_window_days: window,
            threshold,
            current_value: current,
            current_sample_count: samples,
            floor_with_tolerance: floor_tol,
            verdict,
            locked_at,
            notes,
        });
    }
    Ok(rows)
}

/// Mirrors apps/cli/src/commands/ratchet.rs::compute_success_rate so
/// the GUI verdict matches what `ato ratchet check` returns.
fn compute_success_rate(
    conn: &Connection,
    target_kind: &str,
    target_value: &str,
    days: i64,
) -> rusqlite::Result<(Option<f64>, i64)> {
    let cutoff = format!("-{} days", days);
    let (sql, params): (&str, Vec<String>) = match target_kind {
        "agent" => (
            "SELECT
                COUNT(*) AS total,
                SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) AS ok
             FROM execution_logs
             WHERE created_at >= datetime('now', ?1)
               AND agent_slug = ?2",
            vec![cutoff, target_value.to_string()],
        ),
        "runtime" => (
            "SELECT
                COUNT(*) AS total,
                SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) AS ok
             FROM execution_logs
             WHERE created_at >= datetime('now', ?1)
               AND runtime = ?2",
            vec![cutoff, target_value.to_string()],
        ),
        _ => (
            "SELECT
                COUNT(*) AS total,
                SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) AS ok
             FROM execution_logs
             WHERE created_at >= datetime('now', ?1)",
            vec![cutoff],
        ),
    };
    let dyn_params: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let (total, ok): (i64, Option<i64>) =
        conn.query_row(sql, dyn_params.as_slice(), |r| Ok((r.get(0)?, r.get(1)?)))?;
    if total == 0 {
        Ok((None, 0))
    } else {
        Ok((Some(ok.unwrap_or(0) as f64 / total as f64), total))
    }
}

#[tauri::command]
pub fn list_ratchet_breaches(
    db: State<'_, DbState>,
    limit: Option<i64>,
) -> Result<Vec<RatchetBreachEvent>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(20);
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='events_log'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT event_seq, payload, occurred_at
               FROM events_log
              WHERE event_type = 'ratchet_breach'
              ORDER BY event_seq DESC
              LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;

    let rows: Vec<RatchetBreachEvent> = stmt
        .query_map([limit], |r| {
            let event_seq: i64 = r.get(0)?;
            let payload: String = r.get(1)?;
            let occurred_at: String = r.get(2)?;
            Ok((event_seq, payload, occurred_at))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .filter_map(|(seq, payload, occurred_at)| {
            let v: serde_json::Value = serde_json::from_str(&payload).ok()?;
            Some(RatchetBreachEvent {
                event_seq: seq,
                target_kind: v.get("target_kind")?.as_str()?.to_string(),
                target_value: v.get("target_value")?.as_str()?.to_string(),
                baseline_value: v.get("baseline_value")?.as_f64()?,
                current_value: v.get("current_value")?.as_f64()?,
                current_sample_count: v.get("current_sample_count")?.as_i64()?,
                occurred_at,
            })
        })
        .collect();
    Ok(rows)
}

// ───────────────────────────────────────────────────────────────────────
// v2.3.49 — Lock / unlock from the GUI.
//
// Same shell-out pattern as sessions_view::create_session — the CLI
// is the canonical implementation of `ato ratchet lock` so we call
// it directly. Avoids re-implementing baseline computation here.

fn resolve_ato_binary() -> Result<String, String> {
    if let Some(p) = crate::commands::which_cli("ato") {
        return Ok(p);
    }
    // Deliberate fallback to the bare command name: a user may have
    // installed `ato` after the desktop process started, in which case
    // PATH (via login shell) will resolve it at exec time even though
    // our cached `which_cli` came back empty. Worst case: Command::new
    // surfaces ENOENT and we propagate it as a clean spawn-error
    // string — that's a clearer signal than the GUI silently no-op'ing.
    Ok("ato".to_string())
}

#[tauri::command]
pub fn lock_ratchet(
    target: String,
    days: Option<i64>,
    threshold: Option<f64>,
    notes: Option<String>,
) -> Result<(), String> {
    // v2.3.49 — IPC boundary input validation. React side validates
    // these too, but a bypassed UI (custom Tauri caller, automation)
    // can send garbage; the CLI then bails with a less actionable
    // error. Catch it here.
    if let Some(d) = days {
        if !(1..=365).contains(&d) {
            return Err(format!("days must be 1..=365 (got {})", d));
        }
    }
    if let Some(t) = threshold {
        if !(0.0..=1.0).contains(&t) {
            return Err(format!("threshold must be 0.0..=1.0 (got {})", t));
        }
    }
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["ratchet", "lock", "--target", &target]);
    if let Some(d) = days {
        cmd.args(["--days", &d.to_string()]);
    }
    if let Some(t) = threshold {
        cmd.args(["--threshold", &t.to_string()]);
    }
    if let Some(n) = &notes {
        cmd.args(["--notes", n]);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("spawn ato ratchet lock: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!("ato ratchet lock failed: {}", stderr.trim()));
    }
    Ok(())
}

#[tauri::command]
pub fn unlock_ratchet(target: String) -> Result<(), String> {
    let bin = resolve_ato_binary()?;
    let out = Command::new(&bin)
        .args(["ratchet", "unlock", "--target", &target])
        .output()
        .map_err(|e| format!("spawn ato ratchet unlock: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!("ato ratchet unlock failed: {}", stderr.trim()));
    }
    Ok(())
}
