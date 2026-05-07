import { useAuthStore } from "@/hooks/useAuth";
import { tierMeetsRequirement } from "@/lib/tier";

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
  metadata?: Record<string, unknown>;
}

const CLOUD_API_URL =
  (import.meta.env.VITE_CLOUD_API_URL as string | undefined) || "https://api.agentictool.ai";

export async function uploadAgentTrace(trace: AgentTraceInput): Promise<void> {
  const { isCloudUser, accessToken, tier } = useAuthStore.getState();
  if (!isCloudUser || !accessToken) return;
  if (!tierMeetsRequirement(tier, "pro")) return;

  try {
    await fetch(`${CLOUD_API_URL}/api/agent-traces`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${accessToken}`,
      },
      body: JSON.stringify({ traces: [trace] }),
    });
  } catch {
    // Local log already has it. Silent failure is the right call here —
    // never let observability upload break the dispatch path.
  }
}

/** Batch upload helper. The cloud endpoint accepts up to 100 per request. */
export async function uploadAgentTraces(traces: AgentTraceInput[]): Promise<void> {
  if (traces.length === 0) return;
  const { isCloudUser, accessToken, tier } = useAuthStore.getState();
  if (!isCloudUser || !accessToken) return;
  if (!tierMeetsRequirement(tier, "pro")) return;

  // Chunk to 100.
  for (let i = 0; i < traces.length; i += 100) {
    const chunk = traces.slice(i, i + 100);
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
