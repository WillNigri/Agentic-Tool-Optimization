// v2.3.21 Phase 6.x — API-key dispatch for non-CLI providers.
//
// Previously `ato dispatch` only worked for runtimes with a local
// CLI binary (claude, codex, gemini, openclaw, hermes). Users with
// subscriptions to providers that only ship an API (MiniMax, Grok,
// Qwen, DeepSeek, OpenRouter, etc.) had no way to use those
// runtimes as reviewers. This module fills that gap.
//
// Shape:
//   - Provider registry maps a slug (e.g. "minimax") to
//     (base_url, default_model, env_var_name).
//   - Most providers expose an OpenAI-compatible chat-completions
//     endpoint, so one HTTP shape covers them all. MiniMax uses a
//     near-OpenAI shape — `messages` + `model` work, but the URL is
//     `/v1/text/chatcompletion_v2` and the success-check looks at
//     `base_resp.status_code` not just HTTP status.
//   - Key resolution: env var first (e.g. MINIMAX_API_KEY), then
//     llm_api_keys table (base64-decoded) where provider matches.
//     Env var lets ad-hoc / CI flows skip the GUI setup; the table
//     is the UX path.
//   - Persistence: same execution_logs row shape as CLI dispatches.
//     status=success on a real reply, status=error otherwise.

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use rusqlite::Connection;
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::time::Duration;

// v2.3.28 Phase 6.x-E — ApiProvider + registry live in the shared
// `ato-api-providers` crate. Re-exported here so call sites
// (dispatch.rs etc.) keep working without import churn.
pub use ato_api_providers::{find_provider, is_api_provider, registry, ApiProvider};

#[derive(Debug, Serialize)]
pub struct ApiDispatchOutcome {
    pub response: Option<String>,
    pub error_message: Option<String>,
    pub model_used: String,
    pub duration_ms: i64,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
}

/// Resolve an API key for the given provider. Looks at the env var
/// first, then the llm_api_keys table (case-insensitive provider
/// match because the GUI stores "MiniMax", "Grok", etc. but flags
/// come through lowercased).
pub fn resolve_api_key(provider: &ApiProvider, conn: &Connection) -> Result<String> {
    if let Ok(v) = std::env::var(provider.env_var) {
        if !v.trim().is_empty() {
            return Ok(v);
        }
    }
    let row: Option<(String, i32)> = conn
        .query_row(
            "SELECT encrypted_key, is_active FROM llm_api_keys
              WHERE LOWER(provider) = ?1
                AND is_active = 1
              ORDER BY updated_at DESC LIMIT 1",
            [provider.slug],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    let (encrypted, _is_active) = row.ok_or_else(|| {
        anyhow!(
            "No active API key for provider '{}'. Set ${} or add one in ATO → Settings → API Keys.",
            provider.slug,
            provider.env_var,
        )
    })?;
    // The desktop's simple_encrypt is plain base64 — the GUI's banner
    // says so explicitly. Match that decoding here so the CLI doesn't
    // need to call out to the desktop.
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encrypted.as_bytes())
        .context("decode llm_api_keys.encrypted_key (base64)")?;
    String::from_utf8(bytes).context("decoded key is not UTF-8")
}

/// One message in the chat-completions `messages` array. `role` is
/// "user" | "assistant" (we don't use "system" yet). Compatible with
/// every provider in the registry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

pub fn dispatch(
    provider: &ApiProvider,
    prompt: &str,
    model_override: Option<&str>,
    conn: &Connection,
) -> Result<ApiDispatchOutcome> {
    dispatch_with_history(provider, &[], prompt, model_override, conn)
}

/// v2.3.32 Slice A.2 — dispatch with prior conversation history.
/// `history` is the chronological list of past turns; we append the
/// new user prompt and send the whole messages array. Stateless
/// providers like MiniMax need this because they don't maintain
/// session state on their end.
pub fn dispatch_with_history(
    provider: &ApiProvider,
    history: &[Message],
    prompt: &str,
    model_override: Option<&str>,
    conn: &Connection,
) -> Result<ApiDispatchOutcome> {
    let key = resolve_api_key(provider, conn)?;
    let model = match (model_override, provider.default_model) {
        (Some(m), _) if !m.is_empty() => m.to_string(),
        (None, "") => {
            return Err(anyhow!(
                "Provider '{}' has no default model — pass --model explicitly (e.g. --model anthropic/claude-3.5-sonnet).",
                provider.slug
            ));
        }
        (_, default) => default.to_string(),
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("build reqwest client")?;

    // v2.4.2 — Gemini-flavored providers (slug=google) use a
    // structurally different request: model interpolated into the
    // URL path, API key as `?key=` query param, `contents[]` body
    // with role values user/model (not user/assistant), and
    // `generationConfig.maxOutputTokens` instead of `max_tokens`.
    // Branch here rather than at parse-time so a single request
    // builder doesn't have to satisfy two unrelated schemas.
    let (url, body, use_bearer_auth) = if provider.flavor == "gemini" {
        let path = provider.path.replace("{model}", &model);
        let url = format!("{}{}?key={}", provider.base_url, path, urlencode(&key));
        // Translate {role, content} messages → Gemini's {role, parts:[{text}]}.
        // Gemini calls the assistant role "model".
        let mut contents: Vec<serde_json::Value> = history
            .iter()
            .map(|m| {
                let role = if m.role == "assistant" { "model" } else { &m.role };
                serde_json::json!({
                    "role": role,
                    "parts": [{"text": m.content}],
                })
            })
            .collect();
        contents.push(serde_json::json!({
            "role": "user",
            "parts": [{"text": prompt}],
        }));
        let body = serde_json::json!({
            "contents": contents,
            "generationConfig": { "maxOutputTokens": 4096 },
        });
        (url, body, false)
    } else {
        let url = format!("{}{}", provider.base_url, provider.path);
        let mut messages: Vec<serde_json::Value> = history
            .iter()
            .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
            .collect();
        messages.push(serde_json::json!({"role": "user", "content": prompt}));
        let body = serde_json::json!({
            "model": model,
            "messages": messages,
            // 4k cap is generous for a review reply; users can extend by
            // adding a flag later if they need long-form output. Keeps
            // us under cost/latency surprises on most providers.
            "max_tokens": 4096,
        });
        (url, body, true)
    };

    let start = std::time::Instant::now();
    let mut req = client.post(&url).header("Content-Type", "application/json");
    if use_bearer_auth {
        req = req.bearer_auth(&key);
    }
    let resp = req
        .json(&body)
        .send()
        .with_context(|| format!("POST {}", url))?;
    let http_status = resp.status();
    // MiniMax-as-reviewer flagged that unwrap_or_default() here would
    // silently swallow a body-read error after a successful response
    // (e.g. connection reset mid-body) and then surface as "not valid
    // JSON" downstream. Propagate the read error instead so the audit
    // shows the actual root cause.
    let body_text = resp
        .text()
        .with_context(|| format!("read response body from {}", url))?;
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
        });
    }

    let payload: serde_json::Value =
        serde_json::from_str(&body_text).context("response was not valid JSON")?;
    parse_response(provider, payload, model, duration_ms)
}

/// v2.3.47 Phase 6.x-F — streaming dispatch.
///
/// Sets `stream: true` on the request and parses the SSE stream
/// chunk-by-chunk. Each chunk's `choices[0].delta.content` (when
/// present) is forwarded to `on_chunk` so the caller can print
/// tokens to stdout as they arrive. The full assembled response
/// is returned at the end so persistence shape (execution_logs row,
/// session_turns append, events) stays identical to the non-
/// streaming path — no separate code path for the audit log.
///
/// Provider compatibility:
/// - OpenAI shape (Grok, DeepSeek, Qwen, OpenRouter) — works natively.
/// - MiniMax (`flavor = "minimax"`) — also supports `stream=true`
///   with the same `choices[0].delta` shape. We still check
///   `base_resp.status_code` on the final non-streaming-style chunk
///   when MiniMax includes it.
pub fn dispatch_with_history_streaming<F>(
    provider: &ApiProvider,
    history: &[Message],
    prompt: &str,
    model_override: Option<&str>,
    conn: &Connection,
    mut on_chunk: F,
) -> Result<ApiDispatchOutcome>
where
    F: FnMut(&str),
{
    let key = resolve_api_key(provider, conn)?;
    let model = match (model_override, provider.default_model) {
        (Some(m), _) if !m.is_empty() => m.to_string(),
        (None, "") => {
            return Err(anyhow!(
                "Provider '{}' has no default model — pass --model explicitly.",
                provider.slug
            ));
        }
        (_, default) => default.to_string(),
    };

    let url = format!("{}{}", provider.base_url, provider.path);
    let client = reqwest::blocking::Client::builder()
        // Longer timeout for streaming; the response stays open for
        // the duration of generation, which can exceed the 120s cap
        // used for buffered dispatches. 5 min is conservative.
        .timeout(Duration::from_secs(300))
        .build()
        .context("build reqwest client")?;

    let mut messages: Vec<serde_json::Value> = history
        .iter()
        .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
        .collect();
    messages.push(serde_json::json!({"role": "user", "content": prompt}));

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_tokens": 4096,
        "stream": true,
    });

    let start = std::time::Instant::now();
    let resp = client
        .post(&url)
        .bearer_auth(&key)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .with_context(|| format!("POST {} (streaming)", url))?;
    let http_status = resp.status();

    if !http_status.is_success() {
        // Drain the body for the error message so the audit has the
        // full reason, mirroring the buffered path's behavior.
        let body_text = resp.text().unwrap_or_default();
        let duration_ms = start.elapsed().as_millis() as i64;
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
        });
    }

    // Read SSE chunks line-by-line. Reqwest's blocking Response
    // implements io::Read so BufReader::lines() works directly. Each
    // SSE event is a `data: <payload>` line followed by a blank line.
    let reader = BufReader::new(resp);
    let mut full_response = String::new();
    let mut last_usage: Option<serde_json::Value> = None;
    let mut minimax_status: Option<i64> = None;
    let mut stream_error: Option<String> = None;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                stream_error = Some(format!("read SSE stream: {}", e));
                break;
            }
        };
        if line.is_empty() {
            continue;
        }
        let data = match line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:")) {
            Some(d) => d.trim(),
            None => continue, // ignore event:/id:/retry: lines
        };
        if data == "[DONE]" {
            break;
        }
        let payload: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue, // skip malformed chunks rather than abort
        };

        // MiniMax: every chunk carries `base_resp`. status 0 means
        // ok; non-zero means an in-stream failure we should surface.
        if provider.flavor == "minimax" {
            if let Some(code) = payload["base_resp"]["status_code"].as_i64() {
                if code != 0 {
                    minimax_status = Some(code);
                    let msg = payload["base_resp"]["status_msg"]
                        .as_str()
                        .unwrap_or("(no status_msg)")
                        .to_string();
                    stream_error =
                        Some(format!("MiniMax base_resp.status_code={}: {}", code, msg));
                    break;
                }
            }
        }

        // Standard OpenAI shape: choices[0].delta.content per chunk.
        if let Some(delta) =
            payload["choices"][0]["delta"]["content"].as_str().filter(|s| !s.is_empty())
        {
            full_response.push_str(delta);
            on_chunk(delta);
        }
        // Usage normally only appears on the final chunk.
        if payload["usage"].is_object() {
            last_usage = Some(payload["usage"].clone());
        }
    }

    let duration_ms = start.elapsed().as_millis() as i64;

    if let Some(err) = stream_error {
        return Ok(ApiDispatchOutcome {
            response: None,
            error_message: Some(err),
            model_used: model,
            duration_ms,
            tokens_in: None,
            tokens_out: None,
        });
    }
    if minimax_status.is_some() {
        // Already handled above via stream_error; defensive guard for
        // a future code path that might leave it Some without setting
        // the error. Keep the audit trail intact.
        return Ok(ApiDispatchOutcome {
            response: None,
            error_message: Some(format!(
                "MiniMax base_resp.status_code={:?}",
                minimax_status
            )),
            model_used: model,
            duration_ms,
            tokens_in: None,
            tokens_out: None,
        });
    }
    if full_response.is_empty() {
        return Ok(ApiDispatchOutcome {
            response: None,
            error_message: Some("streaming completed without any content".into()),
            model_used: model,
            duration_ms,
            tokens_in: None,
            tokens_out: None,
        });
    }

    let tokens_in = last_usage
        .as_ref()
        .and_then(|u| u["prompt_tokens"].as_i64());
    let tokens_out = last_usage
        .as_ref()
        .and_then(|u| u["completion_tokens"].as_i64());

    Ok(ApiDispatchOutcome {
        response: Some(full_response),
        error_message: None,
        model_used: model,
        duration_ms,
        tokens_in,
        tokens_out,
    })
}

fn parse_response(
    provider: &ApiProvider,
    payload: serde_json::Value,
    model: String,
    duration_ms: i64,
) -> Result<ApiDispatchOutcome> {
    // v2.4.2 — Gemini-flavored response shape is fundamentally
    // different from OpenAI/MiniMax. Branch first; the OpenAI/MiniMax
    // path below assumes choices[].message.content.
    if provider.flavor == "gemini" {
        // Error shape: `{"error": {"code": N, "message": "..."}}` at top.
        if let Some(err) = payload.get("error") {
            let msg = err["message"].as_str().unwrap_or("(no error.message)");
            return Ok(ApiDispatchOutcome {
                response: None,
                error_message: Some(format!("Gemini error: {}", msg)),
                model_used: model,
                duration_ms,
                tokens_in: None,
                tokens_out: None,
            });
        }
        // Success: candidates[0].content.parts is an array of {text:"..."}.
        // We concatenate all text parts so multi-segment replies aren't truncated.
        let text = payload["candidates"][0]["content"]["parts"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .filter(|s| !s.is_empty());
        let response = match text {
            Some(s) => s,
            None => {
                return Ok(ApiDispatchOutcome {
                    response: None,
                    error_message: Some(format!(
                        "no candidates[0].content.parts[].text in Gemini response: {}",
                        truncate_for_audit(&payload.to_string(), 600)
                    )),
                    model_used: model,
                    duration_ms,
                    tokens_in: None,
                    tokens_out: None,
                });
            }
        };
        // usageMetadata = {promptTokenCount, candidatesTokenCount, totalTokenCount}.
        let usage = &payload["usageMetadata"];
        let tokens_in = usage["promptTokenCount"].as_i64();
        let tokens_out = usage["candidatesTokenCount"].as_i64();
        return Ok(ApiDispatchOutcome {
            response: Some(response),
            error_message: None,
            model_used: model,
            duration_ms,
            tokens_in,
            tokens_out,
        });
    }

    // MiniMax flavor: 200 OK but check base_resp.status_code.
    // 0 = success, 2061 = "your token plan doesn't support this model",
    // 2013 = invalid params, etc.
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
            });
        }
    }

    // Standard OpenAI-compatible shape: choices[0].message.content
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
            });
        }
    };

    // usage shape: most providers do {prompt_tokens, completion_tokens,
    // total_tokens}. MiniMax does {total_tokens, total_characters}.
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
    })
}

/// Minimal percent-encoder for the API-key query parameter. Only
/// touches the chars that matter for query-string safety; pulling
/// in a full urlencoding crate for this single use case isn't worth
/// the dep.
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
