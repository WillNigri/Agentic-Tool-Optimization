// commands/chat_threads.rs — v1.5+ persistent chat threads (bottom-pane
// Chat tab data layer).
//
// PR 15 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (8 commands):
//   - list_chat_threads          — paged list (optional project filter)
//   - create_chat_thread         — new thread (default title fallback)
//   - rename_chat_thread         — update title
//   - delete_chat_thread         — cascade delete thread + messages
//   - set_chat_thread_agent      — change sticky agent default
//   - search_chat_threads        — ⌘K full-text search (title + content)
//   - get_chat_messages          — read messages for one thread
//   - append_chat_message        — write one message + bump thread counters
//   - delete_chat_message        — remove + recompute counters
//
// Plus the data shapes (ChatThread / ChatMessage / ChatThreadSearchHit).
//
// Path A consolidation (2026-05-18) is implemented in the desktop's
// sessions_view.rs — those reads UNION chat_threads into the Sessions
// feed. The migration to a unified `sessions` table (Path B Stage 2)
// is war-room-worthy and held for a future PR; today this domain
// stays its own table with its own commands.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::DbState;

// ── Chat threads (v1.5 — sustained sessions) ─────────────────────────────

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatThread {
    pub id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub created_at: String,
    pub last_message_at: Option<String>,
    pub message_count: i64,
    pub archived: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub runtime: Option<String>,
    pub agent_slug: Option<String>,
    pub metadata: Option<String>,
    pub created_at: String,
}

#[tauri::command]
pub fn list_chat_threads(
    db: State<'_, DbState>,
    project_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<ChatThread>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let cap = limit.unwrap_or(50).clamp(1, 500);
    // When project_id is set, restrict to that project; when None, return
    // all (global + project-scoped). NULL match in SQL is a pain — split.
    let (sql, params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = match project_id {
        Some(p) => (
            "SELECT id, title, project_id, agent_id, created_at, last_message_at, message_count, archived
             FROM chat_threads
             WHERE project_id = ?1 AND archived = 0
             ORDER BY COALESCE(last_message_at, created_at) DESC
             LIMIT ?2",
            vec![Box::new(p), Box::new(cap)],
        ),
        None => (
            "SELECT id, title, project_id, agent_id, created_at, last_message_at, message_count, archived
             FROM chat_threads
             WHERE archived = 0
             ORDER BY COALESCE(last_message_at, created_at) DESC
             LIMIT ?1",
            vec![Box::new(cap)],
        ),
    };
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| &**b).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(param_refs.iter()), |row| {
            Ok(ChatThread {
                id: row.get(0)?,
                title: row.get(1)?,
                project_id: row.get(2)?,
                agent_id: row.get(3)?,
                created_at: row.get(4)?,
                last_message_at: row.get(5)?,
                message_count: row.get(6)?,
                archived: row.get::<_, i32>(7)? != 0,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn create_chat_thread(
    db: State<'_, DbState>,
    title: String,
    project_id: Option<String>,
    agent_id: Option<String>,
) -> Result<ChatThread, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let trimmed = if title.trim().is_empty() {
        "New conversation".to_string()
    } else {
        title.trim().chars().take(120).collect()
    };
    conn.execute(
        "INSERT INTO chat_threads (id, title, project_id, agent_id, created_at, last_message_at, message_count, archived, initiator_kind, client_surface, initiator_id)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, 0, 0, 'human', 'desktop', NULL)",
        params![id, trimmed, project_id, agent_id, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(ChatThread {
        id,
        title: trimmed,
        project_id,
        agent_id,
        created_at: now,
        last_message_at: None,
        message_count: 0,
        archived: false,
    })
}

#[tauri::command]
pub fn rename_chat_thread(
    db: State<'_, DbState>,
    id: String,
    title: String,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let trimmed: String = title.trim().chars().take(120).collect();
    if trimmed.is_empty() {
        return Err("title-empty".into());
    }
    conn.execute(
        "UPDATE chat_threads SET title = ?1 WHERE id = ?2",
        params![trimmed, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_chat_thread(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // ON DELETE CASCADE on chat_messages handles the rows, but the FK is
    // only honored when foreign_keys = ON. Defense in depth: delete both.
    conn.execute("DELETE FROM chat_messages WHERE thread_id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM chat_threads WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_chat_thread_agent(
    db: State<'_, DbState>,
    id: String,
    agent_id: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE chat_threads SET agent_id = ?1 WHERE id = ?2",
        params![agent_id, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// v2.1.7+ — Search across persistent chat threads.
///
/// Powers ⌘K's new "Conversations" corpus. Two LIKE passes:
/// (1) match the query against thread titles (cheap), (2) match against
/// chat_messages.content (more expensive but bounded by limit). Results
/// are deduped by thread_id with title-match preferred over content-match.
///
/// Caller passes a free-text query; we wrap it with `%` and case-fold via
/// LOWER() so a search for "binary" matches a thread titled "Binary
/// Search Explained" or any message body containing "binary". Limit is
/// clamped to a reasonable ceiling so an empty query doesn't dump the
/// whole table.
#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadSearchHit {
    pub thread: ChatThread,
    /// "title" or "content" — lets the UI distinguish how the match landed.
    pub match_kind: String,
    /// First ~120 chars of the matching message content when match_kind=content.
    /// None when match_kind=title.
    pub snippet: Option<String>,
}

#[tauri::command]
pub fn search_chat_threads(
    db: State<'_, DbState>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<ChatThreadSearchHit>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let cap = limit.unwrap_or(20).clamp(1, 100);
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = format!("%{}%", trimmed.to_lowercase());

    let mut hits: Vec<ChatThreadSearchHit> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Pass 1: title matches. Cheap, runs against the indexed title col.
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, title, project_id, agent_id, created_at, last_message_at, message_count, archived
                 FROM chat_threads
                 WHERE archived = 0 AND LOWER(title) LIKE ?1
                 ORDER BY COALESCE(last_message_at, created_at) DESC
                 LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![pattern, cap], |row| {
                Ok(ChatThread {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    project_id: row.get(2)?,
                    agent_id: row.get(3)?,
                    created_at: row.get(4)?,
                    last_message_at: row.get(5)?,
                    message_count: row.get(6)?,
                    archived: row.get::<_, i64>(7)? != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        for r in rows {
            let t = r.map_err(|e| e.to_string())?;
            if seen.insert(t.id.clone()) {
                hits.push(ChatThreadSearchHit { thread: t, match_kind: "title".into(), snippet: None });
            }
        }
    }

    // Pass 2: content matches, capped to the remaining slots so a busy
    // user with 1000 messages doesn't timeout the palette. We DISTINCT
    // on thread_id and pick a single representative snippet via the
    // most recent matching message.
    let remaining = cap.saturating_sub(hits.len() as i64);
    if remaining > 0 {
        let mut stmt = conn
            .prepare(
                "SELECT t.id, t.title, t.project_id, t.agent_id, t.created_at, t.last_message_at,
                        t.message_count, t.archived, m.content
                   FROM chat_messages m
                   JOIN chat_threads t ON t.id = m.thread_id
                  WHERE t.archived = 0 AND LOWER(m.content) LIKE ?1
                  ORDER BY m.created_at DESC
                  LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;
        // Pull more rows than needed because dedup will drop already-seen
        // threads. 4× the remaining slots is generous without runaway.
        let probe_limit = remaining.saturating_mul(4).min(400);
        let rows = stmt
            .query_map(params![pattern, probe_limit], |row| {
                let id: String = row.get(0)?;
                let content: String = row.get(8)?;
                let snippet = {
                    let chars: Vec<char> = content.chars().collect();
                    if chars.len() > 120 { format!("{}…", chars.into_iter().take(120).collect::<String>()) } else { content }
                };
                Ok((id, ChatThread {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    project_id: row.get(2)?,
                    agent_id: row.get(3)?,
                    created_at: row.get(4)?,
                    last_message_at: row.get(5)?,
                    message_count: row.get(6)?,
                    archived: row.get::<_, i64>(7)? != 0,
                }, snippet))
            })
            .map_err(|e| e.to_string())?;
        for r in rows {
            let (id, thread, snippet) = r.map_err(|e| e.to_string())?;
            if hits.len() as i64 >= cap { break; }
            if seen.insert(id) {
                hits.push(ChatThreadSearchHit { thread, match_kind: "content".into(), snippet: Some(snippet) });
            }
        }
    }

    Ok(hits)
}

#[tauri::command]
pub fn get_chat_messages(
    db: State<'_, DbState>,
    thread_id: String,
) -> Result<Vec<ChatMessage>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, thread_id, role, content, runtime, agent_slug, metadata, created_at
             FROM chat_messages
             WHERE thread_id = ?1
             ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([&thread_id], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                runtime: row.get(4)?,
                agent_slug: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn append_chat_message(
    db: State<'_, DbState>,
    thread_id: String,
    role: String,
    content: String,
    runtime: Option<String>,
    agent_slug: Option<String>,
    metadata: Option<String>,
) -> Result<ChatMessage, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO chat_messages (id, thread_id, role, content, runtime, agent_slug, metadata, created_at, initiator_kind, client_surface, initiator_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'human', 'desktop', NULL)",
        params![id, thread_id, role, content, runtime, agent_slug, metadata, now],
    )
    .map_err(|e| e.to_string())?;
    // Update thread aggregate fields.
    conn.execute(
        "UPDATE chat_threads
            SET last_message_at = ?1,
                message_count = message_count + 1
          WHERE id = ?2",
        params![now, thread_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(ChatMessage {
        id,
        thread_id,
        role,
        content,
        runtime,
        agent_slug,
        metadata,
        created_at: now,
    })
}

#[tauri::command]
pub fn delete_chat_message(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let thread_id: Option<String> = conn
        .query_row(
            "SELECT thread_id FROM chat_messages WHERE id = ?1",
            [&id],
            |r| r.get::<_, String>(0),
        )
        .ok();
    conn.execute("DELETE FROM chat_messages WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    if let Some(tid) = thread_id {
        // Recompute message_count rather than risk drift.
        conn.execute(
            "UPDATE chat_threads
                SET message_count = (SELECT COUNT(*) FROM chat_messages WHERE thread_id = ?1)
              WHERE id = ?1",
            [&tid],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
