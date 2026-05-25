// v2.10.0 PR-1 — Methodology Runner foundation (Rust types).
//
// Compiles agent + grounded-mode receipts into composed methodology runs:
// take the v2.9 atomic event (one `execution_logs` row with `grounding_verdict`
// + `tool_calls_summary` + cost + duration), fan out N variants × M prompts
// × R reps, score each cell with a rubric, compose into one defensible result
// with confidence intervals + DUAL COST ACCOUNTING (customer-side spend +
// our-side margin tracking).
//
// Architecture spec: docs/methodology-runner.md
// Empirical motivation: the n=150 scaled eval that produced Part 5 of the
// v2.9 build log (~$6, 12 min, confidence intervals, falsified one prior
// n=1 claim). The runner productizes that loop for customers running it
// weekly at n=30/cell across N models.
//
// PR-1 ships only TYPES + cost-estimation helpers. PR-2 wires the CLI
// (`ato evaluations methodology create / run / show / cost`). PR-3 builds
// the fan-out engine + composer. PR-4 ships the rubric library. PR-5
// wires dual cost accounting + admin margin reports.

pub mod archetypes;
pub mod cost;
pub mod types;

#[allow(unused_imports)] // re-exports for consumers (CLI commands, future Pro)
pub use archetypes::Archetype;
#[allow(unused_imports)]
pub use cost::{
    cost_estimate_for_matrix, CostEstimate, CostRateCard, ProviderCostBreakdown,
};
#[allow(unused_imports)]
pub use types::{
    BillingMode, Methodology, MethodologyRun, MethodologyRunDispatch, MethodologyRunStatus,
    VariantMatrix,
};
