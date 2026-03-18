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
