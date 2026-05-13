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
//   - openai  (grok, deepseek, qwen, openrouter, minimax)
//   - gemini  (google)
//
// MiniMax tool-calling has known reliability issues on the
// subscription tier; the caller (`ato review`) decides whether to
// route a given provider through the tool-using path or fall back
// to plain dispatch_with_history.

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use rusqlite::Connection;
use std::time::Duration;

use crate::api_dispatch::{resolve_api_key, ApiDispatchOutcome, ApiProvider, Message, ToolCallAudit};
use crate::review_tools::{self, ToolCall, ToolDef, ToolResult, MAX_TOOL_ROUNDS};

/// Run a dispatch with the review-tools registry available to the
/// model. Iterates: dispatch → execute tool calls → dispatch again →
/// … until the model produces a final text response or we hit the
/// round cap.
pub fn dispatch_with_tools(
    provider: &ApiProvider,
    history: &[Message],
    prompt: &str,
    model_override: Option<&str>,
    conn: &Connection,
) -> Result<ApiDispatchOutcome> {
    let tools = review_tools::registry();
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

        let body = conv.build_request_body(provider, &tools, &model, rounds >= MAX_TOOL_ROUNDS);
        let url = conv.build_url(provider, &model, &key);
        let mut req = client.post(&url).header("Content-Type", "application/json");
        if conv.use_bearer_auth(provider) {
            req = req.bearer_auth(&key);
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
        let calls = conv.parse_tool_calls(provider, &payload);
        let assistant_text = conv.extract_final_text(provider, &payload);
        if !calls.is_empty() && rounds < MAX_TOOL_ROUNDS {
            // Append the assistant's tool-call turn to history, then
            // execute each call and append the results.
            conv.append_assistant_tool_calls(provider, &payload, &calls);
            let results: Vec<ToolResult> = calls
                .iter()
                .map(|c| {
                    let r = review_tools::execute_call(c);
                    eprintln!(
                        "  [tool] {} {} -> {}{}",
                        c.name,
                        truncate(&c.arguments.to_string(), 80),
                        if r.is_error { "ERR " } else { "" },
                        truncate(&r.content, 80)
                    );
                    audit.push(ToolCallAudit {
                        name: c.name.clone(),
                        args_brief: truncate(&c.arguments.to_string(), 120),
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
    matches!(p.flavor, "openai" | "gemini" | "minimax")
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
}

impl Conversation {
    fn new(provider: &ApiProvider, history: &[Message], prompt: &str) -> Self {
        let mut openai_messages = Vec::new();
        let mut gemini_contents = Vec::new();
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
        }
    }

    fn append_user_text(&mut self, provider: &ApiProvider, text: &str) {
        match provider.flavor {
            "gemini" => self.gemini_contents.push(serde_json::json!({
                "role": "user",
                "parts": [{"text": text}],
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

    fn use_bearer_auth(&self, provider: &ApiProvider) -> bool {
        provider.flavor != "gemini"
    }

    fn build_request_body(
        &self,
        provider: &ApiProvider,
        tools: &[ToolDef],
        model: &str,
        suppress_tools: bool,
    ) -> serde_json::Value {
        match provider.flavor {
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

    fn parse_tool_calls(&self, provider: &ApiProvider, payload: &serde_json::Value) -> Vec<ToolCall> {
        match provider.flavor {
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
                let calls = payload["choices"][0]["message"]["tool_calls"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                calls
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
                    .collect()
            }
        }
    }

    fn extract_final_text(
        &self,
        provider: &ApiProvider,
        payload: &serde_json::Value,
    ) -> Option<String> {
        match provider.flavor {
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
        _calls: &[ToolCall],
    ) {
        match provider.flavor {
            "gemini" => {
                // For gemini we echo back the model's content message
                // (including its functionCall parts) verbatim so the
                // next round's request preserves the structure.
                if let Some(content) = payload["candidates"][0]["content"].as_object() {
                    self.gemini_contents.push(serde_json::Value::Object(content.clone()));
                }
            }
            _ => {
                // For OpenAI shape we append the assistant message
                // (with tool_calls) so the upstream parser can match
                // the upcoming `tool` role replies by tool_call_id.
                if let Some(msg) = payload["choices"][0]["message"].as_object() {
                    self.openai_messages.push(serde_json::Value::Object(msg.clone()));
                }
            }
        }
    }

    fn append_tool_results(&mut self, provider: &ApiProvider, results: &[ToolResult]) {
        match provider.flavor {
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
            "gemini" => {
                let u = &payload["usageMetadata"];
                (
                    u["promptTokenCount"].as_i64(),
                    u["candidatesTokenCount"].as_i64(),
                )
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
        let calls = conv.parse_tool_calls(&mock_openai_provider(), &payload);
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
        let calls = conv.parse_tool_calls(&mock_gemini_provider(), &payload);
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
}
