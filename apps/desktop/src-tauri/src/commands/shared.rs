// commands/shared.rs — cross-cutting types and helpers used by more
// than one of the commands/<domain>.rs files.
//
// PR 1 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md alongside)
// establishes this file as a stable foundation. Subsequent PRs move
// individual types here (AgentMessage, DispatchResult, ReplayJob,
// HookConfig, active-runs bookkeeping, log attribution) so domain
// modules can import them via `use super::shared::*;` instead of
// duplicating definitions or threading them through awkward paths.
//
// Codex's Round 1 review on 2026-05-17 named the specific failure
// mode this file prevents: "compile-green, dogfood-green, but
// semantic drift in duplicated helper logic" — two copies of the
// dispatch pipeline that differ in subtle ways. shared.rs is the
// canonical home.
//
// PR 1 lands the file empty (other than this header) so the
// directory + module structure are in place. Each subsequent PR
// that moves a domain into its own commands/<domain>.rs gets to
// pull whatever cross-cutting types it needs from this file in
// the same PR — no half-baked imports, no duplicated helpers.
//
// Convention for future contributors:
//   - A type goes here ONLY if ≥2 domain modules use it.
//   - Single-domain helpers stay in their domain's commands/<x>.rs.
//   - Anything that touches dispatch (AgentMessage, DispatchResult,
//     ApiDispatchResult, ReplayJob) is presumptively cross-cutting
//     and belongs here.
