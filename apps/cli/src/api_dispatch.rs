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
    /// Tier 2 tool-call audit. None for normal dispatches that don't
    /// involve function-calling; Some(vec) when api_dispatch_tools
    /// produced the outcome — empty vec means "tools were offered
    /// but the model chose not to use them" which is itself signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallAudit>>,
}

/// One row of the tool-call audit log written into execution_logs.
/// Kept small on purpose so a long review with 10 tool calls stays
/// under a few KB of TEXT.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallAudit {
    pub name: String,
    pub args_brief: String, // truncated args, ~120 chars
    pub is_error: bool,
}

/// v2.15.0 Slice C: thin shim over the shared `ato-llm-key-resolver`
/// crate (war_room 0D398F74 codex finding — CLI + desktop had drifted
/// on usage_count update and error wording). The crate handles
/// env-var precedence + DB row selection; this shim wraps the result
/// with anyhow's context + this crate's encryption::decrypt for the
/// keychain-aware decryption step. usage_count is bumped after a
/// successful decrypt to ATTribute CLI dispatches in the desktop's
/// API Keys panel.
pub fn resolve_api_key(provider: &ApiProvider, conn: &Connection) -> Result<String> {
    let resolved =
        ato_llm_key_resolver::resolve_key_material(provider, conn).map_err(|e| anyhow!("{}", e))?;
    match resolved.source {
        ato_llm_key_resolver::KeySource::Env { .. } => Ok(resolved.material),
        ato_llm_key_resolver::KeySource::Stored { key_id } => {
            let plaintext = crate::encryption::decrypt(&resolved.material).with_context(|| {
                format!(
                    "Failed to decrypt the stored API key for '{}'. The ciphertext is intact but \
                     cannot be authenticated under the current master key — almost always this means \
                     the macOS keychain master_key entry was rotated or refreshed after the key was \
                     saved, orphaning the stored ciphertext (the 2026-05-14 cliff pattern). \
                     \n\nFix: re-enter the {} API key in ATO → Settings → API Keys. The masked preview \
                     hides the value — you must paste the actual key text to trigger re-encryption \
                     (just hitting Save bumps `updated_at` without re-encrypting).\
                     \n\nAlternative: set ${}=<your-key> in the shell to bypass the stored key entirely.",
                    provider.slug, provider.slug, provider.env_var,
                )
            })?;
            let _ = ato_llm_key_resolver::touch_usage_count(conn, &key_id);
            Ok(plaintext)
        }
    }
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

    // v2.7.13 (Will dogfood 2026-05-21) — buffered timeout raised
    // from 120s to 300s to match the streaming path. MiniMax's
    // content-moderation pass on 20K-token code-review prompts
    // routinely takes 60-180s; the 120s cap silently truncated those
    // as "POST <url>" connect-failures in the audit log. 300s still
    // bounds the worst case (a hung api) without falsely failing
    // legitimate slow responses. Streaming uses the same 300s at
    // line ~360 below; keep them in sync.
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .context("build reqwest client")?;

    // v2.4.2 — Gemini-flavored providers (slug=google) use a
    // structurally different request: model interpolated into the
    // URL path, API key as `?key=` query param, `contents[]` body
    // with role values user/model (not user/assistant), and
    // `generationConfig.maxOutputTokens` instead of `max_tokens`.
    // Branch here rather than at parse-time so a single request
    // builder doesn't have to satisfy two unrelated schemas.
    let (url, body, use_bearer_auth, use_x_api_key) = if provider.flavor == "gemini" {
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
            "generationConfig": { "maxOutputTokens": 8192 },
        });
        (url, body, false, false)
    } else if provider.flavor == "anthropic" {
        // Anthropic Messages API. Same {role, content} shape as
        // OpenAI for the message array, but `max_tokens` is required
        // and lives at the top level. Auth via x-api-key header (set
        // below) — bearer rejected.
        let url = format!("{}{}", provider.base_url, provider.path);
        let mut messages: Vec<serde_json::Value> = history
            .iter()
            .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
            .collect();
        messages.push(serde_json::json!({"role": "user", "content": prompt}));
        let body = serde_json::json!({
            "model": model,
            "max_tokens": 8192,
            "messages": messages,
        });
        (url, body, false, true)
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
            // Bumped from 4096 to 8192 (v2.4.4) — the `ato review`
            // command surfaces 30k+-char prompts where a 4096-token
            // reply caps out mid-sentence. 8192 stays under most
            // providers' free-tier cost cliffs.
            "max_tokens": 8192,
        });
        (url, body, true, false)
    };

    let start = std::time::Instant::now();
    let mut req = client.post(&url).header("Content-Type", "application/json");
    if use_bearer_auth {
        req = req.bearer_auth(&key);
    }
    if use_x_api_key {
        // Anthropic auth: x-api-key header + required version pin.
        // The version is stable across SDK releases for a given
        // major; update only if we adopt a new request shape.
        req = req
            .header("x-api-key", &key)
            .header("anthropic-version", "2023-06-01");
    }
    // v2.7.13 (Will dogfood 2026-05-21) — when reqwest.send() fails,
    // classify into timeout / connect / request shape and surface
    // each distinctly. Pre-fix the audit row showed only "POST <url>",
    // which made a 120s timeout look identical to a DNS failure, a
    // TLS error, or a connection reset — see the war-room
    // 76F7CEEB-… reproduction: MiniMax took >120s to process a 20K-
    // token code-review prompt (content-moderation latency on
    // dense code blocks) and the audit just said "POST <url>".
    // duration_ms is also recorded on error now so the operator can
    // tell "instant fail" from "timed out at 120s" at a glance.
    let resp = match req.json(&body).send() {
        Ok(r) => r,
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as i64;
            let kind = if e.is_timeout() {
                format!("timeout after {}ms (client cap is 120s)", elapsed)
            } else if e.is_connect() {
                "connect failed (DNS / TLS / network)".to_string()
            } else if e.is_request() {
                "request building or transport error".to_string()
            } else {
                "transport error".to_string()
            };
            return Err(anyhow::Error::new(e)).with_context(|| {
                format!("POST {}: {}", url, kind)
            });
        }
    };
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
            tool_calls: None,
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
    // Anthropic uses a different SSE event format than the OpenAI
    // text/event-stream shape this function parses. Rather than ship
    // a broken streaming path, fall back to the buffered dispatch
    // and emit the full reply as a single chunk so callers that
    // pipe on_chunk to stdout still see the output. Real streaming
    // for anthropic is a separate follow-up.
    if provider.flavor == "anthropic" {
        let outcome = dispatch_with_history(provider, history, prompt, model_override, conn)?;
        if let Some(text) = outcome.response.as_deref() {
            on_chunk(text);
        }
        return Ok(outcome);
    }
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
            tool_calls: None,
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
            tool_calls: None,
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
            tool_calls: None,
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
            tool_calls: None,
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
        tool_calls: None,
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
            tool_calls: None,
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
            tool_calls: None,
                });
            }
        };
        // v2.7.15 — usageMetadata shape (Google billing 2026):
        //   { promptTokenCount, candidatesTokenCount, thoughtsTokenCount,
        //     cachedContentTokenCount, totalTokenCount }
        //
        // PRE-FIX BUG (Will dogfood 2026-05-22): we only summed
        // promptTokenCount + candidatesTokenCount. Gemini 2.5 Flash
        // (and the 2.5 family generally) has THINKING ENABLED BY
        // DEFAULT; the model produces hidden "thoughts" before its
        // visible answer. Google bills `thoughtsTokenCount` at the
        // SAME RATE as `candidatesTokenCount` ($2.50/M for 2.5 Flash).
        // Ignoring it meant ATO's recorded output tokens were 30-50%
        // LOWER than what Google billed — Will's R$15.97 actual vs
        // our $1.40 recorded ≈ ~56% undercount.
        //
        // Fix: add thoughts to output. Cached input is billed
        // separately at a discount, but for cost-comparison
        // purposes we count cached input as input (it's still
        // "what the model received" semantically).
        // v2.7.15 — parse_gemini_usage handles the thoughtsTokenCount
        // accounting. See its docstring + test module below.
        let (tokens_in, tokens_out) = parse_gemini_usage(&payload["usageMetadata"]);
        return Ok(ApiDispatchOutcome {
            response: Some(response),
            error_message: None,
            model_used: model,
            duration_ms,
            tokens_in,
            tokens_out,
            tool_calls: None,
        });
    }

    // Anthropic Messages API: success is a JSON object with
    // `content[]` of typed blocks. We consume only `type:"text"`
    // blocks today (tool-use / image blocks land in follow-up).
    // Errors come back with `type:"error"` + `error.message`.
    if provider.flavor == "anthropic" {
        if payload["type"].as_str() == Some("error") {
            let msg = payload["error"]["message"]
                .as_str()
                .unwrap_or("(no error.message)");
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
        // Content is a typed-block array. Concatenate every text
        // block; legitimate empty strings pass through (refusals,
        // tool-only responses, stop_reason=max_tokens with no text).
        // Don't filter — that misreports legitimate stops as parse
        // errors. (claude #4)
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
        let tokens_in = usage["input_tokens"].as_i64();
        let tokens_out = usage["output_tokens"].as_i64();
        // Surface non-end_turn stop reasons as informational so the
        // operator can tell a truncation/tool-use from a normal
        // completion. (minimax #3) max_tokens specifically is the
        // "your output got cut off" signal callers like `ato review`
        // need to act on.
        let stop_reason = payload["stop_reason"].as_str().unwrap_or("");
        let mut error_message = None;
        if stop_reason == "max_tokens" {
            error_message = Some(
                "Anthropic warning: stop_reason=max_tokens — response truncated. Increase max_tokens or split the prompt.".to_string(),
            );
        }
        return Ok(ApiDispatchOutcome {
            response: Some(text),
            error_message,
            model_used: model,
            duration_ms,
            tokens_in,
            tokens_out,
            tool_calls: None,
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
            tool_calls: None,
            });
        }
    }

    // Standard OpenAI-compatible shape: choices[0].message.content
    let choices = payload["choices"].as_array();
    let first_choice = choices.and_then(|arr| arr.first());
    let content = first_choice
        .and_then(|c| c["message"]["content"].as_str())
        .map(|s| s.to_string());
    let finish_reason = first_choice
        .and_then(|c| c["finish_reason"].as_str())
        .unwrap_or("");
    let response = match content {
        Some(s) if !s.is_empty() => s,
        _ => {
            // v2.4.4 — MiniMax has a separate `reasoning_content` for
            // chain-of-thought; when the model burns its budget on
            // reasoning and never emits a final reply (finish_reason
            // == "length"), `content` is empty but `reasoning_content`
            // holds the partial work. Fall back to that so callers
            // get SOMETHING instead of an opaque error. Mark the
            // response with a marker so audit can tell.
            let reasoning = first_choice
                .and_then(|c| c["message"]["reasoning_content"].as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());
            if let Some(r) = reasoning {
                return Ok(ApiDispatchOutcome {
                    response: Some(format!(
                        "[truncated by max_tokens — showing reasoning_content fallback]\n\n{}",
                        r
                    )),
                    error_message: Some(format!(
                        "finish_reason={}; the model used its output budget on reasoning and didn't emit a final reply. Consider --max-tokens override (when implemented) or shorter prompt.",
                        finish_reason
                    )),
                    model_used: model,
                    duration_ms,
                    tokens_in: None,
                    tokens_out: None,
            tool_calls: None,
                });
            }
            return Ok(ApiDispatchOutcome {
                response: None,
                error_message: Some(format!(
                    "no choices[0].message.content (finish_reason={}): {}",
                    finish_reason,
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

    // v2.4.4 — Even on success, surface a warning when the model
    // hit its output cap. Callers (especially `ato review`) need to
    // know the reply is incomplete.
    if finish_reason == "length" {
        eprintln!(
            "ato dispatch: warning — provider truncated response at max_tokens (finish_reason=length). Reply may be incomplete."
        );
    }

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
        tool_calls: None,
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

/// Extract (input, output) token counts from Gemini's
/// `usageMetadata` block. Returns `(tokens_in, tokens_out)` as
/// `(Option<i64>, Option<i64>)` so callers can distinguish
/// "unmeasured" (None — usageMetadata was malformed or absent) from
/// "$0 free dispatch" (Some(0)).
///
/// Output handling: Gemini 2.5 has thinking enabled by default;
/// Google bills `thoughtsTokenCount` at the SAME RATE as
/// `candidatesTokenCount` ($2.50/M for 2.5 Flash). Pre-v2.7.15 we
/// only counted candidates, undercounting real billing by 30-50%
/// (Will dogfood 2026-05-22). `tokens_out` is anchored on
/// `candidatesTokenCount` being present (otherwise None) so a
/// malformed response doesn't get logged as a successful $0 run —
/// claude war-room C37BD156 round 1 #B regression catch.
///
/// Note: `cachedContentTokenCount` is a BREAKDOWN of
/// `promptTokenCount` per Google's docs (the cached portion is
/// counted within prompt, just billed at a discount). We don't
/// add it again here — that would double-count.
pub(crate) fn parse_gemini_usage(
    usage: &serde_json::Value,
) -> (Option<i64>, Option<i64>) {
    let tokens_in = usage["promptTokenCount"].as_i64();
    let tokens_out = usage["candidatesTokenCount"].as_i64().map(|cand| {
        cand + usage["thoughtsTokenCount"].as_i64().unwrap_or(0)
    });
    (tokens_in, tokens_out)
}

#[cfg(test)]
mod gemini_usage_tests {
    use super::parse_gemini_usage;
    use serde_json::json;

    // Pin all three cost-tracking-correctness invariants for the
    // Gemini usageMetadata extraction. The bug Will caught on
    // 2026-05-22 cost ATO ~56% of its tracked Google spend; these
    // tests stop a regression from costing it again.

    #[test]
    fn sums_candidates_plus_thoughts_for_25_thinking_models() {
        // Gemini 2.5 Flash response: 150 thoughts + 200 visible output.
        let usage = json!({
            "promptTokenCount": 1000,
            "candidatesTokenCount": 200,
            "thoughtsTokenCount": 150,
            "totalTokenCount": 1350,
        });
        let (tin, tout) = parse_gemini_usage(&usage);
        assert_eq!(tin, Some(1000));
        assert_eq!(tout, Some(350), "must sum candidates + thoughts");
    }

    #[test]
    fn returns_candidates_only_when_thoughts_field_absent() {
        // Gemini 2.0 Flash (pre-thinking) — no thoughtsTokenCount key.
        let usage = json!({
            "promptTokenCount": 500,
            "candidatesTokenCount": 100,
            "totalTokenCount": 600,
        });
        let (tin, tout) = parse_gemini_usage(&usage);
        assert_eq!(tin, Some(500));
        assert_eq!(tout, Some(100), "no thoughts field → just candidates");
    }

    #[test]
    fn returns_none_when_usagemetadata_is_absent_or_malformed() {
        // Pre-AMEND regression case (claude C37BD156 #B): empty /
        // malformed usageMetadata MUST yield None, not Some(0). The
        // db.rs cost-aggregator treats None as "unmeasured" and
        // Some(0) as "free successful run" — conflating them was
        // the regression the war-room caught.
        let absent = json!({});
        let (tin, tout) = parse_gemini_usage(&absent);
        assert_eq!(tin, None, "no promptTokenCount → unmeasured");
        assert_eq!(tout, None, "no candidatesTokenCount → unmeasured");

        let null_meta = json!(null);
        let (tin, tout) = parse_gemini_usage(&null_meta);
        assert_eq!(tin, None);
        assert_eq!(tout, None);
    }

    #[test]
    fn does_not_double_count_cached_input() {
        // cachedContentTokenCount is a BREAKDOWN of promptTokenCount
        // per Google's docs — the cached portion is INSIDE prompt,
        // billed at a discount. We must NOT add it to tokens_in or
        // we'd over-count input on every cached dispatch.
        let usage = json!({
            "promptTokenCount": 1000,         // total input
            "cachedContentTokenCount": 800,   // 800 of those were cache hits
            "candidatesTokenCount": 50,
            "totalTokenCount": 1050,
        });
        let (tin, _) = parse_gemini_usage(&usage);
        assert_eq!(tin, Some(1000), "tokens_in is the full prompt — cache breakdown stays out");
    }
}
