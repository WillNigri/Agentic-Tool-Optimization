import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation } from "@tanstack/react-query";
import {
  X,
  FolderOpen,
  Plus,
  Check,
  Loader2,
  Search,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  addProject,
  discoverProjects,
  type DiscoveredProject,
} from "@/lib/tauri-api";

interface Props {
  discoveredProjects?: DiscoveredProject[];
  onClose: () => void;
  onAdded: () => void;
}

export default function AddProjectModal({ discoveredProjects, onClose, onAdded }: Props) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<"discover" | "manual">(
    discoveredProjects ? "discover" : "manual"
  );
  const [manualPath, setManualPath] = useState("");
  const [manualName, setManualName] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());

  // Discover projects mutation
  const discoverMutation = useMutation({
    mutationFn: discoverProjects,
  });

  // Add project mutation
  const addMutation = useMutation({
    mutationFn: ({ name, path }: { name: string; path: string }) =>
      addProject(name, path),
    onSuccess: () => {
      onAdded();
    },
  });

  // Add multiple projects
  const addMultipleMutation = useMutation({
    mutationFn: async (projects: { name: string; path: string }[]) => {
      for (const p of projects) {
        await addProject(p.name, p.path);
      }
    },
    onSuccess: () => {
      onAdded();
    },
  });

  const discovered = discoveredProjects || discoverMutation.data || [];

  const filteredDiscovered = discovered.filter(
    (p) =>
      p.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      p.path.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const toggleSelect = (path: string) => {
    const newSelected = new Set(selectedPaths);
    if (newSelected.has(path)) {
      newSelected.delete(path);
    } else {
      newSelected.add(path);
    }
    setSelectedPaths(newSelected);
  };

  const selectAll = () => {
    setSelectedPaths(new Set(filteredDiscovered.map((p) => p.path)));
  };

  const deselectAll = () => {
    setSelectedPaths(new Set());
  };

  const handleAddSelected = () => {
    const projects = filteredDiscovered
      .filter((p) => selectedPaths.has(p.path))
      .map((p) => ({ name: p.name, path: p.path }));
    addMultipleMutation.mutate(projects);
  };

  const handleAddManual = () => {
    if (manualPath.trim() && manualName.trim()) {
      addMutation.mutate({ name: manualName.trim(), path: manualPath.trim() });
    }
  };

  const getRuntimeColor = (runtime: string) => {
    switch (runtime) {
      case "claude":
        return "text-orange-400 bg-orange-400/10";
      case "codex":
        return "text-green-400 bg-green-400/10";
      case "hermes":
        return "text-purple-400 bg-purple-400/10";
      case "openclaw":
        return "text-cyan-400 bg-cyan-400/10";
      default:
        return "text-cs-muted bg-cs-border";
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-cs-card border border-cs-border rounded-lg w-full max-w-2xl mx-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border">
          <div className="flex items-center gap-2">
            <FolderOpen size={18} className="text-cs-accent" />
            <h2 className="font-semibold">
              {t("projectManager.addProject", "Add Project")}
            </h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-cs-border transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Mode tabs */}
        <div className="flex border-b border-cs-border">
          <button
            onClick={() => setMode("discover")}
            className={cn(
              "flex-1 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors",
              mode === "discover"
                ? "border-cs-accent text-cs-accent"
                : "border-transparent text-cs-muted hover:text-cs-text"
            )}
          >
            Discover Projects
          </button>
          <button
            onClick={() => setMode("manual")}
            className={cn(
              "flex-1 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors",
              mode === "manual"
                ? "border-cs-accent text-cs-accent"
                : "border-transparent text-cs-muted hover:text-cs-text"
            )}
          >
            Add Manually
          </button>
        </div>

        {/* Content */}
        <div className="p-4">
          {mode === "discover" ? (
            <>
              {/* Search and actions */}
              <div className="flex items-center gap-3 mb-4">
                <div className="flex-1 relative">
                  <Search
                    size={16}
                    className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted"
                  />
                  <input
                    type="text"
                    placeholder="Filter discovered projects..."
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    className="w-full pl-10 pr-4 py-2 rounded-lg border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
                  />
                </div>
                <button
                  onClick={() => discoverMutation.mutate()}
                  disabled={discoverMutation.isPending}
                  className="flex items-center gap-2 px-3 py-2 rounded-lg border border-cs-border text-sm hover:bg-cs-border/50 transition-colors disabled:opacity-50"
                >
                  {discoverMutation.isPending ? (
                    <Loader2 size={14} className="animate-spin" />
                  ) : (
                    <Search size={14} />
                  )}
                  Scan
                </button>
              </div>

              {/* Selection actions */}
              {filteredDiscovered.length > 0 && (
                <div className="flex items-center justify-between mb-3">
                  <span className="text-sm text-cs-muted">
                    {selectedPaths.size} of {filteredDiscovered.length} selected
                  </span>
                  <div className="flex items-center gap-2">
                    <button
                      onClick={selectAll}
                      className="text-xs text-cs-accent hover:underline"
                    >
                      Select all
                    </button>
                    <span className="text-cs-muted">|</span>
                    <button
                      onClick={deselectAll}
                      className="text-xs text-cs-accent hover:underline"
                    >
                      Clear
                    </button>
                  </div>
                </div>
              )}

              {/* Discovered projects list */}
              <div className="max-h-80 overflow-y-auto border border-cs-border rounded-lg">
                {discoverMutation.isPending ? (
                  <div className="flex items-center justify-center py-12">
                    <Loader2 size={24} className="animate-spin text-cs-muted" />
                    <span className="ml-3 text-cs-muted">Scanning for projects...</span>
                  </div>
                ) : filteredDiscovered.length === 0 ? (
                  <div className="flex flex-col items-center justify-center py-12 text-cs-muted">
                    <FolderOpen size={32} className="mb-3 opacity-50" />
                    <p className="text-sm">
                      {discovered.length === 0
                        ? "Click \"Scan\" to discover projects"
                        : "No projects match your filter"}
                    </p>
                  </div>
                ) : (
                  <div className="divide-y divide-cs-border">
                    {filteredDiscovered.map((project) => (
                      <label
                        key={project.path}
                        className={cn(
                          "flex items-start gap-3 p-3 cursor-pointer hover:bg-cs-border/30 transition-colors",
                          selectedPaths.has(project.path) && "bg-cs-accent/5"
                        )}
                      >
                        <input
                          type="checkbox"
                          checked={selectedPaths.has(project.path)}
                          onChange={() => toggleSelect(project.path)}
                          className="mt-1 rounded border-cs-border text-cs-accent focus:ring-cs-accent"
                        />
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2">
                            <span className="font-medium">{project.name}</span>
                            <span className="text-xs text-cs-muted">
                              {project.skillCount} skills
                            </span>
                          </div>
                          <p className="text-xs text-cs-muted truncate mt-0.5">
                            {project.path}
                          </p>
                          <div className="flex flex-wrap gap-1 mt-2">
                            {project.runtimes.map((runtime) => (
                              <span
                                key={runtime}
                                className={cn(
                                  "px-1.5 py-0.5 rounded text-xs capitalize",
                                  getRuntimeColor(runtime)
                                )}
                              >
                                {runtime}
                              </span>
                            ))}
                          </div>
                        </div>
                      </label>
                    ))}
                  </div>
                )}
              </div>
            </>
          ) : (
            /* Manual mode */
            <div className="space-y-4">
              <div>
                <label className="block text-sm font-medium mb-1.5">
                  Project Name
                </label>
                <input
                  type="text"
                  placeholder="My Project"
                  value={manualName}
                  onChange={(e) => setManualName(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
                />
              </div>
              <div>
                <label className="block text-sm font-medium mb-1.5">
                  Project Path
                </label>
                <input
                  type="text"
                  placeholder="/path/to/project"
                  value={manualPath}
                  onChange={(e) => setManualPath(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent font-mono"
                />
                <p className="text-xs text-cs-muted mt-1.5">
                  Enter the full path to the project directory
                </p>
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 px-4 py-3 border-t border-cs-border">
          <button
            onClick={onClose}
            className="px-4 py-2 rounded-md text-sm hover:bg-cs-border transition-colors"
          >
            {t("common.cancel", "Cancel")}
          </button>
          {mode === "discover" ? (
            <button
              onClick={handleAddSelected}
              disabled={selectedPaths.size === 0 || addMultipleMutation.isPending}
              className="flex items-center gap-2 px-4 py-2 rounded-md text-sm bg-cs-accent text-black font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
            >
              {addMultipleMutation.isPending ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <Plus size={14} />
              )}
              Add {selectedPaths.size} Project{selectedPaths.size !== 1 ? "s" : ""}
            </button>
          ) : (
            <button
              onClick={handleAddManual}
              disabled={!manualPath.trim() || !manualName.trim() || addMutation.isPending}
              className="flex items-center gap-2 px-4 py-2 rounded-md text-sm bg-cs-accent text-black font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
            >
              {addMutation.isPending ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <Check size={14} />
              )}
              Add Project
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
