// v2.9.0 PR-2 — parse claude CLI's --output-format stream-json into
// (final_response_text, tool_call_observations). This is the
// observation channel that PR-1's verdict computation needs for
// claude dispatches; without it, the verdict false-negatives on
// claude because tool_calls_summary stays empty (see PR-1 part-1
// score sheet for the regression this closes).
//
// Stream-json shape (confirmed by probing `claude --print --verbose
// --output-format stream-json "Read ./README.md"`, captured at
// /tmp/claude-stream.txt 2026-05-24):
//
//   Line N events of one-event-per-line NDJSON. Event types include:
//     - "system"       — boot info, ignored for grounding purposes
//     - "assistant"    — claude's response chunk; .message.content[]
//                        carries the structured per-turn content
//     - "user"         — tool results coming back to claude; ignored
//                        here because we count the tool_use, not the
//                        echo
//     - "result"       — the FINAL event with .result = the response
//                        text we'd persist to execution_logs.response
//     - others         — thinking, rate_limit_event, etc; ignored
//
//   Inside each "assistant" event's message.content[] array:
//     - { type: "thinking", thinking: "..." }      — ignored
//     - { type: "text", text: "..." }              — used for assembling
//                                                    fallback response
//                                                    when result event
//                                                    is missing
//     - { type: "tool_use", id, name, input: {} }  — THIS is the tool
//                                                    call observation
//
// The parser is a pure string-in / structured-out function so it's
// trivially unit-testable. It's deliberately permissive: malformed
// lines are skipped (logged at parse time if we wanted, but for v2.9
// PR-2 they fail silently rather than poison the whole dispatch).

use crate::grounding::verdict::ToolCallObservation;

/// Output of parsing claude's stream-json. Both fields are owned so
/// callers can move them into execution_logs writes without lifetime
/// gymnastics.
#[derive(Debug, Clone, Default)]
pub struct ClaudeStreamParseOutput {
    /// The final assistant text. Pulled from the `result` event's
    /// `result` field when present; falls back to concatenating any
    /// `text` content blocks from `assistant` events if `result` is
    /// missing (e.g., the dispatch was cut off).
    pub response_text: String,
    /// Every tool_use block claude emitted in this dispatch. Order
    /// preserved. Used by the grounding verdict computation to count
    /// observed tool calls against the agent's mandatory rules.
    pub tool_calls: Vec<ToolCallObservation>,
}

/// Parse the stream-json output. Input is the full stdout of
/// `claude --print --verbose --output-format stream-json "<prompt>"`.
/// Returns the assistant's final text plus every tool_use observed.
///
/// Returns an empty default rather than an error when parsing fails
/// at the outer level — a malformed dispatch shouldn't corrupt the
/// receipt write path. The caller can detect "parser found nothing"
/// by checking the returned struct's emptiness.
pub fn parse_claude_stream_json(stream: &str) -> ClaudeStreamParseOutput {
    let mut out = ClaudeStreamParseOutput::default();
    let mut fallback_text_chunks: Vec<String> = Vec::new();

    for line in stream.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // skip non-JSON lines (warnings on stdout)
        };

        let event_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match event_type {
            // The final event — has the canonical response text.
            "result" => {
                if let Some(s) = value.get("result").and_then(|r| r.as_str()) {
                    out.response_text = s.to_string();
                }
            }

            // Per-turn content from claude. Each may carry tool_use
            // blocks OR text fragments OR thinking. We walk the
            // content array and pick what we care about.
            "assistant" => {
                let content = value
                    .pointer("/message/content")
                    .and_then(|c| c.as_array());
                let Some(content) = content else { continue };

                for block in content {
                    let block_type = block
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    match block_type {
                        "tool_use" => {
                            let name = block
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string();
                            if name.is_empty() {
                                continue;
                            }
                            // Build a brief args summary — first 120 chars
                            // of the JSON-serialized input. Mirrors the
                            // existing v2.4.5 ToolCallAudit.args_brief
                            // shape so the receipt UI doesn't need a
                            // new render path.
                            let args_brief = block
                                .get("input")
                                .map(|i| {
                                    let s = serde_json::to_string(i).unwrap_or_default();
                                    if s.len() > 120 {
                                        format!("{}…", &s[..120])
                                    } else {
                                        s
                                    }
                                });
                            out.tool_calls.push(ToolCallObservation {
                                name,
                                args_brief,
                                is_error: false,
                            });
                        }
                        "text" => {
                            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                fallback_text_chunks.push(t.to_string());
                            }
                        }
                        // thinking, image, etc — ignored
                        _ => {}
                    }
                }
            }

            // Everything else (system, user, thinking, rate_limit_event,
            // direct) is irrelevant to grounding.
            _ => {}
        }
    }

    // Fallback: if the result event was missing (interrupted dispatch),
    // assemble the response from text blocks we accumulated.
    if out.response_text.is_empty() && !fallback_text_chunks.is_empty() {
        out.response_text = fallback_text_chunks.join("");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_result_event_into_response_text() {
        let stream = r#"
{"type":"system","subtype":"init"}
{"type":"result","result":"hello world","subtype":"success","duration_ms":1234}
"#;
        let out = parse_claude_stream_json(stream);
        assert_eq!(out.response_text, "hello world");
        assert!(out.tool_calls.is_empty());
    }

    #[test]
    fn parses_assistant_tool_use_blocks_in_order() {
        let stream = r#"
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/x/y/z"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"file contents"}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"Grep","input":{"pattern":"foo"}}]}}
{"type":"result","result":"final answer"}
"#;
        let out = parse_claude_stream_json(stream);
        assert_eq!(out.response_text, "final answer");
        assert_eq!(out.tool_calls.len(), 2);
        assert_eq!(out.tool_calls[0].name, "Read");
        assert_eq!(out.tool_calls[1].name, "Grep");
        // args_brief should be the JSON-serialized input
        let read_args = out.tool_calls[0].args_brief.as_ref().unwrap();
        assert!(read_args.contains("file_path"));
        assert!(read_args.contains("/x/y/z"));
    }

    #[test]
    fn skips_malformed_lines_without_failing_the_dispatch() {
        let stream = r#"
NOT JSON AT ALL
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]}}
{this is also garbage
{"type":"result","result":"ok"}
"#;
        let out = parse_claude_stream_json(stream);
        assert_eq!(out.response_text, "ok");
        assert_eq!(out.tool_calls.len(), 1);
    }

    #[test]
    fn assistant_text_blocks_used_as_fallback_when_result_missing() {
        // Simulates a dispatch cut off before the result event.
        let stream = r#"
{"type":"assistant","message":{"content":[{"type":"text","text":"partial answer here"}]}}
{"type":"assistant","message":{"content":[{"type":"text","text":" — more text"}]}}
"#;
        let out = parse_claude_stream_json(stream);
        assert_eq!(out.response_text, "partial answer here — more text");
        assert!(out.tool_calls.is_empty());
    }

    #[test]
    fn empty_input_returns_default_struct() {
        let out = parse_claude_stream_json("");
        assert!(out.response_text.is_empty());
        assert!(out.tool_calls.is_empty());
    }

    #[test]
    fn args_brief_is_truncated_for_huge_inputs() {
        let huge = "x".repeat(500);
        let stream = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"t1","name":"Read","input":{{"x":"{}"}}}}]}}}}
{{"type":"result","result":"ok"}}"#,
            huge
        );
        let out = parse_claude_stream_json(&stream);
        assert_eq!(out.tool_calls.len(), 1);
        let brief = out.tool_calls[0].args_brief.as_ref().unwrap();
        // 120 chars + the ellipsis marker
        assert!(
            brief.len() <= 124,
            "args_brief should be capped at ~120 chars, was {}",
            brief.len()
        );
    }

    #[test]
    fn thinking_and_system_events_are_ignored() {
        let stream = r#"
{"type":"system","subtype":"init","model":"claude"}
{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"long thought..."}]}}
{"type":"result","result":"done"}
"#;
        let out = parse_claude_stream_json(stream);
        assert_eq!(out.response_text, "done");
        assert!(out.tool_calls.is_empty());
    }

    #[test]
    fn tool_use_without_name_is_skipped() {
        let stream = r#"
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","input":{}}]}}
{"type":"result","result":"ok"}
"#;
        let out = parse_claude_stream_json(stream);
        assert!(out.tool_calls.is_empty()); // no name means malformed → skip
    }
}
