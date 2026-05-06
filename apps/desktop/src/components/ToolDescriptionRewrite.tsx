import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles, Loader2, Copy, Check, AlertCircle } from "lucide-react";
import { promptAgent, type AgentRuntime } from "@/lib/tauri-api";
import { cn } from "@/lib/utils";

// v1.4.0 F8 — Tool description rewrite.
//
// Article-driven feature: tool descriptions matter as much as system prompts.
// Stock MCP descriptions are written for "any caller" and miss the user's
// specific goal. This widget asks the user's runtime to rewrite the
// description with their use-case as context. Suggestion-only — we don't try
// to write back into the MCP server's source.

interface Props {
  toolName: string;
  currentDescription: string;
  /** Default runtime to use for the rewrite. The user can change it. */
  defaultRuntime?: AgentRuntime;
}

const RUNTIMES: AgentRuntime[] = ["claude", "codex", "gemini", "openclaw", "hermes"];

const buildPrompt = (toolName: string, currentDescription: string, useCase: string) => `You are improving an MCP tool description so an LLM agent picks this tool at the right time.

Tool name: ${toolName}
Current description: ${currentDescription || "(none)"}

The agent's specific use-case:
${useCase}

Rewrite the description so the agent reliably selects this tool when (and only when) it fits the use-case above. Constraints:
- One paragraph, under 60 words.
- Lead with WHEN to use it.
- Mention what input the tool needs.
- No marketing language. No "this tool will…". State the function.
- If the existing description is already correct for this use-case, return it unchanged and prefix with "(unchanged) ".

Return only the rewritten description, no preamble.`;

export default function ToolDescriptionRewrite({
  toolName,
  currentDescription,
  defaultRuntime = "claude",
}: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [useCase, setUseCase] = useState("");
  const [runtime, setRuntime] = useState<AgentRuntime>(defaultRuntime);
  const [submitting, setSubmitting] = useState(false);
  const [suggestion, setSuggestion] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const handleGenerate = async () => {
    if (!useCase.trim() || submitting) return;
    setSubmitting(true);
    setError(null);
    setSuggestion(null);
    try {
      const result = await promptAgent(runtime, buildPrompt(toolName, currentDescription, useCase.trim()));
      setSuggestion(result.trim());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  const handleCopy = () => {
    if (!suggestion) return;
    void navigator.clipboard.writeText(suggestion);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  if (!open) {
    return (
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          setOpen(true);
        }}
        className="inline-flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-accent transition-colors"
      >
        <Sparkles size={10} />
        {t("toolRewrite.open", "Improve description")}
      </button>
    );
  }

  return (
    <div
      className="mt-2 rounded-md border border-cs-border bg-cs-bg-raised p-2.5 space-y-2"
      onClick={(e) => e.stopPropagation()}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wide text-cs-muted">
          <Sparkles size={10} />
          {t("toolRewrite.title", "Rewrite for your use-case")}
        </div>
        <button
          type="button"
          onClick={() => {
            setOpen(false);
            setSuggestion(null);
            setUseCase("");
            setError(null);
          }}
          className="text-[10px] text-cs-muted hover:text-cs-text"
        >
          {t("common.close", "Close")}
        </button>
      </div>

      <textarea
        value={useCase}
        onChange={(e) => setUseCase(e.target.value)}
        placeholder={t(
          "toolRewrite.placeholder",
          "What does your agent need this tool for? e.g. 'fetch open PRs for code review'"
        )}
        rows={2}
        className="w-full rounded border border-cs-border bg-cs-bg px-2 py-1.5 text-xs text-cs-text placeholder:text-cs-muted focus:border-cs-accent focus:outline-none"
      />

      <div className="flex items-center gap-2">
        <select
          value={runtime}
          onChange={(e) => setRuntime(e.target.value as AgentRuntime)}
          className="rounded border border-cs-border bg-cs-bg px-2 py-1 text-[10px] text-cs-text focus:border-cs-accent focus:outline-none"
        >
          {RUNTIMES.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
        <button
          type="button"
          onClick={handleGenerate}
          disabled={!useCase.trim() || submitting}
          className={cn(
            "inline-flex items-center gap-1 rounded bg-cs-accent px-2 py-1 text-[10px] font-medium text-cs-bg",
            "hover:bg-cs-accent-hover disabled:opacity-50"
          )}
        >
          {submitting ? <Loader2 size={10} className="animate-spin" /> : <Sparkles size={10} />}
          {submitting
            ? t("toolRewrite.generating", "Generating…")
            : t("toolRewrite.generate", "Suggest")}
        </button>
      </div>

      {error && (
        <div className="flex items-start gap-1.5 rounded border border-cs-danger/40 bg-cs-danger/10 p-1.5">
          <AlertCircle size={10} className="text-cs-danger shrink-0 mt-0.5" />
          <span className="text-[10px] text-cs-text">{error}</span>
        </div>
      )}

      {suggestion && (
        <div className="space-y-1.5">
          <div className="text-[10px] uppercase tracking-wide text-cs-muted">
            {t("toolRewrite.suggestionLabel", "Suggestion")}
          </div>
          <pre className="rounded border border-cs-border bg-cs-bg p-2 text-[11px] text-cs-text whitespace-pre-wrap font-sans">
            {suggestion}
          </pre>
          <button
            type="button"
            onClick={handleCopy}
            className="inline-flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-accent"
          >
            {copied ? <Check size={10} className="text-cs-accent" /> : <Copy size={10} />}
            {copied
              ? t("toolRewrite.copied", "Copied")
              : t("toolRewrite.copy", "Copy to clipboard")}
          </button>
        </div>
      )}
    </div>
  );
}
