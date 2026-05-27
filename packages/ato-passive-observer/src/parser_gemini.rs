// Gemini CLI session parser.
//
// Observed file layouts (verified 2026-05-26 against `gemini --help`
// + the published session-log format docs):
//   1. `~/.gemini/tmp/<session-id>/logs.json` — JSON array of event
//      objects (one big top-level [ ... ]). Older Code Assist /
//      legacy gemini CLI layout.
//   2. `~/.gemini/sessions/<session-id>/log.jsonl` — newline-
//      delimited JSON, one event per line. Current gemini CLI builds.
//   3. `~/.gemini/history.jsonl` — flat user-prompt history without
//      assistant responses (skip — not enough to form a pair).
//
// We dispatch on event shape rather than file name. The worker's
// `line_iter` slices the file into newline-delimited chunks; for
// layout (1) the entire array lands as one "line" because there's no
// embedded `\n` between objects in the typical formatter — we detect
// it and unfurl. Most installs use (2) anyway.
//
// Event schema (both layouts share the same per-event fields):
//   {
//     "sessionId": "<uuid>",
//     "messageId": "<uuid>",
//     "type": "user" | "model" | "tool_call" | "tool_response",
//     "message": "...text..."          (older format)
//       OR
//     "parts": [{"text": "..."}, ...]  (newer format)
//     "model": "gemini-2.5-pro",
//     "timestamp": "ISO-8601",
//     "usage": { "promptTokenCount": N, "candidatesTokenCount": N }
//   }

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
    // Layout (1) — entire file parsed as one JSON array because the
    // file lacks newlines. Walk the array; each element is an event.
    if let Value::Array(events) = line {
        for ev in events {
            process_event(db_path, ev, state, last_seq, file_is_fresh);
        }
        return;
    }
    process_event(db_path, line, state, last_seq, file_is_fresh);
}

fn process_event(
    db_path: &Path,
    ev: &Value,
    state: &mut SessionStateMap,
    last_seq: &mut i64,
    file_is_fresh: bool,
) {
    let session_id = match ev.get("sessionId").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return,
    };
    let ty = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let timestamp = ev
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let cwd = ev.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());

    let text = extract_text(ev);

    match ty {
        "user" => {
            let Some(text) = text else { return };
            let pair = state.get_or_init(&session_id);
            pair.user_text = Some(text);
            pair.user_started_at = timestamp.clone();
            if cwd.is_some() {
                pair.cwd = cwd.clone();
            }
            if file_is_fresh {
                mark_in_progress(
                    db_path,
                    SourceKind::Gemini,
                    &session_id,
                    timestamp.as_deref(),
                    cwd.as_deref(),
                );
            }
        }
        // `model` is gemini-cli's term for the assistant turn. Some
        // builds emit `assistant` — accept both for forward compat.
        "model" | "assistant" => {
            let Some(response) = text else { return };
            let model = ev
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let tokens_in = ev
                .get("usage")
                .and_then(|u| u.get("promptTokenCount").or_else(|| u.get("input_tokens")))
                .and_then(|v| v.as_i64());
            let tokens_out = ev
                .get("usage")
                .and_then(|u| {
                    u.get("candidatesTokenCount")
                        .or_else(|| u.get("output_tokens"))
                })
                .and_then(|v| v.as_i64());

            let pair = state.get_or_init(&session_id);
            if let Some(m) = &model {
                pair.last_model = Some(m.clone());
            }
            let prompt = match pair.user_text.take() {
                Some(t) => t,
                None => return,
            };
            let started_at = pair.user_started_at.take().or(timestamp.clone());
            let seq = *last_seq + 1;
            emit(
                db_path,
                SourceKind::Gemini,
                &session_id,
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
        // tool_call / tool_response / anything else: ignore. They're
        // sub-turns that don't map to a billable LLM round-trip.
        _ => {}
    }
}

/// Two shapes in the wild:
///   - `message`: a plain string.
///   - `parts`: an array of `{type, text}` objects (newer SDK).
/// Concat all `text` parts; return None if neither shape yields anything.
fn extract_text(ev: &Value) -> Option<String> {
    if let Some(s) = ev.get("message").and_then(|v| v.as_str()) {
        if s.is_empty() {
            return None;
        }
        return Some(s.to_string());
    }
    if let Some(parts) = ev.get("parts").and_then(|v| v.as_array()) {
        let collected: Vec<&str> = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
            .collect();
        if collected.is_empty() {
            return None;
        }
        return Some(collected.join("\n"));
    }
    // Newer SDK variant: top-level `text` field.
    if let Some(s) = ev.get("text").and_then(|v| v.as_str()) {
        if s.is_empty() {
            return None;
        }
        return Some(s.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_text_handles_message_string() {
        let ev = json!({ "message": "hello" });
        assert_eq!(extract_text(&ev), Some("hello".to_string()));
    }

    #[test]
    fn extract_text_handles_parts_array() {
        let ev = json!({
            "parts": [
                { "text": "alpha" },
                { "text": "beta" }
            ]
        });
        assert_eq!(extract_text(&ev), Some("alpha\nbeta".to_string()));
    }

    #[test]
    fn extract_text_returns_none_when_empty() {
        let ev = json!({ "message": "" });
        assert_eq!(extract_text(&ev), None);
        let ev = json!({});
        assert_eq!(extract_text(&ev), None);
    }
}
