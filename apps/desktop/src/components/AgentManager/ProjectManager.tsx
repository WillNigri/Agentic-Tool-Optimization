import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  FolderOpen,
  Plus,
  RefreshCw,
  Trash2,
  Edit2,
  Check,
  X,
  Search,
  Loader2,
  AlertTriangle,
  Star,
  Copy,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  listProjects,
  discoverProjects,
  deleteProject,
  updateProject,
  setActiveProject,
  type Project,
  type DiscoveredProject,
} from "@/lib/tauri-api";
import AddProjectModal from "./AddProjectModal";
import CloneSkillModal from "./CloneSkillModal";

export default function ProjectManager() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [searchQuery, setSearchQuery] = useState("");
  const [showAddProject, setShowAddProject] = useState(false);
  const [showCloneSkill, setShowCloneSkill] = useState(false);
  const [editingProject, setEditingProject] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [selectedProject, setSelectedProject] = useState<Project | null>(null);

  // Fetch projects
  const {
    data: projects = [],
    isLoading,
    isError,
    error,
    refetch,
    isFetching,
  } = useQuery<Project[]>({
    queryKey: ["projects"],
    queryFn: listProjects,
    retry: 1,
    staleTime: 30000,
  });

  // Discover projects mutation
  const discoverMutation = useMutation({
    mutationFn: discoverProjects,
    onSuccess: () => {
      setShowAddProject(true);
    },
  });

  // Delete project mutation
  const deleteMutation = useMutation({
    mutationFn: deleteProject,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
    },
  });

  // Update project mutation
  const updateMutation = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) =>
      updateProject(id, name, undefined),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      setEditingProject(null);
    },
  });

  // Set active project mutation
  const setActiveMutation = useMutation({
    mutationFn: setActiveProject,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
    },
  });

  const filteredProjects = projects.filter(
    (p) =>
      p.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      p.path.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const handleStartEdit = (project: Project) => {
    setEditingProject(project.id);
    setEditName(project.name);
  };

  const handleSaveEdit = (projectId: string) => {
    if (editName.trim()) {
      updateMutation.mutate({ id: projectId, name: editName.trim() });
    }
  };

  const handleCancelEdit = () => {
    setEditingProject(null);
    setEditName("");
  };

  const getRuntimeBadges = (project: Project) => {
    const badges = [];
    if (project.hasClaude) badges.push({ name: "Claude", color: "text-orange-400 bg-orange-400/10" });
    if (project.hasCodex) badges.push({ name: "Codex", color: "text-green-400 bg-green-400/10" });
    if (project.hasHermes) badges.push({ name: "Hermes", color: "text-purple-400 bg-purple-400/10" });
    if (project.hasOpenclaw) badges.push({ name: "OpenClaw", color: "text-cyan-400 bg-cyan-400/10" });
    return badges;
  };

  if (isLoading) {
    return (
      <div className="h-full flex flex-col items-center justify-center bg-cs-bg p-6">
        <Loader2 size={32} className="animate-spin text-cs-accent mb-4" />
        <p className="text-cs-muted">Loading projects...</p>
      </div>
    );
  }

  if (isError) {
    return (
      <div className="h-full flex flex-col items-center justify-center bg-cs-bg p-6">
        <AlertTriangle size={48} className="mb-4 text-yellow-500" />
        <h2 className="text-lg font-semibold mb-2">Failed to load projects</h2>
        <p className="text-sm text-cs-muted mb-4 max-w-md text-center">
          {error instanceof Error ? error.message : "Unknown error occurred"}
        </p>
        <button
          onClick={() => refetch()}
          className="px-4 py-2 rounded-md bg-cs-accent text-black text-sm font-medium hover:bg-cs-accent/90 transition-colors"
        >
          Retry
        </button>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-cs-bg">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-semibold">
            {t("projectManager.title", "Project Manager")}
          </h1>
          <p className="text-sm text-cs-muted mt-1">
            {t("projectManager.subtitle", "Manage projects and their agent configurations")}
          </p>
        </div>

        <div className="flex items-center gap-3">
          {/* Discover projects */}
          <button
            onClick={() => discoverMutation.mutate()}
            disabled={discoverMutation.isPending}
            className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border text-sm hover:bg-cs-border/50 transition-colors disabled:opacity-50"
          >
            {discoverMutation.isPending ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <Search size={14} />
            )}
            Discover Projects
          </button>

          {/* Refresh */}
          <button
            onClick={() => refetch()}
            disabled={isFetching}
            className="p-2 rounded-md border border-cs-border hover:bg-cs-border/50 transition-colors disabled:opacity-50"
            title={t("common.refresh", "Refresh")}
          >
            <RefreshCw size={16} className={isFetching ? "animate-spin" : ""} />
          </button>

          {/* Add project */}
          <button
            onClick={() => setShowAddProject(true)}
            className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-cs-accent text-black text-sm font-medium hover:bg-cs-accent/90 transition-colors"
          >
            <Plus size={16} />
            Add Project
          </button>
        </div>
      </div>

      {/* Search */}
      <div className="relative mb-4">
        <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted" />
        <input
          type="text"
          placeholder="Search projects..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="w-full pl-10 pr-4 py-2 rounded-lg border border-cs-border bg-cs-card text-sm focus:outline-none focus:border-cs-accent"
        />
      </div>

      {/* Projects list */}
      <div className="flex-1 overflow-y-auto">
        {filteredProjects.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-cs-muted">
            <FolderOpen size={48} className="mb-4 opacity-50" />
            <p className="text-lg mb-2">No projects found</p>
            <p className="text-sm">
              {searchQuery
                ? "Try a different search term"
                : "Click \"Discover Projects\" to find projects on your machine"}
            </p>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {filteredProjects.map((project) => (
              <div
                key={project.id}
                className={cn(
                  "border rounded-lg p-4 transition-colors",
                  project.isActive
                    ? "border-cs-accent bg-cs-accent/5"
                    : "border-cs-border bg-cs-card hover:border-cs-accent/50"
                )}
              >
                {/* Header */}
                <div className="flex items-start justify-between mb-3">
                  <div className="flex-1 min-w-0">
                    {editingProject === project.id ? (
                      <div className="flex items-center gap-2">
                        <input
                          type="text"
                          value={editName}
                          onChange={(e) => setEditName(e.target.value)}
                          className="flex-1 px-2 py-1 rounded border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
                          autoFocus
                          onKeyDown={(e) => {
                            if (e.key === "Enter") handleSaveEdit(project.id);
                            if (e.key === "Escape") handleCancelEdit();
                          }}
                        />
                        <button
                          onClick={() => handleSaveEdit(project.id)}
                          className="p-1 rounded hover:bg-cs-border text-green-400"
                        >
                          <Check size={14} />
                        </button>
                        <button
                          onClick={handleCancelEdit}
                          className="p-1 rounded hover:bg-cs-border text-red-400"
                        >
                          <X size={14} />
                        </button>
                      </div>
                    ) : (
                      <div className="flex items-center gap-2">
                        <h3 className="font-medium truncate">{project.name}</h3>
                        {project.isActive && (
                          <Star size={14} className="text-cs-accent shrink-0" fill="currentColor" />
                        )}
                      </div>
                    )}
                    <p className="text-xs text-cs-muted truncate mt-1" title={project.path}>
                      {project.path}
                    </p>
                  </div>

                  {/* Actions */}
                  {editingProject !== project.id && (
                    <div className="flex items-center gap-1 ml-2">
                      <button
                        onClick={() => handleStartEdit(project)}
                        className="p-1.5 rounded hover:bg-cs-border transition-colors"
                        title="Rename"
                      >
                        <Edit2 size={14} />
                      </button>
                      <button
                        onClick={() => deleteMutation.mutate(project.id)}
                        disabled={deleteMutation.isPending}
                        className="p-1.5 rounded hover:bg-cs-border transition-colors text-red-400"
                        title="Remove"
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  )}
                </div>

                {/* Runtime badges */}
                <div className="flex flex-wrap gap-1.5 mb-3">
                  {getRuntimeBadges(project).map((badge) => (
                    <span
                      key={badge.name}
                      className={cn("px-2 py-0.5 rounded text-xs font-medium", badge.color)}
                    >
                      {badge.name}
                    </span>
                  ))}
                  {getRuntimeBadges(project).length === 0 && (
                    <span className="px-2 py-0.5 rounded text-xs font-medium text-cs-muted bg-cs-border">
                      No configs
                    </span>
                  )}
                </div>

                {/* Stats */}
                <div className="flex items-center justify-between text-sm">
                  <span className="text-cs-muted">
                    {project.skillCount} {project.skillCount === 1 ? "skill" : "skills"}
                  </span>
                  {project.lastAccessed && (
                    <span className="text-xs text-cs-muted">
                      Last: {new Date(project.lastAccessed).toLocaleDateString()}
                    </span>
                  )}
                </div>

                {/* Actions */}
                <div className="flex items-center gap-2 mt-3 pt-3 border-t border-cs-border">
                  {!project.isActive && (
                    <button
                      onClick={() => setActiveMutation.mutate(project.id)}
                      disabled={setActiveMutation.isPending}
                      className="flex-1 px-3 py-1.5 rounded text-xs font-medium bg-cs-accent/10 text-cs-accent hover:bg-cs-accent/20 transition-colors"
                    >
                      Set Active
                    </button>
                  )}
                  <button
                    onClick={() => {
                      setSelectedProject(project);
                      setShowCloneSkill(true);
                    }}
                    className="flex-1 flex items-center justify-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium border border-cs-border hover:bg-cs-border/50 transition-colors"
                  >
                    <Copy size={12} />
                    Clone Skill
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Modals */}
      {showAddProject && (
        <AddProjectModal
          discoveredProjects={discoverMutation.data}
          onClose={() => setShowAddProject(false)}
          onAdded={() => {
            setShowAddProject(false);
            queryClient.invalidateQueries({ queryKey: ["projects"] });
          }}
        />
      )}

      {showCloneSkill && selectedProject && (
        <CloneSkillModal
          targetProject={selectedProject}
          onClose={() => {
            setShowCloneSkill(false);
            setSelectedProject(null);
          }}
          onCloned={() => {
            setShowCloneSkill(false);
            setSelectedProject(null);
            queryClient.invalidateQueries({ queryKey: ["projects"] });
          }}
        />
      )}
    </div>
  );
}
