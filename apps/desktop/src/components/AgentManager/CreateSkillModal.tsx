import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation } from "@tanstack/react-query";
import { X, Sparkles, AlertCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  createAgentSkill,
  type AgentConfigRuntime,
  type AgentConfigScope,
} from "@/lib/tauri-api";

interface Props {
  onClose: () => void;
  onCreated: (path: string) => void;
}

const RUNTIMES: { value: AgentConfigRuntime; label: string }[] = [
  { value: "claude", label: "Claude" },
  { value: "codex", label: "Codex" },
  { value: "openclaw", label: "OpenClaw" },
  { value: "hermes", label: "Hermes" },
];

// Scope paths vary by runtime (per official docs)
const SCOPE_HINTS: Record<AgentConfigRuntime, { global: string; project: string }> = {
  claude: { global: "~/.claude/skills/", project: ".claude/skills/" },
  codex: { global: "~/.agents/skills/", project: ".agents/skills/" },
  hermes: { global: "~/.hermes/skills/", project: ".hermes/skills/" },
  openclaw: { global: "~/.openclaw/skills/", project: "skills/" },
  shared: { global: "~/.agents/skills/", project: ".agents/skills/" },
};

const SCOPES: { value: AgentConfigScope; label: string }[] = [
  { value: "global", label: "Global (Personal)" },
  { value: "project", label: "Project" },
];

export default function CreateSkillModal({ onClose, onCreated }: Props) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [runtime, setRuntime] = useState<AgentConfigRuntime>("claude");
  const [scope, setScope] = useState<AgentConfigScope>("global");

  const createMutation = useMutation({
    mutationFn: () => createAgentSkill(runtime, name, scope, description),
    onSuccess: (path) => {
      onCreated(path);
    },
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (name.trim()) {
      createMutation.mutate();
    }
  };

  const scopeHints = SCOPE_HINTS[runtime];
  const scopeHint = scope === "global" ? scopeHints.global : scopeHints.project;
  const skillPath = name
    ? `${scopeHint}${name.toLowerCase().replace(/\s+/g, "-")}/SKILL.md`
    : "";

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-cs-card border border-cs-border rounded-lg w-full max-w-md mx-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border">
          <div className="flex items-center gap-2">
            <Sparkles size={18} className="text-cs-accent" />
            <h2 className="font-semibold">
              {t("agentManager.createSkill.title", "Create New Skill")}
            </h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-cs-border transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="p-4 space-y-4">
          {/* Name */}
          <div>
            <label className="block text-sm font-medium mb-1.5">
              {t("agentManager.createSkill.name", "Skill Name")}
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("agentManager.createSkill.namePlaceholder", "my-skill")}
              className="w-full px-3 py-2 bg-cs-card border border-cs-border rounded-md text-sm focus:outline-none focus:border-cs-accent"
              autoFocus
            />
          </div>

          {/* Description */}
          <div>
            <label className="block text-sm font-medium mb-1.5">
              {t("agentManager.createSkill.description", "Description")}
            </label>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t(
                "agentManager.createSkill.descriptionPlaceholder",
                "What does this skill do?"
              )}
              rows={3}
              className="w-full px-3 py-2 bg-cs-card border border-cs-border rounded-md text-sm focus:outline-none focus:border-cs-accent resize-none"
            />
          </div>

          {/* Runtime */}
          <div>
            <label className="block text-sm font-medium mb-1.5">
              {t("agentManager.createSkill.runtime", "Runtime")}
            </label>
            <div className="flex flex-wrap gap-2">
              {RUNTIMES.map((r) => (
                <button
                  key={r.value}
                  type="button"
                  onClick={() => setRuntime(r.value)}
                  className={cn(
                    "px-3 py-1.5 rounded-md text-sm border transition-colors",
                    runtime === r.value
                      ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                      : "border-cs-border hover:border-cs-muted"
                  )}
                >
                  {r.label}
                </button>
              ))}
            </div>
          </div>

          {/* Scope */}
          <div>
            <label className="block text-sm font-medium mb-1.5">
              {t("agentManager.createSkill.scope", "Scope")}
            </label>
            <div className="flex gap-2">
              {SCOPES.map((s) => {
                const hint = s.value === "global"
                  ? scopeHints.global
                  : scopeHints.project;
                return (
                  <button
                    key={s.value}
                    type="button"
                    onClick={() => setScope(s.value)}
                    className={cn(
                      "flex-1 px-3 py-2 rounded-md text-sm border transition-colors text-left",
                      scope === s.value
                        ? "border-cs-accent bg-cs-accent/10"
                        : "border-cs-border hover:border-cs-muted"
                    )}
                  >
                    <div className={scope === s.value ? "text-cs-accent" : ""}>
                      {s.label}
                    </div>
                    <div className="text-xs text-cs-muted mt-0.5">{hint}</div>
                  </button>
                );
              })}
            </div>
          </div>

          {/* Preview path */}
          {name && (
            <div className="text-sm">
              <span className="text-cs-muted">
                {t("agentManager.createSkill.willCreate", "Will create:")}
              </span>
              <code className="ml-2 px-2 py-0.5 bg-cs-border rounded text-xs">
                {skillPath}
              </code>
            </div>
          )}

          {/* Error */}
          {createMutation.isError && (
            <div className="flex items-center gap-2 text-sm text-red-400">
              <AlertCircle size={14} />
              <span>
                {createMutation.error instanceof Error
                  ? createMutation.error.message
                  : t("common.error", "An error occurred")}
              </span>
            </div>
          )}

          {/* Actions */}
          <div className="flex items-center justify-end gap-2 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 rounded-md text-sm text-cs-muted hover:text-cs-text transition-colors"
            >
              {t("common.cancel", "Cancel")}
            </button>
            <button
              type="submit"
              disabled={!name.trim() || createMutation.isPending}
              className={cn(
                "px-4 py-2 rounded-md text-sm font-medium transition-colors",
                name.trim()
                  ? "bg-cs-accent text-black hover:bg-cs-accent/90"
                  : "bg-cs-border text-cs-muted cursor-not-allowed"
              )}
            >
              {createMutation.isPending
                ? t("common.creating", "Creating...")
                : t("common.create", "Create")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
