// v2.3.9 Phase 4.3 — CLI-side event publishing.
//
// The desktop process owns the in-memory events::bus. The CLI is a
// separate short-lived process, so it can't send to that channel.
// But the events_log SQLite table is the cross-process audit log,
// and the desktop's engine poll loop reads from it.
//
// So: when the CLI fires a dispatch / replay / etc, it writes the
// event directly to events_log. The next desktop-side poll picks it
// up and runs matching recipes.
//
// Sequence IDs: reserved by reading MAX(event_seq) + 1 and writing
// in one transaction so concurrent CLI writes don't collide. We
// retry up to 3 times on UNIQUE-constraint failure.

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Reserve the next event_seq + insert atomically. Returns the seq
/// used. On collision (extremely rare — two CLIs writing in the same
/// microsecond), retries up to 3 times.
fn insert_event_row(
    conn: &Connection,
    event_type: &str,
    payload_json: &str,
    occurred_at: &str,
) -> Result<i64> {
    for _attempt in 0..3 {
        let max: i64 = conn
            .query_row("SELECT COALESCE(MAX(event_seq), 0) FROM events_log", [], |r| r.get(0))
            .unwrap_or(0);
        let next = max + 1;
        let res = conn.execute(
            "INSERT INTO events_log (event_seq, event_type, payload, occurred_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![next, event_type, payload_json, occurred_at],
        );
        match res {
            Ok(_) => return Ok(next),
            Err(rusqlite::Error::SqliteFailure(_, _)) => {
                // Likely UNIQUE-constraint conflict; retry with a fresh max.
                continue;
            }
            Err(e) => return Err(e).context("insert events_log"),
        }
    }
    anyhow::bail!("events_log insert: too many retries")
}

/// Publish a DispatchFailed event. Mirrors the shape the desktop
/// publishes (apps/desktop/src-tauri/src/events.rs::AtoEvent).
pub fn publish_dispatch_failed(
    conn: &Connection,
    run_id: &str,
    agent_slug: Option<&str>,
    runtime: &str,
    error_message: &str,
    duration_ms: i64,
    failed_at: &str,
) {
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='events_log'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return; // older DB; silent skip
    }
    // Placeholder event_seq — gets replaced after we reserve the real seq.
    let payload = serde_json::json!({
        "type": "dispatch_failed",
        "event_seq": 0_u64,
        "run_id": run_id,
        "agent_slug": agent_slug,
        "runtime": runtime,
        "error_message": error_message,
        "duration_ms": duration_ms,
        "failed_at": failed_at,
    });
    let payload_str = serde_json::to_string(&payload).unwrap_or_default();
    if let Ok(seq) = insert_event_row(conn, "dispatch_failed", &payload_str, failed_at) {
        // Rewrite the payload with the real seq so downstream consumers
        // see a self-consistent event. Best-effort UPDATE.
        let final_payload = serde_json::json!({
            "type": "dispatch_failed",
            "event_seq": seq,
            "run_id": run_id,
            "agent_slug": agent_slug,
            "runtime": runtime,
            "error_message": error_message,
            "duration_ms": duration_ms,
            "failed_at": failed_at,
        });
        let _ = conn.execute(
            "UPDATE events_log SET payload = ?1 WHERE event_seq = ?2",
            rusqlite::params![final_payload.to_string(), seq],
        );
    }
}

/// v2.15.2 (war_room 78617E68) — publish a DispatchExhausted event
/// when a dispatch is short-circuited by the runtime_quotas pre-flight
/// gate (subscription / long-window quota, not transient rate limit).
/// Distinct from dispatch_failed: this event carries the parsed
/// reset_at + the policy that was applied + the fallback runtime
/// chosen (if any). Per codex's alternative design verdict: "emit a
/// first-class dispatch_exhausted event into events_log; desktop
/// reacts with a chooser only for interactive sessions; loops/CLI
/// consume the persisted policy directly. One canonical signal that
/// v2.15.3 pause/resume and notifications bolt onto."
pub fn publish_dispatch_exhausted(
    conn: &Connection,
    runtime: &str,
    reset_at: &str,
    policy_chosen: &str,
    fallback_runtime: Option<&str>,
    raw_message: Option<&str>,
    occurred_at: &str,
) {
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='events_log'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return;
    }
    let payload = serde_json::json!({
        "type": "dispatch_exhausted",
        "event_seq": 0_u64,
        "runtime": runtime,
        "reset_at": reset_at,
        "policy_chosen": policy_chosen,
        "fallback_runtime": fallback_runtime,
        "raw_message": raw_message,
        "occurred_at": occurred_at,
    });
    let payload_str = serde_json::to_string(&payload).unwrap_or_default();
    if let Ok(seq) = insert_event_row(conn, "dispatch_exhausted", &payload_str, occurred_at) {
        let final_payload = serde_json::json!({
            "type": "dispatch_exhausted",
            "event_seq": seq,
            "runtime": runtime,
            "reset_at": reset_at,
            "policy_chosen": policy_chosen,
            "fallback_runtime": fallback_runtime,
            "raw_message": raw_message,
            "occurred_at": occurred_at,
        });
        let _ = conn.execute(
            "UPDATE events_log SET payload = ?1 WHERE event_seq = ?2",
            rusqlite::params![final_payload.to_string(), seq],
        );
    }
}

/// Publish a DispatchResumed event (v2.15.4 — pause-and-wake). Fired when
/// a paused dispatch is woken at its reset_at and successfully re-fired
/// (or, if outcome="re_paused"/"abandoned", when the resume attempt found
/// the runtime still exhausted and bumped pause_count). One source of
/// truth: events_log is consumed by the desktop modal, the CLI status
/// view, and the future scheduler dashboard.
pub fn publish_dispatch_resumed(
    conn: &Connection,
    paused_dispatch_id: &str,
    runtime: &str,
    outcome: &str,        // "resumed" | "re_paused" | "abandoned"
    pause_count: i64,
    reset_at: Option<&str>,
    occurred_at: &str,
) {
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='events_log'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return;
    }
    let payload = serde_json::json!({
        "type": "dispatch_resumed",
        "event_seq": 0_u64,
        "paused_dispatch_id": paused_dispatch_id,
        "runtime": runtime,
        "outcome": outcome,
        "pause_count": pause_count,
        "reset_at": reset_at,
        "occurred_at": occurred_at,
    });
    let payload_str = serde_json::to_string(&payload).unwrap_or_default();
    if let Ok(seq) = insert_event_row(conn, "dispatch_resumed", &payload_str, occurred_at) {
        let final_payload = serde_json::json!({
            "type": "dispatch_resumed",
            "event_seq": seq,
            "paused_dispatch_id": paused_dispatch_id,
            "runtime": runtime,
            "outcome": outcome,
            "pause_count": pause_count,
            "reset_at": reset_at,
            "occurred_at": occurred_at,
        });
        let _ = conn.execute(
            "UPDATE events_log SET payload = ?1 WHERE event_seq = ?2",
            rusqlite::params![final_payload.to_string(), seq],
        );
    }
}

/// Publish a ReplayDone event.
pub fn publish_replay_done(
    conn: &Connection,
    job_id: &str,
    source_trace_id: &str,
    source_runtime: &str,
    target_runtime: &str,
    target_model: Option<&str>,
    status: &str, // "done" | "failed"
    duration_ms: Option<i64>,
    cost_usd_estimated: Option<f64>,
    error_message: Option<&str>,
    finished_at: &str,
) {
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='events_log'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return;
    }
    let payload = serde_json::json!({
        "type": "replay_done",
        "event_seq": 0_u64,
        "job_id": job_id,
        "source_trace_id": source_trace_id,
        "source_runtime": source_runtime,
        "target_runtime": target_runtime,
        "target_model": target_model,
        "status": status,
        "duration_ms": duration_ms,
        "cost_usd_estimated": cost_usd_estimated,
        "error_message": error_message,
        "finished_at": finished_at,
    });
    let payload_str = serde_json::to_string(&payload).unwrap_or_default();
    if let Ok(seq) = insert_event_row(conn, "replay_done", &payload_str, finished_at) {
        let final_payload = serde_json::json!({
            "type": "replay_done",
            "event_seq": seq,
            "job_id": job_id,
            "source_trace_id": source_trace_id,
            "source_runtime": source_runtime,
            "target_runtime": target_runtime,
            "target_model": target_model,
            "status": status,
            "duration_ms": duration_ms,
            "cost_usd_estimated": cost_usd_estimated,
            "error_message": error_message,
            "finished_at": finished_at,
        });
        let _ = conn.execute(
            "UPDATE events_log SET payload = ?1 WHERE event_seq = ?2",
            rusqlite::params![final_payload.to_string(), seq],
        );
    }
}

/// v2.3.40 — Publish a RatchetBreach event.
///
/// One event per breached target so consumers (ops recipes,
/// `ato events watch`, the GUI feed) can react per-target without
/// re-parsing a compound payload. `event_seq` is returned so a
/// caller can attach it to an activity_post via `related_event_seq`
/// for traceability across feed + bus.
pub fn publish_ratchet_breach(
    conn: &Connection,
    target_kind: &str,
    target_value: &str,
    metric: &str,
    baseline_value: f64,
    threshold: f64,
    floor_with_tolerance: f64,
    current_value: f64,
    current_sample_count: i64,
    current_window_days: i64,
    occurred_at: &str,
) -> Option<i64> {
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='events_log'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return None;
    }
    let payload = serde_json::json!({
        "type": "ratchet_breach",
        "event_seq": 0_u64,
        "target_kind": target_kind,
        "target_value": target_value,
        "metric": metric,
        "baseline_value": baseline_value,
        "threshold": threshold,
        "floor_with_tolerance": floor_with_tolerance,
        "current_value": current_value,
        "current_sample_count": current_sample_count,
        "current_window_days": current_window_days,
        "occurred_at": occurred_at,
    });
    let payload_str = serde_json::to_string(&payload).unwrap_or_default();
    let seq = insert_event_row(conn, "ratchet_breach", &payload_str, occurred_at).ok()?;
    let final_payload = serde_json::json!({
        "type": "ratchet_breach",
        "event_seq": seq,
        "target_kind": target_kind,
        "target_value": target_value,
        "metric": metric,
        "baseline_value": baseline_value,
        "threshold": threshold,
        "floor_with_tolerance": floor_with_tolerance,
        "current_value": current_value,
        "current_sample_count": current_sample_count,
        "current_window_days": current_window_days,
        "occurred_at": occurred_at,
    });
    let _ = conn.execute(
        "UPDATE events_log SET payload = ?1 WHERE event_seq = ?2",
        rusqlite::params![final_payload.to_string(), seq],
    );
    Some(seq)
}
