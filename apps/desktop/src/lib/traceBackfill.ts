// traceBackfill.ts — On login/re-login, upload local traces to cloud.
//
// Solves two problems:
// 1. New Pro users get day-1 analytics from their existing local data
// 2. Users who were logged out don't lose traces to the cloud
//
// Runs once per login. Reads the last 30 days from the local Rust
// backend (agent-logs.jsonl), deduplicates against cloud, and
// batch-uploads the missing ones. Non-blocking — failures are silent.

import { invoke } from "@tauri-apps/api/core";
import { useAuthStore } from "@/hooks/useAuth";
import { tierMeetsRequirement } from "@/lib/tier";
import type { AgentTraceInput } from "@/lib/agentTraceUpload";
import type { AgentTraceLine } from "@/lib/agentObservability";

const CLOUD_API_URL =
  (import.meta.env.VITE_CLOUD_API_URL as string | undefined) || "https://api.agentictool.ai";

const BACKFILL_KEY = "ato.trace.lastBackfillAt";
const BATCH_SIZE = 50;

/** Convert a local trace line to the cloud upload shape. */
function toTraceInput(t: AgentTraceLine): AgentTraceInput | null {
  if (!t.slug || !t.ts) return null;
  return {
    agentSlug: t.slug,
    runtime: t.runtime ?? "unknown",
    startedAt: t.ts,
    durationMs: t.durationMs ?? 0,
    ok: t.ok ?? true,
    promptTokens: (t as Record<string, unknown>).promptTokens as number | undefined,
    responseTokens: (t as Record<string, unknown>).responseTokens as number | undefined,
    costUsd: (t as Record<string, unknown>).costUsd as number | undefined,
    error: t.error ?? undefined,
    source: "backfill",
    promptSummary: t.promptPreview?.slice(0, 200),
    filesTouched: (t as Record<string, unknown>).filesTouched as string[] | undefined,
    metadata: { backfilledAt: new Date().toISOString() },
  };
}

/** Run the backfill. Call once after login completes. */
export async function backfillLocalTraces(): Promise<void> {
  const { isCloudUser, accessToken, tier } = useAuthStore.getState();
  if (!isCloudUser || !accessToken) return;
  if (!tierMeetsRequirement(tier, "pro")) return;

  // Don't backfill more than once per 6 hours
  const lastBackfill = localStorage.getItem(BACKFILL_KEY);
  if (lastBackfill) {
    const elapsed = Date.now() - new Date(lastBackfill).getTime();
    if (elapsed < 6 * 60 * 60 * 1000) return;
  }

  try {
    console.log("[trace-backfill] starting — reading local traces from last 30 days");

    // Read local traces from Rust backend
    const since = new Date(Date.now() - 30 * 24 * 60 * 60 * 1000).toISOString();
    const localTraces = await invoke<AgentTraceLine[]>("read_agent_traces", {
      filter: {
        agentSlug: null,
        runtime: null,
        status: null,
        since,
        limit: 1000, // cap at 1000 to avoid huge uploads
      },
    });

    if (localTraces.length === 0) {
      console.log("[trace-backfill] no local traces to backfill");
      localStorage.setItem(BACKFILL_KEY, new Date().toISOString());
      return;
    }

    // Convert to upload format, filter out nulls
    const traces = localTraces
      .map(toTraceInput)
      .filter((t): t is AgentTraceInput => t !== null);

    if (traces.length === 0) {
      localStorage.setItem(BACKFILL_KEY, new Date().toISOString());
      return;
    }

    console.log(`[trace-backfill] uploading ${traces.length} local traces to cloud`);

    // Upload in batches
    let uploaded = 0;
    for (let i = 0; i < traces.length; i += BATCH_SIZE) {
      const chunk = traces.slice(i, i + BATCH_SIZE);
      try {
        const resp = await fetch(`${CLOUD_API_URL}/api/agent-traces`, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${accessToken}`,
          },
          body: JSON.stringify({ traces: chunk }),
        });
        if (resp.ok) {
          const body = await resp.json() as { data?: { inserted?: number } };
          uploaded += body.data?.inserted ?? chunk.length;
        }
      } catch {
        // Continue with next batch — best effort
      }
    }

    localStorage.setItem(BACKFILL_KEY, new Date().toISOString());
    console.log(`[trace-backfill] done — uploaded ${uploaded}/${traces.length} traces`);
  } catch (err) {
    console.error("[trace-backfill] failed:", err);
  }
}
