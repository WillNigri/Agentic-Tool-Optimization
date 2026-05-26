// Claude Code session JSONL parser.
//
// File layout: ~/.claude/projects/<workspace-slug>/<session-uuid>.jsonl.
// Each line is one event; relevant types are `user` and `assistant`.
// `sessionId` is stable per file. `isMeta: true` marks CLI-injected
// reminders (slash commands, local-command-caveat blocks) — we skip
// those because they're not the user's typed prompt.

use std::path::Path;

use serde_json::Value;

use crate::sources::SourceKind;
use crate::worker::{emit, mark_in_progress, SessionStateMap};

pub fn process(
    db_path: &Path,
    line: &Value,
    state: &mut SessionStateMap,
    last_seq: &mut i64,
    file_is_fresh: bool,
) {
    let ty = line.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let session_id = match line.get("sessionId").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return,
    };
    let cwd = line.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let timestamp = line
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match ty {
        "user" => handle_user(
            db_path,
            session_id,
            line,
            cwd.as_deref(),
            timestamp.as_deref(),
            state,
            file_is_fresh,
        ),
        "assistant" => handle_assistant(
            db_path,
            session_id,
            line,
            timestamp.as_deref(),
            state,
            last_seq,
        ),
        _ => {}
    }
}

fn handle_user(
    db_path: &Path,
    session_id: &str,
    line: &Value,
    cwd: Option<&str>,
    timestamp: Option<&str>,
    state: &mut SessionStateMap,
    file_is_fresh: bool,
) {
    // Skip CLI-injected reminders.
    if line.get("isMeta").and_then(|v| v.as_bool()).unwrap_or(false) {
        return;
    }
    let Some(message) = line.get("message") else { return };
    let role = message.get("role").and_then(|v| v.as_str()).unwrap_or("");
    if role != "user" {
        return;
    }
    let text = extract_user_text(message);
    let Some(text) = text else { return };

    let pair = state.get_or_init(session_id);
    pair.user_text = Some(text);
    pair.user_started_at = timestamp.map(|s| s.to_string());
    if cwd.is_some() {
        pair.cwd = cwd.map(|s| s.to_string());
    }
    if file_is_fresh {
        mark_in_progress(db_path, SourceKind::ClaudeCode, session_id, timestamp, cwd);
    }
}

/// `message.content` is either a plain string (typed prompt) or an
/// array (tool result reply, image, etc). For execution_logs we only
/// care about the typed-prompt case.
fn extract_user_text(message: &Value) -> Option<String> {
    match message.get("content") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(parts)) => {
            // If any element is a tool_result, this turn is a tool
            // reply, not a user prompt — skip.
            let any_tool_result = parts
                .iter()
                .any(|p| p.get("type").and_then(|v| v.as_str()) == Some("tool_result"));
            if any_tool_result {
                return None;
            }
            let collected: Vec<&str> = parts
                .iter()
                .filter_map(|p| {
                    if p.get("type").and_then(|v| v.as_str()) == Some("text") {
                        p.get("text").and_then(|v| v.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if collected.is_empty() {
                None
            } else {
                Some(collected.join("\n"))
            }
        }
        _ => None,
    }
}

fn handle_assistant(
    db_path: &Path,
    session_id: &str,
    line: &Value,
    timestamp: Option<&str>,
    state: &mut SessionStateMap,
    last_seq: &mut i64,
) {
    let Some(message) = line.get("message") else { return };
    let content = message.get("content");
    let text_parts: Vec<String> = match content {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| {
                if p.get("type").and_then(|v| v.as_str()) == Some("text") {
                    p.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    if text_parts.is_empty() {
        // Pure tool_use turns — no human-visible text — aren't an
        // "answer" to attribute to a prompt.
        return;
    }
    let response = text_parts.join("\n");
    let model = message
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let tokens_in = message
        .get("usage")
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_i64());
    let tokens_out = message
        .get("usage")
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_i64());

    let pair = state.get_or_init(session_id);
    if let Some(m) = &model {
        pair.last_model = Some(m.clone());
    }
    let prompt = match pair.user_text.take() {
        Some(t) => t,
        None => {
            // Assistant message with no preceding user prompt — most
            // often a continuation after a tool turn. Skip.
            return;
        }
    };
    let started_at = pair.user_started_at.take().or_else(|| timestamp.map(|s| s.to_string()));

    let seq = *last_seq + 1;
    emit(
        db_path,
        SourceKind::ClaudeCode,
        session_id,
        seq,
        &prompt,
        &response,
        tokens_in,
        tokens_out,
        model.as_deref().or(pair.last_model.as_deref()),
        started_at.as_deref(),
    );
    *last_seq = seq;
}
