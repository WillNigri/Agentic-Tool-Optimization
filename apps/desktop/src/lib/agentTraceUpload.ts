import { useAuthStore } from "@/hooks/useAuth";
import { tierMeetsRequirement } from "@/lib/tier";
import { getTrialCohort } from "@/lib/conversionTelemetry";

// v1.4.0 F6 — Pro+ trace upload.
//
// Local logs (~/.ato/agent-logs.jsonl) are the source of truth for the Free
// tier. Pro+ users additionally push each dispatch to the cloud
// `/agent-traces` endpoint so observability follows them across machines and
// retention is enforced server-side.
//
// We POST optimistically and never block the dispatch path on the network —
// failures are silent. The local log already captured the data.

export interface AgentTraceInput {
  agentSlug: string;
  runtime: string;
  startedAt: string;       // ISO datetime
  durationMs: number;
  ok: boolean;
  routedTo?: string;
  variables?: Record<string, unknown>;
  hooksFired?: unknown[];
  promptTokens?: number;
  responseTokens?: number;
  costUsd?: number;
  error?: string;
  source?: string;
  /** Free-form context. Conventional keys (server-side never reads
   *  these; the dashboard does):
   *    - `historyLength: number` — chat-pane streaming context size
   *    - `streamed: boolean` — was this a streaming dispatch
   *    - `origin: string` — for embed traces, the calling page
   *    - `concurrentRuns: OverlapPeer[]` — v2.1.0+ — peers that
   *      overlapped this run's window in the same workspace, marking
   *      file attribution as ambiguous in the dashboard.
   */
  metadata?: Record<string, unknown>;
  /** v2.1.0 — relative file paths touched during this dispatch.
   *  Populated by the desktop's pre/post mtime snapshot; absent for
   *  group-level rollup traces and embed-bundle traces. */
  filesTouched?: string[];
  /** v2.1.0+ — first ~200 chars of the dispatch prompt. Captured at
   *  upload time so the file-history modal can show "what was that run
   *  trying to do?" alongside the agent slug. Truncate with care —
   *  long prompts come pre-trimmed by `summarizePrompt`. */
  promptSummary?: string;
  /** v2.1.0 Phase 7 — When this trace is one stage of a multi-stage
   *  dispatch (sequential pipeline / routed group), all stages share
   *  the same UUID. The pipeline visualizer groups by this field. */
  parentRunId?: string;
}

const CLOUD_API_URL =
  (import.meta.env.VITE_CLOUD_API_URL as string | undefined) || "https://api.agentictool.ai";

export async function uploadAgentTrace(trace: AgentTraceInput): Promise<void> {
  const { isCloudUser, accessToken, tier } = useAuthStore.getState();
  if (!isCloudUser || !accessToken) {
    console.warn("[agent-trace] upload skipped: not signed in to cloud", { isCloudUser, hasToken: !!accessToken });
    return;
  }
  if (!tierMeetsRequirement(tier, "pro")) {
    console.warn("[agent-trace] upload skipped: tier gate failed", { tier });
    return;
  }

  try {
    const response = await fetch(`${CLOUD_API_URL}/api/agent-traces`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${accessToken}`,
      },
      body: JSON.stringify({ traces: [withTrialCohort(trace)] }),
    });
    if (!response.ok) {
      const body = await response.text().catch(() => "<unreadable>");
      console.error("[agent-trace] upload rejected", {
        status: response.status,
        body,
        agentSlug: trace.agentSlug,
        parentRunId: trace.parentRunId,
      });
      // v2.1.10 — when the cloud rejects with 401, the session is dead.
      // Clear tokens + flip isCloudUser=false so the UI surfaces a real
      // "Sign in for Pro" prompt instead of every Pro panel sitting in
      // a silent-empty-state limbo. Without this users hit unbounded
      // 401 spam in DevTools and no visible signal in the app.
      if (response.status === 401 || response.status === 403) {
        useAuthStore.getState().logout();
      }
      return;
    }
    // v2.1.0 Replay link. Cloud returns the inserted trace IDs; we
    // attach the first one to the most-recent matching local
    // execution_logs row by (runtime, ±10s window) so future replay
    // lookups can find the full prompt by trace ID. Best-effort —
    // missing the link only means this trace can't be replayed, not
    // that anything else broke.
    try {
      const body = (await response.json()) as {
        success: boolean;
        data?: { ids?: string[] };
      };
      const insertedId = body?.data?.ids?.[0];
      if (insertedId) {
        const { linkExecutionLogToCloudTrace } = await import("@/lib/tauri-api");
        await linkExecutionLogToCloudTrace(
          insertedId,
          trace.runtime,
          trace.startedAt,
        );
      }
    } catch (linkErr) {
      console.warn("[agent-trace] cloud-trace link failed (replay won't be available for this trace)", linkErr);
    }
  } catch (err) {
    // Local log already has it. Surface to console so we can diagnose
    // when the Pipelines panel comes up empty despite a successful run.
    console.error("[agent-trace] upload threw", err, { agentSlug: trace.agentSlug });
  }
}

/** v2.1.0+ — Compress a raw prompt into the ~200-char summary the
 *  file-history modal displays. Strips a leading <context> block (we
 *  re-inject context every turn — the noise dwarfs the actual ask),
 *  collapses whitespace, then truncates with an ellipsis. Caller
 *  passes the result as `promptSummary` on `uploadAgentTrace`. */
export function summarizePrompt(prompt: string, max = 200): string {
  // Drop a leading <context>...</context> block if present — the
  // pre-call hooks pipeline injects this on every dispatch and it
  // would otherwise dominate the summary.
  let stripped = prompt;
  const ctxMatch = stripped.match(/^<context>[\s\S]*?<\/context>\s*/);
  if (ctxMatch) {
    stripped = stripped.slice(ctxMatch[0].length);
  }
  const collapsed = stripped.replace(/\s+/g, " ").trim();
  if (collapsed.length <= max) return collapsed;
  return collapsed.slice(0, max - 1) + "…";
}

/** Batch upload helper. The cloud endpoint accepts up to 100 per request. */
export async function uploadAgentTraces(traces: AgentTraceInput[]): Promise<void> {
  if (traces.length === 0) return;
  const { isCloudUser, accessToken, tier } = useAuthStore.getState();
  if (!isCloudUser || !accessToken) return;
  if (!tierMeetsRequirement(tier, "pro")) return;

  // Chunk to 100.
  for (let i = 0; i < traces.length; i += 100) {
    const chunk = traces.slice(i, i + 100).map(withTrialCohort);
    try {
      await fetch(`${CLOUD_API_URL}/api/agent-traces`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${accessToken}`,
        },
        body: JSON.stringify({ traces: chunk }),
      });
    } catch {
      // Continue — best effort.
    }
  }
}

/** Strategy PR-B (2026-05-21) A5 amendment — stamp the active trial
 *  cohort onto every uploaded trace. The cohort goes into the existing
 *  `metadata` bag (which the server's zod schema treats as a
 *  `Record<string, unknown>`) so this PR ships without a coordinated
 *  server-side schema change. Top-level addition would have reproduced
 *  the 2026-05-14 RFC3339 silent-strip failure mode.
 *
 *  Cohort is null for users not enrolled — those rows still upload, the
 *  field is just absent. Server-side aggregation should treat absence
 *  as "unenrolled", not "missing data". */
function withTrialCohort(trace: AgentTraceInput): AgentTraceInput {
  const cohort = getTrialCohort();
  if (cohort === null) return trace;
  return {
    ...trace,
    metadata: {
      ...(trace.metadata ?? {}),
      trial_cohort: cohort,
    },
  };
}
