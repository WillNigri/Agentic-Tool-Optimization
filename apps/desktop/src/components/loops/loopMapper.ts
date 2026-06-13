import type { FlowEdge, FlowNode, Workflow } from "./types";
import type { Loop, LoopCreateInput, LoopUpdateInput } from "@/lib/loops-api";

interface CanvasGraph {
  nodes: FlowNode[];
  edges: FlowEdge[];
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isFlowNode(value: unknown): value is FlowNode {
  return isObject(value)
    && typeof value.id === "string"
    && typeof value.label === "string"
    && typeof value.description === "string"
    && typeof value.type === "string"
    && typeof value.x === "number"
    && typeof value.y === "number"
    && isObject(value.stats)
    && typeof value.stats.executions === "number"
    && typeof value.stats.errors === "number"
    && typeof value.stats.avgTimeMs === "number"
    && (value.status === "active" || value.status === "idle" || value.status === "error");
}

function isFlowEdge(value: unknown): value is FlowEdge {
  return isObject(value)
    && typeof value.from === "string"
    && typeof value.to === "string"
    && (value.label === undefined || typeof value.label === "string")
    && (value.animated === undefined || typeof value.animated === "boolean");
}

function emptyCanvas(): CanvasGraph {
  return { nodes: [], edges: [] };
}

function parseGraph(graph: unknown): CanvasGraph {
  try {
    const parsed = JSON.parse(JSON.stringify(graph)) as unknown;
    if (!isObject(parsed) || !Array.isArray(parsed.nodes) || !Array.isArray(parsed.edges)) {
      return emptyCanvas();
    }
    const nodes = parsed.nodes.filter(isFlowNode);
    const edges = parsed.edges.filter(isFlowEdge);
    return { nodes, edges };
  } catch {
    return emptyCanvas();
  }
}

/**
 * Map a persisted Loop row's `source` (free-form text in the DB) to one
 * of the Workflow.source enum values the UI distinguishes for filtering
 * and source-aware behavior. Codex R4 caught the regression: the
 * previous shape collapsed every row to "manual" on read, so
 * skill/cron/hook origin was silently lost in the client.
 */
function loopSourceToWorkflowSource(raw?: string | null): Workflow["source"] {
  switch (raw) {
    case "skill":
    case "cron":
    case "hook":
    case "agent_group":
    case "group":
    case "external":
    case "manual":
      return raw as Workflow["source"];
    default:
      return "manual";
  }
}

export function loopToWorkflow(loop: Loop): Workflow {
  const graph = parseGraph(loop.graph);
  return {
    id: loop.id,
    name: loop.name,
    description: loop.description ?? "",
    enabled: loop.enabled,
    runCount: 0,
    errorCount: 0,
    nodes: graph.nodes,
    edges: graph.edges,
    // Codex R4: preserve provenance from the DB row instead of
    // collapsing every loop to "manual".
    source: loopSourceToWorkflowSource(loop.source),
  };
}

export function workflowToLoopCreateInput(workflow: Workflow): LoopCreateInput {
  return {
    name: workflow.name,
    description: workflow.description || null,
    graph: JSON.parse(JSON.stringify({ nodes: workflow.nodes, edges: workflow.edges })),
    // Codex R4: honor the workflow's actual enabled state instead of
    // letting the Rust default (enabled=1) win for newly-created
    // disabled workflows. Same for source.
    enabled: workflow.enabled,
    source: workflow.source ?? "manual",
    triggerKind: "manual",
  };
}

export function workflowToLoopUpdateInput(workflow: Workflow): LoopUpdateInput {
  return {
    name: workflow.name,
    description: workflow.description || null,
    enabled: workflow.enabled,
    graph: JSON.parse(JSON.stringify({ nodes: workflow.nodes, edges: workflow.edges })),
    triggerKind: "manual",
  };
}
