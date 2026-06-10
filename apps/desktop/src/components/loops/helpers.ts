import { NODE_W, NODE_H } from "./constants";
import type { FlowNode, FlowEdge, Workflow } from "./types";

// ---------------------------------------------------------------------------
// Bezier math
// ---------------------------------------------------------------------------

export function getConnectionPoints(
  fromNode: FlowNode,
  toNode: FlowNode
): { x1: number; y1: number; x2: number; y2: number } {
  const fromW = fromNode.width || NODE_W;
  const toW = toNode.width || NODE_W;
  const fromCenterX = fromNode.x + fromW / 2;
  const fromCenterY = fromNode.y + NODE_H / 2;
  const toCenterX = toNode.x + toW / 2;
  const toCenterY = toNode.y + NODE_H / 2;

  const dx = toCenterX - fromCenterX;
  const dy = toCenterY - fromCenterY;

  let x1: number, y1: number, x2: number, y2: number;

  if (Math.abs(dx) > Math.abs(dy)) {
    if (dx > 0) {
      x1 = fromNode.x + fromW; y1 = fromCenterY;
      x2 = toNode.x; y2 = toCenterY;
    } else {
      x1 = fromNode.x; y1 = fromCenterY;
      x2 = toNode.x + toW; y2 = toCenterY;
    }
  } else {
    if (dy > 0) {
      x1 = fromCenterX; y1 = fromNode.y + NODE_H;
      x2 = toCenterX; y2 = toNode.y;
    } else {
      x1 = fromCenterX; y1 = fromNode.y;
      x2 = toCenterX; y2 = toNode.y + NODE_H;
    }
  }

  return { x1, y1, x2, y2 };
}

export function buildBezierPath(x1: number, y1: number, x2: number, y2: number): string {
  const dx = x2 - x1;
  const dy = y2 - y1;

  if (Math.abs(dx) >= Math.abs(dy)) {
    const cpOffset = Math.abs(dx) * 0.4;
    return `M ${x1} ${y1} C ${x1 + cpOffset} ${y1}, ${x2 - cpOffset} ${y2}, ${x2} ${y2}`;
  } else {
    const cpOffset = Math.abs(dy) * 0.4;
    return `M ${x1} ${y1} C ${x1} ${y1 + Math.sign(dy) * cpOffset}, ${x2} ${y2 - Math.sign(dy) * cpOffset}, ${x2} ${y2}`;
  }
}

// ---------------------------------------------------------------------------
// Connection ports (for edge creation in edit mode)
// ---------------------------------------------------------------------------

export function getOutputPortPos(node: FlowNode): { x: number; y: number } {
  return { x: node.x + NODE_W, y: node.y + NODE_H / 2 };
}

export function getInputPortPos(node: FlowNode): { x: number; y: number } {
  return { x: node.x, y: node.y + NODE_H / 2 };
}

// ---------------------------------------------------------------------------
// Cycle detection — prevent cycles and self-connections
// ---------------------------------------------------------------------------

export function wouldCreateCycle(
  edges: FlowEdge[],
  from: string,
  to: string
): boolean {
  if (from === to) return true;

  // BFS from `to` following edges — if we can reach `from`, it's a cycle
  const adjacency = new Map<string, string[]>();
  for (const e of edges) {
    if (!adjacency.has(e.from)) adjacency.set(e.from, []);
    adjacency.get(e.from)!.push(e.to);
  }
  // Add the proposed edge
  if (!adjacency.has(from)) adjacency.set(from, []);
  adjacency.get(from)!.push(to);

  const visited = new Set<string>();
  const queue = [to];
  while (queue.length > 0) {
    const current = queue.shift()!;
    if (current === from) return true;
    if (visited.has(current)) continue;
    visited.add(current);
    for (const neighbor of adjacency.get(current) || []) {
      queue.push(neighbor);
    }
  }
  return false;
}

// ---------------------------------------------------------------------------
// Topological sort for prompt serialization
// ---------------------------------------------------------------------------

function topologicalSort(nodes: FlowNode[], edges: FlowEdge[]): FlowNode[] {
  const inDegree = new Map<string, number>();
  const adjacency = new Map<string, string[]>();

  for (const node of nodes) {
    inDegree.set(node.id, 0);
    adjacency.set(node.id, []);
  }
  for (const edge of edges) {
    adjacency.get(edge.from)?.push(edge.to);
    inDegree.set(edge.to, (inDegree.get(edge.to) || 0) + 1);
  }

  const queue: string[] = [];
  for (const [id, deg] of inDegree) {
    if (deg === 0) queue.push(id);
  }

  const sorted: string[] = [];
  while (queue.length > 0) {
    const current = queue.shift()!;
    sorted.push(current);
    for (const neighbor of adjacency.get(current) || []) {
      const newDeg = (inDegree.get(neighbor) || 1) - 1;
      inDegree.set(neighbor, newDeg);
      if (newDeg === 0) queue.push(neighbor);
    }
  }

  const nodeMap = new Map(nodes.map((n) => [n.id, n]));
  return sorted.map((id) => nodeMap.get(id)!).filter(Boolean);
}

// ---------------------------------------------------------------------------
// Prompt serializer — converts workflow to structured prompt for Claude
// ---------------------------------------------------------------------------

export function serializeWorkflowToPrompt(workflow: Workflow): string {
  const sorted = topologicalSort(workflow.nodes, workflow.edges);

  const steps = sorted.map((node, i) => {
    const typeLabel = node.type.toUpperCase();
    const serviceLabel = node.service ? `${node.service}/` : "";
    const actionLabel = node.config?.params?.action || node.type;
    const params = node.config?.params || {};
    const paramStr = Object.entries(params)
      .filter(([k]) => k !== "action")
      .map(([k, v]) => `     ${k}: ${v}`)
      .join("\n");

    const runtimeLabel = node.runtime && node.runtime !== "claude" ? ` @${node.runtime}` : "";
    let line = `${i + 1}. [${typeLabel}: ${serviceLabel}${actionLabel}${runtimeLabel}] "${node.label}"`;
    if (node.description) line += ` — ${node.description}`;
    if (node.type === "decision" && node.config?.condition) {
      // Find outgoing edges
      const outEdges = workflow.edges.filter((e) => e.from === node.id);
      const branches = outEdges
        .map((e) => {
          const target = workflow.nodes.find((n) => n.id === e.to);
          return e.label ? `${e.label} -> step ${sorted.findIndex((s) => s.id === e.to) + 1}` : null;
        })
        .filter(Boolean)
        .join(", ");
      line += `\n     Condition: ${node.config.condition}`;
      if (branches) line += `\n     Branches: ${branches}`;
    }
    if (paramStr) line += `\n${paramStr}`;
    return line;
  });

  return `Execute this automation workflow step by step.

Workflow: "${workflow.name}"
${workflow.description ? `Description: ${workflow.description}` : ""}

Steps (execute in order):
${steps.join("\n")}

Rules:
- Use available MCP tools for each service step
- Report: [STEP N] STARTED/COMPLETED/FAILED: description
- Continue to next independent step on failure
- For decision nodes, evaluate the condition and follow the appropriate branch`;
}

// ---------------------------------------------------------------------------
// Screen to canvas coordinate conversion
// ---------------------------------------------------------------------------

export function screenToCanvas(
  clientX: number,
  clientY: number,
  canvasRect: DOMRect,
  panOffset: { x: number; y: number },
  scale: number
): { x: number; y: number } {
  return {
    x: (clientX - canvasRect.left - panOffset.x) / scale,
    y: (clientY - canvasRect.top - panOffset.y) / scale,
  };
}
