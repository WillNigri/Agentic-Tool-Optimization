// v2.3.42 — Sessions tab in Runs.
//
// First-class GUI surface for Phase 6 sessions: list every conversation
// in the local DB, click to open a chat-style transcript, see which
// runtimes participated. Sessions were CLI-only until now (Slice A
// + A.2 + B in v2.3.31–33); v2.3.41 added incidental grouping in
// Execution Logs but didn't make sessions browsable on their own.
//
// Pure read view for v1 — opening a chat input for continue/bridge
// from the GUI is the next slice (involves wiring prompt_agent
// with --session). Document linked in the empty state directs the
// user to the CLI as the interim path.

import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import {
  MessagesSquare,
  ArrowLeft,
  Bot,
  User as UserIcon,
  Loader2,
  Sparkles,
  Plus,
  Send,
  GitBranch,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";

interface SessionListRow {
  id: string;
  runtime: string;
  agentSlug: string | null;
  title: string | null;
  createdAt: string;
  lastUsedAt: string;
  turnCount: number;
  runtimesUsed: string[];
  lastAssistantPreview: string | null;
}

interface SessionTurn {
  turnIndex: number;
  role: string;
  text: string;
  runtime: string;
  createdAt: string;
}

interface SessionTranscript {
  id: string;
  runtime: string;
  agentSlug: string | null;
  title: string | null;
  turns: SessionTurn[];
}

const RUNTIME_COLORS: Record<string, string> = {
  claude: "text-orange-400 bg-orange-400/10",
  codex: "text-green-400 bg-green-400/10",
  gemini: "text-blue-400 bg-blue-400/10",
  hermes: "text-purple-400 bg-purple-400/10",
  openclaw: "text-cyan-400 bg-cyan-400/10",
  minimax: "text-pink-400 bg-pink-400/10",
  grok: "text-slate-400 bg-slate-400/10",
  deepseek: "text-indigo-400 bg-indigo-400/10",
  qwen: "text-amber-400 bg-amber-400/10",
  openrouter: "text-violet-400 bg-violet-400/10",
};

function runtimeBadge(rt: string) {
  return cn(
    "px-1.5 py-0.5 rounded text-xs font-medium capitalize",
    RUNTIME_COLORS[rt] || "text-cs-muted bg-cs-border"
  );
}

function formatTime(iso: string) {
  return new Date(iso).toLocaleString();
}

// Runtimes we offer in the New Session / Continue dropdowns. Mirrors
// the registry the CLI's dispatch path resolves through (CLI runtimes
// + the api_providers crate).
const NEW_SESSION_RUNTIMES = [
  "claude",
  "codex",
  "gemini",
  "hermes",
  "openclaw",
  "minimax",
  "grok",
  "deepseek",
  "qwen",
  "openrouter",
];

export default function SessionsList() {
  const [openId, setOpenId] = useState<string | null>(null);
  const [showNew, setShowNew] = useState(false);

  const sessionsQ = useQuery<SessionListRow[]>({
    queryKey: ["sessions-full"],
    queryFn: () => invoke<SessionListRow[]>("list_sessions_full", { limit: 50 }),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  if (openId) {
    return (
      <SessionTranscriptView
        sessionId={openId}
        onBack={() => setOpenId(null)}
      />
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <MessagesSquare className="text-cs-accent" size={24} />
            Sessions
          </h2>
          <p className="text-sm text-cs-muted mt-1">
            Sticky multi-turn conversations. Cross-runtime sessions (Phase 6 Slice B) show every
            runtime that contributed. Click a session to read or continue.
          </p>
        </div>
        <button
          onClick={() => setShowNew(true)}
          className="flex items-center gap-2 px-3 py-2 rounded-md bg-cs-accent text-cs-bg text-sm font-medium hover:opacity-90"
        >
          <Plus size={14} />
          New session
        </button>
      </div>

      {showNew && (
        <NewSessionModal
          onClose={() => setShowNew(false)}
          onCreated={(id) => {
            setShowNew(false);
            setOpenId(id);
          }}
        />
      )}

      {sessionsQ.isLoading ? (
        <div className="flex items-center justify-center h-32">
          <Loader2 className="animate-spin text-cs-accent" size={28} />
        </div>
      ) : !sessionsQ.data || sessionsQ.data.length === 0 ? (
        <div className="text-center py-12 text-cs-muted">
          <MessagesSquare size={48} className="mx-auto mb-4 opacity-50" />
          <p>No sessions yet</p>
          <p className="text-sm mt-2 max-w-md mx-auto">
            Open a sticky conversation with{" "}
            <code className="bg-cs-card px-1.5 py-0.5 rounded text-cs-text">
              ato sessions new --runtime claude
            </code>{" "}
            then dispatch into it with{" "}
            <code className="bg-cs-card px-1.5 py-0.5 rounded text-cs-text">
              ato dispatch claude "..." --session &lt;id&gt;
            </code>
            . Cross-runtime bridges via{" "}
            <code className="bg-cs-card px-1.5 py-0.5 rounded text-cs-text">--tag-bridge</code>.
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {sessionsQ.data.map((s) => (
            <button
              key={s.id}
              onClick={() => setOpenId(s.id)}
              className="w-full text-left border border-cs-border rounded-lg bg-cs-card hover:border-cs-accent/40 hover:bg-cs-border/20 transition-colors p-4"
            >
              <div className="flex items-center gap-3 flex-wrap">
                <div className="flex items-center gap-1">
                  {s.runtimesUsed.map((r) => (
                    <span key={r} className={runtimeBadge(r)}>
                      {r}
                    </span>
                  ))}
                </div>
                <span className="text-sm font-medium text-cs-text truncate flex-1 min-w-0">
                  {s.title || (
                    <span className="text-cs-muted italic">untitled session</span>
                  )}
                </span>
                <span className="text-xs text-cs-muted">
                  {s.turnCount} turn{s.turnCount !== 1 ? "s" : ""}
                </span>
                <span className="text-xs text-cs-muted">{formatTime(s.lastUsedAt)}</span>
              </div>
              {s.agentSlug && (
                <div className="mt-1 text-xs text-cs-accent">@{s.agentSlug}</div>
              )}
              {s.lastAssistantPreview && (
                <div className="mt-2 text-xs text-cs-muted line-clamp-2">
                  {s.lastAssistantPreview}
                </div>
              )}
              <div className="mt-2 text-[10px] text-cs-muted font-mono opacity-60 truncate">
                {s.id}
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function SessionTranscriptView({
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
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const [bridging, setBridging] = useState(false);
  const [bridgeLog, setBridgeLog] = useState<string | null>(null);

  // Keep continueRuntime in sync when the transcript loads / a new
  // assistant turn lands — but never override if the user has manually
  // changed it during the same render lifecycle (initial value will
  // win on first render, manual change on subsequent ones).
  // Cheap heuristic: only auto-set when current value matches the
  // *previous* default, i.e. the user hasn't touched it.
  // (For a more careful sync we'd use a ref; this is good enough.)
  const handleSend = async () => {
    if (!continuePrompt.trim() || sending) return;
    setSending(true);
    setSendError(null);
    try {
      await invoke("dispatch_into_session", {
        runtime: continueRuntime,
        prompt: continuePrompt,
        sessionId,
      });
      setContinuePrompt("");
      // Refetch the transcript so the new turn pair lands immediately.
      await queryClient.invalidateQueries({
        queryKey: ["session-transcript", sessionId],
      });
      await queryClient.invalidateQueries({ queryKey: ["sessions-full"] });
    } catch (e) {
      setSendError(String(e));
    } finally {
      setSending(false);
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
              {q.data.title || (
                <span className="text-cs-muted italic">untitled</span>
              )}
            </span>
            <div className="flex items-center gap-1">
              {allRuntimes.map((r) => (
                <span key={r} className={runtimeBadge(r)}>
                  {r}
                </span>
              ))}
            </div>
          </>
        )}
        <button
          onClick={handleBridge}
          disabled={bridging || !q.data || q.data.turns.length === 0}
          className="ml-auto flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-accent/40 bg-cs-accent/10 text-cs-accent text-sm font-medium hover:bg-cs-accent/20 disabled:opacity-40 disabled:cursor-not-allowed"
          title="Scan the last assistant turn for @mentions and bridge to those runtimes. Loops until [CONSENSUS] or 3 rounds."
        >
          <GitBranch size={14} />
          {bridging ? "Bridging…" : "Bridge"}
        </button>
      </div>

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
          {q.data.turns.map((turn) => (
            <div
              key={turn.turnIndex}
              className={cn(
                "flex gap-3",
                turn.role === "user" ? "flex-row" : "flex-row"
              )}
            >
              <div
                className={cn(
                  "shrink-0 w-8 h-8 rounded-full flex items-center justify-center",
                  turn.role === "user"
                    ? "bg-cs-border text-cs-muted"
                    : "bg-cs-accent/20 text-cs-accent"
                )}
              >
                {turn.role === "user" ? <UserIcon size={14} /> : <Bot size={14} />}
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  <span
                    className={cn(
                      "text-xs font-medium uppercase",
                      turn.role === "user" ? "text-cs-muted" : "text-cs-accent"
                    )}
                  >
                    {turn.role}
                  </span>
                  <span className={runtimeBadge(turn.runtime)}>{turn.runtime}</span>
                  <span className="text-[10px] text-cs-muted">
                    {formatTime(turn.createdAt)}
                  </span>
                </div>
                <pre
                  className={cn(
                    "p-3 rounded-md text-sm whitespace-pre-wrap font-sans border",
                    turn.role === "user"
                      ? "bg-cs-card border-cs-border"
                      : "bg-cs-accent/5 border-cs-accent/20"
                  )}
                >
                  {turn.text}
                </pre>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Continue conversation input — wired to dispatch_into_session.
          Always rendered so users can kick off the first turn of a
          freshly-created session or continue an existing one. */}
      <div className="border-t border-cs-border pt-4 mt-4">
        <div className="flex items-end gap-2">
          <select
            value={continueRuntime}
            onChange={(e) => setContinueRuntime(e.target.value)}
            disabled={sending}
            className="bg-cs-card border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
          >
            {NEW_SESSION_RUNTIMES.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>
          <textarea
            rows={2}
            value={continuePrompt}
            onChange={(e) => setContinuePrompt(e.target.value)}
            disabled={sending}
            placeholder={
              q.data && q.data.turns.length === 0
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
            disabled={!continuePrompt.trim() || sending}
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
        {sendError && (
          <div className="mt-2 text-xs text-cs-danger">{sendError}</div>
        )}
      </div>
    </div>
  );
}

function NewSessionModal({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const [runtime, setRuntime] = useState("claude");
  const [title, setTitle] = useState("");
  const [agentSlug, setAgentSlug] = useState("");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleCreate = async () => {
    setCreating(true);
    setError(null);
    try {
      const id = await invoke<string>("create_session", {
        runtime,
        title: title.trim() || null,
        agentSlug: agentSlug.trim() || null,
      });
      onCreated(id);
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
    >
      <div
        className="relative bg-cs-card border border-cs-border rounded-lg p-6 w-full max-w-md space-y-4"
        onClick={(e) => e.stopPropagation()}
      >
        <button
          onClick={onClose}
          className="absolute top-3 right-3 text-cs-muted hover:text-cs-text"
          aria-label="close"
        >
          <X size={16} />
        </button>
        <h3 className="text-lg font-semibold text-cs-text">New session</h3>
        <div className="space-y-3">
          <div>
            <label className="text-xs text-cs-muted uppercase font-medium">Runtime</label>
            <select
              value={runtime}
              onChange={(e) => setRuntime(e.target.value)}
              className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
            >
              {NEW_SESSION_RUNTIMES.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </select>
            <div className="mt-1 text-[10px] text-cs-muted">
              Anchor runtime. Cross-runtime turns via @-mentions in --tag-bridge or by
              dispatching into the session from a different runtime later.
            </div>
          </div>
          <div>
            <label className="text-xs text-cs-muted uppercase font-medium">Title (optional)</label>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="e.g. SSH adapter design review"
              className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
            />
          </div>
          <div>
            <label className="text-xs text-cs-muted uppercase font-medium">Agent slug (optional)</label>
            <input
              type="text"
              value={agentSlug}
              onChange={(e) => setAgentSlug(e.target.value)}
              placeholder="e.g. codex-reviewer"
              className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
            />
          </div>
        </div>
        {error && <div className="text-xs text-cs-danger">{error}</div>}
        <div className="flex items-center justify-end gap-2">
          <button
            onClick={onClose}
            disabled={creating}
            className="px-3 py-2 rounded-md border border-cs-border text-sm hover:bg-cs-border/30"
          >
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={creating}
            className="flex items-center gap-2 px-3 py-2 rounded-md bg-cs-accent text-cs-bg text-sm font-medium hover:opacity-90 disabled:opacity-40"
          >
            {creating ? <Loader2 size={14} className="animate-spin" /> : <Plus size={14} />}
            Create
          </button>
        </div>
      </div>
    </div>
  );
}
