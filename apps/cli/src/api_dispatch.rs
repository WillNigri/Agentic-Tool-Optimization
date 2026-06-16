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
    /// Anthropic 5-min prompt-cache WRITE tokens. Billed at 1.25× input
    /// rate. None for non-Anthropic providers and when absent from usage.
    ///
    /// IMPORTANT: Anthropic's `input_tokens` does NOT include this class —
    /// they are separate billing lines. Total billed input:
    ///   input_tokens * rate_in
    ///   + cache_creation_input_tokens * 1.25 * rate_in
    ///   + cache_read_input_tokens * 0.10 * rate_in
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<i64>,
    /// Anthropic 5-min prompt-cache READ tokens. Billed at 0.10× input
    /// rate. None for non-Anthropic providers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,
    /// OpenAI/DeepSeek reasoning tokens (informational only).
    ///
    /// IMPORTANT: OpenAI `completion_tokens` ALREADY INCLUDES reasoning
    /// tokens — `reasoning_tokens` is a breakdown of output, NOT additive.
    /// Do NOT add this to the cost formula. Recorded here for observability
    /// only so the UI can show how much of the output budget was reasoning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i64>,
    /// Tier 2 tool-call audit. None for normal dispatches that don't
    /// involve function-calling; Some(vec) when api_dispatch_tools
    /// produced the outcome — empty vec means "tools were offered
    /// but the model chose not to use them" which is itself signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallAudit>>,
    /// v2.15.1 (war_room 08F8629A) — see desktop mirror's docs.
    #[serde(default)]
    pub retry_count: i64,
    /// v2.15.1 — JSON-serialized AttemptRecord[]; NULL when no retries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_summary_json: Option<String>,
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
                    "Failed to decrypt the stored API key for '{0}'. The ciphertext is intact but \
                     cannot be authenticated under the current master key — the row is an orphan \
                     from a cross-process stale-cache save (the 2026-06-11 pattern, fixed in \
                     f740381 on 2026-06-10 22:30 UTC for forward saves).\n\
                     \n\
                     Three remedies in order of permanence:\n\
                     1. Fast bypass: `export {1}=<your-key>` in your shell — the dispatch path \
                        checks env vars FIRST and never touches the orphan ciphertext.\n\
                     2. Auto-heal where possible: `ato master-key heal-orphans --dry-run` shows \
                        which orphans can be recovered (decrypted under a retired keychain key \
                        that's still present), then re-run without --dry-run to migrate them.\n\
                     3. Manual re-enter: when heal-orphans reports the row is unrecoverable, \
                        open ATO → Settings → API Keys and paste the actual key value (not just \
                        Save — the masked preview hides the value, so Save alone only bumps \
                        updated_at without re-encrypting).",
                    provider.slug, provider.env_var,
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
            // v2.x — raised from 8192 to 16384: gemini-3 thinking models
            // consume the output budget with hidden thought tokens, leaving
            // zero space for visible text. 16384 gives enough headroom.
            "generationConfig": { "maxOutputTokens": 16384 },
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

    // v2.15.1 (war_room 08F8629A) — retry-with-backoff. Blocking
    // mirror of the desktop's async retry loop. Same classifier,
    // same accounting, std::thread::sleep instead of tokio.
    let policy = ato_retry_policy::RetryPolicy::default_v1();
    let mut attempts: Vec<ato_retry_policy::AttemptRecord> = Vec::new();

    let (final_http_status, final_body_text, final_attempt_duration_ms) = loop {
        let attempt_start = std::time::Instant::now();
        let mut req = client.post(&url).header("Content-Type", "application/json");
        if use_bearer_auth {
            req = req.bearer_auth(&key);
        }
        if use_x_api_key {
            req = req
                .header("x-api-key", &key)
                .header("anthropic-version", "2023-06-01");
        }

        let result = req.json(&body).send();
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
                let body = resp.text().unwrap_or_default();
                (Some(s), body, h, None)
            }
            Err(e) => {
                let kind = if e.is_timeout() {
                    format!("timeout after {}ms", elapsed_ms)
                } else if e.is_connect() {
                    "connect failed (DNS / TLS / network)".to_string()
                } else if e.is_request() {
                    "request building or transport error".to_string()
                } else {
                    "transport error".to_string()
                };
                (
                    None,
                    String::new(),
                    std::collections::HashMap::new(),
                    Some(format!("POST {}: {}", url, kind)),
                )
            }
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
                break (http_status_code, body_text, elapsed_ms);
            }
            ato_retry_policy::RetryDisposition::RetryAfter { wait, .. } => {
                std::thread::sleep(wait);
                continue;
            }
        }
    };

    let retry_count = (attempts.len() as i64) - 1;
    let attempt_summary_json = serde_json::to_string(&attempts).ok();

    let http_status_code = match final_http_status {
        Some(s) => s,
        None => {
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

    let payload: serde_json::Value =
        serde_json::from_str(&final_body_text).context("response was not valid JSON")?;
    let mut outcome = parse_response(provider, payload, model, final_attempt_duration_ms)?;
    outcome.retry_count = retry_count;
    outcome.attempt_summary_json = attempt_summary_json;
    Ok(outcome)
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
            cache_creation_tokens: None,
            cache_read_tokens: None,
            reasoning_tokens: None,
            tool_calls: None,
            retry_count: 0,
            attempt_summary_json: None,
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
            cache_creation_tokens: None,
            cache_read_tokens: None,
            reasoning_tokens: None,
            tool_calls: None,
            retry_count: 0,
            attempt_summary_json: None,
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
            cache_creation_tokens: None,
            cache_read_tokens: None,
            reasoning_tokens: None,
            tool_calls: None,
            retry_count: 0,
            attempt_summary_json: None,
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
            cache_creation_tokens: None,
            cache_read_tokens: None,
            reasoning_tokens: None,
            tool_calls: None,
            retry_count: 0,
            attempt_summary_json: None,
        });
    }

    let tokens_in = last_usage
        .as_ref()
        .and_then(|u| u["prompt_tokens"].as_i64());
    let tokens_out = last_usage
        .as_ref()
        .and_then(|u| u["completion_tokens"].as_i64());
    // reasoning_tokens: informational breakdown of completion_tokens.
    // COST NOTE: do NOT add to cost — already included in completion_tokens.
    let reasoning_tokens = last_usage
        .as_ref()
        .and_then(|u| u["completion_tokens_details"]["reasoning_tokens"].as_i64());

    Ok(ApiDispatchOutcome {
        response: Some(full_response),
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

pub(crate) fn parse_response(
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
                cache_creation_tokens: None,
                cache_read_tokens: None,
                reasoning_tokens: None,
                tool_calls: None,
                retry_count: 0,
                attempt_summary_json: None,
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
                // Build an actionable error for gemini-3 / thinking models.
                // Common cause: hidden thought parts consumed all the output
                // budget and left no visible text parts.
                let finish_reason = payload["candidates"][0]["finishReason"]
                    .as_str()
                    .unwrap_or("?");
                let thoughts_tokens = payload["usageMetadata"]["thoughtsTokenCount"]
                    .as_i64();
                let has_thought_parts = payload["candidates"][0]["content"]["parts"]
                    .as_array()
                    .map(|parts| parts.iter().any(|p| p.get("thoughtSignature").is_some()))
                    .unwrap_or(false);

                let thoughts_str = match thoughts_tokens {
                    Some(n) => n.to_string(),
                    None => "?".to_string(),
                };
                let thought_parts_str = if has_thought_parts { "present" } else { "absent" };

                let mut hint = String::new();
                if finish_reason == "MAX_TOKENS" || has_thought_parts {
                    hint = " — thinking likely consumed the output budget; raise maxOutputTokens or lower thinking effort".to_string();
                }

                return Ok(ApiDispatchOutcome {
                    response: None,
                    error_message: Some(format!(
                        "Gemini returned no visible text (finishReason={}, thoughtsTokenCount={}, thought-parts {}){}.  Raw: {}",
                        finish_reason,
                        thoughts_str,
                        thought_parts_str,
                        hint,
                        truncate_for_audit(&payload.to_string(), 300)
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
            cache_creation_tokens: None,
            cache_read_tokens: None,
            reasoning_tokens: None,
            tool_calls: None,
            retry_count: 0,
            attempt_summary_json: None,
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
                cache_creation_tokens: None,
                cache_read_tokens: None,
                reasoning_tokens: None,
                tool_calls: None,
                retry_count: 0,
                attempt_summary_json: None,
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
        // Anthropic billing semantics: input_tokens, cache_creation_input_tokens,
        // and cache_read_input_tokens are SEPARATE billing classes.
        // input_tokens does NOT include the cache classes — total billed input:
        //   input_tokens * rate + cache_creation * 1.25 * rate + cache_read * 0.10 * rate
        let tokens_in = usage["input_tokens"].as_i64();
        let tokens_out = usage["output_tokens"].as_i64();
        let cache_creation_tokens = usage["cache_creation_input_tokens"].as_i64();
        let cache_read_tokens = usage["cache_read_input_tokens"].as_i64();
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
            cache_creation_tokens,
            cache_read_tokens,
            reasoning_tokens: None,
            tool_calls: None,
            retry_count: 0,
            attempt_summary_json: None,
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
                cache_creation_tokens: None,
                cache_read_tokens: None,
                reasoning_tokens: None,
                tool_calls: None,
                retry_count: 0,
                attempt_summary_json: None,
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
                    cache_creation_tokens: None,
                    cache_read_tokens: None,
                    reasoning_tokens: None,
                    tool_calls: None,
                    retry_count: 0,
                    attempt_summary_json: None,
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
                cache_creation_tokens: None,
                cache_read_tokens: None,
                reasoning_tokens: None,
                tool_calls: None,
                retry_count: 0,
                attempt_summary_json: None,
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
    //
    // OpenAI o-series / DeepSeek reasoning: completion_tokens ALREADY
    // INCLUDES reasoning_tokens — they are a breakdown of output, NOT
    // additive. Cost is already correct; reasoning_tokens is recorded
    // for observability only. Never add reasoning_tokens to the cost.
    //
    // DeepSeek-reasoner uses the same OpenAI-compatible usage shape
    // with `completion_tokens_details.reasoning_tokens`. Record-only.
    let usage = &payload["usage"];
    let tokens_in = usage["prompt_tokens"].as_i64();
    let tokens_out = usage["completion_tokens"].as_i64();
    // Reasoning tokens: informational breakdown of completion_tokens.
    // OpenAI: usage.completion_tokens_details.reasoning_tokens
    // DeepSeek: same path (openai-compatible flavor).
    // COST NOTE: do NOT add this to cost — it is already included in
    // completion_tokens. This field is purely for UI observability.
    let reasoning_tokens = usage["completion_tokens_details"]["reasoning_tokens"].as_i64();
    // OpenRouter: some routes carry usage.cost in credits. Record-only
    // when present; we do not attempt to convert credits → USD because
    // the conversion rate is route-specific and may change.
    // TODO(openrouter-cost): if usage["cost"].as_f64() is Some(_),
    //   record it in a future `provider_reported_cost_credits` column.

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

#[cfg(test)]
mod gemini_parse_tests {
    use super::*;
    use ato_api_providers::ApiProvider;
    use serde_json::json;

    fn gemini_provider() -> ApiProvider {
        ApiProvider {
            slug: "test-gemini",
            base_url: "https://example.com",
            path: "/v1beta/models/{model}:generateContent",
            default_model: "gemini-3-test",
            env_var: "TEST_KEY",
            flavor: "gemini",
        }
    }

    /// Task 4a: only a thought part (empty text + thoughtSignature) plus
    /// finishReason "MAX_TOKENS" and usageMetadata.thoughtsTokenCount 9000
    /// must produce an error_message containing "finishReason=MAX_TOKENS"
    /// and "thoughtsTokenCount=9000".
    #[test]
    fn thought_only_part_produces_actionable_error() {
        let payload = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "", "thoughtSignature": "opaque-blob" }
                    ]
                },
                "finishReason": "MAX_TOKENS"
            }],
            "usageMetadata": {
                "promptTokenCount": 1000,
                "candidatesTokenCount": 0,
                "thoughtsTokenCount": 9000,
                "totalTokenCount": 10000
            }
        });
        let outcome = parse_response(
            &gemini_provider(),
            payload,
            "gemini-3-flash".to_string(),
            42,
        )
        .expect("parse_response should return Ok even on error path");

        assert!(
            outcome.error_message.is_some(),
            "expected an error_message for thought-only response"
        );
        let msg = outcome.error_message.unwrap();
        assert!(
            msg.contains("finishReason=MAX_TOKENS"),
            "error must contain finishReason=MAX_TOKENS; got: {msg}"
        );
        assert!(
            msg.contains("thoughtsTokenCount=9000"),
            "error must contain thoughtsTokenCount=9000; got: {msg}"
        );
        assert!(outcome.response.is_none());
    }

    /// Task 4b: mixed parts [thought-only, visible text] must parse
    /// successfully to the visible text, skipping the empty thought part.
    #[test]
    fn mixed_thought_and_text_parts_parses_to_visible_text() {
        let payload = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "", "thoughtSignature": "x" },
                        { "text": "actual answer" }
                    ]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 500,
                "candidatesTokenCount": 20,
                "thoughtsTokenCount": 300,
                "totalTokenCount": 820
            }
        });
        let outcome = parse_response(
            &gemini_provider(),
            payload,
            "gemini-3-flash".to_string(),
            10,
        )
        .expect("parse_response should return Ok");

        assert!(
            outcome.error_message.is_none(),
            "expected no error for mixed-parts response; got: {:?}",
            outcome.error_message
        );
        assert_eq!(
            outcome.response.as_deref(),
            Some("actual answer"),
            "response must be the non-empty text part"
        );
    }
}
