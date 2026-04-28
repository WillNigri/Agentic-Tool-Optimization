import { Workflow, LayoutGrid, RefreshCw, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useWorkspaceStore } from "@/stores/useWorkspaceStore";

interface WorkspaceToolbarProps {
  onRefresh?: () => void;
  isLoading?: boolean;
  projectName?: string;
}

export default function WorkspaceToolbar({ onRefresh, isLoading, projectName }: WorkspaceToolbarProps) {
  const mode = useWorkspaceStore((s) => s.mode);
  const setMode = useWorkspaceStore((s) => s.setMode);
  const nodeCount = useWorkspaceStore((s) => s.nodes.filter((n) => !n.hidden).length);

  return (
    <div className="flex items-center justify-between border-b border-cs-border bg-cs-card px-4 py-2">
      <div className="flex items-center gap-3">
        {/* Mode switcher */}
        <div className="flex rounded-lg border border-cs-border/60 overflow-hidden">
          <button
            onClick={() => setMode("workspace")}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium transition-colors",
              mode === "workspace"
                ? "bg-cs-accent/10 text-cs-accent"
                : "text-cs-muted hover:text-cs-text hover:bg-cs-border/30"
            )}
          >
            <LayoutGrid size={13} />
            Workspace
          </button>
          <button
            onClick={() => setMode("workflows")}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium transition-colors border-l border-cs-border/60",
              mode === "workflows"
                ? "bg-cs-accent/10 text-cs-accent"
                : "text-cs-muted hover:text-cs-text hover:bg-cs-border/30"
            )}
          >
            <Workflow size={13} />
            Workflows
          </button>
        </div>

        {mode === "workspace" && (
          <span className="text-[10px] text-cs-muted">
            {projectName && <span className="font-medium text-cs-text mr-1">{projectName}</span>}
            {nodeCount} nodes
          </span>
        )}
      </div>

      {mode === "workspace" && onRefresh && (
        <button
          onClick={onRefresh}
          disabled={isLoading}
          className="p-1.5 rounded text-cs-muted hover:text-cs-text hover:bg-cs-border/30 disabled:opacity-50"
        >
          {isLoading ? <Loader2 size={14} className="animate-spin" /> : <RefreshCw size={14} />}
        </button>
      )}
    </div>
  );
}
