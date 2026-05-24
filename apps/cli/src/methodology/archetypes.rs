// v2.10.0 PR-1 — Archetype enum: the four pre-built methodology
// templates that ship with v2.10. Customers can build their own on top,
// but archetypes give the runner well-known shapes to optimize for
// (cost estimation, default rubrics, GUI rendering).
//
// Mapped to questions customers actually ask:
//
//   - ModelLadder        — "should Agent X stay on Opus 4.7 or downgrade
//                          to Sonnet 4.6?" The primary use case. Same
//                          agent × N models the customer has keys for ×
//                          M prompts × R reps. Output: Pareto frontier
//                          with cost/quality trade.
//
//   - ToolsVsNoTools     — "does grounding actually change behavior on
//                          MY work?" The methodology that produced the
//                          v2.9 build log, packaged for customers.
//                          Same prompt × same model × {cold, soft,
//                          strict} × R reps.
//
//   - ReviewerOrderEffects — "did the order of voices change the
//                            consensus?" Sticky session with order
//                            permuted N times. Quantifies bias.
//
//   - RegressionWatch    — "did our agent get worse this week?"
//                          Scheduled re-run of any methodology with
//                          diff alerts on quality drop, cost rise,
//                          recommended-model rank change.
//
//   - Custom             — user-defined matrix + rubric. Falls back to
//                          generic rendering and standard cost math.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Archetype {
    ModelLadder,
    ToolsVsNoTools,
    ReviewerOrderEffects,
    RegressionWatch,
    Custom,
}

impl Archetype {
    /// Stringified token used in the `methodologies.archetype` column
    /// AND in the CLI's `--archetype` flag. Stable across versions —
    /// any new archetype lands here AND in the parse() function below.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ModelLadder => "model-ladder",
            Self::ToolsVsNoTools => "tools-vs-no-tools",
            Self::ReviewerOrderEffects => "reviewer-order-effects",
            Self::RegressionWatch => "regression-watch",
            Self::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "model-ladder" => Some(Self::ModelLadder),
            "tools-vs-no-tools" => Some(Self::ToolsVsNoTools),
            "reviewer-order-effects" => Some(Self::ReviewerOrderEffects),
            "regression-watch" => Some(Self::RegressionWatch),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// Customer-facing label rendered in the GUI archetype picker and
    /// at `ato evaluations methodology archetypes list`. Kept short
    /// enough to fit in a CLI table.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ModelLadder => "Which model for this task?",
            Self::ToolsVsNoTools => "Does grounding change behavior?",
            Self::ReviewerOrderEffects => "Did reviewer order bias the consensus?",
            Self::RegressionWatch => "Did our agent get worse this week?",
            Self::Custom => "Custom methodology",
        }
    }

    /// Sentence-length explanation for the GUI picker tooltip.
    pub fn description(&self) -> &'static str {
        match self {
            Self::ModelLadder => {
                "Run the same agent across every model you have keys for, score \
                 each on YOUR prompts, get a cost-quality Pareto frontier with a \
                 recommended pick. Run weekly to catch when a model release \
                 changes the ranking."
            }
            Self::ToolsVsNoTools => {
                "Compare the same model under cold (no grounding), soft, and \
                 strict grounding modes on identical prompts. Quantifies whether \
                 grounded mode actually changes behavior on your work."
            }
            Self::ReviewerOrderEffects => {
                "Run a sticky session with reviewer order permuted N times. \
                 Surfaces 'round 1 reviewer shapes round 2' consensus bias \
                 in multi-LLM reviews."
            }
            Self::RegressionWatch => {
                "Schedule any methodology to re-run weekly. Diff against last \
                 week's verdict. Alert when quality drops, cost rises, or the \
                 recommended model changes."
            }
            Self::Custom => {
                "Bring your own variant matrix + rubric. The runner handles \
                 fan-out, scoring, composition, and cost decomposition the same \
                 way as the pre-built archetypes."
            }
        }
    }

    /// Suggested reps_per_cell when the customer picks this archetype.
    /// Customers can override; this is just the default the wizard
    /// pre-fills. Numbers come from `packages/ato-pricing/pricing.json`
    /// sample-size-advice block — kept in sync as the rate card is
    /// re-calibrated.
    pub fn default_reps_per_cell(&self) -> u32 {
        match self {
            // Industry baseline for shipping decisions
            Self::ModelLadder | Self::ToolsVsNoTools => 30,
            // Slightly smaller — order effects are usually clearer with
            // fewer reps because the signal is per-prompt-per-order
            Self::ReviewerOrderEffects => 10,
            // Smaller again — regression-watch is the OUTER loop;
            // each scheduled run uses its parent methodology's default
            // (typically 30). The standalone default here is for ad-hoc
            // creates.
            Self::RegressionWatch => 30,
            Self::Custom => 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archetype_round_trips_through_string() {
        for archetype in [
            Archetype::ModelLadder,
            Archetype::ToolsVsNoTools,
            Archetype::ReviewerOrderEffects,
            Archetype::RegressionWatch,
            Archetype::Custom,
        ] {
            assert_eq!(Archetype::parse(archetype.as_str()), Some(archetype));
        }
    }

    #[test]
    fn archetype_parses_unknown_to_none() {
        assert_eq!(Archetype::parse(""), None);
        assert_eq!(Archetype::parse("model-glider"), None);
        assert_eq!(Archetype::parse("which-model"), None); // old-spec name
    }

    #[test]
    fn all_archetypes_have_label_and_description() {
        // Guards against a future contributor adding a variant but
        // forgetting to fill in the label/description match arms.
        for archetype in [
            Archetype::ModelLadder,
            Archetype::ToolsVsNoTools,
            Archetype::ReviewerOrderEffects,
            Archetype::RegressionWatch,
            Archetype::Custom,
        ] {
            assert!(!archetype.label().is_empty());
            assert!(archetype.description().len() > 30);
        }
    }

    #[test]
    fn model_ladder_defaults_to_industry_baseline_reps() {
        // The primary archetype must default to industry-baseline
        // (n=30 per cell). Sample-size-advice in the rate card.
        assert_eq!(Archetype::ModelLadder.default_reps_per_cell(), 30);
    }
}
