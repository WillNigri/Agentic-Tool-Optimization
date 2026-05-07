import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import {
  Send,
  Loader2,
  AlertCircle,
  CheckCircle2,
  Settings,
  Plug,
  Server,
  Check,
  Sparkles,
  KeyRound,
  Globe,
  Shield,
} from "lucide-react";
import {
  startConversation,
  continueConversation,
  listReadyRuntimes,
  NoRuntimeError,
  type Turn,
  type AgentSpec,
  type CredentialRequest,
} from "@/lib/agentConversation";
import { createAgent, type AgentRuntime } from "@/lib/agents";
import { loadGuidedDraft, saveGuidedDraft, clearGuidedDraft } from "@/lib/agentDraft";
import {
  getMcpRegistry,
  installMcpToRuntime,
  detectPlaceholders,
  type McpRegistryEntry,
  type InstallableRuntime,
} from "@/lib/mcpRegistry";
import McpInstallOptions, { type OptionValues } from "./McpInstallOptions";
import { promptAgent } from "@/lib/tauri-api";
import { useTerminalStore } from "@/stores/useTerminalStore";
import { useDemoStore } from "@/stores/useDemoStore";

// v1.3.0 T3.c — Multi-turn conversational wizard.
// The LLM asks clarifying questions until it has enough info, then emits a
// review with the full agent spec. After confirm, MCPs install + auth steps
// are surfaced inline.

type Phase = "idle" | "thinking" | "asking" | "review" | "writing" | "done" | "no_runtime" | "error";

interface Props {
  onCreated?: (agentId: string) => void;
  onCancel: () => void;
}

export default function GuidedPath({ onCreated, onCancel }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [phase, setPhase] = useState<Phase>("idle");
  const [goal, setGoal] = useState(() => loadGuidedDraft()?.goal ?? "");
  const [history, setHistory] = useState<Turn[]>([]);
  const [runtime, setRuntime] = useState<AgentRuntime | null>(null);
  const [pending, setPending] = useState<Extract<Turn, { role: "assistant" }> | null>(null);
  const [reply, setReply] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [createdAgent, setCreatedAgent] = useState<{
    name: string;
    runtime: AgentRuntime;
    spec: AgentSpec;
  } | null>(null);

  const scrollRef = useRef<HTMLDivElement | null>(null);

  // All native runtimes — shown in the picker so the user always sees the
  // full menu. Unavailable ones render disabled with a "connect first" hint
  // (better discovery than hiding them entirely).
  const ALL_RUNTIMES: AgentRuntime[] = ["claude", "codex", "gemini", "openclaw", "hermes"];
  const [readyRuntimes, setReadyRuntimes] = useState<AgentRuntime[]>([]);
  const [selectedRuntime, setSelectedRuntime] = useState<AgentRuntime | null>(null);

  useEffect(() => {
    let cancelled = false;
    listReadyRuntimes()
      .then((rts) => {
        if (cancelled) return;
        setReadyRuntimes(rts);
        // Default to the first available; user can switch via the picker chips.
        if (rts.length > 0 && !selectedRuntime) setSelectedRuntime(rts[0]);
      })
      .catch(() => {
        // Stays empty; the no_runtime branch will trigger on submit.
      });
    return () => {
      cancelled = true;
    };
  // intentional one-shot on mount
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Auto-save just the goal so users can resume the wizard.
  useEffect(() => {
    saveGuidedDraft({ goal, submittedGoal: goal });
  }, [goal]);

  // Auto-scroll on new turns.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [history, pending, phase]);

  // Demo runner integration — animate typing the goal + submit.
  const demoIsPlaying = useDemoStore((s) => s.isPlaying);
  const demoGoal = useDemoStore((s) => s.pendingGuidedGoal);
  const demoSubmitGoal = useDemoStore((s) => s.pendingGuidedSubmit);
  useEffect(() => {
    if (demoIsPlaying) setGoal(demoGoal);
  }, [demoIsPlaying, demoGoal]);
  const lastSeenGoalSubmitRef = useRef(0);
  useEffect(() => {
    if (demoSubmitGoal > lastSeenGoalSubmitRef.current) {
      lastSeenGoalSubmitRef.current = demoSubmitGoal;
      requestAnimationFrame(() => {
        const fakeEvent = { preventDefault: () => {} } as React.FormEvent;
        void start(fakeEvent);
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [demoSubmitGoal]);

  const start = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!goal.trim() || phase === "thinking") return;
    setError(null);
    setPhase("thinking");
    try {
      const { runtime: rt, history: h, next } = await startConversation(
        goal.trim(),
        selectedRuntime ?? undefined
      );
      setRuntime(rt);
      setHistory(h);
      setPending(next);
      setPhase(next.type === "review" ? "review" : "asking");
    } catch (err) {
      if (err instanceof NoRuntimeError) {
        setPhase("no_runtime");
      } else {
        setError(err instanceof Error ? err.message : String(err));
        setPhase("error");
      }
    }
  };

  const sendReply = async (text: string) => {
    if (!runtime || !pending || !text.trim() || phase === "thinking") return;
    setError(null);
    // Commit current assistant turn + user reply to history.
    const newHistory: Turn[] = [...history, pending, { role: "user", content: text.trim() }];
    setHistory(newHistory);
    setPending(null);
    setReply("");
    setPhase("thinking");
    try {
      const next = await continueConversation(runtime, newHistory, text.trim());
      setPending(next);
      setPhase(next.type === "review" ? "review" : "asking");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPhase("error");
    }
  };

  const confirmReview = async () => {
    if (!runtime || !pending || pending.type !== "review") return;
    const spec = pending.spec;
    setPhase("writing");
    setError(null);
    try {
      const permissionList = [
        ...spec.permissions.allowed.map((a) => `allow:${a}`),
        ...spec.permissions.requireApproval.map((a) => `approve:${a}`),
        ...spec.permissions.denied.map((a) => `deny:${a}`),
      ];
      const agent = await createAgent({
        displayName: spec.displayName,
        runtime: runtime,
        description: spec.description,
        model: spec.model,
        systemPrompt: spec.systemPrompt,
        goal,
        mcps: spec.recommendedMcps.length > 0 ? spec.recommendedMcps : undefined,
        skills: spec.recommendedSkills.length > 0 ? spec.recommendedSkills : undefined,
        permissions: permissionList.length > 0 ? permissionList : undefined,
      });
      setCreatedAgent({ name: agent.displayName, runtime, spec });
      setPhase("done");
      clearGuidedDraft();
      void queryClient.invalidateQueries({ queryKey: ["agents"] });
      void queryClient.invalidateQueries({ queryKey: ["recent-agents"] });
      onCreated?.(agent.id);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPhase("review");
    }
  };

  return (
    <div ref={scrollRef} className="space-y-3 max-h-[60vh] overflow-y-auto pr-1">
      {/* Initial prompt */}
      {phase === "idle" && (
        <>
          <Bubble who="ato">
            {t(
              "createAgent.guided.askGoal",
              "Hi! What do you want your agent to help with? Describe it in plain English."
            )}
          </Bubble>

          {/* Runtime picker — always shows all six native runtimes so users
              see the full menu. Unavailable ones render disabled with a
              "Connect in Settings" tooltip — discovery beats hiding. */}
          <div className="rounded-lg border border-cs-border bg-cs-bg-raised p-3">
            <div className="text-[11px] uppercase tracking-wide text-cs-muted mb-2">
              {t("createAgent.guided.pickRuntime", "Run this agent on")}
            </div>
            <div className="flex flex-wrap gap-1.5">
              {ALL_RUNTIMES.map((rt) => {
                const ready = readyRuntimes.includes(rt);
                const active = selectedRuntime === rt;
                return (
                  <button
                    key={rt}
                    type="button"
                    onClick={() => ready && setSelectedRuntime(rt)}
                    disabled={!ready}
                    title={
                      ready
                        ? `Run the wizard + agent on ${runtimeLabel(rt)}`
                        : `${runtimeLabel(rt)} isn't connected. Open Settings → Runtimes to connect it.`
                    }
                    className={
                      !ready
                        ? "inline-flex items-center gap-1.5 rounded-full border border-cs-border/60 bg-cs-bg/40 px-3 py-1 text-xs font-medium text-cs-muted/60 opacity-60 cursor-not-allowed"
                        : active
                        ? "inline-flex items-center gap-1.5 rounded-full border border-cs-accent bg-cs-accent/10 px-3 py-1 text-xs font-medium text-cs-accent"
                        : "inline-flex items-center gap-1.5 rounded-full border border-cs-border bg-cs-bg px-3 py-1 text-xs font-medium text-cs-muted hover:border-cs-hover hover:text-cs-text"
                    }
                  >
                    <span className={`inline-block w-1.5 h-1.5 rounded-full ${ready ? runtimeDot(rt) : "bg-cs-muted/40"}`} />
                    {runtimeLabel(rt)}
                    {!ready && <span className="text-[9px] uppercase tracking-wider">· connect</span>}
                  </button>
                );
              })}
            </div>
            <p className="mt-2 text-[10px] text-cs-muted">
              {t(
                "createAgent.guided.pickRuntimeHint",
                "Both the wizard's questions and the agent itself will run on this runtime. You can change it later."
              )}
            </p>

            {/* API-key providers — discovery hint. Full agent dispatch
                via these providers is a v2.0 item; for now we surface
                the menu so users with MiniMax/Qwen/Grok keys see the
                product supports them and know where to configure. */}
            <div className="mt-3 pt-3 border-t border-cs-border/50">
              <div className="flex items-start gap-2">
                <KeyRound size={11} className="text-cs-muted shrink-0 mt-0.5" />
                <div className="flex-1 min-w-0">
                  <p className="text-[10px] uppercase tracking-wide text-cs-muted mb-1">
                    {t("createAgent.guided.apiKeyProviders", "Or via API key")}
                  </p>
                  <p className="text-[10px] text-cs-muted leading-relaxed">
                    {t(
                      "createAgent.guided.apiKeyProvidersList",
                      "Anthropic, OpenAI, Google, Mistral, Groq, xAI/Grok, Together, Fireworks, DeepSeek, Qwen, MiniMax, Kimi, GLM, Yi — once an API key is configured, the agent dispatches against that provider. Full UI lands in v2.0."
                    )}
                  </p>
                  <p className="mt-1 text-[10px]">
                    <span className="text-cs-accent">Settings → API Keys</span>
                    <span className="text-cs-muted"> to set them up.</span>
                  </p>
                </div>
              </div>
            </div>
          </div>

          <form onSubmit={start} className="flex gap-2">
            <input
              type="text"
              value={goal}
              onChange={(e) => setGoal(e.target.value)}
              placeholder={t(
                "createAgent.guided.goalPlaceholder",
                "e.g., review my pull requests for security issues"
              )}
              className="flex-1 rounded-lg border border-cs-border bg-cs-bg px-4 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
              autoFocus
            />
            <button
              type="submit"
              disabled={!goal.trim()}
              className="inline-flex items-center gap-2 rounded-lg bg-cs-accent px-4 py-2 text-sm font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
            >
              <Send size={14} />
              {t("common.send", "Send")}
            </button>
          </form>
        </>
      )}

      {/* Conversation history */}
      {history.map((turn, idx) => (
        <HistoryTurn key={idx} turn={turn} />
      ))}

      {/* Currently-pending assistant turn */}
      {pending && pending.type === "ask" && phase === "asking" && (
        <>
          <Bubble who="ato">{pending.text}</Bubble>
          {pending.suggestions && pending.suggestions.length > 0 && (
            <div className="flex flex-wrap gap-1.5 ml-1">
              {pending.suggestions.map((s) => (
                <button
                  key={s}
                  type="button"
                  onClick={() => sendReply(s)}
                  className="rounded-full border border-cs-border bg-cs-bg-raised px-3 py-1 text-xs text-cs-text hover:border-cs-accent hover:text-cs-accent"
                >
                  {s}
                </button>
              ))}
            </div>
          )}
          <form
            onSubmit={(e) => {
              e.preventDefault();
              sendReply(reply);
            }}
            className="flex gap-2"
          >
            <input
              type="text"
              value={reply}
              onChange={(e) => setReply(e.target.value)}
              placeholder={t("createAgent.guided.replyPlaceholder", "Your answer…")}
              className="flex-1 rounded-lg border border-cs-border bg-cs-bg px-4 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
              autoFocus
            />
            <button
              type="submit"
              disabled={!reply.trim()}
              className="inline-flex items-center gap-2 rounded-lg bg-cs-accent px-4 py-2 text-sm font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
            >
              <Send size={14} />
            </button>
          </form>
        </>
      )}

      {pending && pending.type === "review" && phase === "review" && runtime && (
        <ReviewCard
          spec={pending.spec}
          runtime={runtime}
          onConfirm={confirmReview}
          onTryAgain={() => {
            // Keep history; let the user nudge the LLM via chat.
            setPending({
              role: "assistant",
              type: "ask",
              text: t("createAgent.guided.anythingElse", "What would you change?"),
            });
            setPhase("asking");
          }}
          error={error}
        />
      )}

      {phase === "thinking" && (
        <Bubble who="ato">
          <span className="inline-flex items-center gap-2">
            <Loader2 size={14} className="animate-spin" />
            {t("createAgent.guided.thinking", "Thinking…")}
          </span>
        </Bubble>
      )}

      {phase === "writing" && (
        <Bubble who="ato">
          <span className="inline-flex items-center gap-2">
            <Loader2 size={14} className="animate-spin" />
            {t("createAgent.guided.writing", "Writing the agent file…")}
          </span>
        </Bubble>
      )}

      {phase === "no_runtime" && (
        <div className="rounded-lg border border-cs-warning/40 bg-cs-warning/10 p-4 flex items-start gap-3">
          <Settings size={18} className="text-cs-warning shrink-0" />
          <div className="flex-1">
            <h3 className="text-sm font-medium text-cs-text">
              {t("createAgent.guided.noRuntimeTitle", "Connect a runtime first")}
            </h3>
            <p className="mt-1 text-xs text-cs-muted">
              {t(
                "createAgent.guided.noRuntimeBody",
                "The chat wizard uses your existing CLI subscription (Claude, Codex, Gemini, Hermes, OpenClaw) to drive the conversation. None were detected. Open Settings → Runtimes to connect one, or use the Quick (form) path which doesn't need a runtime to be live."
              )}
            </p>
          </div>
        </div>
      )}

      {phase === "error" && error && (
        <div className="rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 flex items-start gap-2">
          <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
          <div className="flex-1">
            <p className="text-xs text-cs-text">{error}</p>
            <button
              type="button"
              onClick={() => {
                setError(null);
                setPhase(history.length === 0 ? "idle" : "asking");
              }}
              className="mt-1 text-xs text-cs-accent hover:underline"
            >
              {t("createAgent.guided.editGoal", "Try again")}
            </button>
          </div>
        </div>
      )}

      {phase === "done" && createdAgent && (
        <DoneCard
          agentName={createdAgent.name}
          runtime={createdAgent.runtime}
          spec={createdAgent.spec}
          onCloseModal={onCancel}
        />
      )}
    </div>
  );
}

const RUNTIME_LABELS: Record<AgentRuntime, string> = {
  claude: "Claude Code",
  codex: "Codex / GPT",
  gemini: "Gemini CLI",
  openclaw: "OpenClaw",
  hermes: "Hermes",
};

const RUNTIME_DOTS: Record<AgentRuntime, string> = {
  claude: "bg-orange-500",
  codex: "bg-green-500",
  gemini: "bg-blue-500",
  openclaw: "bg-cyan-400",
  hermes: "bg-purple-500",
};

function runtimeLabel(rt: AgentRuntime): string {
  return RUNTIME_LABELS[rt] ?? rt;
}

function runtimeDot(rt: AgentRuntime): string {
  return RUNTIME_DOTS[rt] ?? "bg-cs-muted";
}

function HistoryTurn({ turn }: { turn: Turn }) {
  if (turn.role === "user") {
    return <Bubble who="user">{turn.content}</Bubble>;
  }
  if (turn.type === "ask") {
    return <Bubble who="ato">{turn.text}</Bubble>;
  }
  // review turn: show a compact one-liner so the thread doesn't get noisy
  return <Bubble who="ato">{turn.text || "Here's my proposal."}</Bubble>;
}

function Bubble({ who, children }: { who: "ato" | "user"; children: React.ReactNode }) {
  if (who === "ato") {
    return (
      <div className="rounded-lg bg-cs-bg-raised p-3 text-sm text-cs-text">
        <strong className="text-cs-accent text-xs uppercase tracking-wide">ATO</strong>
        <div className="mt-1">{children}</div>
      </div>
    );
  }
  return (
    <div className="rounded-lg bg-cs-card border border-cs-border p-3 text-sm text-cs-text ml-8">
      {children}
    </div>
  );
}

function ReviewCard({
  spec,
  runtime,
  onConfirm,
  onTryAgain,
  error,
}: {
  spec: AgentSpec;
  runtime: AgentRuntime;
  onConfirm: () => void;
  onTryAgain: () => void;
  error: string | null;
}) {
  const { t } = useTranslation();
  return (
    <div className="rounded-lg border border-cs-accent/40 bg-cs-card p-4 space-y-3">
      <header>
        <div className="flex items-center gap-2">
          <Sparkles size={14} className="text-cs-accent" />
          <h3 className="text-sm font-semibold text-cs-text">{spec.displayName}</h3>
        </div>
        <p className="mt-1 text-xs text-cs-muted">{spec.description}</p>
      </header>

      <dl className="grid grid-cols-2 gap-3 text-xs">
        <Field label={t("createAgent.guided.runtime", "Runtime")}>{runtime}</Field>
        <Field label={t("createAgent.guided.model", "Model")}>{spec.model}</Field>
      </dl>

      <details className="text-xs">
        <summary className="cursor-pointer text-cs-muted hover:text-cs-text">
          {t("createAgent.guided.viewSystemPrompt", "View system prompt")}
        </summary>
        <pre className="mt-2 rounded bg-cs-bg p-3 text-cs-text font-mono whitespace-pre-wrap">
          {spec.systemPrompt}
        </pre>
      </details>

      {spec.recommendedMcps.length > 0 && (
        <Pill icon={<Plug size={12} />} label={t("createAgent.guided.willConnect", "Will connect")}>
          {spec.recommendedMcps.join(", ")}
        </Pill>
      )}

      {spec.recommendedSkills.length > 0 && (
        <Pill icon={<Sparkles size={12} />} label={t("createAgent.guided.willEnableSkills", "Will enable skills")}>
          {spec.recommendedSkills.join(", ")}
        </Pill>
      )}

      {spec.credentials.length > 0 && (
        <Pill icon={<KeyRound size={12} />} label={t("createAgent.guided.willAskCreds", "Will need")}>
          {spec.credentials.map((c) => c.label).join(", ")}
        </Pill>
      )}

      {spec.permissions && (spec.permissions.summary || spec.permissions.allowed.length > 0) && (
        <div className="rounded bg-cs-bg-raised p-2.5 text-xs">
          <div className="flex items-center gap-1.5 text-cs-text font-medium">
            <Shield size={12} className="text-cs-accent" />
            {t("createAgent.guided.permissions", "Permissions")}
          </div>
          {spec.permissions.summary && (
            <p className="mt-1 text-cs-muted">{spec.permissions.summary}</p>
          )}
          {spec.permissions.allowed.length > 0 && (
            <p className="mt-1 text-[11px]">
              <span className="text-cs-accent">✓ </span>
              <span className="text-cs-muted">{spec.permissions.allowed.join(", ")}</span>
            </p>
          )}
          {spec.permissions.requireApproval.length > 0 && (
            <p className="mt-0.5 text-[11px]">
              <span className="text-cs-warning">! </span>
              <span className="text-cs-muted">
                {t("createAgent.guided.requireApproval", "Asks first")}: {spec.permissions.requireApproval.join(", ")}
              </span>
            </p>
          )}
          {spec.permissions.denied.length > 0 && (
            <p className="mt-0.5 text-[11px]">
              <span className="text-cs-danger">✕ </span>
              <span className="text-cs-muted">
                {t("createAgent.guided.denied", "Never")}: {spec.permissions.denied.join(", ")}
              </span>
            </p>
          )}
        </div>
      )}

      {spec.reasoning && (
        <p className="text-[11px] italic text-cs-muted">"{spec.reasoning}"</p>
      )}

      {error && (
        <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 p-2 text-xs text-cs-text flex items-start gap-2">
          <AlertCircle size={12} className="text-cs-danger shrink-0 mt-0.5" />
          <span>{error}</span>
        </div>
      )}

      <div className="flex items-center justify-end gap-2 pt-1">
        <button
          type="button"
          onClick={onTryAgain}
          className="rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-muted hover:text-cs-text"
        >
          {t("createAgent.guided.adjust", "Adjust")}
        </button>
        <button
          type="button"
          onClick={onConfirm}
          className="inline-flex items-center gap-2 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
        >
          <CheckCircle2 size={12} />
          {t("createAgent.guided.confirm", "Create this agent")}
        </button>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt className="text-cs-muted uppercase tracking-wide">{label}</dt>
      <dd className="text-cs-text font-mono mt-0.5">{children}</dd>
    </div>
  );
}

function Pill({
  icon,
  label,
  children,
}: {
  icon: React.ReactNode;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="rounded bg-cs-bg-raised p-2.5 text-xs">
      <div className="flex items-center gap-1.5 text-cs-text font-medium">
        <span className="text-cs-accent">{icon}</span>
        {label}
      </div>
      <div className="mt-1 text-cs-muted font-mono text-[11px] break-words">{children}</div>
    </div>
  );
}

function DoneCard({
  agentName,
  runtime,
  spec,
  onCloseModal,
}: {
  agentName: string;
  runtime: AgentRuntime;
  spec: AgentSpec;
  onCloseModal: () => void;
}) {
  const { t } = useTranslation();
  const canInstall = runtime === "claude" || runtime === "codex" || runtime === "gemini";

  return (
    <div className="space-y-3">
      <div className="rounded-lg border border-cs-accent/40 bg-cs-accent/10 p-4 flex items-start gap-3">
        <CheckCircle2 size={18} className="text-cs-accent shrink-0" />
        <div className="flex-1">
          <h3 className="text-sm font-medium text-cs-text">
            {t("createAgent.guided.doneTitle", "Agent ready")}
          </h3>
          <p className="mt-1 text-xs text-cs-muted">
            {spec.recommendedMcps.length > 0 || spec.credentials.length > 0
              ? t(
                  "createAgent.guided.doneWithMcps",
                  "{{name}} was created. Now connect the MCP tools below so it can actually do its job.",
                  { name: agentName }
                )
              : t(
                  "createAgent.guided.doneBody",
                  "{{name}} was created. Open it from the Agents list.",
                  { name: agentName }
                )}
          </p>
        </div>
      </div>

      {spec.recommendedMcps.length > 0 && (
        <RecommendedMcps
          mcpIds={spec.recommendedMcps}
          runtime={runtime as InstallableRuntime}
          canInstall={canInstall}
          onCloseModal={onCloseModal}
        />
      )}

      {spec.credentials.length > 0 && (
        <CredentialsCard credentials={spec.credentials} />
      )}
    </div>
  );
}

function RecommendedMcps({
  mcpIds,
  runtime,
  canInstall,
  onCloseModal,
}: {
  mcpIds: string[];
  runtime: InstallableRuntime;
  canInstall: boolean;
  onCloseModal: () => void;
}) {
  const { t } = useTranslation();
  const [registryEntries, setRegistryEntries] = useState<McpRegistryEntry[]>([]);
  const [installState, setInstallState] = useState<
    Record<string, { status: "idle" | "installing" | "ok" | "error"; message?: string }>
  >({});
  const [connectState, setConnectState] = useState<
    Record<string, { status: "idle" | "connecting" | "ok" | "error"; message?: string }>
  >({});
  // Per-MCP install options (paths for filesystem, URLs for postgres, etc.).
  const [mcpOptions, setMcpOptions] = useState<Record<string, OptionValues>>({});

  useEffect(() => {
    let cancelled = false;
    getMcpRegistry()
      .then((reg) => {
        if (cancelled) return;
        const entries = mcpIds
          .map((id) => reg.entries.find((e) => e.id === id))
          .filter((e): e is McpRegistryEntry => !!e);
        setRegistryEntries(entries);
      })
      .catch(() => {
        // ignore — fallback registry is bundled
      });
    return () => {
      cancelled = true;
    };
  }, [mcpIds]);

  const install = async (entry: McpRegistryEntry) => {
    setInstallState((s) => ({ ...s, [entry.id]: { status: "installing" } }));
    try {
      const path = await installMcpToRuntime(runtime, entry, {
        values: mcpOptions[entry.id],
      });
      setInstallState((s) => ({ ...s, [entry.id]: { status: "ok", message: path } }));
    } catch (err) {
      setInstallState((s) => ({
        ...s,
        [entry.id]: {
          status: "error",
          message: err instanceof Error ? err.message : String(err),
        },
      }));
    }
  };

  // For OAuth-required MCPs, trigger the OAuth flow by asking the runtime to
  // exercise the tool. The MCP server starts, sees no token, and opens the
  // browser. We just relay output back to the user.
  const connect = async (entry: McpRegistryEntry) => {
    if (runtime !== "claude") {
      setConnectState((s) => ({
        ...s,
        [entry.id]: {
          status: "error",
          message: "Auto-connect only supported for Claude runtime today. Use the Shell tab to test manually.",
        },
      }));
      return;
    }
    setConnectState((s) => ({ ...s, [entry.id]: { status: "connecting" } }));
    try {
      const testPrompt = connectionTestPromptFor(entry.id);
      const output = await promptAgent("claude", testPrompt);
      setConnectState((s) => ({
        ...s,
        [entry.id]: {
          status: "ok",
          message: output.slice(0, 240) + (output.length > 240 ? "…" : ""),
        },
      }));
    } catch (err) {
      setConnectState((s) => ({
        ...s,
        [entry.id]: {
          status: "error",
          message: err instanceof Error ? err.message : String(err),
        },
      }));
    }
  };

  if (registryEntries.length === 0) return null;

  return (
    <div className="rounded-lg border border-cs-border bg-cs-card p-4 space-y-3">
      <header className="flex items-center gap-2">
        <Plug size={14} className="text-cs-accent" />
        <h4 className="text-xs font-semibold text-cs-text uppercase tracking-wide">
          {t("createAgent.guided.connectMcps", "Connect tools")}
        </h4>
      </header>
      <div className="space-y-2">
        {registryEntries.map((entry) => {
          const state = installState[entry.id] ?? { status: "idle" };
          const installed = state.status === "ok";
          const installing = state.status === "installing";
          const hasOptions = detectPlaceholders(entry).length > 0;
          return (
            <div
              key={entry.id}
              className="rounded-md border border-cs-border bg-cs-bg-raised p-3"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <Server size={12} className="text-cs-muted shrink-0" />
                    <span className="text-sm font-medium text-cs-text truncate">{entry.name}</span>
                  </div>
                  <p className="mt-1 text-xs text-cs-muted">{entry.description}</p>
                  {entry.authNote && (
                    <p className="mt-1.5 text-[11px] text-cs-warning">⚠ {entry.authNote}</p>
                  )}
                </div>
                {canInstall ? (
                  <button
                    type="button"
                    onClick={() => install(entry)}
                    disabled={installing || installed}
                    className={
                      installed
                        ? "inline-flex items-center gap-1 rounded-md border border-cs-accent bg-cs-accent/10 px-3 py-1.5 text-xs font-medium text-cs-accent shrink-0"
                        : "inline-flex items-center gap-1 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-60 shrink-0"
                    }
                  >
                    {installing ? (
                      <Loader2 size={12} className="animate-spin" />
                    ) : installed ? (
                      <Check size={12} />
                    ) : null}
                    {installed
                      ? t("createAgent.guided.installed", "Installed")
                      : installing
                      ? t("createAgent.guided.installing", "Installing…")
                      : t("createAgent.guided.install", "Install")}
                  </button>
                ) : (
                  <span className="text-[10px] text-cs-muted shrink-0">
                    {t("createAgent.guided.installManually", "Manual install")}
                  </span>
                )}
              </div>

              {/* Install-time options (paths for filesystem, URLs for postgres,
                  etc.). Only shown when the registry entry has $VAR placeholders
                  AND the MCP isn't already installed. */}
              {hasOptions && !installed && canInstall && (
                <McpInstallOptions
                  entry={entry}
                  values={mcpOptions[entry.id] ?? {}}
                  onChange={(v) => setMcpOptions((s) => ({ ...s, [entry.id]: v }))}
                />
              )}

              {state.status === "ok" && state.message && (
                <p className="mt-2 text-[11px] text-cs-accent font-mono break-all">
                  {t("createAgent.guided.wroteTo", "Wrote to {{path}}", { path: state.message })}
                </p>
              )}
              {state.status === "error" && state.message && (
                <p className="mt-2 text-[11px] text-cs-danger break-all">{state.message}</p>
              )}

              {/* OAuth / connection trigger row — for OAuth MCPs, fire the tool
                  through the runtime so the MCP server starts and prompts for
                  auth in the browser. */}
              {state.status === "ok" && entry.authNote?.toLowerCase().includes("oauth") && (
                <div className="mt-3 pt-3 border-t border-cs-border">
                  <ConnectButton
                    entry={entry}
                    state={connectState[entry.id] ?? { status: "idle" }}
                    onConnect={() => connect(entry)}
                    onCloseModal={onCloseModal}
                  />
                </div>
              )}
            </div>
          );
        })}
      </div>
      <p className="text-[11px] text-cs-muted">
        {t(
          "createAgent.guided.installedHint",
          "Installing only writes the MCP entry to {{runtime}}'s settings. The server starts the first time the agent uses it — that's when OAuth flows fire and API keys get exercised.",
          { runtime }
        )}
      </p>
    </div>
  );
}

function ConnectButton({
  entry,
  state,
  onConnect,
  onCloseModal,
}: {
  entry: McpRegistryEntry;
  state: { status: "idle" | "connecting" | "ok" | "error"; message?: string };
  onConnect: () => void;
  onCloseModal: () => void;
}) {
  const { t } = useTranslation();
  const requestShell = useTerminalStore((s) => s.requestShell);
  const connecting = state.status === "connecting";
  const done = state.status === "ok";
  // Detect when the runtime told us auth must happen via Claude Code's
  // interactive UI (`/mcp` slash command). `claude --print` cannot trigger
  // that flow, so we route the user to the embedded interactive shell.
  const needsInteractive =
    done &&
    !!state.message &&
    (state.message.includes("/mcp") || /authenticate.*Claude Code UI/i.test(state.message));

  if (needsInteractive) {
    return (
      <div className="space-y-2">
        <div className="rounded-md border border-cs-warning/40 bg-cs-warning/10 p-3 text-xs">
          <p className="text-cs-text font-medium">
            {t(
              "createAgent.guided.interactiveTitle",
              "Authentication needs Claude Code's interactive UI"
            )}
          </p>
          <p className="mt-1 text-cs-muted">
            {t(
              "createAgent.guided.interactiveBody",
              "Claude.ai's built-in {{name}} integration authenticates through the /mcp menu inside an interactive Claude session. We'll open the embedded Shell, start Claude, and queue /mcp for you.",
              { name: entry.name }
            )}
          </p>
          <button
            type="button"
            onClick={() => {
              // Queue the command BEFORE closing the modal so the terminal
              // store fires before this component unmounts.
              requestShell("claude");
              onCloseModal();
            }}
            className="mt-2 inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
          >
            <Globe size={12} />
            {t("createAgent.guided.openClaudeInteractive", "Open Claude in Shell")}
          </button>
          <ol className="mt-3 space-y-1 text-[11px] text-cs-muted list-decimal list-inside">
            <li>
              {t(
                "createAgent.guided.step1",
                "When the shell prompt appears, type /mcp and press Enter"
              )}
            </li>
            <li>
              {t(
                "createAgent.guided.step2",
                'Use ↑↓ to select "{{name}}" and press Enter',
                { name: entry.name }
              )}
            </li>
            <li>
              {t(
                "createAgent.guided.step3",
                "Complete OAuth in the browser tab that opens"
              )}
            </li>
            <li>
              {t(
                "createAgent.guided.step4",
                "Come back here — the agent now has access"
              )}
            </li>
          </ol>
        </div>
        {state.message && (
          <details className="text-[10px]">
            <summary className="cursor-pointer text-cs-muted hover:text-cs-text">
              {t("createAgent.guided.viewClaudeReply", "View Claude's reply")}
            </summary>
            <pre className="mt-1 rounded bg-cs-bg p-2 text-cs-muted font-mono whitespace-pre-wrap max-h-32 overflow-y-auto">
              {state.message}
            </pre>
          </details>
        )}
      </div>
    );
  }

  return (
    <div>
      <div className="flex items-center justify-between gap-3">
        <span className="text-[11px] text-cs-muted">
          {done
            ? t(
                "createAgent.guided.connectDone",
                "Connected. If a browser tab opened, complete the auth flow there."
              )
            : t(
                "createAgent.guided.connectHint",
                "Click to ask Claude to test {{name}} now — if OAuth is needed, your browser will open.",
                { name: entry.name }
              )}
        </span>
        <button
          type="button"
          onClick={onConnect}
          disabled={connecting || done}
          className={
            done
              ? "inline-flex items-center gap-1 rounded-md border border-cs-accent bg-cs-accent/10 px-3 py-1.5 text-xs font-medium text-cs-accent shrink-0"
              : "inline-flex items-center gap-1 rounded-md border border-cs-accent bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-accent hover:bg-cs-accent/10 shrink-0 disabled:opacity-60"
          }
        >
          {connecting ? <Loader2 size={12} className="animate-spin" /> : done ? <Check size={12} /> : <Globe size={12} />}
          {connecting
            ? t("createAgent.guided.connecting", "Connecting…")
            : done
            ? t("createAgent.guided.connected", "Connected")
            : t("createAgent.guided.connectNow", "Connect now")}
        </button>
      </div>
      {state.message && state.status !== "idle" && (
        <pre className="mt-2 rounded bg-cs-bg p-2 text-[10px] text-cs-muted font-mono whitespace-pre-wrap max-h-32 overflow-y-auto">
          {state.message}
        </pre>
      )}
    </div>
  );
}

function connectionTestPromptFor(mcpId: string): string {
  switch (mcpId) {
    case "gmail":
      return "Use the gmail MCP to fetch the subject of my single most recent email and reply with just that subject. If you need to authenticate, do that first — the browser may open.";
    case "calendar":
      return "Use the Google Calendar MCP to list my next 3 calendar events. If authentication is required, do that first.";
    case "github":
      return "Use the github MCP to confirm you can list my repositories. Reply with the count.";
    case "slack":
      return "Use the slack MCP to confirm you can list channels in my workspace. Reply with the count.";
    default:
      return `Use the ${mcpId} MCP to verify the connection works. Reply with whatever the tool returns.`;
  }
}

function CredentialsCard({ credentials }: { credentials: CredentialRequest[] }) {
  const { t } = useTranslation();
  return (
    <div className="rounded-lg border border-cs-border bg-cs-card p-4 space-y-2">
      <header className="flex items-center gap-2">
        <KeyRound size={14} className="text-cs-warning" />
        <h4 className="text-xs font-semibold text-cs-text uppercase tracking-wide">
          {t("createAgent.guided.authNeeded", "Authentication needed")}
        </h4>
      </header>
      <p className="text-xs text-cs-muted">
        {t(
          "createAgent.guided.authNeededHint",
          "These tools need credentials before the agent can use them. Set them up in Settings → Secrets, or follow the link below."
        )}
      </p>
      <ul className="space-y-1.5">
        {credentials.map((c) => (
          <li
            key={c.envVar}
            className="rounded-md border border-cs-border bg-cs-bg-raised p-2.5 text-xs"
          >
            <div className="flex items-center justify-between gap-2">
              <span className="text-cs-text font-medium">{c.label}</span>
              <span className="inline-flex items-center gap-1 text-[10px] text-cs-muted">
                {c.kind === "oauth" ? <Globe size={10} /> : <KeyRound size={10} />}
                {c.kind}
              </span>
            </div>
            <code className="mt-1 block text-[10px] text-cs-muted">env: {c.envVar}</code>
            {c.note && <p className="mt-1 text-[11px] text-cs-muted">{c.note}</p>}
          </li>
        ))}
      </ul>
    </div>
  );
}
