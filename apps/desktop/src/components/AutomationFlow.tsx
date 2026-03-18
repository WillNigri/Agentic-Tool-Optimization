import { useState, useRef, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import {
  MessageSquare,
  Layers,
  BookOpen,
  FileText,
  Wrench,
  ShieldCheck,
  Play,
  CheckCircle,
  Package,
  Bell,
  ZoomIn,
  ZoomOut,
  X,
  Search,
  Activity,
  AlertTriangle,
  Clock,
  Hash,
  Mail,
  MessageCircle,
  Database,
  GitBranch,
  Globe,
  Filter,
  Send,
  FileCode,
  ChevronDown,
  Plus,
  ToggleLeft,
  ToggleRight,
  Calendar,
} from "lucide-react";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface FlowNode {
  id: string;
  label: string;
  description: string;
  type: "trigger" | "process" | "decision" | "action" | "output" | "service";
  service?: string; // external service name (gmail, slack, github, etc.)
  x: number;
  y: number;
  stats: {
    executions: number;
    errors: number;
    avgTimeMs: number;
  };
  status: "active" | "idle" | "error";
}

interface FlowEdge {
  from: string;
  to: string;
  label?: string;
  animated?: boolean;
}

interface Workflow {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  lastRun?: string;
  runCount: number;
  errorCount: number;
  nodes: FlowNode[];
  edges: FlowEdge[];
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const NODE_W = 180;
const NODE_H = 90;

const TYPE_COLORS: Record<string, string> = {
  trigger: "#00FFB2",
  process: "#3b82f6",
  decision: "#FFB800",
  action: "#a78bfa",
  output: "#00FFB2",
  service: "#f97316",
};

// Service brand colors
const SERVICE_COLORS: Record<string, string> = {
  gmail: "#EA4335",
  slack: "#4A154B",
  github: "#8b5cf6",
  postgres: "#336791",
  notion: "#000000",
  linear: "#5E6AD2",
  jira: "#0052CC",
  discord: "#5865F2",
  calendar: "#4285F4",
};

const SERVICE_ICONS: Record<string, React.ElementType> = {
  gmail: Mail,
  slack: MessageCircle,
  github: GitBranch,
  postgres: Database,
  notion: FileText,
  linear: Activity,
  jira: Wrench,
  discord: MessageCircle,
  calendar: Calendar,
};

const NODE_ICONS: Record<string, React.ElementType> = {
  trigger: MessageSquare,
  process: Layers,
  decision: ShieldCheck,
  action: Play,
  output: CheckCircle,
  service: Globe,
};

// ---------------------------------------------------------------------------
// Mock Workflows — real user-configured automations
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
// Helpers
// ---------------------------------------------------------------------------

function getConnectionPoints(
  fromNode: FlowNode,
  toNode: FlowNode
): { x1: number; y1: number; x2: number; y2: number } {
  const fromCenterX = fromNode.x + NODE_W / 2;
  const fromCenterY = fromNode.y + NODE_H / 2;
  const toCenterX = toNode.x + NODE_W / 2;
  const toCenterY = toNode.y + NODE_H / 2;

  const dx = toCenterX - fromCenterX;
  const dy = toCenterY - fromCenterY;

  let x1: number, y1: number, x2: number, y2: number;

  if (Math.abs(dx) > Math.abs(dy)) {
    if (dx > 0) {
      x1 = fromNode.x + NODE_W; y1 = fromCenterY;
      x2 = toNode.x; y2 = toCenterY;
    } else {
      x1 = fromNode.x; y1 = fromCenterY;
      x2 = toNode.x + NODE_W; y2 = toCenterY;
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

function buildBezierPath(x1: number, y1: number, x2: number, y2: number): string {
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
// Sub-components
// ---------------------------------------------------------------------------

function FlowNodeCard({
  node,
  isSelected,
  onClick,
}: {
  node: FlowNode;
  isSelected: boolean;
  onClick: () => void;
}) {
  const isService = node.type === "service" && node.service;
  const barColor = isService
    ? SERVICE_COLORS[node.service!] || TYPE_COLORS.service
    : TYPE_COLORS[node.type];
  const IconComponent = isService
    ? SERVICE_ICONS[node.service!] || Globe
    : NODE_ICONS[node.type] || Activity;

  return (
    <div
      onClick={onClick}
      className={cn(
        "absolute cursor-pointer select-none rounded-lg transition-all duration-200",
        "hover:brightness-110"
      )}
      style={{
        left: node.x,
        top: node.y,
        width: NODE_W,
        height: NODE_H,
        zIndex: isSelected ? 20 : 10,
      }}
    >
      <div
        className={cn(
          "relative h-full w-full rounded-lg border overflow-hidden",
          isSelected ? "ring-2" : "",
          node.status === "error" ? "border-[#FF4466]" : "border-[#2a2a3a]"
        )}
        style={{
          background: "#16161e",
          ...(isSelected ? { borderColor: barColor } : {}),
          ...(node.status === "active"
            ? { boxShadow: `0 0 12px 2px ${barColor}33` }
            : {}),
          ...(isSelected ? { boxShadow: `0 0 16px 3px ${barColor}44` } : {}),
        }}
      >
        {/* Top color bar */}
        <div className="w-full" style={{ height: 3, background: barColor }} />

        {/* Content */}
        <div className="px-2.5 pt-1.5 pb-1">
          <div className="flex items-center gap-1.5 mb-0.5">
            <IconComponent size={13} style={{ color: barColor, flexShrink: 0 }} />
            <span
              className="font-semibold text-[#e8e8f0] truncate"
              style={{ fontSize: 13, lineHeight: "18px" }}
            >
              {node.label}
            </span>
            {isService && (
              <span
                className="ml-auto rounded px-1 py-0 text-[8px] font-bold uppercase tracking-wider shrink-0"
                style={{ color: barColor, background: `${barColor}18` }}
              >
                {node.service}
              </span>
            )}
          </div>
          <p
            className="text-[#8888a0] truncate"
            style={{ fontSize: 11, lineHeight: "14px" }}
          >
            {node.description}
          </p>
        </div>

        {/* Stats row */}
        <div
          className="absolute bottom-0 left-0 right-0 flex items-center gap-2 px-2.5 py-1 border-t border-[#2a2a3a]"
          style={{ fontSize: 10 }}
        >
          <span className="flex items-center gap-0.5 text-[#8888a0]">
            <Hash size={9} />
            {node.stats.executions}
          </span>
          <span
            className={cn(
              "flex items-center gap-0.5",
              node.stats.errors > 0 ? "text-[#FF4466]" : "text-[#8888a0]"
            )}
          >
            <AlertTriangle size={9} />
            {node.stats.errors}
          </span>
          <span className="flex items-center gap-0.5 text-[#8888a0] ml-auto">
            <Clock size={9} />
            {node.stats.avgTimeMs}ms
          </span>
        </div>
      </div>
    </div>
  );
}

function EdgeLabel({ label, x, y }: { label: string; x: number; y: number }) {
  return (
    <div
      className="absolute pointer-events-none"
      style={{ left: x - 30, top: y - 10, zIndex: 5 }}
    >
      <span
        className="rounded px-1.5 py-0.5 text-[#e8e8f0] font-medium"
        style={{ fontSize: 9, background: "#16161e", border: "1px solid #2a2a3a" }}
      >
        {label}
      </span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Workflow Selector (top bar)
// ---------------------------------------------------------------------------

function WorkflowSelector({
  workflows,
  activeId,
  onSelect,
  onToggle,
}: {
  workflows: Workflow[];
  activeId: string;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const active = workflows.find((w) => w.id === activeId)!;

  return (
    <div className="relative flex items-center gap-3 px-4 py-3 border-b border-[#2a2a3a]" style={{ background: "#0e0e16" }}>
      {/* Dropdown */}
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-[#2a2a3a] hover:border-[#00FFB2] transition-colors"
        style={{ background: "#16161e" }}
      >
        <span className="text-sm font-semibold text-[#e8e8f0]">{active.name}</span>
        <ChevronDown size={14} className={cn("text-[#8888a0] transition-transform", open && "rotate-180")} />
      </button>

      {/* Active workflow meta */}
      <p className="text-xs text-[#8888a0] flex-1 truncate">{active.description}</p>

      {/* Enable/disable toggle */}
      <button
        onClick={() => onToggle(activeId)}
        className="flex items-center gap-1.5 text-xs shrink-0"
      >
        {active.enabled ? (
          <>
            <ToggleRight size={18} className="text-[#00FFB2]" />
            <span className="text-[#00FFB2]">{t("automation.enabled", "Enabled")}</span>
          </>
        ) : (
          <>
            <ToggleLeft size={18} className="text-[#8888a0]" />
            <span className="text-[#8888a0]">{t("automation.disabled", "Disabled")}</span>
          </>
        )}
      </button>

      {active.lastRun && (
        <span className="text-[10px] text-[#8888a0] shrink-0">
          {t("automation.lastRun", "Last run")}: {active.lastRun}
        </span>
      )}

      {/* Dropdown menu */}
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div
            className="absolute top-full left-4 mt-1 w-80 rounded-lg border border-[#2a2a3a] shadow-xl overflow-hidden z-50"
            style={{ background: "#16161e" }}
          >
            {workflows.map((w) => {
              // Count unique services in this workflow
              const services = [...new Set(w.nodes.filter((n) => n.service).map((n) => n.service!))];
              return (
                <button
                  key={w.id}
                  onClick={() => { onSelect(w.id); setOpen(false); }}
                  className={cn(
                    "w-full text-left px-3 py-2.5 border-b border-[#2a2a3a] last:border-0 transition-colors",
                    w.id === activeId ? "bg-[#00FFB208]" : "hover:bg-[#0a0a0f]"
                  )}
                >
                  <div className="flex items-center gap-2 mb-1">
                    <span className={cn(
                      "w-2 h-2 rounded-full shrink-0",
                      w.enabled ? "bg-[#00FFB2]" : "bg-[#8888a0]/40"
                    )} />
                    <span className="text-sm font-medium text-[#e8e8f0]">{w.name}</span>
                    <span className="text-[10px] text-[#8888a0] ml-auto">{w.runCount} runs</span>
                  </div>
                  <p className="text-[11px] text-[#8888a0] truncate pl-4 mb-1.5">{w.description}</p>
                  {/* Service icons */}
                  <div className="flex items-center gap-1.5 pl-4">
                    {services.map((s) => {
                      const Icon = SERVICE_ICONS[s] || Globe;
                      const color = SERVICE_COLORS[s] || "#8888a0";
                      return (
                        <div
                          key={s}
                          className="flex items-center gap-1 rounded px-1.5 py-0.5"
                          style={{ background: `${color}15` }}
                        >
                          <Icon size={10} style={{ color }} />
                          <span className="text-[9px] font-medium" style={{ color }}>{s}</span>
                        </div>
                      );
                    })}
                  </div>
                </button>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main Component
// ---------------------------------------------------------------------------

export default function AutomationFlow() {
  const { t } = useTranslation();
  const [workflows, setWorkflows] = useState(MOCK_WORKFLOWS);
  const [activeWorkflowId, setActiveWorkflowId] = useState(MOCK_WORKFLOWS[0].id);
  const [selectedNode, setSelectedNode] = useState<FlowNode | null>(null);
  const [scale, setScale] = useState(0.85);
  const [panOffset, setPanOffset] = useState({ x: 20, y: 20 });
  const [isPanning, setIsPanning] = useState(false);
  const [panStart, setPanStart] = useState({ x: 0, y: 0 });
  const [searchQuery, setSearchQuery] = useState("");
  const canvasRef = useRef<HTMLDivElement>(null);

  const activeWorkflow = workflows.find((w) => w.id === activeWorkflowId)!;
  const nodes = activeWorkflow.nodes;
  const edges = activeWorkflow.edges;

  // Reset selection when switching workflows
  useEffect(() => {
    setSelectedNode(null);
    setPanOffset({ x: 20, y: 20 });
  }, [activeWorkflowId]);

  // Canvas dimensions — computed from nodes
  const canvasW = Math.max(1400, ...nodes.map((n) => n.x + NODE_W + 80));
  const canvasH = Math.max(450, ...nodes.map((n) => n.y + NODE_H + 80));

  function toggleWorkflow(id: string) {
    setWorkflows((prev) =>
      prev.map((w) => (w.id === id ? { ...w, enabled: !w.enabled } : w))
    );
  }

  // ---- Pan handlers ----
  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (e.button !== 0) return;
      if ((e.target as HTMLElement).closest("[data-node]")) return;
      setIsPanning(true);
      setPanStart({ x: e.clientX - panOffset.x, y: e.clientY - panOffset.y });
    },
    [panOffset]
  );

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!isPanning) return;
      setPanOffset({ x: e.clientX - panStart.x, y: e.clientY - panStart.y });
    },
    [isPanning, panStart]
  );

  const handleMouseUp = useCallback(() => setIsPanning(false), []);

  // Zoom with wheel
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const handler = (e: WheelEvent) => {
      e.preventDefault();
      const delta = e.deltaY > 0 ? -0.05 : 0.05;
      setScale((s) => Math.min(2, Math.max(0.3, s + delta)));
    };
    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  }, []);

  const zoomIn = () => setScale((s) => Math.min(2, s + 0.1));
  const zoomOut = () => setScale((s) => Math.max(0.3, s - 0.1));

  const nodeMap = new Map(nodes.map((n) => [n.id, n]));

  // Stats
  const totalExec = nodes.reduce((s, n) => s + n.stats.executions, 0);
  const totalErrors = nodes.reduce((s, n) => s + n.stats.errors, 0);
  const services = [...new Set(nodes.filter((n) => n.service).map((n) => n.service!))];

  const filteredNodes = nodes.filter((n) =>
    n.label.toLowerCase().includes(searchQuery.toLowerCase()) ||
    (n.service || "").toLowerCase().includes(searchQuery.toLowerCase())
  );

  return (
    <div className="flex flex-col h-full w-full" style={{ background: "#0a0a0f" }}>
      {/* Workflow selector bar */}
      <WorkflowSelector
        workflows={workflows}
        activeId={activeWorkflowId}
        onSelect={setActiveWorkflowId}
        onToggle={toggleWorkflow}
      />

      <div className="flex flex-1 overflow-hidden">
        {/* ---- Canvas Area ---- */}
        <div
          ref={canvasRef}
          className="relative flex-1 overflow-hidden"
          style={{ cursor: isPanning ? "grabbing" : "grab" }}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
        >
          <div
            style={{
              transform: `translate(${panOffset.x}px, ${panOffset.y}px) scale(${scale})`,
              transformOrigin: "0 0",
              width: canvasW,
              height: canvasH,
              position: "relative",
            }}
          >
            {/* SVG layer */}
            <svg
              width={canvasW}
              height={canvasH}
              className="absolute inset-0"
              style={{ zIndex: 1 }}
            >
              <defs>
                <pattern id="grid" width="20" height="20" patternUnits="userSpaceOnUse">
                  <circle cx="10" cy="10" r="1" fill="#2a2a3a" />
                </pattern>
                <style>{`
                  @keyframes flowDash {
                    to { stroke-dashoffset: -20; }
                  }
                  .edge-animated {
                    stroke-dasharray: 6 4;
                    animation: flowDash 0.8s linear infinite;
                  }
                `}</style>
              </defs>

              <rect width={canvasW} height={canvasH} fill="url(#grid)" />

              {edges.map((edge) => {
                const fromNode = nodeMap.get(edge.from);
                const toNode = nodeMap.get(edge.to);
                if (!fromNode || !toNode) return null;

                const { x1, y1, x2, y2 } = getConnectionPoints(fromNode, toNode);
                const path = buildBezierPath(x1, y1, x2, y2);

                // Color the edge based on destination service
                const edgeColor = toNode.service
                  ? SERVICE_COLORS[toNode.service] || "#00FFB2"
                  : "#00FFB2";

                const isActive = edge.animated || fromNode.status === "active" || toNode.status === "active";

                return (
                  <g key={`${edge.from}-${edge.to}`}>
                    <path d={path} fill="none" stroke={edgeColor} strokeWidth={3} strokeOpacity={0.08} />
                    <path
                      d={path}
                      fill="none"
                      stroke={edgeColor}
                      strokeWidth={1.5}
                      strokeOpacity={isActive ? 0.7 : 0.3}
                      className={cn(edge.animated && "edge-animated")}
                    />
                    <circle cx={x1} cy={y1} r={3} fill={edgeColor} opacity={0.5} />
                    <circle cx={x2} cy={y2} r={3} fill={edgeColor} opacity={0.5} />
                  </g>
                );
              })}
            </svg>

            {/* Edge labels */}
            {edges.map((edge) => {
              if (!edge.label) return null;
              const fromNode = nodeMap.get(edge.from);
              const toNode = nodeMap.get(edge.to);
              if (!fromNode || !toNode) return null;
              const { x1, y1, x2, y2 } = getConnectionPoints(fromNode, toNode);
              return (
                <EdgeLabel
                  key={`label-${edge.from}-${edge.to}`}
                  label={edge.label}
                  x={(x1 + x2) / 2}
                  y={(y1 + y2) / 2}
                />
              );
            })}

            {/* Node cards */}
            {nodes.map((node) => (
              <div key={node.id} data-node>
                <FlowNodeCard
                  node={node}
                  isSelected={selectedNode?.id === node.id}
                  onClick={() =>
                    setSelectedNode((prev) => (prev?.id === node.id ? null : node))
                  }
                />
              </div>
            ))}
          </div>

          {/* Zoom controls */}
          <div className="absolute bottom-4 right-4 flex flex-col gap-1" style={{ zIndex: 30 }}>
            <button
              onClick={zoomIn}
              className="flex items-center justify-center w-8 h-8 rounded-md border border-[#2a2a3a] hover:border-[#00FFB2] transition-colors"
              style={{ background: "#16161e" }}
            >
              <ZoomIn size={14} className="text-[#e8e8f0]" />
            </button>
            <button
              onClick={zoomOut}
              className="flex items-center justify-center w-8 h-8 rounded-md border border-[#2a2a3a] hover:border-[#00FFB2] transition-colors"
              style={{ background: "#16161e" }}
            >
              <ZoomOut size={14} className="text-[#e8e8f0]" />
            </button>
            <span className="text-center text-[10px] text-[#8888a0] mt-0.5" style={{ fontVariantNumeric: "tabular-nums" }}>
              {Math.round(scale * 100)}%
            </span>
          </div>
        </div>

        {/* ---- Right Panel ---- */}
        <div className="w-72 flex-shrink-0 border-l border-[#2a2a3a] overflow-y-auto" style={{ background: "#0e0e16" }}>
          <div className="p-4">
            <h2 className="text-[#e8e8f0] font-semibold text-sm mb-1">
              {activeWorkflow.name}
            </h2>
            <p className="text-[11px] text-[#8888a0] mb-4">{activeWorkflow.description}</p>

            {/* Services used */}
            <div className="mb-4">
              <h3 className="text-[10px] text-[#8888a0] uppercase tracking-wider mb-2 font-medium">
                {t("automation.services", "Connected Services")}
              </h3>
              <div className="flex flex-wrap gap-1.5">
                {services.map((s) => {
                  const Icon = SERVICE_ICONS[s] || Globe;
                  const color = SERVICE_COLORS[s] || "#8888a0";
                  return (
                    <div
                      key={s}
                      className="flex items-center gap-1.5 rounded-md px-2 py-1 border"
                      style={{ borderColor: `${color}40`, background: `${color}10` }}
                    >
                      <Icon size={12} style={{ color }} />
                      <span className="text-xs font-medium capitalize" style={{ color }}>{s}</span>
                    </div>
                  );
                })}
              </div>
            </div>

            {/* Stats */}
            <div className="grid grid-cols-2 gap-2 mb-4">
              <div className="rounded-md border border-[#2a2a3a] px-2.5 py-2 text-center" style={{ background: "#16161e" }}>
                <p className="text-lg font-bold text-[#e8e8f0]">{activeWorkflow.runCount}</p>
                <p className="text-[10px] text-[#8888a0]">{t("automation.totalRuns", "Total Runs")}</p>
              </div>
              <div className="rounded-md border border-[#2a2a3a] px-2.5 py-2 text-center" style={{ background: "#16161e" }}>
                <p className={cn("text-lg font-bold", activeWorkflow.errorCount > 0 ? "text-[#FF4466]" : "text-[#e8e8f0]")}>
                  {activeWorkflow.errorCount}
                </p>
                <p className="text-[10px] text-[#8888a0]">{t("automation.errors", "Errors")}</p>
              </div>
            </div>

            {/* Search */}
            <div className="relative mb-3">
              <Search size={12} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-[#8888a0]" />
              <input
                type="text"
                placeholder={t("automation.search", "Search nodes...")}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 pl-7 pr-2 placeholder-[#8888a0] focus:outline-none focus:border-[#00FFB2] transition-colors"
              />
            </div>

            {/* Node list */}
            <h3 className="text-[10px] text-[#8888a0] uppercase tracking-wider mb-2 font-medium">
              {t("automation.flowSteps", "Flow Steps")} ({nodes.length})
            </h3>
            <div className="flex flex-col gap-1.5">
              {filteredNodes.map((node) => {
                const isService = node.type === "service" && node.service;
                const barColor = isService
                  ? SERVICE_COLORS[node.service!] || TYPE_COLORS.service
                  : TYPE_COLORS[node.type];
                const IconComponent = isService
                  ? SERVICE_ICONS[node.service!] || Globe
                  : NODE_ICONS[node.type] || Activity;
                const isSelected = selectedNode?.id === node.id;

                return (
                  <button
                    key={node.id}
                    onClick={() =>
                      setSelectedNode((prev) => (prev?.id === node.id ? null : node))
                    }
                    className={cn(
                      "flex items-center gap-2 w-full rounded-md px-2.5 py-2 text-left transition-colors border",
                      isSelected
                        ? "border-[#00FFB2] bg-[#00FFB208]"
                        : "border-transparent hover:bg-[#16161e]"
                    )}
                  >
                    <IconComponent size={12} style={{ color: barColor, flexShrink: 0 }} />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-1.5">
                        <p className="text-[11px] font-medium text-[#e8e8f0] truncate">
                          {node.label}
                        </p>
                        {isService && (
                          <span className="text-[8px] font-bold uppercase" style={{ color: barColor }}>
                            {node.service}
                          </span>
                        )}
                      </div>
                      <p className="text-[10px] text-[#8888a0]">
                        {node.stats.executions} runs
                        {node.stats.errors > 0 && (
                          <span className="text-[#FF4466] ml-1">{node.stats.errors} err</span>
                        )}
                        <span className="ml-1">{node.stats.avgTimeMs}ms</span>
                      </p>
                    </div>
                    <div
                      className="w-1.5 h-1.5 rounded-full flex-shrink-0"
                      style={{
                        background:
                          node.status === "active" ? "#00FFB2"
                            : node.status === "error" ? "#FF4466"
                            : "#8888a0",
                      }}
                    />
                  </button>
                );
              })}
            </div>
          </div>
        </div>
      </div>

      {/* ---- Bottom Detail Panel ---- */}
      {selectedNode && (
        <div
          className="border-t border-[#2a2a3a]"
          style={{ background: "#16161e", zIndex: 40 }}
        >
          <div className="flex items-start justify-between px-4 py-3">
            <div className="flex gap-6">
              {/* Node identity */}
              <div>
                <div className="flex items-center gap-2 mb-1">
                  {(() => {
                    const isService = selectedNode.type === "service" && selectedNode.service;
                    const barColor = isService
                      ? SERVICE_COLORS[selectedNode.service!] || TYPE_COLORS.service
                      : TYPE_COLORS[selectedNode.type];
                    const Icon = isService
                      ? SERVICE_ICONS[selectedNode.service!] || Globe
                      : NODE_ICONS[selectedNode.type] || Activity;
                    return <Icon size={16} style={{ color: barColor }} />;
                  })()}
                  <h3 className="text-sm font-semibold text-[#e8e8f0]">{selectedNode.label}</h3>
                  {selectedNode.service && (
                    <span
                      className="rounded px-1.5 py-0.5 text-[9px] font-bold uppercase"
                      style={{
                        color: SERVICE_COLORS[selectedNode.service] || "#f97316",
                        background: `${SERVICE_COLORS[selectedNode.service] || "#f97316"}18`,
                      }}
                    >
                      {selectedNode.service} MCP
                    </span>
                  )}
                  <span
                    className="rounded px-1.5 py-0.5 text-[9px] font-medium uppercase"
                    style={{
                      color: TYPE_COLORS[selectedNode.type],
                      background: `${TYPE_COLORS[selectedNode.type]}18`,
                    }}
                  >
                    {selectedNode.type}
                  </span>
                </div>
                <p className="text-xs text-[#8888a0]">{selectedNode.description}</p>
              </div>

              {/* Stats cards */}
              <div className="flex gap-3">
                <div className="rounded-md border border-[#2a2a3a] px-3 py-2 text-center">
                  <p className="text-lg font-bold text-[#e8e8f0]">{selectedNode.stats.executions}</p>
                  <p className="text-[10px] text-[#8888a0]">{t("automation.executions", "Executions")}</p>
                </div>
                <div className="rounded-md border border-[#2a2a3a] px-3 py-2 text-center">
                  <p className={cn("text-lg font-bold", selectedNode.stats.errors > 0 ? "text-[#FF4466]" : "text-[#e8e8f0]")}>
                    {selectedNode.stats.errors}
                  </p>
                  <p className="text-[10px] text-[#8888a0]">{t("automation.errors", "Errors")}</p>
                </div>
                <div className="rounded-md border border-[#2a2a3a] px-3 py-2 text-center">
                  <p className="text-lg font-bold text-[#e8e8f0]">
                    {selectedNode.stats.avgTimeMs}
                    <span className="text-xs font-normal text-[#8888a0]">ms</span>
                  </p>
                  <p className="text-[10px] text-[#8888a0]">{t("automation.avgTime", "Avg Time")}</p>
                </div>
              </div>
            </div>

            <button
              onClick={() => setSelectedNode(null)}
              className="flex items-center justify-center w-7 h-7 rounded-md border border-[#2a2a3a] hover:border-[#FF4466] transition-colors flex-shrink-0"
              style={{ background: "#0e0e16" }}
            >
              <X size={14} className="text-[#8888a0]" />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
