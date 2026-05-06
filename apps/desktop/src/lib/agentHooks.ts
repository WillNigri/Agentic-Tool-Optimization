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

export interface AgentHook {
  id: string;
  agentId: string;
  position: number;
  name: string;
  kind: HookKind;
  configJson: string;
  enabled: boolean;
  createdAt: string;
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

export function hookConfigToJson(cfg: HookConfig): string {
  const { kind: _kind, ...rest } = cfg as HookConfig & { kind: string };
  return JSON.stringify(rest);
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
}): Promise<AgentHook> {
  return invoke<AgentHook>("save_agent_hook", {
    id: input.id ?? null,
    agentId: input.agentId,
    position: input.position ?? null,
    name: input.name,
    kind: input.kind,
    configJson: input.configJson,
    enabled: input.enabled ?? true,
  });
}

export async function deleteAgentHook(id: string): Promise<void> {
  return invoke("delete_agent_hook", { id });
}
