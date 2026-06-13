import { invoke } from "@tauri-apps/api/core";

export interface LoopRunStarted {
  runId: string;
  status: string;
}

/**
 * v2.14 step 3 — fire the prod `ato loop run <slug>` CLI from the
 * desktop. The CLI writes loop_runs + loop_run_steps with attribution
 * (initiator=human, surface=desktop) and returns the run id so the
 * caller can poll get_loop_run_steps for live status.
 */
export function run_loop_by_slug(slugOrId: string): Promise<LoopRunStarted> {
  return invoke<LoopRunStarted>("run_loop_by_slug", { slugOrId });
}

export interface Loop {
  id: string;
  slug: string;
  name: string;
  description: string | null;
  enabled: boolean;
  graph: unknown;
  variables: unknown | null;
  triggerKind: string;
  triggerConfig: unknown | null;
  source: string;
  sourceRef: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface LoopCreateInput {
  name: string;
  description?: string | null;
  slug?: string | null;
  /** Codex R4 — expose enabled on create so the Rust default
   *  (enabled=true) can be overridden when the user creates a
   *  disabled workflow. */
  enabled?: boolean | null;
  graph: unknown;
  variables?: unknown | null;
  triggerKind?: string | null;
  triggerConfig?: unknown | null;
  source?: string | null;
  sourceRef?: string | null;
}

export interface LoopUpdateInput {
  name?: string;
  description?: string | null;
  enabled?: boolean;
  graph?: unknown;
  variables?: unknown | null;
  triggerKind?: string | null;
  triggerConfig?: unknown | null;
}

export function list_loops(): Promise<Loop[]> {
  return invoke<Loop[]>("list_loops");
}

export function get_loop(id: string): Promise<Loop> {
  return invoke<Loop>("get_loop", { id });
}

export function create_loop(input: LoopCreateInput): Promise<Loop> {
  return invoke<Loop>("create_loop", { input });
}

export function update_loop(id: string, input: LoopUpdateInput): Promise<Loop> {
  return invoke<Loop>("update_loop", { id, input });
}

export function delete_loop(id: string): Promise<void> {
  return invoke<void>("delete_loop", { id });
}

export async function toggle_loop_enabled(id: string, enabled: boolean): Promise<Loop> {
  return invoke<Loop>("update_loop", { id, input: { enabled } });
}
