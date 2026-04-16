import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation } from "@tanstack/react-query";
import {
  X,
  Copy,
  Loader2,
  Check,
  Search,
  AlertTriangle,
  FolderOpen,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  listProjects,
  getProjectSkills,
  cloneSkill,
  type Project,
  type AgentConfigRuntime,
} from "@/lib/api";

interface Props {
  targetProject: Project;
  onClose: () => void;
  onCloned: () => void;
}

interface SkillInfo {
  path: string;
  name: string;
  runtime: string;
}

export default function CloneSkillModal({ targetProject, onClose, onCloned }: Props) {
  const { t } = useTranslation();
  const [selectedSourceProject, setSelectedSourceProject] = useState<string>("");
  const [selectedSkill, setSelectedSkill] = useState<SkillInfo | null>(null);
  const [targetRuntime, setTargetRuntime] = useState<AgentConfigRuntime>("claude");
  const [searchQuery, setSearchQuery] = useState("");

  // Fetch all projects for source selection
  const { data: projects = [] } = useQuery<Project[]>({
    queryKey: ["projects"],
    queryFn: listProjects,
  });

  // Filter out target project from sources
  const sourceProjects = projects.filter((p) => p.id !== targetProject.id);

  // Fetch skills for selected source project
  const { data: sourceSkills = [], isLoading: skillsLoading } = useQuery<SkillInfo[]>({
    queryKey: ["project-skills", selectedSourceProject],
    queryFn: () => getProjectSkills(selectedSourceProject),
    enabled: !!selectedSourceProject,
  });

  // Clone mutation
  const cloneMutation = useMutation({
    mutationFn: async () => {
      if (!selectedSkill) throw new Error("No skill selected");
      return cloneSkill(selectedSkill.path, targetProject.path, targetRuntime);
    },
    onSuccess: () => {
      onCloned();
    },
  });

  // Auto-select first source project
  useEffect(() => {
    if (sourceProjects.length > 0 && !selectedSourceProject) {
      setSelectedSourceProject(sourceProjects[0].id);
    }
  }, [sourceProjects, selectedSourceProject]);

  // Filter skills
  const filteredSkills = sourceSkills.filter(
    (skill) =>
      skill.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      skill.path.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const getRuntimeColor = (runtime: string) => {
    switch (runtime) {
      case "claude":
        return "text-orange-400 bg-orange-400/10 border-orange-400/30";
      case "codex":
        return "text-green-400 bg-green-400/10 border-green-400/30";
      case "hermes":
        return "text-purple-400 bg-purple-400/10 border-purple-400/30";
      case "openclaw":
        return "text-cyan-400 bg-cyan-400/10 border-cyan-400/30";
      default:
        return "text-cs-muted bg-cs-border border-cs-border";
    }
  };

  const RUNTIME_OPTIONS: { value: AgentConfigRuntime; label: string; color: string }[] = [
    { value: "claude", label: "Claude", color: "text-orange-400" },
    { value: "codex", label: "Codex", color: "text-green-400" },
    { value: "hermes", label: "Hermes", color: "text-purple-400" },
    { value: "openclaw", label: "OpenClaw", color: "text-cyan-400" },
  ];

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-cs-card border border-cs-border rounded-lg w-full max-w-xl mx-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border">
          <div className="flex items-center gap-2">
            <Copy size={18} className="text-cs-accent" />
            <h2 className="font-semibold">
              {t("projectManager.cloneSkill", "Clone Skill")}
            </h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-cs-border transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Content */}
        <div className="p-4 space-y-4">
          {/* Target project info */}
          <div className="p-3 rounded-lg border border-cs-border bg-cs-bg">
            <p className="text-xs text-cs-muted mb-1">Clone to:</p>
            <p className="font-medium">{targetProject.name}</p>
            <p className="text-xs text-cs-muted truncate">{targetProject.path}</p>
          </div>

          {/* Source project selection */}
          <div>
            <label className="block text-sm font-medium mb-1.5">
              Source Project
            </label>
            <select
              value={selectedSourceProject}
              onChange={(e) => {
                setSelectedSourceProject(e.target.value);
                setSelectedSkill(null);
              }}
              className="w-full px-3 py-2 rounded-lg border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
            >
              {sourceProjects.length === 0 ? (
                <option value="">No other projects available</option>
              ) : (
                sourceProjects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} ({p.skillCount} skills)
                  </option>
                ))
              )}
            </select>
          </div>

          {/* Skill selection */}
          <div>
            <label className="block text-sm font-medium mb-1.5">
              Select Skill to Clone
            </label>

            {/* Search */}
            <div className="relative mb-2">
              <Search
                size={14}
                className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted"
              />
              <input
                type="text"
                placeholder="Filter skills..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full pl-9 pr-4 py-1.5 rounded-lg border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
              />
            </div>

            {/* Skills list */}
            <div className="max-h-48 overflow-y-auto border border-cs-border rounded-lg">
              {skillsLoading ? (
                <div className="flex items-center justify-center py-8">
                  <Loader2 size={20} className="animate-spin text-cs-muted" />
                </div>
              ) : filteredSkills.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-8 text-cs-muted">
                  <FolderOpen size={24} className="mb-2 opacity-50" />
                  <p className="text-xs">
                    {sourceSkills.length === 0
                      ? "No skills in this project"
                      : "No skills match your filter"}
                  </p>
                </div>
              ) : (
                <div className="divide-y divide-cs-border">
                  {filteredSkills.map((skill) => (
                    <label
                      key={skill.path}
                      className={cn(
                        "flex items-center gap-3 p-2.5 cursor-pointer hover:bg-cs-border/30 transition-colors",
                        selectedSkill?.path === skill.path && "bg-cs-accent/5"
                      )}
                    >
                      <input
                        type="radio"
                        name="skill"
                        checked={selectedSkill?.path === skill.path}
                        onChange={() => setSelectedSkill(skill)}
                        className="rounded-full border-cs-border text-cs-accent focus:ring-cs-accent"
                      />
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="font-medium text-sm">{skill.name}</span>
                          <span
                            className={cn(
                              "px-1.5 py-0.5 rounded text-xs capitalize border",
                              getRuntimeColor(skill.runtime)
                            )}
                          >
                            {skill.runtime}
                          </span>
                        </div>
                        <p className="text-xs text-cs-muted truncate mt-0.5">
                          {skill.path}
                        </p>
                      </div>
                    </label>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* Target runtime */}
          <div>
            <label className="block text-sm font-medium mb-1.5">
              Target Runtime Format
            </label>
            <div className="flex flex-wrap gap-2">
              {RUNTIME_OPTIONS.map((opt) => (
                <button
                  key={opt.value}
                  onClick={() => setTargetRuntime(opt.value)}
                  className={cn(
                    "px-3 py-1.5 rounded-lg border text-sm font-medium transition-colors",
                    targetRuntime === opt.value
                      ? `${getRuntimeColor(opt.value)} border-current`
                      : "border-cs-border hover:border-cs-accent/50"
                  )}
                >
                  {opt.label}
                </button>
              ))}
            </div>
            <p className="text-xs text-cs-muted mt-1.5">
              Skill will be converted to the selected runtime's format
            </p>
          </div>

          {/* Warning if converting */}
          {selectedSkill && selectedSkill.runtime !== targetRuntime && (
            <div className="flex items-start gap-2 p-3 rounded-lg bg-yellow-500/10 border border-yellow-500/30">
              <AlertTriangle size={16} className="text-yellow-500 mt-0.5 shrink-0" />
              <p className="text-xs text-yellow-500">
                Converting from {selectedSkill.runtime} to {targetRuntime}. Some features may not transfer perfectly.
              </p>
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
          <button
            onClick={() => cloneMutation.mutate()}
            disabled={!selectedSkill || cloneMutation.isPending}
            className="flex items-center gap-2 px-4 py-2 rounded-md text-sm bg-cs-accent text-black font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
          >
            {cloneMutation.isPending ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <Check size={14} />
            )}
            Clone Skill
          </button>
        </div>

        {/* Error */}
        {cloneMutation.isError && (
          <div className="px-4 pb-3">
            <p className="text-xs text-red-400">
              {cloneMutation.error instanceof Error
                ? cloneMutation.error.message
                : "Failed to clone skill"}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
