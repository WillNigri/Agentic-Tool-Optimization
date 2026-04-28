import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2, LayoutGrid } from "lucide-react";
import { getProjectBundle, listProjects } from "@/lib/api";
import { useWorkspaceStore } from "@/stores/useWorkspaceStore";
import { useProjectStore } from "@/stores/useProjectStore";
import WorkspaceCanvas from "./WorkspaceCanvas";
import WorkspaceToolbar from "./WorkspaceToolbar";

export default function WorkspaceView() {
  const activeProject = useProjectStore((s) => s.activeProject);
  const populateFromBundle = useWorkspaceStore((s) => s.populateFromBundle);
  const nodes = useWorkspaceStore((s) => s.nodes);
  const clear = useWorkspaceStore((s) => s.clear);

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
      <WorkspaceCanvas />
    </div>
  );
}
