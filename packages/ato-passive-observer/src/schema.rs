// Schema bootstrap for the tables the observer writes.
//
// The desktop's `init_database` (apps/desktop/src-tauri/src/schema.rs)
// is the single source of truth for the full local schema, but it's
// Tauri-bound and runs at app boot. On a headless box without the
// desktop ever launched — the CLI-driven CI runner or systemd unit
// the universal-observability tier targets — those tables don't
// exist when `ato observe start` opens the SQLite file. Without this
// bootstrap, every `INSERT OR IGNORE` would silently fail
// ("no such table") and the watcher would report "started" while
// persisting zero rows.
//
// This module re-creates just the columns the passive observer
// reads/writes, using `IF NOT EXISTS` everywhere so it's safe to
// run alongside the desktop's full initializer. New columns added
// by future desktop migrations don't need to be mirrored here —
// only the ones the observer's INSERT statements name.

use rusqlite::Connection;

/// Idempotent bootstrap of the tables and indexes the passive
/// observer touches. Safe to call before every watcher start.
///
/// Returns `Err` only on a connect failure — the CREATEs themselves
/// are best-effort (they no-op if the table is already present, and
/// the partial-unique index repeats are harmless).
pub fn ensure_schema(db_path: &std::path::Path) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS execution_logs (
            id                      TEXT PRIMARY KEY,
            runtime                 TEXT NOT NULL,
            prompt                  TEXT,
            response                TEXT,
            tokens_in               INTEGER,
            tokens_out              INTEGER,
            duration_ms             INTEGER,
            status                  TEXT,
            error_message           TEXT,
            skill_name              TEXT,
            cloud_trace_id          TEXT,
            created_at              TEXT,
            cost_usd_estimated      REAL,
            agent_slug              TEXT,
            model                   TEXT,
            auth_mode               TEXT,
            dispatch_kind           TEXT NOT NULL DEFAULT 'active',
            billing_surface         TEXT,
            provider_session_id     TEXT,
            sequence_within_session INTEGER
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_logs_session_seq
            ON execution_logs(provider_session_id, sequence_within_session)
            WHERE provider_session_id IS NOT NULL;

        CREATE INDEX IF NOT EXISTS idx_execution_logs_dispatch_kind
            ON execution_logs(dispatch_kind, created_at DESC);

        CREATE TABLE IF NOT EXISTS watcher_state (
            source       TEXT NOT NULL,
            file_path    TEXT NOT NULL,
            byte_offset  INTEGER NOT NULL DEFAULT 0,
            last_seq     INTEGER NOT NULL DEFAULT 0,
            updated_at   TEXT NOT NULL,
            PRIMARY KEY (source, file_path)
        );

        CREATE TABLE IF NOT EXISTS live_runs (
            run_id          TEXT PRIMARY KEY,
            agent_slug      TEXT,
            runtime         TEXT,
            workspace       TEXT,
            source          TEXT,
            started_at      TEXT,
            status          TEXT,
            child_pid       INTEGER,
            dispatch_kind   TEXT NOT NULL DEFAULT 'active',
            billing_surface TEXT
        );
        "#,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn ensure_schema_is_idempotent() {
        let tmp = NamedTempFile::new().unwrap();
        ensure_schema(tmp.path()).expect("first call");
        ensure_schema(tmp.path()).expect("second call");
        let conn = Connection::open(tmp.path()).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN \
                 ('execution_logs','watcher_state','live_runs')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn ensure_schema_coexists_with_extra_columns() {
        // Simulate the desktop having already added bonus columns.
        let tmp = NamedTempFile::new().unwrap();
        ensure_schema(tmp.path()).unwrap();
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute(
            "ALTER TABLE execution_logs ADD COLUMN bonus TEXT",
            [],
        )
        .unwrap();
        drop(conn);
        // Re-bootstrap is still a no-op.
        ensure_schema(tmp.path()).unwrap();
    }
}
