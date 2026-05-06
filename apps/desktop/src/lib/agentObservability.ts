import { invoke } from "@tauri-apps/api/core";
import { runLlmJudge, JudgeError } from "@/lib/agentJudge";

// v1.4.0 F6 + F7 — Frontend wrappers for agent observability + evaluators.

export interface AgentTraceLine {
  ts?: string;
  durationMs?: number;
  runtime?: string;
  slug?: string;
  filePath?: string;
  promptPreview?: string;
  responsePreview?: string;
  ok?: boolean;
  error?: string;
  source?: string;
  /** v1.4 F4 — set when this dispatch was a group routed through its router. */
  routedTo?: string;
  /** Anything else that ends up in the JSON line. */
  [extra: string]: unknown;
}

export interface AgentTraceFilter {
  agentSlug?: string;
  runtime?: string;
  status?: "all" | "ok" | "error";
  /** ISO-8601; only return traces with `ts >= since`. */
  since?: string;
  limit?: number;
}

export interface PerAgentMetrics {
  slug: string;
  runtime: string | null;
  totalRuns: number;
  successful: number;
  failed: number;
  successRate: number;
  p50LatencyMs: number | null;
  p95LatencyMs: number | null;
  lastRunAt: string | null;
}

export interface AgentMetrics {
  totalRuns: number;
  successful: number;
  failed: number;
  successRate: number;
  p50LatencyMs: number | null;
  p95LatencyMs: number | null;
  avgLatencyMs: number | null;
  perAgent: PerAgentMetrics[];
}

export async function readAgentTraces(filter: AgentTraceFilter = {}): Promise<AgentTraceLine[]> {
  return invoke<AgentTraceLine[]>("read_agent_traces", {
    filter: {
      agentSlug: filter.agentSlug ?? null,
      runtime: filter.runtime ?? null,
      status: filter.status ?? null,
      since: filter.since ?? null,
      limit: filter.limit ?? null,
    },
  });
}

export async function getAgentMetrics(filter: AgentTraceFilter = {}): Promise<AgentMetrics> {
  return invoke<AgentMetrics>("get_agent_metrics", {
    filter: {
      agentSlug: filter.agentSlug ?? null,
      runtime: filter.runtime ?? null,
      status: filter.status ?? null,
      since: filter.since ?? null,
      limit: filter.limit ?? null,
    },
  });
}

// ── Evaluators ────────────────────────────────────────────────────────────

export type EvaluatorKind =
  | "contains"        // Free
  | "not-contains"    // Free
  | "length-range"    // Free
  | "tool-called"     // Free
  | "llm-judge";      // Pro

export const FREE_EVALUATOR_KINDS: EvaluatorKind[] = [
  "contains",
  "not-contains",
  "length-range",
  "tool-called",
];

export const PRO_EVALUATOR_KINDS: EvaluatorKind[] = ["llm-judge"];

export interface AgentEvaluator {
  id: string;
  agentSlug: string;
  name: string;
  kind: EvaluatorKind;
  configJson: string;
  enabled: boolean;
  createdAt: string;
}

export type EvaluatorConfig =
  | { kind: "contains"; needle: string; caseSensitive?: boolean }
  | { kind: "not-contains"; needle: string; caseSensitive?: boolean }
  | { kind: "length-range"; min: number; max: number }
  | { kind: "tool-called"; tool: string }
  | { kind: "llm-judge"; prompt: string; model?: string };

export function parseEvaluatorConfig(e: AgentEvaluator): EvaluatorConfig {
  try {
    const obj = JSON.parse(e.configJson) as Record<string, unknown>;
    return { kind: e.kind, ...obj } as EvaluatorConfig;
  } catch {
    return { kind: e.kind } as EvaluatorConfig;
  }
}

export function evaluatorConfigToJson(cfg: EvaluatorConfig): string {
  const { kind: _kind, ...rest } = cfg as EvaluatorConfig & { kind: string };
  return JSON.stringify(rest);
}

export interface EvaluationResult {
  evaluatorId: string;
  kind: string;
  verdict: "pass" | "fail" | "partial" | "unknown";
  score: number; // 0-1
  reason: string;
}

export interface EvaluatedTrace {
  trace: AgentTraceLine;
  results: EvaluationResult[];
}

export async function listAgentEvaluators(agentSlug: string): Promise<AgentEvaluator[]> {
  return invoke<AgentEvaluator[]>("list_agent_evaluators", { agentSlug });
}

export async function saveAgentEvaluator(input: {
  id?: string;
  agentSlug: string;
  name: string;
  kind: EvaluatorKind;
  configJson: string;
  enabled?: boolean;
}): Promise<AgentEvaluator> {
  return invoke<AgentEvaluator>("save_agent_evaluator", {
    id: input.id ?? null,
    agentSlug: input.agentSlug,
    name: input.name,
    kind: input.kind,
    configJson: input.configJson,
    enabled: input.enabled ?? true,
  });
}

export async function deleteAgentEvaluator(id: string): Promise<void> {
  return invoke("delete_agent_evaluator", { id });
}

export async function evaluateRecentTraces(
  agentSlug: string,
  lastN: number
): Promise<EvaluatedTrace[]> {
  const out = await invoke<EvaluatedTrace[]>("evaluate_recent_traces", {
    agentSlug,
    lastN,
  });
  // Post-process LLM-judge results — Rust returns "unknown" placeholders for
  // llm-judge kind because the real call needs the cloud Pro endpoint.
  return enrichWithLlmJudge(agentSlug, out);
}

/** Looks up llm-judge evaluators in the trace results and replaces their
 *  "unknown" placeholder with a real cloud verdict. Best-effort: when the
 *  cloud call fails (offline / not Pro), leaves the placeholder in place. */
async function enrichWithLlmJudge(
  agentSlug: string,
  evaluated: EvaluatedTrace[]
): Promise<EvaluatedTrace[]> {
  // Pull live evaluator configs once — we need the judge prompt per
  // evaluator. The Rust output gives us only verdict shape, not prompt.
  let evaluators: AgentEvaluator[];
  try {
    evaluators = await listAgentEvaluators(agentSlug);
  } catch {
    return evaluated;
  }
  const judgeById = new Map<string, EvaluatorConfig & { kind: "llm-judge" }>();
  for (const e of evaluators) {
    if (e.kind === "llm-judge") {
      const cfg = parseEvaluatorConfig(e);
      if (cfg.kind === "llm-judge") judgeById.set(e.id, cfg);
    }
  }
  if (judgeById.size === 0) return evaluated;

  // Run each judge result in parallel — typical batches are small.
  return Promise.all(
    evaluated.map(async (t) => {
      const newResults = await Promise.all(
        t.results.map(async (r) => {
          if (r.kind !== "llm-judge") return r;
          const cfg = judgeById.get(r.evaluatorId);
          if (!cfg) return r;
          try {
            const verdict = await runLlmJudge({
              judgePrompt: cfg.prompt,
              userMessage: String(t.trace.promptPreview ?? ""),
              agentResponse: String(t.trace.responsePreview ?? ""),
            });
            return {
              evaluatorId: r.evaluatorId,
              kind: r.kind,
              verdict: verdict.verdict,
              score: verdict.score,
              reason: verdict.reason,
            };
          } catch (err) {
            // Surface the upgrade prompt as the reason when tier blocks it.
            const message =
              err instanceof JudgeError ? err.message : err instanceof Error ? err.message : String(err);
            return { ...r, reason: message };
          }
        })
      );
      return { trace: t.trace, results: newResults };
    })
  );
}
