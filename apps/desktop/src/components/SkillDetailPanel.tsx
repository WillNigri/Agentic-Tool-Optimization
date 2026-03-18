import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { X, Pencil, Save, FolderOpen, File, Trash2, AlertTriangle, Info, Link2, ExternalLink, Eye, EyeOff, Bot, Zap, Share2, Upload, Sparkles } from "lucide-react";
import { getSkillDetail, getSkills, updateSkill, deleteSkill, type SkillDetail } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ModelTag, StatusTag, TokenTag } from "./SkillMetaTags";
import { analyzeSkillConflicts, getConflictsForSkill } from "@/lib/skill-similarity";
import { shareSkill, promptAgent } from "@/lib/tauri-api";
import PublishSkillModal from "./PublishSkillModal";

// Anthropic official guideline: keep SKILL.md under 500 lines
const RECOMMENDED_MAX_LINES = 500;

// All standard Claude Code tools
const ALL_TOOLS = [
  { name: "Read", icon: "\u{1F4D6}", desc: "Read file contents" },
  { name: "Write", icon: "\u270F\uFE0F", desc: "Create new files" },
  { name: "Edit", icon: "\u270F\uFE0F", desc: "Edit existing files" },
  { name: "Bash", icon: "\u26A1", desc: "Execute shell commands" },
  { name: "Grep", icon: "\u{1F50D}", desc: "Search file contents" },
  { name: "Glob", icon: "\u{1F4C2}", desc: "Find files by pattern" },
  { name: "Agent", icon: "\u{1F916}", desc: "Launch subagents" },
  { name: "WebFetch", icon: "\u{1F310}", desc: "Fetch web content" },
  { name: "WebSearch", icon: "\u{1F50E}", desc: "Search the web" },
  { name: "NotebookEdit", icon: "\u{1F4D3}", desc: "Edit Jupyter notebooks" },
];

// Available substitutions in skill content
const SUBSTITUTIONS = [
  { token: "$ARGUMENTS", desc: "All arguments passed to skill" },
  { token: "$0, $1, $2...", desc: "Individual arguments by index" },
  { token: "${CLAUDE_SKILL_DIR}", desc: "Directory containing SKILL.md" },
  { token: "${CLAUDE_SESSION_ID}", desc: "Current session ID" },
];

interface SkillDetailPanelProps {
  skillId: string;
  onClose: () => void;
}

/** Extract markdown links from content: [text](path) */
function extractLinks(content: string): { text: string; path: string }[] {
  const regex = /\[([^\]]+)\]\(([^)]+)\)/g;
  const links: { text: string; path: string }[] = [];
  let match;
  while ((match = regex.exec(content)) !== null) {
    const path = match[2];
    // Only include relative links (not http/https)
    if (!path.startsWith("http://") && !path.startsWith("https://")) {
      links.push({ text: match[1], path });
    }
  }
  return links;
}

export default function SkillDetailPanel({ skillId, onClose }: SkillDetailPanelProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [editContent, setEditContent] = useState("");
  const [editTools, setEditTools] = useState<string[]>([]);
  const [editAllTools, setEditAllTools] = useState(true);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [showPublish, setShowPublish] = useState(false);
  const [shareUrl, setShareUrl] = useState<string | null>(null);
  const [autoImproveRunning, setAutoImproveRunning] = useState(false);
  const [autoImproveDiff, setAutoImproveDiff] = useState<{ old: string; new: string } | null>(null);

  const { data: skill, isLoading, error } = useQuery({
    queryKey: ["skill-detail", skillId],
    queryFn: () => getSkillDetail(skillId),
    retry: false,
  });

  // Fetch all skills for conflict analysis
  const { data: allSkills = [] } = useQuery({
    queryKey: ["skills"],
    queryFn: getSkills,
  });

  // Analyze conflicts for this skill
  const skillConflicts = useMemo(() => {
    const enabled = allSkills.filter((s) => s.enabled);
    if (enabled.length < 2) return [];
    const allConflicts = analyzeSkillConflicts(enabled);
    return getConflictsForSkill(skillId, allConflicts);
  }, [allSkills, skillId]);

  const saveMutation = useMutation({
    mutationFn: (content: string) => updateSkill(skillId, content),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skill-detail", skillId] });
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      setEditing(false);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: () => deleteSkill(skillId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      onClose();
    },
  });

  // Line count for current content (edit or read)
  const contentToCount = editing ? editContent : skill?.content || "";
  const lineCount = useMemo(() => contentToCount.split("\n").length, [contentToCount]);
  const isOverLimit = lineCount > RECOMMENDED_MAX_LINES;
  const linePercent = Math.min((lineCount / RECOMMENDED_MAX_LINES) * 100, 100);

  // Extract support file links from content
  const supportLinks = useMemo(() => (skill ? extractLinks(skill.content) : []), [skill]);

  function startEdit(skill: SkillDetail) {
    setEditContent(skill.content);
    const hasRestrictions = skill.frontmatter.allowedTools && skill.frontmatter.allowedTools.length > 0;
    setEditAllTools(!hasRestrictions);
    setEditTools(hasRestrictions ? [...skill.frontmatter.allowedTools!] : ALL_TOOLS.map((t) => t.name));
    setEditing(true);
  }

  function toggleEditTool(tool: string) {
    setEditTools((prev) =>
      prev.includes(tool) ? prev.filter((t) => t !== tool) : [...prev, tool]
    );
  }

  if (isLoading) {
    return (
      <Panel onClose={onClose}>
        <div className="animate-pulse space-y-4 p-6">
          <div className="h-6 w-48 bg-cs-border rounded" />
          <div className="h-4 w-64 bg-cs-border rounded" />
          <div className="h-32 bg-cs-border rounded" />
        </div>
      </Panel>
    );
  }

  if (error || !skill) {
    return (
      <Panel onClose={onClose}>
        <div className="p-6 text-center">
          <p className="text-cs-danger text-sm">{t("common.error")}</p>
          <p className="text-cs-muted text-xs mt-1">{String(error)}</p>
        </div>
      </Panel>
    );
  }

  const hasRestrictions = skill.frontmatter.allowedTools && skill.frontmatter.allowedTools.length > 0;
  const allowedSet = new Set(hasRestrictions ? skill.frontmatter.allowedTools : ALL_TOOLS.map((t) => t.name));
  const fm = skill.frontmatter;

  return (
    <Panel onClose={onClose}>
      {/* Header */}
      <div className="flex items-start justify-between p-4 border-b border-cs-border">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            {skill.isDirectory ? (
              <FolderOpen size={18} className="text-cs-accent shrink-0" />
            ) : (
              <File size={18} className="text-cs-muted shrink-0" />
            )}
            <h3 className="text-lg font-semibold truncate">{skill.name}</h3>
            {fm['argument-hint'] && (
              <span className="text-xs text-cs-muted font-mono bg-cs-bg px-1.5 py-0.5 rounded border border-cs-border shrink-0">
                {fm['argument-hint']}
              </span>
            )}
          </div>
          <p className="text-xs text-cs-muted font-mono mt-1 truncate">{skill.filePath}</p>
        </div>
        <div className="flex items-center gap-2 shrink-0 ml-3">
          {!editing && (
            <button
              onClick={() => startEdit(skill)}
              className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
              title={t("skills.edit")}
            >
              <Pencil size={16} />
            </button>
          )}
          <button
            onClick={() => setConfirmDelete(true)}
            className="p-1.5 rounded hover:bg-red-500/10 transition-colors text-cs-muted hover:text-red-400"
            title={t("skills.delete")}
          >
            <Trash2 size={16} />
          </button>
          <button
            onClick={onClose}
            className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      {/* Delete confirmation */}
      {confirmDelete && (
        <div className="mx-4 mt-4 p-3 rounded-lg border border-red-500/30 bg-red-500/5">
          <p className="text-sm text-red-400 mb-2">{t("skills.confirmDelete")}</p>
          <div className="flex gap-2">
            <button onClick={() => deleteMutation.mutate()} className="px-3 py-1 text-xs rounded bg-red-500 text-white hover:bg-red-600 transition-colors">{t("common.delete")}</button>
            <button onClick={() => setConfirmDelete(false)} className="px-3 py-1 text-xs rounded border border-cs-border text-cs-muted hover:text-cs-text transition-colors">{t("common.cancel")}</button>
          </div>
        </div>
      )}

      <div className="p-4 space-y-5 overflow-y-auto flex-1">
        {/* Status badges row */}
        <div className="flex flex-wrap gap-2">
          <StatusTag enabled={skill.enabled} />
          <TokenTag count={skill.tokenCount} />
          {fm.model && <ModelTag model={fm.model} />}
        </div>

        {/* Action buttons: Share, Publish, Auto-improve */}
        <div className="flex items-center gap-2 flex-wrap">
          {/* Share */}
          <button
            onClick={async () => {
              try {
                const result = await shareSkill(skillId, []);
                setShareUrl(result.shareUrl);
                setTimeout(() => setShareUrl(null), 3000);
              } catch { /* handled */ }
            }}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg border border-cs-border text-cs-muted hover:text-cs-accent hover:border-cs-accent/40 transition-colors"
          >
            <Share2 size={12} />
            {shareUrl ? t("marketplace.share.copied") : t("marketplace.share.title")}
          </button>

          {/* Publish */}
          <button
            onClick={() => setShowPublish(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg border border-cs-border text-cs-muted hover:text-purple-400 hover:border-purple-500/40 transition-colors"
          >
            <Upload size={12} />
            {t("marketplace.publish")}
          </button>

          {/* Auto-improve */}
          <button
            onClick={async () => {
              if (autoImproveRunning || !skill) return;
              setAutoImproveRunning(true);
              try {
                const runtime = skill.runtime || "claude";
                const prompt = `You are improving an AI coding agent skill. Analyze this skill and suggest improvements for clarity, specificity, and effectiveness. Return ONLY the improved skill content (full markdown with frontmatter), nothing else.\n\nCurrent skill:\n\`\`\`\n${skill.content}\n\`\`\``;
                const improved = await promptAgent(runtime as "claude" | "codex" | "openclaw" | "hermes", prompt);
                setAutoImproveDiff({ old: skill.content, new: improved });
              } catch {
                // Claude CLI not available
              } finally {
                setAutoImproveRunning(false);
              }
            }}
            disabled={autoImproveRunning}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg border transition-colors",
              autoImproveRunning
                ? "border-yellow-500/40 text-yellow-400 bg-yellow-500/10"
                : "border-cs-border text-cs-muted hover:text-yellow-400 hover:border-yellow-500/40"
            )}
          >
            <Sparkles size={12} />
            {autoImproveRunning ? t("marketplace.autoImprove.running") : t("marketplace.autoImprove.title")}
          </button>
        </div>

        {/* Auto-improve diff view */}
        {autoImproveDiff && (
          <div className="rounded-lg border border-yellow-500/30 bg-yellow-500/5 p-3">
            <h4 className="text-xs font-semibold text-yellow-400 uppercase tracking-wider mb-2">
              {t("marketplace.autoImprove.diffTitle")}
            </h4>
            <pre className="w-full p-3 bg-cs-bg border border-cs-border rounded-lg text-xs font-mono text-cs-text whitespace-pre-wrap max-h-48 overflow-y-auto mb-3">
              {autoImproveDiff.new}
            </pre>
            <div className="flex gap-2">
              <button
                onClick={() => {
                  saveMutation.mutate(autoImproveDiff.new);
                  setAutoImproveDiff(null);
                }}
                className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors"
              >
                <Save size={12} />
                {t("marketplace.autoImprove.apply")}
              </button>
              <button
                onClick={() => setAutoImproveDiff(null)}
                className="px-3 py-1.5 text-xs rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
              >
                {t("marketplace.autoImprove.discard")}
              </button>
            </div>
          </div>
        )}

        {/* Description — THE trigger for auto-loading */}
        <div>
          <div className="flex items-center gap-2 mb-1">
            <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider">
              {t("skills.description")}
            </h4>
            <span className="flex items-center gap-1 text-[10px] text-cs-accent bg-cs-accent/10 px-1.5 py-0.5 rounded-full border border-cs-accent/20">
              <Zap size={9} />
              {t("skills.detail.triggerField")}
            </span>
          </div>
          <p className="text-sm text-cs-text bg-cs-bg rounded-lg p-2.5 border border-cs-border">
            {fm.description || skill.description}
          </p>
          <p className="text-[10px] text-cs-muted mt-1 italic">
            {t("skills.detail.triggerHint")}
          </p>
        </div>

        {/* Conflict warnings for this skill */}
        {skillConflicts.length > 0 && (
          <div className={cn(
            "rounded-lg border p-3",
            skillConflicts.some((c) => c.severity === "high")
              ? "border-red-500/30 bg-red-500/5"
              : "border-yellow-500/30 bg-yellow-500/5"
          )}>
            <div className="flex items-center gap-2 mb-2">
              <AlertTriangle size={13} className={skillConflicts.some((c) => c.severity === "high") ? "text-red-400" : "text-yellow-400"} />
              <span className="text-xs font-medium text-cs-text">
                {t("skills.conflicts.similarSkills", { count: skillConflicts.length })}
              </span>
            </div>
            <div className="space-y-2">
              {skillConflicts.map((conflict, i) => {
                const otherSkill = conflict.skillA.id === skillId ? conflict.skillB : conflict.skillA;
                return (
                  <div key={i} className="rounded-md bg-cs-bg/50 border border-cs-border p-2">
                    <div className="flex items-center gap-2 mb-1">
                      <span className={cn(
                        "text-[9px] font-bold uppercase px-1.5 py-0.5 rounded",
                        conflict.severity === "high" ? "bg-red-500/15 text-red-400" : "bg-yellow-500/15 text-yellow-400"
                      )}>
                        {conflict.similarity}% {t("skills.conflicts.match")}
                      </span>
                      <span className="text-xs text-cs-accent font-medium">{otherSkill.name}</span>
                    </div>
                    <div className="flex flex-wrap gap-1 mb-1">
                      {conflict.sharedKeywords.slice(0, 5).map((kw) => (
                        <span key={kw} className="text-[9px] font-mono px-1.5 py-0.5 rounded bg-cs-border/60 text-cs-muted">{kw}</span>
                      ))}
                    </div>
                    <p className="text-[10px] text-cs-muted italic">{conflict.suggestion}</p>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* Frontmatter config tags — official Anthropic fields */}
        <div>
          <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-2">
            {t("skills.detail.config")}
          </h4>
          <div className="grid grid-cols-2 gap-1.5">
            {/* user-invocable */}
            <div className={cn(
              "flex items-center gap-2 px-2.5 py-2 rounded-lg border",
              fm['user-invocable'] !== false
                ? "border-cs-accent/20 bg-cs-accent/5"
                : "border-cs-border bg-cs-bg"
            )}>
              {fm['user-invocable'] !== false ? <Eye size={13} className="text-cs-accent shrink-0" /> : <EyeOff size={13} className="text-cs-muted/40 shrink-0" />}
              <div>
                <p className={cn("text-xs font-mono", fm['user-invocable'] !== false ? "text-cs-accent" : "text-cs-muted/40")}>user-invocable</p>
                <p className="text-[10px] text-cs-muted">{fm['user-invocable'] !== false ? t("skills.detail.visibleSlash") : t("skills.detail.hiddenSlash")}</p>
              </div>
            </div>

            {/* disable-model-invocation */}
            <div className={cn(
              "flex items-center gap-2 px-2.5 py-2 rounded-lg border",
              fm['disable-model-invocation']
                ? "border-yellow-500/20 bg-yellow-500/5"
                : "border-cs-accent/20 bg-cs-accent/5"
            )}>
              {fm['disable-model-invocation'] ? <EyeOff size={13} className="text-yellow-400 shrink-0" /> : <Bot size={13} className="text-cs-accent shrink-0" />}
              <div>
                <p className={cn("text-xs font-mono", fm['disable-model-invocation'] ? "text-yellow-400" : "text-cs-accent")}>auto-invoke</p>
                <p className="text-[10px] text-cs-muted">{fm['disable-model-invocation'] ? t("skills.detail.manualOnly") : t("skills.detail.autoInvoke")}</p>
              </div>
            </div>

            {/* context fork */}
            {fm.context && (
              <div className="flex items-center gap-2 px-2.5 py-2 rounded-lg border border-purple-500/20 bg-purple-500/5">
                <Bot size={13} className="text-purple-400 shrink-0" />
                <div>
                  <p className="text-xs font-mono text-purple-400">context: fork</p>
                  <p className="text-[10px] text-cs-muted">{fm.agent ? `Agent: ${fm.agent}` : "Isolated subagent"}</p>
                </div>
              </div>
            )}

            {/* argument-hint */}
            {fm['argument-hint'] && (
              <div className="flex items-center gap-2 px-2.5 py-2 rounded-lg border border-blue-500/20 bg-blue-500/5">
                <Info size={13} className="text-blue-400 shrink-0" />
                <div>
                  <p className="text-xs font-mono text-blue-400">argument-hint</p>
                  <p className="text-[10px] text-cs-muted font-mono">{fm['argument-hint']}</p>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Allowed Tools */}
        <div>
          <div className="flex items-center justify-between mb-2">
            <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider">
              {t("skills.detail.allowedTools")}
            </h4>
            {!hasRestrictions && !editing && (
              <span className="text-[10px] text-cs-muted italic">{t("skills.detail.allToolsDefault")}</span>
            )}
          </div>

          {editing ? (
            <div className="space-y-2">
              <label className="flex items-center gap-2 text-xs text-cs-muted cursor-pointer">
                <input type="checkbox" checked={editAllTools} onChange={(e) => { setEditAllTools(e.target.checked); if (e.target.checked) setEditTools(ALL_TOOLS.map((t) => t.name)); }} className="accent-[#00FFB2]" />
                {t("skills.detail.allowAll")}
              </label>
              <div className="grid grid-cols-2 gap-1.5">
                {ALL_TOOLS.map((tool) => {
                  const isEnabled = editTools.includes(tool.name);
                  return (
                    <button key={tool.name} type="button" onClick={() => { if (!editAllTools) toggleEditTool(tool.name); }} disabled={editAllTools}
                      className={cn("flex items-center gap-2 px-2.5 py-2 rounded-lg border text-left transition-all",
                        editAllTools ? "border-cs-accent/30 bg-cs-accent/5 text-cs-accent opacity-80"
                          : isEnabled ? "border-cs-accent/40 bg-cs-accent/10 text-cs-accent"
                          : "border-cs-border bg-cs-bg text-cs-muted/40"
                      )}>
                      <span className="text-sm">{tool.icon}</span>
                      <div className="min-w-0">
                        <p className="text-xs font-mono font-medium">{tool.name}</p>
                        <p className={cn("text-[10px] truncate", (editAllTools || isEnabled) ? "text-cs-muted" : "text-cs-muted/30")}>({tool.desc})</p>
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-1.5">
              {ALL_TOOLS.map((tool) => {
                const isAllowed = allowedSet.has(tool.name);
                return (
                  <div key={tool.name} className={cn("flex items-center gap-2 px-2.5 py-2 rounded-lg border transition-all", isAllowed ? "border-cs-accent/30 bg-cs-accent/5" : "border-cs-border/50 bg-cs-bg")}>
                    <span className={cn("text-sm", !isAllowed && "opacity-30")}>{tool.icon}</span>
                    <div className="min-w-0">
                      <p className={cn("text-xs font-mono font-medium", isAllowed ? "text-cs-accent" : "text-cs-muted/30")}>{tool.name}</p>
                      <p className={cn("text-[10px] truncate", isAllowed ? "text-cs-muted" : "text-cs-muted/20")}>({tool.desc})</p>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* Content with line counter */}
        <div>
          <div className="flex items-center justify-between mb-1">
            <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider">
              {t("skills.content")}
            </h4>
          </div>

          {/* Line counter bar */}
          <div className="mb-2">
            <div className="flex items-center justify-between mb-1">
              <span className={cn("text-xs font-mono", isOverLimit ? "text-cs-danger" : lineCount > RECOMMENDED_MAX_LINES * 0.8 ? "text-cs-warning" : "text-cs-muted")}>
                {lineCount} / {RECOMMENDED_MAX_LINES} {t("skills.detail.lines")}
              </span>
              {isOverLimit && (
                <span className="flex items-center gap-1 text-[10px] text-cs-danger">
                  <AlertTriangle size={10} />
                  {t("skills.detail.overLimit")}
                </span>
              )}
            </div>
            <div className="w-full h-1.5 bg-cs-bg rounded-full overflow-hidden">
              <div
                className={cn(
                  "h-full rounded-full transition-all duration-300",
                  isOverLimit ? "bg-cs-danger" : lineCount > RECOMMENDED_MAX_LINES * 0.8 ? "bg-cs-warning" : "bg-cs-accent"
                )}
                style={{ width: `${linePercent}%` }}
              />
            </div>
            {isOverLimit && (
              <p className="text-[10px] text-cs-danger/80 mt-1">
                {t("skills.detail.overLimitHint")}
              </p>
            )}
          </div>

          {editing ? (
            <div className="space-y-2">
              <textarea
                className={cn(
                  "w-full h-64 p-3 bg-cs-bg border rounded-lg text-sm font-mono text-cs-text resize-y focus:outline-none",
                  isOverLimit ? "border-cs-danger/50 focus:border-cs-danger" : "border-cs-border focus:border-cs-accent"
                )}
                value={editContent}
                onChange={(e) => setEditContent(e.target.value)}
              />
              <div className="flex gap-2">
                <button onClick={() => saveMutation.mutate(editContent)} disabled={saveMutation.isPending}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50">
                  <Save size={14} />{t("common.save")}
                </button>
                <button onClick={() => setEditing(false)} className="px-3 py-1.5 text-xs rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors">
                  {t("common.cancel")}
                </button>
              </div>
            </div>
          ) : (
            <pre className={cn(
              "w-full p-3 bg-cs-bg border rounded-lg text-sm font-mono text-cs-text whitespace-pre-wrap overflow-x-auto max-h-64 overflow-y-auto",
              isOverLimit ? "border-cs-danger/30" : "border-cs-border"
            )}>
              {skill.content}
            </pre>
          )}
        </div>

        {/* Support file links extracted from content */}
        {supportLinks.length > 0 && (
          <div>
            <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-2">
              {t("skills.detail.linkedFiles")}
            </h4>
            <div className="space-y-1">
              {supportLinks.map((link, i) => (
                <div key={i} className="flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-cs-bg border border-cs-border">
                  <Link2 size={12} className="text-cs-accent shrink-0" />
                  <span className="text-xs font-mono text-cs-accent truncate">{link.path}</span>
                  <span className="text-[10px] text-cs-muted truncate ml-auto">{link.text}</span>
                </div>
              ))}
            </div>
            <p className="text-[10px] text-cs-muted mt-1 italic">
              {t("skills.detail.linkedFilesHint")}
            </p>
          </div>
        )}

        {/* Substitutions reference (in edit mode) */}
        {editing && (
          <div>
            <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-2">
              {t("skills.detail.substitutions")}
            </h4>
            <div className="rounded-lg bg-cs-bg border border-cs-border p-2.5 space-y-1.5">
              {SUBSTITUTIONS.map((sub) => (
                <div key={sub.token} className="flex items-center gap-2">
                  <code className="text-[11px] font-mono text-cs-accent bg-cs-accent/10 px-1.5 py-0.5 rounded shrink-0">{sub.token}</code>
                  <span className="text-[10px] text-cs-muted">{sub.desc}</span>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Directories */}
        {(skill.isDirectory || skill.hasScripts || skill.hasReferences || skill.hasAssets) && (
          <div className="space-y-3">
            <DirectorySection icon={"\u{1F4C1}"} label={t("skills.detail.scripts")} files={skill.scripts} exists={skill.hasScripts} />
            <DirectorySection icon={"\u{1F4C1}"} label={t("skills.detail.references")} files={skill.references} exists={skill.hasReferences} />
            <DirectorySection icon={"\u{1F4C1}"} label={t("skills.detail.assets")} files={skill.assets} exists={skill.hasAssets} />
          </div>
        )}
      </div>

      {/* Publish modal */}
      {showPublish && (
        <PublishSkillModal
          skillId={skillId}
          skillName={skill.name}
          onClose={() => setShowPublish(false)}
        />
      )}
    </Panel>
  );
}

function Panel({ children, onClose }: { children: React.ReactNode; onClose: () => void }) {
  return (
    <>
      <div className="fixed inset-0 bg-black/30 z-40 lg:hidden" onClick={onClose} />
      <div className="fixed right-0 top-0 bottom-0 w-full lg:w-1/2 max-w-2xl bg-cs-card border-l border-cs-border z-50 flex flex-col shadow-2xl animate-slide-in-right">
        {children}
      </div>
    </>
  );
}

function DirectorySection({ icon, label, files, exists }: { icon: string; label: string; files: string[]; exists: boolean }) {
  if (!exists) return null;
  return (
    <div className="rounded-lg border border-cs-border p-3">
      <div className="flex items-center justify-between mb-2">
        <span className="text-sm font-medium flex items-center gap-1.5">
          <span>{icon}</span> {label}
        </span>
      </div>
      {files.length > 0 ? (
        <ul className="space-y-1">
          {files.map((file) => (
            <li key={file} className="text-xs text-cs-muted font-mono pl-5 flex items-center gap-1.5">
              <File size={12} className="shrink-0" />{file}
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-xs text-cs-muted pl-5 italic">(empty)</p>
      )}
    </div>
  );
}
