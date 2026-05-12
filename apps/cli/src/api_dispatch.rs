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
use std::time::Duration;

/// Static registry. Adding a new OpenAI-compatible provider is one
/// entry here + nothing else. MiniMax has its own response shape so
/// it has a custom path; others use the shared OpenAI shape.
#[derive(Debug, Clone)]
pub struct ApiProvider {
    pub slug: &'static str,
    pub base_url: &'static str,
    pub path: &'static str,
    pub default_model: &'static str,
    pub env_var: &'static str,
    /// "openai" = standard chat-completions shape. "minimax" = custom
    /// success check via base_resp.status_code.
    pub flavor: &'static str,
}

pub fn registry() -> &'static [ApiProvider] {
    &[
        ApiProvider {
            slug: "minimax",
            base_url: "https://api.minimax.io",
            path: "/v1/text/chatcompletion_v2",
            // MiniMax-M2.7-highspeed is the Plus Token Plan default;
            // users on other tiers can override with --model. The
            // older MiniMax-M2 and MiniMax-Text-01 are gated on the
            // metered API, not the subscription.
            default_model: "MiniMax-M2.7-highspeed",
            env_var: "MINIMAX_API_KEY",
            flavor: "minimax",
        },
        ApiProvider {
            slug: "grok",
            base_url: "https://api.x.ai",
            path: "/v1/chat/completions",
            default_model: "grok-2-latest",
            env_var: "GROK_API_KEY",
            flavor: "openai",
        },
        ApiProvider {
            slug: "deepseek",
            base_url: "https://api.deepseek.com",
            path: "/v1/chat/completions",
            default_model: "deepseek-chat",
            env_var: "DEEPSEEK_API_KEY",
            flavor: "openai",
        },
        ApiProvider {
            slug: "qwen",
            base_url: "https://dashscope-intl.aliyuncs.com",
            path: "/compatible-mode/v1/chat/completions",
            default_model: "qwen-plus",
            env_var: "DASHSCOPE_API_KEY",
            flavor: "openai",
        },
        ApiProvider {
            slug: "openrouter",
            base_url: "https://openrouter.ai",
            path: "/api/v1/chat/completions",
            // OpenRouter is a meta-provider — no "default" makes
            // sense, so we require --model. Empty string flags that.
            default_model: "",
            env_var: "OPENROUTER_API_KEY",
            flavor: "openai",
        },
    ]
}

pub fn find_provider(slug: &str) -> Option<&'static ApiProvider> {
    let lower = slug.to_ascii_lowercase();
    registry().iter().find(|p| p.slug == lower.as_str())
}

pub fn is_api_provider(slug: &str) -> bool {
    find_provider(slug).is_some()
}

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

pub fn dispatch(
    provider: &ApiProvider,
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

    let url = format!("{}{}", provider.base_url, provider.path);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("build reqwest client")?;

    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        // 4k cap is generous for a review reply; users can extend by
        // adding a flag later if they need long-form output. Keeps
        // us under cost/latency surprises on most providers.
        "max_tokens": 4096,
    });

    let start = std::time::Instant::now();
    let resp = client
        .post(&url)
        .bearer_auth(&key)
        .header("Content-Type", "application/json")
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

fn parse_response(
    provider: &ApiProvider,
    payload: serde_json::Value,
    model: String,
    duration_ms: i64,
) -> Result<ApiDispatchOutcome> {
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

fn truncate_for_audit(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}
