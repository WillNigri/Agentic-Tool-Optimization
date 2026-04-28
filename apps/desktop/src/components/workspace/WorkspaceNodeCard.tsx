import { Bot, Server, Sparkles, Cpu, BookOpen, Zap } from "lucide-react";
import { cn } from "@/lib/utils";
import type { WorkspaceNode, WorkspaceNodeKind } from "@/stores/useWorkspaceStore";

interface WorkspaceNodeCardProps {
  node: WorkspaceNode;
  isSelected: boolean;
  zoomLevel: "bird" | "normal" | "focused";
  onSelect: () => void;
  onDoubleClick?: () => void;
}

const KIND_ICONS: Record<WorkspaceNodeKind, typeof Bot> = {
  runtime: Cpu,
  skill: Sparkles,
  mcp: Server,
  process: Zap,
  memory: BookOpen,
};

const RUNTIME_COLORS: Record<string, string> = {
  claude: "#f97316",
  codex: "#22c55e",
  gemini: "#3b82f6",
  openclaw: "#06b6d4",
  hermes: "#a855f7",
};

const STATUS_COLORS: Record<string, string> = {
  online: "#22c55e",
  offline: "#6b7280",
  busy: "#f59e0b",
  error: "#ef4444",
  idle: "#3b82f6",
};

export default function WorkspaceNodeCard({
  node,
  isSelected,
  zoomLevel,
  onSelect,
  onDoubleClick,
}: WorkspaceNodeCardProps) {
  const Icon = KIND_ICONS[node.kind];
  const accentColor = node.runtime ? RUNTIME_COLORS[node.runtime] : "#00FFB2";
  const statusColor = STATUS_COLORS[node.status];

  if (node.hidden) return null;

  // Bird's eye — minimal circle
  if (zoomLevel === "bird") {
    return (
      <div
        onClick={onSelect}
        onDoubleClick={onDoubleClick}
        className={cn(
          "absolute flex flex-col items-center gap-1 cursor-pointer transition-transform hover:scale-110",
          isSelected && "scale-110"
        )}
        style={{ left: node.x, top: node.y, width: node.width ?? 200 }}
      >
        <div
          className="w-10 h-10 rounded-full flex items-center justify-center border-2"
          style={{
            backgroundColor: `${accentColor}20`,
            borderColor: isSelected ? accentColor : `${accentColor}60`,
            boxShadow: node.status === "online" ? `0 0 12px ${statusColor}40` : undefined,
          }}
        >
          <div className="w-2.5 h-2.5 rounded-full" style={{ backgroundColor: statusColor }} />
        </div>
        <span className="text-[9px] text-cs-muted text-center truncate max-w-[80px]">{node.label}</span>
      </div>
    );
  }

  // Normal + Focused
  return (
    <div
      onClick={onSelect}
      onDoubleClick={onDoubleClick}
      className={cn(
        "absolute rounded-xl border transition-all duration-200 cursor-pointer",
        isSelected ? "ring-1 ring-offset-1 ring-offset-transparent" : "hover:border-opacity-60"
      )}
      style={{
        left: node.x,
        top: node.y,
        width: node.width ?? 200,
        backgroundColor: "#161620",
        borderColor: isSelected ? accentColor : "#2a2a3a",
        boxShadow: isSelected
          ? `0 0 20px ${accentColor}30`
          : node.status === "online"
          ? `0 0 8px ${statusColor}15`
          : undefined,
        ...(isSelected ? { ringColor: accentColor } : {}),
      }}
    >
      {/* Color accent bar */}
      <div className="h-[3px] rounded-t-xl" style={{ backgroundColor: accentColor }} />

      {/* Body */}
      <div className="px-3 py-2.5">
        {/* Header row */}
        <div className="flex items-center gap-2 mb-1">
          <div
            className="w-6 h-6 rounded-md flex items-center justify-center shrink-0"
            style={{ backgroundColor: `${accentColor}15` }}
          >
            <Icon size={13} style={{ color: accentColor }} />
          </div>
          <span className="text-sm font-medium truncate flex-1">{node.label}</span>
          {/* Status dot */}
          <div className="relative shrink-0">
            <div className="w-2 h-2 rounded-full" style={{ backgroundColor: statusColor }} />
            {node.status === "online" && (
              <div
                className="absolute inset-0 w-2 h-2 rounded-full animate-ping"
                style={{ backgroundColor: statusColor, opacity: 0.4 }}
              />
            )}
          </div>
        </div>

        {/* Description */}
        <p className="text-[10px] text-cs-muted line-clamp-1 mb-1.5">{node.description}</p>

        {/* Stats row — only in focused mode */}
        {zoomLevel === "focused" && (
          <div className="flex items-center gap-3 text-[9px] text-cs-muted pt-1 border-t border-white/5">
            {node.skillCount !== undefined && (
              <span className="flex items-center gap-1">
                <Sparkles size={8} /> {node.skillCount} skills
              </span>
            )}
            {node.mcpCount !== undefined && (
              <span className="flex items-center gap-1">
                <Server size={8} /> {node.mcpCount} MCP
              </span>
            )}
            {(node.tokensTodayIn || node.tokensTodayOut) && (
              <span className="font-mono">
                {((node.tokensTodayIn ?? 0) + (node.tokensTodayOut ?? 0)).toLocaleString()} tok
              </span>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
