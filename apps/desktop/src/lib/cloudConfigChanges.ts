// v2.1.0 — Frontend wrapper for /api/agent-config-changes.
//
// Pro+ desktop clients POST one row per meaningful agent config change
// (model swap, prompt edit, hook add) so the External Insights
// dashboard can overlay change markers on trace timelines.
//
// Best-effort: cloud is the source of truth, but local edits MUST NOT
// fail because the cloud is unreachable. Every call is wrapped in a
// silent .catch() at the call site.

import { useAuthStore } from "@/hooks/useAuth";
import { isMockMode, MOCK_CONFIG_CHANGES } from "@/lib/cloudMockData";

const CLOUD_API_URL =
  import.meta.env.VITE_CLOUD_API_URL || "https://api.agentictool.ai";

export type ConfigChangeField =
  // Genesis marker — emitted once per agent on create. Gives the
  // dashboard a "v0" baseline subsequent diffs chain off.
  | "created"
  | "model"
  | "runtime"
  | "system_prompt"
  | "description"
  | "variables"
  | "hooks"
  | "role_models"
  | "memory_policy"
  | "kind"
  | "permissions"
  | "mcps"
  | "skills";

export interface ConfigChange {
  id: string;
  agent_slug: string;
  field: ConfigChangeField | string;
  old_value: unknown;
  new_value: unknown;
  changed_by: string;
  metadata: Record<string, unknown>;
  changed_at: string;
}

interface RecordPayload {
  agentSlug: string;
  field: ConfigChangeField | string;
  oldValue?: unknown;
  newValue?: unknown;
  /** Defaults to `desktop:<email>` if not specified. */
  changedBy?: string;
  metadata?: Record<string, unknown>;
}

function authHeaders(): Record<string, string> | null {
  const { isCloudUser, accessToken } = useAuthStore.getState();
  if (!isCloudUser || !accessToken) return null;
  return {
    "Authorization": `Bearer ${accessToken}`,
    "Content-Type": "application/json",
  };
}

function defaultActor(): string {
  const { user } = useAuthStore.getState();
  return user?.email ? `desktop:${user.email}` : "desktop:anonymous";
}

/** Records a single config change. Best-effort — silently no-ops when:
 *  - the user isn't cloud-logged-in
 *  - the cloud is unreachable
 *  - the user is on Free tier (cloud returns 403, we swallow)
 *
 *  Callers should NOT await this if the local edit shouldn't be
 *  delayed by the network call. The promise is exposed so tests /
 *  diagnostic flows can observe success.
 */
export async function recordConfigChange(payload: RecordPayload): Promise<void> {
  // In mock mode, log to console and no-op the network call. Lets
  // the agent-edit paths run without sign-in and without mutating
  // real cloud state.
  if (isMockMode()) {
    console.log("[mock-cloud] recordConfigChange", payload);
    return;
  }
  const headers = authHeaders();
  if (!headers) return;

  // No-op if old and new are deep-equal — saves a roundtrip when a
  // form re-saves without actually changing anything.
  if (
    payload.oldValue !== undefined &&
    payload.newValue !== undefined &&
    JSON.stringify(payload.oldValue) === JSON.stringify(payload.newValue)
  ) {
    return;
  }

  try {
    await fetch(`${CLOUD_API_URL}/api/agent-config-changes`, {
      method: "POST",
      headers,
      body: JSON.stringify({
        agentSlug: payload.agentSlug,
        field: payload.field,
        oldValue: payload.oldValue,
        newValue: payload.newValue,
        changedBy: payload.changedBy ?? defaultActor(),
        metadata: payload.metadata ?? {},
      }),
    });
  } catch {
    // Silent — never block a local edit on telemetry.
  }
}

/** Reads recent config changes. Returns null when not signed in or
 *  blocked (free tier returns 403). */
export async function listConfigChanges(opts?: {
  agentSlug?: string;
  field?: string;
  days?: number;
  limit?: number;
}): Promise<{ changes: ConfigChange[]; days: number } | null> {
  if (isMockMode()) {
    let changes = [...MOCK_CONFIG_CHANGES];
    if (opts?.agentSlug) changes = changes.filter((c) => c.agent_slug === opts.agentSlug);
    if (opts?.field) changes = changes.filter((c) => c.field === opts.field);
    changes.sort((a, b) => b.changed_at.localeCompare(a.changed_at));
    if (opts?.limit) changes = changes.slice(0, opts.limit);
    return { changes, days: opts?.days ?? 30 };
  }
  const headers = authHeaders();
  if (!headers) return null;

  const params = new URLSearchParams();
  if (opts?.agentSlug) params.set("agentSlug", opts.agentSlug);
  if (opts?.field) params.set("field", opts.field);
  if (opts?.days) params.set("days", String(opts.days));
  if (opts?.limit) params.set("limit", String(opts.limit));

  try {
    const r = await fetch(
      `${CLOUD_API_URL}/api/agent-config-changes?${params.toString()}`,
      { headers },
    );
    if (!r.ok) {
      if (r.status === 401 || r.status === 403) return null;
      throw new Error(`config-changes GET: ${r.status}`);
    }
    const body = await r.json();
    return (body?.data ?? null) as { changes: ConfigChange[]; days: number } | null;
  } catch {
    return null;
  }
}
