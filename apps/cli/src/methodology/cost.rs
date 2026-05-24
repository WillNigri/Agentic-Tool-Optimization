// v2.10.0 PR-1 — Dual cost accounting + pre-run cost estimation.
//
// The transparency story: every methodology run shows TWO columns — what
// the customer is about to spend (their LLM invoice) AND what we're
// about to spend (provider compute + judge + storage). Both numbers come
// from the same open-source rate card at `packages/ato-pricing/pricing.json`
// so customers can audit our margin math.
//
// Empirical calibration from the n=150 v2.9 Part 5 eval:
//
//   - Cold dispatches: ~169 tokens out average (claude refusing / replying
//     from priors). Cost ~$0.0046/dispatch.
//   - Grounded (soft/strict) dispatches: ~4,100-4,400 tokens out average
//     (claude reading files via tools + reasoning over them). Cost
//     ~$0.062/dispatch.
//   - **The ~25× multiplier between cold and grounded is the load-bearing
//     calibration constant** — pre-run cost estimates MUST use grounded
//     assumptions when grounding is on, or customers see 25× bill
//     surprises.
//
// The cost estimate is conservative-leaning: better to over-estimate by
// 20% and have the customer be pleasantly surprised than under-estimate
// and trigger a "you said it would cost $5 but charged $40" support
// ticket.

use serde::{Deserialize, Serialize};

use super::types::{BillingMode, VariantMatrix};

/// The cost-rate card. Loaded from `packages/ato-pricing/pricing.json`
/// at runtime; this struct mirrors the JSON `rates` block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRateCard {
    /// LLM judge default cost per scoring call (USD).
    /// claude-haiku-4-5 baseline ~$0.001/call.
    pub llm_judge_cost_per_call_usd: f64,

    /// Railway shared CPU compute rate (USD per second).
    pub compute_per_second_usd: f64,

    /// Object-storage (R2/S3) rate (USD per byte per month).
    pub storage_per_byte_month_usd: f64,

    /// Egress / CDN bandwidth rate (USD per byte transferred).
    pub bandwidth_per_byte_usd: f64,
}

impl CostRateCard {
    /// Hard-coded defaults matching pricing.json @ 2026-05-24. The
    /// authoritative source is the JSON file; this struct is the
    /// fallback when the file is unreachable. Both must stay in sync.
    pub fn defaults_v1() -> Self {
        Self {
            llm_judge_cost_per_call_usd: 0.001,
            compute_per_second_usd: 0.000005,
            storage_per_byte_month_usd: 0.000000023,
            bandwidth_per_byte_usd: 0.00000009,
        }
    }
}

/// Per-LLM-provider token cost expectations (USD per 1M input + per 1M
/// output). Used to estimate the customer-side spend before fan-out.
/// PR-1 ships a small hardcoded table; PR-2 reads from
/// `packages/ato-api-providers` so it stays in sync with the rest of
/// ATO's pricing knowledge.
#[derive(Debug, Clone, Copy)]
pub struct ModelTokenPrice {
    pub usd_per_million_in: f64,
    pub usd_per_million_out: f64,
}

impl ModelTokenPrice {
    /// Lookup the per-million pricing for a model slug. Returns a
    /// conservative default ($5/M in, $20/M out — roughly mid-tier
    /// claude) for unknown slugs so the estimate never undercounts.
    pub fn for_model(slug: &str) -> Self {
        match slug {
            // Claude Anthropic
            "claude-haiku-4-5" => Self { usd_per_million_in: 0.25, usd_per_million_out: 1.25 },
            "claude-sonnet-4-6" => Self { usd_per_million_in: 3.00, usd_per_million_out: 15.00 },
            "claude-opus-4-7" => Self { usd_per_million_in: 15.00, usd_per_million_out: 75.00 },
            // Gemini Google
            s if s.starts_with("gemini-2.0-flash") => Self { usd_per_million_in: 0.075, usd_per_million_out: 0.30 },
            s if s.starts_with("gemini-2.5-flash") => Self { usd_per_million_in: 0.15, usd_per_million_out: 0.60 },
            s if s.starts_with("gemini-2.5-pro") => Self { usd_per_million_in: 1.25, usd_per_million_out: 5.00 },
            // OpenAI
            "openai/gpt-4o" | "gpt-4o" => Self { usd_per_million_in: 2.50, usd_per_million_out: 10.00 },
            "openai/gpt-4.1" | "gpt-4.1" | "codex/gpt-4.1" => {
                Self { usd_per_million_in: 5.00, usd_per_million_out: 15.00 }
            }
            // Conservative fallback
            _ => Self { usd_per_million_in: 5.00, usd_per_million_out: 20.00 },
        }
    }
}

/// Token-count expectations per dispatch, calibrated against the v2.9 PR-1
/// Part 5 n=150 empirical data. These are the assumptions the pre-run
/// estimate uses; the actual run records real numbers and the receipt
/// shows the delta.
#[derive(Debug, Clone, Copy)]
struct DispatchTokenAssumption {
    tokens_in: f64,
    tokens_out: f64,
}

impl DispatchTokenAssumption {
    /// Per-condition expectations from the Part 5 data (n=50/condition):
    ///
    /// | condition | tokens_in avg | tokens_out avg | $/dispatch on claude |
    /// |-----------|---------------|----------------|----------------------|
    /// | cold      | ~19           | ~169           | $0.0046              |
    /// | soft      | ~19           | ~4,117         | $0.0618              |
    /// | strict    | ~18           | ~4,372         | $0.0656              |
    ///
    /// Cold mode = no grounding flags = pure-text reply. Grounded modes
    /// pull file contents through tool calls, inflating tokens_out 25×.
    /// We use the soft-mode assumption as the conservative grounded
    /// estimate (slightly under strict).
    fn for_condition(condition: &str) -> Self {
        match condition {
            "cold" | "off" => Self { tokens_in: 100.0, tokens_out: 250.0 },
            "soft" => Self { tokens_in: 100.0, tokens_out: 4100.0 },
            "strict" => Self { tokens_in: 100.0, tokens_out: 4400.0 },
            // Unknown condition — assume grounded for safety (over-estimate)
            _ => Self { tokens_in: 100.0, tokens_out: 4100.0 },
        }
    }
}

/// Per-provider cost decomposition (what WE pay) for a methodology run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderCostBreakdown {
    pub llm_cost_usd: f64,
    pub judge_cost_usd: f64,
    pub compute_cost_usd: f64,
    pub storage_cost_usd: f64,
    pub bandwidth_cost_usd: f64,
    pub total_usd: f64,
}

/// Customer-facing cost estimate. Generated before fan-out, surfaced in
/// the GUI / CLI confirmation prompt. The `customer_*` fields = what
/// they're about to spend on their own API keys. The `provider_*` fields
/// = what ATO is about to spend to deliver this run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    pub total_dispatches: u32,
    /// Customer's LLM invoice (zero in `pool` billing mode where they
    /// pay flat $/mo and we eat the LLM cost).
    pub customer_cost_usd: f64,
    /// Per-model token+cost breakdown. Helps the customer see WHICH
    /// model is dominating their spend.
    pub customer_by_model: Vec<ModelCostShare>,
    pub provider: ProviderCostBreakdown,
    /// "Your Pro tier budget covers this" vs "this would push you over"
    /// signal. PR-2's confirmation prompt uses this to nudge customers
    /// toward upgrading rather than getting surprise-billed.
    pub fits_in_tier: TierFit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCostShare {
    pub model: String,
    pub dispatches: u32,
    pub customer_cost_usd: f64,
    pub tokens_in_estimate: i64,
    pub tokens_out_estimate: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TierFit {
    /// Within the tier's allowance — green light.
    Fits,
    /// Within tier but uses > 60% of the monthly budget. Surface a
    /// warning so the customer knows they shouldn't run this 5× this
    /// month.
    HeavyForTier,
    /// Exceeds the tier's per-run cap or monthly allowance. Refuse
    /// without explicit override; suggest upgrade.
    ExceedsTier,
}

/// Compute the pre-run cost estimate for a variant matrix.
///
/// Inputs:
/// - `matrix`: the methodology's variant matrix
/// - `rates`: the rate card (typically `CostRateCard::defaults_v1()`)
/// - `billing_mode`: customer's billing tier
/// - `judge_calls_per_dispatch`: how many LLM-judge calls each dispatch
///   triggers (0 for rule-based rubrics, 1 for simple LLM-judge, >1 for
///   composite rubrics with multiple judges)
///
/// Returns a `CostEstimate` ready to render in the confirmation prompt.
pub fn cost_estimate_for_matrix(
    matrix: &VariantMatrix,
    rates: &CostRateCard,
    billing_mode: BillingMode,
    judge_calls_per_dispatch: u32,
) -> CostEstimate {
    let prompts = matrix.prompts.len().max(1) as u32;
    let conditions_count = matrix.conditions.len().max(1) as u32;
    let dispatches_per_model = prompts * conditions_count * matrix.reps_per_cell;
    let total_dispatches = dispatches_per_model * (matrix.models.len().max(1) as u32);

    // Customer-side: estimate per model × dispatches × token assumption ×
    // per-million price.
    let mut customer_by_model: Vec<ModelCostShare> = Vec::new();
    let mut customer_total: f64 = 0.0;

    for model in &matrix.models {
        let pricing = ModelTokenPrice::for_model(model);
        // Total tokens for this model across all condition rows.
        // Sum token assumption × reps × prompts per condition.
        let mut tokens_in: f64 = 0.0;
        let mut tokens_out: f64 = 0.0;
        for condition in &matrix.conditions {
            let assumption = DispatchTokenAssumption::for_condition(condition);
            let n = (prompts * matrix.reps_per_cell) as f64;
            tokens_in += assumption.tokens_in * n;
            tokens_out += assumption.tokens_out * n;
        }
        // If conditions was empty, assume cold for one pseudo-condition
        if matrix.conditions.is_empty() {
            let assumption = DispatchTokenAssumption::for_condition("cold");
            let n = (prompts * matrix.reps_per_cell) as f64;
            tokens_in += assumption.tokens_in * n;
            tokens_out += assumption.tokens_out * n;
        }

        let cost = tokens_in / 1_000_000.0 * pricing.usd_per_million_in
            + tokens_out / 1_000_000.0 * pricing.usd_per_million_out;
        customer_total += cost;
        customer_by_model.push(ModelCostShare {
            model: model.clone(),
            dispatches: dispatches_per_model,
            customer_cost_usd: cost,
            tokens_in_estimate: tokens_in as i64,
            tokens_out_estimate: tokens_out as i64,
        });
    }

    // Provider-side
    let judge_cost = (total_dispatches as f64) * (judge_calls_per_dispatch as f64)
        * rates.llm_judge_cost_per_call_usd;
    // Conservative orchestrator compute estimate: 0.8 sec per dispatch
    // (queue + composer + DB write). Matches the v2.9 part 5 12-min
    // wall-clock for 150 dispatches with parallelism.
    let compute_cost = (total_dispatches as f64) * 0.8 * rates.compute_per_second_usd;
    // Storage: ~10KB compressed receipt per dispatch, held 28 days
    let storage_bytes = (total_dispatches as i64) * 10_000;
    let storage_cost =
        (storage_bytes as f64) * rates.storage_per_byte_month_usd * (28.0 / 30.0);
    // Bandwidth: receipts pulled to GUI once on completion + occasional re-reads
    let bandwidth_cost = (storage_bytes as f64) * rates.bandwidth_per_byte_usd * 2.0;

    // In `Byok`, we pay nothing for LLM dispatches. In `Pool` we own them.
    let provider_llm_cost = match billing_mode {
        BillingMode::Byok => 0.0,
        BillingMode::Pool => customer_total, // we'd pay the same LLM cost
    };

    let provider_total =
        provider_llm_cost + judge_cost + compute_cost + storage_cost + bandwidth_cost;

    let provider = ProviderCostBreakdown {
        llm_cost_usd: provider_llm_cost,
        judge_cost_usd: judge_cost,
        compute_cost_usd: compute_cost,
        storage_cost_usd: storage_cost,
        bandwidth_cost_usd: bandwidth_cost,
        total_usd: provider_total,
    };

    // Tier fit: simple thresholds. PR-5 reads tier limits from the rate
    // card; PR-1 hard-codes Pro tier ($29/mo, $5/run heavy threshold).
    let fits_in_tier = match billing_mode {
        BillingMode::Byok => {
            // BYOK doesn't constrain on dispatch cost (customer pays it).
            // Just check our provider cost stays under their tier price
            // (so they don't bleed our margin).
            if provider_total > 29.0 {
                TierFit::ExceedsTier
            } else if provider_total > 18.0 {
                TierFit::HeavyForTier
            } else {
                TierFit::Fits
            }
        }
        BillingMode::Pool => {
            // Team tier $99/mo, $50/seat pool credit. If we'd burn more
            // than the seat credit on a single run, it's heavy.
            if provider_total > 50.0 {
                TierFit::ExceedsTier
            } else if provider_total > 30.0 {
                TierFit::HeavyForTier
            } else {
                TierFit::Fits
            }
        }
    };

    CostEstimate {
        total_dispatches,
        customer_cost_usd: customer_total,
        customer_by_model,
        provider,
        fits_in_tier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_matrix() -> VariantMatrix {
        VariantMatrix {
            prompts: vec!["p1".to_string(), "p2".to_string()],
            models: vec!["claude-haiku-4-5".to_string()],
            conditions: vec!["cold".to_string()],
            reps_per_cell: 5,
        }
    }

    #[test]
    fn cold_only_estimate_is_low_cost() {
        // 1 model × 2 prompts × 1 condition × 5 reps = 10 dispatches
        // Cold mode = ~250 tokens out, claude-haiku price = $1.25/M out
        // Customer cost ≈ 10 × 250 / 1M × $1.25 ≈ $0.003
        let est = cost_estimate_for_matrix(
            &small_matrix(),
            &CostRateCard::defaults_v1(),
            BillingMode::Byok,
            0,
        );
        assert_eq!(est.total_dispatches, 10);
        assert!(
            est.customer_cost_usd < 0.05,
            "10 cold haiku dispatches should be cheap; got ${}",
            est.customer_cost_usd
        );
        assert_eq!(est.fits_in_tier, TierFit::Fits);
    }

    #[test]
    fn strict_grounded_dispatches_are_25x_more_expensive_than_cold() {
        // Same matrix shape but strict instead of cold — the load-
        // bearing empirical calibration from Part 5.
        let cold = cost_estimate_for_matrix(
            &small_matrix(),
            &CostRateCard::defaults_v1(),
            BillingMode::Byok,
            0,
        );
        let mut strict_matrix = small_matrix();
        strict_matrix.conditions = vec!["strict".to_string()];
        let strict = cost_estimate_for_matrix(
            &strict_matrix,
            &CostRateCard::defaults_v1(),
            BillingMode::Byok,
            0,
        );
        // 4400 tokens out (strict) vs 250 (cold) = 17.6×, plus the
        // tokens_in change is negligible. The empirical multiplier is
        // ~14-25× depending on prompt; we assert at least 10× to allow
        // calibration drift.
        let ratio = strict.customer_cost_usd / cold.customer_cost_usd;
        assert!(
            ratio > 10.0,
            "strict should cost >>10x more than cold; got {}x",
            ratio
        );
    }

    #[test]
    fn byok_billing_does_not_charge_us_for_llm() {
        let est = cost_estimate_for_matrix(
            &small_matrix(),
            &CostRateCard::defaults_v1(),
            BillingMode::Byok,
            0,
        );
        assert_eq!(
            est.provider.llm_cost_usd, 0.0,
            "byok = customer's keys, we pay $0 for LLM dispatches"
        );
        // We still incur tiny compute + storage + bandwidth costs
        assert!(est.provider.total_usd > 0.0);
        assert!(est.provider.total_usd < 0.05, "small run total should be cents");
    }

    #[test]
    fn pool_billing_charges_us_for_llm() {
        let est = cost_estimate_for_matrix(
            &small_matrix(),
            &CostRateCard::defaults_v1(),
            BillingMode::Pool,
            0,
        );
        assert!(
            est.provider.llm_cost_usd > 0.0,
            "pool = our keys, we pay the LLM cost"
        );
        assert_eq!(
            est.provider.llm_cost_usd, est.customer_cost_usd,
            "in pool mode, our LLM cost equals what the customer's invoice would have been"
        );
    }

    #[test]
    fn judge_calls_add_to_provider_cost_proportionally() {
        let no_judge = cost_estimate_for_matrix(
            &small_matrix(),
            &CostRateCard::defaults_v1(),
            BillingMode::Byok,
            0,
        );
        let one_judge = cost_estimate_for_matrix(
            &small_matrix(),
            &CostRateCard::defaults_v1(),
            BillingMode::Byok,
            1, // one judge call per dispatch
        );
        let delta = one_judge.provider.judge_cost_usd - no_judge.provider.judge_cost_usd;
        // 10 dispatches × 1 judge call each × $0.001 = $0.01
        assert!(
            (delta - 0.01).abs() < 0.0001,
            "1-judge delta should equal 10 × $0.001 = $0.01; got {}",
            delta
        );
    }

    #[test]
    fn tier_fit_flags_heavy_runs() {
        // A big methodology that pushes provider cost into the heavy zone
        let big = VariantMatrix {
            prompts: vec!["p1".to_string(); 5],
            models: vec!["claude-opus-4-7".to_string()], // expensive model
            conditions: vec!["strict".to_string()],
            reps_per_cell: 100,
        };
        let est = cost_estimate_for_matrix(
            &big,
            &CostRateCard::defaults_v1(),
            BillingMode::Pool,
            1, // also judge each dispatch
        );
        // 5 prompts × 1 model × 1 condition × 100 reps = 500 dispatches
        // strict tokens_out = 4400, opus output price = $75/M
        // → customer cost ≈ 500 × 4400 / 1M × 75 = $165
        // → in pool mode, provider LLM cost = $165 too
        // → fits_in_tier should be ExceedsTier
        assert_eq!(est.total_dispatches, 500);
        assert!(
            matches!(est.fits_in_tier, TierFit::ExceedsTier),
            "500 opus strict dispatches must exceed Team tier; got {:?}, provider total ${}",
            est.fits_in_tier,
            est.provider.total_usd
        );
    }

    #[test]
    fn customer_by_model_breaks_down_per_model() {
        let multi = VariantMatrix {
            prompts: vec!["p1".to_string()],
            models: vec![
                "claude-haiku-4-5".to_string(),
                "claude-opus-4-7".to_string(),
            ],
            conditions: vec!["soft".to_string()],
            reps_per_cell: 10,
        };
        let est = cost_estimate_for_matrix(
            &multi,
            &CostRateCard::defaults_v1(),
            BillingMode::Byok,
            0,
        );
        assert_eq!(est.customer_by_model.len(), 2);
        // Opus is ~60× more expensive per output token than haiku, so the
        // opus row should dominate the cost
        let haiku = &est.customer_by_model[0];
        let opus = &est.customer_by_model[1];
        assert!(
            opus.customer_cost_usd > haiku.customer_cost_usd * 10.0,
            "opus should dominate cost vs haiku; haiku=${} opus=${}",
            haiku.customer_cost_usd,
            opus.customer_cost_usd
        );
    }
}
