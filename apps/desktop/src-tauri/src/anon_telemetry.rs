// anon_telemetry.rs — Wave 3 Tauri commands for the local anon-telemetry
// queue and per-share telemetry opt-in preferences.
//
// These tables (anon_telemetry_queue, share_telemetry_prefs) are OSS-local
// only; the cloud POST target is /api/telemetry/e2e-anonymized and is called
// from the App.tsx background timer, not from Rust.
//
// Tauri commands in this file:
//   anon_telemetry_enqueue      — append one JSON entry
//   anon_telemetry_drain_for_post — return up to 100 oldest entries (for the timer)
//   anon_telemetry_clear_ids    — delete entries by id after a successful POST
//   set_share_telemetry_pref    — write/upsert the opt_in flag for a share
//   get_share_telemetry_pref    — read the opt_in flag (returns false if row absent)

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tauri::State;
use crate::DbState;

// ── anon_telemetry_queue ─────────────────────────────────────────────────────

/// Serialized form of one queue row, returned by drain.
#[derive(Debug, Serialize, Deserialize)]
pub struct TelemetryQueueEntry {
    pub id: i64,
    pub data_json: String,
}

/// Enqueue one JSON blob into the anon_telemetry_queue.
/// The queue stores raw JSON; the drain timer does all the batching.
#[tauri::command]
pub fn anon_telemetry_enqueue(
    entry_json: String,
    db: State<'_, DbState>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO anon_telemetry_queue (data_json) VALUES (?1)",
        rusqlite::params![entry_json],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Return up to 100 oldest entries from the queue.
/// The timer calls this, batches them into one POST, then calls clear_ids.
#[tauri::command]
pub fn anon_telemetry_drain_for_post(
    db: State<'_, DbState>,
) -> Result<Vec<TelemetryQueueEntry>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, data_json FROM anon_telemetry_queue
             ORDER BY queued_at ASC LIMIT 100",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(TelemetryQueueEntry {
                id: row.get(0)?,
                data_json: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Delete queue entries by their ids after a successful cloud POST.
#[tauri::command]
pub fn anon_telemetry_clear_ids(
    ids: Vec<i64>,
    db: State<'_, DbState>,
) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // Build a parameterized IN clause dynamically.
    // rusqlite doesn't support binding arrays directly, so we build
    // a comma-separated `?1, ?2, …` and pass the vec as individual params.
    let placeholders = ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "DELETE FROM anon_telemetry_queue WHERE id IN ({})",
        placeholders
    );
    let params: Vec<&dyn rusqlite::ToSql> = ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    conn.execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── share_telemetry_prefs ─────────────────────────────────────────────────────

/// Upsert the opt-in preference for a specific share.
/// Called by FlipToE2eModal on confirm.
#[tauri::command]
pub fn set_share_telemetry_pref(
    team_id: String,
    resource_kind: String,
    resource_id: String,
    opt_in: bool,
    db: State<'_, DbState>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO share_telemetry_prefs (team_id, resource_kind, resource_id, opt_in)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(team_id, resource_kind, resource_id)
         DO UPDATE SET opt_in = excluded.opt_in",
        rusqlite::params![team_id, resource_kind, resource_id, opt_in as i32],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Read the opt-in preference for a share. Returns false if no row exists.
#[tauri::command]
pub fn get_share_telemetry_pref(
    team_id: String,
    resource_kind: String,
    resource_id: String,
    db: State<'_, DbState>,
) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let result: Option<i32> = conn
        .query_row(
            "SELECT opt_in FROM share_telemetry_prefs
             WHERE team_id = ?1 AND resource_kind = ?2 AND resource_id = ?3",
            rusqlite::params![team_id, resource_kind, resource_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    // Absent row → default false.
    Ok(result.map(|v| v != 0).unwrap_or(false))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, OptionalExtension};

    fn open_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        crate::schema::init_database(&conn);
        conn
    }

    #[test]
    fn anon_telemetry_queue_enqueue_drain_clear_roundtrip() {
        let conn = open_db();

        conn.execute(
            "INSERT INTO anon_telemetry_queue (data_json) VALUES (?1)",
            rusqlite::params![r#"{"feature":"hosted-judge","score":0.9}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO anon_telemetry_queue (data_json) VALUES (?1)",
            rusqlite::params![r#"{"feature":"hosted-diagnose","confidence":0.8}"#],
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT id, data_json FROM anon_telemetry_queue ORDER BY queued_at ASC LIMIT 100",
            )
            .unwrap();
        let rows: Vec<TelemetryQueueEntry> = stmt
            .query_map([], |row| {
                Ok(TelemetryQueueEntry {
                    id: row.get(0)?,
                    data_json: row.get(1)?,
                })
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(rows.len(), 2);

        // Delete both by id.
        let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
        let placeholders = ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "DELETE FROM anon_telemetry_queue WHERE id IN ({})",
            placeholders
        );
        let params: Vec<&dyn rusqlite::ToSql> = ids
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        conn.execute(&sql, params.as_slice()).unwrap();

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM anon_telemetry_queue", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0, "queue must be empty after clear");
    }

    #[test]
    fn share_telemetry_prefs_upsert_and_default() {
        let conn = open_db();

        // Default when row absent = false.
        let missing: Option<i32> = conn
            .query_row(
                "SELECT opt_in FROM share_telemetry_prefs
                 WHERE team_id = 'team-1' AND resource_kind = 'session' AND resource_id = 'res-1'",
                [],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        assert!(missing.is_none(), "no row → query returns None");

        // Insert opt_in = true.
        conn.execute(
            "INSERT INTO share_telemetry_prefs (team_id, resource_kind, resource_id, opt_in)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(team_id, resource_kind, resource_id)
             DO UPDATE SET opt_in = excluded.opt_in",
            rusqlite::params!["team-1", "session", "res-1", 1i32],
        )
        .unwrap();

        let opt_in: i32 = conn
            .query_row(
                "SELECT opt_in FROM share_telemetry_prefs
                 WHERE team_id = 'team-1' AND resource_kind = 'session' AND resource_id = 'res-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(opt_in, 1, "opt_in should be 1 after upsert");

        // Toggle to false.
        conn.execute(
            "INSERT INTO share_telemetry_prefs (team_id, resource_kind, resource_id, opt_in)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(team_id, resource_kind, resource_id)
             DO UPDATE SET opt_in = excluded.opt_in",
            rusqlite::params!["team-1", "session", "res-1", 0i32],
        )
        .unwrap();

        let opt_in_off: i32 = conn
            .query_row(
                "SELECT opt_in FROM share_telemetry_prefs
                 WHERE team_id = 'team-1' AND resource_kind = 'session' AND resource_id = 'res-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(opt_in_off, 0, "upsert must overwrite previous opt_in");
    }
}
