// v2.16 Wave 1 — shared API helpers for the web client.
//
// Mirrors a small read-only slice of apps/desktop/src/lib/cloud-api.ts.
// Web is read-only: list teams + browse shared resources + live-stream
// plaintext events. E2E shares show "Open in desktop" — no decrypt path
// because the user's private key lives in the desktop keychain.

export const API_BASE =
  (import.meta.env.VITE_API_URL as string | undefined) ||
  "https://api.agentictool.ai/api";

export const WS_BASE =
  (import.meta.env.VITE_WS_URL as string | undefined) ||
  "wss://api.agentictool.ai";

// ──────────────────────────────────────────────────────────────────
// Auth — JWT bearer from localStorage. Same shape WebDashboard uses.
// ──────────────────────────────────────────────────────────────────

interface StoredAuth {
  state: {
    accessToken: string;
    refreshToken: string;
    user: { id: string; email: string; name: string };
  };
}

export function getStoredAuth(): StoredAuth["state"] | null {
  try {
    const raw = localStorage.getItem("ato-auth");
    if (!raw) return null;
    const parsed = JSON.parse(raw) as StoredAuth;
    return parsed.state ?? null;
  } catch {
    return null;
  }
}

export class ApiError extends Error {
  constructor(public code: string, message: string, public status: number) {
    super(message);
    this.name = "ApiError";
  }
}

interface ApiResponse<T> {
  success: boolean;
  data?: T;
  error?: { code: string; message: string };
}

export async function apiRequest<T>(
  path: string,
  init?: RequestInit,
): Promise<T> {
  const auth = getStoredAuth();
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(init?.headers as Record<string, string> | undefined),
  };
  if (auth?.accessToken) {
    headers.Authorization = `Bearer ${auth.accessToken}`;
  }
  const res = await fetch(`${API_BASE}${path}`, { ...init, headers });
  const body = (await res.json().catch(() => ({}))) as ApiResponse<T>;
  if (!res.ok || !body.success) {
    const code = body.error?.code ?? `HTTP_${res.status}`;
    const message = body.error?.message ?? `${res.status} ${res.statusText}`;
    throw new ApiError(code, message, res.status);
  }
  return body.data as T;
}

// ──────────────────────────────────────────────────────────────────
// Types — mirror cloud row shapes
// ──────────────────────────────────────────────────────────────────

export interface TeamRow {
  id: string;
  name: string;
  slug: string;
  role: "owner" | "admin" | "member";
}

export type SharedResourceKind =
  | "session"
  | "war-room"
  | "chat"
  | "loop"
  | "mission";

// URL segment + display label for each kind. The cloud routes use
// hyphens (`/war-rooms`), the share-row discriminator in v2.14 uses
// underscores (`war_room`). The web client only ever needs the URL
// form because it never touches the local share schema.
export const RESOURCE_KIND_META: Record<
  SharedResourceKind,
  { label: string; segment: string }
> = {
  session: { label: "Sessions", segment: "sessions" },
  "war-room": { label: "War rooms", segment: "war-rooms" },
  chat: { label: "Chats", segment: "chats" },
  loop: { label: "Loops", segment: "loops" },
  mission: { label: "Missions", segment: "missions" },
};

export interface SharedRow {
  // Each parent table has its own id column (session_id, war_room_id,
  // chat_thread_id, loop_id, mission_id) but the list endpoint returns
  // it under the spec's idColumn. We normalize to resource_id here.
  resource_id: string;
  shared_by_user_id: string;
  shared_at: string;
  expires_at: string | null;
  title?: string | null;
  runtime?: string | null;
  agent_slug?: string | null;
  turn_count?: number | null;
}

export interface SharedDetail {
  resource_id: string;
  shared_by_user_id: string;
  shared_at: string;
  expires_at: string | null;
  snapshot: unknown; // Schema-loose JSON; rendered by per-kind helpers.
  encryption_mode: "plaintext" | "e2e";
  last_seq: number;
  team_key_id: string | null;
  title?: string | null;
  runtime?: string | null;
  agent_slug?: string | null;
  turn_count?: number | null;
}

export interface TeamEvent {
  seq_num: number;
  event_kind: string;
  payload_json: unknown | null;
  ciphertext_b64: string | null;
  nonce_b64: string | null;
  encryption_mode: "plaintext" | "e2e";
  signature_b64: string | null;
  signer_key_id: string | null;
  initiator_user_id: string | null;
  initiator_runtime: string | null;
  initiator_agent_slug: string | null;
  surface: "desktop" | "cli" | "web" | "mcp" | "cron";
  created_at: string;
}

// ──────────────────────────────────────────────────────────────────
// Endpoints
// ──────────────────────────────────────────────────────────────────

export async function listTeams(): Promise<TeamRow[]> {
  return apiRequest<TeamRow[]>("/teams");
}

// ──────────────────────────────────────────────────────────────────
// v2.18.1 — Team CRUD + member management from web
// ──────────────────────────────────────────────────────────────────

export interface TeamDetail extends TeamRow {
  created_at: string;
  member_count: number;
  plan: string;
}

export interface TeamMember {
  user_id: string;
  email: string;
  name: string | null;
  role: "owner" | "admin" | "member";
  joined_at: string;
  invite_pending: boolean;
}

export async function createTeam(name: string): Promise<TeamRow> {
  return apiRequest<TeamRow>("/teams", {
    method: "POST",
    body: JSON.stringify({ name }),
  });
}

export async function getTeam(id: string): Promise<TeamDetail> {
  return apiRequest<TeamDetail>(`/teams/${id}`);
}

export async function renameTeam(id: string, name: string): Promise<TeamRow> {
  return apiRequest<TeamRow>(`/teams/${id}`, {
    method: "PUT",
    body: JSON.stringify({ name }),
  });
}

export async function deleteTeam(id: string): Promise<void> {
  await apiRequest<void>(`/teams/${id}`, { method: "DELETE" });
}

export async function listTeamMembers(id: string): Promise<TeamMember[]> {
  // The cloud surfaces members under GET /teams/:id with a `members` array
  // (verified in services/teams/src/routes.ts:186). Fall through to that
  // path so we don't need a separate endpoint round-trip.
  const detail = await apiRequest<TeamDetail & { members?: TeamMember[] }>(
    `/teams/${id}`,
  );
  return detail.members ?? [];
}

export async function inviteTeamMember(
  id: string,
  email: string,
  role: "admin" | "member" = "member",
): Promise<{ invite_id: string }> {
  return apiRequest<{ invite_id: string }>(`/teams/${id}/members`, {
    method: "POST",
    body: JSON.stringify({ email, role }),
  });
}

export async function updateTeamMemberRole(
  teamId: string,
  memberId: string,
  role: "admin" | "member",
): Promise<void> {
  await apiRequest<void>(`/teams/${teamId}/members/${memberId}`, {
    method: "PUT",
    body: JSON.stringify({ role }),
  });
}

export async function removeTeamMember(
  teamId: string,
  memberId: string,
): Promise<void> {
  await apiRequest<void>(`/teams/${teamId}/members/${memberId}`, {
    method: "DELETE",
  });
}

// ──────────────────────────────────────────────────────────────────
// v2.18.1 — User profile + session controls
// ──────────────────────────────────────────────────────────────────

export interface UserProfile {
  id: string;
  email: string;
  name: string | null;
  created_at: string;
  plan: "free" | "pro" | "team" | "enterprise";
}

export async function getMe(): Promise<UserProfile> {
  return apiRequest<UserProfile>("/auth/me");
}

export async function signOut(): Promise<void> {
  await apiRequest<void>("/auth/logout", { method: "POST" });
}

export async function listSharedResources(
  teamId: string,
  kind: SharedResourceKind,
): Promise<SharedRow[]> {
  const seg = RESOURCE_KIND_META[kind].segment;
  // The cloud routes return rows keyed by the per-kind id column
  // (session_id, war_room_id, …). Normalize to `resource_id` so the
  // UI doesn't have to switch per kind.
  const raw = await apiRequest<Array<Record<string, unknown>>>(
    `/teams/${teamId}/${seg}`,
  );
  return raw.map((row) => normalizeSharedRow(kind, row));
}

export async function getSharedDetail(
  teamId: string,
  kind: SharedResourceKind,
  resourceId: string,
): Promise<SharedDetail> {
  const seg = RESOURCE_KIND_META[kind].segment;
  const raw = await apiRequest<Record<string, unknown>>(
    `/teams/${teamId}/${seg}/${resourceId}`,
  );
  return normalizeSharedDetail(kind, raw);
}

export async function backfillTeamEvents(
  teamId: string,
  kind: SharedResourceKind,
  resourceId: string,
  since: number,
  limit = 200,
): Promise<TeamEvent[]> {
  const seg = RESOURCE_KIND_META[kind].segment;
  return apiRequest<TeamEvent[]>(
    `/teams/${teamId}/${seg}/${resourceId}/events?since=${since}&limit=${limit}`,
  );
}

// ──────────────────────────────────────────────────────────────────
// Mesh presence-token mint — needed to open the team-events WS.
// ──────────────────────────────────────────────────────────────────

export interface PresenceCredentials {
  token: string;
  peerId: string;
  expiresAt: number; // unix ms
}

export async function mintPresenceToken(): Promise<PresenceCredentials | null> {
  const auth = getStoredAuth();
  if (!auth?.accessToken) return null;
  try {
    const data = await apiRequest<{
      token: string;
      peer_id: string;
      expires_at: string;
    }>("/auth/mesh-presence-token", { method: "POST" });
    return {
      token: data.token,
      peerId: data.peer_id,
      expiresAt: new Date(data.expires_at).getTime(),
    };
  } catch {
    return null;
  }
}

// ──────────────────────────────────────────────────────────────────
// Internal — per-kind id column normalization
// ──────────────────────────────────────────────────────────────────

const ID_COLUMN: Record<SharedResourceKind, string> = {
  session: "session_id",
  "war-room": "war_room_id",
  chat: "chat_thread_id",
  loop: "loop_id",
  mission: "mission_id",
};

function normalizeSharedRow(
  kind: SharedResourceKind,
  row: Record<string, unknown>,
): SharedRow {
  return {
    resource_id: String(row[ID_COLUMN[kind]] ?? ""),
    shared_by_user_id: String(row.shared_by_user_id ?? ""),
    shared_at: String(row.shared_at ?? ""),
    expires_at: (row.expires_at as string | null) ?? null,
    title: (row.title as string | null) ?? null,
    runtime: (row.runtime as string | null) ?? null,
    agent_slug: (row.agent_slug as string | null) ?? null,
    turn_count: (row.turn_count as number | null) ?? null,
  };
}

function normalizeSharedDetail(
  kind: SharedResourceKind,
  row: Record<string, unknown>,
): SharedDetail {
  const base = normalizeSharedRow(kind, row);
  return {
    ...base,
    snapshot: row.snapshot ?? {},
    encryption_mode:
      row.encryption_mode === "e2e" ? "e2e" : "plaintext",
    last_seq: typeof row.last_seq === "number" ? row.last_seq : 0,
    team_key_id: (row.team_key_id as string | null) ?? null,
  };
}
