import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2, LayoutGrid } from "lucide-react";
import { getProjectBundle, listProjects } from "@/lib/api";
import { useWorkspaceStore } from "@/stores/useWorkspaceStore";
import { useProjectStore } from "@/stores/useProjectStore";
import WorkspaceCanvas from "./WorkspaceCanvas";
import WorkspaceDetailPanel from "./WorkspaceDetailPanel";

export default function WorkspaceView() {
  const activeProject = useProjectStore((s) => s.activeProject);
  const populateFromBundle = useWorkspaceStore((s) => s.populateFromBundle);
  const nodes = useWorkspaceStore((s) => s.nodes);
  const clear = useWorkspaceStore((s) => s.clear);
  const selectedNodeId = useWorkspaceStore((s) => s.selectedNodeId);
  const selectNode = useWorkspaceStore((s) => s.selectNode);
  const selectedNode = nodes.find((n) => n.id === selectedNodeId) ?? null;

  const { data: projects = [] } = useQuery({
    queryKey: ["projects"],
    queryFn: listProjects,
    staleTime: 30_000,
  });

  const projectPath = activeProject?.path ?? projects[0]?.path;
  const projectName = activeProject?.name ?? projects[0]?.name;

  const { data: bundle, isLoading, refetch } = useQuery({
    queryKey: ["project-bundle", projectPath],
    queryFn: () => getProjectBundle(projectPath!),
    enabled: !!projectPath,
    staleTime: 10_000,
  });

  useEffect(() => {
    if (bundle && nodes.length === 0) {
      populateFromBundle(bundle);
    }
  }, [bundle, nodes.length, populateFromBundle]);

  useEffect(() => {
    clear();
  }, [projectPath, clear]);

  if (!projectPath) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center bg-cs-bg p-6">
        <LayoutGrid size={48} className="text-cs-accent mb-4 opacity-30" />
        <h2 className="text-lg font-semibold mb-2">Agent Workspace</h2>
        <p className="text-sm text-cs-muted text-center max-w-md">
          Select a project in the Projects tab to populate the workspace with your runtimes, skills, and MCP servers.
        </p>
      </div>
    );
  }

  if (isLoading && nodes.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center bg-cs-bg">
        <Loader2 size={24} className="animate-spin text-cs-muted" />
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-[#0a0a0f]">
      <div className="flex items-center gap-2 px-4 py-1.5 border-b border-cs-border/60 bg-[#111118] shrink-0">
        {projectName && <span className="text-xs font-medium text-cs-text">{projectName}</span>}
        <span className="text-[10px] text-cs-muted">{nodes.filter((n) => !n.hidden).length} nodes</span>
        <div className="flex-1" />
        <button
          onClick={() => { clear(); refetch(); }}
          className="text-[10px] text-cs-muted hover:text-cs-text px-2 py-0.5 rounded hover:bg-cs-border/30"
        >
          Refresh
        </button>
      </div>
      <div className="flex flex-1 overflow-hidden">
        <WorkspaceCanvas />
        {selectedNode && (
          <WorkspaceDetailPanel
            node={selectedNode}
            onClose={() => selectNode(null)}
          />
        )}
      </div>
    </div>
  );
}
