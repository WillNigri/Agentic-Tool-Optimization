// `ato events recent [--type X] [--limit N]` — tail the events_log table.
// `ato events watch [--type X] [--since N] [--max-rows N] [--poll-ms N]` —
//     stream new events as they land. v2.3.14 Phase 4.8.
//
// The desktop's events::bus::publish persists every event to events_log
// (best-effort) so we have an auditable history. The `watch` mode polls
// the table from a read-only connection at a configurable interval
// (default 500ms) and emits one JSONL row per new event. No IPC channel
// is needed — SQLite gives us a deterministic source for both producer
// and consumer.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;

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

/// Tail events_log as JSONL on stdout. One JSON object per line, in
/// strict event_seq order. Blocks forever (unless --max-rows hits the
/// cap or the user sends SIGINT/SIGTERM).
///
/// Latency: bounded by `poll_ms`. Producers (desktop bus, CLI
/// events_publisher) commit each row before signaling delivery, so
/// once a row is visible to our SELECT it has all its fields. There's
/// no half-published state to handle.
///
/// Connection model: opens a fresh read-only connection each tick.
/// Opening a read-only SQLite handle at ~2 Hz is cheap (sub-ms on
/// macOS/Linux), and the alternative (persistent connection with
/// busy_timeout) would share locks with the desktop's writer.
/// Transient open failures or query errors retry on the next tick
/// rather than killing the watcher.
pub fn watch(
    db_path: &PathBuf,
    type_filter: Option<String>,
    since_seq: Option<i64>,
    max_rows: Option<usize>,
    poll_ms: u64,
    opts: &Opts,
) -> Result<()> {
    let poll_ms = poll_ms.clamp(100, 5_000);
    let interval = std::time::Duration::from_millis(poll_ms);

    // Bootstrap last_seen_seq. Default: skip everything that exists
    // now (only show new events from this moment forward) — matches
    // `tail -f` semantics. --since overrides for "from this seq + 1".
    //
    // Codex round-2 important: bootstrap retries transient failures
    // like the main loop. The previous version did `?` on
    // open_readonly so a momentarily-busy DB at startup would kill
    // the watch immediately, less robust than the steady-state path.
    let mut last_seen_seq: i64 = match since_seq {
        Some(s) => s,
        None => loop {
            match crate::db::open_readonly(db_path) {
                Ok(conn) => {
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
                                "events_log table not found. Launch the ATO desktop (v2.3.8+) once to apply the migration.",
                            );
                        }
                        return Ok(());
                    }
                    break conn
                        .query_row(
                            "SELECT COALESCE(MAX(event_seq), 0) FROM events_log",
                            [],
                            |r| r.get(0),
                        )
                        .unwrap_or(0);
                }
                Err(_) => {
                    std::thread::sleep(interval);
                    continue;
                }
            }
        },
    };

    if opts.human {
        emit_human(&format!(
            "Watching events_log from seq > {} (poll {}ms). Ctrl-C to stop.",
            last_seen_seq, poll_ms
        ));
    }

    let mut emitted: usize = 0;
    // Codex round-2 important: dedupe error logs. Persistent failure
    // modes (no such table, schema mismatch, SQLITE_BUSY in a busy
    // loop) would otherwise spam stderr 2-10 lines/sec. Only log the
    // first occurrence of each distinct message; reset when an OK
    // poll lands.
    let mut last_error_msg: Option<String> = None;
    loop {
        // Note: don't clear last_error_msg on a successful open —
        // codex round-3 caught that if fetch_new keeps failing after
        // a clean open, clearing here resets the dedupe and the
        // fetch-error branch logs every poll. Only the fetch Ok path
        // clears the watermark.
        let conn = match crate::db::open_readonly(db_path) {
            Ok(c) => c,
            Err(e) => {
                let msg = e.to_string();
                if last_error_msg.as_ref() != Some(&msg) {
                    eprintln!("ato events watch: open error (will retry): {}", msg);
                    last_error_msg = Some(msg);
                }
                std::thread::sleep(interval);
                continue;
            }
        };
        // Codex round-1 important: query errors used to bubble out
        // and kill the watcher (e.g. "no such table: events_log" on
        // a fresh DB before the desktop has migrated, or a transient
        // SQLITE_BUSY). Treat all fetch failures as transient.
        let new_rows = match fetch_new(&conn, last_seen_seq, type_filter.as_deref()) {
            Ok(rows) => {
                last_error_msg = None;
                rows
            }
            Err(e) => {
                let msg = e.to_string();
                if last_error_msg.as_ref() != Some(&msg) {
                    eprintln!("ato events watch: fetch error (will retry): {}", msg);
                    last_error_msg = Some(msg);
                }
                drop(conn);
                std::thread::sleep(interval);
                continue;
            }
        };
        for row in new_rows {
            // Advance regardless of filter so we don't re-scan the
            // same seq next tick. fetch_new already applied the filter
            // — but in case of a malformed payload we still bump.
            last_seen_seq = row.event_seq;
            if opts.human {
                emit_human(&format!(
                    "  #{} [{}] {}",
                    row.event_seq, row.event_type, row.occurred_at
                ));
            } else {
                // JSONL: one compact object per line, flush after.
                let line = serde_json::to_string(&row)
                    .unwrap_or_else(|_| String::from("{}"));
                println!("{}", line);
            }
            emitted += 1;
            if let Some(cap) = max_rows {
                if emitted >= cap {
                    return Ok(());
                }
            }
        }
        drop(conn);
        std::thread::sleep(interval);
    }
}

/// Read events_log rows with event_seq > since, optionally filtered by
/// type, in ascending seq order. Capped at 500 per tick so a backlog
/// doesn't blow the audit but also catches up quickly.
fn fetch_new(
    conn: &Connection,
    since_seq: i64,
    type_filter: Option<&str>,
) -> Result<Vec<EventRow>> {
    let mut out: Vec<EventRow> = Vec::new();
    let sql = if type_filter.is_some() {
        "SELECT event_seq, event_type, payload, occurred_at FROM events_log
          WHERE event_seq > ?1 AND event_type = ?2 ORDER BY event_seq ASC LIMIT 500"
    } else {
        "SELECT event_seq, event_type, payload, occurred_at FROM events_log
          WHERE event_seq > ?1 ORDER BY event_seq ASC LIMIT 500"
    };
    let mut stmt = conn.prepare(sql).context("prepare watch query")?;
    let mut rows = match type_filter {
        Some(t) => stmt.query(rusqlite::params![since_seq, t])?,
        None => stmt.query(rusqlite::params![since_seq])?,
    };
    while let Some(r) = rows.next()? {
        let payload_json: String = r.get(2)?;
        let payload: serde_json::Value =
            serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
        out.push(EventRow {
            event_seq: r.get(0)?,
            event_type: r.get(1)?,
            payload,
            occurred_at: r.get(3)?,
        });
    }
    Ok(out)
}
