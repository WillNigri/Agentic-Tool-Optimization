// `ato events recent [--type X] [--limit N]` — tail the events_log table.
//
// The desktop's events::bus::publish persists every event to events_log
// (best-effort) so we have an auditable history. v1 is "recent" only;
// a live `watch` mode would need an IPC channel into the running
// desktop and lands in Phase 4.3.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct EventRow {
    pub event_seq: i64,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub occurred_at: String,
}

pub fn recent(
    conn: &Connection,
    type_filter: Option<String>,
    limit: usize,
    opts: &Opts,
) -> Result<()> {
    let exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='events_log'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        if opts.human {
            emit_human(
                "events_log table not found. Launch the ATO desktop (v2.3.8+) once to apply the migration and start publishing events.",
            );
        } else {
            emit_json(&Vec::<EventRow>::new())?;
        }
        return Ok(());
    }

    let safe_limit = limit.min(10_000) as i64;
    let (sql, has_filter) = match type_filter.as_deref() {
        Some(_) => (
            "SELECT event_seq, event_type, payload, occurred_at FROM events_log WHERE event_type = ?1 ORDER BY event_seq DESC LIMIT ?2",
            true,
        ),
        None => (
            "SELECT event_seq, event_type, payload, occurred_at FROM events_log ORDER BY event_seq DESC LIMIT ?1",
            false,
        ),
    };
    let mut stmt = conn.prepare(sql).context("prepare events query")?;

    let map_row = |r: &rusqlite::Row| -> rusqlite::Result<EventRow> {
        let payload_json: String = r.get(2)?;
        let payload: serde_json::Value =
            serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
        Ok(EventRow {
            event_seq: r.get(0)?,
            event_type: r.get(1)?,
            payload,
            occurred_at: r.get(3)?,
        })
    };
    let rows: Vec<EventRow> = if has_filter {
        let t = type_filter.unwrap();
        stmt.query_map(rusqlite::params![t, safe_limit], map_row)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(rusqlite::params![safe_limit], map_row)?
            .collect::<Result<Vec<_>, _>>()?
    };

    if opts.human {
        if rows.is_empty() {
            emit_human(
                "No events published yet. Events fire on dispatch failures, regression detection, replay completions, and cost-threshold crossings.",
            );
        } else {
            emit_human(&format!("{} events:", rows.len()));
            for r in &rows {
                emit_human(&format!(
                    "  #{} [{}] {}",
                    r.event_seq, r.event_type, r.occurred_at
                ));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}
