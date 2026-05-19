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
import {
  isMockMode,
  mockTraces,
  mockTraceById,
  MOCK_METRICS,
  MOCK_REGRESSIONS,
  MOCK_COST_BENCHMARKS,
  MOCK_COST_RECOMMENDATIONS,
  MOCK_PIPELINE_PARENT_ID,
  MOCK_PROVIDER_USAGE,
  MOCK_PROVIDER_USAGE_TIMELINE,
} from "@/lib/cloudMockData";

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
  // 2026-05-20 — Optional response text; the cloud backend may carry
  // inline response on Pro plans, otherwise consumers fetch the
  // response via getReplayJob / similar. TraceCompareModal's
  // `if (!baselineTrace.response)` is the documented fallback pattern.
  response?: string | null;
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
    if (r.status === 401 || r.status === 403) {
      // v2.1.10 — Same recovery path as agentTraceUpload. Without this
      // every Pro GET (Compare, Pipelines, Regressions, Cost) would
      // silent-fail and the user would have no clue why panels are
      // empty. logout() preserves local mode (isAuthenticated stays
      // true with the localUser) but flips isCloudUser=false so
      // bottom-left flips back to "Sign in for Pro".
      console.warn(`[cloud] ${path}: ${r.status} — session expired, logging out`);
      useAuthStore.getState().logout();
      return null;
    }
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
  if (isMockMode()) return { metrics: MOCK_METRICS, days };
  return cloudGet<{ metrics: CloudAgentTraceMetric[]; days: number }>(
    `/api/agent-traces/metrics?days=${days}`,
  );
}

/** Recent individual traces — used for the drill-down explorer. */
export async function getAgentTraces(
  agentSlug?: string,
  limit = 50,
): Promise<{ traces: CloudAgentTrace[] } | null> {
  if (isMockMode()) {
    return { traces: mockTraces({ agentSlug, limit }) };
  }
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
  if (isMockMode()) {
    return { traces: mockTraces({ file: filePath, limit }) };
  }
  const params = new URLSearchParams();
  params.set("file", filePath);
  params.set("limit", String(limit));
  return cloudGet<{ traces: CloudAgentTrace[] }>(
    `/api/agent-traces?${params.toString()}`,
  );
}

/** v2.1.0 Phase 8 — Cost optimization data.
 *
 *  Per-(agent_slug, runtime) cost stats over a window. Outlier flag
 *  fires when a row's cost-per-success is ≥2× the user's median (and
 *  has at least 2× the minimum sample size, so noise doesn't trip
 *  it). Frontend renders sorted by cost descending. */
export interface CostBenchmarkRow {
  agent_slug: string;
  runtime: string;
  runs: number;
  ok_runs: number;
  cost_per_ok: number;
  cost_per_run: number;
  total_cost_usd: number;
  p50_ms: number;
  is_outlier: boolean;
}
export async function getCostBenchmarks(opts?: {
  days?: number;
  minRuns?: number;
}): Promise<{
  rows: CostBenchmarkRow[];
  medianCostPerOk: number;
  days: number;
  minRuns: number;
} | null> {
  if (isMockMode()) {
    // Median of fixture cost_per_ok = code-writer's 0.0142 (middle of
    // sorted [0.0052, 0.0142, 0.0313]).
    return {
      rows: MOCK_COST_BENCHMARKS,
      medianCostPerOk: 0.0142,
      days: opts?.days ?? 30,
      minRuns: opts?.minRuns ?? 10,
    };
  }
  const params = new URLSearchParams();
  if (opts?.days) params.set("days", String(opts.days));
  if (opts?.minRuns) params.set("minRuns", String(opts.minRuns));
  return cloudGet<{
    rows: CostBenchmarkRow[];
    medianCostPerOk: number;
    days: number;
    minRuns: number;
  }>(`/api/agent-traces/cost-benchmarks?${params.toString()}`);
}

/** v2.1.0 Phase 9 — Eval workbench (compare). Fetches a single trace
 *  by id so the side-by-side view can show two traces against each
 *  other. We could batch this server-side, but two parallel GETs are
 *  fine at human-trigger volume and the cache lines them up.
 *
 *  Returns null when not signed in / blocked. */
export async function getTraceById(id: string): Promise<CloudAgentTrace | null> {
  if (isMockMode()) return mockTraceById(id);
  // No dedicated /traces/:id endpoint today — query the list endpoint
  // with a high limit and filter client-side. The list is already
  // user-scoped and limit-bounded; fine until we hit a workload that
  // blows past 500 recent traces, at which point the right move is
  // a real /traces/:id endpoint.
  const data = await getAgentTraces(undefined, 500);
  if (!data) return null;
  return data.traces.find((t) => t.id === id) ?? null;
}

/** v2.1.0 Phase 7 — Pipeline visualizer fetch. Returns every stage of
 *  a multi-stage dispatch (sequential pipeline / routed group) by
 *  parent_run_id, ordered started_at ascending so the UI can render
 *  Claude → Codex → Gemini as a flow. */
export async function getPipelineTraces(
  parentRunId: string,
): Promise<{ stages: CloudAgentTrace[]; parentRunId: string } | null> {
  if (isMockMode()) {
    // Mock pipeline only exists for the canonical fixture parent_id.
    // Anything else returns empty so the modal renders the empty state.
    const stages =
      parentRunId === MOCK_PIPELINE_PARENT_ID
        ? mockTraces().filter((t) => t.parent_run_id === MOCK_PIPELINE_PARENT_ID)
        : [];
    stages.sort((a, b) => a.started_at.localeCompare(b.started_at));
    return { stages, parentRunId };
  }
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
  /** v2.1 Phase 5b — avg evaluator score over the before window.
   *  null when no evaluators ran. */
  before_eval_score: number | null;
  before_eval_count: number;
  after_runs: number;
  after_ok_rate: number;
  after_p95_ms: number;
  after_cost_per_run: number;
  after_eval_score: number | null;
  after_eval_count: number;
  ok_delta_pp: number;
  p95_delta_pct: number;
  cost_delta_pct: number;
  /** v2.1 Phase 5b — eval score delta in percentage points (a 0.91 → 0.74
   *  drop is -17). null when neither side has evaluators. */
  eval_delta_pp: number | null;
  /** v2.1 Phase 5b — up to 10 post-change trace IDs that ok=false.
   *  Used by the drill-down modal to show actual failing examples
   *  rather than just an aggregate delta. */
  failing_trace_ids: string[];
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
  if (isMockMode()) {
    return {
      regressions: MOCK_REGRESSIONS,
      windowHours: opts?.windowHours ?? 168,
      minSamples: opts?.minSamples ?? 20,
      days: opts?.days ?? 30,
    };
  }
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

/** v2.1 Phase 5 — Cost recommendations.
 *
 *  Prescriptive companion to cost-benchmarks. Returns same-agent swaps
 *  where the user already has historical data on both runtimes and the
 *  alternative is meaningfully cheaper without quality loss. */
export interface CostRecommendation {
  agent_slug: string;
  current_runtime: string;
  current_runs: number;
  current_cost_per_run: number;
  current_ok_rate: number;
  current_eval_score: number | null;
  suggested_runtime: string;
  suggested_runs: number;
  suggested_cost_per_run: number;
  suggested_ok_rate: number;
  suggested_eval_score: number | null;
  savings_per_run_usd: number;
  savings_window_usd: number;
  savings_pct: number;
  projected_monthly_usd: number;
}

export async function getCostRecommendations(opts?: {
  days?: number;
  minRuns?: number;
}): Promise<{
  recommendations: CostRecommendation[];
  days: number;
  minRuns: number;
} | null> {
  if (isMockMode()) {
    return {
      recommendations: MOCK_COST_RECOMMENDATIONS,
      days: opts?.days ?? 30,
      minRuns: opts?.minRuns ?? 10,
    };
  }
  const params = new URLSearchParams();
  if (opts?.days) params.set("days", String(opts.days));
  if (opts?.minRuns) params.set("minRuns", String(opts.minRuns));
  return cloudGet<{
    recommendations: CostRecommendation[];
    days: number;
    minRuns: number;
  }>(`/api/agent-traces/cost-recommendations?${params.toString()}`);
}

/** Returns true when the cloud features are usable from this client.
 *  In mock mode (`VITE_USE_MOCK_CLOUD=true`) returns true without
 *  any auth state — lets local dev verify Pro UIs without sign-in. */
export function canQueryCloudTraces(): boolean {
  if (isMockMode()) return true;
  const { isCloudUser, accessToken } = useAuthStore.getState();
  return Boolean(isCloudUser && accessToken);
}

// ──────────────────────────────────────────────────────────────────
// v2.6 PR-B chunk 5 — cloud-polled provider usage rows.
//
// Fetches the rows that ato-cloud's usage-poller cron materialized
// from each provider's usage API. These are AUTHORITATIVE — they
// reflect what the provider's own books say the user spent, including
// activity from machines / browsers / phones that the local watcher
// (PR-A) cannot see.
//
// Pairs with the local `getBillingSurfaceSummary` in @/lib/localInsights —
// the Usage tab renders both as separate group-by views, so users can
// triangulate "what I saw firing locally" vs "what the provider says
// they billed me for." Discrepancies > a few percent = activity from
// non-observed surfaces (web UI, phone, other machines).
// ──────────────────────────────────────────────────────────────────

/** One aggregate row from /api/analytics/provider-usage. */
export interface ProviderUsageRow {
  provider: string;
  total_requests: number | string;
  total_tokens_in: number | string;
  total_tokens_out: number | string;
  total_cost_usd: number | string | null;
  rows_polled: number;
}

/** One day's slice from /api/analytics/provider-usage/timeline. */
export interface ProviderUsageTimelinePoint {
  date: string; // YYYY-MM-DD
  requests: number | string;
  tokens_in: number | string;
  tokens_out: number | string;
  cost_usd: number | string | null;
}

/** Provider totals over the past N days. Returns null when not signed in. */
export async function getProviderUsage(opts?: { days?: number }): Promise<
  | {
      rows: ProviderUsageRow[];
      days: number;
    }
  | null
> {
  const days = opts?.days ?? 30;
  if (isMockMode()) {
    return { rows: MOCK_PROVIDER_USAGE, days };
  }
  const params = new URLSearchParams();
  params.set("days", String(days));
  const data = await cloudGet<ProviderUsageRow[]>(
    `/api/analytics/provider-usage?${params.toString()}`,
  );
  if (!data) return null;
  return { rows: data, days };
}

/** Daily timeline for a single provider over the past N days. */
export async function getProviderUsageTimeline(opts: {
  days?: number;
  provider: string;
}): Promise<{ rows: ProviderUsageTimelinePoint[]; days: number; provider: string } | null> {
  const days = opts.days ?? 30;
  if (isMockMode()) {
    return {
      rows:
        MOCK_PROVIDER_USAGE_TIMELINE[opts.provider] ??
        MOCK_PROVIDER_USAGE_TIMELINE.openai ??
        [],
      days,
      provider: opts.provider,
    };
  }
  const params = new URLSearchParams();
  params.set("days", String(days));
  params.set("provider", opts.provider);
  const data = await cloudGet<ProviderUsageTimelinePoint[]>(
    `/api/analytics/provider-usage/timeline?${params.toString()}`,
  );
  if (!data) return null;
  return { rows: data, days, provider: opts.provider };
}
