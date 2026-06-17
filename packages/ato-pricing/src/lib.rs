// ato-pricing — shared per-model token pricing + runtime billing-mode
// classification for ATO desktop + CLI.
//
// History: pricing_for_model lived in apps/cli/src/runtime.rs AND
// apps/desktop/src-tauri/src/commands.rs AND (in JS form) apps/desktop/
// src/lib/pricing.ts. Each carried its own copy of the table. The
// drift bit us 2026-05-16 — gemini-2.5-flash was missing from the CLI
// copy for weeks, so every Google API dispatch wrote NULL cost.
//
// This crate is the single Rust source of truth. The JS copy in
// apps/desktop/src/lib/pricing.ts stays as the desktop-frontend
// mirror (different language), but it MUST be kept in sync with this
// table. A future improvement is generating pricing.ts from this
// file at build time.
//
// Scope: pure data structures. No I/O, no HTTP, no database. Suitable
// for embedding in any crate that needs to estimate cost from token
// counts.

use serde::Serialize;

/// Per-million-token (input, output) USD pricing for a model.
/// Returns `None` for any model not in the table. Callers should
/// surface `cost_usd_estimated = NULL` for unknown models rather than
/// faking a "$0" value — the desktop's cost-receipts panel renders
/// NULL as "$? (pricing missing)" specifically so this misses are
/// visible rather than silently free.
///
/// **Rates last verified: 2026-06-12.** Treat as estimates — vendor
/// rates drift. Cross-check against the provider's own dashboard for
/// billing-grade numbers. The `(0.30, 2.50)` Gemini 2.5 Flash entry,
/// for example, came from Google AI Studio pricing as of 2026-05-16.
/// 2026-06-12: added gemini-3 family (flash-preview, flash, 3.5-flash,
/// pro-preview, pro, 3.1-pro-preview, 3.1-pro) — root cause of 7x
/// undercount bug where unknown models returned None → NULL cost.
pub fn pricing_for_model(model: &str) -> Option<(f64, f64)> {
    match model {
        // ---- Anthropic ----
        // opus-4-8 priced at the established Opus tier (same as 4-7/4-6) so the
        // refreshed CLI model picker (#82) stays cost-tracked. Adjust if
        // Anthropic publishes different opus-4-8 rates.
        "claude-opus-4-8" => Some((15.0, 75.0)),
        "claude-opus-4-7" => Some((15.0, 75.0)),
        "claude-opus-4-6" => Some((15.0, 75.0)),
        "claude-sonnet-4-6" => Some((3.0, 15.0)),
        "claude-sonnet-4-5" => Some((3.0, 15.0)),
        "claude-haiku-4-5" | "claude-haiku-4-5-20251001" => Some((1.0, 5.0)),

        // ---- OpenAI ----
        // 2026-05-17 merge: desktop had gpt-4.1 at (2.0, 8.0) and o3 at
        // (2.0, 8.0); CLI had gpt-4.1 at (2.5, 10.0) and o3 at (1.10,
        // 4.40). Resolved to OpenAI's current published rates (gpt-4.1 =
        // $2/$8, o3 = $2/$8 per million as of 2025-Q4).
        "gpt-5" | "gpt-5-2025" => Some((1.25, 10.0)),
        "gpt-4.1" => Some((2.0, 8.0)),
        "gpt-4.1-mini" => Some((0.4, 1.6)),
        "gpt-4.1-nano" => Some((0.1, 0.4)),
        "gpt-4o" => Some((2.5, 10.0)),
        "gpt-4o-mini" => Some((0.15, 0.6)),
        "o3" => Some((2.0, 8.0)),
        "o3-mini" => Some((1.1, 4.4)),

        // ---- Google (Gemini API on AI Studio) ----
        // gemini-3 family — added 2026-06-12 (web-verified). Longer/more-specific
        // names listed first so any future prefix-based matching won't short-circuit.
        // Pro rows priced at the base tier (≤200K context); the >200K tier is
        // $4.00/$18.00 — callers needing that tier must handle it upstream.
        "gemini-3.5-flash"           => Some((1.50,  9.00)),  // launched 2026-05-19
        "gemini-3.1-pro-preview"     => Some((2.00, 12.00)),
        "gemini-3.1-pro"             => Some((2.00, 12.00)),
        "gemini-3-flash-preview"     => Some((0.50,  3.00)),
        "gemini-3-flash"             => Some((0.50,  3.00)),
        "gemini-3-pro-preview"       => Some((2.00, 12.00)),
        "gemini-3-pro"               => Some((2.00, 12.00)),
        // gemini-2.x and older
        "gemini-2.5-pro" => Some((1.25, 10.0)),
        "gemini-2.5-flash" => Some((0.30, 2.50)),
        "gemini-2.5-flash-lite" => Some((0.1, 0.4)),
        "gemini-2.0-flash" => Some((0.1, 0.4)),
        "gemini-2.0-flash-lite" => Some((0.075, 0.3)),
        "gemini-2.0-flash-exp" => Some((0.1, 0.4)),
        "gemini-1.5-pro" => Some((1.25, 5.0)),
        "gemini-1.5-flash" => Some((0.075, 0.3)),

        // ---- xAI Grok ----
        "grok-2-latest" | "grok-2-1212" => Some((2.0, 10.0)),
        "grok-3" => Some((3.0, 15.0)),

        // ---- DeepSeek ----
        "deepseek-chat" => Some((0.27, 1.10)),
        "deepseek-coder" => Some((0.27, 1.10)),
        "deepseek-reasoner" | "deepseek-r1" => Some((0.55, 2.19)),

        // ---- Z.AI (Zhipu GLM) ----
        // Verified 2026-06-17 against docs.z.ai/guides/overview/pricing.
        "glm-5.2" => Some((1.40, 4.40)),
        "glm-4.6" => Some((0.60, 2.20)),
        "glm-4.5" => Some((0.60, 2.20)),
        "glm-4.5-air" => Some((0.20, 1.10)),
        // Currently "limited-time free" per z.ai; (0,0) is accurate today —
        // revisit when the promo ends.
        "glm-4.7-flash" | "glm-4.5-flash" => Some((0.0, 0.0)),

        // ---- Alibaba Qwen (DashScope-Intl) ----
        "qwen-plus" => Some((0.40, 1.20)),
        "qwen-max" => Some((1.40, 5.60)),
        "qwen-turbo" => Some((0.05, 0.20)),

        // ---- MiniMax ----
        // Note: most users hit MiniMax via the Token Plan subscription, in
        // which case there's no metered per-token cost — the dispatch
        // auth mode flags that case. These are the published API rates
        // when on the metered tier.
        "MiniMax-M2.7-highspeed" => Some((1.0, 3.0)),
        "MiniMax-M2" => Some((1.0, 3.0)),
        "MiniMax-Text-01" => Some((0.5, 1.5)),

        _ => None,
    }
}

/// Provider → model tiers, ordered cheapest-output-rate first.
/// Used by the optimizer to enumerate cheaper alternatives.
pub fn models_for_provider(provider: &str) -> &'static [&'static str] {
    match provider {
        "anthropic" => &["claude-haiku-4-5", "claude-sonnet-4-6", "claude-opus-4-6", "claude-opus-4-7", "claude-opus-4-8"],
        "openai" => &["gpt-4.1-nano", "gpt-4o-mini", "gpt-4.1-mini", "o3-mini", "gpt-4.1", "o3", "gpt-4o", "gpt-5"],
        "google" => &[
            "gemini-2.0-flash-lite", "gemini-1.5-flash", "gemini-2.0-flash",
            "gemini-2.5-flash", "gemini-3-flash-preview", "gemini-3-flash",
            "gemini-1.5-pro", "gemini-2.5-pro",
            "gemini-3.5-flash", "gemini-3-pro-preview", "gemini-3-pro",
            "gemini-3.1-pro-preview", "gemini-3.1-pro",
        ],
        "deepseek" => &["deepseek-chat", "deepseek-reasoner"],
        "qwen" => &["qwen-turbo", "qwen-plus", "qwen-max"],
        "minimax" => &["MiniMax-Text-01", "MiniMax-M2", "MiniMax-M2.7-highspeed"],
        "grok" => &["grok-2-latest", "grok-3"],
        "zai" => &["glm-4.5-flash", "glm-4.7-flash", "glm-4.5-air", "glm-4.5", "glm-4.6", "glm-5.2"],
        _ => &[],
    }
}

/// All known providers.
pub fn all_providers() -> &'static [&'static str] {
    &["anthropic", "openai", "google", "deepseek", "qwen", "minimax", "grok", "zai"]
}

/// Model → provider. Returns `None` for unknown models.
pub fn provider_for_model(model: &str) -> Option<&'static str> {
    match model {
        m if m.starts_with("claude-") => Some("anthropic"),
        m if m.starts_with("gpt-") || m.starts_with("o3") => Some("openai"),
        m if m.starts_with("gemini-") => Some("google"),
        m if m.starts_with("deepseek-") => Some("deepseek"),
        m if m.starts_with("qwen-") => Some("qwen"),
        m if m.starts_with("MiniMax-") => Some("minimax"),
        m if m.starts_with("glm-") => Some("zai"),
        m if m.starts_with("grok-") => Some("grok"),
        _ => None,
    }
}

/// Returns models cheaper than `current_model` from the same provider,
/// ordered cheapest first.
pub fn cheaper_same_provider(current_model: &str) -> Vec<&'static str> {
    let provider = match provider_for_model(current_model) {
        Some(p) => p,
        None => return vec![],
    };
    let current_out = match pricing_for_model(current_model) {
        Some((_, out)) => out,
        None => return vec![],
    };
    models_for_provider(provider)
        .iter()
        .copied()
        .filter(|m| {
            *m != current_model
                && pricing_for_model(m)
                    .map(|(_, out)| out < current_out)
                    .unwrap_or(false)
        })
        .collect()
}

/// Returns cheaper models from OTHER providers, ordered cheapest first.
pub fn cheaper_cross_provider(current_model: &str) -> Vec<&'static str> {
    let current_provider = provider_for_model(current_model);
    let current_out = match pricing_for_model(current_model) {
        Some((_, out)) => out,
        None => return vec![],
    };
    let mut alts: Vec<(&str, f64)> = Vec::new();
    for &provider in all_providers() {
        if Some(provider) == current_provider {
            continue;
        }
        for &model in models_for_provider(provider) {
            if let Some((_, out)) = pricing_for_model(model) {
                if out < current_out {
                    alts.push((model, out));
                }
            }
        }
    }
    alts.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    alts.into_iter().map(|(m, _)| m).collect()
}

/// Token-class breakdown for accurate Anthropic cache billing.
///
/// Anthropic's Messages API returns three separate billing classes:
///   - `tokens_in`           — regular (non-cached) input tokens
///   - `cache_creation_in`   — tokens written into the 5-min prompt cache;
///                             billed at 1.25× the input rate
///   - `cache_read_in`       — tokens served from the 5-min cache;
///                             billed at 0.10× the input rate
///
/// Note: `input_tokens` in the Anthropic response does NOT include
/// cache_creation_input_tokens or cache_read_input_tokens — they are
/// separate billing classes. Total billed input =
///   input_tokens * rate_in
///   + cache_creation_input_tokens * 1.25 * rate_in
///   + cache_read_input_tokens * 0.10 * rate_in
///
/// For non-Anthropic providers both cache fields stay `None`.
#[derive(Debug, Clone, Default)]
pub struct TokenClasses {
    pub tokens_in: i64,
    pub tokens_out: i64,
    /// Anthropic 5-min cache WRITE tokens (billed at 1.25× input rate).
    pub cache_creation_in: Option<i64>,
    /// Anthropic 5-min cache READ tokens (billed at 0.10× input rate).
    pub cache_read_in: Option<i64>,
}

/// Compute cost from a `TokenClasses` breakdown.
///
/// Formula (all per-million):
///   cost = tokens_in * rate_in
///        + tokens_out * rate_out
///        + cache_creation_in * 1.25 * rate_in   (5-min cache write)
///        + cache_read_in * 0.10 * rate_in        (5-min cache read)
///
/// Returns `None` when the model is not in the pricing table.
pub fn cost_from_token_classes(model: &str, tc: &TokenClasses) -> Option<f64> {
    let (in_per_m, out_per_m) = pricing_for_model(model)?;
    let base = (tc.tokens_in as f64 / 1_000_000.0) * in_per_m
        + (tc.tokens_out as f64 / 1_000_000.0) * out_per_m;
    let cache_write = tc.cache_creation_in
        .map(|n| (n as f64 / 1_000_000.0) * 1.25 * in_per_m)
        .unwrap_or(0.0);
    let cache_read = tc.cache_read_in
        .map(|n| (n as f64 / 1_000_000.0) * 0.10 * in_per_m)
        .unwrap_or(0.0);
    let cost = base + cache_write + cache_read;
    Some((cost * 1_000_000.0).round() / 1_000_000.0)
}

/// Estimate cost from real token counts (preferred over chars/4
/// when the provider returned a usage block). Returns `None` if the
/// model isn't in the pricing table.
///
/// Delegates to `cost_from_token_classes` with no cache fields.
pub fn cost_from_tokens(model: &str, tokens_in: i64, tokens_out: i64) -> Option<f64> {
    cost_from_token_classes(model, &TokenClasses {
        tokens_in,
        tokens_out,
        cache_creation_in: None,
        cache_read_in: None,
    })
}

/// 4-chars-per-token heuristic for estimating tokens from raw text.
/// Matches the JS pricing.ts:estimateTokens() implementation. Use
/// `cost_from_tokens` whenever the provider's usage block gave us
/// real counts; fall back to this only when token counts are absent.
pub fn estimate_text_tokens(text: &str) -> i64 {
    if text.is_empty() {
        return 0;
    }
    (text.len() as i64 + 3) / 4
}

/// Estimate cost from prompt + response strings using the 4-chars-per-
/// token heuristic. Returns `None` if the model isn't in the pricing
/// table.
pub fn estimate_cost_usd(model: &str, prompt: &str, response: &str) -> Option<f64> {
    let (in_per_m, out_per_m) = pricing_for_model(model)?;
    let in_tokens = estimate_text_tokens(prompt) as f64;
    let out_tokens = estimate_text_tokens(response) as f64;
    let cost = (in_tokens / 1_000_000.0) * in_per_m
        + (out_tokens / 1_000_000.0) * out_per_m;
    Some((cost * 1_000_000.0).round() / 1_000_000.0)
}

/// Runtime billing-mode classification. Distinguishes "this user's auth
/// is a CLI subscription (Claude Max, Codex CLI, Gemini CLI)" from
/// "this is a metered API key" from "this is local."
///
/// Used by the UI cost panel to label rows appropriately — a NULL cost
/// on a Subscription row means "subscription, no per-token billing,"
/// but a NULL cost on an ApiKey row means "we forgot the pricing for
/// this model" (a real bug to surface).
///
/// Per-dispatch authority lives in `execution_logs.auth_mode` when
/// populated; this function is the fallback for older rows where
/// auth_mode is NULL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingMode {
    Subscription,
    ApiKey,
    Local,
}

impl BillingMode {
    /// String form used by the desktop UI's SessionCostRow.billing_mode
    /// field. Keep the values matching the front-end's expectations.
    pub fn as_str(self) -> &'static str {
        match self {
            BillingMode::Subscription => "subscription",
            BillingMode::ApiKey => "api_key",
            BillingMode::Local => "local",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_table_covers_canonical_models() {
        // Smoke test — every model the CLI defaults dispatch to should
        // have an entry. Drift caught at compile time means cost rolls
        // up correctly.
        assert!(pricing_for_model("claude-sonnet-4-6").is_some());
        assert!(pricing_for_model("gemini-2.5-flash").is_some(), "the 2026-05-16 gap");
        assert!(pricing_for_model("MiniMax-M2.7-highspeed").is_some());
        assert!(pricing_for_model("gpt-4.1").is_some());
        // Unknown returns None (surfaces as "$? pricing missing" in UI)
        assert!(pricing_for_model("this-model-does-not-exist").is_none());
    }

    #[test]
    fn cost_from_tokens_arithmetic() {
        // 1,000,000 input tokens @ $3/M = $3.00; 1,000,000 output @ $15/M = $15.00 → $18.00
        let cost = cost_from_tokens("claude-sonnet-4-6", 1_000_000, 1_000_000).unwrap();
        assert!((cost - 18.0).abs() < 1e-9);
    }

    #[test]
    fn cost_from_token_classes_cache_arithmetic() {
        // claude-sonnet-4-6: $3/M in, $15/M out
        // 1M input      → $3.00
        // 1M output     → $15.00
        // 1M cache_write→ 1.25 × $3 = $3.75
        // 1M cache_read → 0.10 × $3 = $0.30
        // total         → $22.05
        let tc = TokenClasses {
            tokens_in: 1_000_000,
            tokens_out: 1_000_000,
            cache_creation_in: Some(1_000_000),
            cache_read_in: Some(1_000_000),
        };
        let cost = cost_from_token_classes("claude-sonnet-4-6", &tc).unwrap();
        assert!(
            (cost - 22.05).abs() < 1e-5,
            "expected $22.05, got ${cost}"
        );
    }

    #[test]
    fn cost_from_token_classes_none_cache_equals_cost_from_tokens() {
        // With no cache fields, the two functions must agree exactly.
        let tc = TokenClasses {
            tokens_in: 500_000,
            tokens_out: 200_000,
            cache_creation_in: None,
            cache_read_in: None,
        };
        let via_classes = cost_from_token_classes("claude-sonnet-4-6", &tc).unwrap();
        let via_tokens  = cost_from_tokens("claude-sonnet-4-6", 500_000, 200_000).unwrap();
        assert!(
            (via_classes - via_tokens).abs() < 1e-12,
            "cache-None should equal cost_from_tokens: {} vs {}",
            via_classes, via_tokens
        );
    }

    #[test]
    fn cost_from_token_classes_unknown_model_returns_none() {
        let tc = TokenClasses {
            tokens_in: 100,
            tokens_out: 50,
            cache_creation_in: Some(10),
            cache_read_in: Some(5),
        };
        assert!(cost_from_token_classes("not-a-real-model", &tc).is_none());
    }

    #[test]
    fn cost_from_tokens_gemini3_flash_preview() {
        // 1,000,000 input @ $0.50/M + 1,000,000 output @ $3.00/M = $3.50
        let cost = cost_from_tokens("gemini-3-flash-preview", 1_000_000, 1_000_000)
            .expect("gemini-3-flash-preview must be in pricing table");
        assert!(
            (cost - 3.5).abs() < 1e-9,
            "expected $3.50, got ${cost}"
        );
    }

    #[test]
    fn billing_mode_classifies() {
        assert_eq!(billing_mode("claude"), BillingMode::Subscription);
        assert_eq!(billing_mode("google"), BillingMode::ApiKey);
        assert_eq!(billing_mode("ollama"), BillingMode::Local);
        assert_eq!(billing_mode("openai"), BillingMode::ApiKey);
        // Future provider not yet in the match arm should default to ApiKey
        assert_eq!(billing_mode("future-vendor"), BillingMode::ApiKey);
    }

    #[test]
    fn model_registry_basics() {
        for &p in all_providers() {
            assert!(!models_for_provider(p).is_empty(), "provider {} has no models", p);
        }
        assert_eq!(provider_for_model("claude-sonnet-4-6"), Some("anthropic"));
        assert_eq!(provider_for_model("gpt-4.1"), Some("openai"));
        assert_eq!(provider_for_model("gemini-2.5-flash"), Some("google"));
        assert_eq!(provider_for_model("unknown-model"), None);
    }

    #[test]
    fn cheaper_same_provider_works() {
        let alts = cheaper_same_provider("claude-opus-4-6");
        assert!(alts.contains(&"claude-sonnet-4-6"));
        assert!(alts.contains(&"claude-haiku-4-5"));
        assert_eq!(alts[0], "claude-haiku-4-5");
        assert!(cheaper_same_provider("claude-haiku-4-5").is_empty());
    }

    #[test]
    fn cheaper_cross_provider_works() {
        let alts = cheaper_cross_provider("claude-opus-4-6");
        assert!(alts.iter().any(|m| m.starts_with("gemini-")));
        assert!(alts.iter().any(|m| m.starts_with("gpt-")));
        assert!(!alts.iter().any(|m| m.starts_with("claude-")));
    }

    /// Parity contract — these are the model prices the dispatch + cost-
    /// receipts paths depend on staying stable. If you intentionally change
    /// a price, update both the table AND this test in the same commit so
    /// the change is loud, not silent.
    ///
    /// Added 2026-05-17 per codex-reviewer feedback on the extraction PR:
    /// "the refactor needs contract tests to prove both consumers resolve
    /// identical prices."
    #[test]
    fn pricing_parity_contract() {
        let cases: &[(&str, f64, f64)] = &[
            // Anthropic
            ("claude-opus-4-7",  15.0, 75.0),
            ("claude-sonnet-4-6", 3.0, 15.0),
            ("claude-haiku-4-5",  1.0,  5.0),
            // OpenAI — these were the conflict points during merge
            ("gpt-5",            1.25, 10.0),
            ("gpt-4.1",          2.0,   8.0),  // desktop's value, replaces CLI's (2.5, 10.0)
            ("o3",               2.0,   8.0),  // desktop's value, replaces CLI's (1.10, 4.40)
            ("gpt-4o-mini",      0.15,  0.6),
            // Google — the 2026-05-16 bug
            ("gemini-2.5-flash", 0.30,  2.50),
            ("gemini-2.5-pro",   1.25, 10.0),
            ("gemini-1.5-flash", 0.075, 0.3),
            // gemini-3 family — added 2026-06-12; was causing 7x undercount (NULL cost)
            ("gemini-3-flash-preview",  0.50,  3.00),
            ("gemini-3-flash",          0.50,  3.00),
            ("gemini-3.5-flash",        1.50,  9.00),
            ("gemini-3-pro-preview",    2.00, 12.00),
            ("gemini-3-pro",            2.00, 12.00),
            ("gemini-3.1-pro-preview",  2.00, 12.00),
            ("gemini-3.1-pro",          2.00, 12.00),
            // MiniMax — CLI had these, desktop didn't
            ("MiniMax-M2.7-highspeed", 1.0, 3.0),
            // DeepSeek
            ("deepseek-chat",    0.27, 1.10),
            // Qwen
            ("qwen-plus",        0.40, 1.20),
            // Grok
            ("grok-3",           3.0, 15.0),
        ];
        for (model, want_in, want_out) in cases {
            let (got_in, got_out) =
                pricing_for_model(model).unwrap_or_else(|| panic!("model {} missing from pricing table", model));
            assert!(
                (got_in - want_in).abs() < 1e-9 && (got_out - want_out).abs() < 1e-9,
                "price drift on {}: got ({}, {}), expected ({}, {})",
                model, got_in, got_out, want_in, want_out
            );
        }
    }
}
