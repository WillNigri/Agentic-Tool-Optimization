// commands/events_activity.rs — Real-time log watcher, telemetry event
// queue, and audit logging.
//
// PR 19 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (10 commands across 3 sub-domains):
//
//   Log watcher (3) — Phase 2 real-time agent log file watcher:
//     - start_log_watcher       — kick off the notify-crate file watcher
//     - stop_log_watcher        — terminate it
//     - is_log_watcher_running  — state probe
//
//   Telemetry events (3) — anonymous usage events queued + optionally
//   POSTed to a self-hosted endpoint:
//     - track_event             — enqueue or POST
//     - get_queued_events       — debug/export queue contents
//     - export_telemetry_events — dump queue to JSON file
//
//   Audit logging (4) — local audit trail of high-signal user actions
//   (writes to agent configs, secret access, etc.):
//     - add_audit_log
//     - get_audit_logs          — list with optional action/resource filter
//     - get_audit_log_stats     — counts + top actions
//     - clear_audit_logs        — purge old rows
//
// Telemetry *settings* commands (get_telemetry_settings,
// update_telemetry_settings) live in mod.rs and travel with PR 24
// (settings_config.rs) where preferences are grouped.
//
// AuditLogEntry, TelemetryState, TelemetryEvent, TelemetrySettings,
// LogWatcherState all live in crate root (lib.rs / telemetry.rs) and
// are pulled in via use crate::*.

use rusqlite::params;
use serde_json::json;
use tauri::State;

use crate::{
    telemetry::{TelemetryEvent, TelemetryState},
    AuditLogEntry, DbState, LogWatcherState,
};

// ── Phase 2: Real-time Monitoring Commands ─────────────────────────────────

/// Start the log file watcher for real-time updates
#[tauri::command]
pub fn start_log_watcher(
    app: tauri::AppHandle,
    watcher_state: State<'_, LogWatcherState>,
) -> Result<bool, String> {
    let mut watcher = watcher_state.0.lock().map_err(|e| e.to_string())?;
    watcher.start(app)?;
    Ok(true)
}

/// Stop the log file watcher
#[tauri::command]
pub fn stop_log_watcher(watcher_state: State<'_, LogWatcherState>) -> Result<bool, String> {
    let mut watcher = watcher_state.0.lock().map_err(|e| e.to_string())?;
    watcher.stop();
    Ok(true)
}

/// Check if log watcher is running
#[tauri::command]
pub fn is_log_watcher_running(watcher_state: State<'_, LogWatcherState>) -> Result<bool, String> {
    let watcher = watcher_state.0.lock().map_err(|e| e.to_string())?;
    Ok(watcher.is_watching())
}

// ── Telemetry event queue ──────────────────────────────────────────────────

/// Track a telemetry event
#[tauri::command]
pub async fn track_event(
    state: State<'_, TelemetryState>,
    event_type: String,
    properties: std::collections::HashMap<String, serde_json::Value>,
) -> Result<(), String> {
    // Extract all needed data from the lock, then drop it before any .await
    let (enabled, device_id, endpoint) = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        (
            settings.enabled,
            settings.device_id.clone(),
            settings.endpoint.clone(),
        )
    };

    if !enabled {
        return Ok(());
    }

    let event = TelemetryEvent {
        event_type,
        properties,
        timestamp: chrono::Utc::now().to_rfc3339(),
        session_id: state.session_id.clone(),
        device_id,
    };

    if let Some(endpoint) = endpoint {
        state
            .client
            .post(&endpoint)
            .json(&event)
            .send()
            .await
            .map_err(|e| e.to_string())?;
    } else {
        let mut queue = state.events_queue.lock().map_err(|e| e.to_string())?;
        queue.push(event);

        if queue.len() > 1000 {
            queue.drain(0..500);
        }
    }

    Ok(())
}

/// Get queued telemetry events (for debugging/export)
#[tauri::command]
pub fn get_queued_events(
    state: State<'_, TelemetryState>,
) -> Result<Vec<TelemetryEvent>, String> {
    let queue = state.events_queue.lock().map_err(|e| e.to_string())?;
    Ok(queue.clone())
}

/// Export telemetry events to JSON file
#[tauri::command]
pub fn export_telemetry_events(
    state: State<'_, TelemetryState>,
    path: String,
) -> Result<usize, String> {
    let queue = state.events_queue.lock().map_err(|e| e.to_string())?;
    let count = queue.len();

    let json = serde_json::to_string_pretty(&*queue)
        .map_err(|e| format!("Failed to serialize events: {}", e))?;

    std::fs::write(&path, json).map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(count)
}

// ── Audit Logging Commands ──────────────────────────────────────────────

#[tauri::command]
pub fn add_audit_log(
    db: State<'_, DbState>,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    resource_name: Option<String>,
    details: Option<String>,
) -> Result<AuditLogEntry, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO audit_logs (id, action, resource_type, resource_id, resource_name, details, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, action, resource_type, resource_id, resource_name, details, now],
    ).map_err(|e| e.to_string())?;

    Ok(AuditLogEntry {
        id,
        action,
        resource_type,
        resource_id,
        resource_name,
        details,
        created_at: now,
    })
}

#[tauri::command]
pub fn get_audit_logs(
    db: State<'_, DbState>,
    action: Option<String>,
    resource_type: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<AuditLogEntry>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(100);
    let offset = offset.unwrap_or(0);

    let mut sql = String::from(
        "SELECT id, action, resource_type, resource_id, resource_name, details, created_at
         FROM audit_logs WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref a) = action {
        sql.push_str(&format!(" AND action = ?{}", param_idx));
        param_values.push(Box::new(a.clone()));
        param_idx += 1;
    }
    if let Some(ref rt) = resource_type {
        sql.push_str(&format!(" AND resource_type = ?{}", param_idx));
        param_values.push(Box::new(rt.clone()));
        param_idx += 1;
    }

    sql.push_str(&format!(
        " ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
        param_idx,
        param_idx + 1
    ));
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(AuditLogEntry {
                id: row.get(0)?,
                action: row.get(1)?,
                resource_type: row.get(2)?,
                resource_id: row.get(3)?,
                resource_name: row.get(4)?,
                details: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut logs = Vec::new();
    for row in rows {
        logs.push(row.map_err(|e| e.to_string())?);
    }
    Ok(logs)
}

#[tauri::command]
pub fn get_audit_log_stats(db: State<'_, DbState>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM audit_logs", [], |row| row.get(0))
        .unwrap_or(0);
    let today: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE created_at > datetime('now', '-1 day')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let this_week: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE created_at > datetime('now', '-7 days')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut stmt = conn
        .prepare(
            "SELECT action, COUNT(*) as cnt FROM audit_logs GROUP BY action ORDER BY cnt DESC LIMIT 10",
        )
        .map_err(|e| e.to_string())?;
    let top_actions: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(json!({ "action": row.get::<_, String>(0)?, "count": row.get::<_, i64>(1)? }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(json!({ "total": total, "today": today, "thisWeek": this_week, "topActions": top_actions }))
}

#[tauri::command]
pub fn clear_audit_logs(
    db: State<'_, DbState>,
    before_date: Option<String>,
) -> Result<u64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let deleted = if let Some(date) = before_date {
        conn.execute(
            "DELETE FROM audit_logs WHERE created_at < ?1",
            params![date],
        )
    } else {
        conn.execute("DELETE FROM audit_logs", [])
    }
    .map_err(|e| e.to_string())?;
    Ok(deleted as u64)
}
