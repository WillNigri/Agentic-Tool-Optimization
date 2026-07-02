// ato-bench — the open-box benchmark harness.
//
// This crate is the trust anchor for ATO's "open box" thesis: your keys → your
// models → a VERIFIABLE grader → a REPRODUCIBLE receipt. Where a closed router
// hands you a score you can't reproduce and a route you can't inspect, this
// produces a scorecard whose every input is hashed and re-runnable.
//
// Layers (this slice — Phase 1a-1 — is grader + receipt + stats; model dispatch
// and the LiveCodeBench importer are later slices):
//
//   problem  — the code-generation problem schema (stdin→stdout tests).
//   sandbox  — isolated execution backends (Docker preferred; macOS seatbelt
//              fallback; unconfined only on explicit opt-in).
//   grader   — extract code, run it, compare stdout → pass/fail TaskReceipt.
//   receipt  — the execution-receipt contract + deterministic hashes
//              (dataset / harness / exec-env) + contamination classifier.
//   stats    — binomial pass-rate with Wilson confidence intervals.
//   scorecard— composes the above into one model's result over one pinned
//              dataset, with a contamination-clean headline pass rate.
//
// OSS / free: this is the "run your own tests with your own keys" half of the
// product. Curated premium suites and continuous auto-routing live in the paid
// ato-cloud backend, never here.

pub mod grader;
pub mod problem;
pub mod receipt;
pub mod sandbox;
pub mod scorecard;
pub mod stats;

pub use grader::{extract_code, grade_problem, GraderConfig, DEFAULT_EXTRACTION_REGEX};
pub use problem::{ComparisonMode, Language, Problem, TestCase};
pub use receipt::{
    classify_contamination, stable_hash, ContaminationFlag, DatasetSnapshot, ExecEnv, FailureKind,
    HarnessConfig, RunContext, Sampling, TaskReceipt,
};
pub use sandbox::{
    select_sandbox, ExecLimits, ExecOutcome, Sandbox, SandboxError, SandboxOptions, SandboxReport,
};
pub use scorecard::{ContaminationSummary, Scorecard};
pub use stats::{wilson_interval, WilsonInterval, Z_90, Z_95, Z_99};
