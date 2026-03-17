/**
 * Unified API layer for the desktop app.
 *
 * - In Tauri mode: uses local Tauri commands (SQLite, file system)
 * - Falls back to HTTP cloud API when Tauri is not available (web dev mode)
 *
 * Components import from this file — they don't need to know
 * whether data comes from local or cloud.
 */

import * as tauriApi from './tauri-api';

const isTauri = typeof window !== 'undefined' && '__TAURI__' in window;
const API_BASE = import.meta.env.VITE_API_URL || 'http://localhost:3000/api';

// ---- HTTP helpers (cloud fallback for web dev mode) ----

function getAuthHeaders(): Record<string, string> {
  const stored = localStorage.getItem('ato-auth');
  if (!stored) return {};
  try {
    const { state } = JSON.parse(stored);
    if (state?.accessToken) {
      return { Authorization: `Bearer ${state.accessToken}` };
    }
  } catch {
    /* ignore */
  }
  return {};
}

async function fetchApi<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...getAuthHeaders(),
      ...options?.headers,
    },
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: { message: res.statusText } }));
    throw new Error(err.error?.message || `API error ${res.status}`);
  }
  const json = await res.json();
  return json.data ?? json;
}

// ---- Auth (cloud only — desktop app works local-first, no login needed) ----

export interface AuthResponse {
  user: { id: string; email: string; name: string };
  accessToken: string;
  refreshToken: string;
}

export async function login(data: { email: string; password: string }) {
  return fetchApi<AuthResponse>('/auth/login', { method: 'POST', body: JSON.stringify(data) });
}

export async function register(data: { name: string; email: string; password: string }) {
  return fetchApi<AuthResponse>('/auth/register', { method: 'POST', body: JSON.stringify(data) });
}

export function refreshToken(token: string) {
  return fetchApi<{ accessToken: string }>('/auth/refresh', {
    method: 'POST',
    body: JSON.stringify({ refreshToken: token }),
  });
}

// ---- Context ----

export interface ContextBreakdown {
  totalTokens: number;
  limit: number;
  categories: { name: string; tokens: number; color: string }[];
}

export async function getContextBreakdown(): Promise<ContextBreakdown> {
  if (isTauri) return tauriApi.getContextBreakdown();
  return fetchApi<ContextBreakdown>('/context/breakdown');
}

// ---- Skills ----

export type Skill = tauriApi.LocalSkill;

export async function getSkills(): Promise<Skill[]> {
  if (isTauri) return tauriApi.getSkills();
  return fetchApi<Skill[]>('/skills');
}

export async function toggleSkill(id: string, enabled: boolean): Promise<void> {
  if (isTauri) return tauriApi.toggleSkill(id, enabled);
  await fetchApi(`/skills/${id}/toggle`, { method: 'POST', body: JSON.stringify({ enabled }) });
}

// ---- Usage Analytics ----

export interface UsageSummary {
  today: { inputTokens: number; outputTokens: number; costCents: number };
  week: { inputTokens: number; outputTokens: number; costCents: number };
  month: { inputTokens: number; outputTokens: number; costCents: number };
}

export async function getUsageSummary(): Promise<UsageSummary> {
  if (isTauri) return tauriApi.getUsageSummary();
  return fetchApi<UsageSummary>('/analytics/summary');
}

export interface DailyUsage {
  date: string;
  inputTokens: number;
  outputTokens: number;
}

export async function getDailyUsage(days: number = 30): Promise<DailyUsage[]> {
  if (isTauri) return tauriApi.getDailyUsage(days);
  return fetchApi<DailyUsage[]>(`/analytics/daily?days=${days}`);
}

export interface BurnRate {
  tokensPerHour: number;
  costPerHour: number;
  estimatedHoursToLimit: number | null;
  limit: number | null;
}

export async function getBurnRate(): Promise<BurnRate> {
  if (isTauri) return tauriApi.getBurnRate();
  return fetchApi<BurnRate>('/analytics/burn-rate');
}

// ---- MCP Servers ----

export type McpServer = tauriApi.LocalMcpServer;

export async function getMcpServers(): Promise<McpServer[]> {
  if (isTauri) return tauriApi.getMcpServers();
  return fetchApi<McpServer[]>('/mcp/servers');
}

export async function restartMcpServer(id: string): Promise<void> {
  if (isTauri) return tauriApi.restartMcpServer(id);
  await fetchApi(`/mcp/servers/${id}/restart`, { method: 'POST' });
}

// ---- Config Files ----

export interface ConfigFile {
  path: string;
  exists: boolean;
  scope: string;
}

export async function getConfigFiles(): Promise<ConfigFile[]> {
  if (isTauri) return tauriApi.getConfigFiles();
  return fetchApi<ConfigFile[]>('/config/files');
}

// ---- Sync Status ----

export async function getSyncStatus() {
  if (isTauri) return tauriApi.getSyncStatus();
  return { enabled: true, lastSyncAt: null, cloudUrl: API_BASE };
}

export async function setSyncEnabled(enabled: boolean, cloudUrl?: string) {
  if (isTauri) return tauriApi.setSyncEnabled(enabled, cloudUrl);
}
