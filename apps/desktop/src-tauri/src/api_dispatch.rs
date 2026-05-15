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
}

pub fn resolve_api_key(provider: &ApiProvider, conn: &Connection) -> Result<String, String> {
    if let Ok(v) = std::env::var(provider.env_var) {
        if !v.trim().is_empty() {
            return Ok(v);
        }
    }
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT id, encrypted_key FROM llm_api_keys
              WHERE LOWER(provider) = ?1
                AND is_active = 1
              ORDER BY updated_at DESC LIMIT 1",
            [provider.slug],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    let (key_id, encrypted) = row.ok_or_else(|| {
        format!(
            "No active API key for provider '{}'. Set ${} or add one in Settings → API Keys.",
            provider.slug, provider.env_var,
        )
    })?;
    // 2026-05-15 fix (codex review finding): the prior code base64-
    // decoded `encrypted_key` directly, which only works for legacy
    // plain-base64 rows. v1: AES-256-GCM rows (the migration-018-era
    // default since the H1 security audit) need to route through the
    // encryption module. Stale path missed in the H1 audit follow-up;
    // the CLI mirror (apps/cli/src/api_dispatch.rs) already routes
    // through encryption::decrypt — desktop was the orphan.
    let key = encryption::decrypt(&encrypted).map_err(|e| e.to_string())?;
    // Touch last_used + usage_count so the API Keys panel's "0 uses"
    // counter increments when the GUI dispatches. Best-effort.
    let now = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "UPDATE llm_api_keys
            SET last_used = ?1, usage_count = usage_count + 1, updated_at = ?1
          WHERE id = ?2",
        rusqlite::params![now, key_id],
    );
    Ok(key)
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
    let url = format!("{}{}", provider.base_url, provider.path);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("build reqwest client: {}", e))?;

    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 4096,
    });

    let start = std::time::Instant::now();
    let mut req = client.post(&url).header("Content-Type", "application/json");
    if provider.flavor == "anthropic" {
        // Anthropic Messages API auth: x-api-key + required version
        // header. Bearer rejected. Body shape is OpenAI-compatible
        // for {messages,max_tokens,model} so we don't need a separate
        // body builder here.
        req = req
            .header("x-api-key", &key)
            .header("anthropic-version", "2023-06-01");
    } else {
        req = req.bearer_auth(&key);
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
    })
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
