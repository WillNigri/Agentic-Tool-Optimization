// Runtime auth method preference (T6).
// Tracks which auth method (CLI subscription vs stored API key) the user has
// chosen as ACTIVE for each runtime. Both can be configured simultaneously;
// the active one is what outbound calls and the agent-suggest wizard use.
//
// Persistence: localStorage today. A future Rust-side migration will move this
// to ~/.ato/runtime-auth.json so the cron daemon can read it too.

export type RuntimeId = "claude" | "codex" | "gemini" | "openclaw" | "hermes";
export type AuthMethod = "subscription" | "apiKey";

export type RuntimeAuthState = Partial<Record<RuntimeId, AuthMethod>>;

const STORAGE_KEY = "ato.runtime-auth.v1";

export function loadRuntimeAuth(): RuntimeAuthState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as RuntimeAuthState;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

export function setRuntimeAuthMethod(runtime: RuntimeId, method: AuthMethod): RuntimeAuthState {
  const current = loadRuntimeAuth();
  const next = { ...current, [runtime]: method };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // ignore quota/perm errors
  }
  return next;
}

// Provider matching for LLM API keys → runtime.
export const RUNTIME_TO_PROVIDER: Record<RuntimeId, string[]> = {
  claude: ["anthropic"],
  codex: ["openai"],
  gemini: ["google", "gemini"],
  openclaw: ["openclaw", "anthropic", "openai"],
  hermes: ["hermes", "anthropic"],
};

export function isProviderForRuntime(provider: string, runtime: RuntimeId): boolean {
  const providers = RUNTIME_TO_PROVIDER[runtime];
  return providers ? providers.includes(provider.toLowerCase()) : false;
}
