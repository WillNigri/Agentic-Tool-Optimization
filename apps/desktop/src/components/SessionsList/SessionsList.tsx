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

import { useState, useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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
  Lock,
  Unlock,
  Tag,
  Search,
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
  // 2026-05-16 — distinct agent slugs that appeared on assistant turns
  // in this session, in first-spoken order. Empty when every dispatch
  // was a generalist (no --agent). Drives the persona-badge cluster
  // on the SessionsList card.
  agentsUsed: string[];
  // 2026-05-16 — session-total cost USD. NULL when no execution_logs
  // rows reference this session (pre-session-id-on-logs sessions). 0.0
  // when there are rows but all were on subscription (no metered cost).
  totalCostUsd: number | null;
  lastAssistantPreview: string | null;
  // v2.6 Slice C — lifecycle + coordinator-generated metadata.
  status: "open" | "closed";
  closedAt: string | null;
  autoTitle: string | null;
  summary: string | null;
  tags: string[];
  projectId: string | null;
}

interface SessionTurn {
  turnIndex: number;
  role: string;
  text: string;
  runtime: string;
  createdAt: string;
  // 2026-05-16 — null for generalist dispatches, slug otherwise.
  agentSlug: string | null;
}

// 2026-05-16 — cost-receipts panel data shape, mirrors the backend
// SessionCostBreakdown / SessionCostRow.
interface SessionCostRow {
  runtime: string;
  agentSlug: string | null;
  totalTurns: number;
  successfulTurns: number;
  tokensIn: number | null;
  tokensOut: number | null;
  totalDurationMs: number | null;
  costNullTurns: number;
  totalCostUsd: number;
  // 2026-05-16 — from execution_logs.auth_mode (authoritative per-row)
  // with a runtime-string fallback for pre-auth-mode rows.
  billingMode: string; // "subscription" | "api_key" | "local"
}

interface SessionCostBreakdown {
  sessionId: string;
  totalCostUsd: number;
  totalTurns: number;
  totalTokensIn: number;
  totalTokensOut: number;
  totalDurationMs: number;
  rows: SessionCostRow[];
}

interface SessionTranscript {
  id: string;
  runtime: string;
  agentSlug: string | null;
  title: string | null;
  turns: SessionTurn[];
  status: "open" | "closed";
  closedAt: string | null;
  autoTitle: string | null;
  summary: string | null;
  tags: string[];
  projectId: string | null;
}

interface CloseSessionResult {
  id: string;
  status: string;
  autoTitle: string | null;
  summary: string | null;
  tags: string[];
  projectId: string | null;
  coordinatorRuntime: string;
  coordinatorModel: string | null;
  durationMs: number;
}

const RUNTIME_COLORS: Record<string, string> = {
  claude: "text-orange-400 bg-orange-400/10",
  codex: "text-green-400 bg-green-400/10",
  gemini: "text-blue-400 bg-blue-400/10",
  google: "text-blue-400 bg-blue-400/10",
  hermes: "text-purple-400 bg-purple-400/10",
  openclaw: "text-cyan-400 bg-cyan-400/10",
  minimax: "text-pink-400 bg-pink-400/10",
  grok: "text-slate-400 bg-slate-400/10",
  deepseek: "text-indigo-400 bg-indigo-400/10",
  qwen: "text-amber-400 bg-amber-400/10",
  openrouter: "text-violet-400 bg-violet-400/10",
  anthropic: "text-orange-400 bg-orange-400/10",
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

// Pretty-name lookup for runtimes. Used in chat-bubble sender labels
// where "google" or "minimax" alone is opaque. Pairs with the model
// when known (e.g. "Google AI · Gemini 2.5 Flash"). Falls back to the
// capitalized runtime slug for unknown values.
const RUNTIME_DISPLAY: Record<string, string> = {
  claude: "Claude",
  codex: "OpenAI Codex",
  gemini: "Google Gemini",
  google: "Google Gemini",
  hermes: "Hermes",
  openclaw: "OpenClaw",
  minimax: "MiniMax",
  grok: "xAI Grok",
  deepseek: "DeepSeek",
  qwen: "Qwen",
  openrouter: "OpenRouter",
  anthropic: "Anthropic",
};

function runtimeDisplay(rt: string): string {
  return RUNTIME_DISPLAY[rt] ?? rt.replace(/^[a-z]/, (c) => c.toUpperCase());
}

// 2026-05-16 — persona slug → human label. "positioning" → "Positioning",
// "office-hours" → "Office Hours". Falls back to capitalized slug for
// custom personas users define (security-specialist → "Security Specialist").
function personaDisplay(slug: string): string {
  return slug
    .split("-")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

// Persona-badge styling for the SessionsList card cluster + chat-bubble
// role labels. Uses a single cyan-tinted treatment so the cluster reads
// as "these are the named seats that spoke" without competing with the
// per-turn runtime badges.
function personaBadge(): string {
  return "px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-cs-accent/10 text-cs-accent border border-cs-accent/20";
}

// Heuristic to detect when a `user`-role turn was authored by the
// `ato review` orchestrator (or another scripted dispatch) versus a
// human-typed prompt. The orchestrator's prompts have a predictable
// opener — "# Code review request for `<runtime>`" or "<runtime> —
// consensus round." — that we lean on to flip the rendered sender from
// "You" to "ATO Coordinator → @<addressee>". Best-effort: if neither
// pattern matches, treat as human input. (No false positives observed
// for human prose in 2026-05-15 dogfooding, but the regex is narrow
// enough to fix if one shows up.)
function inferCoordinatorTarget(text: string): string | null {
  const m1 = text.match(
    /^\s*#\s*Code review request for\s+`([a-z][a-z0-9_-]*)`/i
  );
  if (m1) return m1[1];
  const m2 = text.match(
    /^\s*([a-z][a-z0-9_-]*)\s+—\s+consensus round/i
  );
  if (m2) return m2[1];
  return null;
}

// Two-letter avatar from the speaker label. "MiniMax" → "Mi",
// "Google Gemini" → "GG", "ATO Coordinator" → "AC". Easier to scan
// in a chat list than a generic robot icon.
function avatarInitials(label: string): string {
  const words = label.split(/\s+/).filter(Boolean);
  if (words.length >= 2) {
    return (words[0][0] + words[1][0]).toUpperCase();
  }
  return label.slice(0, 2).toUpperCase();
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

type StatusFilter = "all" | "open" | "closed";

/// Case-insensitive substring search across every human-readable
/// field on a session row plus the 8-char id prefix. Returns the
/// filtered list; tokens that look like a single word are matched
/// individually so users can type "review consensus" and find rows
/// where both words appear somewhere.
function filterSessions(
  sessions: SessionListRow[],
  query: string,
  status: StatusFilter,
): SessionListRow[] {
  const trimmed = query.trim().toLowerCase();
  const tokens = trimmed.length === 0 ? [] : trimmed.split(/\s+/);
  return sessions.filter((s) => {
    if (status === "open" && s.status !== "open") return false;
    if (status === "closed" && s.status !== "closed") return false;
    if (tokens.length === 0) return true;
    // Build a single haystack string per row so each token can run a
    // cheap String.includes. Avoids re-allocating arrays inside the
    // inner loop. Limited to the fields a human would actually type.
    const haystack = [
      s.autoTitle ?? "",
      s.title ?? "",
      s.summary ?? "",
      s.lastAssistantPreview ?? "",
      s.agentSlug ?? "",
      s.tags.join(" "),
      s.runtimesUsed.join(" "),
      s.runtime,
      s.id.slice(0, 8),
    ]
      .join(" ")
      .toLowerCase();
    return tokens.every((t) => haystack.includes(t));
  });
}

export default function SessionsList() {
  const [openId, setOpenId] = useState<string | null>(null);
  const [showNew, setShowNew] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  // Debounce the search input by 300ms before firing the backend turn
  // search. Typing fast shouldn't fire a query on every keystroke —
  // the metadata filter runs instantly, the content search lags
  // slightly. State only updates when the user has stopped for the
  // debounce window.
  const [debouncedQuery, setDebouncedQuery] = useState("");
  useEffect(() => {
    const id = window.setTimeout(() => setDebouncedQuery(searchQuery), 300);
    return () => window.clearTimeout(id);
  }, [searchQuery]);

  const sessionsQ = useQuery<SessionListRow[]>({
    queryKey: ["sessions-full"],
    queryFn: () => invoke<SessionListRow[]>("list_sessions_full", { limit: 50 }),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  // Backend search across turn text. Returns the set of session ids
  // whose turns contain all the search tokens. Combined with the
  // metadata filter (client-side) via union: a row matches if it
  // either matches the metadata or appears in the content-match set.
  // Min length 2 (after trim) so a 1-char query doesn't hammer the
  // DB with a LIKE that matches almost every session.
  const contentSearchEnabled = debouncedQuery.trim().length >= 2;
  const turnSearchQ = useQuery<string[]>({
    queryKey: ["session-turn-search", debouncedQuery],
    queryFn: () =>
      invoke<string[]>("search_session_turns", { query: debouncedQuery }),
    enabled: contentSearchEnabled,
    staleTime: 30_000,
  });
  const contentMatchIds = new Set(
    contentSearchEnabled ? turnSearchQ.data ?? [] : [],
  );

  // Union of metadata matches and content-match ids. When the query
  // is empty, contentMatchIds is empty and the filter is metadata-only
  // (which itself is empty-query => "all").
  const filteredSessions = sessionsQ.data
    ? (() => {
        const metaMatched = filterSessions(
          sessionsQ.data,
          searchQuery,
          statusFilter,
        );
        if (!contentSearchEnabled || contentMatchIds.size === 0) {
          return metaMatched;
        }
        // Build a set of metadata-matched ids and union with content
        // matches (also gated by status filter to keep the chips
        // honest).
        const metaIds = new Set(metaMatched.map((s) => s.id));
        return sessionsQ.data.filter((s) => {
          if (statusFilter === "open" && s.status !== "open") return false;
          if (statusFilter === "closed" && s.status !== "closed") return false;
          return metaIds.has(s.id) || contentMatchIds.has(s.id);
        });
      })()
    : [];

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

      {/* Search + status filter. The input matches case-insensitively
          across title, summary, tags, agent slug, runtime names, and
          the 8-char id prefix — every field a human would type. The
          three status chips below let you scope to open or closed
          sessions; "All" is the default so the list looks the same as
          before by default. */}
      {sessionsQ.data && sessionsQ.data.length > 0 && (
        <div className="space-y-2">
          <div className="relative">
            <Search
              size={14}
              className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted pointer-events-none"
            />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search titles, summaries, tags, and words inside the conversations…"
              className="w-full bg-cs-card border border-cs-border rounded-md pl-9 pr-9 py-2 text-sm focus:outline-none focus:border-cs-accent placeholder:text-cs-muted"
            />
            {searchQuery && (
              <button
                onClick={() => setSearchQuery("")}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-cs-muted hover:text-cs-text"
                aria-label="clear search"
              >
                {turnSearchQ.isFetching ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : (
                  <X size={14} />
                )}
              </button>
            )}
          </div>
          <div className="flex items-center gap-2 text-xs">
            {(["all", "open", "closed"] as StatusFilter[]).map((s) => {
              const count =
                s === "all"
                  ? sessionsQ.data!.length
                  : sessionsQ.data!.filter((row) => row.status === s).length;
              return (
                <button
                  key={s}
                  onClick={() => setStatusFilter(s)}
                  className={cn(
                    "px-2 py-1 rounded-md border capitalize transition-colors",
                    statusFilter === s
                      ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                      : "border-cs-border bg-cs-card text-cs-muted hover:text-cs-text"
                  )}
                >
                  {s}
                  <span className="ml-1 opacity-60">({count})</span>
                </button>
              );
            })}
            {(searchQuery || statusFilter !== "all") && (
              <span className="text-cs-muted ml-auto">
                {filteredSessions.length} of {sessionsQ.data.length} shown
              </span>
            )}
          </div>
        </div>
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
      ) : filteredSessions.length === 0 ? (
        <div className="text-center py-12 text-cs-muted">
          <Search size={36} className="mx-auto mb-3 opacity-50" />
          <p>No sessions match your search.</p>
          <p className="text-xs mt-2">
            Try a different word, or clear the filter to see all{" "}
            {sessionsQ.data.length} sessions.
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {filteredSessions.map((s) => {
            // Prefer the coordinator-generated auto_title when present
            // (it's distilled from the actual conversation); fall back
            // to the user-supplied title, then to a muted "untitled".
            const displayTitle = s.autoTitle || s.title;
            // For closed sessions, the summary is a better preview than
            // the last assistant turn (which is often a tool result or
            // mid-thought fragment). For open sessions, keep the live
            // last-turn preview so users see what's happening now.
            const previewText =
              s.status === "closed" && s.summary
                ? s.summary
                : s.lastAssistantPreview;
            return (
              <button
                key={s.id}
                onClick={() => setOpenId(s.id)}
                className={cn(
                  "w-full text-left border rounded-lg transition-colors p-4",
                  s.status === "closed"
                    ? "border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40"
                    : "border-cs-border bg-cs-card hover:border-cs-accent/40 hover:bg-cs-border/20"
                )}
              >
                <div className="flex items-center gap-3 flex-wrap">
                  <div className="flex items-center gap-1">
                    {s.runtimesUsed.map((r) => (
                      <span
                        key={r}
                        className={runtimeBadge(r)}
                        title={
                          r === s.runtime
                            ? `Coordinator runtime: ${runtimeDisplay(r)}`
                            : `Participant: ${runtimeDisplay(r)}`
                        }
                      >
                        {r === s.runtime ? `★ ${r}` : r}
                      </span>
                    ))}
                  </div>
                  {/* 2026-05-16 — persona cluster. Renders the distinct
                      seat slugs that spoke in this session, in first-
                      spoken order. Empty (so the cluster is hidden) for
                      generalist-only sessions. */}
                  {s.agentsUsed.length > 0 && (
                    <div className="flex items-center gap-1">
                      {s.agentsUsed.map((slug) => (
                        <span
                          key={slug}
                          className={personaBadge()}
                          title={`Persona seat: ${personaDisplay(slug)}`}
                        >
                          {personaDisplay(slug)}
                        </span>
                      ))}
                    </div>
                  )}
                  {s.status === "closed" && (
                    <span
                      className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-cs-muted/20 text-cs-muted"
                      title={
                        s.closedAt
                          ? `Closed ${formatTime(s.closedAt)}`
                          : "Closed"
                      }
                    >
                      <Lock size={10} /> closed
                    </span>
                  )}
                  <span className="text-sm font-medium text-cs-text truncate flex-1 min-w-0">
                    {displayTitle || (
                      <span className="text-cs-muted italic">
                        untitled session
                      </span>
                    )}
                  </span>
                  <span className="text-xs text-cs-muted">
                    {s.turnCount} turn{s.turnCount !== 1 ? "s" : ""}
                  </span>
                  {/* 2026-05-16 — session-total cost pill. Shows the
                      summed cost from execution_logs.cost_usd_estimated.
                      Sum mixes metered + subscription-estimate rows; the
                      Receipts panel inside has the proper per-row labels.
                      Hidden when no execution_logs rows reference the
                      session (pre-session-id-on-logs). */}
                  {s.totalCostUsd !== null && s.totalCostUsd > 0 && (
                    <span
                      className="text-xs text-cs-muted font-mono"
                      title="Estimated session cost (sum of execution_logs). Open the session to see the per-runtime breakdown including which rows are metered API vs subscription-estimate."
                    >
                      ${s.totalCostUsd.toFixed(4)}
                    </span>
                  )}
                  <span className="text-xs text-cs-muted">
                    {formatTime(s.lastUsedAt)}
                  </span>
                </div>
                {/* 2026-05-16 — coordinator + project line. Coordinator
                    is the session's anchor runtime (where the session
                    was created). The session-level agent slug (when
                    set) is the agent the SESSION was anchored to —
                    separate from the per-turn personas in the cluster
                    above. Project shows which project the conversation
                    is scoped to. */}
                <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
                  <span>
                    coordinator:{" "}
                    <span className="text-cs-text">
                      {runtimeDisplay(s.runtime)}
                    </span>
                    {s.agentSlug && (
                      <>
                        {" / "}
                        <span className="text-cs-accent">
                          {personaDisplay(s.agentSlug)}
                        </span>
                      </>
                    )}
                  </span>
                  {s.projectId && (
                    <span>
                      project:{" "}
                      <span className="text-cs-text font-mono">
                        {s.projectId}
                      </span>
                    </span>
                  )}
                </div>
                {previewText && (
                  <div className="mt-2 text-xs text-cs-muted line-clamp-2">
                    {previewText}
                  </div>
                )}
                {s.tags.length > 0 && (
                  <div className="mt-2 flex items-center gap-1 flex-wrap">
                    <Tag size={10} className="text-cs-muted" />
                    {s.tags.map((tag) => (
                      <span
                        key={tag}
                        className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-cs-accent/10 text-cs-accent"
                      >
                        {tag}
                      </span>
                    ))}
                  </div>
                )}
                <div className="mt-2 text-[10px] text-cs-muted font-mono opacity-60 truncate">
                  {s.id}
                </div>
              </button>
            );
          })}
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

  const handleSend = async () => {
    if (!continuePrompt.trim() || sending) return;
    setSending(true);
    setSendError(null);
    const useStreaming = API_STREAMING_RUNTIMES.has(continueRuntime);
    streamingRef.current = "";
    setStreamingText("");
    setStreamingRuntime(useStreaming ? continueRuntime : null);
    try {
      if (useStreaming) {
        await invoke("dispatch_into_session_streaming", {
          runtime: continueRuntime,
          prompt: continuePrompt,
          sessionId,
        });
      } else {
        await invoke("dispatch_into_session", {
          runtime: continueRuntime,
          prompt: continuePrompt,
          sessionId,
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

  const handleClose = async () => {
    if (closing) return;
    setClosing(true);
    setCloseError(null);
    setReopenError(null);
    try {
      await invoke<CloseSessionResult>("close_session", {
        sessionId,
        agentSlug: q.data?.agentSlug ?? null,
        model: null,
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
              onClick={handleClose}
              disabled={closing || !q.data || q.data.turns.length === 0}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border bg-cs-card hover:bg-cs-border/30 text-sm font-medium disabled:opacity-40 disabled:cursor-not-allowed"
              title="Close this session. The coordinator agent will summarize the conversation, generate topic tags, and infer a project."
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
            onChange={(e) => setContinueRuntime(e.target.value)}
            disabled={sending || isClosed}
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
            disabled={!continuePrompt.trim() || sending || isClosed}
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
