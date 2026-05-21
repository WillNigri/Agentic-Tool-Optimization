import { invoke } from "@tauri-apps/api/core";

import type { RuntimeId as AgentRuntime } from "./runtimes";

// v1.3.0 — Frontend wrappers for the Rust agents commands (T3).
//
// 2026-05-18 — AgentRuntime used to be hand-typed as just the 5 CLI
// runtimes. After v2.3.21 added API-key dispatch (MiniMax/Grok/DeepSeek/
// Qwen/OpenRouter) the wider union flowed through every callsite and
// the narrow type here silently rejected valid values. Now re-exported
// from the single source of truth at @/lib/runtimes so adding a runtime
// updates this type for free.
export type { AgentRuntime };

/**
 * v2.0.0 — runs locally on the developer's laptop ("internal") or is designed
 * for customer-facing deployment via embed widget / Cloudflare Worker / Docker
 * etc. ("external"). External agents auto-lock to a read-only permission set
 * and unlock the Deploy + Knowledge tabs.
 */
export type AgentKind = "internal" | "external";

export interface Agent {
  id: string;
  slug: string;
  displayName: string;
  description: string | null;
  runtime: AgentRuntime;
  model: string | null;
  projectId: string | null;
  systemPrompt: string | null;
  permissions: string | null; // JSON-encoded array
  skills: string | null;      // JSON-encoded array
  mcps: string | null;        // JSON-encoded array
  goal: string | null;
  filePath: string | null;
  createdAt: string;
  lastUsedAt: string | null;
  // v1.4.0 — JSON-encoded; parse with parseRoleModels / parseMemoryPolicy.
  roleModels?: string | null;
  memoryPolicy?: string | null;
  // v2.0.0 — defaults to 'internal' for older rows.
  kind?: AgentKind | null;
  // v2.7.9 Felipe P5 — optional default dispatch prompt. Not returned
  // by list_agents/get_agent (those are owned by S9's dispatch lock);
  // load it on demand via getAgentDefaultPrompt(id).
  defaultPrompt?: string | null;
}

// v1.4.0 F5 — per-task model selection.
export interface RoleModels {
  /** Model to use for routing decisions (cheap/fast). Optional — falls back to response. */
  router?: string;
  /** Model to use for conversation summarization. Optional. */
  summarizer?: string;
  /** Model that produces the user-visible response. Required for any agent. */
  response?: string;
  /** Model used for LLM-as-judge evaluators. Optional. */
  evaluator?: string;
}

export function parseRoleModels(agent: Agent): RoleModels {
  if (!agent.roleModels) return {};
  try {
    const obj = JSON.parse(agent.roleModels) as RoleModels;
    return obj && typeof obj === "object" ? obj : {};
  } catch {
    return {};
  }
}

// v1.4.0 F3 — conversation memory / summarizer policy.
export interface MemoryPolicy {
  /** Trigger summarization when message count exceeds this. */
  summarizeAfter: number;
  /** Number of recent messages to keep verbatim after summarization. */
  keepLastK: number;
  /** Model to use for summarization. Empty string → runtime default. */
  summarizerModel: string;
}

export const DEFAULT_MEMORY_POLICY: MemoryPolicy = {
  summarizeAfter: 30,
  keepLastK: 5,
  summarizerModel: "",
};

export function parseMemoryPolicy(agent: Agent): MemoryPolicy {
  if (!agent.memoryPolicy) return DEFAULT_MEMORY_POLICY;
  try {
    const obj = JSON.parse(agent.memoryPolicy) as Partial<MemoryPolicy>;
    return {
      summarizeAfter:
        typeof obj.summarizeAfter === "number" ? obj.summarizeAfter : DEFAULT_MEMORY_POLICY.summarizeAfter,
      keepLastK:
        typeof obj.keepLastK === "number" ? obj.keepLastK : DEFAULT_MEMORY_POLICY.keepLastK,
      summarizerModel:
        typeof obj.summarizerModel === "string" ? obj.summarizerModel : DEFAULT_MEMORY_POLICY.summarizerModel,
    };
  } catch {
    return DEFAULT_MEMORY_POLICY;
  }
}

export async function updateAgentMemoryPolicy(id: string, policy: MemoryPolicy | null): Promise<void> {
  await invoke("update_agent_memory_policy", {
    id,
    policyJson: policy ? JSON.stringify(policy) : null,
  });
  void recordChangeFor(id, "memory_policy", policy);
}

/** Replace the MCPs attached to an agent. Used by the one-click "Add
 *  browser tools" flow and any future MCP-attach UX. */
export async function updateAgentMcps(id: string, mcps: string[]): Promise<void> {
  await invoke("update_agent_mcps", { id, mcps });
  void recordChangeFor(id, "mcps", mcps);
}

export async function updateAgentRoleModels(id: string, models: RoleModels | null): Promise<void> {
  await invoke("update_agent_role_models", {
    id,
    roleModelsJson: models ? JSON.stringify(models) : null,
  });
  void recordChangeFor(id, "role_models", models);
}

export interface CreateAgentInput {
  displayName: string;
  runtime: AgentRuntime;
  description?: string;
  model?: string;
  projectId?: string;
  systemPrompt?: string;
  permissions?: string[];
  skills?: string[];
  mcps?: string[];
  goal?: string;
  /** Default true — actually writes the agent file to disk in the runtime's directory. */
  writeFile?: boolean;
  /** v2.0.0 — defaults to 'internal' if omitted. 'external' auto-locks permissions. */
  kind?: AgentKind;
  /** v2.7.9 Felipe P5 — optional default dispatch prompt; fires automatically when
   *  the agent is dispatched without one. Whitespace-only collapses to null. */
  defaultPrompt?: string;
}

export async function createAgent(input: CreateAgentInput): Promise<Agent> {
  const agent = await invoke<Agent>("create_agent", {
    displayName: input.displayName,
    runtime: input.runtime,
    description: input.description ?? null,
    model: input.model ?? null,
    projectId: input.projectId ?? null,
    systemPrompt: input.systemPrompt ?? null,
    permissions: input.permissions ?? null,
    skills: input.skills ?? null,
    mcps: input.mcps ?? null,
    goal: input.goal ?? null,
    writeFile: input.writeFile ?? true,
    kind: input.kind ?? "internal",
    defaultPrompt: input.defaultPrompt ?? null,
  });
  // v2.1.0 — record the genesis event so the dashboard has a "v0"
  // marker per agent. Subsequent edits chain off this. Best-effort.
  try {
    const { recordConfigChange } = await import("./cloudConfigChanges");
    void recordConfigChange({
      agentSlug: agent.slug,
      field: "created",
      newValue: {
        runtime: agent.runtime,
        model: agent.model ?? null,
        kind: agent.kind,
      },
      metadata: { genesis: true },
    });
  } catch {
    // ignore
  }
  return agent;
}

export async function updateAgentKind(id: string, kind: AgentKind): Promise<void> {
  await invoke("update_agent_kind", { id, kind });
  void recordChangeFor(id, "kind", kind);
}

/** v2.7.9 Felipe P5 — persist the optional default dispatch prompt.
 *  Pass `null` (or whitespace) to clear it. The backend normalizes
 *  whitespace-only inputs to NULL so S9's "use default when blank"
 *  branch can rely on a single IS NOT NULL check.
 *
 *  Cloud config-change telemetry is intentionally skipped — `default_prompt`
 *  isn't in the `ConfigChangeField` union yet (cloudConfigChanges.ts is
 *  out of scope for this session). A follow-up can wire it once the
 *  cloud schema accepts the new field. */
export async function updateAgentDefaultPrompt(
  id: string,
  value: string | null,
): Promise<void> {
  await invoke("update_agent_default_prompt", { id, value });
}

/** v2.7.9 Felipe P5 — read the current default dispatch prompt.
 *  Surfaced via a dedicated getter because list_agents/get_agent are
 *  owned by S9's dispatch lock. Returns null when unset. */
export async function getAgentDefaultPrompt(id: string): Promise<string | null> {
  return invoke<string | null>("get_agent_default_prompt", { id });
}

// v2.1.0 — Configuration impact ledger. Each update fn fires this
// post-success so the cloud has a timestamped audit of every meaningful
// change. Best-effort — silently no-ops when not signed in or offline.
async function recordChangeFor(
  id: string,
  field: import("./cloudConfigChanges").ConfigChangeField,
  newValue: unknown,
): Promise<void> {
  try {
    const agent = await getAgent(id);
    const { recordConfigChange } = await import("./cloudConfigChanges");
    await recordConfigChange({
      agentSlug: agent.slug,
      field,
      newValue,
    });
  } catch {
    // Telemetry must never break the local edit.
  }
}

export async function listAgents(filter?: {
  runtime?: AgentRuntime;
  projectId?: string;
}): Promise<Agent[]> {
  return invoke<Agent[]>("list_agents", {
    runtime: filter?.runtime ?? null,
    projectId: filter?.projectId ?? null,
  });
}

/** S11 (v2.7.11) — number of agents created before v2.7.8 whose
 *  permission DSL is recorded but not enforced. Powers MigrationToast.
 *  Re-saving an agent via the existing flow stamps
 *  `permissions_migrated_at = now` and the count drops. */
export async function countUnmigratedAgents(): Promise<number> {
  return invoke<number>("count_unmigrated_agents");
}

export async function getAgent(id: string): Promise<Agent> {
  return invoke<Agent>("get_agent", { id });
}

export async function deleteAgent(id: string, deleteFile = true): Promise<void> {
  return invoke("delete_agent", { id, deleteFile });
}

export async function touchAgentLastUsed(id: string): Promise<void> {
  return invoke("touch_agent_last_used", { id });
}

// Helpers for parsing JSON-encoded array fields.
export function parsePermissions(agent: Agent): string[] {
  return parseJsonArray(agent.permissions);
}

export function parseSkills(agent: Agent): string[] {
  return parseJsonArray(agent.skills);
}

export function parseMcps(agent: Agent): string[] {
  return parseJsonArray(agent.mcps);
}

function parseJsonArray(s: string | null): string[] {
  if (!s) return [];
  try {
    const v = JSON.parse(s);
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}
