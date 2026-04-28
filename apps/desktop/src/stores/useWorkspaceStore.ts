import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { ProjectBundle, LocalSkill } from "@/lib/api";

export type WorkspaceNodeKind = "runtime" | "skill" | "mcp" | "process" | "memory";
export type WorkspaceNodeStatus = "online" | "offline" | "busy" | "error" | "idle";
export type WorkspaceEdgeKind = "uses-skill" | "connects-mcp" | "delegates-to" | "triggers";

export interface WorkspaceNode {
  id: string;
  kind: WorkspaceNodeKind;
  label: string;
  description: string;
  status: WorkspaceNodeStatus;
  runtime?: string;
  x: number;
  y: number;
  width?: number;
  lastHeartbeat?: number;
  tokensTodayIn?: number;
  tokensTodayOut?: number;
  skillCount?: number;
  mcpCount?: number;
  filePath?: string;
  projectPath?: string;
  hidden?: boolean;
}

export interface WorkspaceEdge {
  from: string;
  to: string;
  kind: WorkspaceEdgeKind;
  animated?: boolean;
}

interface WorkspaceStore {
  nodes: WorkspaceNode[];
  edges: WorkspaceEdge[];
  scale: number;
  panOffset: { x: number; y: number };
  selectedNodeId: string | null;
  mode: "workspace" | "workflows";

  setMode: (mode: "workspace" | "workflows") => void;
  populateFromBundle: (bundle: ProjectBundle) => void;
  moveNode: (id: string, x: number, y: number) => void;
  selectNode: (id: string | null) => void;
  updateNodeStatus: (id: string, status: WorkspaceNodeStatus) => void;
  updateNodeTokens: (id: string, tokensIn: number, tokensOut: number) => void;
  setScale: (scale: number) => void;
  setPanOffset: (offset: { x: number; y: number }) => void;
  toggleNodeHidden: (id: string) => void;
  clear: () => void;
}

const RUNTIME_COLORS: Record<string, string> = {
  claude: "#f97316",
  codex: "#22c55e",
  gemini: "#3b82f6",
  openclaw: "#06b6d4",
  hermes: "#a855f7",
};

function autoLayout(bundle: ProjectBundle): { nodes: WorkspaceNode[]; edges: WorkspaceEdge[] } {
  const nodes: WorkspaceNode[] = [];
  const edges: WorkspaceEdge[] = [];
  let runtimeX = 80;
  const runtimeY = 60;
  const skillRowY = 280;
  const mcpRowY = 480;

  const runtimes: Array<{ key: string; label: string; has: boolean; skills: LocalSkill[]; files: { label: string; path: string; exists: boolean }[] }> = [
    { key: "claude", label: "Claude Code", has: bundle.hasClaude, skills: bundle.skills, files: bundle.memoryFiles as any },
    { key: "codex", label: "Codex", has: bundle.hasCodex, skills: bundle.codexSkills, files: bundle.codexFiles as any },
    { key: "gemini", label: "Gemini CLI", has: bundle.hasGemini, skills: bundle.geminiSkills, files: bundle.geminiFiles as any },
    { key: "openclaw", label: "OpenClaw", has: bundle.hasOpenclaw, skills: bundle.openclawSkills, files: bundle.openclawFiles as any },
    { key: "hermes", label: "Hermes", has: bundle.hasHermes, skills: bundle.hermesSkills, files: bundle.hermesFiles as any },
  ];

  for (const rt of runtimes) {
    if (!rt.has) continue;

    const runtimeId = `rt-${rt.key}`;
    nodes.push({
      id: runtimeId,
      kind: "runtime",
      label: rt.label,
      description: `${rt.skills.length} skills`,
      status: "idle",
      runtime: rt.key,
      x: runtimeX,
      y: runtimeY,
      width: 220,
      skillCount: rt.skills.length,
      mcpCount: 0,
      projectPath: bundle.projectPath,
    });

    let skillX = runtimeX - 30;
    for (const skill of rt.skills.slice(0, 6)) {
      const skillId = `sk-${skill.id}`;
      nodes.push({
        id: skillId,
        kind: "skill",
        label: skill.name,
        description: skill.description || `${skill.tokenCount} tokens`,
        status: skill.enabled ? "online" : "offline",
        runtime: rt.key,
        x: skillX,
        y: skillRowY + Math.random() * 40,
        filePath: skill.filePath,
      });
      edges.push({ from: runtimeId, to: skillId, kind: "uses-skill" });
      skillX += 170;
    }

    runtimeX += 320;
  }

  // MCP servers
  let mcpX = 80;
  for (const mcp of bundle.mcpServers) {
    const mcpId = `mcp-${mcp.name}`;
    nodes.push({
      id: mcpId,
      kind: "mcp",
      label: mcp.name,
      description: `${mcp.kind} — ${mcp.commandOrUrl}`.slice(0, 50),
      status: "online",
      x: mcpX,
      y: mcpRowY,
    });

    // Connect MCP to its scope's runtime
    const targetRuntime = mcp.scope === "project" ? "rt-claude" : "rt-claude";
    if (nodes.some((n) => n.id === targetRuntime)) {
      edges.push({ from: targetRuntime, to: mcpId, kind: "connects-mcp" });
    }
    mcpX += 220;
  }

  // Memory files as a single node
  const existingMemory = bundle.memoryFiles.filter((f) => f.exists);
  if (existingMemory.length > 0) {
    nodes.push({
      id: "memory-root",
      kind: "memory",
      label: "Memory",
      description: `${existingMemory.length} CLAUDE.md files`,
      status: "online",
      x: runtimeX + 40,
      y: runtimeY,
    });
  }

  return { nodes, edges };
}

export const useWorkspaceStore = create<WorkspaceStore>()(
  persist(
    (set, get) => ({
      nodes: [],
      edges: [],
      scale: 0.85,
      panOffset: { x: 20, y: 20 },
      selectedNodeId: null,
      mode: "workspace",

      setMode: (mode) => set({ mode }),

      populateFromBundle: (bundle) => {
        const existing = get().nodes;
        if (existing.length > 0) {
          // Update statuses on existing nodes, don't reset positions
          return;
        }
        const { nodes, edges } = autoLayout(bundle);
        set({ nodes, edges });
      },

      moveNode: (id, x, y) =>
        set((s) => ({
          nodes: s.nodes.map((n) => (n.id === id ? { ...n, x, y } : n)),
        })),

      selectNode: (id) => set({ selectedNodeId: id }),

      updateNodeStatus: (id, status) =>
        set((s) => ({
          nodes: s.nodes.map((n) =>
            n.id === id ? { ...n, status, lastHeartbeat: Date.now() } : n
          ),
        })),

      updateNodeTokens: (id, tokensIn, tokensOut) =>
        set((s) => ({
          nodes: s.nodes.map((n) =>
            n.id === id ? { ...n, tokensTodayIn: tokensIn, tokensTodayOut: tokensOut } : n
          ),
        })),

      setScale: (scale) => set({ scale: Math.max(0.2, Math.min(2.5, scale)) }),
      setPanOffset: (offset) => set({ panOffset: offset }),
      toggleNodeHidden: (id) =>
        set((s) => ({
          nodes: s.nodes.map((n) => (n.id === id ? { ...n, hidden: !n.hidden } : n)),
        })),
      clear: () => set({ nodes: [], edges: [], selectedNodeId: null }),
    }),
    {
      name: "ato-workspace",
      partialize: (s) => ({
        nodes: s.nodes.map(({ status, lastHeartbeat, tokensTodayIn, tokensTodayOut, ...rest }) => ({
          ...rest,
          status: "idle" as const,
        })),
        edges: s.edges,
        scale: s.scale,
        panOffset: s.panOffset,
        mode: s.mode,
      }),
    }
  )
);
