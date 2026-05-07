// v1.6.0 — Automations multi-source aggregator.
//
// The Automations tab today only renders skill-derived flows (parsed from
// `## Step N` headers in SKILL.md). v1.6 turns that tab into the canonical
// visualization for **everything that runs without a human in the loop**:
// agent groups (routed + sequential), scheduled cron jobs, pre-call hooks,
// and skill flows. This module is the layer that pulls each source and
// normalizes it into the existing Workflow shape so the canvas can render.
//
// Why a separate aggregator: skill parsing already lives in
// `lib/skill-to-workflow.ts`. Agent groups, crons, hooks each have their
// own data shape. Rather than scatter conversions across the canvas
// component, every source flows through here and emits the same
// `Workflow[]` the canvas already understands.

import type { Workflow, FlowNode, FlowEdge } from "@/components/automation/types";
import type { AgentGroup, AgentGroupMember, RouterConfig } from "@/lib/agentGroups";
import { parseRouterConfig } from "@/lib/agentGroups";
import type { CronJob } from "@/components/cron/types";
import type { AgentHook } from "@/lib/agentHooks";
import type { Agent } from "@/lib/agents";
import { cronToHuman } from "@/lib/cron-utils";

// Layout constants — kept loose so node widths don't conflict with the
// canvas's drag/zoom math. NODE_W matches the canvas's default node size.
const NODE_W = 220;
const NODE_GAP_X = 280;
const NODE_GAP_Y = 140;
const ROUTER_FANOUT_RADIUS = 220;

/**
 * Convert an `AgentGroup` (sequential or routed) to a Workflow the
 * Automations canvas can render. Each child agent becomes a node; edges
 * encode either the pipeline order (sequential) or the routing rule
 * (routed). The router itself is rendered as a synthetic node at the
 * center of routed groups.
 */
export function groupToWorkflow(group: AgentGroup): Workflow {
  const children = group.members
    .filter((m) => m.role === "child")
    .sort((a, b) => a.position - b.position);

  if (group.dispatchKind === "sequential") {
    return sequentialGroupToWorkflow(group, children);
  }
  return routedGroupToWorkflow(group, children);
}

function sequentialGroupToWorkflow(
  group: AgentGroup,
  children: AgentGroupMember[]
): Workflow {
  // Left-to-right pipeline. Each child runs on its own runtime — that's
  // the v1.5 differentiator, so we render the runtime under each node.
  const nodes: FlowNode[] = children.map((child, i) => ({
    id: `${group.id}:child:${child.agentSlug}`,
    label: child.agentDisplayName || `@${child.agentSlug}`,
    description: `Stage ${i + 1} of ${children.length}`,
    type: "action",
    runtime: child.agentRuntime as FlowNode["runtime"],
    agentId: child.agentId,
    agentName: child.agentSlug,
    width: NODE_W,
    x: 80 + i * NODE_GAP_X,
    y: 120,
    stats: { executions: 0, errors: 0, avgTimeMs: 0 },
    status: "idle",
  }));

  const edges: FlowEdge[] = [];
  for (let i = 0; i < nodes.length - 1; i++) {
    edges.push({
      from: nodes[i].id,
      to: nodes[i + 1].id,
      animated: true,
      label: i === 0 ? "→" : undefined,
    });
  }

  return {
    id: `group:${group.id}`,
    name: `${group.slug} (sequential)`,
    description:
      group.description ?? `Sequential pipeline: ${children.length} stages, output of each feeds the next.`,
    enabled: true,
    runCount: 0,
    errorCount: 0,
    nodes,
    edges,
    source: "group-sequential",
  };
}

function routedGroupToWorkflow(
  group: AgentGroup,
  children: AgentGroupMember[]
): Workflow {
  // Router in the center, children fan out radially. Edges carry the
  // matching rule keywords as the label so the routing logic is visible
  // at a glance.
  const routerNode: FlowNode = {
    id: `${group.id}:router`,
    label: `${group.slug} router`,
    description: "Picks one child per prompt",
    type: "decision",
    width: NODE_W,
    x: 80 + ROUTER_FANOUT_RADIUS,
    y: 120,
    stats: { executions: 0, errors: 0, avgTimeMs: 0 },
    status: "idle",
  };

  const childNodes: FlowNode[] = children.map((child, i) => {
    const isOdd = children.length % 2 === 1;
    const middle = (children.length - 1) / 2;
    // Stagger children vertically; spread horizontally to the right of
    // the router.
    const offsetY = isOdd
      ? (i - middle) * NODE_GAP_Y
      : (i - middle) * NODE_GAP_Y;
    return {
      id: `${group.id}:child:${child.agentSlug}`,
      label: child.agentDisplayName || `@${child.agentSlug}`,
      description: `Specialist child`,
      type: "action",
      runtime: child.agentRuntime as FlowNode["runtime"],
      agentId: child.agentId,
      agentName: child.agentSlug,
      width: NODE_W,
      x: routerNode.x + ROUTER_FANOUT_RADIUS + NODE_GAP_X * 0.4,
      y: 120 + offsetY,
      stats: { executions: 0, errors: 0, avgTimeMs: 0 },
      status: "idle",
    };
  });

  // Build edges: each rule's keywords surface on the edge to its target
  // child. Children with no matching rule get an unlabeled "fallback"
  // edge — that's how the LLM-classifier fallback also routes.
  const router = parseRouterConfig(group.routerConfig ?? null);
  const edges: FlowEdge[] = [];
  const ruleByChild = ruleKeywordsByTargetSlug(router);
  for (const child of childNodes) {
    const slug = child.agentName ?? "";
    const keywords = ruleByChild[slug];
    edges.push({
      from: routerNode.id,
      to: child.id,
      label: keywords && keywords.length > 0 ? keywords.slice(0, 3).join(", ") : undefined,
      animated: false,
    });
  }

  return {
    id: `group:${group.id}`,
    name: `${group.slug} (routed)`,
    description: group.description ?? `Routed group: router picks 1 of ${children.length}`,
    enabled: true,
    runCount: 0,
    errorCount: 0,
    nodes: [routerNode, ...childNodes],
    edges,
    source: "group-routed",
  };
}

function ruleKeywordsByTargetSlug(router: RouterConfig): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const rule of router.rules ?? []) {
    if (!rule.then || !rule.if) continue;
    const keywords = rule.if.keyword ?? [];
    if (!out[rule.then]) out[rule.then] = [];
    out[rule.then].push(...keywords);
  }
  return out;
}

/**
 * Returns one Workflow per AgentGroup, ready to be merged with the
 * existing skill-derived workflows on the canvas. Wave 2 of v1.6.0
 * extends this to also include cron jobs + hooks.
 */
export function groupsToWorkflows(groups: AgentGroup[]): Workflow[] {
  return groups.map(groupToWorkflow);
}

// ── Wave 2: Cron jobs + hooks ────────────────────────────────────────────

/**
 * Convert a CronJob into a Workflow rooted at a clock-trigger node that
 * fires on schedule and dispatches to its target (agent / group / raw
 * runtime). The target shows up as the second node so the dispatch path
 * is visible at a glance.
 */
export function cronToWorkflow(
  cron: CronJob,
  agents: Agent[] = [],
  groups: AgentGroup[] = []
): Workflow {
  const triggerNode: FlowNode = {
    id: `cron:${cron.id}:trigger`,
    label: cronToHuman(cron.schedule),
    description: cron.name,
    type: "trigger",
    width: NODE_W,
    x: 80,
    y: 120,
    stats: { executions: 0, errors: 0, avgTimeMs: 0 },
    status: cron.enabled ? "idle" : "error",
  };

  // Resolve the dispatch target. Prefer agent/group references; fall
  // back to a "runtime" placeholder when the cron uses raw runtime+prompt.
  let targetNode: FlowNode | null = null;
  if (cron.agentSlug) {
    const agent = agents.find((a) => a.slug === cron.agentSlug);
    targetNode = {
      id: `cron:${cron.id}:agent`,
      label: agent?.displayName ?? `@${cron.agentSlug}`,
      description: agent ? `Agent on ${agent.runtime}` : "Referenced agent (not found locally)",
      type: "action",
      runtime: (agent?.runtime ?? cron.runtime) as FlowNode["runtime"],
      agentId: agent?.id,
      agentName: cron.agentSlug,
      width: NODE_W,
      x: 80 + NODE_GAP_X,
      y: 120,
      stats: { executions: 0, errors: 0, avgTimeMs: 0 },
      status: "idle",
    };
  } else if (cron.groupSlug) {
    const group = groups.find((g) => g.slug === cron.groupSlug);
    targetNode = {
      id: `cron:${cron.id}:group`,
      label: group?.displayName ?? cron.groupSlug,
      description: group ? `${group.dispatchKind} group` : "Referenced group (not found locally)",
      type: "process",
      width: NODE_W,
      x: 80 + NODE_GAP_X,
      y: 120,
      stats: { executions: 0, errors: 0, avgTimeMs: 0 },
      status: "idle",
    };
  } else {
    // Raw runtime + prompt — show the runtime as the dispatch target.
    targetNode = {
      id: `cron:${cron.id}:runtime`,
      label: `runtime: ${cron.runtime}`,
      description: "Raw prompt dispatch",
      type: "action",
      runtime: cron.runtime as FlowNode["runtime"],
      width: NODE_W,
      x: 80 + NODE_GAP_X,
      y: 120,
      stats: { executions: 0, errors: 0, avgTimeMs: 0 },
      status: "idle",
    };
  }

  const edges: FlowEdge[] = [
    {
      from: triggerNode.id,
      to: targetNode.id,
      animated: cron.enabled,
      label: cron.wakeFromSleep ? "wakes from sleep" : undefined,
    },
  ];

  return {
    id: `cron:${cron.id}`,
    name: `${cron.name} (cron)`,
    description: cron.description || cronToHuman(cron.schedule),
    enabled: cron.enabled,
    runCount: 0,
    errorCount: 0,
    nodes: [triggerNode, targetNode],
    edges,
    source: "cron",
  };
}

/**
 * Convert an agent's pre-call hooks into a Workflow showing hook nodes
 * feeding into the agent. Position hooks vertically on the input side
 * so the "data flowing into the agent" mental model is obvious.
 */
export function hooksToWorkflow(
  agent: Agent,
  hooks: AgentHook[]
): Workflow | null {
  if (hooks.length === 0) return null;
  const sorted = [...hooks].sort((a, b) => a.position - b.position);

  const agentNode: FlowNode = {
    id: `hooks:${agent.id}:agent`,
    label: agent.displayName,
    description: `Agent on ${agent.runtime}`,
    type: "action",
    runtime: agent.runtime as FlowNode["runtime"],
    agentId: agent.id,
    agentName: agent.slug,
    width: NODE_W,
    x: 80 + NODE_GAP_X,
    y: 80 + Math.max(0, (sorted.length - 1) * NODE_GAP_Y) / 2,
    stats: { executions: 0, errors: 0, avgTimeMs: 0 },
    status: "idle",
  };

  const hookNodes: FlowNode[] = sorted.map((hook, i) => ({
    id: `hooks:${agent.id}:hook:${hook.id}`,
    label: hook.name,
    description: hookKindLabel(hook.kind),
    type: "service",
    width: NODE_W,
    x: 80,
    y: 80 + i * NODE_GAP_Y,
    stats: { executions: 0, errors: 0, avgTimeMs: 0 },
    status: hook.enabled ? "idle" : "error",
  }));

  const edges: FlowEdge[] = hookNodes.map((node) => ({
    from: node.id,
    to: agentNode.id,
    animated: false,
    label: "<context>",
  }));

  return {
    id: `hooks:${agent.id}`,
    name: `${agent.slug} hooks`,
    description: `${sorted.length} pre-call ${sorted.length === 1 ? "hook" : "hooks"} feed context into ${agent.slug} on every dispatch.`,
    enabled: true,
    runCount: 0,
    errorCount: 0,
    nodes: [...hookNodes, agentNode],
    edges,
    source: "hook",
  };
}

function hookKindLabel(kind: AgentHook["kind"]): string {
  switch (kind) {
    case "file": return "Reads a file";
    case "webhook": return "GET webhook";
    case "mcp-call": return "Calls an MCP tool";
    case "db-query": return "Runs a SQL query";
    case "computed": return "Computed JS expression";
  }
}

/**
 * Convert all cron jobs to Workflows. Pass agents + groups so the
 * dispatch target can resolve to the right node.
 */
export function cronsToWorkflows(
  crons: CronJob[],
  agents: Agent[] = [],
  groups: AgentGroup[] = []
): Workflow[] {
  return crons.map((c) => cronToWorkflow(c, agents, groups));
}

/**
 * Build hook-flow workflows for every agent that has at least one
 * pre-call hook configured.
 */
export function hooksToWorkflows(
  agents: Agent[],
  hooksByAgentId: Map<string, AgentHook[]>
): Workflow[] {
  const out: Workflow[] = [];
  for (const agent of agents) {
    const hooks = hooksByAgentId.get(agent.id) ?? [];
    const wf = hooksToWorkflow(agent, hooks);
    if (wf) out.push(wf);
  }
  return out;
}
