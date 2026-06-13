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
    /// Anthropic 5-min prompt-cache WRITE tokens. Billed at 1.25× input
    /// rate. None for non-Anthropic providers.
    ///
    /// IMPORTANT: Anthropic `input_tokens` does NOT include this class.
    /// Total billed input = input_tokens + cache_creation*1.25 + cache_read*0.10
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<i64>,
    /// Anthropic 5-min prompt-cache READ tokens. Billed at 0.10× input rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,
    /// OpenAI/DeepSeek reasoning tokens (informational breakdown only).
    ///
    /// IMPORTANT: completion_tokens ALREADY INCLUDES reasoning_tokens.
    /// Do NOT add this to cost. Purely for observability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i64>,
    /// v2.7.8 PR-3b — tool-call audit populated by
    /// `api_dispatch_tools::dispatch_with_tools` when the dispatch
    /// engaged the tool-call loop. None for legacy text-only
    /// dispatches. Empty vec means "tools were offered, model declined."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallAudit>>,
    /// v2.15.1 (war_room 08F8629A) — number of retries that fired
    /// before the final outcome. 0 = first attempt succeeded.
    /// Always populated; defaults to 0 for compatibility.
    #[serde(default)]
    pub retry_count: i64,
    /// v2.15.1 — JSON-serialized array of AttemptRecord from
    /// ato-retry-policy. NULL when no retries fired. Used by the
    /// receipt UI to render the attempt timeline + by analytics
    /// to identify providers with high transient-failure rates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_summary_json: Option<String>,
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

    // v2.15.1 (war_room 08F8629A) — retry-with-backoff. Wraps the
    // send/receive cycle in the shared classifier from ato-retry-policy.
    // For ALL providers + ALL retriable codes (503/502/504/429 +
    // transport timeouts + MiniMax body-status). Loops up to
    // policy.max_attempts times; sleeps with backoff between attempts.
    // The final attempt's response is what gets parsed. retry_count +
    // JSON attempt summary land on the returned ApiDispatchOutcome so
    // the audit row records the full timeline.
    let policy = ato_retry_policy::RetryPolicy::default_v1();
    let mut attempts: Vec<ato_retry_policy::AttemptRecord> = Vec::new();
    let overall_start = std::time::Instant::now();

    let (final_http_status, final_body_text, final_attempt_duration_ms) = loop {
        let attempt_start = std::time::Instant::now();
        let mut req = client.post(&url).header("Content-Type", "application/json");
        if provider.flavor == "anthropic" {
            req = req
                .header("x-api-key", &key)
                .header("anthropic-version", "2023-06-01");
        } else if set_bearer {
            req = req.bearer_auth(&key);
        }
        // Gemini path puts the key in the URL — no auth header.

        let result = req.json(&body).send().await;
        let elapsed_ms = attempt_start.elapsed().as_millis() as i64;

        let (http_status_code, body_text, headers_map, transport_err): (
            Option<u16>,
            String,
            std::collections::HashMap<String, String>,
            Option<String>,
        ) = match result {
            Ok(resp) => {
                let s = resp.status().as_u16();
                let mut h = std::collections::HashMap::new();
                for (k, v) in resp.headers().iter() {
                    if let Ok(vs) = v.to_str() {
                        h.insert(k.as_str().to_string(), vs.to_string());
                    }
                }
                let body = resp.text().await.unwrap_or_default();
                (Some(s), body, h, None)
            }
            Err(e) => (
                None,
                String::new(),
                std::collections::HashMap::new(),
                Some(format!("POST {}: {}", url, e)),
            ),
        };

        let outcome = ato_retry_policy::classify_attempt(
            provider.flavor,
            http_status_code,
            &headers_map,
            Some(&body_text),
            transport_err.as_deref(),
        );
        let outcome_class =
            ato_retry_policy::AttemptRecord::outcome_class_for(&outcome, http_status_code);
        let error_brief = match &outcome {
            ato_retry_policy::AttemptOutcome::Success => None,
            ato_retry_policy::AttemptOutcome::PermanentError { reason }
            | ato_retry_policy::AttemptOutcome::RetriableError { reason, .. }
            | ato_retry_policy::AttemptOutcome::TransportFailure { reason } => {
                Some(truncate_for_audit(reason, 240))
            }
        };
        attempts.push(ato_retry_policy::AttemptRecord {
            attempt_index: attempts.len() as u32,
            started_at_ms: chrono::Utc::now().timestamp_millis(),
            duration_ms: elapsed_ms,
            status_code: http_status_code,
            outcome_class,
            error_brief,
        });

        let history_before = &attempts[..attempts.len() - 1];
        let disposition = ato_retry_policy::next_disposition(&policy, history_before, outcome);

        match disposition {
            ato_retry_policy::RetryDisposition::GiveUpSuccess
            | ato_retry_policy::RetryDisposition::GiveUpPermanent { .. }
            | ato_retry_policy::RetryDisposition::GiveUpExhausted { .. } => {
                break (
                    http_status_code,
                    body_text,
                    elapsed_ms,
                );
            }
            ato_retry_policy::RetryDisposition::RetryAfter { wait, .. } => {
                tokio::time::sleep(wait).await;
                continue;
            }
        }
    };

    let _overall_ms = overall_start.elapsed().as_millis() as i64;
    let retry_count = (attempts.len() as i64) - 1;
    let attempt_summary_json =
        serde_json::to_string(&attempts).ok();

    let http_status_code = match final_http_status {
        Some(s) => s,
        None => {
            // All attempts were transport failures.
            let last_brief = attempts
                .last()
                .and_then(|a| a.error_brief.clone())
                .unwrap_or_else(|| "transport failure".to_string());
            return Ok(ApiDispatchOutcome {
                response: None,
                error_message: Some(last_brief),
                model_used: model,
                duration_ms: final_attempt_duration_ms,
                tokens_in: None,
                tokens_out: None,
                cache_creation_tokens: None,
                cache_read_tokens: None,
                reasoning_tokens: None,
                tool_calls: None,
                retry_count,
                attempt_summary_json,
            });
        }
    };

    if !(200..300).contains(&http_status_code) {
        return Ok(ApiDispatchOutcome {
            response: None,
            error_message: Some(format!(
                "HTTP {}: {}",
                http_status_code,
                truncate_for_audit(&final_body_text, 1000)
            )),
            model_used: model,
            duration_ms: final_attempt_duration_ms,
            tokens_in: None,
            tokens_out: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            reasoning_tokens: None,
            tool_calls: None,
            retry_count,
            attempt_summary_json,
        });
    }
    let payload: serde_json::Value = serde_json::from_str(&final_body_text)
        .map_err(|e| format!("response not valid JSON: {}", e))?;
    let mut outcome = parse_response(provider, payload, model, final_attempt_duration_ms)?;
    outcome.retry_count = retry_count;
    outcome.attempt_summary_json = attempt_summary_json;
    Ok(outcome)
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
                cache_creation_tokens: None,
                cache_read_tokens: None,
                reasoning_tokens: None,
                tool_calls: None,
                retry_count: 0,
                attempt_summary_json: None,
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
        let usage = &payload["usage"];
        // Anthropic billing: input_tokens, cache_creation_input_tokens, and
        // cache_read_input_tokens are SEPARATE billing classes. input_tokens
        // does NOT include the cache classes.
        let tokens_in = usage["input_tokens"].as_i64();
        let tokens_out = usage["output_tokens"].as_i64();
        let cache_creation_tokens = usage["cache_creation_input_tokens"].as_i64();
        let cache_read_tokens = usage["cache_read_input_tokens"].as_i64();
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
            tokens_in,
            tokens_out,
            cache_creation_tokens,
            cache_read_tokens,
            reasoning_tokens: None,
            tool_calls: None,
            retry_count: 0,
            attempt_summary_json: None,
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
                cache_creation_tokens: None,
                cache_read_tokens: None,
                reasoning_tokens: None,
                tool_calls: None,
                retry_count: 0,
                attempt_summary_json: None,
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
            cache_creation_tokens: None,
            cache_read_tokens: None,
            reasoning_tokens: None,
            tool_calls: None,
            retry_count: 0,
            attempt_summary_json: None,
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
                cache_creation_tokens: None,
                cache_read_tokens: None,
                reasoning_tokens: None,
                tool_calls: None,
                retry_count: 0,
                attempt_summary_json: None,
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
                cache_creation_tokens: None,
                cache_read_tokens: None,
                reasoning_tokens: None,
                tool_calls: None,
                retry_count: 0,
                attempt_summary_json: None,
            });
        }
    };
    let usage = &payload["usage"];
    let tokens_in = usage["prompt_tokens"].as_i64();
    let tokens_out = usage["completion_tokens"].as_i64();
    // Reasoning tokens: informational breakdown of completion_tokens.
    // COST NOTE: completion_tokens ALREADY INCLUDES reasoning_tokens —
    // do NOT add to cost. Record for observability only.
    let reasoning_tokens = usage["completion_tokens_details"]["reasoning_tokens"].as_i64();
    Ok(ApiDispatchOutcome {
        response: Some(response),
        error_message: None,
        model_used: model,
        duration_ms,
        tokens_in,
        tokens_out,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        reasoning_tokens,
        tool_calls: None,
        retry_count: 0,
        attempt_summary_json: None,
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
