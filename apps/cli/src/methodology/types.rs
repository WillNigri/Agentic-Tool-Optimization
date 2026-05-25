// v2.10.0 PR-1 — Rust types matching the schema added in v2.10 PR-1
// (apps/desktop/src-tauri/src/schema.rs). Stay in lockstep: any column
// added or renamed in the schema MUST land here too, otherwise CLI
// readers will silently drop fields.

use serde::{Deserialize, Serialize};

/// A methodology = a reusable test recipe (e.g., "which model for security
/// review"). Persisted in the `methodologies` table; one row per slug.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Methodology {
    pub id: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
    /// One of `Archetype::as_str()` values; "custom" for user-defined.
    pub archetype: String,
    /// JSON serialization of `VariantMatrix`. Stored as TEXT in SQLite so
    /// older clients reading newer rows can preserve unknown fields.
    pub variant_matrix: String,
    /// JSON serialization of the rubric config. Stored as TEXT for same
    /// forward-compat reason. Rubric kinds: `regex`, `structural`,
    /// `llm_judge`, `composite`. Detailed shape lives in
    /// `methodology::rubric` (PR-4).
    pub rubric: String,
    pub created_at: String,
    #[serde(default)]
    pub created_by: Option<String>,
}

/// Variant matrix definition — the fan-out shape for one methodology.
/// Serialized into `Methodology.variant_matrix` JSON.
///
/// Example:
/// ```ignore
/// VariantMatrix {
///   prompts: vec!["./prompts/security/*.md".to_string()],
///   models: vec!["claude-sonnet-4-6".to_string(), "claude-opus-4-7".to_string()],
///   conditions: vec!["cold".to_string(), "soft".to_string(), "strict".to_string()],
///   reps_per_cell: 30,
/// }
/// ```
/// Produces 1 prompt-glob × 2 models × 3 conditions × 30 reps = up to
/// `glob_expansion * 180` total dispatches, where `glob_expansion` is the
/// number of files the prompts glob resolves to at run time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VariantMatrix {
    /// Prompt sources. Each entry is a literal prompt OR a glob path the
    /// runner expands at run time (e.g. `./prompts/*.md`). Glob expansion
    /// happens once at run start; the expanded list is recorded in the
    /// `methodology_runs.verdict_json` so the customer can re-run the
    /// exact same set later.
    pub prompts: Vec<String>,

    /// Model identifiers. Use the `<provider>/<model>` format where the
    /// provider isn't the model's namespace (e.g. `openai/gpt-4o`,
    /// `codex/gpt-4.1`). For claude / anthropic models the slug alone
    /// suffices (e.g. `claude-sonnet-4-6`).
    pub models: Vec<String>,

    /// Conditions overlaid on every dispatch. For the v2.9 grounded-mode
    /// archetype these are `cold` / `soft` / `strict`. For custom
    /// methodologies, any string the rubric understands.
    pub conditions: Vec<String>,

    /// Replications per cell. **Industry baseline default: 30.** The
    /// sample-size advisor (see `packages/ato-pricing/pricing.json`)
    /// surfaces this number at methodology-create time with explicit
    /// references to Promptfoo / Braintrust / HumanEval / etc baselines.
    pub reps_per_cell: u32,

    /// Optional runtime override. When `None`, the runner auto-derives
    /// the runtime from each model name (`claude-*` → anthropic API,
    /// `gemini-*` → google API, etc.). When `Some("claude")` /
    /// `Some("codex")` / `Some("gemini")`, the runner uses the
    /// **CLI** path instead of the API — same model, customer's
    /// existing subscription pays, no BYOK keys required. v2.10 PR-3
    /// addition: needed so a customer can A/B their Claude Max
    /// subscription vs. an Anthropic API key on the same prompt.
    #[serde(default)]
    pub runtime: Option<String>,

    /// v2.11 PR-12.0 — holdout prompts (Q7 overfitting defense #1 per
    /// `docs/v2.11-learning-loop.md`). These prompts are kept OUT of
    /// the standard fan-out so the diagnose agent never sees them. The
    /// A/B win condition that follows a learning-loop diagnose must
    /// hold on holdout cells too, not just on visible cells. Without
    /// this, applying a diagnose proposal that games the visible
    /// rubric ships agents that regress in production on the cases
    /// nobody measured. Empty by default — methodologies without a
    /// learning loop don't pay any cost for this field.
    #[serde(default)]
    pub holdout_prompts: Vec<String>,
}

impl VariantMatrix {
    /// Total dispatches the matrix would fan out to assuming every prompt
    /// is literal (no glob expansion). Cost estimation uses this as the
    /// upper-bound count; the runner re-computes after glob expansion.
    pub fn total_dispatches(&self) -> u32 {
        let prompts = self.prompts.len().max(1) as u32;
        let models = self.models.len().max(1) as u32;
        let conditions = self.conditions.len().max(1) as u32;
        prompts * models * conditions * self.reps_per_cell
    }
}

/// A methodology RUN = one execution of a methodology with its full dual
/// cost accounting ledger. Persisted in `methodology_runs`. The
/// load-bearing schema for Pro economics: every customer-paid Pro tier
/// dollar must be measurable against the provider-cost we incurred to
/// deliver this run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodologyRun {
    pub id: String,
    pub methodology_id: String,
    #[serde(default)]
    pub customer_user_id: Option<String>,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    pub status: MethodologyRunStatus,
    pub total_dispatches_planned: u32,
    #[serde(default)]
    pub total_dispatches_completed: u32,

    // ---- Customer-side cost (their LLM invoice / pool burn) ----
    #[serde(default)]
    pub customer_cost_usd: f64,
    #[serde(default)]
    pub customer_tokens_in: i64,
    #[serde(default)]
    pub customer_tokens_out: i64,
    #[serde(default)]
    pub customer_dispatches: u32,
    #[serde(default = "default_billing_mode")]
    pub customer_billing_mode: BillingMode,

    // ---- Provider-side cost (what WE pay) ----
    #[serde(default)]
    pub provider_llm_cost_usd: f64,
    #[serde(default)]
    pub provider_judge_cost_usd: f64,
    #[serde(default)]
    pub provider_compute_seconds: f64,
    #[serde(default)]
    pub provider_storage_bytes: i64,
    #[serde(default)]
    pub provider_bandwidth_bytes: i64,
    #[serde(default)]
    pub provider_total_cost_usd: f64,

    // ---- Computed margin ----
    #[serde(default)]
    pub margin_usd: f64,

    // ---- Result ----
    #[serde(default)]
    pub verdict_json: Option<String>,
    #[serde(default)]
    pub receipt_url: Option<String>,

    /// v2.11 PR-12.0 — variant A/B linkage. NULL on baseline runs;
    /// set to the baseline run's id when this row is a variant A/B
    /// being compared against its parent. The diagnose pipeline
    /// populates this on `--ab` runs so `runs show` can present the
    /// before/after composition side by side.
    #[serde(default)]
    pub parent_run_id: Option<String>,
}

fn default_billing_mode() -> BillingMode {
    BillingMode::Byok
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodologyRunStatus {
    Queued,
    Running,
    Complete,
    Failed,
    Cancelled,
}

impl MethodologyRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "complete" => Some(Self::Complete),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Customer billing mode for THIS run. Determines who pays the API bill
/// and which provider_*_cost columns get populated.
///
/// - `Byok` (default $29/mo Pro): the customer's own API keys are used for
///   every dispatch. `provider_llm_cost_usd` stays $0 (we don't pay LLM
///   provider costs); `provider_judge_cost_usd` may be non-zero if the
///   rubric uses an LLM-judge running on our pool key.
/// - `Pool` (Team $99/mo): our shared Pro pool keys are used. Every
///   dispatch's cost lands in `provider_llm_cost_usd`; the customer's
///   `customer_cost_usd` shows their tier allocation, not raw spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BillingMode {
    Byok,
    Pool,
}

impl BillingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Byok => "byok",
            Self::Pool => "pool",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "byok" => Some(Self::Byok),
            "pool" => Some(Self::Pool),
            _ => None,
        }
    }
}

/// One row in the composition table — links a methodology run to every
/// execution_log row it composed, with the variant cell coordinates and
/// the rubric score that dispatch earned. Composite PK enforced at the
/// schema level: (methodology_run_id, execution_log_id) can appear at
/// most once.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodologyRunDispatch {
    pub methodology_run_id: String,
    pub execution_log_id: String,
    /// JSON: `{"prompt_id": "...", "model": "...", "condition": "...",
    /// "rep_idx": N}` — the cell coordinates in the variant matrix.
    pub variant_cell: String,
    #[serde(default)]
    pub score: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_matrix_default_holdout_prompts_is_empty() {
        // v2.11 PR-12.0 — methodologies without a learning loop must
        // deserialize cleanly from JSON that doesn't mention the new
        // field (existing methodology rows in the DB).
        let json = r#"{
            "prompts": ["p1"],
            "models": ["claude-sonnet-4-6"],
            "conditions": ["soft"],
            "reps_per_cell": 30
        }"#;
        let m: VariantMatrix = serde_json::from_str(json).expect("back-compat deserialize");
        assert_eq!(m.holdout_prompts.len(), 0);
        assert_eq!(m.runtime, None);
    }

    #[test]
    fn variant_matrix_holdout_prompts_round_trip() {
        let m = VariantMatrix {
            prompts: vec!["visible".to_string()],
            models: vec!["claude-sonnet-4-6".to_string()],
            conditions: vec!["soft".to_string()],
            reps_per_cell: 30,
            runtime: None,
            holdout_prompts: vec![
                "this prompt is NEVER shown to the diagnose agent".to_string(),
            ],
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let round: VariantMatrix =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(round.holdout_prompts, m.holdout_prompts);
        // Holdouts don't count toward total_dispatches — they're
        // evaluated separately by the A/B step.
        assert_eq!(round.total_dispatches(), 30);
    }

    #[test]
    fn methodology_run_parent_run_id_default_is_none() {
        // v2.11 PR-12.0 — back-compat: existing methodology_runs rows
        // don't have parent_run_id (NULL), and the deserialize path
        // must accept that.
        let json = r#"{
            "id": "r-baseline",
            "methodology_id": "m-1",
            "started_at": "2026-05-25T00:00:00Z",
            "status": "complete",
            "total_dispatches_planned": 30
        }"#;
        let r: MethodologyRun =
            serde_json::from_str(json).expect("deserialize without parent_run_id");
        assert!(r.parent_run_id.is_none());
    }

    #[test]
    fn variant_matrix_total_dispatches_multiplies_all_axes() {
        let m = VariantMatrix {
            prompts: vec!["p1".to_string(), "p2".to_string(), "p3".to_string()],
            models: vec!["claude".to_string(), "gemini".to_string()],
            conditions: vec!["cold".to_string(), "soft".to_string(), "strict".to_string()],
            reps_per_cell: 10,
            runtime: None,
            holdout_prompts: Vec::new(),
        };
        // 3 × 2 × 3 × 10 = 180
        assert_eq!(m.total_dispatches(), 180);
    }

    #[test]
    fn variant_matrix_empty_axes_count_as_one_for_dispatch_count() {
        // If a customer leaves conditions empty (e.g. they only care
        // about which-model with no grounding variation), the matrix
        // should still produce prompts × models × reps dispatches,
        // not zero.
        let m = VariantMatrix {
            prompts: vec!["p1".to_string()],
            models: vec!["claude".to_string(), "gemini".to_string()],
            conditions: vec![],
            reps_per_cell: 5,
            runtime: None,
            holdout_prompts: Vec::new(),
        };
        assert_eq!(m.total_dispatches(), 10);
    }

    #[test]
    fn run_status_round_trips_through_string() {
        for status in [
            MethodologyRunStatus::Queued,
            MethodologyRunStatus::Running,
            MethodologyRunStatus::Complete,
            MethodologyRunStatus::Failed,
            MethodologyRunStatus::Cancelled,
        ] {
            assert_eq!(MethodologyRunStatus::parse(status.as_str()), Some(status));
        }
    }

    #[test]
    fn run_status_parses_unknown_to_none_not_panicking() {
        assert_eq!(MethodologyRunStatus::parse("future-state"), None);
        assert_eq!(MethodologyRunStatus::parse(""), None);
    }

    #[test]
    fn billing_mode_round_trips_and_defaults_to_byok() {
        assert_eq!(default_billing_mode(), BillingMode::Byok);
        assert_eq!(BillingMode::parse("byok"), Some(BillingMode::Byok));
        assert_eq!(BillingMode::parse("pool"), Some(BillingMode::Pool));
        assert_eq!(BillingMode::parse("enterprise-future"), None);
    }

    #[test]
    fn methodology_run_serializes_with_back_compat_defaults() {
        // A minimally-populated MethodologyRun (just the fields the
        // schema requires NOT NULL on) should deserialize back from
        // JSON cleanly. Forward-compat for older clients reading
        // newer rows: every additive field uses #[serde(default)] so
        // missing columns don't panic.
        let json = r#"{
            "id": "r-1",
            "methodology_id": "m-1",
            "started_at": "2026-05-24T20:00:00Z",
            "status": "queued",
            "total_dispatches_planned": 30
        }"#;
        let r: MethodologyRun = serde_json::from_str(json).expect("deserialize minimal");
        assert_eq!(r.id, "r-1");
        assert_eq!(r.status, MethodologyRunStatus::Queued);
        assert_eq!(r.customer_billing_mode, BillingMode::Byok);
        assert_eq!(r.customer_cost_usd, 0.0);
        assert_eq!(r.provider_total_cost_usd, 0.0);
        assert_eq!(r.margin_usd, 0.0);
    }
}
