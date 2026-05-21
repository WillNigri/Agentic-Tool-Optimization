import { useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import {
  X,
  Send,
  Loader2,
  AlertCircle,
  Terminal as TerminalIcon,
  Bot,
  User,
} from "lucide-react";
import { touchAgentLastUsed, type Agent } from "@/lib/agents";
import { useTerminalStore } from "@/stores/useTerminalStore";
import { buildPromptForAgent, shellRequestForAgent, getRuntimeCapability } from "@/lib/runtimeCapabilities";
import { promptAgentWithContext } from "@/lib/agentVariables";
import { useProjectStore } from "@/stores/useProjectStore";

// v1.3.0 → v1.4 — Quick-test dialog for an agent.
// Calls `promptAgent(runtime, "@<slug> <prompt>")` (CLI --print mode) and
// shows the response inline. For ongoing interactive use, "Open in Shell"
// drops to the embedded terminal with `claude` queued so the user can
// continue the session.

type Turn = { role: "user" | "agent"; text: string; ts: number };

interface Props {
  agent: Agent;
  open: boolean;
  onClose: () => void;
  /**
   * Felipe P4 — when the caller knows the prompt up front (e.g. the
   * agent has a stored `default_prompt`), pre-fill it here so the
   * dialog doesn't make the user retype it. Pair with `autoFire` to
   * dispatch on mount.
   */
  initialPrompt?: string;
  /**
   * Felipe P4 — when true (and `initialPrompt` is non-empty), the
   * dialog fires the dispatch as soon as it mounts and then behaves
   * like a normal Run dialog for any follow-up turns the user types.
   */
  autoFire?: boolean;
}

export default function RunAgentDialog({
  agent,
  open,
  onClose,
  initialPrompt,
  autoFire,
}: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const requestShell = useTerminalStore((s) => s.requestShell);
  const activeProject = useProjectStore((s) => s.activeProject);
  const [prompt, setPrompt] = useState(initialPrompt ?? "");
  const [running, setRunning] = useState(false);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [error, setError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  // Felipe P4 — guards the auto-fire effect so it can't loop if React
  // re-runs the effect (e.g. StrictMode double-invoke in dev). The
  // dialog is short-lived (mount → run → user closes), so a single
  // boolean is enough.
  const autoFiredRef = useRef(false);

  useEffect(() => {
    if (!open) {
      setPrompt(initialPrompt ?? "");
      setTurns([]);
      setError(null);
      setRunning(false);
      autoFiredRef.current = false;
    }
  }, [open, initialPrompt]);

  useEffect(() => {
    scrollRef.current?.scrollTo({
      top: scrollRef.current.scrollHeight,
      behavior: "smooth",
    });
  }, [turns]);

  const dispatch = async (rawText: string) => {
    const userText = rawText.trim();
    if (!userText || running) return;
    setPrompt("");
    setError(null);
    setTurns((ts) => [...ts, { role: "user", text: userText, ts: Date.now() }]);
    setRunning(true);
    try {
      // Build the right prompt shape for this runtime (Claude → @-mention,
      // Codex/Gemini → "[acting as ...]" prefix). The capability matrix
      // owns the per-runtime semantics.
      const fullPrompt = buildPromptForAgent(agent.runtime, agent.slug, userText);
      // v1.4 F1: dispatch through prompt_agent_with_context so any {variables}
      // in the prompt resolve at runtime against the agent's configured
      // resolvers. Equivalent to promptAgent for agents with no variables.
      const response = await promptAgentWithContext({
        agentId: agent.id,
        runtime: agent.runtime,
        prompt: fullPrompt,
        activeProjectPath: activeProject?.path,
      });
      setTurns((ts) => [
        ...ts,
        { role: "agent", text: response, ts: Date.now() },
      ]);
      // Bump last-used so the agent moves up in Recent Agents.
      try {
        await touchAgentLastUsed(agent.id);
        void queryClient.invalidateQueries({ queryKey: ["agents"] });
        void queryClient.invalidateQueries({ queryKey: ["recent-agents"] });
      } catch {
        // ignore — touch is best-effort
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setRunning(false);
    }
  };

  const onFormSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    void dispatch(prompt);
  };

  // Felipe P4 — auto-fire when MyAgentsList opens us with a pre-loaded
  // default_prompt. Runs once per open cycle (autoFiredRef gates the
  // StrictMode double-invoke + any stray re-render). The dispatch
  // helper still does its own running/empty guard.
  useEffect(() => {
    if (!open) return;
    if (!autoFire) return;
    if (autoFiredRef.current) return;
    const seed = (initialPrompt ?? "").trim();
    if (!seed) return;
    autoFiredRef.current = true;
    void dispatch(seed);
    // dispatch / initialPrompt are stable enough for this effect's
    // intent ("once when the dialog opens with a seed"); listing
    // open/autoFire/initialPrompt is the right set.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, autoFire, initialPrompt]);

  const openInShell = async () => {
    const req = await shellRequestForAgent(agent.runtime, agent.slug);
    if (!req) return; // capability matrix says this runtime can't shell-run
    requestShell(req.initialCommand, {
      followUpKeys: req.followUpKeys,
      followUpDelayMs: req.followUpDelayMs,
    });
    onClose();
  };

  const cap = getRuntimeCapability(agent.runtime);
  const canShell = cap.invocation.kind !== "manual";

  if (!open) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-2xl max-h-[85vh] flex flex-col rounded-2xl border border-cs-border bg-cs-card shadow-2xl">
        <header className="flex items-center justify-between p-5 border-b border-cs-border">
          <div className="flex items-center gap-3 min-w-0">
            <Bot size={18} className="text-cs-accent shrink-0" />
            <div className="min-w-0">
              <h2 className="text-sm font-semibold text-cs-text truncate">
                {t("runAgent.title", "Run {{name}}", { name: agent.displayName })}
              </h2>
              <p className="text-[11px] text-cs-muted truncate">
                <code className="font-mono">@{agent.slug}</code>
                {" · "}
                {agent.runtime}
                {agent.model ? ` · ${agent.model}` : ""}
              </p>
            </div>
          </div>
          <button
            type="button"
            aria-label={t("common.close", "Close")}
            onClick={onClose}
            className="text-cs-muted hover:text-cs-text"
          >
            <X size={18} />
          </button>
        </header>

        <div ref={scrollRef} className="flex-1 overflow-y-auto p-5 space-y-3 min-h-0">
          {turns.length === 0 && !running && (
            <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 text-center">
              <Bot size={24} className="mx-auto text-cs-muted mb-2" />
              <p className="text-sm text-cs-text">
                {t("runAgent.emptyTitle", "Send a prompt to test this agent")}
              </p>
              {agent.description && (
                <p className="mt-1 text-xs text-cs-muted">{agent.description}</p>
              )}
            </div>
          )}

          {turns.map((turn, idx) => (
            <TurnBubble key={idx} turn={turn} agentName={agent.displayName} />
          ))}

          {running && (
            <div className="rounded-lg bg-cs-bg-raised p-3 text-sm">
              <div className="flex items-center gap-2 text-cs-muted">
                <Loader2 size={14} className="animate-spin" />
                <span className="text-xs">
                  {t("runAgent.running", "Running…")}
                </span>
              </div>
            </div>
          )}

          {error && (
            <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3">
              <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
              <span className="text-xs text-cs-text">{error}</span>
            </div>
          )}
        </div>

        <footer className="border-t border-cs-border p-4 space-y-2">
          <form onSubmit={onFormSubmit} className="flex gap-2">
            <input
              type="text"
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder={t(
                "runAgent.promptPlaceholder",
                "What do you want this agent to do?"
              )}
              className="flex-1 rounded-lg border border-cs-border bg-cs-bg px-4 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
              disabled={running}
              autoFocus
            />
            <button
              type="submit"
              disabled={!prompt.trim() || running}
              className="inline-flex items-center gap-2 rounded-lg bg-cs-accent px-4 py-2 text-sm font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
            >
              {running ? <Loader2 size={14} className="animate-spin" /> : <Send size={14} />}
              {t("common.send", "Send")}
            </button>
          </form>
          <div className="flex items-center justify-between text-[11px] text-cs-muted">
            <span>
              {t(
                "runAgent.singleShotHint",
                "Each prompt is independent (no memory between runs). For a continuous session, open the shell."
              )}
            </span>
            {canShell ? (
              <button
                type="button"
                onClick={openInShell}
                className="inline-flex items-center gap-1 text-cs-accent hover:underline"
              >
                <TerminalIcon size={11} />
                {t("runAgent.openInShell", "Open in Shell")}
              </button>
            ) : (
              <span className="text-cs-muted/60">
                {t("runAgent.shellUnavailable", "Shell run not yet supported for {{runtime}}", {
                  runtime: cap.label,
                })}
              </span>
            )}
          </div>
        </footer>
      </div>
    </div>
  );
}

function TurnBubble({ turn, agentName }: { turn: Turn; agentName: string }) {
  if (turn.role === "user") {
    return (
      <div className="rounded-lg bg-cs-card border border-cs-border p-3 text-sm text-cs-text ml-12">
        <div className="flex items-center gap-1.5 mb-1">
          <User size={11} className="text-cs-muted" />
          <span className="text-[10px] uppercase tracking-wide text-cs-muted">You</span>
        </div>
        <p className="whitespace-pre-wrap">{turn.text}</p>
      </div>
    );
  }
  return (
    <div className="rounded-lg bg-cs-bg-raised p-3 text-sm text-cs-text">
      <div className="flex items-center gap-1.5 mb-1">
        <Bot size={11} className="text-cs-accent" />
        <span className="text-[10px] uppercase tracking-wide text-cs-accent">
          {agentName}
        </span>
      </div>
      <pre className="whitespace-pre-wrap font-sans text-cs-text">{turn.text}</pre>
    </div>
  );
}
