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

// Pricing primitives live in `packages/ato-pricing` (extracted 2026-05-17).
// Full re-export preserves the historical `runtime::pricing_for_model`
// / `runtime::billing_mode` surface for any caller that imports through
// this module — silent breakage on a refactor would be worse than a
// few unused imports.
pub use ato_pricing::{
    billing_mode, cost_from_tokens, estimate_cost_usd, estimate_text_tokens,
    pricing_for_model, BillingMode,
};

/// CLI-runtime default model. Distinct from `pricing_for_model` — the
/// latter is keyed by provider model id (e.g. "claude-sonnet-4-6"); this
/// is keyed by runtime slug (e.g. "claude"). Lives in runtime.rs (not
/// the shared pricing crate) because the CLI runtime list is specific
/// to the dispatch CLI, not a generic pricing concern.
pub fn default_model_for_runtime(runtime: &str) -> Option<&'static str> {
    match runtime {
        "claude" => Some("claude-sonnet-4-6"),
        "codex" => Some("gpt-4.1"),
        "gemini" => Some("gemini-2.0-flash"),
        _ => None,
    }
}
