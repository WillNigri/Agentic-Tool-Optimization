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

/// Per-million-token (input, output) pricing. Mirror of pricing.ts +
/// commands.rs's pricing_for_model. Keep in sync when adding models.
///
/// v2.3.6 — unused inside the CLI's dispatch / replay paths since the
/// switch to NULL cost for subscription runs. Kept for the future
/// direct-API path (`ato dispatch --api-key`) and for ad-hoc usage
/// like cost-of-equivalent comparisons.
#[allow(dead_code)]
pub fn pricing_for_model(model: &str) -> Option<(f64, f64)> {
    match model {
        // Anthropic
        "claude-opus-4-7" => Some((15.0, 75.0)),
        "claude-opus-4-6" => Some((15.0, 75.0)),
        "claude-sonnet-4-6" => Some((3.0, 15.0)),
        "claude-sonnet-4-5" => Some((3.0, 15.0)),
        "claude-haiku-4-5-20251001" => Some((1.0, 5.0)),
        // OpenAI
        "gpt-4.1" => Some((2.5, 10.0)),
        "gpt-4o" => Some((2.5, 10.0)),
        "gpt-4o-mini" => Some((0.15, 0.6)),
        // Google
        "gemini-2.0-flash" => Some((0.1, 0.4)),
        "gemini-1.5-pro" => Some((1.25, 5.0)),
        "gemini-1.5-flash" => Some((0.075, 0.3)),
        _ => None,
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
///
/// v2.3.6 — unused inside the CLI's dispatch / replay paths (those use
/// runtime-CLI subscriptions). Kept for the future direct-API path.
#[allow(dead_code)]
pub fn estimate_cost_usd(model: &str, prompt: &str, response: &str) -> Option<f64> {
    let (in_per_m, out_per_m) = pricing_for_model(model)?;
    let in_tokens = estimate_text_tokens(prompt) as f64;
    let out_tokens = estimate_text_tokens(response) as f64;
    let cost = (in_tokens / 1_000_000.0) * in_per_m + (out_tokens / 1_000_000.0) * out_per_m;
    Some((cost * 1_000_000.0).round() / 1_000_000.0)
}
