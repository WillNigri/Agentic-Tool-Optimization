// v2.1.x — Local-dev mock fixtures for cloud-fetched data.
//
// Beatriz can't sign in to ato-cloud from `npm run dev:desktop`; the
// Pro UIs that depend on cloud queries (Insights → External /
// Regressions / Cost, History tab, FileHistory + Pipeline + Compare
// modals) all show the sign-in wall in local dev which blocks
// verification.
//
// Toggle with VITE_USE_MOCK_CLOUD=true. The cloud wrapper modules
// (cloudAgentTraces.ts, cloudConfigChanges.ts) check `isMockMode()`
// at call time and short-circuit to these fixtures. Components that
// gate on `canQueryCloudTraces()` accept mock mode as a valid auth
// state so the sign-in walls don't block.
//
// What's mocked:
//   - 4 agents: code-writer, security-reviewer, doc-summarizer (all
//     external) + history-demo (internal)
//   - ~12 traces across the agents with realistic timing / files /
//     overlap / pipeline relations
//   - 4 config changes (genesis + model swap + hook add + memory
//     policy change)
//   - 1 regression (model swap on @code-writer that dropped ok rate)
//   - 1 outlier in cost benchmarks (@doc-summarizer on Sonnet $$)
//   - 1 pipeline of 2 stages
//   - 1 file (`src/auth.ts`) touched by 3 different traces

import type { CloudAgentTrace, CloudAgentTraceMetric, RegressionRow, CostBenchmarkRow } from "./cloudAgentTraces";
import type { ConfigChange } from "./cloudConfigChanges";
import type { OverlapEvidence } from "./activeRuns";

/** True when the desktop should serve mock cloud data instead of
 *  hitting the network. Read at every call site so build-time vs
 *  runtime confusion doesn't trip people up. */
export function isMockMode(): boolean {
  return import.meta.env.VITE_USE_MOCK_CLOUD === "true";
}

/** Stable parent_run_id for the pipeline fixture so getPipelineTraces
 *  can filter against it. */
export const MOCK_PIPELINE_PARENT_ID = "00000000-0000-4000-8000-000000000001";

const NOW = Date.now();
const HOUR = 3_600_000;

/** Helper to build an ISO timestamp `n` hours ago. */
function hoursAgo(n: number): string {
  return new Date(NOW - n * HOUR).toISOString();
}

/** A single "successful Claude run" fixture template. Other traces
 *  derive from this by overriding fields. Keeps the fixture noise
 *  down. */
function trace(over: Partial<CloudAgentTrace>): CloudAgentTrace {
  return {
    id: over.id ?? `trace-${Math.random().toString(36).slice(2, 11)}`,
    user_id: "mock-user",
    agent_slug: "code-writer",
    runtime: "claude",
    started_at: hoursAgo(2),
    duration_ms: 4200,
    ok: true,
    routed_to: null,
    variables: null,
    hooks_fired: null,
    prompt_tokens: 1840,
    response_tokens: 612,
    cost_usd: 0.0142,
    error: null,
    source: "desktop:promptbar:stream",
    metadata: null,
    files_touched: null,
    prompt_summary: null,
    parent_run_id: null,
    ...over,
  };
}

export const MOCK_TRACES: CloudAgentTrace[] = [
  // Pipeline — 2 stages sharing parent_run_id. Renders the pipeline
  // visualizer and the ↪ pipeline link.
  trace({
    id: "trace-pipe-1",
    agent_slug: "code-writer",
    runtime: "claude",
    started_at: hoursAgo(1),
    duration_ms: 3400,
    cost_usd: 0.0098,
    files_touched: ["src/auth.ts", "src/auth.test.ts"],
    prompt_summary: "Add token rotation to the auth middleware.",
    parent_run_id: MOCK_PIPELINE_PARENT_ID,
    metadata: { stageIndex: 0, totalStages: 2, groupSlug: "write-and-review" },
  }),
  trace({
    id: "trace-pipe-2",
    agent_slug: "security-reviewer",
    runtime: "codex",
    started_at: new Date(NOW - 1 * HOUR + 3500).toISOString(),
    duration_ms: 6100,
    cost_usd: 0.0076,
    files_touched: ["src/auth.ts"],
    prompt_summary: "Review the previous output for security issues.",
    parent_run_id: MOCK_PIPELINE_PARENT_ID,
    metadata: { stageIndex: 1, totalStages: 2, groupSlug: "write-and-review" },
  }),
  // Concurrent overlap — two runs in the same workspace, ambiguous
  // attribution. Renders the ⚠ ambiguous badge.
  trace({
    id: "trace-overlap-a",
    agent_slug: "code-writer",
    runtime: "claude",
    started_at: hoursAgo(3),
    duration_ms: 5400,
    cost_usd: 0.0162,
    files_touched: ["src/auth.ts", "src/util.ts"],
    prompt_summary: "Refactor session handling.",
    metadata: {
      concurrentRuns: [
        {
          run_id: "concurrent-1",
          agent_slug: "doc-summarizer",
          runtime: "gemini",
          started_at_unix: Math.floor((NOW - 3 * HOUR + 1000) / 1000),
        },
      ],
    },
  }),
  trace({
    id: "trace-overlap-b",
    agent_slug: "doc-summarizer",
    runtime: "gemini",
    started_at: hoursAgo(2.95),
    duration_ms: 5800,
    cost_usd: 0.024,
    files_touched: ["src/util.ts", "docs/auth.md"],
    prompt_summary: "Summarize recent auth changes for the changelog.",
    metadata: {
      concurrentRuns: [
        {
          run_id: "concurrent-2",
          agent_slug: "code-writer",
          runtime: "claude",
          started_at_unix: Math.floor((NOW - 3 * HOUR) / 1000),
        },
      ],
    },
  }),
  // A cluster of code-writer runs spanning a model swap so the
  // regression detector has something to detect.
  trace({
    id: "trace-cw-old-1",
    agent_slug: "code-writer",
    runtime: "claude",
    started_at: hoursAgo(48),
    duration_ms: 3200,
    cost_usd: 0.011,
    files_touched: ["src/api.ts"],
    prompt_summary: "Add input validation to /v1/users.",
  }),
  trace({
    id: "trace-cw-old-2",
    agent_slug: "code-writer",
    runtime: "claude",
    started_at: hoursAgo(40),
    duration_ms: 3600,
    cost_usd: 0.012,
    files_touched: ["src/api.ts"],
    ok: true,
  }),
  trace({
    id: "trace-cw-new-1",
    agent_slug: "code-writer",
    runtime: "claude",
    started_at: hoursAgo(20),
    duration_ms: 4900,
    cost_usd: 0.018,
    files_touched: ["src/api.ts"],
    ok: false,
    error: "Schema validation failed: expected string, got null",
  }),
  trace({
    id: "trace-cw-new-2",
    agent_slug: "code-writer",
    runtime: "claude",
    started_at: hoursAgo(10),
    duration_ms: 5800,
    cost_usd: 0.019,
    files_touched: ["src/api.ts"],
    ok: true,
  }),
  // Doc summarizer — expensive outlier for the cost panel.
  trace({
    id: "trace-doc-1",
    agent_slug: "doc-summarizer",
    runtime: "gemini",
    started_at: hoursAgo(6),
    duration_ms: 8900,
    cost_usd: 0.034,
    files_touched: ["docs/architecture.md"],
  }),
  trace({
    id: "trace-doc-2",
    agent_slug: "doc-summarizer",
    runtime: "gemini",
    started_at: hoursAgo(4),
    duration_ms: 9100,
    cost_usd: 0.036,
  }),
  // Security reviewer — clean baseline.
  trace({
    id: "trace-sec-1",
    agent_slug: "security-reviewer",
    runtime: "codex",
    started_at: hoursAgo(8),
    duration_ms: 5400,
    cost_usd: 0.0042,
  }),
  trace({
    id: "trace-sec-2",
    agent_slug: "security-reviewer",
    runtime: "codex",
    started_at: hoursAgo(5),
    duration_ms: 5100,
    cost_usd: 0.0038,
    files_touched: ["src/auth.ts"],
  }),
];

export const MOCK_METRICS: CloudAgentTraceMetric[] = [
  {
    agent_slug: "code-writer",
    run_count: 5,
    ok_count: 4,
    fail_count: 1,
    prompt_tokens: 9200,
    response_tokens: 3060,
    cost_usd: 0.071,
    p50_ms: 4200,
    p95_ms: 5800,
  },
  {
    agent_slug: "security-reviewer",
    run_count: 3,
    ok_count: 3,
    fail_count: 0,
    prompt_tokens: 5400,
    response_tokens: 1800,
    cost_usd: 0.0156,
    p50_ms: 5400,
    p95_ms: 6100,
  },
  {
    agent_slug: "doc-summarizer",
    run_count: 3,
    ok_count: 3,
    fail_count: 0,
    prompt_tokens: 12600,
    response_tokens: 4200,
    cost_usd: 0.094,
    p50_ms: 8900,
    p95_ms: 9100,
  },
];

export const MOCK_CONFIG_CHANGES: ConfigChange[] = [
  {
    id: "change-1",
    agent_slug: "code-writer",
    field: "created",
    old_value: null,
    new_value: { runtime: "claude", model: "claude-sonnet-4-5", kind: "internal" },
    changed_by: "wizard:beatriz@nigri.io",
    metadata: { genesis: true },
    changed_at: hoursAgo(72),
  },
  {
    id: "change-2",
    agent_slug: "code-writer",
    field: "model",
    old_value: "claude-sonnet-4-5",
    new_value: "claude-sonnet-4-6",
    changed_by: "desktop:beatriz@nigri.io",
    metadata: {},
    changed_at: hoursAgo(30),
  },
  {
    id: "change-3",
    agent_slug: "code-writer",
    field: "hooks",
    old_value: [],
    new_value: [{ kind: "file", path: "~/CLAUDE.md", maxBytes: 16384 }],
    changed_by: "desktop:beatriz@nigri.io",
    metadata: {},
    changed_at: hoursAgo(15),
  },
  {
    id: "change-4",
    agent_slug: "history-demo",
    field: "created",
    old_value: null,
    new_value: { runtime: "claude", model: "claude-sonnet-4-6", kind: "internal" },
    changed_by: "wizard:beatriz@nigri.io",
    metadata: { genesis: true },
    changed_at: hoursAgo(0.1),
  },
];

export const MOCK_REGRESSIONS: RegressionRow[] = [
  {
    change_id: "change-2",
    agent_slug: "code-writer",
    field: "model",
    old_value: "claude-sonnet-4-5",
    new_value: "claude-sonnet-4-6",
    changed_at: hoursAgo(30),
    before_runs: 2,
    before_ok_rate: 1.0,
    before_p95_ms: 3600,
    before_cost_per_run: 0.0115,
    after_runs: 2,
    after_ok_rate: 0.5,
    after_p95_ms: 5800,
    after_cost_per_run: 0.0185,
    ok_delta_pp: -50,
    p95_delta_pct: 61.1,
    cost_delta_pct: 60.9,
    severity: "regression",
  },
];

export const MOCK_COST_BENCHMARKS: CostBenchmarkRow[] = [
  {
    agent_slug: "doc-summarizer",
    runtime: "gemini",
    runs: 3,
    ok_runs: 3,
    cost_per_ok: 0.0313,
    cost_per_run: 0.0313,
    total_cost_usd: 0.094,
    p50_ms: 8900,
    is_outlier: true,
  },
  {
    agent_slug: "code-writer",
    runtime: "claude",
    runs: 5,
    ok_runs: 4,
    cost_per_ok: 0.0142,
    cost_per_run: 0.0142,
    total_cost_usd: 0.071,
    p50_ms: 4200,
    is_outlier: false,
  },
  {
    agent_slug: "security-reviewer",
    runtime: "codex",
    runs: 3,
    ok_runs: 3,
    cost_per_ok: 0.0052,
    cost_per_run: 0.0052,
    total_cost_usd: 0.0156,
    p50_ms: 5400,
    is_outlier: false,
  },
];

export const MOCK_OVERLAP_EVIDENCE: OverlapEvidence = {
  overlapped_with: [
    {
      run_id: "concurrent-1",
      agent_slug: "doc-summarizer",
      runtime: "gemini",
      started_at_unix: Math.floor((NOW - 3 * HOUR + 1000) / 1000),
    },
  ],
};

export const MOCK_EMBED_KEY = "eba_MOCK1234ABCDEFGHJKLMNPQRSTUVWXYZ23";

/** Look up a single trace by id from the fixture set. */
export function mockTraceById(id: string): CloudAgentTrace | null {
  return MOCK_TRACES.find((t) => t.id === id) ?? null;
}

/** Filter the fixture set for the trace list endpoint. */
export function mockTraces(opts?: {
  agentSlug?: string;
  file?: string;
  limit?: number;
}): CloudAgentTrace[] {
  let out = [...MOCK_TRACES];
  if (opts?.agentSlug) out = out.filter((t) => t.agent_slug === opts.agentSlug);
  if (opts?.file) out = out.filter((t) => (t.files_touched ?? []).includes(opts.file!));
  out.sort((a, b) => b.started_at.localeCompare(a.started_at));
  if (opts?.limit) out = out.slice(0, opts.limit);
  return out;
}
