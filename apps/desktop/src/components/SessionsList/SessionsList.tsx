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
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import {
  MessagesSquare,
  ArrowLeft,
  Bot,
  User as UserIcon,
  Loader2,
  Sparkles,
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

export default function SessionsList() {
  const [openId, setOpenId] = useState<string | null>(null);

  const sessionsQ = useQuery<SessionListRow[]>({
    queryKey: ["sessions-full"],
    queryFn: () => invoke<SessionListRow[]>("list_sessions_full", { limit: 50 }),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  if (openId) {
    return <SessionTranscriptView sessionId={openId} onBack={() => setOpenId(null)} />;
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-semibold flex items-center gap-2">
          <MessagesSquare className="text-cs-accent" size={24} />
          Sessions
        </h2>
        <p className="text-sm text-cs-muted mt-1">
          Sticky multi-turn conversations. Cross-runtime sessions (Phase 6 Slice B) show every
          runtime that contributed. Click a session to read the transcript.
        </p>
      </div>

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

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
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
      </div>

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
    </div>
  );
}
