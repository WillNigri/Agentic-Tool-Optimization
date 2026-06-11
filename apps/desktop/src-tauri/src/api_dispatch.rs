// v2.3.26 Phase 6.x-C — desktop-side API-key dispatch.
//
// Mirror of apps/cli/src/api_dispatch.rs so the GUI's PromptBar can
// dispatch to MiniMax / Grok / DeepSeek / Qwen / OpenRouter directly
// (instead of falling through the prompt_agent path and erroring on
// "no CLI for runtime 'minimax'"). Two source-of-truths is fine for
// v1; Phase 6.x-E will extract the registry to a shared crate.
//
// Shape:
//   - Same provider registry as the CLI module (slugs must match for
//     llm_api_keys lookups to succeed).
//   - Async reqwest::Client (the desktop runs on tokio); the CLI uses
//     blocking. Identical request/response handling.
//   - Same MiniMax-flavored success check (base_resp.status_code).
//   - Key resolution: env var → llm_api_keys (case-insensitive,
//     base64-decoded). usage_count + last_used columns updated on
//     successful dispatch so the GUI's "API Keys" panel reflects use.

use rusqlite::Connection;
use serde::Serialize;
use std::time::Duration;

use crate::encryption;

// v2.3.28 Phase 6.x-E — ApiProvider + registry live in the shared
// `ato-api-providers` crate. Re-exported so the rest of this file
// (and commands.rs) keep their existing import shape.
pub use ato_api_providers::{find_provider, is_api_provider, registry, ApiProvider};

#[derive(Debug, Serialize, Clone)]
pub struct ApiDispatchOutcome {
    pub response: Option<String>,
    pub error_message: Option<String>,
    pub model_used: String,
    pub duration_ms: i64,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    /// v2.7.8 PR-3b — tool-call audit populated by
    /// `api_dispatch_tools::dispatch_with_tools` when the dispatch
    /// engaged the tool-call loop. None for legacy text-only
    /// dispatches. Empty vec means "tools were offered, model declined."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallAudit>>,
}

/// One row of the tool-call audit log written into
/// `execution_logs.tool_calls_summary` (a JSON array of these).
/// Mirrors `apps/cli/src/api_dispatch.rs::ToolCallAudit` exactly so the
/// GUI's Receipts table can render both transports uniformly.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallAudit {
    pub name: String,
    pub args_brief: String,
    pub is_error: bool,
}

/// A single turn in the conversation history passed into
/// `dispatch_with_tools`. v2.7.8 PR-3b — desktop needs this to mirror
/// the CLI shape; today's `dispatch()` builds a single-message body
/// inline, but the tool-call loop carries multi-turn state across
/// rounds.
#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// v2.15.0 Slice C: thin shim over the shared `ato-llm-key-resolver`
/// crate (war_room 0D398F74 codex finding). Encryption::decrypt is the
/// keychain-aware step that stays in this crate; the resolver handles
/// the env-var precedence + DB row selection + usage_count update so
/// CLI and desktop share one source of truth.
pub fn resolve_api_key(provider: &ApiProvider, conn: &Connection) -> Result<String, String> {
    let resolved = ato_llm_key_resolver::resolve_key_material(provider, conn)
        .map_err(|e| e.to_string())?;
    match resolved.source {
        ato_llm_key_resolver::KeySource::Env { .. } => Ok(resolved.material),
        ato_llm_key_resolver::KeySource::Stored { key_id } => {
            let plaintext = encryption::decrypt(&resolved.material).map_err(|e| e.to_string())?;
            let _ = ato_llm_key_resolver::touch_usage_count(conn, &key_id);
            Ok(plaintext)
        }
    }
}

pub async fn dispatch(
    provider: &ApiProvider,
    prompt: &str,
    model_override: Option<&str>,
    db_path: &std::path::Path,
) -> Result<ApiDispatchOutcome, String> {
    // Resolve the key in a scoped sync block so the Connection
    // drops before we hit any .await. Connection isn't Send; holding
    // it across await trips the "future cannot be sent between
    // threads safely" check Tauri imposes on commands.
    let key = {
        let conn = Connection::open(db_path).map_err(|e| format!("open db: {}", e))?;
        resolve_api_key(provider, &conn)?
    };
    let model = match (model_override, provider.default_model) {
        (Some(m), _) if !m.is_empty() => m.to_string(),
        (None, "") => {
            return Err(format!(
                "Provider '{}' has no default model — pass a model explicitly.",
                provider.slug
            ));
        }
        (_, default) => default.to_string(),
    };
    // v2.14.6 — gemini branch. Pre-2.14.6 the desktop dispatcher
    // applied OpenAI's shape uniformly: model in the body, key as
    // Bearer in the Authorization header, literal `{model}` token
    // left in `provider.path` because no replacement step was wired.
    // Gemini's API takes the model in the URL path and the key as a
    // `?key=` query param — Bearer auth is silently ignored, so
    // every desktop chat-to-gemini call hit Google as unauthenticated
    // and returned HTTP 401 SERVICE_BLOCKED. CLI's mirror
    // (apps/cli/src/api_dispatch.rs) had the gemini branch from v2.3
    // onward, which is why CLI dispatches worked while desktop chat
    // didn't. This block ports the same shape over.
    let (url, body, set_bearer) = if provider.flavor == "gemini" {
        let path = provider.path.replace("{model}", &model);
        let url = format!("{}{}?key={}", provider.base_url, path, urlencode(&key));
        let body = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [{"text": prompt}],
            }],
            "generationConfig": { "maxOutputTokens": 8192 },
        });
        (url, body, false)
    } else {
        let url = format!("{}{}", provider.base_url, provider.path);
        let body = serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": 4096,
        });
        (url, body, true)
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("build reqwest client: {}", e))?;

    let start = std::time::Instant::now();
    let mut req = client.post(&url).header("Content-Type", "application/json");
    if provider.flavor == "anthropic" {
        // Anthropic Messages API auth: x-api-key + required version
        // header. Bearer rejected.
        req = req
            .header("x-api-key", &key)
            .header("anthropic-version", "2023-06-01");
    } else if set_bearer {
        req = req.bearer_auth(&key);
    }
    // Gemini path puts the key in the URL — no auth header.
    let resp = req
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("POST {}: {}", url, e))?;
    let http_status = resp.status();
    let body_text = resp
        .text()
        .await
        .map_err(|e| format!("read response body from {}: {}", url, e))?;
    let duration_ms = start.elapsed().as_millis() as i64;

    if !http_status.is_success() {
        return Ok(ApiDispatchOutcome {
            response: None,
            error_message: Some(format!(
                "HTTP {}: {}",
                http_status.as_u16(),
                truncate_for_audit(&body_text, 1000)
            )),
            model_used: model,
            duration_ms,
            tokens_in: None,
            tokens_out: None,
            tool_calls: None,
        });
    }
    let payload: serde_json::Value =
        serde_json::from_str(&body_text).map_err(|e| format!("response not valid JSON: {}", e))?;
    parse_response(provider, payload, model, duration_ms)
}

fn parse_response(
    provider: &ApiProvider,
    payload: serde_json::Value,
    model: String,
    duration_ms: i64,
) -> Result<ApiDispatchOutcome, String> {
    if provider.flavor == "anthropic" {
        // Same shape as the CLI mirror — typed content blocks +
        // `usage.input_tokens / output_tokens`. Errors carry
        // `type:"error"` at the top level.
        if payload["type"].as_str() == Some("error") {
            let msg = payload["error"]["message"]
                .as_str()
                .unwrap_or("(no error.message)")
                .to_string();
            return Ok(ApiDispatchOutcome {
                response: None,
                error_message: Some(format!("Anthropic error: {}", msg)),
                model_used: model,
                duration_ms,
                tokens_in: None,
                tokens_out: None,
                tool_calls: None,
            });
        }
        // Concatenate text blocks without filtering empty strings —
        // legitimate refusals / tool-only responses / max_tokens stops
        // can produce empty text, and treating those as parse errors
        // misreports outcomes. (claude #4)
        let text = payload["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| {
                        if b["type"].as_str() == Some("text") {
                            b["text"].as_str().map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        let stop_reason = payload["stop_reason"].as_str().unwrap_or("");
        let error_message = if stop_reason == "max_tokens" {
            Some(
                "Anthropic warning: stop_reason=max_tokens — response truncated.".to_string(),
            )
        } else {
            None
        };
        return Ok(ApiDispatchOutcome {
            response: Some(text),
            error_message,
            model_used: model,
            duration_ms,
            tokens_in: payload["usage"]["input_tokens"].as_i64(),
            tokens_out: payload["usage"]["output_tokens"].as_i64(),
            tool_calls: None,
        });
    }
    // v2.14.6 — gemini response parsing. Mirror of the CLI's shape:
    // text lives at `candidates[0].content.parts[0..].text`; usage
    // metadata is `usageMetadata.promptTokenCount` / `candidatesTokenCount`
    // (+ optional `thoughtsTokenCount` which the CLI sums into output).
    if provider.flavor == "gemini" {
        if let Some(err) = payload["error"].as_object() {
            let code = err.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
            let msg = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("(no error.message)");
            return Ok(ApiDispatchOutcome {
                response: None,
                error_message: Some(format!("Gemini error {}: {}", code, msg)),
                model_used: model,
                duration_ms,
                tokens_in: None,
                tokens_out: None,
                tool_calls: None,
            });
        }
        let text = payload["candidates"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|c| c["content"]["parts"].as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p["text"].as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        let usage = &payload["usageMetadata"];
        let tokens_in = usage["promptTokenCount"].as_i64();
        let cand_out = usage["candidatesTokenCount"].as_i64().unwrap_or(0);
        let thought_out = usage["thoughtsTokenCount"].as_i64().unwrap_or(0);
        let tokens_out = if cand_out > 0 || thought_out > 0 {
            Some(cand_out + thought_out)
        } else {
            None
        };
        return Ok(ApiDispatchOutcome {
            response: if text.is_empty() { None } else { Some(text) },
            error_message: None,
            model_used: model,
            duration_ms,
            tokens_in,
            tokens_out,
            tool_calls: None,
        });
    }
    if provider.flavor == "minimax" {
        let br = &payload["base_resp"];
        let status = br["status_code"].as_i64();
        if status != Some(0) {
            let msg = br["status_msg"]
                .as_str()
                .unwrap_or("(no status_msg)")
                .to_string();
            return Ok(ApiDispatchOutcome {
                response: None,
                error_message: Some(format!(
                    "MiniMax base_resp.status_code={:?}: {}",
                    status, msg
                )),
                model_used: model,
                duration_ms,
                tokens_in: None,
                tokens_out: None,
                tool_calls: None,
            });
        }
    }
    let choices = payload["choices"].as_array();
    let content = choices
        .and_then(|arr| arr.first())
        .and_then(|c| c["message"]["content"].as_str())
        .map(|s| s.to_string());
    let response = match content {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Ok(ApiDispatchOutcome {
                response: None,
                error_message: Some(format!(
                    "no choices[0].message.content in response: {}",
                    truncate_for_audit(&payload.to_string(), 600)
                )),
                model_used: model,
                duration_ms,
                tokens_in: None,
                tokens_out: None,
                tool_calls: None,
            });
        }
    };
    let usage = &payload["usage"];
    let tokens_in = usage["prompt_tokens"].as_i64();
    let tokens_out = usage["completion_tokens"].as_i64();
    Ok(ApiDispatchOutcome {
        response: Some(response),
        error_message: None,
        model_used: model,
        duration_ms,
        tokens_in,
        tokens_out,
        tool_calls: None,
    })
}

/// v2.14.6 — RFC 3986 percent-encoder for the Gemini URL `?key=` param.
/// Mirrors apps/cli/src/api_dispatch.rs::urlencode so both crates encode
/// the same way without pulling in the `url` crate (the dep). Adequate for
/// the small character class API keys can contain.
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

fn truncate_for_audit(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}
