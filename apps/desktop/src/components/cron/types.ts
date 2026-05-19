// ---------------------------------------------------------------------------
// Cron Monitoring Types
// ---------------------------------------------------------------------------

// 2026-05-18 — AgentRuntime used to be hand-typed as the 4 CLI
// runtimes only. The codebase actively dispatches to gemini /
// minimax / grok / deepseek / qwen / openrouter too; the type lied.
// Re-imported AND re-exported from the canonical runtime registry so
// the rest of this file can reference `AgentRuntime` locally AND
// downstream importers still get it — adding a runtime updates this
// type everywhere for free.
import type { RuntimeId as AgentRuntime } from "@/lib/runtimes";
export type { AgentRuntime };

export type CronJobStatus =
  | "healthy"
  | "failed"
  | "silent-failure"
  | "warning"
  | "paused";

export type CronJobSource = "ato" | "claude-native" | "openclaw-gateway" | "hermes-fs";

export interface CronJob {
  id: string;
  name: string;
  description: string;
  schedule: string; // cron expression, e.g. "0 7 * * *"
  runtime: AgentRuntime;
  prompt: string;
  enabled: boolean;
  status: CronJobStatus;
  linkedWorkflowId?: string;
  runtimeConfig?: RuntimeConfig;
  createdAt: string;
  updatedAt: string;
  lastRunAt?: string;
  nextRunAt?: string;
  source?: CronJobSource;
  readOnly?: boolean; // true for jobs from external runtimes
  /** Preferred dispatch target — use one of these when set, fall back to
   *  raw `runtime` + `prompt` when both are null. */
  agentSlug?: string;
  groupSlug?: string;
  /** v1.5.0 — when true and the OS supports it (macOS today), the job is
   *  registered with the OS scheduler so it fires even when ATO is closed
   *  or the laptop just woke from sleep. */
  wakeFromSleep?: boolean;
}

export interface CronExecution {
  id: string;
  jobId: string;
  startedAt: string;
  finishedAt?: string;
  durationMs?: number;
  status: "running" | "success" | "failed" | "skipped";
  output?: string;
  error?: string;
  retryOf?: string; // ID of the execution this retried
}

export interface CronAlert {
  id: string;
  jobId: string;
  type: CronJobStatus;
  message: string;
  createdAt: string;
  acknowledged: boolean;
}

// ---------------------------------------------------------------------------
// Multi-Agent Runtime Config Types
// ---------------------------------------------------------------------------

export interface OpenClawConfig {
  sshHost: string;
  sshPort: number;
  sshUser?: string;
  sshKeyPath?: string;
}

export interface CodexConfig {
  apiKeyPath?: string;
}

export interface HermesConfig {
  endpoint?: string;
}

export type RuntimeConfig = OpenClawConfig | CodexConfig | HermesConfig;

export interface DetectedRuntime {
  runtime: AgentRuntime;
  available: boolean;
  version?: string;
  path?: string;
}
