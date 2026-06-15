/**
 * Missions API — v2.16 PR-7 desktop bindings.
 * Thin wrappers over Tauri commands; mirrors CLI surface (read + set-category/set-state only).
 */

import { invoke } from "@tauri-apps/api/core";

// ── Types — mirror Rust structs (camelCase from serde rename_all) ─────

export interface MissionSummary {
  id: string;
  slug: string;
  name: string;
  goal: string;
  state: MissionState;
  category: MissionCategory;
  workspaceStrategy: string;
  mergeStrategy: string;
  maxLoops: number | null;
  tokenBudgetUsd: number | null;
  spentUsd: number;
  dispatchCount: number;
  updatedAt: string;
  initiatorKind?: string | null;
  clientSurface?: string | null;
  initiatorId?: string | null;
}

export interface MissionEvent {
  id: string;
  missionId: string;
  kind: string;
  payload: Record<string, unknown> | null;
  occurredAt: string;
}

export interface MissionDetail extends MissionSummary {
  successCriteria: unknown[];
  escalationPolicy: unknown | null;
  baseSha: string | null;
  cleanupPolicy: string;
  resultMetadata: unknown | null;
  narrativeMdPath: string;
  createdAt: string;
  repoRoot: string | null;
  workerConfig: { runtime: string; model: string | null; requireTools: string[] } | null;
  events: MissionEvent[];
  narrativeBody: string | null;
  pendingEscalations: MissionEvent[];
}

export type MissionState = "open" | "in_progress" | "blocked" | "complete";
export type MissionCategory = "autonomous" | "needs_owner" | "ignored" | "done";

export const VALID_STATES: MissionState[] = ["open", "in_progress", "blocked", "complete"];
export const VALID_CATEGORIES: MissionCategory[] = ["autonomous", "needs_owner", "ignored", "done"];

// ── API calls ─────────────────────────────────────────────────────────

export async function missionsList(
  stateFilter?: MissionState,
  categoryFilter?: MissionCategory
): Promise<MissionSummary[]> {
  return invoke<MissionSummary[]>("missions_list", {
    stateFilter: stateFilter ?? null,
    categoryFilter: categoryFilter ?? null,
  });
}

export async function missionDetail(slugOrId: string): Promise<MissionDetail> {
  return invoke<MissionDetail>("mission_detail", { slugOrId });
}

export async function missionSetCategory(
  slugOrId: string,
  category: MissionCategory
): Promise<MissionDetail> {
  return invoke<MissionDetail>("mission_set_category", { slugOrId, category });
}

export async function missionSetState(
  slugOrId: string,
  state: MissionState
): Promise<MissionDetail> {
  return invoke<MissionDetail>("mission_set_state", { slugOrId, state });
}
