// v2.6 PR-B chunk 5+ — Desktop client for the cloud provider-keys API.
//
// Backed by services/auth/src/provider-keys.ts on ato-cloud. The gateway
// rewrites /api/provider-keys → /auth/me/provider-keys internally; the
// desktop only talks to the public /api path. All routes are Pro-tier
// gated server-side; the desktop additionally checks isCloudUser before
// rendering the page.
//
// SECURITY: the user types the plaintext key once. We POST it over TLS;
// the server encrypts under AES-256-GCM (AAD bound to user_id + provider)
// and never returns the plaintext back. The list endpoint returns only
// metadata (key_prefix sigil + audit fields). Revoke is a soft delete
// the cron observes — the row stays for audit history.

// Matches the local-redeclaration pattern in agentTraceUpload.ts,
// cloudAgentTraces.ts, and agentJudge.ts. CLOUD_API_URL is not exported
// from cloud-api.ts; modules redeclare it.
const CLOUD_API_URL =
  (import.meta.env.VITE_CLOUD_API_URL as string | undefined) ||
  "https://api.agentictool.ai";

export type ProviderSlug =
  | "openai"
  | "anthropic_org"
  | "gemini"
  | "minimax"
  | "openrouter"
  | "deepseek"
  | "groq"
  | "together";

export type LastPollStatus =
  | "ok"
  | "auth_failed"
  | "rate_limited"
  | "provider_error"
  | "unsupported_provider"
  | "timeout";

export interface ProviderKey {
  id: string;
  provider: ProviderSlug;
  label: string | null;
  keyPrefix: string;
  keyVersion: number;
  lastPolledAt: string | null;
  lastPollStatus: LastPollStatus | null;
  createdAt: string;
  revokedAt: string | null;
}

export interface CreateProviderKeyInput {
  provider: ProviderSlug;
  key: string;
  label?: string;
}

export interface CreatedProviderKey {
  id: string;
  provider: ProviderSlug;
  label: string | null;
  keyPrefix: string;
  keyVersion: number;
  createdAt: string;
}

/** Catalog driving the form: slug → display name + signup URL.
 *  Order reflects v2.6 poll-viable status (openai + anthropic_org first)
 *  then the gap providers covered by PR-D (roadmap). */
export const PROVIDER_CATALOG: ReadonlyArray<{
  slug: ProviderSlug;
  displayName: string;
  pollStatus: "viable" | "balance-only" | "no-aggregate";
  signupUrl: string;
}> = [
  {
    slug: "openai",
    displayName: "OpenAI",
    pollStatus: "viable",
    signupUrl: "https://platform.openai.com/api-keys",
  },
  {
    slug: "anthropic_org",
    displayName: "Anthropic (org admin key)",
    pollStatus: "viable",
    signupUrl: "https://console.anthropic.com/settings/admin-keys",
  },
  {
    slug: "deepseek",
    displayName: "DeepSeek",
    pollStatus: "balance-only",
    signupUrl: "https://platform.deepseek.com/api_keys",
  },
  {
    slug: "minimax",
    displayName: "MiniMax",
    pollStatus: "balance-only",
    signupUrl: "https://platform.minimax.io/user-center/basic-information/interface-key",
  },
  {
    slug: "gemini",
    displayName: "Google Gemini",
    pollStatus: "no-aggregate",
    signupUrl: "https://aistudio.google.com/apikey",
  },
  {
    slug: "groq",
    displayName: "Groq",
    pollStatus: "no-aggregate",
    signupUrl: "https://console.groq.com/keys",
  },
  {
    slug: "openrouter",
    displayName: "OpenRouter",
    pollStatus: "no-aggregate",
    signupUrl: "https://openrouter.ai/settings/keys",
  },
  {
    slug: "together",
    displayName: "Together AI",
    pollStatus: "no-aggregate",
    signupUrl: "https://api.together.ai/settings/api-keys",
  },
];

function authHeader(accessToken: string | null): Record<string, string> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (accessToken) headers["Authorization"] = `Bearer ${accessToken}`;
  return headers;
}

interface Envelope<T> {
  success: boolean;
  data?: T;
  error?: { code: string; message: string; details?: unknown };
}

async function unwrap<T>(resp: Response): Promise<T> {
  const body = (await resp.json()) as Envelope<T>;
  if (!resp.ok || !body.success || body.data === undefined) {
    const code = body.error?.code ?? "UNKNOWN_ERROR";
    const message = body.error?.message ?? `Request failed (${resp.status})`;
    throw new Error(`${code}: ${message}`);
  }
  return body.data;
}

export async function listProviderKeys(accessToken: string | null): Promise<ProviderKey[]> {
  const resp = await fetch(`${CLOUD_API_URL}/api/provider-keys`, {
    method: "GET",
    headers: authHeader(accessToken),
  });
  return unwrap<ProviderKey[]>(resp);
}

export async function createProviderKey(
  accessToken: string | null,
  input: CreateProviderKeyInput
): Promise<CreatedProviderKey> {
  const resp = await fetch(`${CLOUD_API_URL}/api/provider-keys`, {
    method: "POST",
    headers: authHeader(accessToken),
    body: JSON.stringify(input),
  });
  return unwrap<CreatedProviderKey>(resp);
}

export async function revokeProviderKey(
  accessToken: string | null,
  id: string
): Promise<{ id: string; revokedAt: string }> {
  const resp = await fetch(`${CLOUD_API_URL}/api/provider-keys/${id}/revoke`, {
    method: "POST",
    headers: authHeader(accessToken),
  });
  return unwrap<{ id: string; revokedAt: string }>(resp);
}
