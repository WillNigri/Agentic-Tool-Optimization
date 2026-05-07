import { invoke } from "@tauri-apps/api/core";
import type { AgentRuntime } from "@/lib/agents";

// v1.4.0 F4 — Multi-agent groups (router + N specialized children).
// Groups are MIT-licensed code in the OSS repo; the only server-side gate is
// "Free tier capped at 3 children" which the UI enforces (see lib/tier.ts).

export interface AgentGroupMember {
  /** Optional — older serialized state may lack it. New rows include the
   *  child's runtime so cross-runtime sequential pipelines render correctly. */
  agentRuntime?: string;
  agentId: string;
  agentSlug: string;
  agentDisplayName: string;
  role: "router" | "child";
  position: number;
}

export interface AgentGroup {
  id: string;
  slug: string;
  displayName: string;
  description: string | null;
  runtime: AgentRuntime;
  /** JSON-encoded — parse with parseRouterConfig. */
  routerConfig: string | null;
  filePath: string | null;
  createdAt: string;
  lastUsedAt: string | null;
  members: AgentGroupMember[];
  /** "routed" (router picks one child) or "sequential" (children run in
   *  order, output of N is input to N+1). Older rows default to "routed". */
  dispatchKind: "routed" | "sequential";
}

export interface RouterRule {
  if?: {
    keyword?: string[];
    regex?: string;
  };
  then?: string;
}

export interface RouterConfig {
  rules: RouterRule[];
  llmFallback: {
    enabled: boolean;
    /** Optional model override — defaults to the runtime's cheap classifier. */
    model?: string;
  };
}

export const DEFAULT_ROUTER_CONFIG: RouterConfig = {
  rules: [],
  llmFallback: { enabled: true },
};

export function parseRouterConfig(raw: string | null | undefined): RouterConfig {
  if (!raw) return DEFAULT_ROUTER_CONFIG;
  try {
    const obj = JSON.parse(raw) as Partial<RouterConfig>;
    return {
      rules: Array.isArray(obj.rules) ? obj.rules : [],
      llmFallback: {
        enabled: obj.llmFallback?.enabled ?? true,
        model: obj.llmFallback?.model,
      },
    };
  } catch {
    return DEFAULT_ROUTER_CONFIG;
  }
}

export interface GroupMemberInput {
  agentSlug: string;
  role: "router" | "child";
  position: number;
}

export async function listAgentGroups(runtime?: AgentRuntime): Promise<AgentGroup[]> {
  return invoke<AgentGroup[]>("list_agent_groups", { runtime: runtime ?? null });
}

export async function getAgentGroup(slug: string): Promise<AgentGroup> {
  return invoke<AgentGroup>("get_agent_group", { slug });
}

export type DispatchKind = "routed" | "sequential";

export async function createAgentGroup(input: {
  displayName: string;
  runtime: AgentRuntime;
  description?: string;
  routerConfig?: RouterConfig;
  members: GroupMemberInput[];
  /** "routed" (router picks one child) | "sequential" (children run in
   *  order, output of N is input to N+1). Defaults to "routed". */
  dispatchKind?: DispatchKind;
}): Promise<AgentGroup> {
  return invoke<AgentGroup>("create_agent_group", {
    displayName: input.displayName,
    runtime: input.runtime,
    description: input.description ?? null,
    routerConfigJson: input.routerConfig
      ? JSON.stringify(input.routerConfig)
      : JSON.stringify(DEFAULT_ROUTER_CONFIG),
    members: input.members,
    dispatchKind: input.dispatchKind ?? null,
  });
}

export async function updateAgentGroup(input: {
  id: string;
  description?: string;
  routerConfig?: RouterConfig;
  members?: GroupMemberInput[];
}): Promise<AgentGroup> {
  return invoke<AgentGroup>("update_agent_group", {
    id: input.id,
    description: input.description ?? null,
    routerConfigJson: input.routerConfig ? JSON.stringify(input.routerConfig) : null,
    members: input.members ?? null,
  });
}

export async function deleteAgentGroup(id: string): Promise<void> {
  return invoke("delete_agent_group", { id });
}

export interface GroupStageResult {
  agentSlug: string;
  runtime: string;
  response: string;
  ok: boolean;
}

export interface GroupDispatchResult {
  /** Stitched transcript (or single response for routed groups). */
  response: string;
  routedTo: string;
  routingReason: string;
  /** One per stage. Routed groups have one entry; sequential groups have
   *  one per child in pipeline order. Frontend can render each stage as
   *  its own chat message (preferred for sequential groups). */
  stages?: GroupStageResult[];
}

export async function dispatchToGroup(input: {
  slug: string;
  prompt: string;
  config?: string;
  activeProjectPath?: string;
}): Promise<GroupDispatchResult> {
  return invoke<GroupDispatchResult>("dispatch_to_group", {
    slug: input.slug,
    prompt: input.prompt,
    config: input.config ?? null,
    activeProjectPath: input.activeProjectPath ?? null,
  });
}
