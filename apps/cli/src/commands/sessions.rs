// v2.3.31 Phase 6 Slice A — sticky multi-turn sessions.
//
// ATO maintains its own session id (uuid). The dispatch path passes
// the session id through to the underlying runtime via its native
// resume mechanism:
//   - claude: --resume <claude-session-id>
//   - codex: resume <codex-session-id>   (Slice B)
//   - gemini: similar                    (Slice B)
//
// On the FIRST dispatch into an ATO session, the runtime_session_id
// column is NULL. The dispatch runs without --resume, then captures
// the runtime's native session id from --output-format json metadata
// and persists it back into the sessions row. SUBSEQUENT dispatches
// in that session pass --resume <runtime_session_id>.
//
// Slice A scope: claude support only. codex's signing cert is
// currently revoked which makes it unsafe to spawn anyway; once
// OpenAI ships a re-signed binary we'll add codex support as Slice
// A.1.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub id: String,
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub runtime_session_id: Option<String>,
    pub title: Option<String>,
    pub created_at: String,
    pub last_used_at: String,
    pub turn_count: i64,
}

fn has_table(conn: &Connection) -> bool {
    let c: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='sessions'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    c > 0
}

// v2.3.32 Slice A.2 — sessions work with claude (native --resume),
// and the API providers from the registry (history replay since
// they're stateless). Codex / Gemini still need their resume flag
// wiring (and codex needs its signing cert back); they'll land
// in Slice A.3 / A.4. Hermes / OpenClaw have no session story yet.
fn supported_runtimes() -> Vec<&'static str> {
    let mut v = vec!["claude"];
    for p in ato_api_providers::registry() {
        v.push(p.slug);
    }
    v
}

fn validate_runtime(runtime: &str) -> Result<()> {
    let supported = supported_runtimes();
    if !supported.contains(&runtime) {
        return Err(anyhow!(
            "Runtime '{}' is not yet supported by `ato sessions`. Currently: {}. Codex/Gemini land in follow-up slices.",
            runtime,
            supported.join(", ")
        ));
    }
    Ok(())
}

/// "native" runtimes maintain conversation state themselves; ATO
/// just hands them a resume token. "history_replay" runtimes are
/// stateless APIs; ATO rebuilds the prior conversation into the
/// messages array on every turn.
pub fn session_strategy(runtime: &str) -> &'static str {
    if runtime == "claude" {
        "native_resume"
    } else if ato_api_providers::is_api_provider(runtime) {
        "history_replay"
    } else {
        "unsupported"
    }
}

// ─── Turn history (dual-written by both strategies) ────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Turn {
    pub session_id: String,
    pub turn_index: i64,
    pub role: String,
    pub text: String,
    pub runtime: String,
    pub created_at: String,
}

/// Fetch all turns for a session in chronological order. Used by
/// history_replay dispatchers to rebuild the messages array.
pub fn fetch_turns(conn: &Connection, session_id: &str) -> Result<Vec<Turn>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, turn_index, role, text, runtime, created_at
           FROM session_turns
          WHERE session_id = ?1
          ORDER BY turn_index ASC",
    )?;
    let rows = stmt.query_map([session_id], |r| {
        Ok(Turn {
            session_id: r.get(0)?,
            turn_index: r.get(1)?,
            role: r.get(2)?,
            text: r.get(3)?,
            runtime: r.get(4)?,
            created_at: r.get(5)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Append one turn. Auto-increments turn_index by querying MAX+1.
/// Best-effort: a failure here doesn't fail the dispatch, it just
/// means the next turn won't see this one in context — surface via
/// log but don't propagate.
pub fn append_turn(
    conn: &Connection,
    session_id: &str,
    role: &str,
    text: &str,
    runtime: &str,
) -> Result<()> {
    let next_index: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(turn_index), -1) + 1 FROM session_turns WHERE session_id = ?1",
            [session_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO session_turns (session_id, turn_index, role, text, runtime, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![session_id, next_index, role, text, runtime, now],
    )?;
    Ok(())
}

pub fn new(
    conn: &Connection,
    runtime: String,
    agent_slug: Option<String>,
    title: Option<String>,
    opts: &Opts,
) -> Result<()> {
    if !has_table(conn) {
        return Err(anyhow!(
            "sessions table not found. Launch the ATO desktop (v2.3.31+) once to apply the migration."
        ));
    }
    validate_runtime(&runtime)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sessions (id, runtime, agent_slug, runtime_session_id, title, created_at, last_used_at, turn_count)
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?5, 0)",
        rusqlite::params![id, runtime, agent_slug, title, now],
    )
    .context("insert session row")?;
    let s = Session {
        id: id.clone(),
        runtime,
        agent_slug,
        runtime_session_id: None,
        title,
        created_at: now.clone(),
        last_used_at: now,
        turn_count: 0,
    };
    if opts.human {
        let title_part = s
            .title
            .as_deref()
            .map(|t| format!(" \"{}\"", t))
            .unwrap_or_default();
        emit_human(&format!(
            "Created session {}{} (runtime={}). Pass --session {} on the next `ato dispatch` to resume.",
            s.id, title_part, s.runtime, s.id
        ));
    } else {
        emit_json(&s)?;
    }
    Ok(())
}

pub fn list(conn: &Connection, limit: usize, opts: &Opts) -> Result<()> {
    if !has_table(conn) {
        if opts.human {
            emit_human("sessions table not found. Launch the ATO desktop (v2.3.31+) once.");
        } else {
            emit_json(&Vec::<Session>::new())?;
        }
        return Ok(());
    }
    let safe_limit = limit.min(10_000) as i64;
    let mut stmt = conn.prepare(
        "SELECT id, runtime, agent_slug, runtime_session_id, title, created_at, last_used_at, turn_count
           FROM sessions
          ORDER BY last_used_at DESC
          LIMIT ?1",
    )?;
    let rows = stmt.query_map([safe_limit], |r| {
        Ok(Session {
            id: r.get(0)?,
            runtime: r.get(1)?,
            agent_slug: r.get(2)?,
            runtime_session_id: r.get(3)?,
            title: r.get(4)?,
            created_at: r.get(5)?,
            last_used_at: r.get(6)?,
            turn_count: r.get(7)?,
        })
    })?;
    let sessions: Vec<Session> = rows.filter_map(|r| r.ok()).collect();
    if opts.human {
        if sessions.is_empty() {
            emit_human("No sessions yet. Try `ato sessions new --runtime claude` to start one.");
        } else {
            emit_human(&format!("{} sessions:", sessions.len()));
            for s in &sessions {
                let title = s.title.as_deref().unwrap_or("(untitled)");
                let resumed = if s.runtime_session_id.is_some() {
                    "resumable"
                } else {
                    "fresh"
                };
                emit_human(&format!(
                    "  {} [{}] {} turns={} {} — {}",
                    &s.id[..8.min(s.id.len())],
                    s.runtime,
                    resumed,
                    s.turn_count,
                    s.last_used_at,
                    title
                ));
            }
        }
    } else {
        emit_json(&sessions)?;
    }
    Ok(())
}

pub fn get(conn: &Connection, id: &str, opts: &Opts) -> Result<()> {
    if !has_table(conn) {
        return Err(anyhow!(
            "sessions table not found. Launch the ATO desktop (v2.3.31+) once."
        ));
    }
    let row: Option<Session> = conn
        .query_row(
            "SELECT id, runtime, agent_slug, runtime_session_id, title, created_at, last_used_at, turn_count
               FROM sessions WHERE id = ?1",
            [id],
            |r| {
                Ok(Session {
                    id: r.get(0)?,
                    runtime: r.get(1)?,
                    agent_slug: r.get(2)?,
                    runtime_session_id: r.get(3)?,
                    title: r.get(4)?,
                    created_at: r.get(5)?,
                    last_used_at: r.get(6)?,
                    turn_count: r.get(7)?,
                })
            },
        )
        .optional()?;
    match row {
        Some(s) => {
            if opts.human {
                emit_human(&format!(
                    "Session {}\n  runtime: {}\n  agent_slug: {}\n  runtime_session_id: {}\n  title: {}\n  created_at: {}\n  last_used_at: {}\n  turn_count: {}",
                    s.id,
                    s.runtime,
                    s.agent_slug.as_deref().unwrap_or("—"),
                    s.runtime_session_id.as_deref().unwrap_or("(none — first dispatch will set this)"),
                    s.title.as_deref().unwrap_or("(untitled)"),
                    s.created_at,
                    s.last_used_at,
                    s.turn_count,
                ));
            } else {
                emit_json(&s)?;
            }
            Ok(())
        }
        None => Err(anyhow!("No session with id '{}'.", id)),
    }
}

pub fn delete(conn: &Connection, id: &str, opts: &Opts) -> Result<()> {
    if !has_table(conn) {
        return Err(anyhow!("sessions table not found."));
    }
    let n = conn.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
    if opts.human {
        if n == 0 {
            emit_human(&format!("No session with id '{}' to delete.", id));
        } else {
            emit_human(&format!("Deleted session {}.", id));
        }
    } else {
        emit_json(&serde_json::json!({ "id": id, "deleted": n > 0 }))?;
    }
    Ok(())
}

// ─── Helpers used by dispatch.rs ──────────────────────────────────────

/// Look up a session by id. Returns Err if the table is missing OR
/// the session doesn't exist. Used by dispatch.rs's --session path.
pub fn lookup(conn: &Connection, id: &str) -> Result<Session> {
    let row: Option<Session> = conn
        .query_row(
            "SELECT id, runtime, agent_slug, runtime_session_id, title, created_at, last_used_at, turn_count
               FROM sessions WHERE id = ?1",
            [id],
            |r| {
                Ok(Session {
                    id: r.get(0)?,
                    runtime: r.get(1)?,
                    agent_slug: r.get(2)?,
                    runtime_session_id: r.get(3)?,
                    title: r.get(4)?,
                    created_at: r.get(5)?,
                    last_used_at: r.get(6)?,
                    turn_count: r.get(7)?,
                })
            },
        )
        .optional()?;
    row.ok_or_else(|| anyhow!("No session with id '{}'.", id))
}

/// Persist the runtime's native session id (captured from
/// --output-format json metadata) and bump turn_count + last_used_at.
/// Called by dispatch.rs after a successful dispatch in a session.
pub fn record_turn(
    conn: &Connection,
    session_id: &str,
    runtime_session_id: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    // Only overwrite runtime_session_id when we have one to set (the
    // first turn captures it; later turns reuse the same id and the
    // runtime CLI keeps the conversation going).
    match runtime_session_id {
        Some(rsid) => {
            conn.execute(
                "UPDATE sessions
                    SET last_used_at = ?1,
                        turn_count = turn_count + 1,
                        runtime_session_id = COALESCE(runtime_session_id, ?2)
                  WHERE id = ?3",
                rusqlite::params![now, rsid, session_id],
            )?;
        }
        None => {
            conn.execute(
                "UPDATE sessions
                    SET last_used_at = ?1, turn_count = turn_count + 1
                  WHERE id = ?2",
                rusqlite::params![now, session_id],
            )?;
        }
    }
    Ok(())
}
