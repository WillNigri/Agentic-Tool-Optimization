/**
 * Hooks that aggregate data from all runtimes (Claude, OpenClaw, Hermes, Codex)
 * into unified views for Cron Monitor, Skills, and Subagents.
 */

import { useQuery } from "@tanstack/react-query";
import type { CronJob, AgentRuntime } from "@/components/cron/types";

const isTauri =
  typeof window !== "undefined" &&
  ("__TAURI__" in window || "__TAURI_INTERNALS__" in window);

import * as tauriApiModule from "@/lib/tauri-api";

function tauriApiSync() {
  return tauriApiModule;
}

// ---------------------------------------------------------------------------
// OpenClaw cron job normalization
// ---------------------------------------------------------------------------

interface OpenClawCronJob {
  id?: string;
  name?: string;
  description?: string;
  schedule?: string;
  enabled?: boolean;
  lastRun?: string;
  nextRun?: string;
  prompt?: string;
  agent?: string;
  delivery?: string;
}

function normalizeOpenClawJob(raw: OpenClawCronJob): CronJob {
  const now = new Date().toISOString();
  return {
    id: `oc-${raw.id || raw.name || Math.random().toString(36).slice(2)}`,
    name: raw.name || "Unnamed Job",
    description: raw.description || raw.delivery || "",
    schedule: raw.schedule || "* * * * *",
    runtime: "openclaw" as AgentRuntime,
    prompt: raw.prompt || "",
    enabled: raw.enabled !== false,
    status: raw.enabled !== false ? "healthy" : "paused",
    createdAt: now,
    updatedAt: now,
    lastRunAt: raw.lastRun,
    nextRunAt: raw.nextRun,
    source: "openclaw-gateway",
    readOnly: true,
  };
}

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

/**
 * Fetch and merge cron jobs from all runtimes.
 */
export function useRuntimeCronJobs() {
  // Local ATO + Claude native cron jobs
  const local = useQuery({
    queryKey: ["cron-jobs-local"],
    queryFn: async () => {
      if (!isTauri) return [];
      try {
        const api = tauriApiSync();
        const jobs = await api.listCronJobs();
        return (jobs as CronJob[]).map((j) => ({ ...j, source: "ato" as const }));
      } catch {
        return [];
      }
    },
    refetchInterval: 30_000,
  });

  // OpenClaw gateway cron jobs
  const openclaw = useQuery({
    queryKey: ["cron-jobs-openclaw"],
    queryFn: async () => {
      if (!isTauri) return [];
      try {
        const api = tauriApiSync();
        const result = await api.openclawListCronJobs();
        // OpenClaw returns { jobs: [...] } or an array directly
        const raw = Array.isArray(result)
          ? result
          : (result as Record<string, unknown>)?.jobs ?? [];
        return (raw as OpenClawCronJob[]).map(normalizeOpenClawJob);
      } catch {
        return [];
      }
    },
    refetchInterval: 60_000,
    retry: 1,
  });

  const allJobs = [...(local.data || []), ...(openclaw.data || [])];
  const isLoading = local.isLoading || openclaw.isLoading;

  return {
    jobs: allJobs,
    isLoading,
    openclawConnected: !openclaw.isError && (openclaw.data?.length ?? 0) > 0,
    refetch: () => {
      local.refetch();
      openclaw.refetch();
    },
  };
}

/**
 * Fetch OpenClaw gateway health status.
 */
export function useOpenClawStatus() {
  return useQuery({
    queryKey: ["openclaw-status"],
    queryFn: async () => {
      if (!isTauri) return null;
      try {
        const api = tauriApiSync();
        return await api.openclawGatewayStatus();
      } catch {
        return null;
      }
    },
    refetchInterval: 30_000,
    retry: 1,
  });
}

/**
 * Fetch agents from OpenClaw gateway.
 */
export function useRuntimeAgents() {
  return useQuery({
    queryKey: ["openclaw-agents"],
    queryFn: async () => {
      if (!isTauri) return [];
      try {
        const api = tauriApiSync();
        const result = await api.openclawListAgents();
        const agents = Array.isArray(result)
          ? result
          : (result as Record<string, unknown>)?.agents ?? [];
        return agents;
      } catch {
        return [];
      }
    },
    refetchInterval: 60_000,
    retry: 1,
  });
}

/**
 * Fetch OpenClaw skills status.
 */
export function useOpenClawSkills() {
  return useQuery({
    queryKey: ["openclaw-skills"],
    queryFn: async () => {
      if (!isTauri) return null;
      try {
        const api = tauriApiSync();
        return await api.openclawSkillsStatus();
      } catch {
        return null;
      }
    },
    refetchInterval: 60_000,
    retry: 1,
  });
}
