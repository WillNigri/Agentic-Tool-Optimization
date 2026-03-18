// ---------------------------------------------------------------------------
// Cron Monitoring Types
// ---------------------------------------------------------------------------

export type AgentRuntime = "claude" | "codex" | "openclaw" | "hermes";

export type CronJobStatus =
  | "healthy"
  | "failed"
  | "silent-failure"
  | "warning"
  | "paused";

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
