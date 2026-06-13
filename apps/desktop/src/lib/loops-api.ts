import { invoke } from "@tauri-apps/api/core";

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
  triggerKind?: string;
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
