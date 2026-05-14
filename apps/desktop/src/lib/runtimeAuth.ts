// Runtime auth method preference.
//
// Tracks which auth method (CLI subscription vs stored API key) the
// user has chosen as ACTIVE for each runtime. Both can be configured
// simultaneously; the active one is what outbound calls actually use.
//
// Persistence: SQLite `settings` table via Tauri commands, key
// `runtime_auth_mode.<runtime>`. The CLI dispatch path reads the same
// key, so cron jobs and headless runs honor the same preference as
// the desktop.
//
// Backward compat: this file previously persisted to localStorage
// with `AuthMethod = "subscription" | "apiKey"`. The on-disk format
// is `subscription | api_key` (snake case, to match Anthropic /
// OpenAI / Google env var naming conventions in the byok Rust
// module). Conversion happens at the boundary.

import { invoke } from "@tauri-apps/api/core";

export type RuntimeId = "claude" | "codex" | "gemini" | "openclaw" | "hermes";
export type AuthMethod = "subscription" | "apiKey";

export type RuntimeAuthState = Partial<Record<RuntimeId, AuthMethod>>;

/// Backend-shape from `get_runtime_auth_info`.
export interface RuntimeAuthInfo {
  runtime: string;
  userChoice: string | null; // "subscription" | "api_key" | null
  effective: string; // "subscription" | "api_key"
  hasKey: boolean;
  supportsByok: boolean;
}

const RUNTIMES: RuntimeId[] = ["claude", "codex", "gemini", "openclaw", "hermes"];

function fromWire(m: string | null | undefined): AuthMethod | undefined {
  if (m === "subscription") return "subscription";
  if (m === "api_key") return "apiKey";
  return undefined;
}

function toWire(m: AuthMethod): string {
  return m === "apiKey" ? "api_key" : "subscription";
}

/// Load the user's chosen auth method per runtime from SQLite. Only
/// returns explicit choices — runtimes without a stored preference
/// are absent from the result. The caller decides the default
/// (typically: use the stored API key if one exists, otherwise
/// subscription).
export async function loadRuntimeAuth(): Promise<RuntimeAuthState> {
  const out: RuntimeAuthState = {};
  await Promise.all(
    RUNTIMES.map(async (runtime) => {
      try {
        const info = await invoke<RuntimeAuthInfo>("get_runtime_auth_info", {
          runtime,
        });
        const choice = fromWire(info.userChoice);
        if (choice) out[runtime] = choice;
      } catch {
        // Runtime without BYOK mapping or transient error — leave absent.
      }
    }),
  );
  return out;
}

/// Set the user's preference for one runtime. Returns the resulting
/// full state for callers that want to update local component state
/// in one shot.
export async function setRuntimeAuthMethod(
  runtime: RuntimeId,
  method: AuthMethod,
): Promise<RuntimeAuthState> {
  await invoke("set_runtime_auth_mode", {
    runtime,
    mode: toWire(method),
  });
  return loadRuntimeAuth();
}

/// Clear the user's preference for one runtime (falls back to default
/// "use key if stored, else subscription").
export async function clearRuntimeAuthMethod(
  runtime: RuntimeId,
): Promise<RuntimeAuthState> {
  await invoke("set_runtime_auth_mode", { runtime, mode: "clear" });
  return loadRuntimeAuth();
}

/// Fetch the full info struct for a single runtime, including the
/// effective mode (what the next dispatch would actually use) and
/// whether a key is configured. Used by status badges that want to
/// show both intent and outcome.
export async function getRuntimeAuthInfo(
  runtime: RuntimeId,
): Promise<RuntimeAuthInfo | null> {
  try {
    return await invoke<RuntimeAuthInfo>("get_runtime_auth_info", { runtime });
  } catch {
    return null;
  }
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
