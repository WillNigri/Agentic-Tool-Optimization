import { invoke } from "@tauri-apps/api/core";

// v1.3.0 — Frontend wrappers for the Rust agents commands (T3).

export type AgentRuntime = "claude" | "codex" | "gemini" | "openclaw" | "hermes";

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
  return invoke("update_agent_memory_policy", {
    id,
    policyJson: policy ? JSON.stringify(policy) : null,
  });
}

/** Replace the MCPs attached to an agent. Used by the one-click "Add
 *  browser tools" flow and any future MCP-attach UX. */
export async function updateAgentMcps(id: string, mcps: string[]): Promise<void> {
  return invoke("update_agent_mcps", { id, mcps });
}

export async function updateAgentRoleModels(id: string, models: RoleModels | null): Promise<void> {
  return invoke("update_agent_role_models", {
    id,
    roleModelsJson: models ? JSON.stringify(models) : null,
  });
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
}

export async function createAgent(input: CreateAgentInput): Promise<Agent> {
  return invoke<Agent>("create_agent", {
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
  });
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
