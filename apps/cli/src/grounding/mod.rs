// v2.9.0 — Grounded mode (PR-1: foundation).
//
// The harness layer that makes "every AI follows your rules" a checked
// invariant instead of a marketing claim. Sits between the agent record
// and the per-runtime dispatch path: compiles the agent's policy
// (mode + permissions + mandatory rules + allowed_mode_floor) into a
// structured `GroundingPolicy`, applies it at dispatch time (system-prompt
// prepend in soft mode; per-call interceptor + post-validation in strict
// mode in PR-2+), and writes a verdict + override audit onto every receipt.
//
// PR-1 ships only the foundation — policy compilation, badge derivation,
// verdict computation, and the data-shape exchanged with `dispatch.rs`.
// Strict-mode enforcement (mid-stream tool rejection, structured-error
// response, refuse-with-options for parserless runtimes) lands in PR-2.
// API-provider routing through `api_dispatch_tools.rs` lands in PR-3.
// `ato agents serve` + bundle baking lands in PR-4.
//
// Rationale and the empirical research that informed the shape lives in
// `docs/grounding.md` and the plan at
// `/Users/beatriznigri/.claude/plans/witty-crafting-harp.md`.

pub mod badges;
pub mod claude_stream_parser;
pub mod policy;
pub mod verdict;

pub use badges::derive_badges;
pub use claude_stream_parser::{parse_claude_stream_json, ClaudeStreamParseOutput};
pub use policy::{GroundingMode, GroundingPolicy, MandatoryRule, MandatoryRuleKind, OverrideAudit};
pub use verdict::{compile_verdict, GroundingVerdict, ToolCallObservation};
