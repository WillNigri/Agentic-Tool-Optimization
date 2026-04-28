import { X, ExternalLink, Sparkles, Server, BookOpen, Cpu, ToggleLeft, ToggleRight } from "lucide-react";
import { cn } from "@/lib/utils";
import type { WorkspaceNode } from "@/stores/useWorkspaceStore";

interface WorkspaceDetailPanelProps {
  node: WorkspaceNode;
  onClose: () => void;
  onOpenFile?: (path: string) => void;
  onOpenProjects?: () => void;
}

const STATUS_LABELS: Record<string, { label: string; color: string }> = {
  online: { label: "Online", color: "text-green-400" },
  offline: { label: "Offline", color: "text-gray-400" },
  busy: { label: "Busy", color: "text-amber-400" },
  error: { label: "Error", color: "text-red-400" },
  idle: { label: "Idle", color: "text-blue-400" },
};

export default function WorkspaceDetailPanel({ node, onClose, onOpenFile, onOpenProjects }: WorkspaceDetailPanelProps) {
  const statusInfo = STATUS_LABELS[node.status] ?? STATUS_LABELS.idle;

  return (
    <div className="w-72 border-l border-cs-border bg-cs-card flex flex-col shrink-0 overflow-y-auto">
      {/* Header */}
      <div className="flex items-start justify-between p-3 border-b border-cs-border">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold truncate">{node.label}</h3>
          <div className="flex items-center gap-2 mt-0.5">
            <span className={cn("text-[10px] font-medium uppercase", statusInfo.color)}>
              {statusInfo.label}
            </span>
            <span className="text-[10px] text-cs-muted capitalize">{node.kind}</span>
          </div>
        </div>
        <button onClick={onClose} className="p-1 rounded hover:bg-cs-border text-cs-muted"><X size={14} /></button>
      </div>

      {/* Description */}
      <div className="px-3 py-2 border-b border-cs-border/60">
        <p className="text-[11px] text-cs-muted">{node.description}</p>
      </div>

      {/* Stats */}
      {(node.tokensTodayIn !== undefined || node.tokensTodayOut !== undefined || node.skillCount !== undefined) && (
        <div className="px-3 py-2 border-b border-cs-border/60 grid grid-cols-2 gap-2">
          {node.skillCount !== undefined && (
            <StatCard icon={Sparkles} label="Skills" value={String(node.skillCount)} />
          )}
          {node.mcpCount !== undefined && (
            <StatCard icon={Server} label="MCP" value={String(node.mcpCount)} />
          )}
          {node.tokensTodayIn !== undefined && (
            <StatCard icon={Cpu} label="Tokens In" value={node.tokensTodayIn.toLocaleString()} />
          )}
          {node.tokensTodayOut !== undefined && (
            <StatCard icon={Cpu} label="Tokens Out" value={node.tokensTodayOut.toLocaleString()} />
          )}
        </div>
      )}

      {/* Runtime info */}
      {node.runtime && (
        <div className="px-3 py-2 border-b border-cs-border/60">
          <label className="text-[9px] text-cs-muted uppercase tracking-wide">Runtime</label>
          <p className="text-xs font-medium capitalize">{node.runtime}</p>
        </div>
      )}

      {/* Last heartbeat */}
      {node.lastHeartbeat && (
        <div className="px-3 py-2 border-b border-cs-border/60">
          <label className="text-[9px] text-cs-muted uppercase tracking-wide">Last Check</label>
          <p className="text-xs text-cs-muted font-mono">{new Date(node.lastHeartbeat).toLocaleTimeString()}</p>
        </div>
      )}

      {/* Actions */}
      <div className="px-3 py-3 space-y-1.5 mt-auto">
        {node.filePath && onOpenFile && (
          <button
            onClick={() => onOpenFile(node.filePath!)}
            className="flex w-full items-center gap-2 rounded-md border border-cs-border px-2.5 py-1.5 text-xs text-cs-muted transition-colors hover:bg-cs-border/30 hover:text-cs-text"
          >
            <ExternalLink size={12} /> Open in editor
          </button>
        )}
        {node.kind === "runtime" && onOpenProjects && (
          <button
            onClick={onOpenProjects}
            className="flex w-full items-center gap-2 rounded-md border border-cs-border px-2.5 py-1.5 text-xs text-cs-muted transition-colors hover:bg-cs-border/30 hover:text-cs-text"
          >
            <BookOpen size={12} /> View in Projects
          </button>
        )}
      </div>
    </div>
  );
}

function StatCard({ icon: Icon, label, value }: { icon: typeof Cpu; label: string; value: string }) {
  return (
    <div className="rounded-md border border-cs-border/60 bg-cs-bg/40 px-2 py-1.5 text-center">
      <Icon size={10} className="mx-auto text-cs-muted mb-0.5" />
      <p className="text-xs font-medium">{value}</p>
      <p className="text-[8px] text-cs-muted">{label}</p>
    </div>
  );
}
