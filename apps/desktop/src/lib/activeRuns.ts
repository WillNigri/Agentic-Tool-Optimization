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
