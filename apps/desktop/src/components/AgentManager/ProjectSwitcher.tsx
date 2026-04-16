import { useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  ChevronDown,
  FolderOpen,
  Check,
  Plus,
  Settings,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  listProjects,
  getActiveProject,
  setActiveProject,
  type Project,
} from "@/lib/api";

interface Props {
  onManageProjects: () => void;
  onAddProject: () => void;
}

export default function ProjectSwitcher({ onManageProjects, onAddProject }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // Fetch projects
  const { data: projects = [], isLoading: projectsLoading } = useQuery<Project[]>({
    queryKey: ["projects"],
    queryFn: listProjects,
  });

  // Fetch active project
  const { data: activeProject, isLoading: activeLoading } = useQuery<Project | null>({
    queryKey: ["active-project"],
    queryFn: getActiveProject,
  });

  // Set active project mutation
  const setActiveMutation = useMutation({
    mutationFn: setActiveProject,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      queryClient.invalidateQueries({ queryKey: ["active-project"] });
      setIsOpen(false);
    },
  });

  // Close dropdown on outside click
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setIsOpen(false);
      }
    };

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
    }

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [isOpen]);

  const isLoading = projectsLoading || activeLoading;

  const getRuntimeDots = (project: Project) => {
    const dots = [];
    if (project.hasClaude) dots.push("bg-orange-400");
    if (project.hasCodex) dots.push("bg-green-400");
    if (project.hasHermes) dots.push("bg-purple-400");
    if (project.hasOpenclaw) dots.push("bg-cyan-400");
    return dots;
  };

  return (
    <div className="relative" ref={dropdownRef}>
      <button
        onClick={() => setIsOpen(!isOpen)}
        disabled={isLoading}
        className={cn(
          "flex items-center gap-2 px-3 py-1.5 rounded-md border text-sm transition-colors min-w-[160px]",
          isOpen
            ? "border-cs-accent bg-cs-accent/5"
            : "border-cs-border hover:border-cs-accent/50"
        )}
      >
        {isLoading ? (
          <Loader2 size={14} className="animate-spin text-cs-muted" />
        ) : (
          <FolderOpen size={14} className="text-cs-muted" />
        )}
        <span className="flex-1 text-left truncate">
          {activeProject?.name || "No Project"}
        </span>
        <ChevronDown
          size={14}
          className={cn(
            "text-cs-muted transition-transform",
            isOpen && "rotate-180"
          )}
        />
      </button>

      {/* Dropdown */}
      {isOpen && (
        <div className="absolute top-full left-0 mt-1 w-64 bg-cs-card border border-cs-border rounded-lg shadow-lg z-50 overflow-hidden">
          {/* Projects list */}
          <div className="max-h-64 overflow-y-auto">
            {projects.length === 0 ? (
              <div className="px-3 py-6 text-center text-cs-muted">
                <FolderOpen size={24} className="mx-auto mb-2 opacity-50" />
                <p className="text-sm">No projects yet</p>
              </div>
            ) : (
              projects.map((project) => (
                <button
                  key={project.id}
                  onClick={() => setActiveMutation.mutate(project.id)}
                  disabled={setActiveMutation.isPending}
                  className={cn(
                    "w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-cs-border/50 transition-colors",
                    project.isActive && "bg-cs-accent/5"
                  )}
                >
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium truncate">
                        {project.name}
                      </span>
                      {/* Runtime dots */}
                      <div className="flex items-center gap-0.5">
                        {getRuntimeDots(project).map((color, i) => (
                          <span
                            key={i}
                            className={cn("w-1.5 h-1.5 rounded-full", color)}
                          />
                        ))}
                      </div>
                    </div>
                    <p className="text-xs text-cs-muted truncate">
                      {project.skillCount} skills
                    </p>
                  </div>
                  {project.isActive && (
                    <Check size={14} className="text-cs-accent shrink-0" />
                  )}
                </button>
              ))
            )}
          </div>

          {/* Actions */}
          <div className="border-t border-cs-border">
            <button
              onClick={() => {
                setIsOpen(false);
                onAddProject();
              }}
              className="w-full flex items-center gap-2 px-3 py-2 text-sm text-cs-muted hover:text-cs-text hover:bg-cs-border/50 transition-colors"
            >
              <Plus size={14} />
              Add Project
            </button>
            <button
              onClick={() => {
                setIsOpen(false);
                onManageProjects();
              }}
              className="w-full flex items-center gap-2 px-3 py-2 text-sm text-cs-muted hover:text-cs-text hover:bg-cs-border/50 transition-colors"
            >
              <Settings size={14} />
              Manage Projects
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
