// v2.3.42 — Sessions view for the GUI.
//
// Phase 6 sessions (Slice A + A.2 + B) are CLI-first today: they live
// in the `sessions` + `session_turns` tables and the CLI exposes
// `ato sessions ...` and `ato bridge`. The desktop GUI never had a
// first-class surface for them — they only appeared incidentally
// under Execution Logs after v2.3.41's grouping.
//
// This module adds two Tauri commands:
//   - list_sessions_full   — overview rows for the Sessions tab list
//   - get_session_transcript — every turn for one session, ordered
//
// Both are read-only. Continuing a session from the GUI uses the
// existing prompt_agent flow with an extra session_id parameter
// (wired separately so this module stays narrowly scoped to the
// view layer).

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::DbState;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionListRow {
    pub id: String,
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub title: Option<String>,
    pub created_at: String,
    pub last_used_at: String,
    pub turn_count: i64,
    /// Distinct runtimes that appear in this session's turns. For a
    /// claude-only session this is `["claude"]`. For a Slice B
    /// cross-runtime conversation it's e.g. `["claude", "minimax"]`.
    /// Drives the runtime badges in the list UI.
    pub runtimes_used: Vec<String>,
    /// Last (assistant) turn's text, truncated. Gives the user a
    /// "what was this conversation about" preview without expanding.
    pub last_assistant_preview: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionTurn {
    pub turn_index: i64,
    pub role: String,
    pub text: String,
    pub runtime: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionTranscript {
    pub id: String,
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub title: Option<String>,
    pub turns: Vec<SessionTurn>,
}

#[tauri::command]
pub fn list_sessions_full(
    db: State<'_, DbState>,
    limit: Option<i64>,
) -> Result<Vec<SessionListRow>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_sessions_inner(&conn, limit.unwrap_or(50)).map_err(|e| e.to_string())
}

fn list_sessions_inner(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<SessionListRow>> {
    // Some installs have older `sessions` rows from before the v2.3.31
    // migration completed. Tolerate missing columns by SELECTing only
    // what we know is always there + joining session_turns for the
    // computed fields. Fallback paths if the tables don't exist at all
    // are folded into the `?` chain — caller surfaces the string error.
    let mut stmt = conn.prepare(
        "SELECT s.id, s.runtime, s.agent_slug, s.title, s.created_at, s.last_used_at, s.turn_count
           FROM sessions s
          ORDER BY s.last_used_at DESC
          LIMIT ?1",
    )?;
    let rows: Vec<SessionListRow> = stmt
        .query_map([limit], |r| {
            Ok(SessionListRow {
                id: r.get(0)?,
                runtime: r.get(1)?,
                agent_slug: r.get(2)?,
                title: r.get(3)?,
                created_at: r.get(4)?,
                last_used_at: r.get(5)?,
                turn_count: r.get(6)?,
                runtimes_used: Vec::new(),
                last_assistant_preview: None,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Enrich each row with computed fields. Two cheap follow-up queries
    // per session — fine for the default limit of 50, and the indexes
    // on session_turns(session_id, turn_index ASC) make them O(log N).
    let mut enriched = Vec::with_capacity(rows.len());
    for mut row in rows {
        // Distinct runtimes in this session's turns. For Slice B
        // cross-runtime sessions this is what makes the multi-badge UI
        // render correctly.
        let mut rt_stmt = conn.prepare_cached(
            "SELECT DISTINCT runtime FROM session_turns WHERE session_id = ?1 ORDER BY turn_index ASC",
        )?;
        let runtimes: Vec<String> = rt_stmt
            .query_map([&row.id], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        // Fall back to the session's anchor runtime when session_turns
        // is empty (e.g. a freshly opened session before its first
        // dispatch lands a turn).
        row.runtimes_used = if runtimes.is_empty() {
            vec![row.runtime.clone()]
        } else {
            runtimes
        };

        // Last assistant turn → preview. Order by turn_index DESC so we
        // get the chronologically last assistant message, not whichever
        // arrived first.
        let mut last_stmt = conn.prepare_cached(
            "SELECT text FROM session_turns
              WHERE session_id = ?1 AND role = 'assistant'
              ORDER BY turn_index DESC
              LIMIT 1",
        )?;
        let preview: Option<String> = last_stmt
            .query_row([&row.id], |r| r.get::<_, String>(0))
            .ok();
        row.last_assistant_preview = preview.map(|s| {
            // Trim to 160 chars max so list rows stay one-line on most
            // viewports. The full text is available in the transcript.
            if s.chars().count() > 160 {
                let truncated: String = s.chars().take(160).collect();
                format!("{}…", truncated)
            } else {
                s
            }
        });

        enriched.push(row);
    }
    Ok(enriched)
}

#[tauri::command]
pub fn get_session_transcript(
    db: State<'_, DbState>,
    session_id: String,
) -> Result<SessionTranscript, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let (runtime, agent_slug, title): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT runtime, agent_slug, title FROM sessions WHERE id = ?1",
            [&session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| format!("session not found: {}", e))?;

    let mut stmt = conn
        .prepare(
            "SELECT turn_index, role, text, runtime, created_at
               FROM session_turns
              WHERE session_id = ?1
              ORDER BY turn_index ASC",
        )
        .map_err(|e| e.to_string())?;

    let turns: Vec<SessionTurn> = stmt
        .query_map([&session_id], |r| {
            Ok(SessionTurn {
                turn_index: r.get(0)?,
                role: r.get(1)?,
                text: r.get(2)?,
                runtime: r.get(3)?,
                created_at: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(SessionTranscript {
        id: session_id,
        runtime,
        agent_slug,
        title,
        turns,
    })
}
