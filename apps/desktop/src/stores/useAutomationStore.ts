import { create } from "zustand";
import type {
  FlowNode,
  FlowEdge,
  Workflow,
  BuilderMode,
  ConnectingState,
  ExecutionState,
  ExecutionNodeStatus,
} from "@/components/automation/types";

// ---------------------------------------------------------------------------
// Mock workflows (moved from AutomationFlow.tsx)
// ---------------------------------------------------------------------------

const MOCK_WORKFLOWS: Workflow[] = [
  {
    id: "pr-review-notify",
    name: "PR Review Pipeline",
    description: "Auto-review PRs, post summary to Slack, update Linear ticket",
    enabled: true,
    lastRun: "2 min ago",
    runCount: 142,
    errorCount: 3,
    nodes: [
      { id: "gh-webhook", label: "GitHub PR Opened", description: "Triggered when a new PR is created", type: "trigger", service: "github", x: 50, y: 180, stats: { executions: 142, errors: 0, avgTimeMs: 80 }, status: "active" },
      { id: "fetch-diff", label: "Fetch PR Diff", description: "Read changed files via GitHub MCP", type: "service", service: "github", x: 280, y: 180, stats: { executions: 142, errors: 1, avgTimeMs: 650 }, status: "active" },
      { id: "load-review-skill", label: "Load code-review", description: "Activate code-review skill for analysis", type: "process", x: 510, y: 100, stats: { executions: 142, errors: 0, avgTimeMs: 30 }, status: "idle" },
      { id: "load-conventions", label: "Load conventions", description: "Load project-conventions skill", type: "process", x: 510, y: 260, stats: { executions: 142, errors: 0, avgTimeMs: 25 }, status: "idle" },
      { id: "analyze-code", label: "Analyze Changes", description: "Claude reviews diff with loaded skills", type: "action", x: 740, y: 180, stats: { executions: 142, errors: 2, avgTimeMs: 4200 }, status: "active" },
      { id: "check-security", label: "Security Check", description: "Run security-policy skill validation", type: "decision", x: 970, y: 100, stats: { executions: 142, errors: 0, avgTimeMs: 890 }, status: "idle" },
      { id: "post-gh-comment", label: "Post PR Comment", description: "Write review comment on GitHub PR", type: "service", service: "github", x: 970, y: 260, stats: { executions: 139, errors: 0, avgTimeMs: 340 }, status: "active" },
      { id: "notify-slack", label: "Notify Slack", description: "Post review summary to #code-reviews", type: "service", service: "slack", x: 1200, y: 100, stats: { executions: 139, errors: 1, avgTimeMs: 280 }, status: "active" },
      { id: "update-linear", label: "Update Linear", description: "Move ticket to 'In Review' status", type: "service", service: "linear", x: 1200, y: 260, stats: { executions: 134, errors: 0, avgTimeMs: 190 }, status: "idle" },
    ],
    edges: [
      { from: "gh-webhook", to: "fetch-diff", animated: true },
      { from: "fetch-diff", to: "load-review-skill" },
      { from: "fetch-diff", to: "load-conventions" },
      { from: "load-review-skill", to: "analyze-code" },
      { from: "load-conventions", to: "analyze-code" },
      { from: "analyze-code", to: "check-security" },
      { from: "analyze-code", to: "post-gh-comment", animated: true },
      { from: "check-security", to: "notify-slack", label: "issues found" },
      { from: "post-gh-comment", to: "notify-slack" },
      { from: "post-gh-comment", to: "update-linear" },
    ],
  },
  {
    id: "email-digest",
    name: "Daily Email Digest",
    description: "Summarize unread emails, create tasks in Linear, send Slack brief",
    enabled: true,
    lastRun: "6h ago",
    runCount: 89,
    errorCount: 1,
    nodes: [
      { id: "cron-trigger", label: "Daily 9 AM", description: "Scheduled cron trigger every morning", type: "trigger", x: 50, y: 180, stats: { executions: 89, errors: 0, avgTimeMs: 10 }, status: "idle" },
      { id: "fetch-gmail", label: "Fetch Unread", description: "Get unread emails from Gmail inbox", type: "service", service: "gmail", x: 280, y: 180, stats: { executions: 89, errors: 1, avgTimeMs: 1200 }, status: "active" },
      { id: "filter-important", label: "Filter Priority", description: "Classify emails by urgency", type: "decision", x: 510, y: 180, stats: { executions: 89, errors: 0, avgTimeMs: 800 }, status: "idle" },
      { id: "summarize", label: "Summarize", description: "Generate concise summaries with Claude", type: "action", x: 740, y: 100, stats: { executions: 89, errors: 0, avgTimeMs: 3200 }, status: "active" },
      { id: "create-tasks", label: "Create Tasks", description: "Action items become Linear tickets", type: "service", service: "linear", x: 740, y: 260, stats: { executions: 67, errors: 0, avgTimeMs: 450 }, status: "idle" },
      { id: "slack-brief", label: "Post Brief", description: "Send morning brief to Slack #general", type: "service", service: "slack", x: 970, y: 180, stats: { executions: 89, errors: 0, avgTimeMs: 220 }, status: "active" },
    ],
    edges: [
      { from: "cron-trigger", to: "fetch-gmail", animated: true },
      { from: "fetch-gmail", to: "filter-important" },
      { from: "filter-important", to: "summarize", label: "high priority" },
      { from: "filter-important", to: "create-tasks", label: "action items" },
      { from: "summarize", to: "slack-brief", animated: true },
      { from: "create-tasks", to: "slack-brief" },
    ],
  },
  {
    id: "db-migration-guard",
    name: "DB Migration Guard",
    description: "Validate SQL migrations, check schema diffs, alert on breaking changes",
    enabled: true,
    lastRun: "1d ago",
    runCount: 34,
    errorCount: 5,
    nodes: [
      { id: "file-watch", label: "Migration File", description: "Detect new .sql files in migrations/", type: "trigger", x: 50, y: 180, stats: { executions: 34, errors: 0, avgTimeMs: 50 }, status: "idle" },
      { id: "read-migration", label: "Read SQL", description: "Parse migration file contents", type: "process", x: 280, y: 180, stats: { executions: 34, errors: 0, avgTimeMs: 15 }, status: "idle" },
      { id: "check-schema", label: "Schema Diff", description: "Compare against current DB schema", type: "service", service: "postgres", x: 510, y: 100, stats: { executions: 34, errors: 2, avgTimeMs: 780 }, status: "active" },
      { id: "validate-sql", label: "Validate SQL", description: "Check for dangerous operations", type: "decision", x: 510, y: 260, stats: { executions: 34, errors: 0, avgTimeMs: 1100 }, status: "idle" },
      { id: "alert-breaking", label: "Alert Breaking", description: "Post breaking change warning to Slack", type: "service", service: "slack", x: 740, y: 100, stats: { executions: 8, errors: 0, avgTimeMs: 190 }, status: "error" },
      { id: "approve-safe", label: "Auto-Approve", description: "Add approved label to safe migrations", type: "service", service: "github", x: 740, y: 260, stats: { executions: 26, errors: 3, avgTimeMs: 310 }, status: "active" },
    ],
    edges: [
      { from: "file-watch", to: "read-migration", animated: true },
      { from: "read-migration", to: "check-schema" },
      { from: "read-migration", to: "validate-sql" },
      { from: "check-schema", to: "alert-breaking", label: "breaking" },
      { from: "validate-sql", to: "approve-safe", label: "safe" },
      { from: "validate-sql", to: "alert-breaking", label: "dangerous" },
    ],
  },
  {
    id: "standup-bot",
    name: "Standup Bot",
    description: "Collect daily standups from Slack, summarize, post to Notion",
    enabled: false,
    lastRun: "3d ago",
    runCount: 21,
    errorCount: 0,
    nodes: [
      { id: "slack-collect", label: "Collect Updates", description: "Read #standup channel messages", type: "service", service: "slack", x: 50, y: 180, stats: { executions: 21, errors: 0, avgTimeMs: 600 }, status: "idle" },
      { id: "parse-updates", label: "Parse Updates", description: "Extract blockers, progress, plans", type: "process", x: 280, y: 180, stats: { executions: 21, errors: 0, avgTimeMs: 900 }, status: "idle" },
      { id: "generate-summary", label: "Generate Summary", description: "Create team standup digest", type: "action", x: 510, y: 180, stats: { executions: 21, errors: 0, avgTimeMs: 2800 }, status: "idle" },
      { id: "post-notion", label: "Save to Notion", description: "Create page in Team Standups DB", type: "service", service: "notion", x: 740, y: 120, stats: { executions: 21, errors: 0, avgTimeMs: 440 }, status: "idle" },
      { id: "post-summary", label: "Post Summary", description: "Share digest back to Slack", type: "service", service: "slack", x: 740, y: 260, stats: { executions: 21, errors: 0, avgTimeMs: 180 }, status: "idle" },
    ],
    edges: [
      { from: "slack-collect", to: "parse-updates" },
      { from: "parse-updates", to: "generate-summary" },
      { from: "generate-summary", to: "post-notion" },
      { from: "generate-summary", to: "post-summary" },
    ],
  },
];

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

interface AutomationStore {
  // Mode
  mode: BuilderMode;
  setMode: (mode: BuilderMode) => void;

  // Workflows
  workflows: Workflow[];
  activeWorkflowId: string;
  setActiveWorkflowId: (id: string) => void;
  toggleWorkflow: (id: string) => void;
  dirty: boolean;

  // Active workflow accessors
  getActiveWorkflow: () => Workflow;

  // Selection
  selectedNodeId: string | null;
  selectedEdgeKey: string | null;
  selectNode: (id: string | null) => void;
  selectEdge: (key: string | null) => void;

  // Node operations (edit mode)
  addNode: (node: FlowNode) => void;
  updateNode: (id: string, updates: Partial<FlowNode>) => void;
  moveNode: (id: string, x: number, y: number) => void;
  deleteNode: (id: string) => void;

  // Edge operations
  connecting: ConnectingState | null;
  startConnecting: (fromNodeId: string) => void;
  cancelConnecting: () => void;
  addEdge: (edge: FlowEdge) => void;
  deleteEdge: (from: string, to: string) => void;

  // Workflow CRUD
  createWorkflow: (name: string) => void;
  deleteWorkflow: (id: string) => void;
  saveWorkflow: () => void;
  loadWorkflows: (workflows: Workflow[]) => void;

  // Execution
  execution: ExecutionState;
  startExecution: () => void;
  updateNodeExecStatus: (nodeId: string, status: ExecutionNodeStatus) => void;
  appendOutput: (text: string) => void;
  finishExecution: (error?: string) => void;
}

let idCounter = 0;
function genId() {
  return `node-${Date.now()}-${idCounter++}`;
}

export const useAutomationStore = create<AutomationStore>((set, get) => ({
  mode: "view",
  setMode: (mode) => set({ mode }),

  workflows: [],
  activeWorkflowId: "",
  dirty: false,

  setActiveWorkflowId: (id) =>
    set({ activeWorkflowId: id, selectedNodeId: null, selectedEdgeKey: null }),

  toggleWorkflow: (id) =>
    set((s) => ({
      workflows: s.workflows.map((w) =>
        w.id === id ? { ...w, enabled: !w.enabled } : w
      ),
    })),

  getActiveWorkflow: () => {
    const s = get();
    return s.workflows.find((w) => w.id === s.activeWorkflowId) || {
      id: "",
      name: "",
      description: "",
      enabled: false,
      runCount: 0,
      errorCount: 0,
      nodes: [],
      edges: [],
    };
  },

  selectedNodeId: null,
  selectedEdgeKey: null,
  selectNode: (id) => set({ selectedNodeId: id, selectedEdgeKey: null }),
  selectEdge: (key) => set({ selectedEdgeKey: key, selectedNodeId: null }),

  // Node ops
  addNode: (node) =>
    set((s) => ({
      dirty: true,
      workflows: s.workflows.map((w) =>
        w.id === s.activeWorkflowId
          ? { ...w, nodes: [...w.nodes, node] }
          : w
      ),
    })),

  updateNode: (id, updates) =>
    set((s) => ({
      dirty: true,
      workflows: s.workflows.map((w) =>
        w.id === s.activeWorkflowId
          ? { ...w, nodes: w.nodes.map((n) => (n.id === id ? { ...n, ...updates } : n)) }
          : w
      ),
    })),

  moveNode: (id, x, y) =>
    set((s) => ({
      workflows: s.workflows.map((w) =>
        w.id === s.activeWorkflowId
          ? { ...w, nodes: w.nodes.map((n) => (n.id === id ? { ...n, x, y } : n)) }
          : w
      ),
    })),

  deleteNode: (id) =>
    set((s) => ({
      dirty: true,
      selectedNodeId: s.selectedNodeId === id ? null : s.selectedNodeId,
      workflows: s.workflows.map((w) =>
        w.id === s.activeWorkflowId
          ? {
              ...w,
              nodes: w.nodes.filter((n) => n.id !== id),
              edges: w.edges.filter((e) => e.from !== id && e.to !== id),
            }
          : w
      ),
    })),

  // Edge ops
  connecting: null,
  startConnecting: (fromNodeId) => set({ connecting: { fromNodeId, fromPort: "output" } }),
  cancelConnecting: () => set({ connecting: null }),

  addEdge: (edge) =>
    set((s) => ({
      dirty: true,
      connecting: null,
      workflows: s.workflows.map((w) =>
        w.id === s.activeWorkflowId
          ? {
              ...w,
              edges: w.edges.some((e) => e.from === edge.from && e.to === edge.to)
                ? w.edges
                : [...w.edges, edge],
            }
          : w
      ),
    })),

  deleteEdge: (from, to) =>
    set((s) => ({
      dirty: true,
      selectedEdgeKey: null,
      workflows: s.workflows.map((w) =>
        w.id === s.activeWorkflowId
          ? { ...w, edges: w.edges.filter((e) => !(e.from === from && e.to === to)) }
          : w
      ),
    })),

  // Workflow CRUD
  createWorkflow: (name) => {
    const id = `workflow-${Date.now()}`;
    const newWorkflow: Workflow = {
      id,
      name,
      description: "",
      enabled: false,
      runCount: 0,
      errorCount: 0,
      nodes: [],
      edges: [],
    };
    set((s) => ({
      workflows: [...s.workflows, newWorkflow],
      activeWorkflowId: id,
      dirty: true,
    }));
  },

  deleteWorkflow: (id) =>
    set((s) => {
      const remaining = s.workflows.filter((w) => w.id !== id);
      if (remaining.length === 0) return s;
      return {
        workflows: remaining,
        activeWorkflowId:
          s.activeWorkflowId === id ? remaining[0].id : s.activeWorkflowId,
      };
    }),

  saveWorkflow: () => set({ dirty: false }),

  loadWorkflows: (workflows) =>
    set({
      workflows,
      activeWorkflowId: workflows[0]?.id || "",
    }),

  // Execution
  execution: {
    running: false,
    nodeStatuses: {},
    output: "",
  },

  startExecution: () =>
    set({
      execution: {
        running: true,
        nodeStatuses: {},
        output: "",
        startedAt: Date.now(),
      },
    }),

  updateNodeExecStatus: (nodeId, status) =>
    set((s) => ({
      execution: {
        ...s.execution,
        nodeStatuses: { ...s.execution.nodeStatuses, [nodeId]: status },
      },
    })),

  appendOutput: (text) =>
    set((s) => ({
      execution: { ...s.execution, output: s.execution.output + text },
    })),

  finishExecution: (error) =>
    set((s) => ({
      execution: { ...s.execution, running: false, error },
    })),
}));
