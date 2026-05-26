// Codex CLI session parser.
//
// File layout: ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl. First
// line is `session_meta` (carries session id + model_provider + cwd).
// Subsequent lines are `response_item` (messages) and `event_msg`
// (state updates incl. token_count). We pair user `input_text`
// messages with the following assistant `output_text` message.

use std::path::Path;

use serde_json::Value;

use crate::persist::update_tokens_for_latest;
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
    let timestamp = line
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let Some(payload) = line.get("payload") else { return };

    match ty {
        "session_meta" => handle_session_meta(payload, state),
        "event_msg" => handle_event_msg(db_path, payload, state),
        "response_item" => handle_response_item(
            db_path,
            payload,
            timestamp.as_deref(),
            state,
            last_seq,
            file_is_fresh,
        ),
        _ => {}
    }
}

fn handle_session_meta(payload: &Value, state: &mut SessionStateMap) {
    let sid = match payload.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return,
    };
    let cwd = payload
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let pair = state.get_or_init(&sid);
    if pair.cwd.is_none() {
        pair.cwd = cwd;
    }
}

fn handle_event_msg(db_path: &Path, payload: &Value, state: &mut SessionStateMap) {
    let inner = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if inner != "token_count" {
        return;
    }
    let usage = payload.get("info").and_then(|v| v.get("last_token_usage"));
    let Some(u) = usage else { return };
    let t_in = u.get("input_tokens").and_then(|v| v.as_i64());
    let t_out = u.get("output_tokens").and_then(|v| v.as_i64());

    // Codex's event order is `task_started → message → token_count`,
    // so by the time we see this event the assistant row for the
    // most recent turn has already been INSERTed with NULL tokens.
    // Earlier drafts latched the counts to attach to the *next*
    // assistant row — that attributed them to the wrong turn (review
    // MEDIUM-5). Walk back and UPDATE the row this event applies to.
    //
    // Rollout files are 1-session-per-file so the single key in the
    // map is the right session to attribute against.
    let Some(sid) = state.sessions.keys().next().cloned() else { return };
    update_tokens_for_latest(db_path, SourceKind::Codex, &sid, t_in, t_out);
}

fn handle_response_item(
    db_path: &Path,
    payload: &Value,
    timestamp: Option<&str>,
    state: &mut SessionStateMap,
    last_seq: &mut i64,
    file_is_fresh: bool,
) {
    let inner = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if inner != "message" {
        return;
    }
    let role = payload.get("role").and_then(|v| v.as_str()).unwrap_or("");
    let model = payload
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let Some(sid) = state.sessions.keys().next().cloned() else {
        // session_meta hadn't landed yet — skip until we see it.
        return;
    };

    let content_items = payload
        .get("content")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let pick_text = |types: &[&str]| -> String {
        content_items
            .iter()
            .filter_map(|it| {
                let t = it.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if types.contains(&t) {
                    it.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    match role {
        "user" => {
            let text = pick_text(&["input_text"]);
            if text.is_empty() {
                return;
            }
            // Filter out codex's prepended AGENTS.md / permissions
            // injection — not the user's typed prompt.
            if text.starts_with("<permissions instructions>")
                || text.starts_with("# AGENTS.md")
                || text.starts_with("You are continuing an ongoing conversation")
            {
                return;
            }
            let cwd_now = state.sessions.get(&sid).and_then(|s| s.cwd.clone());
            let pair = state.get_or_init(&sid);
            pair.user_text = Some(text);
            pair.user_started_at = timestamp.map(|s| s.to_string());
            if file_is_fresh {
                mark_in_progress(
                    db_path,
                    SourceKind::Codex,
                    &sid,
                    timestamp,
                    cwd_now.as_deref(),
                );
            }
        }
        "assistant" => {
            let response = pick_text(&["output_text"]);
            if response.is_empty() {
                return;
            }
            let pair = state.get_or_init(&sid);
            if let Some(m) = model.as_ref() {
                pair.last_model = Some(m.clone());
            }
            let prompt = match pair.user_text.take() {
                Some(t) => t,
                None => return,
            };
            let started_at = pair
                .user_started_at
                .take()
                .or_else(|| timestamp.map(|s| s.to_string()));
            // Tokens land via update_tokens_for_latest when the
            // subsequent `token_count` event_msg arrives — emitting
            // NULL here is the honest signal until then.
            let model_str = pair.last_model.clone();
            let seq = *last_seq + 1;
            emit(
                db_path,
                SourceKind::Codex,
                &sid,
                seq,
                &prompt,
                &response,
                None,
                None,
                model_str.as_deref(),
                started_at.as_deref(),
            );
            *last_seq = seq;
        }
        _ => {}
    }
}
