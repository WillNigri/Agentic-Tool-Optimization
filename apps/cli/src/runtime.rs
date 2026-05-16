// Runtime CLI resolution + pricing helpers.
//
// Mirrors the equivalent code in apps/desktop/src-tauri/src/commands.rs
// (pricing_for_model, default_model_for_runtime, estimate_text_tokens).
// Keeping them duplicated for Phase 1 — the right long-term shape is a
// shared `crates/ato-core` lib both desktop and CLI depend on, but
// premature extraction would slow Phase 1.

use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// Find the CLI binary for a runtime on the user's PATH. Mirrors
/// `which_claude` / `which_cli` from the desktop crate.
pub fn resolve_runtime_cli(runtime: &str) -> Result<PathBuf> {
    let cand = match runtime {
        "claude" => "claude",
        "codex" => "codex",
        "gemini" => "gemini",
        "openclaw" => "openclaw",
        "hermes" => "hermes",
        _ => {
            return Err(anyhow!(
                "Unknown runtime '{}'. Supported: claude, codex, gemini, openclaw, hermes.",
                runtime
            ))
        }
    };
    which::which(cand).map_err(|_| {
        anyhow!(
            "Runtime CLI for '{}' not found on PATH. Install it first (Claude Code, Codex, Gemini CLI, etc.).",
            runtime
        )
    })
}

/// Per-million-token (input, output) USD pricing.
///
/// **Rates last verified: 2026-05-16.** Treat as estimates — published
/// vendor rates drift. For accurate billing always cross-check against
/// the provider's own dashboard. The `(0.30, 2.50)` Gemini 2.5 Flash
/// numbers, for example, came from Google AI Studio pricing as of that
/// date; verify before quoting publicly.
///
/// Returning `None` for an unknown model is signal — the dispatch path
/// will write `cost_usd_estimated = NULL` and the UI surfaces "$? (model
/// not in pricing table)" rather than a misleading "$0.00".
pub fn pricing_for_model(model: &str) -> Option<(f64, f64)> {
    match model {
        // ---- Anthropic ----
        "claude-opus-4-7" => Some((15.0, 75.0)),
        "claude-opus-4-6" => Some((15.0, 75.0)),
        "claude-sonnet-4-6" => Some((3.0, 15.0)),
        "claude-sonnet-4-5" => Some((3.0, 15.0)),
        "claude-haiku-4-5" | "claude-haiku-4-5-20251001" => Some((1.0, 5.0)),

        // ---- OpenAI ----
        "gpt-5" | "gpt-5-2025" => Some((1.25, 10.0)),
        "gpt-4.1" => Some((2.5, 10.0)),
        "gpt-4o" => Some((2.5, 10.0)),
        "gpt-4o-mini" => Some((0.15, 0.6)),
        "o3" => Some((1.10, 4.40)),
        "o3-mini" => Some((1.10, 4.40)),

        // ---- Google (Gemini API on AI Studio) ----
        "gemini-2.5-pro" => Some((1.25, 10.0)),
        "gemini-2.5-flash" => Some((0.30, 2.50)),
        "gemini-2.0-flash" => Some((0.1, 0.4)),
        "gemini-1.5-pro" => Some((1.25, 5.0)),
        "gemini-1.5-flash" => Some((0.075, 0.3)),

        // ---- xAI Grok ----
        "grok-2-latest" | "grok-2-1212" => Some((2.0, 10.0)),
        "grok-3" => Some((3.0, 15.0)),

        // ---- DeepSeek ----
        "deepseek-chat" => Some((0.27, 1.10)),
        "deepseek-coder" => Some((0.27, 1.10)),
        "deepseek-reasoner" | "deepseek-r1" => Some((0.55, 2.19)),

        // ---- Alibaba Qwen (DashScope-Intl) ----
        "qwen-plus" => Some((0.40, 1.20)),
        "qwen-max" => Some((1.40, 5.60)),
        "qwen-turbo" => Some((0.05, 0.20)),

        // ---- MiniMax ----
        // Note: most users hit MiniMax via the Token Plan subscription, in
        // which case there's no metered per-token cost — the dispatch
        // auth mode flags that case. These are the published API rates if
        // a user is on the metered tier.
        "MiniMax-M2.7-highspeed" => Some((1.0, 3.0)),
        "MiniMax-M2" => Some((1.0, 3.0)),
        "MiniMax-Text-01" => Some((0.5, 1.5)),

        _ => None,
    }
}

/// Runtime billing-mode classification. Distinguishes "this user's auth
/// is a CLI subscription (Claude Max, Codex CLI, Gemini CLI)" from "this
/// is a metered API key" from "this is local."
///
/// Used by the UI cost panel to label rows appropriately — a NULL cost
/// on a Subscription row means "subscription, no per-token billing,"
/// but a NULL cost on an ApiKey row means "we forgot the pricing for
/// this model" (a real bug to surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingMode {
    Subscription,
    ApiKey,
    Local,
}

pub fn billing_mode(runtime: &str) -> BillingMode {
    match runtime {
        // CLI runtimes that use the user's existing subscription auth.
        "claude" | "codex" | "gemini" => BillingMode::Subscription,
        // Local runtimes — no network, no cost.
        "ollama" | "openclaw" | "hermes" => BillingMode::Local,
        // Anything else is an API key path (anthropic, openai, google,
        // minimax, grok, deepseek, qwen, openrouter, together, groq…).
        _ => BillingMode::ApiKey,
    }
}

pub fn default_model_for_runtime(runtime: &str) -> Option<&'static str> {
    match runtime {
        "claude" => Some("claude-sonnet-4-6"),
        "codex" => Some("gpt-4.1"),
        "gemini" => Some("gemini-2.0-flash"),
        _ => None,
    }
}

/// 4-chars-per-token heuristic, matches pricing.ts estimateTokens().
pub fn estimate_text_tokens(text: &str) -> i64 {
    if text.is_empty() {
        return 0;
    }
    (text.len() as i64 + 3) / 4
}

/// Estimate cost. Returns None when the model isn't in our pricing table.
pub fn estimate_cost_usd(model: &str, prompt: &str, response: &str) -> Option<f64> {
    let (in_per_m, out_per_m) = pricing_for_model(model)?;
    let in_tokens = estimate_text_tokens(prompt) as f64;
    let out_tokens = estimate_text_tokens(response) as f64;
    let cost = (in_tokens / 1_000_000.0) * in_per_m + (out_tokens / 1_000_000.0) * out_per_m;
    Some((cost * 1_000_000.0).round() / 1_000_000.0)
}

/// Cost from real token counts (when the provider returned a usage
/// block). Prefer this over `estimate_cost_usd` for api-provider
/// dispatches since the actual numbers are billable, not the
/// chars/4 heuristic.
pub fn cost_from_tokens(model: &str, tokens_in: i64, tokens_out: i64) -> Option<f64> {
    let (in_per_m, out_per_m) = pricing_for_model(model)?;
    let cost = (tokens_in as f64 / 1_000_000.0) * in_per_m
        + (tokens_out as f64 / 1_000_000.0) * out_per_m;
    Some((cost * 1_000_000.0).round() / 1_000_000.0)
}
