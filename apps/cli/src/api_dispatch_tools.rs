// v2.4.5 Tier 2 — function-calling dispatch loop for reviewers.
//
// `dispatch_with_tools` extends `dispatch_with_history` to support
// providers' tool-calling protocols. The reviewer LLM can emit
// `tool_calls` for `read_file` / `grep` / `git_log` (per
// review_tools::registry()); we execute, append results to the
// message history, call again, loop until the model produces a
// final text response with no further tool calls OR we hit
// MAX_TOOL_ROUNDS.
//
// Per-flavor differences are isolated to:
//   - tools_field()      — request-body marshalling of the registry
//   - parse_tool_calls() — extracting tool_calls from the response
//   - append_tool_results() — appending tool results to messages
//   - extract_final_text() — pulling the model's final reply
//
// Supported flavors today:
//   - openai     (grok, deepseek, qwen, openrouter, minimax)
//   - gemini     (google)
//   - anthropic  (added v2.7.8 PR-3 — the audit's primary target)
//
// MiniMax tool-calling has known reliability issues on the
// subscription tier; the caller (`ato review`) decides whether to
// route a given provider through the tool-using path or fall back
// to plain dispatch_with_history.
//
// v2.7.8 PR-3 — `dispatch_with_tools` now takes the tools list as a
// parameter rather than hardcoding `review_tools::registry()`. This
// lets `ato dispatch` (war-room flows) pass a permission-gated subset
// while `ato review` keeps passing the full registry.

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use rusqlite::Connection;
use std::time::Duration;

use crate::api_dispatch::{resolve_api_key, ApiDispatchOutcome, ApiProvider, Message, ToolCallAudit};
use crate::review_tools::{self, ToolCall, ToolDef, ToolResult, MAX_TOOL_ROUNDS};

/// Run a dispatch with a permission-aware tool registry available to
/// the model. Iterates: dispatch → execute tool calls → dispatch
/// again → … until the model produces a final text response or we
/// hit the round cap.
///
/// `tools` is caller-provided so each call site can pass either the
/// full review registry (Tier 2 reviews) or an agent-permission-
/// filtered subset (war-room dispatches).
pub fn dispatch_with_tools(
    provider: &ApiProvider,
    history: &[Message],
    prompt: &str,
    model_override: Option<&str>,
    tools: &[ToolDef],
    conn: &Connection,
) -> Result<ApiDispatchOutcome> {
    if !provider_supports_tools(provider) {
        anyhow::bail!(
            "provider '{}' does not have a tools-flavor mapping; fall back to dispatch_with_history",
            provider.slug
        );
    }
    let key = resolve_api_key(provider, conn)?;
    let model = resolve_model(provider, model_override)?;

    let client = reqwest::blocking::Client::builder()
        // Tool loops can run several seconds per turn for 4–8 turns;
        // a 10 min wall budget keeps a hung provider from blocking
        // ato review indefinitely without giving up on legitimate
        // multi-tool reviews.
        .timeout(Duration::from_secs(600))
        .build()
        .context("build reqwest client")?;

    // The conversation state we mutate across rounds. Shape is
    // flavor-specific (openai uses `messages[]`, gemini uses
    // `contents[]`). We keep both representations live in one struct
    // to avoid double-conversion.
    let mut conv = Conversation::new(provider, history, prompt);

    let start = std::time::Instant::now();
    let mut rounds = 0usize;
    let mut accumulated_text = String::new();
    let mut tokens_in_total: Option<i64> = None;
    let mut tokens_out_total: Option<i64> = None;
    // Per-dispatch audit so the GUI can show "verified via N tool
    // calls" instead of guessing from response text. Always populated
    // (empty vec is itself signal: "tools were offered, model declined").
    let mut audit: Vec<ToolCallAudit> = Vec::new();
    // v2.4.6 — Gemini specifically can return finishReason=STOP with
    // no tool calls AND no text parts when its private thinking
    // tokens consume all the generation budget on a tool-use round.
    // We retry ONCE with an explicit "please write your final reply
    // now" nudge before giving up; more attempts would just burn the
    // budget on more thinking with no return.
    let mut empty_response_retries = 0usize;
    const MAX_EMPTY_RETRIES: usize = 1;

    eprintln!(
        "  [tools] dispatch_with_tools provider={} flavor={} model={}",
        provider.slug, provider.flavor, model
    );
    loop {
        if rounds >= MAX_TOOL_ROUNDS {
            // Tell the model "one more turn, no tools, just finalize."
            // Surface the cap in the audit so the caller knows.
            conv.append_user_text(
                provider,
                "Maximum tool rounds reached. Write your final review now using what you've learned. No more tool calls.",
            );
        }
        eprintln!("  [tools] round {} begins", rounds);

        let body = conv.build_request_body(provider, tools, &model, rounds >= MAX_TOOL_ROUNDS);
        let url = conv.build_url(provider, &model, &key);
        let mut req = client.post(&url).header("Content-Type", "application/json");
        // Auth header per flavor:
        //   - openai / minimax: Bearer <key>
        //   - gemini:           API key in URL ?key= (no header)
        //   - anthropic:        x-api-key + anthropic-version
        match provider.flavor {
            "anthropic" => {
                req = req
                    .header("x-api-key", &key)
                    .header("anthropic-version", "2023-06-01");
            }
            "gemini" => { /* key is in URL ?key= */ }
            _ => {
                req = req.bearer_auth(&key);
            }
        }
        let resp = req.json(&body).send().with_context(|| format!("POST {}", url))?;
        let http_status = resp.status();
        let body_text = resp.text().context("read response body")?;
        if !http_status.is_success() {
            return Ok(ApiDispatchOutcome {
                response: None,
                error_message: Some(format!(
                    "HTTP {}: {}",
                    http_status.as_u16(),
                    truncate(&body_text, 1000)
                )),
                model_used: model,
                duration_ms: start.elapsed().as_millis() as i64,
                tokens_in: tokens_in_total,
                tokens_out: tokens_out_total,
                tool_calls: Some(audit),
            });
        }
        let payload: serde_json::Value =
            serde_json::from_str(&body_text).context("response was not valid JSON")?;

        // Pull usage tokens if the provider exposes them — we
        // accumulate across rounds so the final outcome's
        // tokens_in/out reflect the whole loop.
        let (ti, to) = conv.extract_usage(provider, &payload);
        if ti.is_some() {
            tokens_in_total = Some(tokens_in_total.unwrap_or(0) + ti.unwrap_or(0));
        }
        if to.is_some() {
            tokens_out_total = Some(tokens_out_total.unwrap_or(0) + to.unwrap_or(0));
        }

        // Did the model emit tool calls?
        let calls = conv.parse_tool_calls(provider, &payload, tools);
        let assistant_text = conv.extract_final_text(provider, &payload);
        if !calls.is_empty() && rounds < MAX_TOOL_ROUNDS {
            // Append the assistant's tool-call turn to history, then
            // execute each call and append the results.
            conv.append_assistant_tool_calls(provider, &payload, &calls);
            let results: Vec<ToolResult> = calls
                .iter()
                .map(|c| {
                    let r = review_tools::execute_call(c);
                    // S10 (v2.7.11) — shared log + audit-args helper. Was a
                    // 10-line block duplicated verbatim with desktop's async
                    // dispatch_with_tools.
                    let args_brief = review_tools::log_tool_call_and_brief_args(c, &r);
                    audit.push(ToolCallAudit {
                        name: c.name.clone(),
                        args_brief,
                        is_error: r.is_error,
                    });
                    r
                })
                .collect();
            conv.append_tool_results(provider, &results);
            rounds += 1;
            continue;
        }

        // No tool calls — this is the final reply. Accumulate any
        // assistant text emitted alongside earlier tool calls (rare)
        // and return.
        if let Some(t) = assistant_text {
            accumulated_text.push_str(&t);
        }
        let duration_ms = start.elapsed().as_millis() as i64;
        if accumulated_text.trim().is_empty() {
            // Gemini-on-thinking-budget escape hatch: one retry with
            // an explicit "write your reply now" nudge. If we've
            // already used the retry OR we've used no tools (so the
            // model never had a chance to reason productively), give
            // up and surface the diagnostic.
            if empty_response_retries < MAX_EMPTY_RETRIES && !audit.is_empty() {
                empty_response_retries += 1;
                eprintln!(
                    "  [tools] empty response after round {}; nudging model to write final reply (retry {}/{})",
                    rounds, empty_response_retries, MAX_EMPTY_RETRIES
                );
                conv.append_user_text(
                    provider,
                    "You returned no text. Please write your final review now in plain markdown, using the tool-call results from earlier rounds. Do not call any more tools.",
                );
                rounds += 1;
                continue;
            }
            return Ok(ApiDispatchOutcome {
                response: None,
                error_message: Some(format!(
                    "tool-loop ended without a final text reply after {} round(s). Last payload: {}",
                    rounds,
                    truncate(&payload.to_string(), 600)
                )),
                model_used: model,
                duration_ms,
                tokens_in: tokens_in_total,
                tokens_out: tokens_out_total,
                tool_calls: Some(audit),
            });
        }
        return Ok(ApiDispatchOutcome {
            response: Some(accumulated_text),
            error_message: None,
            model_used: model,
            duration_ms,
            tokens_in: tokens_in_total,
            tokens_out: tokens_out_total,
            tool_calls: Some(audit),
        });
    }
}

pub fn provider_supports_tools(p: &ApiProvider) -> bool {
    // MiniMax-M2+ exposes the same OpenAI-style `tools` field, `tool_choice`,
    // and `choices[0].message.tool_calls[]` response shape as the openai
    // flavor — the only structural difference between the two is MiniMax's
    // URL path (`/v1/text/chatcompletion_v2`) and its `base_resp.status_code`
    // success wrapper, neither of which the tool-call protocol cares about.
    // So we let minimax fall through the same openai branch in build_url /
    // build_request_body / parse_tool_calls / extract_final_text.
    //
    // v2.7.8 PR-3 — anthropic added. Different content-block model
    // (tool_use / tool_result types) but the loop shape is identical
    // and the audit doc's primary verification target lives here.
    matches!(p.flavor, "openai" | "gemini" | "minimax" | "anthropic")
}

fn resolve_model(provider: &ApiProvider, override_: Option<&str>) -> Result<String> {
    match (override_, provider.default_model) {
        (Some(m), _) if !m.is_empty() => Ok(m.to_string()),
        (None, "") => Err(anyhow!(
            "Provider '{}' has no default model — pass --model explicitly.",
            provider.slug
        )),
        (_, default) => Ok(default.to_string()),
    }
}

/// v2.7.9 PR-A — MiniMax content-as-args detection.
///
/// MiniMax-M2.7-highspeed (and its tier-mates) sometimes emits
/// function-call arguments as plain JSON in `message.content` rather
/// than using OpenAI's `choices[0].message.tool_calls[]` shape. The
/// `tools` field IS honored on the request side; the emission side
/// just doesn't structure it back. Without this fallback, the loop
/// reads the JSON as text and returns it to the user verbatim
/// (looks like the model "hallucinated" code that it actually
/// intended to read via a tool call).
///
/// Algorithm (4-seat war-room A803A3C3, unanimous; tightened in S1):
///   1. Try parsing the trimmed content as JSON directly. If that
///      succeeds, use the parsed value (handles "JSON only" AND the
///      "JSON with literal ``` inside a string value" case — e.g., a
///      tool argument carrying a code snippet).
///   2. Otherwise, look for a fenced ```json (or bare ```) block:
///      - exactly one fenced block (with or without a prose prefix
///        like "I'll call read_file: ```json {...}```") → parse the
///        inner contents
///      - zero fence markers or more than one fenced block →
///        ambiguous, return no call
///   3. Parsed value must be an object. Anything else → no call.
///   4. Strict-match against each offered tool's input_schema:
///      - All `required` fields present in obj.
///      - All obj keys present in schema's `properties`.
///      - When `required` is absent AND obj is non-empty, require at
///        least one obj key to intersect with `properties` (closes a
///        loose-match hole where a schema with no `required` would
///        otherwise accept anything).
///   5. Require EXACTLY ONE matching tool. Zero matches → text.
///      Two or more → ambiguous, prefer text over a guess.
///   6. Synthesize a ToolCall with a stable id (`mm-{round_idx}-0`)
///      so the next round's tool_result can reference it.
fn parse_minimax_content_as_tool_calls(
    content: &str,
    offered_tools: &[ToolDef],
    round_idx: usize,
) -> Vec<ToolCall> {
    let trimmed = content.trim();
    // Raw-first: a successful JSON parse of the whole trimmed content
    // wins, so that legitimate args carrying literal ``` in string
    // values (e.g., a code snippet) aren't mis-extracted by the fence
    // scanner. Fence detection is the prose-prefix fallback.
    let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => {
            let candidate = match find_fenced_json_block(trimmed) {
                FenceMatch::None => return Vec::new(),
                FenceMatch::Single(inner) => inner,
                FenceMatch::Multiple => return Vec::new(),
            };
            match serde_json::from_str(candidate) {
                Ok(v) => v,
                Err(_) => return Vec::new(),
            }
        }
    };
    let obj = match parsed.as_object() {
        Some(o) => o,
        None => return Vec::new(),
    };
    let matches: Vec<&ToolDef> = offered_tools
        .iter()
        .filter(|t| minimax_content_matches_schema(obj, &t.schema))
        .collect();
    if matches.len() != 1 {
        return Vec::new();
    }
    let t = matches[0];
    vec![ToolCall {
        id: format!("mm-{}-0", round_idx),
        name: t.name.to_string(),
        arguments: parsed,
    }]
}

/// Strict JSON-Schema field-presence match for content-as-args detection.
/// All required fields present + no keys outside `properties`. Type
/// checking deliberately omitted: missing/wrong types surface as tool
/// execution errors the model can correct on the next round, which is
/// the right error UX (vs. silently dropping the call here).
fn minimax_content_matches_schema(
    obj: &serde_json::Map<String, serde_json::Value>,
    schema: &serde_json::Value,
) -> bool {
    let required_present = schema.get("required").and_then(|v| v.as_array()).is_some();
    if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
        for r in required {
            if let Some(key) = r.as_str() {
                if !obj.contains_key(key) {
                    return false;
                }
            }
        }
    }
    let properties = schema.get("properties").and_then(|v| v.as_object());
    if let Some(props) = properties {
        for key in obj.keys() {
            if !props.contains_key(key) {
                return false;
            }
        }
    }
    // Without `required`, the only structural rule is "no extra keys",
    // which never fires when `properties` is absent — so any non-empty
    // object would match every such schema. Require a property-key
    // intersection in that case so empty-schema tools don't capture
    // unrelated payloads. Empty obj with absent `required` still
    // matches (preserves the schema-says-nothing edge case).
    if !required_present && !obj.is_empty() {
        let intersects = match properties {
            Some(props) => obj.keys().any(|k| props.contains_key(k)),
            None => false,
        };
        if !intersects {
            return false;
        }
    }
    true
}

/// Outcome of scanning for a fenced JSON block inside content.
enum FenceMatch<'a> {
    /// No fence markers — caller parses the raw content.
    None,
    /// Exactly one fenced block (whether the entire trimmed content
    /// is the fence or a prose prefix precedes it) — caller parses
    /// the inner content.
    Single(&'a str),
    /// More than one fenced block — ambiguous, caller should reject.
    Multiple,
}

/// Locate a fenced ```json (or bare ```) block inside `s`. Tolerates
/// a prose prefix (real-world MiniMax sometimes emits
/// `I'll call read_file: ```json {...}````). Counts ``` markers:
/// exactly two → one block (extract inner); zero or one → no block;
/// three or more → multiple blocks (ambiguous).
fn find_fenced_json_block(s: &str) -> FenceMatch<'_> {
    let positions: Vec<usize> = s.match_indices("```").map(|(i, _)| i).collect();
    match positions.len() {
        0 | 1 => FenceMatch::None,
        2 => {
            let open_end = positions[0] + 3;
            let close_start = positions[1];
            if close_start < open_end {
                return FenceMatch::None;
            }
            let inner = &s[open_end..close_start];
            // Strip optional language tag after the opening fence.
            let inner = inner
                .strip_prefix("json\n")
                .or_else(|| inner.strip_prefix("json\r\n"))
                .or_else(|| inner.strip_prefix("json "))
                .or_else(|| inner.strip_prefix("\n"))
                .or_else(|| inner.strip_prefix("\r\n"))
                .unwrap_or(inner);
            FenceMatch::Single(inner.trim())
        }
        _ => FenceMatch::Multiple,
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n).collect();
        format!("{}…", head)
    }
}

/// Conversation state across tool-call rounds. Holds whichever
/// shape (`messages` or `contents`) the active provider's flavor
/// expects, plus the small helpers needed to extend it.
struct Conversation {
    openai_messages: Vec<serde_json::Value>,
    gemini_contents: Vec<serde_json::Value>,
    /// v2.7.8 PR-3 — anthropic's Messages API uses a similar
    /// `messages` array to OpenAI but content blocks are structured
    /// (`text` / `tool_use` / `tool_result` types) rather than flat
    /// strings. Kept separate so we can preserve assistant turns'
    /// `tool_use` blocks across rounds.
    anthropic_messages: Vec<serde_json::Value>,
}

impl Conversation {
    fn new(provider: &ApiProvider, history: &[Message], prompt: &str) -> Self {
        let mut openai_messages = Vec::new();
        let mut gemini_contents = Vec::new();
        let mut anthropic_messages = Vec::new();
        match provider.flavor {
            "gemini" => {
                for m in history {
                    let role = if m.role == "assistant" { "model" } else { &m.role };
                    gemini_contents.push(serde_json::json!({
                        "role": role,
                        "parts": [{"text": m.content}],
                    }));
                }
                gemini_contents.push(serde_json::json!({
                    "role": "user",
                    "parts": [{"text": prompt}],
                }));
            }
            "anthropic" => {
                // Anthropic uses {role, content} like OpenAI but
                // content can be either a string OR an array of
                // typed blocks. For history we use plain string
                // content; tool-use rounds build typed-block arrays.
                for m in history {
                    anthropic_messages.push(serde_json::json!({
                        "role": m.role,
                        "content": m.content,
                    }));
                }
                anthropic_messages.push(serde_json::json!({
                    "role": "user",
                    "content": prompt,
                }));
            }
            _ => {
                for m in history {
                    openai_messages.push(serde_json::json!({
                        "role": m.role,
                        "content": m.content,
                    }));
                }
                openai_messages.push(serde_json::json!({
                    "role": "user",
                    "content": prompt,
                }));
            }
        }
        Self {
            openai_messages,
            gemini_contents,
            anthropic_messages,
        }
    }

    fn append_user_text(&mut self, provider: &ApiProvider, text: &str) {
        match provider.flavor {
            "gemini" => self.gemini_contents.push(serde_json::json!({
                "role": "user",
                "parts": [{"text": text}],
            })),
            "anthropic" => self.anthropic_messages.push(serde_json::json!({
                "role": "user",
                "content": text,
            })),
            _ => self.openai_messages.push(serde_json::json!({
                "role": "user",
                "content": text,
            })),
        }
    }

    fn build_url(&self, provider: &ApiProvider, model: &str, key: &str) -> String {
        if provider.flavor == "gemini" {
            let path = provider.path.replace("{model}", model);
            format!("{}{}?key={}", provider.base_url, path, urlencode(key))
        } else {
            format!("{}{}", provider.base_url, provider.path)
        }
    }

    fn build_request_body(
        &self,
        provider: &ApiProvider,
        tools: &[ToolDef],
        model: &str,
        suppress_tools: bool,
    ) -> serde_json::Value {
        match provider.flavor {
            "anthropic" => {
                let mut body = serde_json::json!({
                    "model": model,
                    "max_tokens": 8192,
                    "messages": self.anthropic_messages,
                });
                if !suppress_tools {
                    // Anthropic tool definitions use {name, description, input_schema}.
                    let anth_tools: Vec<serde_json::Value> = tools
                        .iter()
                        .map(|t| serde_json::json!({
                            "name": t.name,
                            "description": t.description,
                            "input_schema": t.schema,
                        }))
                        .collect();
                    body["tools"] = serde_json::json!(anth_tools);
                    body["tool_choice"] = serde_json::json!({ "type": "auto" });
                }
                body
            }
            "gemini" => {
                let mut body = serde_json::json!({
                    "contents": self.gemini_contents,
                    "generationConfig": { "maxOutputTokens": 8192 },
                });
                if !suppress_tools {
                    let decls: Vec<serde_json::Value> = tools
                        .iter()
                        .map(|t| serde_json::json!({
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.schema,
                        }))
                        .collect();
                    body["tools"] = serde_json::json!([{ "functionDeclarations": decls }]);
                }
                body
            }
            _ => {
                let mut body = serde_json::json!({
                    "model": model,
                    "messages": self.openai_messages,
                    "max_tokens": 8192,
                });
                if !suppress_tools {
                    let oa_tools: Vec<serde_json::Value> = tools
                        .iter()
                        .map(|t| serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.schema,
                            }
                        }))
                        .collect();
                    body["tools"] = serde_json::json!(oa_tools);
                    body["tool_choice"] = serde_json::json!("auto");
                }
                body
            }
        }
    }

    fn parse_tool_calls(
        &self,
        provider: &ApiProvider,
        payload: &serde_json::Value,
        offered_tools: &[ToolDef],
    ) -> Vec<ToolCall> {
        match provider.flavor {
            "anthropic" => {
                // Anthropic puts content as an array of blocks; each
                // block with type=tool_use is a tool invocation.
                let blocks = payload["content"].as_array().cloned().unwrap_or_default();
                let mut out = Vec::new();
                for b in blocks {
                    if b["type"].as_str() == Some("tool_use") {
                        let id = b["id"].as_str().unwrap_or("").to_string();
                        let name = b["name"].as_str().unwrap_or("").to_string();
                        let arguments = b.get("input").cloned().unwrap_or(serde_json::json!({}));
                        out.push(ToolCall { id, name, arguments });
                    }
                }
                out
            }
            "gemini" => {
                // Gemini puts function calls inside candidates[0].content.parts[].functionCall.
                let parts = payload["candidates"][0]["content"]["parts"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let mut out = Vec::new();
                for (idx, p) in parts.iter().enumerate() {
                    if let Some(fc) = p.get("functionCall") {
                        let name = fc["name"].as_str().unwrap_or("").to_string();
                        let arguments = fc.get("args").cloned().unwrap_or(serde_json::json!({}));
                        // Gemini doesn't give us a tool_call_id; synthesize a
                        // stable one tied to round + index so we can match
                        // tool results back up if the model emits multiple
                        // parallel calls in one turn.
                        let id = format!("gem-{}-{}", self.gemini_contents.len(), idx);
                        out.push(ToolCall { id, name, arguments });
                    }
                }
                out
            }
            _ => {
                let calls: Vec<ToolCall> = payload["choices"][0]["message"]["tool_calls"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|c| {
                        let id = c["id"].as_str()?.to_string();
                        let name = c["function"]["name"].as_str()?.to_string();
                        // OpenAI sends arguments as a JSON STRING, not an
                        // object. Parse it; on parse failure treat as
                        // empty object so the tool surfaces a useful error.
                        let raw_args = c["function"]["arguments"].as_str().unwrap_or("{}");
                        let arguments = serde_json::from_str(raw_args)
                            .unwrap_or(serde_json::json!({}));
                        Some(ToolCall { id, name, arguments })
                    })
                    .collect();
                // v2.7.9 PR-A — MiniMax content-as-args fallback.
                // MiniMax-M2.7-highspeed (subscription tier) emits
                // function-call args as plain JSON in message.content
                // instead of using OpenAI's choices[0].message.tool_calls[]
                // shape. When the OpenAI parser returns empty AND this
                // is the minimax flavor, attempt to detect content-as-
                // args by strict-matching the JSON against the offered
                // tools' input_schemas. Algorithm chosen by the
                // 4-seat war-room A803A3C3 (claude + google + minimax
                // unanimous on schema-strict-match with exactly-one-
                // match disambiguation; codex was rate-limited).
                if calls.is_empty() && provider.flavor == "minimax" {
                    if let Some(content) = payload["choices"][0]["message"]["content"].as_str() {
                        return parse_minimax_content_as_tool_calls(
                            content,
                            offered_tools,
                            self.openai_messages.len(),
                        );
                    }
                }
                calls
            }
        }
    }

    fn extract_final_text(
        &self,
        provider: &ApiProvider,
        payload: &serde_json::Value,
    ) -> Option<String> {
        match provider.flavor {
            "anthropic" => {
                let blocks = payload["content"].as_array()?;
                let texts: Vec<&str> = blocks
                    .iter()
                    .filter(|b| b["type"].as_str() == Some("text"))
                    .filter_map(|b| b["text"].as_str())
                    .collect();
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join(""))
                }
            }
            "gemini" => {
                let parts = payload["candidates"][0]["content"]["parts"].as_array()?;
                let texts: Vec<&str> = parts.iter().filter_map(|p| p["text"].as_str()).collect();
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join(""))
                }
            }
            // OpenAI-shape responses (openai-flavored providers + minimax).
            // Standard path: choices[0].message.content. MiniMax has a
            // quirk where if its response is truncated at max_tokens, the
            // main `content` arrives empty and the actual text lands in
            // `reasoning_content`. Mirror the same fallback we use in the
            // non-tool dispatch path (api_dispatch.rs) so a length-capped
            // MiniMax tool-loop reply isn't dropped silently.
            _ => {
                let msg = &payload["choices"][0]["message"];
                if let Some(c) = msg["content"].as_str() {
                    if !c.is_empty() {
                        return Some(c.to_string());
                    }
                }
                let finish_reason = payload["choices"][0]["finish_reason"].as_str().unwrap_or("");
                if finish_reason == "length" {
                    if let Some(r) = msg["reasoning_content"].as_str() {
                        if !r.is_empty() {
                            eprintln!(
                                "  [tools] {} finish_reason=length, falling back to reasoning_content ({} chars)",
                                provider.slug,
                                r.len()
                            );
                            return Some(r.to_string());
                        }
                    }
                }
                None
            }
        }
    }

    fn append_assistant_tool_calls(
        &mut self,
        provider: &ApiProvider,
        payload: &serde_json::Value,
        calls: &[ToolCall],
    ) {
        match provider.flavor {
            "anthropic" => {
                // Echo back the assistant's full content array
                // (text + tool_use blocks) so the next round preserves
                // the tool_use id needed to match tool_result.
                if let Some(content) = payload["content"].as_array() {
                    self.anthropic_messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": content,
                    }));
                }
            }
            "gemini" => {
                // For gemini we echo back the model's content message
                // (including its functionCall parts) verbatim so the
                // next round's request preserves the structure.
                if let Some(content) = payload["candidates"][0]["content"].as_object() {
                    self.gemini_contents.push(serde_json::Value::Object(content.clone()));
                }
            }
            _ => {
                // v2.7.9 PR-A — MiniMax content-as-args case:
                // payload's choices[0].message has `content: "<json>"`
                // but NO `tool_calls[]` field. Appending verbatim
                // leaves the next round's tool_result references
                // dangling. Synthesize an OpenAI-shape assistant
                // message with `tool_calls[]` from the parsed calls
                // so tool_call_ids resolve.
                let payload_has_tool_calls = payload["choices"][0]["message"]["tool_calls"]
                    .as_array()
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);
                if !payload_has_tool_calls && !calls.is_empty() {
                    let synthesized_tool_calls: Vec<serde_json::Value> = calls
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "id": c.id,
                                "type": "function",
                                "function": {
                                    "name": c.name,
                                    // OpenAI's tool_calls.function.arguments is
                                    // a JSON STRING, not an object. Match shape.
                                    "arguments": serde_json::to_string(&c.arguments)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                }
                            })
                        })
                        .collect();
                    self.openai_messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": serde_json::Value::Null,
                        "tool_calls": synthesized_tool_calls,
                    }));
                } else if let Some(msg) = payload["choices"][0]["message"].as_object() {
                    // Normal OpenAI shape — append verbatim.
                    self.openai_messages.push(serde_json::Value::Object(msg.clone()));
                }
            }
        }
    }

    fn append_tool_results(&mut self, provider: &ApiProvider, results: &[ToolResult]) {
        match provider.flavor {
            "anthropic" => {
                // Anthropic expects ALL tool_results for a turn in
                // one `user` message with multiple tool_result blocks.
                let blocks: Vec<serde_json::Value> = results
                    .iter()
                    .map(|r| serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": r.tool_call_id,
                        "content": r.content,
                        "is_error": r.is_error,
                    }))
                    .collect();
                self.anthropic_messages.push(serde_json::json!({
                    "role": "user",
                    "content": blocks,
                }));
            }
            "gemini" => {
                // Gemini expects ALL results for a turn in one
                // `user` message with multiple functionResponse parts.
                let parts: Vec<serde_json::Value> = results
                    .iter()
                    .map(|r| serde_json::json!({
                        "functionResponse": {
                            "name": r.name,
                            "response": { "content": r.content },
                        }
                    }))
                    .collect();
                self.gemini_contents.push(serde_json::json!({
                    "role": "user",
                    "parts": parts,
                }));
            }
            _ => {
                // OpenAI: one `tool` role message per result, keyed
                // by tool_call_id.
                for r in results {
                    self.openai_messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": r.tool_call_id,
                        "content": r.content,
                    }));
                }
            }
        }
    }

    fn extract_usage(
        &self,
        provider: &ApiProvider,
        payload: &serde_json::Value,
    ) -> (Option<i64>, Option<i64>) {
        match provider.flavor {
            "anthropic" => {
                let u = &payload["usage"];
                (u["input_tokens"].as_i64(), u["output_tokens"].as_i64())
            }
            "gemini" => {
                // v2.7.15 — include thoughtsTokenCount. Gemini 2.5
                // has thinking enabled by default; Google bills
                // thoughts at the output rate alongside candidates.
                // Some(..) anchored on candidates per war-room
                // C37BD156 round 1 #B — preserves None passthrough
                // when usageMetadata is missing entirely.
                let u = &payload["usageMetadata"];
                let tokens_out = u["candidatesTokenCount"].as_i64().map(|cand| {
                    cand + u["thoughtsTokenCount"].as_i64().unwrap_or(0)
                });
                (u["promptTokenCount"].as_i64(), tokens_out)
            }
            _ => {
                let u = &payload["usage"];
                (u["prompt_tokens"].as_i64(), u["completion_tokens"].as_i64())
            }
        }
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    // Suppress unused-import warning: base64 is used elsewhere.
    let _ = base64::engine::general_purpose::STANDARD;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_dispatch::ApiProvider;

    fn mock_openai_provider() -> ApiProvider {
        ApiProvider {
            slug: "test-openai",
            base_url: "https://example.com",
            path: "/v1/chat/completions",
            default_model: "gpt-test",
            env_var: "TEST_KEY",
            flavor: "openai",
        }
    }

    fn mock_gemini_provider() -> ApiProvider {
        ApiProvider {
            slug: "test-gemini",
            base_url: "https://example.com",
            path: "/v1beta/models/{model}:generateContent",
            default_model: "gemini-test",
            env_var: "TEST_KEY",
            flavor: "gemini",
        }
    }

    #[test]
    fn parses_openai_tool_call() {
        let payload = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"foo.rs\"}"
                        }
                    }]
                }
            }]
        });
        let conv = Conversation::new(&mock_openai_provider(), &[], "hi");
        let calls = conv.parse_tool_calls(&mock_openai_provider(), &payload, &[]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments["path"], "foo.rs");
        assert_eq!(calls[0].id, "call_abc");
    }

    #[test]
    fn parses_gemini_function_call() {
        let payload = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "grep",
                            "args": { "pattern": "fn dispatch" }
                        }
                    }]
                }
            }]
        });
        let conv = Conversation::new(&mock_gemini_provider(), &[], "hi");
        let calls = conv.parse_tool_calls(&mock_gemini_provider(), &payload, &[]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "grep");
        assert_eq!(calls[0].arguments["pattern"], "fn dispatch");
    }

    #[test]
    fn final_text_extracted_openai() {
        let payload = serde_json::json!({
            "choices": [{
                "message": { "content": "All good." }
            }]
        });
        let conv = Conversation::new(&mock_openai_provider(), &[], "hi");
        assert_eq!(
            conv.extract_final_text(&mock_openai_provider(), &payload).as_deref(),
            Some("All good.")
        );
    }

    #[test]
    fn final_text_extracted_gemini_multi_part() {
        let payload = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"text": "Findings:\n"},
                        {"text": "1. HIGH — …"}
                    ]
                }
            }]
        });
        let conv = Conversation::new(&mock_gemini_provider(), &[], "hi");
        assert_eq!(
            conv.extract_final_text(&mock_gemini_provider(), &payload).as_deref(),
            Some("Findings:\n1. HIGH — …")
        );
    }

    fn mock_anthropic_provider() -> ApiProvider {
        ApiProvider {
            slug: "test-anthropic",
            base_url: "https://api.anthropic.com",
            path: "/v1/messages",
            default_model: "claude-test",
            env_var: "TEST_KEY",
            flavor: "anthropic",
        }
    }

    // v2.7.8 PR-3 — Anthropic emits tool invocations as content
    // blocks of type "tool_use" with name/id/input fields. Pin the
    // parser shape so a future SDK shape change is loud.
    #[test]
    fn parses_anthropic_tool_use() {
        let payload = serde_json::json!({
            "content": [
                { "type": "text", "text": "I'll read that file." },
                {
                    "type": "tool_use",
                    "id": "toolu_01abc",
                    "name": "read_file",
                    "input": { "path": "apps/cli/src/main.rs" }
                }
            ],
            "stop_reason": "tool_use"
        });
        let conv = Conversation::new(&mock_anthropic_provider(), &[], "hi");
        let calls = conv.parse_tool_calls(&mock_anthropic_provider(), &payload, &[]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].id, "toolu_01abc");
        assert_eq!(calls[0].arguments["path"], "apps/cli/src/main.rs");
    }

    // v2.7.8 PR-3 — Anthropic final text comes out of content[] blocks
    // of type "text". Multiple blocks concatenate.
    #[test]
    fn final_text_extracted_anthropic_multi_block() {
        let payload = serde_json::json!({
            "content": [
                { "type": "text", "text": "Reviewing the file. " },
                { "type": "text", "text": "Findings:\n1. HIGH — bug at line 42." }
            ],
            "stop_reason": "end_turn"
        });
        let conv = Conversation::new(&mock_anthropic_provider(), &[], "hi");
        assert_eq!(
            conv.extract_final_text(&mock_anthropic_provider(), &payload).as_deref(),
            Some("Reviewing the file. Findings:\n1. HIGH — bug at line 42.")
        );
    }

    // v2.7.8 PR-3 — Anthropic request body includes `tools` with
    // `input_schema` (not `parameters` as OpenAI uses) and
    // `tool_choice: { type: "auto" }`.
    #[test]
    fn builds_anthropic_request_body() {
        let p = mock_anthropic_provider();
        let conv = Conversation::new(&p, &[], "hello");
        let tools = vec![review_tools::ToolDef {
            name: "read_file".to_string(),
            description: "read a file".to_string(),
            schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}}}),
        }];
        let body = conv.build_request_body(&p, &tools, "claude-test", false);
        assert_eq!(body["model"], "claude-test");
        assert_eq!(body["tool_choice"]["type"], "auto");
        assert_eq!(body["tools"][0]["name"], "read_file");
        assert_eq!(body["tools"][0]["input_schema"]["properties"]["path"]["type"], "string");
        // No top-level `parameters` field (that's the OpenAI shape).
        assert!(body["tools"][0].get("parameters").is_none());
    }

    // v2.7.9 PR-A — MiniMax content-as-args detection. Pinned by
    // the 4-seat war-room A803A3C3 algorithm.
    fn mock_minimax_provider() -> ApiProvider {
        ApiProvider {
            slug: "test-minimax",
            base_url: "https://api.minimax.io",
            path: "/v1/text/chatcompletion_v2",
            default_model: "MiniMax-test",
            env_var: "TEST_KEY",
            flavor: "minimax",
        }
    }

    fn read_file_tool() -> review_tools::ToolDef {
        review_tools::ToolDef {
            name: "read_file".to_string(),
            description: "Read a file from the repo.".to_string(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "start_line": { "type": "integer" },
                    "end_line": { "type": "integer" }
                },
                "required": ["path"]
            }),
        }
    }

    fn grep_tool() -> review_tools::ToolDef {
        review_tools::ToolDef {
            name: "grep".to_string(),
            description: "Search the repo for a regex pattern.".to_string(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "glob": { "type": "string" }
                },
                "required": ["pattern"]
            }),
        }
    }

    #[test]
    fn minimax_content_as_args_strict_match_returns_call() {
        let tools = vec![read_file_tool(), grep_tool()];
        let content = r#"{"path":"apps/cli/src/main.rs","start_line":80,"end_line":95}"#;
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments["path"], "apps/cli/src/main.rs");
        assert_eq!(calls[0].id, "mm-1-0");
    }

    #[test]
    fn minimax_content_as_args_fenced_block_stripped() {
        let tools = vec![read_file_tool()];
        let content = "```json\n{\"path\":\"foo.rs\"}\n```";
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 2);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn minimax_content_as_args_missing_required_no_match() {
        // No `path` field (required for read_file). No tool matches → empty.
        let tools = vec![read_file_tool(), grep_tool()];
        let content = r#"{"start_line": 80, "end_line": 95}"#;
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        assert!(calls.is_empty());
    }

    #[test]
    fn minimax_content_as_args_extra_keys_no_match() {
        // Has `path` but also `unexpected_field` not in properties → strict
        // match fails. Avoids matching prose-like content that happens to
        // contain a `path` key.
        let tools = vec![read_file_tool()];
        let content = r#"{"path":"foo.rs","unexpected_field":"hello"}"#;
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        assert!(calls.is_empty());
    }

    #[test]
    fn minimax_content_as_args_ambiguous_match_returns_empty() {
        // A tool that ONLY has optional fields (no required) — every JSON
        // object would match. Combined with read_file, a `{path: "..."}`
        // payload matches both. War-room rule: zero or >1 matches → text.
        let permissive = review_tools::ToolDef {
            name: "list_directory".to_string(),
            description: "List a directory".to_string(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
                // No "required" — every object matches
            }),
        };
        let tools = vec![read_file_tool(), permissive];
        let content = r#"{"path":"foo.rs"}"#;
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        // Both tools' schemas accept this object → ambiguous → empty.
        assert!(calls.is_empty(), "ambiguous matches must return empty");
    }

    #[test]
    fn minimax_content_as_args_non_json_content_returns_empty() {
        let tools = vec![read_file_tool()];
        let content = "This is just regular prose, not JSON.";
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        assert!(calls.is_empty());
    }

    // S1 — prose-prefix tolerance. Real MiniMax sometimes narrates
    // before the fenced JSON ("I'll call read_file: ```json …```").
    // The fence scanner counts ``` markers; exactly two = one block,
    // even when prose precedes the opening fence.
    #[test]
    fn prose_prefix_then_fenced_json_matches() {
        let tools = vec![read_file_tool()];
        let content = "I'll call read_file: ```json\n{\"path\":\"foo.rs\"}\n```";
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments["path"], "foo.rs");
    }

    // S1 — more than one fenced block is ambiguous: we won't guess
    // which block to use. Four ``` markers = two blocks → reject.
    #[test]
    fn multiple_fenced_blocks_returns_empty() {
        let tools = vec![read_file_tool()];
        let content = "first: ```json\n{\"path\":\"a.rs\"}\n``` and second: ```json\n{\"path\":\"b.rs\"}\n```";
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        assert!(calls.is_empty(), "multiple fenced blocks must return empty");
    }

    // S1 — the empty-obj edge case is preserved. A schema that lists
    // properties but no required field is "every input is valid"; an
    // empty payload `{}` is still a valid call for that schema.
    #[test]
    fn empty_object_no_required_still_matches() {
        let list_dir = review_tools::ToolDef {
            name: "list_directory".to_string(),
            description: "List a directory".to_string(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
                // no "required"
            }),
        };
        let tools = vec![list_dir];
        let content = "{}";
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_directory");
        assert!(calls[0].arguments.as_object().unwrap().is_empty());
    }

    // S1 — regression pin (war-room MiniMax catch). Valid top-level
    // JSON whose string values contain literal ``` (e.g., a code
    // snippet argument) must NOT be mis-extracted by the fence
    // scanner — the raw-first parse wins before fence detection runs.
    #[test]
    fn literal_backticks_inside_json_string_still_matches() {
        let snippet_tool = review_tools::ToolDef {
            name: "write_file".to_string(),
            description: "Write a file with a snippet".to_string(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "snippet": { "type": "string" }
                },
                "required": ["path", "snippet"]
            }),
        };
        let tools = vec![snippet_tool];
        let content = r#"{"path":"src/lib.rs","snippet":"```rust\nfn x() {}\n```"}"#;
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[0].arguments["path"], "src/lib.rs");
        assert!(calls[0]
            .arguments["snippet"]
            .as_str()
            .unwrap()
            .contains("fn x()"));
    }

    // S1 — strict-match tightening. A schema with no `required` AND
    // no `properties` used to accept any object (only the never-fires
    // extra-keys check ran). The new intersection rule rejects a
    // non-empty payload that has nothing to do with the schema.
    #[test]
    fn nonempty_object_no_required_no_property_intersection_returns_empty() {
        let opaque = review_tools::ToolDef {
            name: "opaque".to_string(),
            description: "A tool with an unconstrained schema".to_string(),
            schema: serde_json::json!({
                "type": "object"
                // neither "required" nor "properties"
            }),
        };
        let tools = vec![opaque];
        let content = r#"{"foo":"bar"}"#;
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        assert!(
            calls.is_empty(),
            "non-empty payload must not match a schema with no properties to intersect"
        );
    }

    #[test]
    fn minimax_content_as_args_json_array_returns_empty() {
        // Arrays at top level aren't a valid tool-call shape (we need
        // an object). Don't accidentally match against array input.
        let tools = vec![read_file_tool()];
        let content = r#"["path", "foo.rs"]"#;
        let calls = parse_minimax_content_as_tool_calls(content, &tools, 1);
        assert!(calls.is_empty());
    }

    #[test]
    fn minimax_payload_native_tool_calls_take_precedence() {
        // When MiniMax DOES emit tool_calls properly, content-as-args
        // detection must not fire — the native shape wins. Pin this so
        // future MiniMax model versions that fix the emission don't
        // double-fire calls.
        let payload = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_native",
                        "type": "function",
                        "function": {
                            "name": "grep",
                            "arguments": "{\"pattern\":\"fn dispatch\"}"
                        }
                    }],
                    "content": r#"{"path":"foo.rs"}"#
                }
            }]
        });
        let tools = vec![read_file_tool(), grep_tool()];
        let conv = Conversation::new(&mock_minimax_provider(), &[], "hi");
        let calls = conv.parse_tool_calls(&mock_minimax_provider(), &payload, &tools);
        assert_eq!(calls.len(), 1);
        // Native id used, not the synthesized mm-N-0 id.
        assert_eq!(calls[0].id, "call_native");
        assert_eq!(calls[0].name, "grep");
    }

    #[test]
    fn minimax_synthesized_assistant_message_has_tool_calls() {
        // History-echo test (claude #3 risk). When the model emitted
        // content-as-args, append_assistant_tool_calls must synthesize
        // an OpenAI-shape `tool_calls[]` field so the next round's
        // tool_result references resolve. Without this fix, the next
        // request body would be malformed.
        let p = mock_minimax_provider();
        let mut conv = Conversation::new(&p, &[], "hi");
        let payload = serde_json::json!({
            "choices": [{
                "message": {
                    // Note: NO tool_calls field, just content (the bug).
                    "content": r#"{"path":"foo.rs","start_line":1,"end_line":5}"#
                }
            }]
        });
        let calls = vec![ToolCall {
            id: "mm-0-0".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"foo.rs","start_line":1,"end_line":5}),
        }];
        conv.append_assistant_tool_calls(&p, &payload, &calls);
        let last = conv.openai_messages.last().expect("appended");
        assert_eq!(last["role"], "assistant");
        // The synthesized message MUST include tool_calls so tool_results match.
        let tc = last["tool_calls"]
            .as_array()
            .expect("synthesized tool_calls[] required");
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0]["id"], "mm-0-0");
        assert_eq!(tc[0]["function"]["name"], "read_file");
        // OpenAI's arguments is a JSON STRING, not an object.
        let args_str = tc[0]["function"]["arguments"].as_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(args_str).unwrap();
        assert_eq!(parsed["path"], "foo.rs");
    }

    // v2.7.8 PR-3 — Anthropic tool_result reply shape: role=user with
    // content array of tool_result blocks keyed by tool_use_id.
    #[test]
    fn appends_anthropic_tool_results() {
        let p = mock_anthropic_provider();
        let mut conv = Conversation::new(&p, &[], "hello");
        let results = vec![ToolResult {
            tool_call_id: "toolu_01abc".to_string(),
            name: "read_file".to_string(),
            content: "fn main() {}".to_string(),
            is_error: false,
        }];
        conv.append_tool_results(&p, &results);
        let last = conv.anthropic_messages.last().expect("appended");
        assert_eq!(last["role"], "user");
        let block = &last["content"][0];
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["tool_use_id"], "toolu_01abc");
        assert_eq!(block["content"], "fn main() {}");
        assert_eq!(block["is_error"], false);
    }

    // v2.8.0 closing — API-provider tool-call loop acceptance.
    //
    // ROADMAP item 5 promised "OpenAI/Anthropic function-call loop
    // for all 7 API providers." Most of the surface (parse_tool_calls,
    // append_tool_results, dispatch_with_tools) was already shipped
    // by v2.7.8 PR-3 for openai/gemini/minimax/anthropic. What was
    // missing was explicit acceptance proof for the OpenAI-FLAVOR
    // providers in the live registry — grok, deepseek, qwen,
    // openrouter, and openai (the slug added in v2.7.14 commit
    // 08796d6). These two tests pin the contract: any future provider
    // added to packages/ato-api-providers with flavor="openai"
    // automatically gains tool support + is verified by the existing
    // handler. A new flavor that needs its own parser branch can't
    // ship "marked supported" without breaking these tests.
    #[test]
    fn provider_supports_tools_covers_every_registry_provider_with_known_flavor() {
        for p in ato_api_providers::registry() {
            let supported = provider_supports_tools(p);
            let expected = matches!(p.flavor, "openai" | "gemini" | "minimax" | "anthropic");
            assert_eq!(
                supported, expected,
                "provider {} (flavor={}) expected supports_tools={}, got {}",
                p.slug, p.flavor, expected, supported
            );
        }
    }

    #[test]
    fn parses_openai_tool_call_works_for_every_openai_flavor_provider() {
        // Same payload shape (OpenAI chat-completions tool_calls) MUST
        // parse identically for every OpenAI-flavor provider — that's
        // the entire premise of the flavor abstraction. If a provider
        // ever diverges (e.g. DeepSeek changes their tool_call envelope),
        // this test fails BEFORE a user hits it in dogfood.
        let payload = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_xyz",
                        "type": "function",
                        "function": {
                            "name": "grep",
                            "arguments": "{\"pattern\":\"fn dispatch\"}"
                        }
                    }]
                }
            }]
        });
        let openai_flavor_providers: Vec<&'static ato_api_providers::ApiProvider> =
            ato_api_providers::registry()
                .iter()
                .filter(|p| p.flavor == "openai")
                .collect();
        assert!(
            openai_flavor_providers.len() >= 5,
            "expected at least 5 OpenAI-flavor providers (grok, deepseek, qwen, openrouter, openai); got {}",
            openai_flavor_providers.len()
        );
        for p in openai_flavor_providers {
            let conv = Conversation::new(p, &[], "test prompt");
            let calls = conv.parse_tool_calls(p, &payload, &[]);
            assert_eq!(
                calls.len(),
                1,
                "provider {} (flavor=openai) failed to parse tool_calls",
                p.slug
            );
            assert_eq!(calls[0].name, "grep", "provider {} name mismatch", p.slug);
            assert_eq!(
                calls[0].arguments["pattern"], "fn dispatch",
                "provider {} arguments mismatch",
                p.slug
            );
            assert_eq!(calls[0].id, "call_xyz", "provider {} id mismatch", p.slug);
        }
    }
}
