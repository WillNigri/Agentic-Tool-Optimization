/**
 * Desktop API layer that uses Tauri commands for local data
 * and optionally proxies to the cloud API when sync is enabled.
 */

// Tauri invoke is available at runtime in the desktop app
// In dev/web mode, we fall back to HTTP API calls
const isTauri = typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  // Always try the Tauri import — it will succeed in the desktop app
  // even if __TAURI__ global isn't set yet at module load time
  let tauriInvoke;
  try {
    ({ invoke: tauriInvoke } = await import('@tauri-apps/api/core'));
  } catch {
    throw new Error(`Tauri not available for command: ${cmd}`);
  }
  return await tauriInvoke<T>(cmd, args);
}

// ---- Context ----
export async function getContextBreakdown() {
  return invoke<{
    totalTokens: number;
    limit: number;
    categories: Array<{ name: string; tokens: number; color: string }>;
  }>('get_context_estimate');
}

export async function getContextForRuntime(runtime: AgentRuntime) {
  return invoke<{
    totalTokens: number;
    limit: number;
    categories: Array<{ name: string; tokens: number; color: string }>;
  }>('get_context_for_runtime', { runtime });
}

// ---- Live Session Tracking (Phase 4) ----

export interface SessionFileRead {
  path: string;
  timestamp: string;
  tokenEstimate: number;
}

export interface LiveSessionData {
  sessionId: string | null;
  projectPath: string | null;
  totalInputTokens: number;
  totalOutputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  messageCount: number;
  toolCallCount: number;
  filesRead: SessionFileRead[];
  startedAt: string | null;
  lastActivity: string | null;
  model: string | null;
  isActive: boolean;
}

/**
 * Get live session data from Claude Code's session logs
 */
export async function getLiveSessionData(): Promise<LiveSessionData> {
  return invoke<LiveSessionData>('get_live_session_data');
}

/**
 * Get context breakdown with live session data for Claude
 */
export async function getLiveContextBreakdown() {
  return invoke<{
    totalTokens: number;
    limit: number;
    categories: Array<{ name: string; tokens: number; color: string }>;
  }>('get_live_context_breakdown');
}

// ---- MCP Tool Discovery (Phase 4) ----

export interface McpTool {
  name: string;
  description: string | null;
  inputSchema: unknown | null;
}

export interface McpServerDetails {
  serverName: string;
  serverVersion: string | null;
  protocolVersion: string | null;
  tools: McpTool[];
  connected: boolean;
  error: string | null;
}

/**
 * Discover tools from a specific MCP server
 */
export async function discoverMcpServerTools(serverName: string): Promise<McpServerDetails> {
  return invoke<McpServerDetails>('discover_mcp_server_tools', { serverName });
}

/**
 * Get all MCP servers with discovered tools
 */
export async function getMcpServersWithTools(): Promise<McpServerDetails[]> {
  return invoke<McpServerDetails[]>('get_mcp_servers_with_tools');
}

// ---- Hooks Read/Write (Phase 4) ----

export interface HookConfig {
  id: string;
  name: string;
  event: string;
  command: string;
  matcher: string | null;
  timeout: number | null;
  scope: string;
  enabled: boolean;
}

/**
 * Get all hooks from settings files
 */
export async function getHooks(): Promise<HookConfig[]> {
  return invoke<HookConfig[]>('get_hooks');
}

/**
 * Save a hook to settings file
 */
export async function saveHook(hook: HookConfig): Promise<void> {
  return invoke<void>('save_hook', { hook });
}

/**
 * Delete a hook from settings file
 */
export async function deleteHook(hookId: string): Promise<void> {
  return invoke<void>('delete_hook', { hookId });
}

// ---- Skills ----
export type SkillScope = 'enterprise' | 'personal' | 'project' | 'plugin';

export interface LocalSkill {
  id: string;
  name: string;
  description: string;
  filePath: string;
  scope: SkillScope;
  runtime: 'claude' | 'codex' | 'openclaw' | 'hermes';
  project: string | null; // project name for project-scoped skills
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
  runtime: 'claude' | 'codex' | 'openclaw' | 'hermes';
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
  return invoke<SkillDetail>('create_skill', { data: JSON.stringify(data) });
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

// ---- Runtime Path Override ----

/**
 * Save a custom CLI path when auto-detect fails.
 * Persisted to ~/.ato/{runtime}-path
 */
export async function setRuntimePath(runtime: AgentRuntime, path: string): Promise<void> {
  try {
    await invoke('set_runtime_path', { runtime, path });
  } catch {
    localStorage.setItem(`ato-runtime-path-${runtime}`, path);
  }
}

/**
 * Get a previously saved custom CLI path.
 */
export async function getRuntimePath(runtime: AgentRuntime): Promise<string | null> {
  try {
    return await invoke<string | null>('get_runtime_path', { runtime });
  } catch {
    return localStorage.getItem(`ato-runtime-path-${runtime}`);
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

// ---- Runtime Configuration ----
export async function saveRuntimeConfig(runtime: string, config: string): Promise<void> {
  return invoke('save_runtime_config', { runtime, config });
}

export async function loadRuntimeConfig(runtime: string): Promise<string | null> {
  return invoke<string | null>('load_runtime_config', { runtime });
}

export async function testRuntimeConnection(runtime: string, config: string): Promise<{ connected: boolean; version?: string; error?: string }> {
  return invoke('test_runtime_connection', { runtime, config });
}

// ---- OpenClaw Gateway ----
export async function openclawGatewayStatus(): Promise<unknown> {
  return invoke('openclaw_gateway_status');
}

export async function openclawListCronJobs(): Promise<unknown> {
  return invoke('openclaw_list_cron_jobs');
}

export async function openclawCronStatus(): Promise<unknown> {
  return invoke('openclaw_cron_status');
}

export async function openclawListAgents(): Promise<unknown> {
  return invoke('openclaw_list_agents');
}

export async function openclawSkillsStatus(): Promise<unknown> {
  return invoke('openclaw_skills_status');
}

export async function openclawListSessions(): Promise<unknown> {
  return invoke('openclaw_list_sessions');
}

export async function openclawTestConnection(wsUrl: string, token: string): Promise<unknown> {
  return invoke('openclaw_test_connection', { wsUrl, token });
}

// ---- OpenClaw Cron CRUD ----
export async function openclawEditCronJob(id: string, args: string): Promise<unknown> {
  return invoke('openclaw_edit_cron_job', { id, args });
}
export async function openclawAddCronJob(args: string): Promise<unknown> {
  return invoke('openclaw_add_cron_job', { args });
}
export async function openclawDeleteCronJob(id: string): Promise<unknown> {
  return invoke('openclaw_delete_cron_job', { id });
}
export async function openclawRunCronJob(id: string): Promise<unknown> {
  return invoke('openclaw_run_cron_job', { id });
}
export async function openclawToggleCronJob(id: string, enable: boolean): Promise<unknown> {
  return invoke('openclaw_toggle_cron_job', { id, enable });
}

// ---- OpenClaw Skills ----
export async function openclawListSkills(): Promise<LocalSkill[]> {
  return invoke<LocalSkill[]>('openclaw_list_skills');
}

// ---- Context Files ----

export interface ContextFile {
  runtime: string;
  name: string;
  filePath: string;
  tokenCount: number;
  exists: boolean;
}

export async function listContextFiles(): Promise<ContextFile[]> {
  return invoke<ContextFile[]>('list_context_files');
}

export async function readContextFile(filePath: string): Promise<string> {
  return invoke<string>('read_context_file', { filePath });
}

export async function writeContextFile(filePath: string, content: string): Promise<void> {
  return invoke('write_context_file', { filePath, content });
}

// ---- Agent Configuration Manager ----

export type AgentConfigRuntime = 'claude' | 'codex' | 'openclaw' | 'hermes' | 'shared';
export type AgentConfigScope = 'global' | 'project';
export type AgentConfigFileType = 'skill' | 'settings' | 'project-config' | 'mcp' | 'soul';

export interface AgentConfigFile {
  path: string;
  scope: AgentConfigScope;
  runtime: AgentConfigRuntime;
  fileType: AgentConfigFileType;
  exists: boolean;
  lastModified: string | null;
  tokenCount: number | null;
  projectName: string | null;
}

export interface ParsedConfigFile {
  path: string;
  format: 'yaml-frontmatter' | 'json' | 'toml' | 'yaml' | 'markdown' | 'unknown';
  content: unknown;
  raw: string;
}

export interface AgentPermission {
  tool: string;
  pattern: string | null;
  allowed: boolean;
  requiresApproval: boolean;
}

export interface ContextPreviewSection {
  name: string;
  tokens: number;
  files: string[];
}

export interface AgentContextPreview {
  totalTokens: number;
  limit: number;
  sections: ContextPreviewSection[];
}

/**
 * Scan all config files for all agent runtimes
 */
export async function scanAgentConfigFiles(projectPath?: string): Promise<AgentConfigFile[]> {
  try {
    return await invoke<AgentConfigFile[]>('scan_agent_config_files', { projectPath: projectPath || null });
  } catch {
    return [];
  }
}

/**
 * Read and parse a config file
 */
export async function readAgentConfigFile(path: string): Promise<ParsedConfigFile> {
  return invoke<ParsedConfigFile>('read_agent_config_file', { path });
}

/**
 * Write a config file back to disk
 */
export async function writeAgentConfigFile(path: string, content: string): Promise<void> {
  return invoke('write_agent_config_file', { path, content });
}

/**
 * Create a new skill file from template
 */
export async function createAgentSkill(
  runtime: AgentConfigRuntime,
  name: string,
  scope: AgentConfigScope,
  description: string
): Promise<string> {
  return invoke<string>('create_agent_skill', { runtime, name, scope, description });
}

/**
 * Parse permissions from a settings file
 */
export async function parseAgentPermissions(path: string): Promise<AgentPermission[]> {
  try {
    return await invoke<AgentPermission[]>('parse_agent_permissions', { path });
  } catch {
    return [];
  }
}

/**
 * Get context preview for a runtime
 */
export async function getAgentContextPreview(runtime: AgentConfigRuntime): Promise<AgentContextPreview> {
  return invoke<AgentContextPreview>('get_agent_context_preview', { runtime });
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 1: Skill Health Check
// ══════════════════════════════════════════════════════════════════════════════

export interface ValidationIssue {
  code: string;
  severity: 'error' | 'warning';
  message: string;
  line?: number;
  suggestion?: string;
}

export interface SkillValidation {
  path: string;
  skillName?: string;
  valid: boolean;
  errors: ValidationIssue[];
  warnings: ValidationIssue[];
  tokenCount: number;
}

/**
 * Validate a single skill file
 */
export async function validateSkill(path: string): Promise<SkillValidation> {
  return invoke<SkillValidation>('validate_skill', { path });
}

/**
 * Validate all skill files across all runtimes
 */
export async function validateAllSkills(): Promise<SkillValidation[]> {
  return invoke<SkillValidation[]>('validate_all_skills');
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 2: Onboarding Checklist
// ══════════════════════════════════════════════════════════════════════════════

export interface OnboardingAction {
  actionType: 'create_file' | 'open_editor' | 'run_command' | 'external_link';
  target: string;
}

export interface OnboardingItem {
  id: string;
  label: string;
  completed: boolean;
  action?: OnboardingAction;
}

export interface OnboardingStatus {
  runtime: string;
  items: OnboardingItem[];
  completionPercent: number;
}

/**
 * Get onboarding status for a specific runtime
 */
export async function getOnboardingStatus(runtime: AgentConfigRuntime): Promise<OnboardingStatus> {
  return invoke<OnboardingStatus>('get_onboarding_status', { runtime });
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 3: Profile Snapshots
// ══════════════════════════════════════════════════════════════════════════════

export interface ProfileFile {
  path: string;
  content: string;
  scope: 'global' | 'project';
}

export interface ProfileSnapshot {
  id: string;
  name: string;
  description?: string;
  runtime: string;
  files: ProfileFile[];
  createdAt: string;
}

/**
 * Save current configuration as a profile snapshot
 */
export async function saveProfileSnapshot(
  name: string,
  description: string | null,
  runtime: AgentConfigRuntime
): Promise<string> {
  return invoke<string>('save_profile_snapshot', { name, description, runtime });
}

/**
 * List all profile snapshots
 */
export async function listProfileSnapshots(): Promise<ProfileSnapshot[]> {
  return invoke<ProfileSnapshot[]>('list_profile_snapshots');
}

/**
 * Load a profile snapshot (writes files to disk)
 */
export async function loadProfileSnapshot(profileId: string): Promise<void> {
  return invoke<void>('load_profile_snapshot', { profileId });
}

/**
 * Delete a profile snapshot
 */
export async function deleteProfileSnapshot(profileId: string): Promise<void> {
  return invoke<void>('delete_profile_snapshot', { profileId });
}

/**
 * Export a profile snapshot as JSON
 */
export async function exportProfileSnapshot(profileId: string): Promise<string> {
  return invoke<string>('export_profile_snapshot', { profileId });
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 4: Skill Usage Analytics
// ══════════════════════════════════════════════════════════════════════════════

export interface SkillUsageStat {
  skillPath: string;
  skillName: string;
  triggerCount: number;
  lastUsed?: string;
  avgTokens?: number;
}

/**
 * Get usage statistics for all skills
 */
export async function getSkillUsageStats(): Promise<SkillUsageStat[]> {
  return invoke<SkillUsageStat[]>('get_skill_usage_stats');
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 6: Project Manager
// ══════════════════════════════════════════════════════════════════════════════

export interface Project {
  id: string;
  name: string;
  path: string;
  isActive: boolean;
  skillCount: number;
  lastAccessed?: string;
  createdAt: string;
  hasClaude: boolean;
  hasCodex: boolean;
  hasHermes: boolean;
  hasOpenclaw: boolean;
}

export interface DiscoveredProject {
  path: string;
  name: string;
  skillCount: number;
  runtimes: string[];
}

/**
 * Discover projects on the system that have agent configurations
 */
export async function discoverProjects(): Promise<DiscoveredProject[]> {
  return invoke<DiscoveredProject[]>('discover_projects');
}

/**
 * List all saved projects
 */
export async function listProjects(): Promise<Project[]> {
  return invoke<Project[]>('list_projects');
}

/**
 * Add a project to the list
 */
export async function addProject(name: string, path: string): Promise<Project> {
  return invoke<Project>('add_project', { name, path });
}

/**
 * Update a project's name
 */
export async function updateProject(projectId: string, name: string): Promise<void> {
  return invoke<void>('update_project', { projectId, name });
}

/**
 * Delete a project from the list (doesn't delete files)
 */
export async function deleteProject(projectId: string): Promise<void> {
  return invoke<void>('delete_project', { projectId });
}

/**
 * Set the active project
 */
export async function setActiveProject(projectId: string): Promise<void> {
  return invoke<void>('set_active_project', { projectId });
}

/**
 * Get the active project
 */
export async function getActiveProject(): Promise<Project | null> {
  return invoke<Project | null>('get_active_project');
}

/**
 * Get skills for a specific project
 */
export async function getProjectSkills(projectPath: string): Promise<LocalSkill[]> {
  return invoke<LocalSkill[]>('get_project_skills', { projectPath });
}

/**
 * Clone a skill from one project to another
 */
export async function cloneSkill(
  sourceSkillPath: string,
  targetProjectPath: string,
  targetRuntime: AgentConfigRuntime
): Promise<string> {
  return invoke<string>('clone_skill', { sourceSkillPath, targetProjectPath, targetRuntime });
}

/**
 * Refresh skill count for a project
 */
export async function refreshProjectSkills(projectId: string): Promise<number> {
  return invoke<number>('refresh_project_skills', { projectId });
}

// ============================================================================
// Secrets Manager
// ============================================================================

export interface Secret {
  id: string;
  name: string;
  keyType: string;
  runtime?: string;
  projectId?: string;
  createdAt: string;
  updatedAt: string;
  hasValue: boolean;
}

/**
 * List all secrets (metadata only)
 */
export async function listSecrets(): Promise<Secret[]> {
  return invoke<Secret[]>('list_secrets');
}

/**
 * Save a new secret
 */
export async function saveSecret(
  name: string,
  keyType: string,
  value: string,
  runtime?: string,
  projectId?: string
): Promise<Secret> {
  return invoke<Secret>('save_secret', { name, keyType, value, runtime, projectId });
}

/**
 * Get secret value (requires user action)
 */
export async function getSecretValue(secretId: string): Promise<string> {
  return invoke<string>('get_secret_value', { secretId });
}

/**
 * Update a secret
 */
export async function updateSecret(
  secretId: string,
  name?: string,
  value?: string
): Promise<void> {
  return invoke<void>('update_secret', { secretId, name, value });
}

/**
 * Delete a secret
 */
export async function deleteSecret(secretId: string): Promise<void> {
  return invoke<void>('delete_secret', { secretId });
}

// ============================================================================
// Environment Variables
// ============================================================================

export interface EnvVar {
  id: string;
  projectId?: string;
  runtime?: string;
  key: string;
  value: string;
  createdAt: string;
}

/**
 * List environment variables
 */
export async function listEnvVars(projectId?: string, runtime?: string): Promise<EnvVar[]> {
  return invoke<EnvVar[]>('list_env_vars', { projectId, runtime });
}

/**
 * Save an environment variable
 */
export async function saveEnvVar(
  key: string,
  value: string,
  projectId?: string,
  runtime?: string
): Promise<EnvVar> {
  return invoke<EnvVar>('save_env_var', { key, value, projectId, runtime });
}

/**
 * Update an environment variable
 */
export async function updateEnvVar(
  envId: string,
  key?: string,
  value?: string
): Promise<void> {
  return invoke<void>('update_env_var', { envId, key, value });
}

/**
 * Delete an environment variable
 */
export async function deleteEnvVar(envId: string): Promise<void> {
  return invoke<void>('delete_env_var', { envId });
}

/**
 * Import environment variables from a .env file
 */
export async function importEnvFile(
  filePath: string,
  projectId?: string,
  runtime?: string
): Promise<EnvVar[]> {
  return invoke<EnvVar[]>('import_env_file', { filePath, projectId, runtime });
}

// ============================================================================
// Model Configuration
// ============================================================================

export interface ModelConfig {
  id: string;
  runtime: string;
  projectId?: string;
  modelId: string;
  maxTokens?: number;
  temperature?: number;
  createdAt: string;
  updatedAt: string;
}

/**
 * List all model configurations
 */
export async function listModelConfigs(): Promise<ModelConfig[]> {
  return invoke<ModelConfig[]>('list_model_configs');
}

/**
 * Save or update model configuration
 */
export async function saveModelConfig(
  runtime: string,
  modelId: string,
  projectId?: string,
  maxTokens?: number,
  temperature?: number
): Promise<ModelConfig> {
  return invoke<ModelConfig>('save_model_config', { runtime, modelId, projectId, maxTokens, temperature });
}

/**
 * Get model config for a runtime
 */
export async function getModelConfig(runtime: string, projectId?: string): Promise<ModelConfig | null> {
  return invoke<ModelConfig | null>('get_model_config', { runtime, projectId });
}

// ============================================================================
// Execution Logs
// ============================================================================

export interface ExecutionLog {
  id: string;
  runtime: string;
  prompt?: string;
  response?: string;
  tokensIn?: number;
  tokensOut?: number;
  durationMs?: number;
  status: string;
  errorMessage?: string;
  skillName?: string;
  createdAt: string;
}

/**
 * Get execution logs
 */
export async function getExecutionLogs(
  runtime?: string,
  status?: string,
  limit?: number
): Promise<ExecutionLog[]> {
  return invoke<ExecutionLog[]>('get_execution_logs', { runtime, status, limit });
}

/**
 * Add an execution log entry
 */
export async function addExecutionLog(
  runtime: string,
  status: string,
  prompt?: string,
  response?: string,
  tokensIn?: number,
  tokensOut?: number,
  durationMs?: number,
  errorMessage?: string,
  skillName?: string
): Promise<ExecutionLog> {
  return invoke<ExecutionLog>('add_execution_log', {
    runtime, status, prompt, response, tokensIn, tokensOut, durationMs, errorMessage, skillName
  });
}

// ============================================================================
// Health Checks
// ============================================================================

export interface RuntimeHealth {
  runtime: string;
  status: string;
  latencyMs?: number;
  uptimePercent?: number;
  lastCheck?: string;
  errorMessage?: string;
}

/**
 * Get health status for all runtimes
 */
export async function getHealthStatus(): Promise<RuntimeHealth[]> {
  return invoke<RuntimeHealth[]>('get_health_status');
}

/**
 * Record a health check
 */
export async function recordHealthCheck(
  runtime: string,
  status: string,
  latencyMs?: number,
  errorMessage?: string
): Promise<void> {
  return invoke<void>('record_health_check', { runtime, status, latencyMs, errorMessage });
}

// ---- Phase 2: Real-time Monitoring ----

/**
 * Start the log file watcher for real-time updates
 */
export async function startLogWatcher(): Promise<boolean> {
  return invoke<boolean>('start_log_watcher');
}

/**
 * Stop the log file watcher
 */
export async function stopLogWatcher(): Promise<boolean> {
  return invoke<boolean>('stop_log_watcher');
}

/**
 * Check if log watcher is running
 */
export async function isLogWatcherRunning(): Promise<boolean> {
  return invoke<boolean>('is_log_watcher_running');
}

/**
 * Start the background health poller
 */
export async function startHealthPoller(): Promise<boolean> {
  return invoke<boolean>('start_health_poller');
}

/**
 * Stop the background health poller
 */
export async function stopHealthPoller(): Promise<boolean> {
  return invoke<boolean>('stop_health_poller');
}

/**
 * Check if health poller is running
 */
export async function isHealthPollerRunning(): Promise<boolean> {
  return invoke<boolean>('is_health_poller_running');
}

/**
 * Health history data point
 */
export interface HealthHistoryPoint {
  timestamp: string;
  latencyMs: number | null;
  status: string;
}

/**
 * Runtime health history with stats
 */
export interface RuntimeHealthHistory {
  runtime: string;
  dataPoints: HealthHistoryPoint[];
  avgLatencyMs: number | null;
  uptimePercent: number;
  totalChecks: number;
}

/**
 * Get health check history for charts
 */
export async function getHealthHistory(
  runtime?: string,
  hours?: number
): Promise<RuntimeHealthHistory[]> {
  return invoke<RuntimeHealthHistory[]>('get_health_history', { runtime, hours });
}

/**
 * Runtime execution count
 */
export interface RuntimeExecutionCount {
  runtime: string;
  count: number;
  successCount: number;
  errorCount: number;
}

/**
 * Daily execution count
 */
export interface DailyExecutionCount {
  date: string;
  count: number;
  successCount: number;
  errorCount: number;
}

/**
 * Aggregated usage metrics
 */
export interface UsageMetrics {
  totalExecutions: number;
  successfulExecutions: number;
  failedExecutions: number;
  totalTokensIn: number;
  totalTokensOut: number;
  avgDurationMs: number | null;
  executionsByRuntime: RuntimeExecutionCount[];
  executionsByDay: DailyExecutionCount[];
}

/**
 * Get aggregated usage metrics
 */
export async function getUsageMetrics(days?: number): Promise<UsageMetrics> {
  return invoke<UsageMetrics>('get_usage_metrics', { days });
}

// ---- v0.8.0: Workflow Webhooks ----

export interface WorkflowWebhook {
  id: string;
  workflowId: string;
  path: string;
  method: string;
  secret: string | null;
  enabled: boolean;
  createdAt: string;
  lastTriggeredAt: string | null;
  triggerCount: number;
}

/**
 * Register a webhook for a workflow
 */
export async function registerWorkflowWebhook(
  workflowId: string,
  path: string,
  method: string,
  secret?: string
): Promise<WorkflowWebhook> {
  return invoke<WorkflowWebhook>('register_workflow_webhook', {
    workflowId,
    path,
    method,
    secret,
  });
}

/**
 * List all registered webhooks
 */
export async function listWorkflowWebhooks(): Promise<WorkflowWebhook[]> {
  return invoke<WorkflowWebhook[]>('list_workflow_webhooks');
}

/**
 * Delete a webhook
 */
export async function deleteWorkflowWebhook(webhookId: string): Promise<void> {
  return invoke<void>('delete_workflow_webhook', { webhookId });
}

/**
 * Toggle webhook enabled state
 */
export async function toggleWorkflowWebhook(
  webhookId: string,
  enabled: boolean
): Promise<void> {
  return invoke<void>('toggle_workflow_webhook', { webhookId, enabled });
}

// ---- v0.8.0: Workflow Templates ----

export interface WorkflowTemplateInfo {
  id: string;
  name: string;
  description: string;
  category: string;
  tags: string[];
  version: string;
  isBuiltIn: boolean;
  nodes: unknown;
  edges: unknown;
}

/**
 * List available workflow templates
 */
export async function listWorkflowTemplates(): Promise<WorkflowTemplateInfo[]> {
  return invoke<WorkflowTemplateInfo[]>('list_workflow_templates');
}

// ---- v0.5.5: Notifications ----

export interface NotificationChannel {
  id: string;
  provider: string;
  name: string;
  config: Record<string, string>;
  events: string[];
  enabled: boolean;
  createdAt: string;
  lastSentAt: string | null;
}

export interface SendNotificationRequest {
  eventType: string;
  title: string;
  message: string;
  data?: unknown;
}

export interface NotificationResult {
  channelId: string;
  success: boolean;
  error: string | null;
}

/**
 * Save a notification channel configuration
 */
export async function saveNotificationChannel(
  channel: NotificationChannel
): Promise<NotificationChannel> {
  return invoke<NotificationChannel>('save_notification_channel', { channel });
}

/**
 * List all notification channels
 */
export async function listNotificationChannels(): Promise<NotificationChannel[]> {
  return invoke<NotificationChannel[]>('list_notification_channels');
}

/**
 * Delete a notification channel
 */
export async function deleteNotificationChannel(channelId: string): Promise<void> {
  return invoke<void>('delete_notification_channel', { channelId });
}

/**
 * Toggle notification channel enabled state
 */
export async function toggleNotificationChannel(
  channelId: string,
  enabled: boolean
): Promise<void> {
  return invoke<void>('toggle_notification_channel', { channelId, enabled });
}

/**
 * Send a notification to all matching channels
 */
export async function sendNotification(
  request: SendNotificationRequest
): Promise<NotificationResult[]> {
  return invoke<NotificationResult[]>('send_notification', { request });
}

/**
 * Test a notification channel configuration
 */
export async function testNotificationChannel(
  channel: NotificationChannel
): Promise<NotificationResult> {
  return invoke<NotificationResult>('test_notification_channel', { channel });
}

// ---- Telemetry & Analytics (v1.0.0) ----

export interface TelemetrySettings {
  enabled: boolean;
  deviceId: string;
  endpoint: string | null;
}

export interface TelemetryEvent {
  eventType: string;
  properties: Record<string, unknown>;
  timestamp: string;
  sessionId: string;
  deviceId: string;
}

export interface AnalyticsSummary {
  skills: number;
  workflows: number;
  notificationChannels: number;
  cronJobs: number;
  recentExecutions: number;
  sessionId: string;
  generatedAt: string;
}

/**
 * Get telemetry settings
 */
export async function getTelemetrySettings(): Promise<TelemetrySettings> {
  return invoke<TelemetrySettings>('get_telemetry_settings');
}

/**
 * Update telemetry settings
 */
export async function updateTelemetrySettings(
  enabled: boolean,
  endpoint?: string | null
): Promise<TelemetrySettings> {
  return invoke<TelemetrySettings>('update_telemetry_settings', { enabled, endpoint });
}

/**
 * Track a telemetry event (only if telemetry is enabled)
 */
export async function trackEvent(
  eventType: string,
  properties: Record<string, unknown> = {}
): Promise<void> {
  return invoke<void>('track_event', { eventType, properties });
}

/**
 * Get queued telemetry events
 */
export async function getQueuedEvents(): Promise<TelemetryEvent[]> {
  return invoke<TelemetryEvent[]>('get_queued_events');
}

/**
 * Export telemetry events to a JSON file
 */
export async function exportTelemetryEvents(path: string): Promise<number> {
  return invoke<number>('export_telemetry_events', { path });
}

/**
 * Get analytics summary for the dashboard
 */
export async function getAnalyticsSummary(): Promise<AnalyticsSummary> {
  return invoke<AnalyticsSummary>('get_analytics_summary');
}

// ---- Tracking Helper Functions ----

/**
 * Standard event types for consistency
 */
export const TelemetryEventTypes = {
  APP_LAUNCHED: 'app_launched',
  APP_CLOSED: 'app_closed',
  SIGNUP_STARTED: 'signup_started',
  SIGNUP_COMPLETED: 'signup_completed',
  LOGIN_COMPLETED: 'login_completed',
  SKILL_CREATED: 'skill_created',
  SKILL_INSTALLED: 'skill_installed',
  SKILL_EXECUTED: 'skill_executed',
  AUTOMATION_CREATED: 'automation_created',
  AUTOMATION_EXECUTED: 'automation_executed',
  RUNTIME_CONNECTED: 'runtime_connected',
  NOTIFICATION_SENT: 'notification_sent',
  SETTINGS_CHANGED: 'settings_changed',
  FEATURE_USED: 'feature_used',
  ERROR_OCCURRED: 'error_occurred',
} as const;

/**
 * Track app launch event
 */
export async function trackAppLaunch(): Promise<void> {
  return trackEvent(TelemetryEventTypes.APP_LAUNCHED, {
    platform: navigator.platform,
    userAgent: navigator.userAgent,
    language: navigator.language,
    screenWidth: window.screen.width,
    screenHeight: window.screen.height,
  });
}

/**
 * Track signup event
 */
export async function trackSignup(
  method: 'github' | 'email' | 'other',
  success: boolean
): Promise<void> {
  return trackEvent(success ? TelemetryEventTypes.SIGNUP_COMPLETED : TelemetryEventTypes.SIGNUP_STARTED, {
    method,
    success,
  });
}

/**
 * Track feature usage
 */
export async function trackFeatureUsage(
  feature: string,
  action: string,
  metadata?: Record<string, unknown>
): Promise<void> {
  return trackEvent(TelemetryEventTypes.FEATURE_USED, {
    feature,
    action,
    ...metadata,
  });
}
