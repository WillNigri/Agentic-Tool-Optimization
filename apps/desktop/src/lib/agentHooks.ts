import { invoke } from "@tauri-apps/api/core";

// v1.4.0 F2 — Frontend wrappers for agent hooks.
// Hooks are pre-call resolvers that fetch data and inject it into a
// <context> block in the user message before each turn.

export type HookKind =
  | "file"          // Free
  | "webhook"       // Free
  | "mcp-call"      // Pro stub
  | "db-query"      // Pro stub
  | "computed";     // Pro stub

export const FREE_HOOK_KINDS: HookKind[] = ["file", "webhook"];
export const PRO_HOOK_KINDS: HookKind[] = ["mcp-call", "db-query", "computed"];

/** v2.0.0 — when a hook should fire, evaluated per-turn against the user
 *  message. 'always' is current behavior (every turn). 'keyword' fires when
 *  the user message matches one of `whenKeywords`. 'llm-decides' asks a
 *  cheap classifier (config.classifierModel/Provider) whether the
 *  user_prompt is described by `whenDescription`. */
export type HookFireMode = "always" | "keyword" | "llm-decides";

export interface AgentHook {
  id: string;
  agentId: string;
  position: number;
  name: string;
  kind: HookKind;
  configJson: string;
  enabled: boolean;
  createdAt: string;
  fireMode: HookFireMode;
}

/** Classifier provider + default model pairs for the llm-decides path.
 *  Classifier should be fast + cheap — these are the recommended models
 *  per provider. The picker can be overridden by the user. */
export const CLASSIFIER_MODELS: Record<string, string> = {
  anthropic: "claude-haiku-4-5",
  openai:    "gpt-4o-mini",
  groq:      "llama-3.1-8b-instant",
  xai:       "grok-2-latest",
  mistral:   "mistral-small-latest",
  deepseek:  "deepseek-chat",
  together:  "meta-llama/Llama-3.1-8B-Instruct-Turbo",
  fireworks: "accounts/fireworks/models/llama-v3p1-8b-instruct",
};

export const CLASSIFIER_PROVIDERS = Object.keys(CLASSIFIER_MODELS);

/** Optional fire-eval fields the UI may stuff into config_json on top of
 *  the hook-kind-specific config. The Rust side reads these from the
 *  same JSON blob in `should_fire_hook`. */
export interface FireEvalConfig {
  /** Used by fire_mode='keyword'. Any case-insensitive match fires the hook. */
  whenKeywords?: string[];
  /** Used by fire_mode='llm-decides'. Plain-language description of when to fire. */
  whenDescription?: string;
  /** Used by fire_mode='llm-decides'. Defaults to claude-haiku-4-5. */
  classifierModel?: string;
  /** Used by fire_mode='llm-decides'. Defaults to anthropic. */
  classifierProvider?: string;
}

export type HookConfig =
  | { kind: "file"; path: string; maxBytes?: number }
  | { kind: "webhook"; url: string; headers?: Record<string, string>; maxBytes?: number }
  | { kind: "mcp-call"; server: string; tool: string; args: Record<string, unknown> }
  | { kind: "db-query"; connection: string; sql: string }
  | { kind: "computed"; expr: string };

export function parseHookConfig(h: AgentHook): HookConfig {
  try {
    const obj = JSON.parse(h.configJson) as Record<string, unknown>;
    return { kind: h.kind, ...obj } as HookConfig;
  } catch {
    return { kind: h.kind } as HookConfig;
  }
}

export function hookConfigToJson(cfg: HookConfig, fireEval?: FireEvalConfig): string {
  const { kind: _kind, ...rest } = cfg as HookConfig & { kind: string };
  // Merge fire-eval fields into the same JSON blob — the Rust side
  // reads whenKeywords / whenDescription / classifierModel /
  // classifierProvider straight off of config_json.
  const merged: Record<string, unknown> = { ...rest };
  if (fireEval) {
    if (fireEval.whenKeywords !== undefined) merged.whenKeywords = fireEval.whenKeywords;
    if (fireEval.whenDescription !== undefined) merged.whenDescription = fireEval.whenDescription;
    if (fireEval.classifierModel !== undefined) merged.classifierModel = fireEval.classifierModel;
    if (fireEval.classifierProvider !== undefined) merged.classifierProvider = fireEval.classifierProvider;
  }
  return JSON.stringify(merged);
}

/** Pull fire-eval fields back out of an existing hook's configJson. */
export function parseFireEval(h: AgentHook): FireEvalConfig {
  try {
    const obj = JSON.parse(h.configJson) as Record<string, unknown>;
    const out: FireEvalConfig = {};
    if (Array.isArray(obj.whenKeywords)) out.whenKeywords = obj.whenKeywords.filter((s) => typeof s === "string") as string[];
    if (typeof obj.whenDescription === "string") out.whenDescription = obj.whenDescription;
    if (typeof obj.classifierModel === "string") out.classifierModel = obj.classifierModel;
    if (typeof obj.classifierProvider === "string") out.classifierProvider = obj.classifierProvider;
    return out;
  } catch {
    return {};
  }
}

export async function listAgentHooks(agentId: string): Promise<AgentHook[]> {
  return invoke<AgentHook[]>("list_agent_hooks", { agentId });
}

export async function saveAgentHook(input: {
  id?: string;
  agentId: string;
  position?: number;
  name: string;
  kind: HookKind;
  configJson: string;
  enabled?: boolean;
  fireMode?: HookFireMode;
}): Promise<AgentHook> {
  return invoke<AgentHook>("save_agent_hook", {
    id: input.id ?? null,
    agentId: input.agentId,
    position: input.position ?? null,
    name: input.name,
    kind: input.kind,
    configJson: input.configJson,
    enabled: input.enabled ?? true,
    fireMode: input.fireMode ?? "always",
  });
}

export async function deleteAgentHook(id: string): Promise<void> {
  return invoke("delete_agent_hook", { id });
}
