// v2.15.4 (war_room E063A89E) — pause-and-wake persistence + lifecycle.
//
// Storage shape (paused_dispatches table from schema.rs):
//   - id, runtime, reset_at, loop_run_id, step_id, prompt, model,
//     agent_slug, workspace_root, pause_count, max_pause_count, status,
//     paused_at, resumed_at, abandoned_at, audit_json, created_at
//
// Status transitions:
//   paused → resuming → resumed     (happy path; row stays for audit)
//   paused → resuming → paused     (re-pause when reset still future at wake)
//   paused → abandoned              (pause_count hit max_pause_count)
//
// Codex amendments (war_room E063A89E) applied:
//   - paused_dispatches AUTHORITATIVE; loop_runs.paused_until is a mirror
//   - max_pause_count default 3 (hardcoded reliability guard, not a
//     product surface — per codex "this is a reliability guard, not
//     product surface")
//   - At wake, claim transactionally then re-run quota pre-flight (never
//     trust the stale stored reset_at)

use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PausedDispatch {
    pub id: String,
    pub runtime: String,
    pub reset_at: String,
    pub loop_run_id: Option<String>,
    pub step_id: Option<String>,
    pub prompt: String,
    pub model: Option<String>,
    pub agent_slug: Option<String>,
    pub workspace_root: Option<String>,
    pub pause_count: i64,
    pub max_pause_count: i64,
    pub status: String,
    pub paused_at: String,
    pub resumed_at: Option<String>,
    pub abandoned_at: Option<String>,
    pub audit_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct InsertPausedDispatch<'a> {
    pub runtime: &'a str,
    pub reset_at: &'a str,
    pub loop_run_id: Option<&'a str>,
    pub step_id: Option<&'a str>,
    pub prompt: &'a str,
    pub model: Option<&'a str>,
    pub agent_slug: Option<&'a str>,
    pub workspace_root: Option<&'a str>,
}

/// Insert a brand-new paused row for a freshly-detected exhaustion.
/// pause_count starts at 1 (this is the first pause). Returns the row id.
pub fn insert_new(conn: &Connection, input: InsertPausedDispatch) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO paused_dispatches
            (id, runtime, reset_at, loop_run_id, step_id, prompt, model,
             agent_slug, workspace_root, pause_count, max_pause_count,
             status, paused_at, audit_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, 3, 'paused', ?10, ?11, ?10)",
        rusqlite::params![
            id,
            input.runtime,
            input.reset_at,
            input.loop_run_id,
            input.step_id,
            input.prompt,
            input.model,
            input.agent_slug,
            input.workspace_root,
            now,
            initial_audit_json(input.reset_at),
        ],
    )?;
    // Mirror to loop_runs if this pause is part of a loop.
    if let Some(loop_run_id) = input.loop_run_id {
        let _ = conn.execute(
            "UPDATE loop_runs
                SET paused_until = ?1, paused_dispatch_id = ?2
              WHERE id = ?3",
            rusqlite::params![input.reset_at, id, loop_run_id],
        );
    }
    Ok(id)
}

/// At wake time, claim the row transactionally so the polling-startup-
/// scanner AND a manual `ato loop resume <id>` can't both fire.
/// Returns the claimed row or Err if the row is already non-paused or
/// has been abandoned.
pub fn claim_for_resume(conn: &Connection, id: &str) -> Result<PausedDispatch> {
    let tx = conn.unchecked_transaction()?;
    let current_status: String = tx
        .query_row(
            "SELECT status FROM paused_dispatches WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .map_err(|_| anyhow!("paused dispatch '{}' not found", id))?;
    if current_status != "paused" {
        anyhow::bail!(
            "paused dispatch '{}' is already in status '{}' — cannot claim for resume",
            id,
            current_status
        );
    }
    tx.execute(
        "UPDATE paused_dispatches SET status = 'resuming' WHERE id = ?1 AND status = 'paused'",
        [id],
    )?;
    let row = load_by_id(&tx, id)?;
    tx.commit()?;
    Ok(row)
}

/// Mark a claimed row as successfully resumed.
pub fn mark_resumed(conn: &Connection, id: &str, resume_note: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let audit = append_audit_event(conn, id, "resumed", resume_note)?;
    conn.execute(
        "UPDATE paused_dispatches
            SET status = 'resumed',
                resumed_at = ?1,
                audit_json = ?2
          WHERE id = ?3",
        rusqlite::params![now, audit, id],
    )?;
    // Clear the loop_runs mirror.
    let _ = conn.execute(
        "UPDATE loop_runs
            SET paused_until = NULL,
                paused_dispatch_id = NULL
          WHERE paused_dispatch_id = ?1",
        [id],
    );
    Ok(())
}

/// At wake time the runtime is STILL exhausted (newer reset_at exists).
/// Re-pause: bump pause_count, replace reset_at with the new value,
/// flip status back to 'paused'. If pause_count would exceed
/// max_pause_count, abandon the row instead.
pub fn re_pause_or_abandon(
    conn: &Connection,
    id: &str,
    new_reset_at: &str,
    reason: &str,
) -> Result<&'static str> {
    let (pause_count, max_pause_count): (i64, i64) = conn.query_row(
        "SELECT pause_count, max_pause_count FROM paused_dispatches WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let next_count = pause_count + 1;
    if next_count > max_pause_count {
        let now = Utc::now().to_rfc3339();
        let audit = append_audit_event(
            conn,
            id,
            "abandoned",
            &format!(
                "pause_count {} would exceed max_pause_count {}; abandoning. {}",
                next_count, max_pause_count, reason
            ),
        )?;
        conn.execute(
            "UPDATE paused_dispatches
                SET status = 'abandoned',
                    abandoned_at = ?1,
                    audit_json = ?2
              WHERE id = ?3",
            rusqlite::params![now, audit, id],
        )?;
        // Surface the abandonment on loop_runs too.
        let _ = conn.execute(
            "UPDATE loop_runs
                SET paused_until = NULL,
                    paused_dispatch_id = NULL,
                    status = 'error',
                    error = COALESCE(error, ?1),
                    finished_at = COALESCE(finished_at, ?2)
              WHERE paused_dispatch_id = ?3",
            rusqlite::params![
                format!("pause-and-wake abandoned after {} pauses", max_pause_count),
                now,
                id
            ],
        );
        return Ok("abandoned");
    }
    let audit = append_audit_event(
        conn,
        id,
        "re_paused",
        &format!(
            "reset_at moved from prior to {}. pause_count {} → {}. {}",
            new_reset_at, pause_count, next_count, reason
        ),
    )?;
    conn.execute(
        "UPDATE paused_dispatches
            SET status = 'paused',
                reset_at = ?1,
                pause_count = ?2,
                audit_json = ?3
          WHERE id = ?4",
        rusqlite::params![new_reset_at, next_count, audit, id],
    )?;
    let _ = conn.execute(
        "UPDATE loop_runs SET paused_until = ?1 WHERE paused_dispatch_id = ?2",
        rusqlite::params![new_reset_at, id],
    );
    Ok("re_paused")
}

/// Scan for paused rows whose reset_at is now in the past. Called on
/// every CLI/desktop startup so a missed launchd fire still resumes
/// eventually. Returns ids in oldest-first order so a backlog drains
/// FIFO.
pub fn list_due(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM paused_dispatches
          WHERE status = 'paused'
            AND reset_at <= ?1
       ORDER BY reset_at ASC",
    )?;
    let now = Utc::now().to_rfc3339();
    let ids: Vec<String> = stmt
        .query_map([now], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(ids)
}

/// List rows that are still pending — for the loop UI's "paused"
/// state and for `ato loop list-paused` (future CLI command).
pub fn list_paused(conn: &Connection) -> Result<Vec<PausedDispatch>> {
    let mut stmt = conn.prepare(
        "SELECT id, runtime, reset_at, loop_run_id, step_id, prompt, model,
                agent_slug, workspace_root, pause_count, max_pause_count,
                status, paused_at, resumed_at, abandoned_at, audit_json,
                created_at
           FROM paused_dispatches
          WHERE status = 'paused'
       ORDER BY paused_at DESC",
    )?;
    let rows: Vec<PausedDispatch> = stmt
        .query_map([], |r| {
            Ok(PausedDispatch {
                id: r.get(0)?,
                runtime: r.get(1)?,
                reset_at: r.get(2)?,
                loop_run_id: r.get(3)?,
                step_id: r.get(4)?,
                prompt: r.get(5)?,
                model: r.get(6)?,
                agent_slug: r.get(7)?,
                workspace_root: r.get(8)?,
                pause_count: r.get(9)?,
                max_pause_count: r.get(10)?,
                status: r.get(11)?,
                paused_at: r.get(12)?,
                resumed_at: r.get(13)?,
                abandoned_at: r.get(14)?,
                audit_json: r.get(15)?,
                created_at: r.get(16)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn load_by_id(conn: &Connection, id: &str) -> Result<PausedDispatch> {
    let row = conn.query_row(
        "SELECT id, runtime, reset_at, loop_run_id, step_id, prompt, model,
                agent_slug, workspace_root, pause_count, max_pause_count,
                status, paused_at, resumed_at, abandoned_at, audit_json,
                created_at
           FROM paused_dispatches
          WHERE id = ?1",
        [id],
        |r| {
            Ok(PausedDispatch {
                id: r.get(0)?,
                runtime: r.get(1)?,
                reset_at: r.get(2)?,
                loop_run_id: r.get(3)?,
                step_id: r.get(4)?,
                prompt: r.get(5)?,
                model: r.get(6)?,
                agent_slug: r.get(7)?,
                workspace_root: r.get(8)?,
                pause_count: r.get(9)?,
                max_pause_count: r.get(10)?,
                status: r.get(11)?,
                paused_at: r.get(12)?,
                resumed_at: r.get(13)?,
                abandoned_at: r.get(14)?,
                audit_json: r.get(15)?,
                created_at: r.get(16)?,
            })
        },
    )?;
    Ok(row)
}

fn initial_audit_json(reset_at: &str) -> String {
    serde_json::json!({
        "events": [{
            "kind": "paused",
            "at": Utc::now().to_rfc3339(),
            "reset_at": reset_at,
        }],
    })
    .to_string()
}

fn append_audit_event(conn: &Connection, id: &str, kind: &str, note: &str) -> Result<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT audit_json FROM paused_dispatches WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .ok();
    let mut parsed: serde_json::Value = existing
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({ "events": [] }));
    if let Some(events) = parsed["events"].as_array_mut() {
        events.push(serde_json::json!({
            "kind": kind,
            "at": Utc::now().to_rfc3339(),
            "note": note,
        }));
    }
    Ok(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE paused_dispatches (
                id TEXT PRIMARY KEY,
                runtime TEXT NOT NULL,
                reset_at TEXT NOT NULL,
                loop_run_id TEXT,
                step_id TEXT,
                prompt TEXT NOT NULL,
                model TEXT,
                agent_slug TEXT,
                workspace_root TEXT,
                pause_count INTEGER NOT NULL DEFAULT 1,
                max_pause_count INTEGER NOT NULL DEFAULT 3,
                status TEXT NOT NULL DEFAULT 'paused',
                paused_at TEXT NOT NULL,
                resumed_at TEXT,
                abandoned_at TEXT,
                audit_json TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE loop_runs (
                id TEXT PRIMARY KEY,
                loop_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                started_at TEXT NOT NULL,
                finished_at TEXT,
                error TEXT,
                paused_until TEXT,
                paused_dispatch_id TEXT
            );",
        )
        .unwrap();
        c
    }

    fn insert_loop_run(c: &Connection, id: &str) {
        c.execute(
            "INSERT INTO loop_runs (id, loop_id, status, started_at)
             VALUES (?1, 'lp1', 'running', ?2)",
            rusqlite::params![id, Utc::now().to_rfc3339()],
        )
        .unwrap();
    }

    #[test]
    fn insert_new_persists_and_mirrors_to_loop_runs() {
        let c = open();
        insert_loop_run(&c, "lr-1");
        let id = insert_new(
            &c,
            InsertPausedDispatch {
                runtime: "codex",
                reset_at: "2026-07-10T11:22:00+00:00",
                loop_run_id: Some("lr-1"),
                step_id: Some("step-1"),
                prompt: "do the thing",
                model: None,
                agent_slug: None,
                workspace_root: None,
            },
        )
        .unwrap();
        // Row exists with paused status.
        let row = load_by_id(&c, &id).unwrap();
        assert_eq!(row.runtime, "codex");
        assert_eq!(row.status, "paused");
        assert_eq!(row.pause_count, 1);
        // Mirror updated on loop_runs.
        let (paused_until, paused_dispatch_id): (Option<String>, Option<String>) = c
            .query_row(
                "SELECT paused_until, paused_dispatch_id FROM loop_runs WHERE id='lr-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(paused_until.as_deref(), Some("2026-07-10T11:22:00+00:00"));
        assert_eq!(paused_dispatch_id.as_deref(), Some(id.as_str()));
    }

    #[test]
    fn claim_for_resume_transitions_status_and_blocks_double_claim() {
        let c = open();
        insert_loop_run(&c, "lr-2");
        let id = insert_new(
            &c,
            InsertPausedDispatch {
                runtime: "codex",
                reset_at: "2026-01-01T00:00:00+00:00",
                loop_run_id: Some("lr-2"),
                step_id: None,
                prompt: "p",
                model: None,
                agent_slug: None,
                workspace_root: None,
            },
        )
        .unwrap();
        let claimed = claim_for_resume(&c, &id).unwrap();
        assert_eq!(claimed.runtime, "codex");
        // Second claim must fail because status is now 'resuming'.
        assert!(claim_for_resume(&c, &id).is_err());
    }

    #[test]
    fn mark_resumed_clears_loop_runs_mirror() {
        let c = open();
        insert_loop_run(&c, "lr-3");
        let id = insert_new(
            &c,
            InsertPausedDispatch {
                runtime: "claude",
                reset_at: "2026-01-01T00:00:00+00:00",
                loop_run_id: Some("lr-3"),
                step_id: None,
                prompt: "p",
                model: None,
                agent_slug: None,
                workspace_root: None,
            },
        )
        .unwrap();
        claim_for_resume(&c, &id).unwrap();
        mark_resumed(&c, &id, "test resume").unwrap();
        let (paused_until, paused_dispatch_id): (Option<String>, Option<String>) = c
            .query_row(
                "SELECT paused_until, paused_dispatch_id FROM loop_runs WHERE id='lr-3'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(paused_until.is_none());
        assert!(paused_dispatch_id.is_none());
        // Status moved to 'resumed', resumed_at populated.
        let row = load_by_id(&c, &id).unwrap();
        assert_eq!(row.status, "resumed");
        assert!(row.resumed_at.is_some());
    }

    #[test]
    fn re_pause_increments_count_within_cap() {
        let c = open();
        insert_loop_run(&c, "lr-4");
        let id = insert_new(
            &c,
            InsertPausedDispatch {
                runtime: "codex",
                reset_at: "2026-01-01T00:00:00+00:00",
                loop_run_id: Some("lr-4"),
                step_id: None,
                prompt: "p",
                model: None,
                agent_slug: None,
                workspace_root: None,
            },
        )
        .unwrap();
        claim_for_resume(&c, &id).unwrap();
        let outcome = re_pause_or_abandon(&c, &id, "2026-12-01T00:00:00+00:00", "still rate-limited").unwrap();
        assert_eq!(outcome, "re_paused");
        let row = load_by_id(&c, &id).unwrap();
        assert_eq!(row.status, "paused");
        assert_eq!(row.pause_count, 2);
        assert_eq!(row.reset_at, "2026-12-01T00:00:00+00:00");
    }

    #[test]
    fn re_pause_abandons_when_count_exceeds_max() {
        let c = open();
        insert_loop_run(&c, "lr-5");
        let id = insert_new(
            &c,
            InsertPausedDispatch {
                runtime: "codex",
                reset_at: "2026-01-01T00:00:00+00:00",
                loop_run_id: Some("lr-5"),
                step_id: None,
                prompt: "p",
                model: None,
                agent_slug: None,
                workspace_root: None,
            },
        )
        .unwrap();
        // Already at pause_count=1 (first pause). Hit max_pause_count=3
        // by re-pausing twice (count 2, then 3), then re-pause again should
        // abandon.
        for _ in 0..2 {
            claim_for_resume(&c, &id).unwrap();
            assert_eq!(
                re_pause_or_abandon(&c, &id, "2026-12-01T00:00:00+00:00", "still out").unwrap(),
                "re_paused"
            );
        }
        // Now pause_count=3; one more would be 4 > max=3 → abandon.
        claim_for_resume(&c, &id).unwrap();
        let outcome =
            re_pause_or_abandon(&c, &id, "2026-12-01T00:00:00+00:00", "still out").unwrap();
        assert_eq!(outcome, "abandoned");
        let row = load_by_id(&c, &id).unwrap();
        assert_eq!(row.status, "abandoned");
        assert!(row.abandoned_at.is_some());
        // loop_runs propagation: status='error', finished_at populated.
        let (lr_status, lr_finished, lr_error): (String, Option<String>, Option<String>) = c
            .query_row(
                "SELECT status, finished_at, error FROM loop_runs WHERE id='lr-5'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(lr_status, "error");
        assert!(lr_finished.is_some());
        assert!(lr_error.unwrap().contains("pause-and-wake abandoned"));
    }

    #[test]
    fn list_due_returns_only_past_paused_rows_oldest_first() {
        let c = open();
        insert_loop_run(&c, "lr-a");
        insert_loop_run(&c, "lr-b");
        insert_loop_run(&c, "lr-c");
        let past_old = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let past_recent = (Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        let future = (Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
        let id_old = insert_new(
            &c,
            InsertPausedDispatch {
                runtime: "claude",
                reset_at: &past_old,
                loop_run_id: Some("lr-a"),
                step_id: None,
                prompt: "p",
                model: None,
                agent_slug: None,
                workspace_root: None,
            },
        )
        .unwrap();
        let id_recent = insert_new(
            &c,
            InsertPausedDispatch {
                runtime: "claude",
                reset_at: &past_recent,
                loop_run_id: Some("lr-b"),
                step_id: None,
                prompt: "p",
                model: None,
                agent_slug: None,
                workspace_root: None,
            },
        )
        .unwrap();
        let _id_future = insert_new(
            &c,
            InsertPausedDispatch {
                runtime: "claude",
                reset_at: &future,
                loop_run_id: Some("lr-c"),
                step_id: None,
                prompt: "p",
                model: None,
                agent_slug: None,
                workspace_root: None,
            },
        )
        .unwrap();
        let due = list_due(&c).unwrap();
        // Future row not returned. Older past row first.
        assert_eq!(due.len(), 2);
        assert_eq!(due[0], id_old);
        assert_eq!(due[1], id_recent);
    }
}
