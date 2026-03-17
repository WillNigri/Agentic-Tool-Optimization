/**
 * Unified API layer for the desktop app.
 *
 * Priority:
 * 1. Tauri commands (desktop app with Rust backend)
 * 2. HTTP cloud API (when cloud backend is running)
 * 3. Mock data (browser dev mode — no backend needed)
 */

import * as tauriApi from './tauri-api';
import * as mock from './mock-data';

const isTauri = typeof window !== 'undefined' && '__TAURI__' in window;
const API_BASE = import.meta.env.VITE_API_URL || 'http://localhost:3000/api';

// Check if cloud API is reachable (cached, fast fail)
let cloudAvailable: boolean | null = null;
async function isCloudAvailable(): Promise<boolean> {
  if (cloudAvailable !== null) return cloudAvailable;
  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 1500);
    const res = await fetch(`${API_BASE}/health`, { signal: controller.signal });
    clearTimeout(timeout);
    cloudAvailable = res.ok;
  } catch {
    cloudAvailable = false;
  }
  return cloudAvailable;
}

// ---- HTTP helpers ----

function getAuthHeaders(): Record<string, string> {
  const stored = localStorage.getItem('ato-auth');
  if (!stored) return {};
  try {
    const { state } = JSON.parse(stored);
    if (state?.accessToken) {
      return { Authorization: `Bearer ${state.accessToken}` };
    }
  } catch { /* ignore */ }
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

// ---- Auth (cloud only) ----

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
  if (await isCloudAvailable()) return fetchApi<ContextBreakdown>('/context/breakdown');
  return mock.mockContextBreakdown;
}

// ---- Skills ----

export type Skill = tauriApi.LocalSkill;

export async function getSkills(): Promise<Skill[]> {
  if (isTauri) return tauriApi.getSkills();
  if (await isCloudAvailable()) return fetchApi<Skill[]>('/skills');
  return mock.mockSkills;
}

export async function toggleSkill(id: string, enabled: boolean): Promise<void> {
  if (isTauri) return tauriApi.toggleSkill(id, enabled);
  if (await isCloudAvailable()) {
    await fetchApi(`/skills/${id}/toggle`, { method: 'POST', body: JSON.stringify({ enabled }) });
    return;
  }
  // Mock: update in-place
  const skill = mock.mockSkills.find(s => s.id === id);
  if (skill) skill.enabled = enabled;
}

// ---- Usage Analytics ----

export interface UsageSummary {
  today: { inputTokens: number; outputTokens: number; costCents: number };
  week: { inputTokens: number; outputTokens: number; costCents: number };
  month: { inputTokens: number; outputTokens: number; costCents: number };
}

export async function getUsageSummary(): Promise<UsageSummary> {
  if (isTauri) return tauriApi.getUsageSummary();
  if (await isCloudAvailable()) return fetchApi('/analytics/summary');
  return mock.mockUsageSummary;
}

export interface DailyUsage {
  date: string;
  inputTokens: number;
  outputTokens: number;
}

export async function getDailyUsage(days: number = 30): Promise<DailyUsage[]> {
  if (isTauri) return tauriApi.getDailyUsage(days);
  if (await isCloudAvailable()) return fetchApi(`/analytics/daily?days=${days}`);
  return mock.mockDailyUsage;
}

export interface BurnRate {
  tokensPerHour: number;
  costPerHour: number;
  estimatedHoursToLimit: number | null;
  limit: number | null;
}

export async function getBurnRate(): Promise<BurnRate> {
  if (isTauri) return tauriApi.getBurnRate();
  if (await isCloudAvailable()) return fetchApi('/analytics/burn-rate');
  return mock.mockBurnRate;
}

// ---- MCP Servers ----

export type McpServer = tauriApi.LocalMcpServer;

export async function getMcpServers(): Promise<McpServer[]> {
  if (isTauri) return tauriApi.getMcpServers();
  if (await isCloudAvailable()) return fetchApi<McpServer[]>('/mcp/servers');
  return mock.mockMcpServers;
}

export async function restartMcpServer(id: string): Promise<void> {
  if (isTauri) return tauriApi.restartMcpServer(id);
  if (await isCloudAvailable()) {
    await fetchApi(`/mcp/servers/${id}/restart`, { method: 'POST' });
    return;
  }
}

// ---- Config Files ----

export interface ConfigFile {
  path: string;
  exists: boolean;
  scope: string;
}

export async function getConfigFiles(): Promise<ConfigFile[]> {
  if (isTauri) return tauriApi.getConfigFiles();
  if (await isCloudAvailable()) return fetchApi<ConfigFile[]>('/config/files');
  return mock.mockConfigFiles;
}

// ---- Sync ----

export async function getSyncStatus() {
  if (isTauri) return tauriApi.getSyncStatus();
  return { enabled: false, lastSyncAt: null, cloudUrl: null };
}

export async function setSyncEnabled(enabled: boolean, cloudUrl?: string) {
  if (isTauri) return tauriApi.setSyncEnabled(enabled, cloudUrl);
}
