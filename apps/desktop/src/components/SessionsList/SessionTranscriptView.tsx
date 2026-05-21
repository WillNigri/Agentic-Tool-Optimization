// SessionTranscriptView — full multi-turn session detail view.
//
// Extracted from SessionsList.tsx (2026-05-18 elegance push #2) so
// the parent file shrinks from ~2400 lines to a manageable orchestrator.
// Same component shape as SingleRunDetailView / WarRoomDetailView /
// ChatThreadDetailView — takes a session id + onBack callback,
// renders the full transcript with cost receipts + close affordance.
//
// All shared types + helpers come from `_helpers.ts` so this view
// stays self-contained.

import { useState, useRef, useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  ArrowLeft,
  Bot,
  GitBranch,
  Loader2,
  Sparkles,
  Send,
  X,
  Lock,
  Unlock,
  Tag,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { useProjectStore } from "@/stores/useProjectStore";
import { listAgents, type Agent } from "@/lib/agents";
import CloseConversationModal from "./CloseConversationModal";
import {
  runtimeBadge,
  formatTime,
  personaDisplay,
  personaBadge,
  runtimeDisplay,
  inferCoordinatorTarget,
  avatarInitials,
  NEW_SESSION_RUNTIMES,
  RUNTIME_COLORS,
  type SessionTranscript,
  type SessionCostBreakdown,
  type CloseSessionResult,
} from "./_helpers";

export default function SessionTranscriptView({
  sessionId,
  onBack,
}: {
  sessionId: string;
  onBack: () => void;
}) {
  const queryClient = useQueryClient();
  const q = useQuery<SessionTranscript>({
    queryKey: ["session-transcript", sessionId],
    queryFn: () =>
      invoke<SessionTranscript>("get_session_transcript", { sessionId }),
    staleTime: 5_000,
    refetchInterval: 10_000,
  });

  // 2026-05-16 — cost-receipts panel. Joined view from execution_logs
  // by session_id, grouped by (runtime, agent_slug). Same staleness as
  // the transcript so they refresh together when new turns land.
  const costQ = useQuery<SessionCostBreakdown>({
    queryKey: ["session-cost", sessionId],
    queryFn: () =>
      invoke<SessionCostBreakdown>("get_session_cost_breakdown", {
        sessionId,
      }),
    staleTime: 5_000,
    refetchInterval: 10_000,
  });

  const allRuntimes = Array.from(
    new Set((q.data?.turns ?? []).map((t) => t.runtime))
  );
  // Default the Continue picker to the runtime of the last assistant
  // turn — that's almost always what the user wants ("reply to whoever
  // just spoke"). Falls back to the session's anchor runtime when no
  // turns exist yet.
  const lastAssistant = q.data?.turns?.slice().reverse().find((t) => t.role === "assistant");
  const defaultContinueRuntime =
    lastAssistant?.runtime || q.data?.runtime || "claude";

  const [continueRuntime, setContinueRuntime] = useState(defaultContinueRuntime);
  const [continuePrompt, setContinuePrompt] = useState("");
  // v2.7.8 PR-3c — per-message agent override in the in-session
  // dispatcher. Empty string means "use the session's stored agent
  // (or none)". Will's dogfood: "only can select the llms in the
  // sessions not agents still" — the runtime select had no agent
  // partner. This list mirrors what FirstChatWizard's seat picker
  // shows, filtered by the currently-selected continueRuntime.
  const [continueAgent, setContinueAgent] = useState<string>("");
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const [bridging, setBridging] = useState(false);
  const [bridgeLog, setBridgeLog] = useState<string | null>(null);
  // v2.6 Slice C — close/reopen lifecycle. `closing` blocks the UI
  // with a modal while the coordinator LLM produces title/summary/tags
  // (typically 5–20s). closeError/reopenError are split so a failed
  // reopen doesn't get rendered with a "close failed" banner label,
  // and starting either action clears the other's stale message.
  const [closing, setClosing] = useState(false);
  const [closeError, setCloseError] = useState<string | null>(null);
  const [reopening, setReopening] = useState(false);
  const [reopenError, setReopenError] = useState<string | null>(null);
  // v2.7.12 — pre-close modal toggle. Opens when user clicks "Close
  // session"; closes when user submits / cancels / clicks the backdrop.
  // Distinct from `closing` (which gates the spinner overlay while the
  // backend dispatch is in flight).
  const [closeModalOpen, setCloseModalOpen] = useState(false);
  const isClosed = q.data?.status === "closed";
  // v2.3.48 — streaming buffer for the in-flight assistant turn.
  // Populated chunk-by-chunk from the Tauri `session-stream-chunk`
  // event; cleared on `session-stream-done` or send error.
  const [streamingText, setStreamingText] = useState("");
  const [streamingRuntime, setStreamingRuntime] = useState<string | null>(null);
  const streamingRef = useRef("");

  // Listen for streaming chunks scoped to this session. We filter on
  // sessionId because the chat pane elsewhere may stream concurrently.
  useEffect(() => {
    let unlistenChunk: UnlistenFn | undefined;
    let unlistenDone: UnlistenFn | undefined;
    (async () => {
      unlistenChunk = await listen<{ sessionId: string; text: string }>(
        "session-stream-chunk",
        (e) => {
          if (e.payload.sessionId !== sessionId) return;
          streamingRef.current += e.payload.text;
          setStreamingText(streamingRef.current);
        },
      );
      unlistenDone = await listen<{ sessionId: string }>(
        "session-stream-done",
        (e) => {
          if (e.payload.sessionId !== sessionId) return;
          streamingRef.current = "";
          setStreamingText("");
          setStreamingRuntime(null);
        },
      );
    })();
    return () => {
      unlistenChunk?.();
      unlistenDone?.();
    };
  }, [sessionId]);

  // Keep continueRuntime in sync when the transcript loads / a new
  // assistant turn lands — but never override if the user has manually
  // changed it during the same render lifecycle (initial value will
  // win on first render, manual change on subsequent ones).
  // Cheap heuristic: only auto-set when current value matches the
  // *previous* default, i.e. the user hasn't touched it.
  // (For a more careful sync we'd use a ref; this is good enough.)
  // Runtimes whose CLI streams via SSE (the api_providers crate's
  // registry). For these, we use the streaming Tauri command so
  // chunks render live in the transcript. Other runtimes (claude /
  // codex / gemini / hermes / openclaw — CLI subprocess dispatch)
  // don't yet emit JSONL chunks; fall back to the buffered path.
  const API_STREAMING_RUNTIMES = new Set([
    "minimax",
    "grok",
    "deepseek",
    "qwen",
    "openrouter",
  ]);

  // v2.7.8 PR-3c — agents for the per-message agent picker.
  const agentsQuery = useQuery({
    queryKey: ["agents"],
    queryFn: () => listAgents(),
  });
  const agentsForCurrentRuntime: Agent[] = (agentsQuery.data ?? []).filter(
    (a) => a.runtime === continueRuntime,
  );

  // v2.7.8 PR-3c dogfood — backend refuses tool-using API dispatches
  // without an explicit `workspace_root`. If no project is active in
  // the sidebar, the dispatch would error server-side ("Tool-using
  // API dispatch requires an explicit workspace_root"). Detect that
  // state here and disable Send + show a clear hint so the user
  // doesn't end up confused by a server error. Conservatively
  // checks: agent is picked AND runtime is an API provider — those
  // are the only paths that engage the tool loop.
  const activeProject = useProjectStore((s) => s.activeProject);
  const isApiProviderRuntime = (rt: string): boolean =>
    ["anthropic", "google", "minimax", "grok", "deepseek", "qwen", "openrouter"].includes(rt);
  const wouldEngageToolLoop = !!continueAgent && isApiProviderRuntime(continueRuntime);
  const blockedByMissingProject = wouldEngageToolLoop && !activeProject;

  const handleSend = async () => {
    if (!continuePrompt.trim() || sending) return;
    setSending(true);
    setSendError(null);
    const useStreaming = API_STREAMING_RUNTIMES.has(continueRuntime);
    streamingRef.current = "";
    setStreamingText("");
    setStreamingRuntime(useStreaming ? continueRuntime : null);
    try {
      // PR-3c — agentSlugOverride lets the per-message picker
      // override the session's stored agent_slug for THIS dispatch.
      // Backend defaults to the session's stored agent when this is
      // omitted, so today's "no picker" callers still work.
      const agentSlugOverride = continueAgent || null;
      if (useStreaming) {
        await invoke("dispatch_into_session_streaming", {
          runtime: continueRuntime,
          prompt: continuePrompt,
          sessionId,
          agentSlugOverride,
        });
      } else {
        await invoke("dispatch_into_session", {
          runtime: continueRuntime,
          prompt: continuePrompt,
          sessionId,
          agentSlugOverride,
        });
      }
      setContinuePrompt("");
      await queryClient.invalidateQueries({
        queryKey: ["session-transcript", sessionId],
      });
      await queryClient.invalidateQueries({ queryKey: ["sessions-full"] });
    } catch (e) {
      setSendError(String(e));
      streamingRef.current = "";
      setStreamingText("");
      setStreamingRuntime(null);
    } finally {
      setSending(false);
    }
  };

  // v2.7.12 — close is now two-step: the user picks coordinator +
  // optional human comment in CloseSessionModal first, then we invoke
  // the backend with those choices. Defaults preserve the pre-modal
  // behaviour (null coordinator → backend's auto-pick chain).
  const handleClose = async (opts: {
    coordinator: string | null;
    humanComment: string | null;
  }) => {
    if (closing) return;
    setCloseModalOpen(false);
    setClosing(true);
    setCloseError(null);
    setReopenError(null);
    try {
      await invoke<CloseSessionResult>("close_session", {
        sessionId,
        agentSlug: q.data?.agentSlug ?? null,
        model: null,
        coordinator: opts.coordinator,
        humanComment: opts.humanComment,
      });
      await queryClient.invalidateQueries({
        queryKey: ["session-transcript", sessionId],
      });
      await queryClient.invalidateQueries({ queryKey: ["sessions-full"] });
    } catch (e) {
      // The backend signals user-cancelled with the sentinel
      // "__cancelled__" so the UI doesn't render a "close failed"
      // banner — the user *meant* to abort. Any other error string
      // is surfaced as-is.
      const msg = String(e);
      if (!msg.includes("__cancelled__")) {
        setCloseError(msg);
      }
    } finally {
      setClosing(false);
    }
  };

  const handleCancelClose = async () => {
    // Fire and forget — the SIGTERM races with close_session's
    // wait_with_output, which then returns the cancelled-sentinel
    // error and unwinds the modal via the catch block above.
    try {
      await invoke("cancel_close_session", { sessionId });
    } catch {
      // Silent: if the cancel itself errors (e.g., subprocess
      // finished a millisecond ago), the close already succeeded or
      // failed on its own — no need for a separate error banner.
    }
  };

  const handleReopen = async () => {
    if (reopening) return;
    setReopening(true);
    setReopenError(null);
    setCloseError(null);
    try {
      await invoke("reopen_session", { sessionId });
      await queryClient.invalidateQueries({
        queryKey: ["session-transcript", sessionId],
      });
      await queryClient.invalidateQueries({ queryKey: ["sessions-full"] });
    } catch (e) {
      setReopenError(String(e));
    } finally {
      setReopening(false);
    }
  };

  const handleBridge = async () => {
    if (bridging) return;
    setBridging(true);
    setBridgeLog(null);
    try {
      const out = await invoke<string>("bridge_session", {
        sessionId,
        maxRounds: 3,
      });
      setBridgeLog(out);
      await queryClient.invalidateQueries({
        queryKey: ["session-transcript", sessionId],
      });
      await queryClient.invalidateQueries({ queryKey: ["sessions-full"] });
    } catch (e) {
      setBridgeLog(`Bridge failed: ${e}`);
    } finally {
      setBridging(false);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3 flex-wrap">
        <button
          onClick={onBack}
          className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border hover:bg-cs-border/30 transition-colors text-sm"
        >
          <ArrowLeft size={14} /> Back to sessions
        </button>
        {q.data && (
          <>
            <span className="text-sm font-medium text-cs-text">
              {q.data.autoTitle || q.data.title || (
                <span className="text-cs-muted italic">untitled</span>
              )}
            </span>
            {isClosed && (
              <span className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-cs-muted/20 text-cs-muted">
                <Lock size={10} /> closed
              </span>
            )}
            <div className="flex items-center gap-1">
              {allRuntimes.map((r) => (
                <span key={r} className={runtimeBadge(r)}>
                  {r}
                </span>
              ))}
            </div>
          </>
        )}
        <div className="ml-auto flex items-center gap-2">
          {isClosed ? (
            <button
              onClick={handleReopen}
              disabled={reopening}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border bg-cs-card hover:bg-cs-border/30 text-sm font-medium disabled:opacity-40 disabled:cursor-not-allowed"
              title="Reopen this session so you can continue the conversation. The next close will refresh the summary."
            >
              {reopening ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <Unlock size={14} />
              )}
              {reopening ? "Reopening…" : "Reopen"}
            </button>
          ) : (
            <button
              onClick={() => setCloseModalOpen(true)}
              disabled={closing || !q.data || q.data.turns.length === 0}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border bg-cs-card hover:bg-cs-border/30 text-sm font-medium disabled:opacity-40 disabled:cursor-not-allowed"
              title="Close this session. You'll pick the coordinator LLM and can attach a note before the conversation is summarized."
            >
              <Lock size={14} /> Close session
            </button>
          )}
          <button
            onClick={handleBridge}
            disabled={bridging || !q.data || q.data.turns.length === 0}
            className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-accent/40 bg-cs-accent/10 text-cs-accent text-sm font-medium hover:bg-cs-accent/20 disabled:opacity-40 disabled:cursor-not-allowed"
            title="Scan the last assistant turn for @mentions and bridge to those runtimes. Loops until [CONSENSUS] or 3 rounds."
          >
            <GitBranch size={14} />
            {bridging ? "Bridging…" : "Bridge"}
          </button>
        </div>
      </div>

      {/* Coordinator-generated summary banner. Only renders when the
          session is closed AND we have a summary. Tags render as chips
          underneath. The user can reopen with the button above to
          continue the conversation — the next close refreshes this. */}
      {q.data && isClosed && q.data.summary && (
        <div className="border border-cs-accent/30 rounded-md bg-cs-accent/5 p-3 space-y-2">
          <div className="text-xs font-medium uppercase text-cs-accent flex items-center gap-2">
            <Sparkles size={12} /> Coordinator summary
            {q.data.closedAt && (
              <span className="text-[10px] text-cs-muted normal-case font-normal">
                · closed {formatTime(q.data.closedAt)}
              </span>
            )}
          </div>
          <div className="text-sm text-cs-text whitespace-pre-wrap">
            {q.data.summary}
          </div>
          {/* v2.7.12 — human's free-form note. Rendered as a distinct
              sub-block so a glance separates LLM output from human
              framing. Skipped entirely when null/empty. */}
          {q.data.humanComment && q.data.humanComment.trim() && (
            <div className="border-t border-cs-accent/20 pt-2 mt-2">
              <div className="text-[10px] uppercase tracking-wider font-medium text-cs-muted mb-1">
                Note from human
              </div>
              <div className="text-sm text-cs-text whitespace-pre-wrap">
                {q.data.humanComment}
              </div>
            </div>
          )}
          {q.data.tags.length > 0 && (
            <div className="flex items-center gap-1 flex-wrap pt-1">
              <Tag size={10} className="text-cs-muted" />
              {q.data.tags.map((tag) => (
                <span
                  key={tag}
                  className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-cs-accent/10 text-cs-accent"
                >
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
      )}

      {/* v2.7.12 — pre-close modal. Opens before close_session is
          invoked so the user can pick the coordinator LLM and add a
          comment. Submit calls handleClose with the user's choices;
          the existing "Coordinator is summarizing…" blocker takes over
          from there. */}
      <CloseConversationModal
        open={closeModalOpen}
        busy={closing}
        conversationType="session"
        onCancel={() => setCloseModalOpen(false)}
        onSubmit={handleClose}
      />

      {/* Blocking close modal. While the coordinator runs, the UI is
          intentionally locked — the user picked "block with progress"
          over fire-and-forget so the new title/summary/tags are
          visible immediately when control returns. The Cancel button
          sends SIGTERM to the underlying `ato sessions close` process
          via cancel_close_session; the session stays 'open' and the
          modal closes without writing any summary. */}
      {closing && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-cs-bg/80 backdrop-blur-sm">
          <div className="border border-cs-border bg-cs-card rounded-lg p-6 max-w-md w-full mx-4 space-y-4">
            <div className="flex items-center gap-3">
              <Loader2
                size={20}
                className="animate-spin text-cs-accent shrink-0"
              />
              <div className="flex-1 min-w-0">
                <div className="text-sm font-medium text-cs-text">
                  Coordinator is summarizing…
                </div>
                <div className="text-xs text-cs-muted mt-1">
                  Generating title, summary, topic tags, and project
                  association from {q.data?.turns.length ?? 0} turn
                  {q.data && q.data.turns.length !== 1 ? "s" : ""}. Typically
                  5–20 seconds.
                </div>
              </div>
            </div>
            <div className="flex justify-end">
              <button
                onClick={handleCancelClose}
                className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border bg-cs-card hover:bg-cs-border/30 text-xs font-medium text-cs-muted hover:text-cs-text"
              >
                <X size={12} /> Cancel
              </button>
            </div>
          </div>
        </div>
      )}

      {closeError && (
        <div className="border border-cs-danger/40 bg-cs-danger/5 rounded-md p-3 text-sm text-cs-danger flex items-start gap-2">
          <span className="flex-1">
            <span className="font-medium">Close failed: </span>
            {closeError}
          </span>
          <button
            onClick={() => setCloseError(null)}
            className="text-cs-muted hover:text-cs-text"
            aria-label="dismiss"
          >
            <X size={14} />
          </button>
        </div>
      )}
      {reopenError && (
        <div className="border border-cs-danger/40 bg-cs-danger/5 rounded-md p-3 text-sm text-cs-danger flex items-start gap-2">
          <span className="flex-1">
            <span className="font-medium">Reopen failed: </span>
            {reopenError}
          </span>
          <button
            onClick={() => setReopenError(null)}
            className="text-cs-muted hover:text-cs-text"
            aria-label="dismiss"
          >
            <X size={14} />
          </button>
        </div>
      )}

      {bridgeLog && (
        <div className="border border-cs-accent/30 rounded-md bg-cs-accent/5 p-3 text-xs text-cs-text font-mono whitespace-pre-wrap relative">
          <button
            onClick={() => setBridgeLog(null)}
            className="absolute top-2 right-2 text-cs-muted hover:text-cs-text"
            aria-label="dismiss"
          >
            <X size={12} />
          </button>
          {bridgeLog}
        </div>
      )}

      {q.isLoading ? (
        <div className="flex items-center justify-center h-32">
          <Loader2 className="animate-spin text-cs-accent" size={24} />
        </div>
      ) : !q.data || q.data.turns.length === 0 ? (
        <div className="text-center py-12 text-cs-muted">
          <Sparkles size={36} className="mx-auto mb-3 opacity-50" />
          <p>No turns in this session yet.</p>
          <p className="text-xs mt-2">
            Dispatch into it with{" "}
            <code className="bg-cs-card px-1.5 py-0.5 rounded text-cs-text">
              ato dispatch &lt;runtime&gt; "..." --session {sessionId.slice(0, 8)}…
            </code>
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {q.data.turns.map((turn) => {
            // Sender resolution. For assistant turns, the speaker IS
            // the runtime. For user turns, we distinguish ato-orchestrator
            // prompts (auto-generated, e.g. `ato review`) from human-typed
            // dispatches (manual `ato dispatch <runtime> --session ...`).
            const isAssistant = turn.role === "assistant";
            const coordTarget = !isAssistant
              ? inferCoordinatorTarget(turn.text)
              : null;
            // 2026-05-16 — persona-aware speaker label. When a turn
            // was dispatched with `--agent <slug>`, the assistant
            // speaks AS the persona (e.g. "Positioning") rather than
            // as the raw runtime. The runtime stays visible in the
            // pill badge so users still see who answered underneath.
            // For user turns with a slug, label as "You → Positioning"
            // so the multi-seat war-room read order is legible.
            const personaLabel = turn.agentSlug
              ? personaDisplay(turn.agentSlug)
              : null;
            // Speaker = who's TALKING in this bubble.
            //   - assistant + persona:   "Positioning"
            //   - assistant generalist:  the responding runtime
            //   - user/coordinator:      "ATO Coordinator"
            //   - user/human:            "You"
            const speakerLabel = isAssistant
              ? personaLabel ?? runtimeDisplay(turn.runtime)
              : coordTarget !== null
              ? "ATO Coordinator"
              : "You";
            // Avatar bg color: themed by runtime for assistant; neutral
            // for human; coordinator-accent for orchestrator.
            const avatarColorCls = isAssistant
              ? RUNTIME_COLORS[turn.runtime] ?? "text-cs-muted bg-cs-border"
              : coordTarget !== null
              ? "text-cs-accent bg-cs-accent/15"
              : "text-cs-muted bg-cs-border";
            // Bubble border picks up the runtime tint for assistant
            // turns so back-to-back replies from different reviewers
            // visually contrast. Subtle for user turns.
            const runtimeTintClass = (
              RUNTIME_COLORS[turn.runtime] ?? "text-cs-muted"
            ).split(" ")[0]; // pull "text-X-400" → use for border
            const bubbleBorderCls = isAssistant
              ? cn("border", runtimeTintClass.replace("text-", "border-") + "/30")
              : "border border-cs-border";
            const bubbleBgCls = isAssistant
              ? cn(runtimeTintClass.replace("text-", "bg-") + "/5")
              : coordTarget !== null
              ? "bg-cs-accent/5"
              : "bg-cs-card";
            // WhatsApp alignment: human (you) right-aligned, everyone
            // else (assistants + coordinator-generated) left.
            const isYou = !isAssistant && coordTarget === null;
            return (
              <div
                key={turn.turnIndex}
                className={cn("flex gap-3", isYou && "flex-row-reverse")}
              >
                <div
                  className={cn(
                    "shrink-0 w-8 h-8 rounded-full flex items-center justify-center text-[10px] font-semibold",
                    avatarColorCls
                  )}
                  title={
                    isAssistant
                      ? `${speakerLabel} (${turn.runtime})`
                      : coordTarget !== null
                      ? `ATO Coordinator addressing @${coordTarget}`
                      : "You (manual dispatch)"
                  }
                >
                  {avatarInitials(speakerLabel)}
                </div>
                <div className={cn("flex-1 min-w-0", isYou && "text-right")}>
                  <div
                    className={cn(
                      "flex items-center gap-2 mb-1",
                      isYou && "justify-end"
                    )}
                  >
                    <span
                      className={cn(
                        "text-xs font-medium",
                        isAssistant
                          ? "text-cs-text"
                          : coordTarget !== null
                          ? "text-cs-accent"
                          : "text-cs-muted"
                      )}
                    >
                      {speakerLabel}
                    </span>
                    {coordTarget !== null && (
                      <span className="text-[11px] text-cs-muted">
                        →{" "}
                        <span className={runtimeBadge(coordTarget)}>
                          @{coordTarget}
                        </span>
                      </span>
                    )}
                    {isAssistant && (
                      <span className={runtimeBadge(turn.runtime)}>
                        {turn.runtime}
                      </span>
                    )}
                    <span className="text-[10px] text-cs-muted">
                      {formatTime(turn.createdAt)}
                    </span>
                  </div>
                  <pre
                    className={cn(
                      "p-3 rounded-md text-sm whitespace-pre-wrap font-sans text-left",
                      bubbleBgCls,
                      bubbleBorderCls
                    )}
                  >
                    {turn.text}
                  </pre>
                </div>
              </div>
            );
          })}
          {/* v2.3.48 — streaming placeholder turn. Renders while
              session-stream-chunk events are landing; cleared by
              session-stream-done + transcript refetch. The cursor
              signals "live". */}
          {/* 2026-05-16 — cost-receipts panel. Renders below the chat
              transcript whenever costQ has rows. Joined view of
              execution_logs by session_id grouped by (runtime,
              agent_slug). Highlights: cheapest model, total cost, per-
              seat breakdown. This is the "receipts" the Loom is about. */}
          {costQ.data && costQ.data.rows.length > 0 && (
            <div className="mt-6 border border-cs-border rounded-lg overflow-hidden">
              <div className="px-3 py-2 bg-cs-card border-b border-cs-border flex items-center justify-between">
                <span className="text-xs font-medium text-cs-text uppercase tracking-wide">
                  Receipts
                </span>
                <span className="text-xs text-cs-muted font-mono">
                  total{" "}
                  <span className="text-cs-accent">
                    {costQ.data.totalCostUsd === 0
                      ? "free (subscription)"
                      : `$${costQ.data.totalCostUsd.toFixed(4)}`}
                  </span>
                  {" · "}
                  {costQ.data.totalTurns} turn
                  {costQ.data.totalTurns !== 1 ? "s" : ""}
                  {" · "}
                  {(costQ.data.totalDurationMs / 1000).toFixed(1)}s
                  {" · "}
                  {(
                    costQ.data.totalTokensIn + costQ.data.totalTokensOut
                  ).toLocaleString()}{" "}
                  tok
                </span>
              </div>
              <div className="overflow-x-auto">
                <table className="w-full text-xs">
                  <thead className="text-cs-muted border-b border-cs-border bg-cs-card/40">
                    <tr>
                      <th className="text-left px-3 py-1.5 font-medium">
                        Runtime
                      </th>
                      <th className="text-left px-3 py-1.5 font-medium">
                        Seat
                      </th>
                      <th className="text-right px-3 py-1.5 font-medium">
                        Turns
                      </th>
                      <th className="text-right px-3 py-1.5 font-medium">
                        Tokens in
                      </th>
                      <th className="text-right px-3 py-1.5 font-medium">
                        Tokens out
                      </th>
                      <th className="text-right px-3 py-1.5 font-medium">
                        Duration
                      </th>
                      <th className="text-right px-3 py-1.5 font-medium">
                        Cost
                      </th>
                    </tr>
                  </thead>
                  <tbody className="font-mono">
                    {costQ.data.rows.map((row, i) => (
                      <tr
                        key={`${row.runtime}-${row.agentSlug ?? "_"}-${i}`}
                        className="border-b border-cs-border/40 last:border-0"
                      >
                        <td className="px-3 py-1.5">
                          <span className={runtimeBadge(row.runtime)}>
                            {row.runtime}
                          </span>
                        </td>
                        <td className="px-3 py-1.5">
                          {row.agentSlug ? (
                            <span className={personaBadge()}>
                              {personaDisplay(row.agentSlug)}
                            </span>
                          ) : (
                            <span className="text-cs-muted italic">
                              generalist
                            </span>
                          )}
                        </td>
                        <td className="text-right px-3 py-1.5">
                          {row.successfulTurns}
                          {row.totalTurns !== row.successfulTurns && (
                            <span
                              className="text-cs-muted ml-1"
                              title={`${row.totalTurns - row.successfulTurns} error turn(s)`}
                            >
                              (+
                              {row.totalTurns - row.successfulTurns}e)
                            </span>
                          )}
                        </td>
                        <td className="text-right px-3 py-1.5 text-cs-muted">
                          {(row.tokensIn ?? 0).toLocaleString()}
                        </td>
                        <td className="text-right px-3 py-1.5 text-cs-muted">
                          {(row.tokensOut ?? 0).toLocaleString()}
                        </td>
                        <td className="text-right px-3 py-1.5 text-cs-muted">
                          {((row.totalDurationMs ?? 0) / 1000).toFixed(1)}s
                        </td>
                        <td
                          className={cn(
                            "text-right px-3 py-1.5",
                            row.totalCostUsd === 0
                              ? "text-cs-muted"
                              : "text-cs-text"
                          )}
                          title={
                            row.billingMode === "subscription"
                              ? "Subscription auth (Claude Code / Codex CLI / Gemini CLI). No per-token billing — cost is the equivalent if you were paying per-token directly."
                              : row.billingMode === "local"
                              ? "Local runtime (Ollama / OpenClaw / Hermes). No network, no cost."
                              : row.costNullTurns > 0
                              ? `${row.costNullTurns} turn(s) had no cost computed — model missing from pricing table. Add the model's per-million rates in apps/cli/src/runtime.rs.`
                              : "Estimated from published per-token rates. Matches your provider's metered billing."
                          }
                        >
                          {row.costNullTurns > 0 ? (
                            <span className="text-amber-400">
                              $? <span className="text-[10px]">(pricing missing)</span>
                            </span>
                          ) : row.billingMode === "local" ? (
                            <span className="text-cs-muted">local</span>
                          ) : row.totalCostUsd === 0 ? (
                            row.billingMode === "subscription" ? (
                              <span className="text-cs-muted">subscription</span>
                            ) : (
                              <span className="text-cs-muted">$0.0000</span>
                            )
                          ) : row.billingMode === "subscription" ? (
                            <span>
                              <span className="text-cs-muted">≈ </span>
                              ${row.totalCostUsd.toFixed(4)}
                              <span className="text-[10px] text-cs-muted ml-1">
                                (sub est.)
                              </span>
                            </span>
                          ) : (
                            <>${row.totalCostUsd.toFixed(4)}</>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {/* Cheapest-success callout — only over genuinely-metered
                  (api_key) rows so we don't compare apples (subscription
                  estimate) to oranges (real billable). */}
              {(() => {
                const metered = costQ.data.rows.filter(
                  (r) =>
                    r.billingMode === "api_key" &&
                    r.totalCostUsd > 0 &&
                    r.successfulTurns > 0
                );
                if (metered.length < 2) return null;
                const cheapest = metered.reduce((a, b) =>
                  a.totalCostUsd < b.totalCostUsd ? a : b
                );
                return (
                  <div className="px-3 py-1.5 text-xs text-cs-muted border-t border-cs-border/40 bg-cs-card/40">
                    Cheapest metered:{" "}
                    <span className="text-cs-accent">{cheapest.runtime}</span>
                    {cheapest.agentSlug && (
                      <> as {personaDisplay(cheapest.agentSlug)}</>
                    )}{" "}
                    at ${cheapest.totalCostUsd.toFixed(4)}.
                  </div>
                );
              })()}
              {/* Caveat line. Always present so the reader knows the
                  cost numbers are estimates from a per-runtime pricing
                  table, not the provider's own bill. */}
              <div className="px-3 py-1.5 text-[10px] text-cs-muted border-t border-cs-border/40">
                Costs estimated from published per-runtime rates × tokens
                used. For metered providers (api_key) this should match
                your bill. For subscription runtimes this is the equivalent
                if you were paying per-token. "$?" means the model is
                missing from the pricing table — see{" "}
                <code className="text-cs-text">
                  apps/cli/src/runtime.rs:pricing_for_model
                </code>
                .
              </div>
            </div>
          )}

          {streamingText && streamingRuntime && (
            <div className="flex gap-3">
              <div className="shrink-0 w-8 h-8 rounded-full flex items-center justify-center bg-cs-accent/20 text-cs-accent">
                <Bot size={14} />
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  <span className="text-xs font-medium uppercase text-cs-accent">
                    assistant
                  </span>
                  <span className={runtimeBadge(streamingRuntime)}>
                    {streamingRuntime}
                  </span>
                  <span className="text-[10px] text-cs-muted animate-pulse">
                    streaming…
                  </span>
                </div>
                <pre className="p-3 rounded-md text-sm whitespace-pre-wrap font-sans border bg-cs-accent/5 border-cs-accent/20">
                  {streamingText}
                  <span className="animate-pulse">▎</span>
                </pre>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Continue conversation input — wired to dispatch_into_session.
          Always rendered so users can kick off the first turn of a
          freshly-created session or continue an existing one. When the
          session is closed, we disable the controls and prompt the
          user to reopen rather than silently dropping their input. */}
      <div className="border-t border-cs-border pt-4 mt-4">
        {isClosed && (
          <div className="mb-2 text-xs text-cs-muted flex items-center gap-2">
            <Lock size={12} />
            Session is closed. Reopen to continue — the next close will
            refresh the summary.
          </div>
        )}
        <div className="flex items-end gap-2">
          <select
            value={continueRuntime}
            onChange={(e) => {
              setContinueRuntime(e.target.value);
              // Reset agent when runtime changes — old agent
              // doesn't apply to the new runtime.
              setContinueAgent("");
            }}
            disabled={sending || isClosed}
            className="bg-cs-card border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
          >
            {NEW_SESSION_RUNTIMES.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>
          {/* v2.7.8 PR-3c — agent picker. "no agent" preserves
              today's behaviour (use session's stored agent, or
              text-only). Picking an agent overrides per message. */}
          <select
            value={continueAgent}
            onChange={(e) => setContinueAgent(e.target.value)}
            disabled={sending || isClosed || agentsForCurrentRuntime.length === 0}
            title={
              agentsForCurrentRuntime.length === 0
                ? `No agents created on '${continueRuntime}' yet.`
                : "Override the session's agent for this message."
            }
            className="bg-cs-card border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent disabled:opacity-50"
          >
            <option value="">— no agent —</option>
            {agentsForCurrentRuntime.map((a) => (
              <option key={a.id} value={a.slug}>
                {a.displayName} ({a.slug})
              </option>
            ))}
          </select>
          <textarea
            rows={2}
            value={continuePrompt}
            onChange={(e) => setContinuePrompt(e.target.value)}
            disabled={sending || isClosed}
            placeholder={
              isClosed
                ? "Reopen this session to send a message…"
                : q.data && q.data.turns.length === 0
                  ? "Send the first message…"
                  : "Continue the conversation…"
            }
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                handleSend();
              }
            }}
            className="flex-1 bg-cs-card border border-cs-border rounded-md px-3 py-2 text-sm font-sans resize-none focus:outline-none focus:border-cs-accent"
          />
          <button
            onClick={handleSend}
            disabled={
              !continuePrompt.trim() ||
              sending ||
              isClosed ||
              blockedByMissingProject
            }
            title={
              blockedByMissingProject
                ? "Pick a project in the left sidebar — tool-using API dispatches need a workspace root."
                : undefined
            }
            className="flex items-center gap-2 px-3 py-2 rounded-md bg-cs-accent text-cs-bg text-sm font-medium hover:opacity-90 disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {sending ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <Send size={14} />
            )}
            Send
          </button>
        </div>
        <div className="mt-1 text-[10px] text-cs-muted">
          ⌘/Ctrl + Enter to send. The dispatch routes via `ato dispatch &lt;runtime&gt; --session &lt;id&gt;`,
          so cross-runtime continuation just works (history is replayed for non-anchor runtimes).
        </div>
        {blockedByMissingProject && (
          <div className="mt-2 text-xs text-cs-warning bg-cs-warning/10 border border-cs-warning/40 rounded px-2 py-1">
            ⚠ Pick a project in the left sidebar — tool-using API
            dispatches (agent +{" "}
            <code className="text-cs-text">{continueRuntime}</code>) need a
            workspace root to sandbox the file reads.
          </div>
        )}
        {sendError && (
          <div className="mt-2 text-xs text-cs-danger">{sendError}</div>
        )}
      </div>
    </div>
  );
}
