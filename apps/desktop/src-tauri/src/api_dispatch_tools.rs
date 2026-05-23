// v2.7.8 PR-3b — desktop async port of the CLI's tool-call loop.
//
// The CLI's `apps/cli/src/api_dispatch_tools.rs` implements a tool-call
// loop in BLOCKING reqwest because the CLI binary's dispatch path is
// synchronous. The desktop runs on tokio and the Tauri command
// `prompt_api_provider` is `async fn`, so it needs an async port.
//
// This is currently a parallel implementation — codex's PR-3b review
// flagged option (c) "extract the loop body into a flavor-agnostic
// async helper, CLI wraps in a blocking shim, desktop calls natively"
// as the cleanest long-term design. v2.7.8 ships duplication for
// speed; v2.7.9 can do the extraction. The Conversation type, request
// shapes, and parser logic are intentionally identical to the CLI
// version so a future merge is a straight rename.
//
// Supported flavors today (matches the CLI):
//   - openai     (grok, deepseek, qwen, openrouter, minimax)
//   - gemini     (google)
//   - anthropic  (PR-3 target)

use crate::api_dispatch::{resolve_api_key, ApiDispatchOutcome, ApiProvider, ToolCallAudit};
use crate::commands::mcp_dispatch::{execute_mcp_tool, McpToolBinding};
use ato_review_tools::{execute_call_with_root, ToolCall, ToolDef, ToolResult, MAX_TOOL_ROUNDS};
use rusqlite::Connection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Run a dispatch with a permission-aware tool registry available to
/// the model. Async mirror of `apps/cli/src/api_dispatch_tools.rs::
/// dispatch_with_tools`. The `tools` parameter is caller-provided
/// (filtered by the agent's permission gate before the call) and the
/// `workspace_root` parameter scopes the executor sandbox — required
/// because the desktop process cwd is `apps/desktop/`, not the user's
/// repo.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_with_tools(
    provider: &ApiProvider,
    history: &[crate::api_dispatch::Message],
    prompt: &str,
    model_override: Option<&str>,
    tools: &[ToolDef],
    // v2.7.10 PR-B — MCP bindings the agent's gate allowed. When a
    // tool_call's name matches an entry here, route to
    // execute_mcp_tool instead of the in-process review_tools
    // executor. Empty for legacy callers that only offer built-in
    // tools.
    mcp_bindings: &[McpToolBinding],
    workspace_root: &Path,
    db_path: &Path,
) -> Result<ApiDispatchOutcome, String> {
    if !provider_supports_tools(provider) {
        return Err(format!(
            "provider '{}' does not have a tools-flavor mapping; fall back to dispatch_with_history",
            provider.slug
        ));
    }

    // Resolve key in a scoped sync block so Connection drops before
    // we hit any .await — same pattern as `dispatch()`.
    let key = {
        let conn = Connection::open(db_path).map_err(|e| format!("open db: {}", e))?;
        resolve_api_key(provider, &conn)?
    };
    let model = resolve_model(provider, model_override)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| format!("build reqwest client: {}", e))?;

    let mut conv = Conversation::new(provider, history, prompt);
    let start = std::time::Instant::now();
    let mut rounds = 0usize;
    let mut accumulated_text = String::new();
    let mut tokens_in_total: Option<i64> = None;
    let mut tokens_out_total: Option<i64> = None;
    let mut audit: Vec<ToolCallAudit> = Vec::new();
    let mut empty_response_retries = 0usize;
    const MAX_EMPTY_RETRIES: usize = 1;

    eprintln!(
        "  [tools] desktop dispatch_with_tools provider={} flavor={} model={}",
        provider.slug, provider.flavor, model
    );
    loop {
        if rounds >= MAX_TOOL_ROUNDS {
            conv.append_user_text(
                provider,
                "Maximum tool rounds reached. Write your final reply now using what you've learned. No more tool calls.",
            );
        }

        let body = conv.build_request_body(provider, tools, &model, rounds >= MAX_TOOL_ROUNDS);
        let url = conv.build_url(provider, &model, &key);
        let mut req = client.post(&url).header("Content-Type", "application/json");
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
        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("POST {}: {}", url, e))?;
        let http_status = resp.status();
        let body_text = resp
            .text()
            .await
            .map_err(|e| format!("read response body: {}", e))?;
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
            serde_json::from_str(&body_text).map_err(|e| format!("response not valid JSON: {}", e))?;

        let (ti, to) = conv.extract_usage(provider, &payload);
        if ti.is_some() {
            tokens_in_total = Some(tokens_in_total.unwrap_or(0) + ti.unwrap_or(0));
        }
        if to.is_some() {
            tokens_out_total = Some(tokens_out_total.unwrap_or(0) + to.unwrap_or(0));
        }

        let calls = conv.parse_tool_calls(provider, &payload, tools);
        let assistant_text = conv.extract_final_text(provider, &payload);
        if !calls.is_empty() && rounds < MAX_TOOL_ROUNDS {
            conv.append_assistant_tool_calls(provider, &payload, &calls);
            // v2.7.10 PR-B — route each tool_call: built-in tools
            // (read_file/grep/…) execute synchronously via the
            // review-tools sandbox; MCP tools execute via the awaited
            // execute_mcp_tool which wraps the stdio JSON-RPC round-
            // trip in spawn_blocking. Anything matching neither yields
            // the existing "unknown tool" error so the model sees a
            // recoverable failure on the next turn.
            let builtin_names: HashSet<String> = ato_review_tools::registry()
                .into_iter()
                .map(|t| t.name)
                .collect();
            let mut results: Vec<ToolResult> = Vec::with_capacity(calls.len());
            for c in &calls {
                let r = if builtin_names.contains(&c.name) {
                    // PR-3b — sandbox root is the explicit workspace
                    // root, not the desktop process cwd. Otherwise
                    // every read_file would resolve under
                    // `apps/desktop/`, not the user's repo.
                    execute_call_with_root(workspace_root, c)
                } else if let Some(binding) = mcp_bindings.iter().find(|b| b.name == c.name) {
                    execute_mcp_tool(&binding.mcp_slug, &c.name, &c.arguments, &c.id).await
                } else {
                    // Mirror execute_call_with_root's unknown-tool
                    // shape so the model gets a uniform error surface.
                    ToolResult {
                        tool_call_id: c.id.clone(),
                        name: c.name.clone(),
                        content: format!(
                            "error: unknown tool '{}'. Tool was not in the offered set.",
                            c.name
                        ),
                        is_error: true,
                    }
                };
                // S10 (v2.7.11) — shared log + audit-args helper. Was a
                // 10-line block duplicated verbatim with the CLI's sync
                // dispatch_with_tools.
                let args_brief = ato_review_tools::log_tool_call_and_brief_args(c, &r);
                audit.push(ToolCallAudit {
                    name: c.name.clone(),
                    args_brief,
                    is_error: r.is_error,
                });
                results.push(r);
            }
            conv.append_tool_results(provider, &results);
            rounds += 1;
            continue;
        }

        if let Some(t) = assistant_text {
            accumulated_text.push_str(&t);
        }
        let duration_ms = start.elapsed().as_millis() as i64;
        if accumulated_text.trim().is_empty() {
            if empty_response_retries < MAX_EMPTY_RETRIES && !audit.is_empty() {
                empty_response_retries += 1;
                eprintln!(
                    "  [tools] empty response after round {}; nudging model to write final reply (retry {}/{})",
                    rounds, empty_response_retries, MAX_EMPTY_RETRIES
                );
                conv.append_user_text(
                    provider,
                    "You returned no text. Please write your final reply now in plain markdown, using the tool-call results from earlier rounds. Do not call any more tools.",
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
    matches!(p.flavor, "openai" | "gemini" | "minimax" | "anthropic")
}

fn resolve_model(provider: &ApiProvider, override_: Option<&str>) -> Result<String, String> {
    match (override_, provider.default_model) {
        (Some(m), _) if !m.is_empty() => Ok(m.to_string()),
        (None, "") => Err(format!(
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

// v2.7.9 PR-A — MiniMax content-as-args detection. Mirror of
// `apps/cli/src/api_dispatch_tools.rs::parse_minimax_content_as_tool_calls`.
// MiniMax-M2.7-highspeed emits function-call args as plain JSON in
// `message.content` rather than using OpenAI's tool_calls[] shape;
// this fallback strict-matches the JSON against offered tools'
// input_schemas. Unit tests live in the CLI side; this is a verbatim
// port (the algorithm is pure-data, sync OR async).
fn parse_minimax_content_as_tool_calls(
    content: &str,
    offered_tools: &[ToolDef],
    round_idx: usize,
) -> Vec<ToolCall> {
    let trimmed = content.trim();
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

enum FenceMatch<'a> {
    None,
    Single(&'a str),
    Multiple,
}

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

/// Conversation state across tool-call rounds — direct port of the
/// CLI's Conversation struct. Holds whichever shape (`messages` or
/// `contents`) the active provider's flavor expects.
struct Conversation {
    openai_messages: Vec<serde_json::Value>,
    gemini_contents: Vec<serde_json::Value>,
    anthropic_messages: Vec<serde_json::Value>,
}

impl Conversation {
    fn new(
        provider: &ApiProvider,
        history: &[crate::api_dispatch::Message],
        prompt: &str,
    ) -> Self {
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
        // v2.8.x P0 — UNTRUSTED_INPUT system fragment when tools are
        // active. Mirror of CLI api_dispatch_tools.rs::build_request_body.
        let tools_active = !suppress_tools && !tools.is_empty();
        match provider.flavor {
            "anthropic" => {
                let mut body = serde_json::json!({
                    "model": model,
                    "max_tokens": 8192,
                    "messages": self.anthropic_messages,
                });
                if tools_active {
                    body["system"] = serde_json::json!(ato_review_tools::UNTRUSTED_INPUT_PROMPT_FRAGMENT);
                }
                if !suppress_tools {
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
                if tools_active {
                    body["systemInstruction"] = serde_json::json!({
                        "parts": [{ "text": ato_review_tools::UNTRUSTED_INPUT_PROMPT_FRAGMENT }],
                    });
                }
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
                // OpenAI: prepend system message inline (don't mutate
                // self.openai_messages so the next round still starts
                // from the same conversation history).
                let messages: Vec<serde_json::Value> = if tools_active {
                    let mut prefixed = Vec::with_capacity(self.openai_messages.len() + 1);
                    prefixed.push(serde_json::json!({
                        "role": "system",
                        "content": ato_review_tools::UNTRUSTED_INPUT_PROMPT_FRAGMENT,
                    }));
                    prefixed.extend(self.openai_messages.iter().cloned());
                    prefixed
                } else {
                    self.openai_messages.clone()
                };
                let mut body = serde_json::json!({
                    "model": model,
                    "messages": messages,
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
                let parts = payload["candidates"][0]["content"]["parts"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let mut out = Vec::new();
                for (idx, p) in parts.iter().enumerate() {
                    if let Some(fc) = p.get("functionCall") {
                        let name = fc["name"].as_str().unwrap_or("").to_string();
                        let arguments = fc.get("args").cloned().unwrap_or(serde_json::json!({}));
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
                        let raw_args = c["function"]["arguments"].as_str().unwrap_or("{}");
                        let arguments = serde_json::from_str(raw_args)
                            .unwrap_or(serde_json::json!({}));
                        Some(ToolCall { id, name, arguments })
                    })
                    .collect();
                // v2.7.9 PR-A — MiniMax content-as-args fallback. See
                // apps/cli/src/api_dispatch_tools.rs for full rationale
                // and the 4-seat war-room A803A3C3 algorithm. Mirror
                // implementation; tests in CLI side.
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
                if let Some(content) = payload["content"].as_array() {
                    self.anthropic_messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": content,
                    }));
                }
            }
            "gemini" => {
                if let Some(content) = payload["candidates"][0]["content"].as_object() {
                    self.gemini_contents.push(serde_json::Value::Object(content.clone()));
                }
            }
            _ => {
                // v2.7.9 PR-A — MiniMax content-as-args synthesizes
                // an OpenAI-shape tool_calls[] so the next round's
                // tool_result references resolve. See CLI mirror.
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
                    self.openai_messages.push(serde_json::Value::Object(msg.clone()));
                }
            }
        }
    }

    fn append_tool_results(&mut self, provider: &ApiProvider, results: &[ToolResult]) {
        // v2.8.x P0 — see ato_review_tools::wrap_untrusted docs and
        // the CLI mirror at api_dispatch_tools.rs::append_tool_results.
        // Source attribution is "tool:<name>"; helper defangs in-payload
        // closing tags so an injection can't break out of the wrapper.
        let wrap = |r: &ToolResult| -> String {
            ato_review_tools::wrap_untrusted_input(&format!("tool:{}", r.name), &r.content)
        };
        match provider.flavor {
            "anthropic" => {
                let blocks: Vec<serde_json::Value> = results
                    .iter()
                    .map(|r| serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": r.tool_call_id,
                        "content": wrap(r),
                        "is_error": r.is_error,
                    }))
                    .collect();
                self.anthropic_messages.push(serde_json::json!({
                    "role": "user",
                    "content": blocks,
                }));
            }
            "gemini" => {
                let parts: Vec<serde_json::Value> = results
                    .iter()
                    .map(|r| serde_json::json!({
                        "functionResponse": {
                            "name": r.name,
                            "response": { "content": wrap(r) },
                        }
                    }))
                    .collect();
                self.gemini_contents.push(serde_json::json!({
                    "role": "user",
                    "parts": parts,
                }));
            }
            _ => {
                for r in results {
                    self.openai_messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": r.tool_call_id,
                        "content": wrap(r),
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
                // when usageMetadata is missing entirely so we
                // don't conflate "unmeasured" with "$0 free run".
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
    out
}

/// Public helper for the call site: given an agent's permission gate
/// and the optional MCP tools, return the filtered review_tools list
/// to pass to dispatch_with_tools. Mirrors the CLI's filter logic at
/// `apps/cli/src/commands/dispatch.rs::run_api` so the two paths agree
/// on what the agent sees. RequireApproval tools are NOT included
/// (war-room finding: PR-5 approval UI not yet built, so denying
/// implicitly is safer than executing without approval).
pub fn build_filtered_review_tools(
    gate: &ato_agent_permissions::ToolGate,
) -> Vec<ToolDef> {
    if gate.allowed_tools.is_empty() {
        return ato_review_tools::registry();
    }
    ato_review_tools::registry()
        .into_iter()
        .filter(|t| {
            matches!(
                gate.check(&t.name),
                ato_agent_permissions::GateDecision::Allow
            )
        })
        .collect()
}

/// Resolve the workspace root for tool-call sandboxing by walking up
/// from `start` looking for `.git` or `.claude/`. RETAINED as a
/// helper for future call sites that have a known starting point
/// (e.g. an agent's stored project path), but NOT used by
/// `prompt_api_provider` today.
///
/// War-room finding (codex 2026-05-20): walking up from
/// `std::env::current_dir()` is unsafe — the desktop's cwd is
/// `apps/desktop/` (in dev) or wherever Tauri was launched (in prod),
/// neither of which is the user's intended project. The Tauri command
/// now requires the frontend to pass `workspace_root` explicitly and
/// refuses tool-using dispatches without it. This function stays
/// available for callers who already have a trusted starting point.
#[allow(dead_code)]
pub fn resolve_workspace_root(start: &Path) -> PathBuf {
    let mut cur: PathBuf = start.to_path_buf();
    loop {
        if cur.join(".git").exists() || cur.join(".claude").exists() {
            return cur;
        }
        if !cur.pop() {
            return start.to_path_buf();
        }
    }
}
