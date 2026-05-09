// v2.0.0 Wave 5 — Frontend wrappers for the cloud /api/agent-traces endpoints.
//
// Pro+ desktop clients can read traces from any agent the user owns —
// internal (uploaded by the desktop's own telemetry pipeline) and
// external (POSTed back from deployed Cloudflare/Vercel/Docker/Node
// bundles). This module wraps the GET endpoints; POST is owned by the
// internal telemetry uploader (lib/agentTraceUpload.ts) and the
// deployed bundles themselves.
//
// All these endpoints require:
//   1. JWT auth (the user must be cloud-logged-in)
//   2. Pro tier (free users get a 403)
// Returns null when neither holds — caller renders an upgrade prompt.

import { useAuthStore } from "@/hooks/useAuth";

const CLOUD_API_URL =
  import.meta.env.VITE_CLOUD_API_URL || "https://api.agentictool.ai";

export interface CloudAgentTraceMetric {
  agent_slug: string;
  run_count: number;
  ok_count: number;
  fail_count: number;
  prompt_tokens: number;
  response_tokens: number;
  cost_usd: number;
  p50_ms: number;
  p95_ms: number;
}

export interface CloudAgentTrace {
  id: string;
  user_id: string;
  agent_slug: string;
  runtime: string;
  started_at: string;
  duration_ms: number;
  ok: boolean;
  routed_to: string | null;
  variables: Record<string, unknown> | null;
  hooks_fired: unknown[] | null;
  prompt_tokens: number | null;
  response_tokens: number | null;
  cost_usd: number | null;
  error: string | null;
  source: string | null;
  metadata: Record<string, unknown> | null;
  // v2.1.0 — relative file paths the agent touched during this dispatch.
  // Captured by mtime diff in the desktop layer; null for traces from
  // bundles (the customer-facing deploy doesn't have a "project root").
  files_touched: string[] | null;
  // v2.1.0+ — first ~200 chars of the dispatch prompt. Surfaced in the
  // file-history modal so reviewers can answer "why was this file
  // changed?" not just who/when.
  prompt_summary: string | null;
  // v2.1.0 Phase 7 — When this trace is one stage of a multi-stage
  // dispatch, all stages share the same UUID. The pipeline visualizer
  // groups by this field.
  parent_run_id: string | null;
}

/** Read auth headers from the local store. Returns null if the user
 *  isn't cloud-logged-in or doesn't have a token. */
function authHeaders(): Record<string, string> | null {
  const { isCloudUser, accessToken } = useAuthStore.getState();
  if (!isCloudUser || !accessToken) return null;
  return {
    "Authorization": `Bearer ${accessToken}`,
    "Content-Type": "application/json",
  };
}

async function cloudGet<T>(path: string): Promise<T | null> {
  const headers = authHeaders();
  if (!headers) return null;
  const r = await fetch(`${CLOUD_API_URL}${path}`, { headers });
  if (!r.ok) {
    if (r.status === 401 || r.status === 403) return null;
    throw new Error(`cloud GET ${path}: ${r.status}`);
  }
  const body = await r.json();
  return (body?.data ?? body) as T;
}

/** Per-agent rollup over the past `days` days. Server-side aggregation
 *  via percentile_cont; cheap to call. */
export async function getAgentTraceMetrics(
  days = 30,
): Promise<{ metrics: CloudAgentTraceMetric[]; days: number } | null> {
  return cloudGet<{ metrics: CloudAgentTraceMetric[]; days: number }>(
    `/api/agent-traces/metrics?days=${days}`,
  );
}

/** Recent individual traces — used for the drill-down explorer. */
export async function getAgentTraces(
  agentSlug?: string,
  limit = 50,
): Promise<{ traces: CloudAgentTrace[] } | null> {
  const params = new URLSearchParams();
  if (agentSlug) params.set("agentSlug", agentSlug);
  params.set("limit", String(limit));
  return cloudGet<{ traces: CloudAgentTrace[] }>(
    `/api/agent-traces?${params.toString()}`,
  );
}

/** v2.1.0 — every dispatch that touched a specific file, across all
 *  agents. Powers the "who changed this file when" view. Returns the
 *  full trace records so the modal can show agent + runtime + time +
 *  duration + error per touch. */
export async function getTracesByFile(
  filePath: string,
  limit = 200,
): Promise<{ traces: CloudAgentTrace[] } | null> {
  const params = new URLSearchParams();
  params.set("file", filePath);
  params.set("limit", String(limit));
  return cloudGet<{ traces: CloudAgentTrace[] }>(
    `/api/agent-traces?${params.toString()}`,
  );
}

/** v2.1.0 Phase 7 — Pipeline visualizer fetch. Returns every stage of
 *  a multi-stage dispatch (sequential pipeline / routed group) by
 *  parent_run_id, ordered started_at ascending so the UI can render
 *  Claude → Codex → Gemini as a flow. */
export async function getPipelineTraces(
  parentRunId: string,
): Promise<{ stages: CloudAgentTrace[]; parentRunId: string } | null> {
  return cloudGet<{ stages: CloudAgentTrace[]; parentRunId: string }>(
    `/api/agent-traces/pipeline/${encodeURIComponent(parentRunId)}`,
  );
}

/** v2.1.0 Phase 5 — Cross-runtime regression detection.
 *
 *  For every model / role_models / system_prompt / runtime change in
 *  the window, returns before/after aggregate stats so the dashboard
 *  can flag "switching @reviewer from sonnet-4-6 → 4-7 dropped success
 *  rate from 91% → 74% across 412 conversations." */
export interface RegressionRow {
  change_id: string;
  agent_slug: string;
  field: string;
  old_value: unknown;
  new_value: unknown;
  changed_at: string;
  before_runs: number;
  before_ok_rate: number;
  before_p95_ms: number;
  before_cost_per_run: number;
  after_runs: number;
  after_ok_rate: number;
  after_p95_ms: number;
  after_cost_per_run: number;
  ok_delta_pp: number;
  p95_delta_pct: number;
  cost_delta_pct: number;
  severity: "regression" | "improvement" | "neutral";
}
export async function getRegressions(opts?: {
  days?: number;
  windowHours?: number;
  minSamples?: number;
}): Promise<{
  regressions: RegressionRow[];
  windowHours: number;
  minSamples: number;
  days: number;
} | null> {
  const params = new URLSearchParams();
  if (opts?.days) params.set("days", String(opts.days));
  if (opts?.windowHours) params.set("windowHours", String(opts.windowHours));
  if (opts?.minSamples) params.set("minSamples", String(opts.minSamples));
  return cloudGet<{
    regressions: RegressionRow[];
    windowHours: number;
    minSamples: number;
    days: number;
  }>(`/api/agent-traces/regressions?${params.toString()}`);
}

/** Returns true when the cloud features are usable from this client. */
export function canQueryCloudTraces(): boolean {
  const { isCloudUser, accessToken } = useAuthStore.getState();
  return Boolean(isCloudUser && accessToken);
}
