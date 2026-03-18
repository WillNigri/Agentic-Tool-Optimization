import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Shield, Save, X, FileText, Check, AlertTriangle } from "lucide-react";
import { cn } from "@/lib/utils";
import { createSkill } from "@/lib/api";
import type { AgentRuntime } from "@/components/cron/types";

interface ApprovalDialogProps {
  /** The full response text from the agent */
  content: string;
  /** File path suggestion (extracted from response) */
  filePath: string;
  /** Skill name (extracted) */
  skillName: string;
  /** Which runtime generated this */
  runtime: AgentRuntime;
  onApprove: () => void;
  onDeny: () => void;
}

/**
 * Extract a skill file from an agent response.
 * Looks for markdown frontmatter blocks (---...---) that look like SKILL.md content.
 */
export function extractSkillFromResponse(response: string): {
  content: string;
  name: string;
  path: string;
} | null {
  // Pattern 1: code block with frontmatter
  const codeBlockMatch = response.match(/```(?:markdown|md)?\n(---[\s\S]*?---[\s\S]*?)```/);
  if (codeBlockMatch) {
    const content = codeBlockMatch[1].trim();
    const nameMatch = content.match(/^name:\s*(.+)$/m);
    const name = nameMatch ? nameMatch[1].trim() : "generated-skill";
    return { content, name, path: `~/.claude/skills/${name}.md` };
  }

  // Pattern 2: raw frontmatter at start of response
  const rawMatch = response.match(/^(---\n[\s\S]*?---\n[\s\S]+)/);
  if (rawMatch) {
    const content = rawMatch[1].trim();
    const nameMatch = content.match(/^name:\s*(.+)$/m);
    const name = nameMatch ? nameMatch[1].trim() : "generated-skill";
    return { content, name, path: `~/.claude/skills/${name}.md` };
  }

  // Pattern 3: mentions saving to a file path
  const pathMatch = response.match(/`(~?\/?[\w./~-]*skills\/[\w-]+\.md)`/);
  if (pathMatch) {
    // Find the content block near the path mention
    const afterPath = response.slice(response.indexOf(pathMatch[0]));
    const blockMatch = afterPath.match(/```(?:markdown|md)?\n([\s\S]*?)```/);
    if (blockMatch) {
      const content = blockMatch[1].trim();
      const name = pathMatch[1].split("/").pop()?.replace(".md", "") || "generated-skill";
      return { content, name, path: pathMatch[1] };
    }
  }

  return null;
}

export default function ApprovalDialog({
  content,
  filePath,
  skillName,
  runtime,
  onApprove,
  onDeny,
}: ApprovalDialogProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [scope, setScope] = useState<"personal" | "project">("personal");

  async function handleApprove() {
    setSaving(true);
    setError(null);
    try {
      await createSkill({
        name: skillName,
        description: "",
        scope,
        runtime,
        content,
        isDirectory: false,
      });
      queryClient.invalidateQueries({ queryKey: ["skills"] });
      setSaved(true);
      setTimeout(onApprove, 1000);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }

  if (saved) {
    return (
      <div className="rounded-lg border border-green-500/30 bg-green-500/5 p-3 my-2">
        <div className="flex items-center gap-2 text-green-400 text-xs">
          <Check size={14} />
          <span className="font-medium">Skill "{skillName}" saved to {scope === "personal" ? "~/" : "."}{runtime === "claude" ? ".claude" : `.${runtime}`}/skills/</span>
        </div>
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-yellow-500/30 bg-yellow-500/5 p-3 my-2">
      {/* Header */}
      <div className="flex items-center gap-2 mb-2">
        <Shield size={14} className="text-yellow-400" />
        <span className="text-xs font-semibold text-yellow-400 uppercase tracking-wider">
          Approval Required
        </span>
      </div>

      <p className="text-xs text-cs-muted mb-2">
        The agent wants to create a skill file. Review the content and approve to save it.
      </p>

      {/* File info */}
      <div className="flex items-center gap-2 mb-2 px-2 py-1.5 rounded bg-cs-bg border border-cs-border">
        <FileText size={12} className="text-cs-accent shrink-0" />
        <span className="text-xs font-mono text-cs-accent truncate">{skillName}.md</span>
        <span className="text-[10px] text-cs-muted ml-auto">{content.split("\n").length} lines</span>
      </div>

      {/* Content preview */}
      <pre className="w-full p-2.5 bg-cs-bg border border-cs-border rounded-lg text-[11px] font-mono text-cs-text whitespace-pre-wrap max-h-32 overflow-y-auto mb-3">
        {content.slice(0, 500)}{content.length > 500 ? "\n..." : ""}
      </pre>

      {/* Scope selector */}
      <div className="flex items-center gap-2 mb-3">
        <span className="text-[10px] text-cs-muted">Save to:</span>
        <button
          onClick={() => setScope("personal")}
          className={cn(
            "px-2 py-0.5 text-[10px] rounded border transition-colors",
            scope === "personal"
              ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
              : "border-cs-border text-cs-muted"
          )}
        >
          Personal (~/)
        </button>
        <button
          onClick={() => setScope("project")}
          className={cn(
            "px-2 py-0.5 text-[10px] rounded border transition-colors",
            scope === "project"
              ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
              : "border-cs-border text-cs-muted"
          )}
        >
          Project (./)
        </button>
      </div>

      {error && (
        <div className="flex items-center gap-1.5 text-[11px] text-red-400 mb-2">
          <AlertTriangle size={10} />
          {error}
        </div>
      )}

      {/* Actions */}
      <div className="flex gap-2">
        <button
          onClick={handleApprove}
          disabled={saving}
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
        >
          <Save size={12} />
          {saving ? "Saving..." : "Approve & Save"}
        </button>
        <button
          onClick={onDeny}
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
        >
          <X size={12} />
          Dismiss
        </button>
      </div>
    </div>
  );
}
