/**
 * ATO Cloud API Client
 * Handles communication with the cloud backend for sync, teams, and auth
 */

// Default points at the real cloud; override via VITE_CLOUD_API_URL for
// local dev. The previous default was an unrouted `api.ato.dev` which
// caused every signup to fail with "Load failed" before this file ever
// hit the network.
const CLOUD_API_URL = import.meta.env.VITE_CLOUD_API_URL || 'https://api.agentictool.ai';

// ============================================================
// Types
// ============================================================

export type CloudSubscriptionTier = 'free' | 'pro' | 'team' | 'enterprise';

export interface CloudUser {
  id: string;
  email: string;
  name: string;
  avatar_url: string | null;
  auth_method: 'email' | 'oauth';
  github_id: string | null;
  github_username: string | null;
  /** From users.subscription_tier — drives TierGate. May be missing on
   *  older backends that haven't shipped the migration yet; treat as 'free'. */
  subscription_tier?: CloudSubscriptionTier;
  /** v2.12 (2026-05-26) — From users.email_verified. False on fresh
   *  signups until the user clicks the verification link. Drives the
   *  email-verification banner in the desktop. Optional for back-compat
   *  with older /auth/me responses; treat absence as "verified" so
   *  pre-migration users don't see a stale banner. */
  email_verified?: boolean;
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
// v2.13 — Team Shared Agents / Methodologies API (Team tier)
// ============================================================

export interface SharedTeamAgent {
  team_id: string;
  agent_id: string;
  shared_by_user_id: string;
  shared_at: string;
  slug: string;
  display_name: string;
  description: string | null;
  runtime: string;
  model: string | null;
  shared_by_email: string | null;
  shared_by_name: string | null;
}

export interface SharedTeamMethodology {
  team_id: string;
  methodology_id: string;
  shared_by_user_id: string;
  slug: string;
  name: string;
  description: string | null;
  config: unknown;
  shared_at: string;
  updated_at: string;
  shared_by_email: string | null;
  shared_by_name: string | null;
}

export interface SharedTeamSession {
  team_id: string;
  session_id: string;
  shared_by_user_id: string;
  shared_at: string;
  snapshot: unknown;
  // v2.14 — list endpoint excludes snapshot (kept for v1 back-compat
  // typings) but DOES include the extra columns the cloud route
  // surfaces: title / runtime / agent_slug / turn_count.
  title?: string | null;
  runtime?: string | null;
  agent_slug?: string | null;
  turn_count?: number | null;
  expires_at?: string | null;
}

export interface SharedTeamWarRoom {
  team_id: string;
  war_room_id: string;
  shared_by_user_id: string;
  shared_at: string;
  snapshot: unknown;
  title?: string | null;
  expires_at?: string | null;
}

export interface SharedTeamChat {
  team_id: string;
  // Codex final-review F2: cloud wire shape uses chat_thread_id
  // (matches the OSS-local chat_threads table). Earlier OSS wrappers
  // typed it as chat_id which made every share/unshare/list call
  // fail validation server-side. Aligned to chat_thread_id.
  chat_thread_id: string;
  shared_by_user_id: string;
  shared_at: string;
  snapshot: unknown;
  title?: string | null;
  runtime?: string | null;
  expires_at?: string | null;
}

export async function getTeamSharedAgents(teamId: string): Promise<SharedTeamAgent[]> {
  return apiRequest<SharedTeamAgent[]>(`/api/teams/${teamId}/agents`);
}

export async function shareAgentWithTeam(teamId: string, agentId: string): Promise<{ team_id: string; agent_id: string; shared_at: string }> {
  return apiRequest(`/api/teams/${teamId}/agents/share`, {
    method: 'POST',
    body: JSON.stringify({ agent_id: agentId }),
  });
}

export async function unshareAgentFromTeam(teamId: string, agentId: string): Promise<void> {
  await apiRequest(`/api/teams/${teamId}/agents/${agentId}/share`, { method: 'DELETE' });
}

export async function getTeamSharedMethodologies(teamId: string): Promise<SharedTeamMethodology[]> {
  return apiRequest<SharedTeamMethodology[]>(`/api/teams/${teamId}/methodologies`);
}

export async function shareMethodologyWithTeam(
  teamId: string,
  payload: { methodology_id: string; slug: string; name: string; description?: string; config: unknown }
): Promise<{ team_id: string; methodology_id: string; shared_at: string }> {
  return apiRequest(`/api/teams/${teamId}/methodologies/share`, {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function unshareMethodologyFromTeam(teamId: string, methodologyId: string): Promise<void> {
  await apiRequest(`/api/teams/${teamId}/methodologies/${methodologyId}/share`, { method: 'DELETE' });
}

export async function shareSessionWithTeam(teamId: string, sessionId: string, payload: unknown): Promise<{ team_id: string; session_id: string; shared_at: string }> {
  return apiRequest(`/api/teams/${teamId}/sessions/share`, {
    method: 'POST',
    body: JSON.stringify({ session_id: sessionId, ...((payload as Record<string, unknown>) ?? {}) }),
  });
}

export async function shareWarRoomWithTeam(teamId: string, warRoomId: string, payload: unknown): Promise<{ team_id: string; war_room_id: string; shared_at: string }> {
  return apiRequest(`/api/teams/${teamId}/war-rooms/share`, {
    method: 'POST',
    body: JSON.stringify({ war_room_id: warRoomId, ...((payload as Record<string, unknown>) ?? {}) }),
  });
}

export async function shareChatWithTeam(teamId: string, chatId: string, payload: unknown): Promise<{ team_id: string; chat_thread_id: string; shared_at: string }> {
  // Codex final-review F2: cloud schema is chat_thread_id (matches
  // OSS chat_threads.id). Use the canonical key on the wire.
  return apiRequest(`/api/teams/${teamId}/chats/share`, {
    method: 'POST',
    body: JSON.stringify({ chat_thread_id: chatId, ...((payload as Record<string, unknown>) ?? {}) }),
  });
}

export async function unshareSessionFromTeam(teamId: string, sessionId: string): Promise<void> {
  await apiRequest(`/api/teams/${teamId}/sessions/${sessionId}/share`, { method: 'DELETE' });
}

export async function unshareWarRoomFromTeam(teamId: string, warRoomId: string): Promise<void> {
  await apiRequest(`/api/teams/${teamId}/war-rooms/${warRoomId}/share`, { method: 'DELETE' });
}

export async function unshareChatFromTeam(teamId: string, chatId: string): Promise<void> {
  await apiRequest(`/api/teams/${teamId}/chats/${chatId}/share`, { method: 'DELETE' });
}

export async function getSharedSessions(teamId: string): Promise<SharedTeamSession[]> {
  return apiRequest<SharedTeamSession[]>(`/api/teams/${teamId}/sessions`);
}

export async function getSharedWarRooms(teamId: string): Promise<SharedTeamWarRoom[]> {
  return apiRequest<SharedTeamWarRoom[]>(`/api/teams/${teamId}/war-rooms`);
}

export async function getSharedChats(teamId: string): Promise<SharedTeamChat[]> {
  return apiRequest<SharedTeamChat[]>(`/api/teams/${teamId}/chats`);
}

// v2.14 — single-share detail (snapshot blob included).
// Used by the read-only SharedDetailView (#6) for teammates who don't
// have the local row.
export interface SharedSessionDetail extends SharedTeamSession {
  snapshot: Record<string, unknown>;
  expires_at: string | null;
}
export interface SharedWarRoomDetail extends SharedTeamWarRoom {
  snapshot: Record<string, unknown>;
  expires_at: string | null;
}
export interface SharedChatDetail extends SharedTeamChat {
  snapshot: Record<string, unknown>;
  expires_at: string | null;
}

export async function getSharedSessionDetail(teamId: string, sessionId: string): Promise<SharedSessionDetail> {
  return apiRequest<SharedSessionDetail>(`/api/teams/${teamId}/sessions/${sessionId}`);
}

// v2.14 #12 — loops + missions sharing. Same shape as the other
// shared resources; the desktop cloud-api wrappers mirror the cloud
// routes added in migrations 033.
export interface SharedTeamLoop {
  team_id: string;
  loop_id: string;
  shared_by_user_id: string;
  shared_at: string;
  snapshot: unknown;
  title?: string | null;
  expires_at?: string | null;
}

export interface SharedTeamMission {
  team_id: string;
  mission_id: string;
  shared_by_user_id: string;
  shared_at: string;
  snapshot: unknown;
  title?: string | null;
  expires_at?: string | null;
}

export interface SharedLoopDetail extends SharedTeamLoop {
  snapshot: Record<string, unknown>;
  expires_at: string | null;
}
export interface SharedMissionDetail extends SharedTeamMission {
  snapshot: Record<string, unknown>;
  expires_at: string | null;
}

export async function shareLoopWithTeam(teamId: string, loopId: string, payload: unknown): Promise<{ team_id: string; loop_id: string; shared_at: string }> {
  return apiRequest(`/api/teams/${teamId}/loops/share`, {
    method: 'POST',
    body: JSON.stringify({ loop_id: loopId, ...((payload as Record<string, unknown>) ?? {}) }),
  });
}

export async function shareMissionWithTeam(teamId: string, missionId: string, payload: unknown): Promise<{ team_id: string; mission_id: string; shared_at: string }> {
  return apiRequest(`/api/teams/${teamId}/missions/share`, {
    method: 'POST',
    body: JSON.stringify({ mission_id: missionId, ...((payload as Record<string, unknown>) ?? {}) }),
  });
}

export async function unshareLoopFromTeam(teamId: string, loopId: string): Promise<void> {
  await apiRequest(`/api/teams/${teamId}/loops/${loopId}/share`, { method: 'DELETE' });
}

export async function unshareMissionFromTeam(teamId: string, missionId: string): Promise<void> {
  await apiRequest(`/api/teams/${teamId}/missions/${missionId}/share`, { method: 'DELETE' });
}

export async function getSharedLoops(teamId: string): Promise<SharedTeamLoop[]> {
  return apiRequest<SharedTeamLoop[]>(`/api/teams/${teamId}/loops`);
}

export async function getSharedMissions(teamId: string): Promise<SharedTeamMission[]> {
  return apiRequest<SharedTeamMission[]>(`/api/teams/${teamId}/missions`);
}

export async function getSharedLoopDetail(teamId: string, loopId: string): Promise<SharedLoopDetail> {
  return apiRequest<SharedLoopDetail>(`/api/teams/${teamId}/loops/${loopId}`);
}

export async function getSharedMissionDetail(teamId: string, missionId: string): Promise<SharedMissionDetail> {
  return apiRequest<SharedMissionDetail>(`/api/teams/${teamId}/missions/${missionId}`);
}

// v2.14 #14 — team activity feed entry. The cloud route exposes a
// generic activity log; the feed UI filters for share-related actions.
export interface TeamActivityEntry {
  id: string;
  team_id: string;
  user_id: string;
  user_email?: string | null;
  user_name?: string | null;
  action: string;
  resource_type: string | null;
  resource_id: string | null;
  resource_name: string | null;
  changes: Record<string, unknown> | null;
  created_at: string;
}

export async function getTeamActivity(teamId: string, limit = 50): Promise<TeamActivityEntry[]> {
  return apiRequest<TeamActivityEntry[]>(`/api/teams/${teamId}/activity?limit=${limit}`);
}

export async function getSharedWarRoomDetail(teamId: string, warRoomId: string): Promise<SharedWarRoomDetail> {
  return apiRequest<SharedWarRoomDetail>(`/api/teams/${teamId}/war-rooms/${warRoomId}`);
}

export async function getSharedChatDetail(teamId: string, chatId: string): Promise<SharedChatDetail> {
  return apiRequest<SharedChatDetail>(`/api/teams/${teamId}/chats/${chatId}`);
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

// v2.1.0 — Embed key for deployed-bundle trace ingestion. Mint-on-read
// (the cloud generates the key the first time GET hits and persists
// it). Pro+ only — free tier gets a 403 + TIER_REQUIRED error code.
//
// Mock mode: returns a fixed fixture key so the Deploy tab's panel
// can be verified without cloud sign-in.
export async function getEmbedKey(): Promise<string> {
  if (import.meta.env.VITE_USE_MOCK_CLOUD === "true") {
    return "eba_MOCK1234ABCDEFGHJKLMNPQRSTUVWXYZ23";
  }
  const data = await apiRequest<{ embedKey: string }>('/api/auth/me/embed-key');
  return data.embedKey;
}

// Rotates the embed key — old key stops working immediately. Use when
// a deployed bundle's key is suspected leaked. Returns the fresh key.
export async function rotateEmbedKey(): Promise<string> {
  if (import.meta.env.VITE_USE_MOCK_CLOUD === "true") {
    return "eba_MOCK_ROTATED_VWXYZ23456789ABCDEFG";
  }
  const data = await apiRequest<{ embedKey: string }>('/api/auth/me/embed-key/rotate', {
    method: 'POST',
  });
  return data.embedKey;
}

// ============================================================
// v2.15 Wave 1 — E2E Key Management API
// ============================================================
// (crypto imports consolidated at Wave 3 block below)

/**
 * Publish (or rotate) this user's E2E public keys.
 * The cloud stores them indexed by key_id so team admins can seal Team Keys
 * to each member's current X25519 public key.
 * Returns the key_id assigned by the cloud and whether a previous key was rotated.
 */
export async function pushE2ePublicKeys(
  x25519PublicKey: Uint8Array,
  ed25519PublicKey: Uint8Array,
): Promise<{ id: string; rotated: boolean }> {
  return apiRequest<{ id: string; rotated: boolean }>('/api/auth/me/e2e-keys', {
    method: 'POST',
    body: JSON.stringify({
      x25519_pubkey: toBase64(x25519PublicKey),
      ed25519_pubkey: toBase64(ed25519PublicKey),
    }),
  });
}

/**
 * List all E2E public keys for members of a team.
 * Used by team admins to seal the Team Key for each member.
 */
export async function getTeamMemberE2eKeys(
  teamId: string,
): Promise<
  Array<{
    member_user_id: string;
    key_id: string;
    x25519_pubkey: string;
    ed25519_pubkey: string;
  }>
> {
  return apiRequest(`/api/auth/me/e2e-keys?team_id=${encodeURIComponent(teamId)}`);
}

/**
 * Fetch the sealed Team Key envelope for the currently-authenticated user.
 * The cloud returns the envelope that was sealed to this user's X25519 public key.
 */
export async function getTeamKeyEnvelope(teamKeyId: string): Promise<{
  team_key_id: string;
  sealed_key: string;
  sealed_by_user_id: string;
  created_at: string;
}> {
  return apiRequest(
    `/api/auth/me/e2e-envelope?team_key_id=${encodeURIComponent(teamKeyId)}`,
  );
}

// ============================================================
// v2.15 Wave 2 — Team Event Log API
// ============================================================

/**
 * A single event row from a team_shared_<kind>_events table.
 * Plaintext shares populate payload_json; E2E shares (Wave 3)
 * populate ciphertext_b64 + nonce_b64 instead.
 */
export interface TeamEvent {
  seq_num: number;
  event_kind: string;
  payload_json: unknown | null;
  ciphertext_b64: string | null;
  nonce_b64: string | null;
  signature_b64: string | null;
  signer_key_id: string | null;
  initiator_user_id: string | null;
  initiator_runtime: string | null;
  initiator_agent_slug: string | null;
  surface: 'desktop' | 'cli' | 'web' | 'mcp' | 'cron';
  created_at: string;
}

/**
 * The five resource kinds that have per-kind event tables.
 * URL segment uses hyphens (war-room → /war-rooms/:id/events).
 */
export type SharedResourceKind =
  | 'session'
  | 'war-room'
  | 'chat'
  | 'loop'
  | 'mission';

/** Map a SharedResourceKind to its URL path segment (plural). */
function kindToPathSegment(kind: SharedResourceKind): string {
  const map: Record<SharedResourceKind, string> = {
    session: 'sessions',
    'war-room': 'war-rooms',
    chat: 'chats',
    loop: 'loops',
    mission: 'missions',
  };
  return map[kind];
}

/**
 * Append a new event to a team-shared resource.
 * On success the server returns the assigned seq_num (monotonically
 * increasing per resource, assigned under a row-locked counter) and
 * the server-side created_at timestamp.
 */
export async function appendTeamEvent(
  teamId: string,
  kind: SharedResourceKind,
  resourceId: string,
  body: {
    event_kind: string;
    payload_json?: unknown;
    ciphertext_b64?: string;
    nonce_b64?: string;
    signature_b64?: string;
    signer_key_id?: string;
    initiator_runtime?: string;
    initiator_agent_slug?: string;
    surface: 'desktop' | 'cli' | 'web' | 'mcp' | 'cron';
  },
): Promise<{ seq_num: number; created_at: string }> {
  const segment = kindToPathSegment(kind);
  return apiRequest<{ seq_num: number; created_at: string }>(
    `/api/teams/${teamId}/${segment}/${resourceId}/events`,
    {
      method: 'POST',
      body: JSON.stringify(body),
    },
  );
}

/**
 * Backfill events for a resource since a given seq_num (exclusive).
 * Used on WS reconnect and on the initial SharedDetailView mount to
 * fetch events that arrived after the REST snapshot was taken.
 */
export async function backfillTeamEvents(
  teamId: string,
  kind: SharedResourceKind,
  resourceId: string,
  since: number,
  limit = 200,
): Promise<TeamEvent[]> {
  const segment = kindToPathSegment(kind);
  return apiRequest<TeamEvent[]>(
    `/api/teams/${teamId}/${segment}/${resourceId}/events?since=${since}&limit=${limit}`,
  );
}

export { CloudApiError };

// ============================================================
// v2.15 Wave 3 — E2E encrypted event append + share metadata
// ============================================================

import {
  encryptPayload,
  signMessage,
  toBase64,
  fromBase64,
} from '@/lib/e2e/crypto';

/**
 * Reserve the next seq_num for an E2E-encrypted event append.
 * The reservation TTL is 30s server-side; the client must commit within that window.
 */
async function reserveEventSeq(
  teamId: string,
  kind: SharedResourceKind,
  resourceId: string,
): Promise<number> {
  const segment = kindToPathSegment(kind);
  const result = await apiRequest<{ seq_num: number }>(
    `/api/teams/${teamId}/${segment}/${resourceId}/events/reserve`,
    { method: 'POST', body: JSON.stringify({}) },
  );
  return result.seq_num;
}

/**
 * Append an E2E-encrypted event to a team-shared resource.
 *
 * Two-step protocol (Wave 3 REWORK, synthesis Q6):
 *   1. Reserve seq_num via POST .../events/reserve.
 *   2. Build AAD = utf8(`${teamId}|${resourceId}|${seq_num}|${eventKind}`).
 *   3. Encrypt payload with AEAD; sign (ciphertext || nonce || AD) with Ed25519.
 *   4. Commit via POST .../events with the reserved seq_num + encrypted blob.
 *
 * Callers (UI) invoke this when `getShareEncryptionMode` returns 'e2e'.
 * The existing plaintext `appendTeamEvent` is unchanged for plaintext shares.
 */
export async function appendTeamEventEncrypted(
  teamId: string,
  kind: SharedResourceKind,
  resourceId: string,
  eventKind: string,
  payloadJson: unknown,
  surface: 'desktop' | 'cli' | 'web' | 'mcp' | 'cron',
  teamKey: Uint8Array,
  signerEd25519PrivateKey: Uint8Array,
  signerKeyId: string,
): Promise<{ seq_num: number; created_at: string }> {
  // Step 1: reserve seq_num.
  const seqNum = await reserveEventSeq(teamId, kind, resourceId);

  // Step 2: build AEAD associated data.
  // Separator is literal `|`; all fields are UUIDs/numbers/short strings — no collision risk.
  const adStr = `${teamId}|${resourceId}|${seqNum}|${eventKind}`;
  const adBytes = new TextEncoder().encode(adStr);

  // Step 3: encrypt payload.
  const plaintextBytes = new TextEncoder().encode(JSON.stringify(payloadJson));
  const { nonce, ciphertext } = await encryptPayload(plaintextBytes, teamKey, adBytes);

  // Step 4: sign (ciphertext || nonce || AD).
  const signedBytes = new Uint8Array(ciphertext.length + nonce.length + adBytes.length);
  signedBytes.set(ciphertext, 0);
  signedBytes.set(nonce, ciphertext.length);
  signedBytes.set(adBytes, ciphertext.length + nonce.length);
  const signature = await signMessage(signedBytes, signerEd25519PrivateKey);

  // Step 5: commit.
  const segment = kindToPathSegment(kind);
  return apiRequest<{ seq_num: number; created_at: string }>(
    `/api/teams/${teamId}/${segment}/${resourceId}/events`,
    {
      method: 'POST',
      body: JSON.stringify({
        seq_num: seqNum,
        event_kind: eventKind,
        ciphertext_b64: toBase64(ciphertext),
        nonce_b64: toBase64(nonce),
        signature_b64: toBase64(signature),
        signer_key_id: signerKeyId,
        surface,
      }),
    },
  );
}

/**
 * Read the encryption_mode for a shared resource.
 * Returns 'e2e' when the share is end-to-end encrypted, 'plaintext' otherwise.
 * Reads from the existing GET .../detail response which already carries encryption_mode.
 */
export async function getShareEncryptionMode(
  teamId: string,
  kind: SharedResourceKind,
  resourceId: string,
): Promise<'plaintext' | 'e2e'> {
  const segment = kindToPathSegment(kind);
  const data = await apiRequest<{ encryption_mode?: string }>(
    `/api/teams/${teamId}/${segment}/${resourceId}`,
  );
  return data.encryption_mode === 'e2e' ? 'e2e' : 'plaintext';
}

/**
 * POST to flip a share's encryption_mode from plaintext → e2e.
 * Returns the updated encryption_mode ('e2e').
 * Server may return 409 HAS_PLAINTEXT_HISTORY if the resource already has plaintext events.
 */
export async function setShareEncryptionMode(
  teamId: string,
  kind: SharedResourceKind,
  resourceId: string,
  mode: 'e2e',
): Promise<void> {
  const segment = kindToPathSegment(kind);
  await apiRequest(
    `/api/teams/${teamId}/${segment}/${resourceId}/encryption-mode`,
    {
      method: 'POST',
      body: JSON.stringify({ mode }),
    },
  );
}

/**
 * Push key envelopes for a new Team Key rotation.
 * Use for first-time E2E setup on a share (creates a fresh team_keys generation
 * and fans the sealed Team Key out to every member).
 */
export async function pushKeyRotation(
  teamId: string,
  envelopes: Array<{
    member_user_id: string;
    key_id: string;
    sealed_key_b64: string;
  }>,
): Promise<{ team_key_id: string }> {
  return apiRequest<{ team_key_id: string }>(
    `/api/teams/${teamId}/key-rotations`,
    {
      method: 'POST',
      body: JSON.stringify({ envelopes }),
    },
  );
}

/**
 * Push sealed Team Key envelopes to an EXISTING team_keys generation.
 * Use when adding a new member under an already-live Team Key.
 */
export async function pushKeyEnvelopes(
  teamId: string,
  teamKeyId: string,
  envelopes: Array<{
    member_user_id: string;
    key_id: string;
    sealed_key_b64: string;
  }>,
): Promise<void> {
  await apiRequest(
    `/api/teams/${teamId}/key-envelopes`,
    {
      method: 'POST',
      body: JSON.stringify({ team_key_id: teamKeyId, envelopes }),
    },
  );
}

/**
 * Decrypt a single E2E event payload using the supplied team key and
 * ed25519 public key cache (member_user_id → {ed25519_pubkey: string, key_id: string}).
 *
 * Returns a new TeamEvent with payload_json populated, or with
 * { __decrypt_error: true } sentinel on any failure (bad sig, wrong key, etc.).
 * Never throws — errors are swallowed to sentinel so live events don't break the stream.
 */
export async function decryptEventPayload(
  raw: TeamEvent,
  teamKey: Uint8Array,
  memberPubkeys: Record<string, { ed25519_pubkey: string; key_id: string }>,
): Promise<TeamEvent> {
  if (!raw.ciphertext_b64 || !raw.nonce_b64) {
    // No ciphertext — already a plaintext event or a sentinel, pass through.
    return raw;
  }

  try {
    const { decryptPayload, verifyMessage } = await import('@/lib/e2e/crypto');

    const ciphertext = fromBase64(raw.ciphertext_b64);
    const nonce = fromBase64(raw.nonce_b64);

    // Verify signature before decrypting (authenticate-then-decrypt pattern).
    if (raw.signature_b64 && raw.signer_key_id && raw.initiator_user_id) {
      const signerPubkey = memberPubkeys[raw.initiator_user_id];
      if (signerPubkey) {
        const pubkeyBytes = fromBase64(signerPubkey.ed25519_pubkey);
        // AD must be reconstructed from known-good event metadata.
        // seq_num, event_kind are on the raw event; teamId + resourceId must be provided
        // at the call site. For the stream decryptor we pass via closure (see teamEventStream).
        // Here we pass them in the raw event itself via __ad_hint if available, else skip.
        if ((raw as TeamEvent & { __ad_hint?: string }).__ad_hint) {
          const adBytes = new TextEncoder().encode(
            (raw as TeamEvent & { __ad_hint?: string }).__ad_hint!,
          );
          const sigBytes = new Uint8Array(
            ciphertext.length + nonce.length + adBytes.length,
          );
          sigBytes.set(ciphertext, 0);
          sigBytes.set(nonce, ciphertext.length);
          sigBytes.set(adBytes, ciphertext.length + nonce.length);
          const sig = fromBase64(raw.signature_b64);
          const ok = await verifyMessage(sigBytes, sig, pubkeyBytes);
          if (!ok) {
            return { ...raw, payload_json: { __decrypt_error: true } };
          }
        }
      }
    }

    // AEAD AD = teamId|resourceId|seq_num|event_kind — must be provided at call site.
    // Use __ad_hint if set; otherwise decrypt without AD verification (safe because
    // the AEAD tag still covers authenticity of the ciphertext itself).
    const adHint = (raw as TeamEvent & { __ad_hint?: string }).__ad_hint;
    const adBytes = adHint
      ? new TextEncoder().encode(adHint)
      : new Uint8Array(0);

    const plaintext = await decryptPayload(ciphertext, nonce, teamKey, adBytes);
    const payloadJson = JSON.parse(new TextDecoder().decode(plaintext)) as unknown;

    return { ...raw, payload_json: payloadJson };
  } catch {
    return { ...raw, payload_json: { __decrypt_error: true } };
  }
}

/**
 * POST anonymized telemetry batch to the cloud.
 * Called by the hourly drain timer in App.tsx.
 */
export async function postAnonTelemetryBatch(
  entries: Array<{ id: number; data_json: string }>,
): Promise<void> {
  await apiRequest('/api/telemetry/e2e-anonymized', {
    method: 'POST',
    body: JSON.stringify({
      entries: entries.map((e) => JSON.parse(e.data_json) as unknown),
    }),
  });
}
