import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2, LayoutGrid } from "lucide-react";
import { getProjectBundle, listProjects, queryAllAgentStatuses, detectOllama } from "@/lib/api";
import { useWorkspaceStore } from "@/stores/useWorkspaceStore";
import { useProjectStore } from "@/stores/useProjectStore";
import WorkspaceCanvas from "./WorkspaceCanvas";
import WorkspaceToolbar from "./WorkspaceToolbar";
import WorkspaceDetailPanel from "./WorkspaceDetailPanel";

export default function WorkspaceView() {
  const activeProject = useProjectStore((s) => s.activeProject);
  const populateFromBundle = useWorkspaceStore((s) => s.populateFromBundle);
  const nodes = useWorkspaceStore((s) => s.nodes);
  const clear = useWorkspaceStore((s) => s.clear);
  const updateNodeStatus = useWorkspaceStore((s) => s.updateNodeStatus);
  const selectedNodeId = useWorkspaceStore((s) => s.selectedNodeId);
  const selectNode = useWorkspaceStore((s) => s.selectNode);
  const selectedNode = nodes.find((n) => n.id === selectedNodeId) ?? null;

  // Get the active project or first project
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

  // Populate workspace when bundle arrives
  useEffect(() => {
    if (bundle && nodes.length === 0) {
      populateFromBundle(bundle);
    }
  }, [bundle, nodes.length, populateFromBundle]);

  // Clear workspace when project changes
  useEffect(() => {
    clear();
  }, [projectPath, clear]);

  // Live health polling — update runtime node statuses every 5s
  const { data: runtimeStatuses } = useQuery({
    queryKey: ["agent-statuses-workspace"],
    queryFn: queryAllAgentStatuses,
    refetchInterval: 5_000,
    enabled: nodes.length > 0,
    staleTime: 3_000,
  });

  useEffect(() => {
    if (!runtimeStatuses || !Array.isArray(runtimeStatuses)) return;
    for (const rs of runtimeStatuses) {
      const nodeId = `rt-${rs.runtime}`;
      const status = rs.status === "healthy" || rs.status === "connected"
        ? "online"
        : rs.status === "degraded"
        ? "busy"
        : rs.status === "error"
        ? "error"
        : "offline";
      updateNodeStatus(nodeId, status);
    }
  }, [runtimeStatuses, updateNodeStatus]);

  if (!projectPath) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-cs-muted gap-3">
        <LayoutGrid size={48} className="opacity-30" />
        <p className="text-sm">No project selected</p>
        <p className="text-xs">Add a project in the Projects tab to see its workspace</p>
      </div>
    );
  }

  if (isLoading && nodes.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <Loader2 size={24} className="animate-spin text-cs-muted" />
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <WorkspaceToolbar
        onRefresh={() => { clear(); refetch(); }}
        isLoading={isLoading}
        projectName={projectName}
      />
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
