/**
 * ATO Cloud API Client
 * Handles communication with the cloud backend for sync, teams, and auth
 */

const CLOUD_API_URL = import.meta.env.VITE_CLOUD_API_URL || 'https://api.ato.dev';

// ============================================================
// Types
// ============================================================

export interface CloudUser {
  id: string;
  email: string;
  name: string;
  avatar_url: string | null;
  auth_method: 'email' | 'oauth';
  github_id: string | null;
  github_username: string | null;
  created_at: string;
  updated_at: string;
}

export interface AuthTokens {
  accessToken: string;
  refreshToken: string;
}

export interface AuthResponse {
  user: CloudUser;
  tokens: AuthTokens;
}

export interface Team {
  id: string;
  name: string;
  slug: string;
  description: string | null;
  owner_id: string;
  avatar_url: string | null;
  created_at: string;
  updated_at: string;
  role?: 'owner' | 'admin' | 'member';
  member_count?: number;
}

export interface TeamMember {
  id: string;
  team_id: string;
  user_id: string;
  role: 'owner' | 'admin' | 'member';
  joined_at: string;
  user?: CloudUser;
}

export interface TeamWithMembers extends Team {
  members: TeamMember[];
  member_count: number;
}

export interface TeamSkill {
  id: string;
  team_id: string;
  original_skill_id: string | null;
  shared_by: string;
  name: string;
  description: string | null;
  content: string;
  token_count: number;
  version: number;
  is_pinned: boolean;
  created_at: string;
  updated_at: string;
  shared_by_user?: CloudUser;
}

export interface TeamInvitation {
  id: string;
  team_id: string;
  email: string;
  role: 'admin' | 'member';
  token: string;
  expires_at: string;
  created_at: string;
  team?: Team;
  invited_by_user?: CloudUser;
}

export interface ApiResponse<T> {
  success: boolean;
  data: T;
}

export interface ApiError {
  success: false;
  error: {
    code: string;
    message: string;
    details?: unknown;
  };
}

// ============================================================
// Token Management
// ============================================================

const TOKEN_KEY = 'ato_cloud_tokens';

export function getStoredTokens(): AuthTokens | null {
  try {
    const stored = localStorage.getItem(TOKEN_KEY);
    if (!stored) return null;
    return JSON.parse(stored);
  } catch {
    return null;
  }
}

export function storeTokens(tokens: AuthTokens): void {
  localStorage.setItem(TOKEN_KEY, JSON.stringify(tokens));
}

export function clearTokens(): void {
  localStorage.removeItem(TOKEN_KEY);
}

// ============================================================
// API Client
// ============================================================

class CloudApiError extends Error {
  code: string;
  details?: unknown;

  constructor(code: string, message: string, details?: unknown) {
    super(message);
    this.name = 'CloudApiError';
    this.code = code;
    this.details = details;
  }
}

async function apiRequest<T>(
  endpoint: string,
  options: RequestInit = {},
  requireAuth = true
): Promise<T> {
  const tokens = getStoredTokens();

  const headers: HeadersInit = {
    'Content-Type': 'application/json',
    ...options.headers,
  };

  if (requireAuth) {
    if (!tokens?.accessToken) {
      throw new CloudApiError('UNAUTHORIZED', 'Not authenticated');
    }
    (headers as Record<string, string>)['Authorization'] = `Bearer ${tokens.accessToken}`;
  }

  const response = await fetch(`${CLOUD_API_URL}${endpoint}`, {
    ...options,
    headers,
  });

  const data = await response.json();

  if (!response.ok || !data.success) {
    const error = data.error || { code: 'UNKNOWN_ERROR', message: 'An unknown error occurred' };

    // Handle token expiration
    if (response.status === 401 && tokens?.refreshToken) {
      try {
        const newTokens = await refreshTokens(tokens.refreshToken);
        storeTokens(newTokens);
        // Retry the request with new token
        return apiRequest<T>(endpoint, options, requireAuth);
      } catch {
        clearTokens();
        throw new CloudApiError('SESSION_EXPIRED', 'Your session has expired. Please log in again.');
      }
    }

    throw new CloudApiError(error.code, error.message, error.details);
  }

  return data.data as T;
}

async function refreshTokens(refreshToken: string): Promise<AuthTokens> {
  const response = await fetch(`${CLOUD_API_URL}/api/auth/refresh`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ refreshToken }),
  });

  const data = await response.json();
  if (!response.ok || !data.success) {
    throw new Error('Failed to refresh token');
  }

  return data.data.tokens;
}

// ============================================================
// Auth API
// ============================================================

export async function login(email: string, password: string): Promise<AuthResponse> {
  return apiRequest<AuthResponse>('/api/auth/login', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  }, false);
}

export async function register(email: string, password: string, name: string): Promise<AuthResponse> {
  return apiRequest<AuthResponse>('/api/auth/register', {
    method: 'POST',
    body: JSON.stringify({ email, password, name }),
  }, false);
}

export async function getCurrentUser(): Promise<{ user: CloudUser }> {
  return apiRequest<{ user: CloudUser }>('/api/auth/me');
}

export function getGitHubAuthUrl(redirectUri?: string): string {
  const params = redirectUri ? `?redirect=${encodeURIComponent(redirectUri)}` : '';
  return `${CLOUD_API_URL}/api/auth/github${params}`;
}

export async function exchangeGitHubCode(code: string, state: string): Promise<AuthResponse> {
  return apiRequest<AuthResponse>('/api/auth/github/token', {
    method: 'POST',
    body: JSON.stringify({ code, state }),
  }, false);
}

// ============================================================
// Teams API
// ============================================================

export async function getTeams(): Promise<Team[]> {
  return apiRequest<Team[]>('/api/teams');
}

export async function getTeam(teamId: string): Promise<TeamWithMembers> {
  return apiRequest<TeamWithMembers>(`/api/teams/${teamId}`);
}

export async function createTeam(name: string, description?: string): Promise<Team> {
  return apiRequest<Team>('/api/teams', {
    method: 'POST',
    body: JSON.stringify({ name, description }),
  });
}

export async function updateTeam(teamId: string, data: { name?: string; description?: string }): Promise<Team> {
  return apiRequest<Team>(`/api/teams/${teamId}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

export async function deleteTeam(teamId: string): Promise<void> {
  await apiRequest(`/api/teams/${teamId}`, { method: 'DELETE' });
}

export async function inviteTeamMember(teamId: string, email: string, role: 'admin' | 'member' = 'member'): Promise<TeamInvitation> {
  return apiRequest<TeamInvitation>(`/api/teams/${teamId}/members`, {
    method: 'POST',
    body: JSON.stringify({ email, role }),
  });
}

export async function removeTeamMember(teamId: string, userId: string): Promise<void> {
  await apiRequest(`/api/teams/${teamId}/members/${userId}`, { method: 'DELETE' });
}

export async function updateTeamMemberRole(teamId: string, userId: string, role: 'owner' | 'admin' | 'member'): Promise<void> {
  await apiRequest(`/api/teams/${teamId}/members/${userId}`, {
    method: 'PUT',
    body: JSON.stringify({ role }),
  });
}

export async function getPendingInvitations(): Promise<TeamInvitation[]> {
  return apiRequest<TeamInvitation[]>('/api/teams/invitations/pending');
}

export async function acceptInvitation(token: string): Promise<{ team: Team }> {
  return apiRequest<{ team: Team }>('/api/teams/invitations/accept', {
    method: 'POST',
    body: JSON.stringify({ token }),
  });
}

// ============================================================
// Team Skills API
// ============================================================

export async function getTeamSkills(teamId: string): Promise<TeamSkill[]> {
  return apiRequest<TeamSkill[]>(`/api/teams/${teamId}/skills`);
}

export async function shareSkillWithTeam(teamId: string, skillId: string): Promise<TeamSkill> {
  return apiRequest<TeamSkill>(`/api/teams/${teamId}/skills`, {
    method: 'POST',
    body: JSON.stringify({ skill_id: skillId }),
  });
}

export async function createTeamSkill(teamId: string, name: string, content: string, description?: string): Promise<TeamSkill> {
  return apiRequest<TeamSkill>(`/api/teams/${teamId}/skills`, {
    method: 'POST',
    body: JSON.stringify({ name, content, description }),
  });
}

export async function updateTeamSkill(teamId: string, skillId: string, data: { name?: string; content?: string; description?: string; is_pinned?: boolean }): Promise<TeamSkill> {
  return apiRequest<TeamSkill>(`/api/teams/${teamId}/skills/${skillId}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

export async function deleteTeamSkill(teamId: string, skillId: string): Promise<void> {
  await apiRequest(`/api/teams/${teamId}/skills/${skillId}`, { method: 'DELETE' });
}

// ============================================================
// Sync API
// ============================================================

export interface SyncDevice {
  id: string;
  user_id: string;
  device_name: string;
  device_type: 'desktop' | 'laptop' | 'server' | 'other';
  device_id: string;
  os_name: string | null;
  os_version: string | null;
  app_version: string | null;
  last_sync_at: string | null;
  sync_enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface SyncConflict {
  skill_id: string;
  skill_name: string;
  local_hash: string;
  cloud_hash: string;
  local_updated_at: string;
  cloud_updated_at: string;
}

export interface SyncStatus {
  device: SyncDevice;
  lastSyncAt: string | null;
  pendingUploads: number;
  pendingDownloads: number;
  cloudSkillCount: number;
  conflicts: SyncConflict[];
}

export interface LocalSkillForSync {
  id?: string;
  file_path: string;
  name: string;
  description?: string;
  content: string;
  content_hash: string;
  source: 'personal' | 'project';
  updated_at: string;
}

export interface CloudSkill {
  id: string;
  user_id: string;
  name: string;
  description: string | null;
  file_path: string;
  source: 'personal' | 'project';
  content: string | null;
  token_count: number;
  enabled: boolean;
  content_hash: string | null;
  created_at: string;
  updated_at: string;
}

export interface SyncResult {
  uploaded: string[];
  downloaded: CloudSkill[];
  conflicts: SyncConflict[];
  syncTimestamp: string;
}

// Register device for syncing
export async function registerSyncDevice(device: {
  device_name: string;
  device_type: 'desktop' | 'laptop' | 'server' | 'other';
  device_id: string;
  os_name?: string;
  os_version?: string;
  app_version?: string;
}): Promise<SyncDevice> {
  return apiRequest<SyncDevice>('/api/skills/sync/register-device', {
    method: 'POST',
    body: JSON.stringify(device),
  });
}

// Get list of sync devices
export async function getSyncDevices(): Promise<SyncDevice[]> {
  return apiRequest<SyncDevice[]>('/api/skills/sync/devices');
}

// Remove a sync device
export async function removeSyncDevice(deviceId: string): Promise<void> {
  await apiRequest(`/api/skills/sync/devices/${deviceId}`, { method: 'DELETE' });
}

// Get sync status for a device
export async function getSyncStatus(deviceId: string): Promise<SyncStatus> {
  return apiRequest<SyncStatus>(`/api/skills/sync/status?device_id=${deviceId}`);
}

// Perform skill sync
export async function syncSkills(deviceId: string, localSkills: LocalSkillForSync[]): Promise<SyncResult> {
  return apiRequest<SyncResult>('/api/skills/sync', {
    method: 'POST',
    body: JSON.stringify({
      device_id: deviceId,
      skills: localSkills,
    }),
  });
}

// Resolve a sync conflict
export async function resolveSyncConflict(
  skillId: string,
  deviceId: string,
  resolution: 'keep_local' | 'keep_cloud' | 'merge',
  content?: string
): Promise<CloudSkill> {
  return apiRequest<CloudSkill>('/api/skills/sync/resolve-conflict', {
    method: 'POST',
    body: JSON.stringify({
      skill_id: skillId,
      device_id: deviceId,
      resolution,
      content,
    }),
  });
}

// Get all cloud skills
export async function getCloudSkills(): Promise<CloudSkill[]> {
  return apiRequest<CloudSkill[]>('/api/skills');
}

export { CloudApiError };
