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
    /// v2.6 Slice C — 'open' or 'closed'. Dispatch refuses to write
    /// turns into a 'closed' session; close()/reopen() use this to
    /// enforce idempotency. Defaults to 'open' on pre-v2.6 rows that
    /// predate the migration (COALESCE in the read).
    pub status: String,
}

// v2.7.14 — Closeable impl so sessions delegate to the shared
// `conversation_close::close_conversation` orchestrator instead of
// carrying an inline 250-line implementation. War-rooms + chats have
// used the shared path since v2.7.13; bringing sessions in matches
// the architecture (one prompt, one parser, one validator) and lets
// future fixes land in one file.
impl crate::commands::conversation_close::Closeable for Session {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_label(&self) -> &'static str {
        "session"
    }
    fn status(&self) -> &str {
        &self.status
    }
    fn stored_agent_slug(&self) -> Option<&str> {
        self.agent_slug.as_deref()
    }
    fn anchor_runtime(&self) -> Option<&str> {
        // Sessions DO have an anchor runtime (unlike war-rooms / chats).
        // The resolve_summarizer chain in conversation_close uses this
        // when --coordinator and --as aren't passed: if the anchor
        // runtime is a registered API provider, summarize there.
        Some(self.runtime.as_str())
    }
    fn existing_title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    fn fetch_turns(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<Vec<crate::commands::conversation_close::ConversationTurn>> {
        // Reuses the existing sessions::fetch_turns; maps Turn →
        // ConversationTurn (drop session_id / turn_index / created_at
        // — the orchestrator doesn't need them).
        //
        // v2.7.14 — close path caps at the most-recent 1000 turns.
        // Matches the LIMIT 1000 added to war_rooms + chats in
        // commit 737a3c6 (DoS / bill-shock guard from MiniMax dogfood
        // review — claude X6 from the sessions refactor war-room
        // 8E5D733D-…). Slicing happens in Rust (not via SQL LIMIT)
        // because the underlying `fetch_turns` is shared with
        // history-replay dispatchers that NEED the full transcript
        // for accurate replay — a SQL LIMIT there would silently
        // truncate replays. Scoping the cap to the Closeable impl
        // keeps replay correct + bounds the prompt-to-LLM at close
        // time. Typical session (5-50 turns) is unaffected.
        let mut turns = fetch_turns(conn, &self.id)?;
        const CLOSE_TURN_CAP: usize = 1000;
        if turns.len() > CLOSE_TURN_CAP {
            // Drop the oldest (turns.len() - CAP) turns; keep the
            // LATEST 1000 in chronological order.
            let drop_count = turns.len() - CLOSE_TURN_CAP;
            turns.drain(0..drop_count);
        }
        Ok(turns
            .into_iter()
            .map(|t| crate::commands::conversation_close::ConversationTurn {
                role: t.role,
                text: t.text,
                runtime: t.runtime,
            })
            .collect())
    }
    fn persist_close(
        &self,
        conn: &rusqlite::Connection,
        fields: &crate::commands::conversation_close::CloseFields,
    ) -> Result<usize> {
        // PR 3 stickiness — category/team/human_comment use COALESCE so
        // a later close (after reopen) without an explicit replacement
        // preserves the prior value. project_id is conditional: only
        // included in the SQL when the coordinator suggested one AND
        // the row doesn't already have one (COALESCE inside the
        // clause), matching the pre-refactor behavior at sessions.rs's
        // old inline UPDATE. status guard satisfies the Closeable
        // contract (changed == 0 ⇒ raced).
        let tags_json =
            serde_json::to_string(&fields.tags).unwrap_or_else(|_| "[]".to_string());
        let project_id_clause = if fields.project_id.is_some() {
            ", project_id = COALESCE(project_id, ?)"
        } else {
            ""
        };
        let sql = format!(
            "UPDATE sessions
                SET status = 'closed',
                    closed_at = ?,
                    auto_title = ?,
                    summary = ?,
                    tags_json = ?,
                    category = COALESCE(?, category),
                    team = COALESCE(?, team),
                    human_comment = COALESCE(?, human_comment){}
              WHERE id = ? AND status = 'open'",
            project_id_clause
        );
        let changed = if let Some(pid) = fields.project_id.as_ref() {
            conn.execute(
                &sql,
                rusqlite::params![
                    fields.closed_at,
                    fields.auto_title,
                    fields.summary,
                    tags_json,
                    fields.category,
                    fields.team,
                    fields.human_comment,
                    pid,
                    self.id
                ],
            )?
        } else {
            conn.execute(
                &sql,
                rusqlite::params![
                    fields.closed_at,
                    fields.auto_title,
                    fields.summary,
                    tags_json,
                    fields.category,
                    fields.team,
                    fields.human_comment,
                    self.id
                ],
            )?
        };
        Ok(changed)
    }
    fn persist_reopen(&self, conn: &rusqlite::Connection) -> Result<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let changed = conn.execute(
            "UPDATE sessions
                SET status = 'open',
                    closed_at = NULL,
                    last_used_at = ?1
              WHERE id = ?2 AND status = 'closed'",
            rusqlite::params![now, self.id],
        )?;
        Ok(changed)
    }
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

// 2026-05-19 (Will dogfood) — opened sessions to every runtime.
// History-replay is the universal fallback: dispatch.rs:477-501
// already prefixes prior turns into the prompt for any non-anchor
// runtime, which is exactly what "stateless API replay" did for the
// api_providers. Codex / Gemini / OpenClaw / Hermes get the same
// treatment — they're CLI subprocesses that ATO hands a stitched
// prompt to. Native resume (claude --resume) is still the
// fast-path for claude-anchored sessions; everything else gets
// history_replay.
//
// Previous gating ("Codex / Gemini still need their resume flag
// wiring") punted on the wrong question — native resume is the
// optimization, not the requirement. Replay works for any prompt-
// in / text-out runtime, which is all of them.
fn supported_runtimes() -> Vec<&'static str> {
    let mut v = vec!["claude", "codex", "gemini", "openclaw", "hermes"];
    for p in ato_api_providers::registry() {
        v.push(p.slug);
    }
    v
}

fn validate_runtime(runtime: &str) -> Result<()> {
    let supported = supported_runtimes();
    if !supported.contains(&runtime) {
        return Err(anyhow!(
            "Runtime '{}' is not in the registry. Currently: {}.",
            runtime,
            supported.join(", ")
        ));
    }
    Ok(())
}

/// "native_resume" runtimes maintain conversation state themselves
/// and accept a resume token (claude today). "history_replay"
/// runtimes are prompt-in / text-out — ATO rebuilds the prior
/// conversation into the prompt on every turn. After 2026-05-19
/// every non-claude runtime falls into history_replay; future
/// optimization can promote codex/gemini to native_resume once
/// their --resume wiring lands, but until then replay is correct
/// and works.
pub fn session_strategy(runtime: &str) -> &'static str {
    if runtime == "claude" {
        "native_resume"
    } else {
        "history_replay"
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
    agent_slug: Option<&str>,
) -> Result<()> {
    let next_index: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(turn_index), -1) + 1 FROM session_turns WHERE session_id = ?1",
            [session_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let now = chrono::Utc::now().to_rfc3339();
    // Per-message attribution: every turn carries who/where on its own
    // row (FOLLOWUPS #3). Detect at append-time so live-attached agents
    // mid-conversation show up correctly per-message instead of
    // inheriting the session-open initiator.
    let attribution = crate::attribution::Attribution::detect();
    conn.execute(
        "INSERT INTO session_turns (session_id, turn_index, role, text, runtime, created_at, agent_slug, initiator_kind, client_surface, initiator_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![session_id, next_index, role, text, runtime, now, agent_slug, attribution.kind, attribution.surface, attribution.id],
    )?;
    Ok(())
}

/// PR 11 — validate a candidate project_id against the projects
/// table. See the three-state model in the comment above
/// `resolved_project_id` in `create_inner`. Extracted so the unit
/// tests can exercise each branch deterministically (codex Round-1
/// #4: missing test coverage on the validation paths).
pub(crate) fn resolve_project_id(conn: &Connection, pid: &str) -> Option<String> {
    let has_projects = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='projects'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_projects {
        // State 1: no projects table. Silent None — fresh CLI-only
        // install before the desktop's first run.
        return None;
    }
    let exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM projects WHERE id = ?1",
            [pid],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists > 0 {
        // State 3: id resolved.
        Some(pid.to_string())
    } else {
        // State 2: table exists, id missing. Real bug surface —
        // warn so it's not silently invisible.
        eprintln!(
            "warn: --project '{}' not found in projects table; session created project-less. \
             If you expected this project to exist, check the desktop sidebar selector or run `ato projects list`.",
            pid
        );
        None
    }
}

/// Programmatic session creation — no stdout side effects. Used by
/// callers like `ato review` that orchestrate sessions on the user's
/// behalf and shouldn't double-emit the "created session X" line.
///
/// PR 11 (2026-05-17) — `project_id` is now snapshotted at create
/// time instead of being filled only by the close-time coordinator's
/// `suggested_project_id`. When the desktop's active-project sidebar
/// is set, every new session inherits that id; the close coordinator
/// can still refine it (via COALESCE behavior in close()). Old call
/// sites that don't pass a project_id pass None and stay project-less
/// until close.
pub fn create_inner(
    conn: &Connection,
    runtime: &str,
    agent_slug: Option<&str>,
    title: Option<&str>,
    project_id: Option<&str>,
) -> Result<Session> {
    if !has_table(conn) {
        return Err(anyhow!(
            "sessions table not found. Launch the ATO desktop (v2.3.31+) once to apply the migration."
        ));
    }
    validate_runtime(runtime)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    // PR 11 — project_id validated against the projects table.
    // Three distinct states, only one of which warns:
    //   1. projects table missing → return None silently. A fresh
    //      CLI-only install (no desktop ever opened) has no projects
    //      table, and that's a valid state — nothing to validate
    //      against, so the field stays NULL.
    //   2. table present + id NOT in it → return None with a stderr
    //      warning. This branch indicates a real wiring bug (UI cache
    //      stale, or someone passed a stray id via --project). Codex
    //      Round-1 #2: "make dropped project_id observable."
    //   3. table present + id IS in it → use it.
    // Tolerant on insert because losing the snapshot is recoverable
    // (close-time coordinator can re-suggest); a hard failure on a
    // stale UI cache would be a worse UX.
    let resolved_project_id: Option<String> = match project_id {
        Some(pid) if !pid.is_empty() => resolve_project_id(conn, pid),
        _ => None,
    };
    // v2.16 attribution — resolve initiator provenance for the session row.
    let attribution = crate::attribution::Attribution::detect();
    conn.execute(
        "INSERT INTO sessions (id, runtime, agent_slug, runtime_session_id, title, created_at, last_used_at, turn_count, project_id, initiator_kind, client_surface, initiator_id)
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?5, 0, ?6, ?7, ?8, ?9)",
        rusqlite::params![id, runtime, agent_slug, title, now, resolved_project_id, attribution.kind, attribution.surface, attribution.id],
    )
    .context("insert session row")?;
    Ok(Session {
        id,
        runtime: runtime.to_string(),
        agent_slug: agent_slug.map(String::from),
        runtime_session_id: None,
        title: title.map(String::from),
        created_at: now.clone(),
        last_used_at: now,
        turn_count: 0,
        status: "open".to_string(),
    })
}

pub fn new(
    conn: &Connection,
    runtime: String,
    agent_slug: Option<String>,
    title: Option<String>,
    project_id: Option<String>,
    opts: &Opts,
) -> Result<()> {
    let s = create_inner(conn, &runtime, agent_slug.as_deref(), title.as_deref(), project_id.as_deref())?;
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
        "SELECT id, runtime, agent_slug, runtime_session_id, title, created_at, last_used_at, turn_count,
                COALESCE(status, 'open')
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
            status: r.get(8)?,
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
            "SELECT id, runtime, agent_slug, runtime_session_id, title, created_at, last_used_at, turn_count,
                    COALESCE(status, 'open')
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
                    status: r.get(8)?,
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
            "SELECT id, runtime, agent_slug, runtime_session_id, title, created_at, last_used_at, turn_count,
                    COALESCE(status, 'open')
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
                    status: r.get(8)?,
                })
            },
        )
        .optional()?;
    row.ok_or_else(|| anyhow!("No session with id '{}'.", id))
}

// ─── v2.6 Slice C — close / reopen lifecycle ────────────────────────────
//
// Closing a session is the user's signal that the conversation is
// "done for now" and worth summarizing. The session's coordinator
// (resolved from the explicit --as override, else session.agent_slug,
// else a generic summarizer on the session's anchor runtime) consumes
// the full turn history and returns a single JSON object:
//
//   {
//     "title": "...",            // 6-10 words, human-readable
//     "summary": "...",          // 2-4 sentences, what was decided
//     "tags": ["...", "..."],    // 2-4 short topic tags
//     "suggested_project_id": "..." // optional, null when no good match
//     "category": "Dev",         // PR 3 — strict vocab, see ALLOWED_CATEGORIES
//     "team": "founder"          // PR 3 — free-form band label
//   }
//
// We persist all six on the sessions row, flip status='closed', and
// stamp closed_at. Reopen reverts to status='open'; the next close
// overwrites the summary fields with the refreshed transcript.
//
// PR 3 (Sessions UX polish, 2026-05-17) added `category` + `team` to
// the close contract. Category is gated by a controlled vocabulary so
// UI filters can rely on it; an out-of-vocab value is a parse-time
// hard fail (clearer than letting the SQL CHECK trip later). A NULL/
// missing category is a soft warning — the session still closes, but
// stderr surfaces "category not provided by coordinator" so future
// listings can flag the gap. `--force-close-without-context`
// suppresses the warning for users who deliberately close without
// taxonomy (e.g. throwaway smoke tests). The flag does NOT downgrade
// the out-of-vocab hard fail — garbage never reaches the column.
//
// **Stickiness asymmetry (codex Round-1 finding #4):** category +
// team are *sticky* — a later close that fails to elicit them does
// NOT erase the prior values (UPDATE uses COALESCE on both, so NULL
// from the parser leaves the existing value alone). The other close
// outputs (auto_title, summary, tags_json) DO refresh on every close
// because they're per-conversation derivatives. Taxonomy is a label
// on the *session*; once a human or coordinator has labelled it, a
// weaker re-close shouldn't undo that work.
//
// The LLM is invoked via api_dispatch::dispatch_with_history when the
// coordinator's runtime is an API provider (Anthropic/Minimax/OpenAI/
// Google/etc.). For native-resume runtimes (claude CLI), we fall back
// to the user's first registered API provider — close-and-summarize is
// a small focused call where the model that ran the conversation
// doesn't have to be the model that summarizes it.

#[derive(Debug, Clone, Serialize)]
pub struct SessionCloseResult {
    pub id: String,
    pub status: String,
    pub auto_title: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub project_id: Option<String>,
    pub category: Option<String>,
    pub team: Option<String>,
    pub coordinator_runtime: String,
    pub coordinator_model: Option<String>,
    pub duration_ms: i64,
    /// v2.7.12 — the human's free-form note attached at close time.
    /// Echoes whatever was passed via `--human-comment` (normalized:
    /// trimmed, empty → None). Null when the caller didn't pass one.
    pub human_comment: Option<String>,
}


/// v2.7.14 — thin wrapper around the shared
/// `commands::conversation_close::close_conversation` orchestrator.
/// Pre-refactor this was a ~280-line inline implementation; war-rooms
/// + chats had already migrated to the shared path in v2.7.13. Now
/// sessions joins them: one prompt, one parser, one validator, one
/// place for future fixes to land. The `Closeable` impl on `Session`
/// (above) supplies the session-specific bits: anchor runtime,
/// stored agent slug, transcript source, UPDATE shape with sticky
/// COALESCE on category/team/human_comment + conditional project_id.
#[allow(clippy::too_many_arguments)]
pub fn close(
    conn: &Connection,
    id: &str,
    agent_slug_override: Option<String>,
    model_override: Option<String>,
    coordinator_override: Option<String>,
    human_comment: Option<String>,
    force_close_without_context: bool,
    opts: &Opts,
) -> Result<()> {
    if !has_table(conn) {
        return Err(anyhow!(
            "sessions table not found. Launch the ATO desktop (v2.3.31+) once to apply the migration."
        ));
    }
    let session = lookup(conn, id)?;
    let fields = crate::commands::conversation_close::close_conversation(
        conn,
        &session,
        agent_slug_override.as_deref(),
        model_override.as_deref(),
        coordinator_override.as_deref(),
        human_comment.as_deref(),
        force_close_without_context,
        opts,
    )?;
    // Resolve the turn count for the human-mode output. The orchestrator
    // fetched the transcript internally; we don't need to refetch, but
    // we DO need a count for the "(N turns)" line. fetch_turns is cheap
    // (indexed) — accept the small redundancy in exchange for not
    // threading the count back through the trait.
    let turn_count = fetch_turns(conn, &session.id).map(|t| t.len()).unwrap_or(0);

    // Emit the SessionCloseResult wire shape (kept for backwards-compat
    // with the desktop's parser — see sessions_view/write.rs which
    // deserializes the JSON stdout into CloseSessionResult).
    let result = SessionCloseResult {
        id: session.id.clone(),
        status: "closed".to_string(),
        auto_title: fields.auto_title.clone(),
        summary: fields.summary.clone(),
        tags: fields.tags.clone(),
        project_id: fields.project_id.clone(),
        category: fields.category.clone(),
        team: fields.team.clone(),
        coordinator_runtime: fields.coordinator_runtime.clone(),
        coordinator_model: fields.coordinator_model.clone(),
        duration_ms: fields.duration_ms,
        human_comment: fields.human_comment.clone(),
    };

    if opts.human {
        emit_human(&format!(
            "Closed session {} ({} turns).\n  title: {}\n  summary: {}\n  tags: {}\n  category: {}\n  team: {}\n  coordinator: {} ({}) in {}ms{}",
            session.id,
            turn_count,
            fields.auto_title.as_deref().unwrap_or("(none)"),
            fields.summary.as_deref().unwrap_or("(none)"),
            if fields.tags.is_empty() { "(none)".to_string() } else { fields.tags.join(", ") },
            fields.category.as_deref().unwrap_or("(none)"),
            fields.team.as_deref().unwrap_or("(none)"),
            fields.coordinator_runtime,
            fields.coordinator_model.as_deref().unwrap_or("(unknown)"),
            fields.duration_ms,
            fields
                .coordinator_slug
                .as_deref()
                .map(|s| format!("\n  agent: @{}", s))
                .unwrap_or_default(),
        ));
    } else {
        emit_json(&result)?;
    }
    Ok(())
}


/// v2.7.14 — thin wrapper around `conversation_close::reopen_conversation`.
/// Preserves the v2.3.31+ `has_table` guard with the friendly error
/// (war-room review claude X5) so a fresh-clone without the migration
/// still gets actionable advice instead of a raw rusqlite "no such
/// table." The session-specific human/JSON output stays here.
pub fn reopen(conn: &Connection, id: &str, opts: &Opts) -> Result<()> {
    if !has_table(conn) {
        return Err(anyhow!(
            "sessions table not found. Launch the ATO desktop (v2.3.31+) once."
        ));
    }
    let session = lookup(conn, id)?;
    crate::commands::conversation_close::reopen_conversation(conn, &session)?;
    if opts.human {
        emit_human(&format!(
            "Reopened session {}. Continue with `ato dispatch <runtime> \"...\" --session {}` — the next close will refresh the summary.",
            id, id
        ));
    } else {
        emit_json(&serde_json::json!({ "id": id, "status": "open" }))?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;


    // PR 11 — resolve_project_id paths. Codex Round-1 #4: cover
    // the three-state model explicitly so the tolerant-by-design
    // behavior is intentional not accidental.

    fn fresh_conn_no_projects_table() -> rusqlite::Connection {
        // In-memory DB with no `projects` table at all — simulates a
        // CLI-only fresh install before the desktop has ever run.
        rusqlite::Connection::open_in_memory().unwrap()
    }

    fn fresh_conn_with_projects_table() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL)",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn resolve_project_id_no_projects_table_returns_none() {
        let conn = fresh_conn_no_projects_table();
        // Even with a real-looking id, the absence of the projects
        // table is a State 1: silent None, no warn.
        assert_eq!(resolve_project_id(&conn, "any-id"), None);
    }

    #[test]
    fn resolve_project_id_table_present_id_missing_returns_none() {
        let conn = fresh_conn_with_projects_table();
        // No row matches; State 2: returns None (and emits a stderr
        // warning, which we don't capture here but is observable in
        // integration). The session create stays tolerant.
        assert_eq!(resolve_project_id(&conn, "ghost-id"), None);
    }

    #[test]
    fn resolve_project_id_valid_id_resolves() {
        let conn = fresh_conn_with_projects_table();
        conn.execute(
            "INSERT INTO projects (id, name) VALUES (?1, ?2)",
            ["proj-abc", "ATO"],
        )
        .unwrap();
        // State 3: id resolves, returned as-is.
        assert_eq!(
            resolve_project_id(&conn, "proj-abc"),
            Some("proj-abc".to_string())
        );
    }

    #[test]
    fn create_inner_with_valid_project_persists() {
        // End-to-end check: a session created with a known project_id
        // ends up with that id on the persisted row. Builds the
        // minimum sessions schema in-memory (no migration runner
        // available in the test crate).
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                runtime TEXT NOT NULL,
                agent_slug TEXT,
                runtime_session_id TEXT,
                title TEXT,
                created_at TEXT NOT NULL,
                last_used_at TEXT NOT NULL,
                turn_count INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'open',
                project_id TEXT,
                initiator_kind TEXT,
                client_surface TEXT,
                initiator_id TEXT
             );
             INSERT INTO projects (id, name) VALUES ('proj-abc', 'ATO');",
        )
        .unwrap();
        let s = create_inner(&conn, "claude", None, Some("test"), Some("proj-abc"))
            .expect("create_inner should succeed");
        let stored: Option<String> = conn
            .query_row(
                "SELECT project_id FROM sessions WHERE id = ?1",
                [&s.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, Some("proj-abc".to_string()));
    }

    #[test]
    fn create_inner_empty_project_id_persists_null() {
        // Empty string is treated as "no project" (same as None).
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                runtime TEXT NOT NULL,
                agent_slug TEXT,
                runtime_session_id TEXT,
                title TEXT,
                created_at TEXT NOT NULL,
                last_used_at TEXT NOT NULL,
                turn_count INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'open',
                project_id TEXT,
                initiator_kind TEXT,
                client_surface TEXT,
                initiator_id TEXT
             );",
        )
        .unwrap();
        let s = create_inner(&conn, "claude", None, None, Some(""))
            .expect("create_inner with empty project_id should succeed");
        let stored: Option<String> = conn
            .query_row(
                "SELECT project_id FROM sessions WHERE id = ?1",
                [&s.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(stored.is_none(), "empty project_id should land as NULL");
    }
}
