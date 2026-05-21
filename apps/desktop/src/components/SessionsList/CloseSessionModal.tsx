import { useState, useMemo } from "react";
import { Lock, X, Loader2, Sparkles } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { listLlmApiKeys, type LlmApiKey } from "@/lib/tauri-api";
import { cn } from "@/lib/utils";

// v2.7.12 — Pre-close modal. Lets the user pick which LLM runtime
// summarizes the session AND attach a free-form human note that lives
// in the summary card alongside the coordinator's output.
//
// Why a separate modal (vs inline pickers in the header): the old close
// button fired close_session synchronously with whatever defaults the
// backend picked. Users had no surface for "summarize this with claude
// instead of minimax" or "add my own framing of what we just decided"
// — both promised by the CLI flags (--coordinator, --human-comment) but
// invisible from the UI. The modal makes both first-class.
//
// Coordinator picker: populated from listLlmApiKeys() so users only see
// providers they can actually dispatch to. Empty list → modal renders
// the field as a disabled "(no API keys configured)" hint and still
// lets the user submit; the backend falls through to its default
// resolution chain (session agent_slug → anchor runtime → first key).

// API-provider slugs that the close-summarizer can dispatch to. Mirrors
// crate::api_dispatch::registry() on the Rust side. Any provider in this
// set that ALSO has a configured key shows up in the picker.
const SUPPORTED_COORDINATORS: { slug: string; label: string }[] = [
  { slug: "anthropic", label: "Anthropic (Claude API)" },
  { slug: "google", label: "Google (Gemini API)" },
  { slug: "minimax", label: "MiniMax" },
  { slug: "grok", label: "Grok (xAI)" },
  { slug: "deepseek", label: "DeepSeek" },
  { slug: "qwen", label: "Qwen" },
  { slug: "openrouter", label: "OpenRouter" },
];

interface Props {
  open: boolean;
  onCancel: () => void;
  /** Called with the user's choices. Caller invokes close_session and
   *  shows the existing "Coordinator is summarizing…" blocker. */
  onSubmit: (opts: { coordinator: string | null; humanComment: string | null }) => void;
  /** Disables the Submit button while the parent is mid-dispatch. */
  busy?: boolean;
}

export default function CloseSessionModal({ open, onCancel, onSubmit, busy = false }: Props) {
  const [coordinator, setCoordinator] = useState<string>("");
  const [humanComment, setHumanComment] = useState<string>("");

  const { data: apiKeys = [] } = useQuery<LlmApiKey[]>({
    queryKey: ["llm-api-keys"],
    queryFn: () => listLlmApiKeys(),
    enabled: open,
    staleTime: 60_000,
  });

  const availableCoordinators = useMemo(() => {
    const configured = new Set(apiKeys.map((k) => k.provider));
    return SUPPORTED_COORDINATORS.filter((c) => configured.has(c.slug));
  }, [apiKeys]);

  if (!open) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const c = coordinator.trim();
    const t = humanComment.trim();
    onSubmit({
      coordinator: c.length > 0 ? c : null,
      humanComment: t.length > 0 ? t : null,
    });
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-cs-bg/80 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-labelledby="close-session-title"
      onClick={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <form
        onSubmit={handleSubmit}
        className="border border-cs-border bg-cs-card rounded-lg p-5 max-w-lg w-full mx-4 space-y-4"
      >
        <div className="flex items-start gap-3">
          <Sparkles size={18} className="text-cs-accent mt-0.5 shrink-0" />
          <div className="flex-1 min-w-0">
            <h2 id="close-session-title" className="text-sm font-semibold text-cs-text">
              Close session
            </h2>
            <p className="text-xs text-cs-muted mt-1 leading-relaxed">
              The coordinator will read the conversation and produce a
              title, summary, topic tags, and category. Pick who summarizes
              — and add any framing of your own that should travel with
              the summary.
            </p>
          </div>
          <button
            type="button"
            onClick={onCancel}
            aria-label="Cancel"
            className="text-cs-muted hover:text-cs-text"
          >
            <X size={16} />
          </button>
        </div>

        <div className="space-y-1.5">
          <label
            htmlFor="close-coordinator-select"
            className="block text-[11px] uppercase tracking-wider font-medium text-cs-muted"
          >
            Coordinator
          </label>
          <select
            id="close-coordinator-select"
            value={coordinator}
            onChange={(e) => setCoordinator(e.target.value)}
            disabled={busy || availableCoordinators.length === 0}
            className="w-full rounded-md border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none disabled:opacity-50"
          >
            <option value="">
              {availableCoordinators.length === 0
                ? "(no API keys configured — backend default will be used)"
                : "Default (auto-pick from session agent → anchor → first key)"}
            </option>
            {availableCoordinators.map((c) => (
              <option key={c.slug} value={c.slug}>
                {c.label}
              </option>
            ))}
          </select>
          {availableCoordinators.length === 0 && (
            <p className="text-[10px] text-cs-muted">
              Add a provider key in Settings → API Keys to enable the picker.
            </p>
          )}
        </div>

        <div className="space-y-1.5">
          <label
            htmlFor="close-human-comment"
            className="block text-[11px] uppercase tracking-wider font-medium text-cs-muted"
          >
            Your note (optional)
          </label>
          <textarea
            id="close-human-comment"
            value={humanComment}
            onChange={(e) => setHumanComment(e.target.value)}
            disabled={busy}
            rows={4}
            maxLength={4096}
            placeholder="e.g. 'We agreed to ship the migration toast first; revisit war-room close after v2.7.12.'"
            className="w-full rounded-md border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none disabled:opacity-50"
          />
          <p className="text-[10px] text-cs-muted text-right">
            {humanComment.length} / 4096
          </p>
        </div>

        <div className="flex items-center justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={onCancel}
            disabled={busy}
            className="px-3 py-1.5 rounded-md border border-cs-border bg-cs-card text-sm text-cs-muted hover:text-cs-text disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={busy}
            className={cn(
              "inline-flex items-center gap-2 px-3 py-1.5 rounded-md text-sm font-medium",
              "bg-cs-accent text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50",
            )}
          >
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Lock size={14} />}
            {busy ? "Closing…" : "Close session"}
          </button>
        </div>
      </form>
    </div>
  );
}
