// v2.3.2 Phase 2.x — Wrappers for the local-mode Tauri commands
// (compute_regressions_local + compute_cost_recommendations_local).
//
// Same shape as the cloud responses in cloudAgentTraces.ts — the
// regressions panel and cost-benchmarks panel can render either
// without a fork. The `source` field on the returned object tells the
// UI which dataset it's showing so the "Local mode (this machine)"
// vs "Cloud (cross-device)" badge can render appropriately.

import { invoke } from "@tauri-apps/api/core";
import type { RegressionRow, CostRecommendation } from "./cloudAgentTraces";

export interface LocalRegressionsResult {
  regressions: RegressionRow[];
  window_hours: number;
  min_samples: number;
  days: number;
  /** "local" — regression data computed over this machine's SQLite.
   *  "local-no-schema" — v2.3.2 migration hasn't run yet (older
   *  desktop install before agent_config_changes table existed). */
  source: "local" | "local-no-schema";
}

export interface LocalCostRecsResult {
  recommendations: CostRecommendation[];
  days: number;
  min_runs: number;
  source: "local" | "local-no-schema";
}

/** Local-mode regression detection. Runs entirely over the local
 *  SQLite — no cloud sign-in required. Mirrors the cloud
 *  /api/agent-traces/regressions endpoint. */
export async function getRegressionsLocal(opts?: {
  days?: number;
  windowHours?: number;
  minSamples?: number;
}): Promise<LocalRegressionsResult> {
  return invoke<LocalRegressionsResult>("compute_regressions_local", {
    days: opts?.days ?? null,
    windowHours: opts?.windowHours ?? null,
    minSamples: opts?.minSamples ?? null,
  });
}

/** Local-mode cost recommendations. Same gates as the cloud
 *  endpoint (≥30% cheaper, ok-rate within 10pp), no sign-in needed. */
export async function getCostRecommendationsLocal(opts?: {
  days?: number;
  minRuns?: number;
}): Promise<LocalCostRecsResult> {
  return invoke<LocalCostRecsResult>("compute_cost_recommendations_local", {
    days: opts?.days ?? null,
    minRuns: opts?.minRuns ?? null,
  });
}
