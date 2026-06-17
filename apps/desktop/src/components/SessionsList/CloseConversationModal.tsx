import { useState, useMemo } from "react";
import { Lock, X, Loader2, Sparkles } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { listLlmApiKeys, type LlmApiKey } from "@/lib/tauri-api";
import { cn } from "@/lib/utils";

// v2.7.12 (sessions) → v2.7.13 (generalized) — Pre-close modal for
// any closeable conversation: sessions, war rooms, chat threads. Lets
// the user pick which LLM runtime summarizes AND attach a free-form
// human note that lives in the summary card alongside the
// coordinator's output. One component for all three types; the
// `conversationType` prop drives just the copy (title + placeholder +
// button label). The actual close invocation is the parent's job —
// this modal only collects the user's intent and hands it off via
// `onSubmit`.
//
// Why a separate modal (vs inline pickers in the header): the close
// CLI surface accepts --coordinator and --human-comment for every
// conversation type, but until this modal those flags were invisible
// from the UI — clicking Close fired whatever defaults the backend
// picked, no surface for "summarize with claude not minimax" or "add
// my own framing." The modal makes both first-class for all three
// types in one place.
//
// Coordinator picker: populated from listLlmApiKeys() so users only see
// providers they can actually dispatch to. Empty list → modal renders
// the field as a disabled "(no API keys configured)" hint and still
// lets the user submit; the backend falls through to its default
// resolution chain (session agent_slug / chat agent → anchor runtime
// → first key).

// API-provider slugs that the close-summarizer can dispatch to. Mirrors
// crate::api_dispatch::registry() on the Rust side. EVERY entry shows
// up in the picker; ones without a configured key render as disabled
// options with an explanatory tag so the user knows what's missing.
// (Pre-fix the picker silently hid them, which made it look like
// `claude` / codex weren't summarization-capable when really the user
// just needed to add an API key — Will's dogfood 2026-05-21.)
const SUPPORTED_COORDINATORS: { slug: string; label: string }[] = [
  { slug: "anthropic", label: "Anthropic (Claude API)" },
  { slug: "google", label: "Google (Gemini API)" },
  { slug: "minimax", label: "MiniMax" },
  { slug: "grok", label: "Grok (xAI)" },
  { slug: "deepseek", label: "DeepSeek" },
  { slug: "qwen", label: "Qwen" },
  { slug: "openrouter", label: "OpenRouter" },
  { slug: "zai", label: "Z.AI (GLM)" },
];

export type ConversationType = "session" | "war_room" | "chat";

/** Per-type copy. Centralized so a future fourth conversation type
 *  only needs an entry here (plus the matching backend Closeable
 *  impl). The placeholder string is intentionally type-specific so
 *  the example in the empty textarea matches what a user typically
 *  writes for that conversation type. */
const COPY: Record<ConversationType, {
  title: string;
  description: string;
  buttonLabel: string;
  placeholder: string;
}> = {
  session: {
    title: "Close session",
    description:
      "The coordinator will read the conversation and produce a title, summary, topic tags, and category. Pick who summarizes — and add any framing of your own that should travel with the summary.",
    buttonLabel: "Close session",
    placeholder:
      "e.g. 'We agreed to ship the migration toast first; revisit war-room close after v2.7.12.'",
  },
  war_room: {
    title: "Close war room",
    description:
      "The coordinator will read every seat's reply across all rounds and produce a single summary capturing where they agreed, where they diverged, and what got decided. Pick who summarizes and add any framing of your own.",
    buttonLabel: "Close war room",
    placeholder:
      "e.g. 'Claude + Codex converged on the migration toast; Minimax was an outlier — discounted.'",
  },
  chat: {
    title: "Close chat thread",
    description:
      "The coordinator will read every message and produce a title, summary, topic tags, and category. Pick who summarizes — and add any framing of your own that should travel with the summary.",
    buttonLabel: "Close chat",
    placeholder:
      "e.g. 'Quick triage convo — actual fix tracked in war room ABC123.'",
  },
};

interface Props {
  open: boolean;
  onCancel: () => void;
  /** Called with the user's choices. Caller invokes the matching
   *  Tauri command (close_session / close_war_room / close_chat) and
   *  shows the "Coordinator is summarizing…" blocker. */
  onSubmit: (opts: { coordinator: string | null; humanComment: string | null }) => void;
  /** Disables the Submit button while the parent is mid-dispatch. */
  busy?: boolean;
  /** Drives the per-type copy. Defaults to "session" so existing
   *  callers keep working without an explicit prop. */
  conversationType?: ConversationType;
}

export default function CloseConversationModal({
  open,
  onCancel,
  onSubmit,
  busy = false,
  conversationType = "session",
}: Props) {
  const copy = COPY[conversationType];
  const [coordinator, setCoordinator] = useState<string>("");
  const [humanComment, setHumanComment] = useState<string>("");

  const { data: apiKeys = [] } = useQuery<LlmApiKey[]>({
    queryKey: ["llm-api-keys"],
    queryFn: () => listLlmApiKeys(),
    enabled: open,
    staleTime: 60_000,
  });

  // v2.7.13 fix — instead of filtering hidden, return EVERY supported
  // coordinator with a `configured` flag so the picker can render
  // unconfigured ones as disabled. The disabled-option pattern keeps
  // claude / codex / etc. discoverable so users know they exist + how
  // to enable them; the prior filter-out shape silently hid them and
  // made the picker look like only Google + MiniMax were ever options.
  const coordinatorOptions = useMemo(() => {
    const configured = new Set(apiKeys.map((k) => k.provider));
    return SUPPORTED_COORDINATORS.map((c) => ({
      ...c,
      configured: configured.has(c.slug),
    }));
  }, [apiKeys]);
  const anyConfigured = coordinatorOptions.some((c) => c.configured);

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
      aria-labelledby="close-conversation-title"
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
            <h2 id="close-conversation-title" className="text-sm font-semibold text-cs-text">
              {copy.title}
            </h2>
            <p className="text-xs text-cs-muted mt-1 leading-relaxed">
              {copy.description}
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
            disabled={busy}
            className="w-full rounded-md border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none disabled:opacity-50"
          >
            <option value="">
              {anyConfigured
                ? "Default (auto-pick from session agent → anchor → first key)"
                : "(no API keys configured — backend default will be used)"}
            </option>
            {coordinatorOptions.map((c) => (
              <option key={c.slug} value={c.slug} disabled={!c.configured}>
                {c.label}
                {!c.configured ? " — no API key configured" : ""}
              </option>
            ))}
          </select>
          {!anyConfigured && (
            <p className="text-[10px] text-cs-muted">
              Add a provider key in Settings → API Keys to enable the picker.
              Note: Claude Code / Codex / Gemini CLI subscriptions don't
              count — the summarizer dispatches via the provider's API.
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
            placeholder={copy.placeholder}
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
            {busy ? "Closing…" : copy.buttonLabel}
          </button>
        </div>
      </form>
    </div>
  );
}
