/**
 * Desktop API layer that uses Tauri commands for local data
 * and optionally proxies to the cloud API when sync is enabled.
 */

// Tauri invoke is available at runtime in the desktop app
// In dev/web mode, we fall back to HTTP API calls
const isTauri = '__TAURI__' in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
    return tauriInvoke<T>(cmd, args);
  }
  // Fallback to HTTP API for web/dev mode
  throw new Error(`Tauri not available for command: ${cmd}`);
}

// ---- Context ----
export async function getContextBreakdown() {
  return invoke<{
    totalTokens: number;
    limit: number;
    categories: Array<{ name: string; tokens: number; color: string }>;
  }>('get_context_estimate');
}

// ---- Skills ----
export type SkillScope = 'enterprise' | 'personal' | 'project' | 'plugin';

export interface LocalSkill {
  id: string;
  name: string;
  description: string;
  filePath: string;
  scope: SkillScope;
  tokenCount: number;
  enabled: boolean;
  contentHash: string;
}

export interface SkillDetail extends LocalSkill {
  content: string;
  frontmatter: {
    name: string;
    description: string;
    'argument-hint'?: string;
    'allowed-tools'?: string;          // comma-separated: "Read, Write, Bash(npm run *)"
    'disable-model-invocation'?: boolean;
    'user-invocable'?: boolean;
    model?: string;
    context?: 'fork';
    agent?: string;                    // subagent type when context=fork
    // Legacy parsed convenience field
    allowedTools?: string[];
    [key: string]: unknown;
  };
  hasScripts: boolean;
  hasReferences: boolean;
  hasAssets: boolean;
  scripts: string[];
  references: string[];
  assets: string[];
  isDirectory: boolean;
}

export interface CreateSkillData {
  name: string;
  description: string;
  scope: SkillScope;
  content: string;
  allowedTools?: string[];
  model?: string;
  isDirectory: boolean;
}

export async function getSkills(): Promise<LocalSkill[]> {
  return invoke<LocalSkill[]>('get_local_skills');
}

export async function getSkillDetail(id: string): Promise<SkillDetail> {
  return invoke<SkillDetail>('get_skill_detail', { id });
}

export async function toggleSkill(filePath: string, enabled: boolean): Promise<void> {
  return invoke('toggle_local_skill', { filePath, enabled });
}

export async function updateSkill(id: string, content: string): Promise<void> {
  return invoke('update_skill', { id, content });
}

export async function createSkill(data: CreateSkillData): Promise<SkillDetail> {
  return invoke<SkillDetail>('create_skill', { data });
}

export async function deleteSkill(id: string): Promise<void> {
  return invoke('delete_skill', { id });
}

// ---- Usage ----
export interface UsageSummaryLocal {
  today: { inputTokens: number; outputTokens: number; costCents: number };
  week: { inputTokens: number; outputTokens: number; costCents: number };
  month: { inputTokens: number; outputTokens: number; costCents: number };
}

export async function getUsageSummary(): Promise<UsageSummaryLocal> {
  return invoke<UsageSummaryLocal>('get_local_usage');
}

export interface DailyUsage {
  date: string;
  inputTokens: number;
  outputTokens: number;
}

export async function getDailyUsage(days: number = 30): Promise<DailyUsage[]> {
  return invoke<DailyUsage[]>('get_daily_usage', { days });
}

export interface BurnRateLocal {
  tokensPerHour: number;
  costPerHour: number;
  estimatedHoursToLimit: number | null;
  limit: number | null;
}

export async function getBurnRate(): Promise<BurnRateLocal> {
  return invoke<BurnRateLocal>('get_burn_rate');
}

// ---- MCP Servers ----
export interface LocalMcpServer {
  id: string;
  name: string;
  transport: string;
  status: 'running' | 'stopped' | 'error';
  toolCount: number;
  command?: string;
  url?: string;
}

export async function getMcpServers(): Promise<LocalMcpServer[]> {
  return invoke<LocalMcpServer[]>('get_local_config');
}

export async function restartMcpServer(name: string): Promise<void> {
  return invoke('restart_mcp_server', { name });
}

// ---- Config Files ----
export interface ConfigFile {
  path: string;
  exists: boolean;
  scope: string;
  sizeBytes?: number;
}

export async function getConfigFiles(): Promise<ConfigFile[]> {
  return invoke<ConfigFile[]>('get_config_files');
}

// ---- Sync Settings ----
export interface SyncStatus {
  enabled: boolean;
  lastSyncAt: string | null;
  cloudUrl: string | null;
}

export async function getSyncStatus(): Promise<SyncStatus> {
  return invoke<SyncStatus>('get_sync_status');
}

export async function setSyncEnabled(enabled: boolean, cloudUrl?: string): Promise<void> {
  return invoke('set_sync_enabled', { enabled, cloudUrl });
}

// ---- Workflows (Automation Builder) ----
export interface WorkflowData {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  lastRun?: string;
  runCount: number;
  errorCount: number;
  nodes: Array<{
    id: string;
    label: string;
    description: string;
    type: string;
    service?: string;
    x: number;
    y: number;
    stats: { executions: number; errors: number; avgTimeMs: number };
    status: string;
    config?: { params: Record<string, string>; condition?: string };
  }>;
  edges: Array<{ from: string; to: string; label?: string; animated?: boolean }>;
}

export async function listWorkflows(): Promise<WorkflowData[]> {
  try {
    return await invoke<WorkflowData[]>('list_workflows');
  } catch {
    // localStorage fallback for browser dev mode
    const raw = localStorage.getItem('ato-workflows');
    return raw ? JSON.parse(raw) : [];
  }
}

export async function saveWorkflow(workflow: WorkflowData): Promise<void> {
  try {
    await invoke('save_workflow', { workflow: JSON.stringify(workflow) });
  } catch {
    // localStorage fallback
    const all = await listWorkflows();
    const idx = all.findIndex((w) => w.id === workflow.id);
    if (idx >= 0) all[idx] = workflow;
    else all.push(workflow);
    localStorage.setItem('ato-workflows', JSON.stringify(all));
  }
}

export async function loadWorkflow(id: string): Promise<WorkflowData | null> {
  try {
    return await invoke<WorkflowData>('load_workflow', { id });
  } catch {
    const all = await listWorkflows();
    return all.find((w) => w.id === id) || null;
  }
}

export async function deleteWorkflowFile(id: string): Promise<void> {
  try {
    await invoke('delete_workflow', { id });
  } catch {
    const all = await listWorkflows();
    const filtered = all.filter((w) => w.id !== id);
    localStorage.setItem('ato-workflows', JSON.stringify(filtered));
  }
}

// ---- Claude CLI ----
export async function promptClaude(prompt: string): Promise<string> {
  return invoke<string>('prompt_claude', { prompt });
}

// ---- Multi-Agent Runtime ----
import type { AgentRuntime, DetectedRuntime, RuntimeConfig, CronJob, CronExecution } from '@/components/cron/types';
export type { AgentRuntime, DetectedRuntime, RuntimeConfig };

export async function detectAgentRuntimes(): Promise<DetectedRuntime[]> {
  try {
    return await invoke<DetectedRuntime[]>('detect_agent_runtimes');
  } catch {
    // Fallback: only Claude available
    return [
      { runtime: 'claude', available: true, version: 'CLI' },
      { runtime: 'codex', available: false },
      { runtime: 'openclaw', available: false },
      { runtime: 'hermes', available: false },
    ];
  }
}

export async function promptAgent(
  runtime: AgentRuntime,
  prompt: string,
  config?: RuntimeConfig
): Promise<string> {
  const startTime = Date.now();
  try {
    let result: string;
    try {
      result = await invoke<string>('prompt_agent', { runtime, prompt, config: config ? JSON.stringify(config) : null });
    } catch {
      if (runtime === 'claude') {
        result = await promptClaude(prompt);
      } else {
        throw new Error(`Runtime "${runtime}" is not available`);
      }
    }

    // Log successful execution
    appendAgentLog({
      timestamp: new Date().toISOString(),
      runtime,
      level: 'info',
      message: `Execution completed (${prompt.slice(0, 80)}...)`,
      durationMs: Date.now() - startTime,
    }).catch(() => {});

    return result;
  } catch (err) {
    // Log failed execution
    appendAgentLog({
      timestamp: new Date().toISOString(),
      runtime,
      level: 'error',
      message: err instanceof Error ? err.message : String(err),
      durationMs: Date.now() - startTime,
    }).catch(() => {});

    throw err;
  }
}

// ---- Agent Status (Inbound / Two-Way) ----

export interface AgentStatus {
  runtime: string;
  available: boolean;
  healthy: boolean;
  version: string | null;
  path: string | null;
  details: Record<string, unknown>;
}

export interface AgentLogEntry {
  timestamp: string;
  runtime: AgentRuntime;
  level: 'info' | 'warn' | 'error';
  message: string;
  jobId?: string;
  durationMs?: number;
}

/**
 * Deep health check for a single runtime.
 * Checks CLI availability, version, authentication, and connectivity.
 */
export async function queryAgentStatus(
  runtime: AgentRuntime,
  config?: RuntimeConfig
): Promise<AgentStatus> {
  try {
    return await invoke<AgentStatus>('query_agent_status', {
      runtime,
      config: config ? JSON.stringify(config) : null,
    });
  } catch {
    return {
      runtime,
      available: false,
      healthy: false,
      version: null,
      path: null,
      details: { error: 'Tauri not available' },
    };
  }
}

/**
 * Fast status check for all runtimes (no auth verification).
 */
export async function queryAllAgentStatuses(): Promise<AgentStatus[]> {
  try {
    return await invoke<AgentStatus[]>('query_all_agent_statuses');
  } catch {
    return [
      { runtime: 'claude', available: false, healthy: false, version: null, path: null, details: {} },
      { runtime: 'codex', available: false, healthy: false, version: null, path: null, details: {} },
      { runtime: 'openclaw', available: false, healthy: false, version: null, path: null, details: {} },
      { runtime: 'hermes', available: false, healthy: false, version: null, path: null, details: {} },
    ];
  }
}

/**
 * Append a structured log entry for agent execution tracking.
 */
export async function appendAgentLog(entry: AgentLogEntry): Promise<void> {
  try {
    await invoke('append_agent_log', { entry: JSON.stringify(entry) });
  } catch {
    // Fallback: localStorage
    const logs = JSON.parse(localStorage.getItem('ato-agent-logs') || '[]');
    logs.push(entry);
    // Keep last 500
    if (logs.length > 500) logs.splice(0, logs.length - 500);
    localStorage.setItem('ato-agent-logs', JSON.stringify(logs));
  }
}

/**
 * Read agent execution logs, optionally filtered by runtime.
 */
export async function getAgentLogs(
  runtime?: AgentRuntime,
  limit = 50
): Promise<AgentLogEntry[]> {
  try {
    return await invoke<AgentLogEntry[]>('get_agent_logs', {
      runtime: runtime || null,
      limit,
    });
  } catch {
    const raw = localStorage.getItem('ato-agent-logs');
    const all: AgentLogEntry[] = raw ? JSON.parse(raw) : [];
    const filtered = runtime ? all.filter((e) => e.runtime === runtime) : all;
    return filtered.slice(-limit);
  }
}

// ---- Marketplace / Skills Sharing ----

export interface MarketplaceInstallData {
  id: string;
  name: string;
  content: string;
}

export async function installMarketplaceSkill(data: MarketplaceInstallData): Promise<void> {
  try {
    await invoke('create_skill', {
      data: {
        name: data.name,
        description: '',
        scope: 'personal',
        content: data.content,
        isDirectory: false,
      },
    });
  } catch {
    // localStorage fallback
    const installed = JSON.parse(localStorage.getItem('ato-installed-skills') || '[]');
    installed.push({ ...data, installedAt: new Date().toISOString() });
    localStorage.setItem('ato-installed-skills', JSON.stringify(installed));
  }
}

export async function publishSkill(skillId: string, metadata: {
  category: string;
  tags: string[];
}): Promise<void> {
  // For now, store published skills locally
  const published = JSON.parse(localStorage.getItem('ato-published-skills') || '[]');
  published.push({ skillId, ...metadata, publishedAt: new Date().toISOString() });
  localStorage.setItem('ato-published-skills', JSON.stringify(published));
}

export async function shareSkill(skillId: string, userIds: string[]): Promise<{ shareUrl: string }> {
  // Generate local share JSON
  const shareData = { skillId, sharedWith: userIds, sharedAt: new Date().toISOString() };
  const shares = JSON.parse(localStorage.getItem('ato-shared-skills') || '[]');
  shares.push(shareData);
  localStorage.setItem('ato-shared-skills', JSON.stringify(shares));
  return { shareUrl: `ato://skill/${skillId}` };
}

// ---- Cron Jobs ----

export async function listCronJobs(): Promise<CronJob[]> {
  try {
    return await invoke<CronJob[]>('list_cron_jobs');
  } catch {
    const raw = localStorage.getItem('ato-cron-jobs');
    return raw ? JSON.parse(raw) : [];
  }
}

export async function saveCronJob(job: CronJob): Promise<void> {
  try {
    await invoke('save_cron_job', { job: JSON.stringify(job) });
  } catch {
    const all = await listCronJobs();
    const idx = all.findIndex((j) => j.id === job.id);
    if (idx >= 0) all[idx] = job;
    else all.push(job);
    localStorage.setItem('ato-cron-jobs', JSON.stringify(all));
  }
}

export async function deleteCronJob(id: string): Promise<void> {
  try {
    await invoke('delete_cron_job', { id });
  } catch {
    const all = await listCronJobs();
    localStorage.setItem('ato-cron-jobs', JSON.stringify(all.filter((j) => j.id !== id)));
  }
}

export async function getCronHistory(jobId: string): Promise<CronExecution[]> {
  try {
    return await invoke<CronExecution[]>('get_cron_history', { jobId });
  } catch {
    const raw = localStorage.getItem('ato-cron-history');
    const all: CronExecution[] = raw ? JSON.parse(raw) : [];
    return all.filter((e) => e.jobId === jobId);
  }
}

export async function triggerCronJob(id: string): Promise<void> {
  try {
    await invoke('trigger_cron_job', { id });
  } catch {
    // Mock trigger handled by store
  }
}
