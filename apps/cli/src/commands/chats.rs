// v2.7.13 — chats become first-class closeable conversations.
//
// Pre-existing shape: chat_threads (one row per thread) + chat_messages
// (one row per turn). The `archived` flag predates this module and
// covers UI hide/show; it's distinct from the coordinator-driven
// close lifecycle being added here.
//
// What this module adds:
//   - `ChatThread` struct + Closeable impl that reads messages from
//     `chat_messages` and persists close fields onto the augmented
//     `chat_threads` row (see schema.rs for the new columns).
//   - `lookup` / `close` / `reopen` / `get` thin functions the CLI
//     calls.
//
// Shared orchestration (prompt + summarizer + parse + validate) lives
// in `conversation_close`; this module only owns the per-table I/O.

use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::Serialize;

use crate::commands::conversation_close::{
    close_conversation, reopen_conversation, CloseFields, Closeable, ConversationTurn,
};
use crate::output::Opts;

/// In-memory snapshot of a chat thread's metadata + lifecycle state.
#[derive(Debug, Clone, Serialize)]
pub struct ChatThread {
    pub id: String,
    pub title: String,
    pub agent_id: Option<String>,
    pub status: String,
    pub closed_at: Option<String>,
    pub auto_title: Option<String>,
    pub summary: Option<String>,
    pub coordinator_runtime: Option<String>,
    pub message_count: i64,
    /// The agent_slug derived from agent_id (when set + resolvable).
    /// Fed to the summarizer resolution chain as the "stored agent"
    /// candidate so a chat anchored to e.g. `@reviewer` defaults to
    /// using the reviewer agent's runtime if no override is passed.
    pub agent_slug: Option<String>,
}

impl Closeable for ChatThread {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_label(&self) -> &'static str {
        "chat thread"
    }
    fn status(&self) -> &str {
        &self.status
    }
    fn stored_agent_slug(&self) -> Option<&str> {
        self.agent_slug.as_deref()
    }
    fn anchor_runtime(&self) -> Option<&str> {
        // Each chat message records its own runtime — there's no
        // single anchor. Fall through to the registry default for the
        // summarizer (matches the war-room policy).
        None
    }
    fn existing_title(&self) -> Option<&str> {
        // Prefer the human-typed title over an earlier auto_title so
        // the coordinator sees what the user named it.
        if !self.title.trim().is_empty() {
            Some(self.title.as_str())
        } else {
            self.auto_title.as_deref()
        }
    }
    fn fetch_turns(&self, conn: &Connection) -> Result<Vec<ConversationTurn>> {
        let mut stmt = conn.prepare(
            "SELECT role, content, runtime
               FROM chat_messages
              WHERE thread_id = ?1
              ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([&self.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut turns = Vec::new();
        for row in rows {
            let (role, content, runtime) = row?;
            // Skip the synthetic 'system' / 'attachment' / 'error'
            // rows — they're metadata, not part of the conversation
            // the coordinator needs to summarize. 'user' and
            // 'assistant' are the only first-class roles.
            if role != "user" && role != "assistant" {
                continue;
            }
            if content.trim().is_empty() {
                continue;
            }
            turns.push(ConversationTurn {
                role,
                text: content,
                runtime: runtime.unwrap_or_else(|| "(unknown)".to_string()),
            });
        }
        Ok(turns)
    }
    fn persist_close(&self, conn: &Connection, fields: &CloseFields) -> Result<usize> {
        let tags_json = serde_json::to_string(&fields.tags).unwrap_or_else(|_| "[]".to_string());
        let changed = conn.execute(
            "UPDATE chat_threads
                SET status = 'closed',
                    closed_at = ?1,
                    auto_title = ?2,
                    summary = ?3,
                    tags_json = ?4,
                    category = COALESCE(?5, category),
                    team = COALESCE(?6, team),
                    coordinator_runtime = ?7,
                    coordinator_model = ?8,
                    human_comment = COALESCE(?9, human_comment)
              WHERE id = ?10 AND status = 'open'",
            rusqlite::params![
                fields.closed_at,
                fields.auto_title,
                fields.summary,
                tags_json,
                fields.category,
                fields.team,
                fields.coordinator_runtime,
                fields.coordinator_model,
                fields.human_comment,
                self.id,
            ],
        )?;
        Ok(changed)
    }
    fn persist_reopen(&self, conn: &Connection) -> Result<usize> {
        let changed = conn.execute(
            "UPDATE chat_threads
                SET status = 'open',
                    closed_at = NULL
              WHERE id = ?1 AND status = 'closed'",
            [&self.id],
        )?;
        Ok(changed)
    }
}

/// Load a chat thread snapshot by id. agent_slug is resolved
/// opportunistically — a thread anchored to an agent whose row no
/// longer exists falls through to None (the summarizer chain handles
/// the missing case gracefully).
pub fn lookup(conn: &Connection, id: &str) -> Result<ChatThread> {
    let (title, agent_id, status, closed_at, auto_title, summary, coordinator_runtime, message_count): (
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
    ) = conn
        .query_row(
            "SELECT title, agent_id, COALESCE(status, 'open'), closed_at,
                    auto_title, summary, coordinator_runtime, message_count
               FROM chat_threads WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .map_err(|e| anyhow!("chat thread '{}' not found: {}", id, e))?;
    let agent_slug = match agent_id.as_deref() {
        Some(aid) => conn
            .query_row(
                "SELECT slug FROM agents WHERE id = ?1",
                [aid],
                |r| r.get::<_, String>(0),
            )
            .ok(),
        None => None,
    };
    Ok(ChatThread {
        id: id.to_string(),
        title,
        agent_id,
        status,
        closed_at,
        auto_title,
        summary,
        coordinator_runtime,
        message_count,
        agent_slug,
    })
}

/// Close a chat thread with the shared coordinator orchestration.
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
    let target = lookup(conn, id)?;
    let fields = close_conversation(
        conn,
        &target,
        agent_slug_override.as_deref(),
        model_override.as_deref(),
        coordinator_override.as_deref(),
        human_comment.as_deref(),
        force_close_without_context,
        opts,
    )?;
    if opts.human {
        emit_human_close(&target, &fields);
    } else {
        emit_json_close(&target, &fields)?;
    }
    Ok(())
}

/// Reopen a closed chat thread.
pub fn reopen(conn: &Connection, id: &str, opts: &Opts) -> Result<()> {
    let target = lookup(conn, id)?;
    reopen_conversation(conn, &target)?;
    if opts.human {
        println!("Reopened chat thread {}.", id);
    } else {
        println!(
            "{}",
            serde_json::json!({
                "id": id,
                "action": "reopened",
                "status": "open",
            })
        );
    }
    let _ = opts;
    Ok(())
}

/// Print the chat thread snapshot (for `ato chats get <id>`).
pub fn get(conn: &Connection, id: &str, opts: &Opts) -> Result<()> {
    let target = lookup(conn, id)?;
    if opts.human {
        println!(
            "Chat thread {}\n  status: {}\n  messages: {}\n  title: {}\n  summary: {}",
            target.id,
            target.status,
            target.message_count,
            target.auto_title.as_deref().unwrap_or(&target.title),
            target.summary.as_deref().unwrap_or("(none)"),
        );
    } else {
        println!("{}", serde_json::to_string(&target)?);
    }
    Ok(())
}

fn emit_human_close(target: &ChatThread, fields: &CloseFields) {
    println!(
        "Closed chat thread {} ({} messages).\n  title: {}\n  summary: {}\n  tags: {}\n  coordinator: {}{}",
        target.id,
        target.message_count,
        fields.auto_title.as_deref().unwrap_or(&target.title),
        fields.summary.as_deref().unwrap_or("(none)"),
        if fields.tags.is_empty() {
            "(none)".to_string()
        } else {
            fields.tags.join(", ")
        },
        fields.coordinator_runtime,
        fields
            .human_comment
            .as_deref()
            .map(|c| format!("\n  human note: {}", c))
            .unwrap_or_default(),
    );
}

fn emit_json_close(target: &ChatThread, fields: &CloseFields) -> Result<()> {
    let payload = serde_json::json!({
        "id": target.id,
        "kind": "chat",
        "status": "closed",
        "message_count": target.message_count,
        "auto_title": fields.auto_title,
        "summary": fields.summary,
        "tags": fields.tags,
        "category": fields.category,
        "team": fields.team,
        "project_id": fields.project_id,
        "coordinator_runtime": fields.coordinator_runtime,
        "coordinator_model": fields.coordinator_model,
        "human_comment": fields.human_comment,
        "duration_ms": fields.duration_ms,
        "closed_at": fields.closed_at,
    });
    println!("{}", serde_json::to_string(&payload)?);
    Ok(())
}
