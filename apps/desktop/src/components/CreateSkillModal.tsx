import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { X, Sparkles, Loader2 } from "lucide-react";
import { createSkill, type CreateSkillData } from "@/lib/api";
import type { SkillScope } from "@/lib/tauri-api";
import { promptAgent } from "@/lib/tauri-api";
import type { AgentRuntime } from "@/components/cron/types";

const AVAILABLE_TOOLS = ["Read", "Write", "Edit", "Bash", "Grep", "Glob", "Agent"];
const AVAILABLE_MODELS = ["claude-sonnet-4-5", "claude-opus-4-5", "claude-haiku-4-5"];

const SCOPE_OPTIONS: { value: SkillScope; path: string }[] = [
  { value: "enterprise", path: "/etc/claude/skills/" },
  { value: "personal", path: "~/.claude/skills/" },
  { value: "project", path: ".claude/skills/" },
  { value: "plugin", path: "~/.claude/plugins/" },
];

interface CreateSkillModalProps {
  onClose: () => void;
}

export default function CreateSkillModal({ onClose }: CreateSkillModalProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [scope, setScope] = useState<SkillScope>("personal");
  const [content, setContent] = useState("");
  const [isDirectory, setIsDirectory] = useState(false);
  const [selectedTools, setSelectedTools] = useState<string[]>([]);
  const [model, setModel] = useState("");
  const [aiPrompt, setAiPrompt] = useState("");
  const [aiGenerating, setAiGenerating] = useState(false);
  const [aiRuntime, setAiRuntime] = useState<AgentRuntime>("claude");
  const [aiError, setAiError] = useState<string | null>(null);

  const createMutation = useMutation({
    mutationFn: (data: CreateSkillData) => createSkill(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      onClose();
    },
  });

  function toggleTool(tool: string) {
    setSelectedTools((prev) =>
      prev.includes(tool) ? prev.filter((t) => t !== tool) : [...prev, tool]
    );
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;

    const frontmatterLines = [
      "---",
      `name: ${name}`,
      `description: ${description}`,
    ];
    if (selectedTools.length > 0) {
      frontmatterLines.push(`allowed-tools: [${selectedTools.join(", ")}]`);
    }
    if (model) {
      frontmatterLines.push(`model: ${model}`);
    }
    frontmatterLines.push("---", "");

    const fullContent = frontmatterLines.join("\n") + (content || `# ${name}\n\n`);

    createMutation.mutate({
      name: name.trim(),
      description: description.trim(),
      scope,
      content: fullContent,
      allowedTools: selectedTools.length > 0 ? selectedTools : undefined,
      model: model || undefined,
      isDirectory,
    });
  }

  return (
    <>
      {/* Backdrop */}
      <div className="fixed inset-0 bg-black/50 z-50" onClick={onClose} />

      {/* Modal */}
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <div
          className="bg-cs-card border border-cs-border rounded-xl w-full max-w-lg max-h-[90vh] overflow-y-auto shadow-2xl"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className="flex items-center justify-between p-4 border-b border-cs-border">
            <h3 className="text-lg font-semibold">{t("skills.createNew")}</h3>
            <button
              onClick={onClose}
              className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
            >
              <X size={16} />
            </button>
          </div>

          <form onSubmit={handleSubmit} className="p-4 space-y-4">
            {/* Name */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("skills.name")}
              </label>
              <input
                type="text"
                className="input"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="my-skill"
                required
              />
            </div>

            {/* Description */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("skills.description")}
              </label>
              <input
                type="text"
                className="input"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder={t("skills.create.descriptionPlaceholder")}
              />
            </div>

            {/* Scope */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("skills.source")}
              </label>
              <div className="grid grid-cols-2 gap-2">
                {SCOPE_OPTIONS.map((opt) => (
                  <button
                    key={opt.value}
                    type="button"
                    onClick={() => setScope(opt.value)}
                    className={`px-3 py-2 text-sm rounded-lg border transition-colors text-left ${
                      scope === opt.value
                        ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                        : "border-cs-border text-cs-muted hover:text-cs-text"
                    }`}
                  >
                    {t(`skills.scopes.${opt.value}`)}
                    <span className="block text-xs opacity-60 mt-0.5">{opt.path}</span>
                  </button>
                ))}
              </div>
            </div>

            {/* Template type */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("skills.create.template")}
              </label>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => setIsDirectory(false)}
                  className={`flex-1 px-3 py-2 text-sm rounded-lg border transition-colors ${
                    !isDirectory
                      ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                      : "border-cs-border text-cs-muted hover:text-cs-text"
                  }`}
                >
                  {t("skills.create.singleFile")}
                </button>
                <button
                  type="button"
                  onClick={() => setIsDirectory(true)}
                  className={`flex-1 px-3 py-2 text-sm rounded-lg border transition-colors ${
                    isDirectory
                      ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                      : "border-cs-border text-cs-muted hover:text-cs-text"
                  }`}
                >
                  {t("skills.create.directory")}
                </button>
              </div>
              {isDirectory && (
                <p className="text-xs text-cs-muted mt-1.5">
                  {t("skills.create.directoryHint")}
                </p>
              )}
            </div>

            {/* Allowed Tools */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("skills.detail.allowedTools")}
              </label>
              <div className="flex flex-wrap gap-2">
                {AVAILABLE_TOOLS.map((tool) => (
                  <button
                    key={tool}
                    type="button"
                    onClick={() => toggleTool(tool)}
                    className={`px-2.5 py-1 text-xs font-mono rounded-full border transition-colors ${
                      selectedTools.includes(tool)
                        ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                        : "border-cs-border text-cs-muted hover:text-cs-text"
                    }`}
                  >
                    {tool}
                  </button>
                ))}
              </div>
            </div>

            {/* Model */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("skills.detail.model")}
              </label>
              <select
                className="input"
                value={model}
                onChange={(e) => setModel(e.target.value)}
              >
                <option value="">{t("skills.create.anyModel")}</option>
                {AVAILABLE_MODELS.map((m) => (
                  <option key={m} value={m}>{m}</option>
                ))}
              </select>
            </div>

            {/* AI Generate */}
            <div className="rounded-lg border border-cs-accent/20 bg-cs-accent/5 p-3">
              <div className="flex items-center gap-2 mb-2">
                <Sparkles size={14} className="text-cs-accent" />
                <label className="text-xs font-semibold text-cs-accent uppercase tracking-wider">
                  {t("skills.create.aiGenerate")}
                </label>
              </div>
              <p className="text-[10px] text-cs-muted mb-2">
                {t("skills.create.aiGenerateHint")}
              </p>
              <textarea
                className="w-full h-16 p-2.5 bg-cs-bg border border-cs-border rounded-lg text-sm text-cs-text resize-none focus:outline-none focus:border-cs-accent mb-2"
                value={aiPrompt}
                onChange={(e) => setAiPrompt(e.target.value)}
                placeholder={t("skills.create.aiPromptPlaceholder")}
              />
              {aiError && (
                <p className="text-[11px] text-red-400 mb-2">{aiError}</p>
              )}
              <button
                type="button"
                disabled={!aiPrompt.trim() || aiGenerating}
                onClick={async () => {
                  setAiGenerating(true);
                  setAiError(null);
                  try {
                    const result = await promptAgent(aiRuntime, `You are creating a Claude Code skill file. Based on this description, generate a complete SKILL.md file with proper YAML frontmatter (name, description, allowed-tools if needed) and detailed markdown instructions. Return ONLY the file content, no explanation.\n\nSkill name: ${name || "unnamed"}\nDescription: ${description || aiPrompt}\nUser request: ${aiPrompt}`);
                    setContent(result.trim());
                    // Try to extract name/description from generated frontmatter
                    const nameMatch = result.match(/^name:\s*(.+)$/m);
                    const descMatch = result.match(/^description:\s*(.+)$/m);
                    if (nameMatch && !name) setName(nameMatch[1].trim());
                    if (descMatch && !description) setDescription(descMatch[1].trim());
                  } catch (err) {
                    setAiError(err instanceof Error ? err.message : String(err));
                  } finally {
                    setAiGenerating(false);
                  }
                }}
                className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
              >
                {aiGenerating ? (
                  <>
                    <Loader2 size={12} className="animate-spin" />
                    {t("skills.create.aiGenerating")}
                  </>
                ) : (
                  <>
                    <Sparkles size={12} />
                    {t("skills.create.aiGenerateButton")}
                  </>
                )}
              </button>
            </div>

            {/* Content */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("skills.content")}
              </label>
              <textarea
                className="w-full h-32 p-3 bg-cs-bg border border-cs-border rounded-lg text-sm font-mono text-cs-text resize-y focus:outline-none focus:border-cs-accent"
                value={content}
                onChange={(e) => setContent(e.target.value)}
                placeholder={`# ${name || "My Skill"}\n\nDescribe the skill behavior...`}
              />
            </div>

            {/* Actions */}
            <div className="flex gap-2 pt-2">
              <button
                type="submit"
                disabled={!name.trim() || createMutation.isPending}
                className="flex-1 px-4 py-2 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
              >
                {t("common.create")}
              </button>
              <button
                type="button"
                onClick={onClose}
                className="px-4 py-2 text-sm rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
              >
                {t("common.cancel")}
              </button>
            </div>
          </form>
        </div>
      </div>
    </>
  );
}
