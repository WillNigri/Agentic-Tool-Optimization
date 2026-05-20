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
use ato_review_tools::{execute_call_with_root, ToolCall, ToolDef, ToolResult, MAX_TOOL_ROUNDS};
use rusqlite::Connection;
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

        let calls = conv.parse_tool_calls(provider, &payload);
        let assistant_text = conv.extract_final_text(provider, &payload);
        if !calls.is_empty() && rounds < MAX_TOOL_ROUNDS {
            conv.append_assistant_tool_calls(provider, &payload, &calls);
            let results: Vec<ToolResult> = calls
                .iter()
                .map(|c| {
                    // PR-3b — sandbox root is the explicit workspace
                    // root, not the desktop process cwd. Otherwise
                    // every read_file would resolve under
                    // `apps/desktop/`, not the user's repo.
                    let r = execute_call_with_root(workspace_root, c);
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
        match provider.flavor {
            "anthropic" => {
                let mut body = serde_json::json!({
                    "model": model,
                    "max_tokens": 8192,
                    "messages": self.anthropic_messages,
                });
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
                let calls = payload["choices"][0]["message"]["tool_calls"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                calls
                    .into_iter()
                    .filter_map(|c| {
                        let id = c["id"].as_str()?.to_string();
                        let name = c["function"]["name"].as_str()?.to_string();
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
        _calls: &[ToolCall],
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
                if let Some(msg) = payload["choices"][0]["message"].as_object() {
                    self.openai_messages.push(serde_json::Value::Object(msg.clone()));
                }
            }
        }
    }

    fn append_tool_results(&mut self, provider: &ApiProvider, results: &[ToolResult]) {
        match provider.flavor {
            "anthropic" => {
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
                gate.check(t.name),
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
