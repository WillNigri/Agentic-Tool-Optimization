import { invoke } from "@tauri-apps/api/core";

// v2.1.0 Phase 4 — Active runs registry (frontend wrappers).
//
// The Rust source of truth is `apps/desktop/src-tauri/src/active_runs.rs`.
// Polled by the Live sub-tab in Insights so users can see what's
// running and kill anything stuck without hunting through the
// terminal buffer.

export interface ActiveRun {
  run_id: string;
  agent_slug: string | null;
  runtime: string;
  workspace: string | null;
  /** Unix epoch seconds. */
  started_at_unix: number;
  /** "running" | "killing" */
  status: string;
  /** Human-readable origin label. e.g. "desktop:context-dispatch". */
  source: string | null;
  /** v2.6 PR-A — "active" = ATO fired this; "passive_observation" =
   *  the watcher saw it happen in an external CLI session
   *  (Claude Code / Codex). Older live_runs rows that pre-date the
   *  column come back as "active". */
  dispatch_kind: "active" | "passive_observation";
  /** v2.6 PR-A — which billing surface this dispatch is hitting.
   *  null when unknown (e.g. openclaw/hermes which don't expose an
   *  auth-mode signal). */
  billing_surface:
    | "claude_code_subscription"
    | "codex_cli_subscription"
    | "gemini_cli_subscription"
    | "anthropic_api"
    | "openai_api"
    | "gemini_api"
    | "minimax_api"
    | "openrouter_api"
    | "ollama_local"
    | "unknown"
    | null;
}

export async function listActiveRuns(): Promise<ActiveRun[]> {
  try {
    return await invoke<ActiveRun[]>("list_active_runs");
  } catch {
    return [];
  }
}

/** Returns true when the kill signal landed on a process. False when
 *  the run is unknown OR the dispatch path didn't expose a child handle
 *  (the run still gets marked status='killing' so the UI reflects intent,
 *  but only the active-process variants can actually terminate). */
export async function killActiveRun(runId: string): Promise<boolean> {
  try {
    return await invoke<boolean>("kill_active_run", { runId });
  } catch {
    return false;
  }
}

// v2.1.0+ Concurrent attribution refinement.
//
// When two agents dispatch into the same workspace, neither can be
// individually disambiguated as the writer of any specific file (the
// OS gives us mtimes, not PIDs). Instead of pretending we can, we
// record honest "this run overlapped with @other" evidence that
// surfaces in the dashboard as an "ambiguous" badge.
export interface OverlapPeer {
  run_id: string;
  agent_slug: string | null;
  runtime: string;
  started_at_unix: number;
}
export interface OverlapEvidence {
  overlapped_with: OverlapPeer[];
}

/** Snapshot the overlap evidence for a finished run. Call BEFORE
 *  finish_run on the same run_id (registry holds the data until then).
 *  Returns an empty list when the run was alone in its workspace. */
export async function getOverlapEvidence(runId: string): Promise<OverlapEvidence> {
  try {
    return await invoke<OverlapEvidence>("get_overlap_evidence", { runId });
  } catch {
    return { overlapped_with: [] };
  }
}

// Note: there's no isMockMode override here — overlap evidence is
// computed locally by the Rust active_runs registry, not fetched
// from cloud. Real Tauri call always works regardless of cloud auth.

/** v2.6 PR-A — human-readable label for a billing_surface. Used by
 *  the Live tab chip + the Usage tab group-by toggle so the same
 *  vocabulary appears everywhere. Keep in sync with the Rust
 *  `billing_surface` enum values written by the active dispatch
 *  path (active_runs::billing_surface_for_active) and the passive
 *  watcher (passive_observer::SourceKind::default_billing_surface). */
export function billingSurfaceLabel(s: ActiveRun["billing_surface"]): string {
  switch (s) {
    case "claude_code_subscription":
      return "Claude Code subscription";
    case "codex_cli_subscription":
      return "Codex CLI subscription";
    case "gemini_cli_subscription":
      return "Gemini CLI subscription";
    case "anthropic_api":
      return "Anthropic API credits";
    case "openai_api":
      return "OpenAI API credits";
    case "gemini_api":
      return "Gemini API credits";
    case "minimax_api":
      return "MiniMax API credits";
    case "openrouter_api":
      return "OpenRouter credits";
    case "ollama_local":
      return "Local (Ollama)";
    case "unknown":
    case null:
    default:
      return "Unknown";
  }
}

/** Compact label for the Live tab chip — full strings overflow on
 *  narrow rows. Subscription / API / Local family. */
export function billingSurfaceShortLabel(s: ActiveRun["billing_surface"]): string {
  switch (s) {
    case "claude_code_subscription":
    case "codex_cli_subscription":
    case "gemini_cli_subscription":
      return "Subscription";
    case "anthropic_api":
    case "openai_api":
    case "gemini_api":
    case "minimax_api":
    case "openrouter_api":
      return "API credits";
    case "ollama_local":
      return "Local";
    case "unknown":
    case null:
    default:
      return "—";
  }
}
