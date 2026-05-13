import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ScrollText,
  RefreshCw,
  Loader2,
  CheckCircle,
  XCircle,
  Clock,
  ChevronDown,
  ChevronRight,
  Search,
  Filter,
  Circle,
  Pause,
  MessageSquare,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  getExecutionLogs,
  startLogWatcher,
  stopLogWatcher,
  isLogWatcherRunning,
  parseToolCallsSummary,
  type ExecutionLog,
} from "@/lib/api";

const STATUS_ICONS = {
  success: CheckCircle,
  error: XCircle,
  timeout: Clock,
};

const STATUS_COLORS = {
  success: "text-green-400",
  error: "text-red-400",
  timeout: "text-yellow-400",
};

const RUNTIME_COLORS: Record<string, string> = {
  claude: "text-orange-400 bg-orange-400/10",
  codex: "text-green-400 bg-green-400/10",
  hermes: "text-purple-400 bg-purple-400/10",
  openclaw: "text-cyan-400 bg-cyan-400/10",
};

export default function LogViewer() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [filterRuntime, setFilterRuntime] = useState<string>("");
  const [filterStatus, setFilterStatus] = useState<string>("");
  const [searchQuery, setSearchQuery] = useState("");
  const [isLive, setIsLive] = useState(false);
  const [newLogCount, setNewLogCount] = useState(0);

  // Fetch logs
  const { data: logs = [], isLoading, refetch, isFetching } = useQuery({
    queryKey: ["execution-logs", filterRuntime, filterStatus],
    queryFn: () => getExecutionLogs(
      filterRuntime || undefined,
      filterStatus || undefined,
      200
    ),
    refetchInterval: isLive ? false : 10000, // Disable auto-refresh when live
  });

  // Toggle live mode
  const toggleLive = useCallback(async () => {
    try {
      if (isLive) {
        await stopLogWatcher();
        setIsLive(false);
      } else {
        await startLogWatcher();
        setIsLive(true);
        setNewLogCount(0);
      }
    } catch (err) {
      console.error("Failed to toggle log watcher:", err);
    }
  }, [isLive]);

  // Check if watcher is already running on mount
  useEffect(() => {
    isLogWatcherRunning().then(setIsLive).catch(() => {});
  }, []);

  // Listen for real-time log entries
  useEffect(() => {
    if (!isLive) return;

    let unlisten: (() => void) | null = null;

    const setupListener = async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<Record<string, unknown>>("log-entry", (event) => {
          // Add the new log entry to the cache
          const newLog = event.payload as ExecutionLog;

          queryClient.setQueryData<ExecutionLog[]>(
            ["execution-logs", filterRuntime, filterStatus],
            (old) => {
              if (!old) return [newLog];
              // Prepend new log and keep max 200
              return [newLog, ...old].slice(0, 200);
            }
          );

          setNewLogCount((c) => c + 1);

          // Clear count after 3 seconds
          setTimeout(() => setNewLogCount((c) => Math.max(0, c - 1)), 3000);
        });
      } catch (err) {
        console.error("Failed to setup log listener:", err);
      }
    };

    setupListener();

    return () => {
      if (unlisten) unlisten();
    };
  }, [isLive, filterRuntime, filterStatus, queryClient]);

  // Filter by search
  const filteredLogs = logs.filter((log) => {
    if (!searchQuery) return true;
    const query = searchQuery.toLowerCase();
    return (
      log.prompt?.toLowerCase().includes(query) ||
      log.skillName?.toLowerCase().includes(query) ||
      log.errorMessage?.toLowerCase().includes(query)
    );
  });

  // v2.3.41 — group rows by sessionId so multi-turn conversations
  // collapse under one header instead of scattering. Each group is
  // either a SessionGroup (sessionId set, N rows in chronological
  // order) or a SoloLog (standalone). Logs come from the API in
  // newest-first order; sessions are placed at the newest turn's
  // position so the relative ordering stays "most recent activity
  // first."
  type SessionGroup = {
    kind: "session";
    sessionId: string;
    logs: ExecutionLog[]; // chronological, oldest → newest
    newestAt: string;
  };
  type SoloLog = { kind: "solo"; log: ExecutionLog };
  type Item = SessionGroup | SoloLog;

  const groupedItems: Item[] = (() => {
    const sessionAcc: Record<string, SessionGroup> = {};
    const result: Item[] = [];
    for (const log of filteredLogs) {
      const sid = log.sessionId;
      if (!sid) {
        result.push({ kind: "solo", log });
        continue;
      }
      if (!sessionAcc[sid]) {
        const group: SessionGroup = {
          kind: "session",
          sessionId: sid,
          logs: [],
          newestAt: log.createdAt,
        };
        sessionAcc[sid] = group;
        // Insert placeholder at the position of the newest turn
        // (which is the first one we see in the newest-first feed).
        result.push(group);
      }
      sessionAcc[sid].logs.push(log);
    }
    // Each session's logs were appended newest→oldest. Flip them so
    // chronology reads top-to-bottom inside the expander.
    for (const g of Object.values(sessionAcc)) {
      g.logs.reverse();
    }
    return result;
  })();

  const [expandedSessions, setExpandedSessions] = useState<Set<string>>(new Set());
  const toggleSession = (sid: string) => {
    setExpandedSessions((prev) => {
      const next = new Set(prev);
      if (next.has(sid)) next.delete(sid);
      else next.add(sid);
      return next;
    });
  };

  const formatDuration = (ms?: number) => {
    if (!ms) return "-";
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
  };

  const formatTokens = (tokensIn?: number, tokensOut?: number) => {
    if (!tokensIn && !tokensOut) return "-";
    return `${tokensIn || 0} / ${tokensOut || 0}`;
  };

  const formatTime = (isoString: string) => {
    const date = new Date(isoString);
    return date.toLocaleTimeString();
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="animate-spin text-cs-accent" size={32} />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <ScrollText className="text-cs-accent" size={24} />
            {t("logs.title", "Execution Logs")}
            {isLive && (
              <span className="flex items-center gap-1 text-xs font-normal text-green-400 bg-green-400/10 px-2 py-1 rounded">
                <span className="w-2 h-2 rounded-full bg-green-400 animate-pulse" />
                Live
                {newLogCount > 0 && <span>+{newLogCount}</span>}
              </span>
            )}
          </h2>
          <p className="text-sm text-cs-muted mt-1">
            {t("logs.subtitle", "View execution history across all runtimes")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={toggleLive}
            className={cn(
              "flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium transition-colors",
              isLive
                ? "bg-green-500/20 text-green-400 border border-green-500/30"
                : "border border-cs-border hover:bg-cs-border/50"
            )}
          >
            {isLive ? <Circle size={16} className="fill-current" /> : <Pause size={16} />}
            {isLive ? "Live" : "Paused"}
          </button>
          <button
            onClick={() => refetch()}
            disabled={isFetching}
            className="flex items-center gap-2 p-2 rounded-md border border-cs-border hover:bg-cs-border/50 transition-colors disabled:opacity-50"
          >
            <RefreshCw size={16} className={isFetching ? "animate-spin" : ""} />
          </button>
        </div>
      </div>

      {/* Filters */}
      <div className="flex items-center gap-4 flex-wrap">
        <div className="relative flex-1 min-w-[200px]">
          <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted" />
          <input
            type="text"
            placeholder="Search logs..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full pl-9 pr-4 py-2 rounded-md border border-cs-border bg-cs-card text-sm focus:outline-none focus:border-cs-accent"
          />
        </div>

        <div className="flex items-center gap-2">
          <Filter size={14} className="text-cs-muted" />
          <select
            value={filterRuntime}
            onChange={(e) => setFilterRuntime(e.target.value)}
            className="bg-cs-card border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
          >
            <option value="">All Runtimes</option>
            <option value="claude">Claude</option>
            <option value="codex">Codex</option>
            <option value="hermes">Hermes</option>
            <option value="openclaw">OpenClaw</option>
          </select>
          <select
            value={filterStatus}
            onChange={(e) => setFilterStatus(e.target.value)}
            className="bg-cs-card border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
          >
            <option value="">All Statuses</option>
            <option value="success">Success</option>
            <option value="error">Error</option>
            <option value="timeout">Timeout</option>
          </select>
        </div>

        <span className="text-xs text-cs-muted">
          {filteredLogs.length} log{filteredLogs.length !== 1 ? "s" : ""}
        </span>
      </div>

      {/* Logs list */}
      {filteredLogs.length === 0 ? (
        <div className="text-center py-12 text-cs-muted">
          <ScrollText size={48} className="mx-auto mb-4 opacity-50" />
          <p>No execution logs found</p>
          <p className="text-sm mt-1">Logs will appear here as you use your agents</p>
        </div>
      ) : (
        <div className="space-y-2">
          {groupedItems.map((item) => {
            // v2.3.41 — session group: render one collapsible header
            // summarizing the conversation, with each turn nested
            // inside (chronological top→bottom).
            if (item.kind === "session") {
              const isOpen = expandedSessions.has(item.sessionId);
              const runtimes = Array.from(new Set(item.logs.map((l) => l.runtime)));
              const totalDur = item.logs.reduce((s, l) => s + (l.durationMs ?? 0), 0);
              const totalTokIn = item.logs.reduce((s, l) => s + (l.tokensIn ?? 0), 0);
              const totalTokOut = item.logs.reduce((s, l) => s + (l.tokensOut ?? 0), 0);
              const firstPrompt = item.logs[0]?.prompt ?? "";
              const anyError = item.logs.some((l) => l.status === "error");
              return (
                <div
                  key={`session-${item.sessionId}`}
                  className="border border-cs-accent/30 rounded-lg bg-cs-accent/[0.03] overflow-hidden"
                >
                  <button
                    onClick={() => toggleSession(item.sessionId)}
                    className="w-full flex items-center gap-4 p-3 hover:bg-cs-border/30 transition-colors text-left"
                  >
                    <div className="flex items-center gap-2">
                      {isOpen ? (
                        <ChevronDown size={14} className="text-cs-muted" />
                      ) : (
                        <ChevronRight size={14} className="text-cs-muted" />
                      )}
                      <MessageSquare size={16} className={anyError ? "text-red-400" : "text-cs-accent"} />
                    </div>
                    <span className="text-xs font-medium text-cs-accent">
                      Session · {item.logs.length} turn{item.logs.length !== 1 ? "s" : ""}
                    </span>
                    <div className="flex items-center gap-1">
                      {runtimes.map((r) => (
                        <span
                          key={r}
                          className={cn(
                            "px-1.5 py-0.5 rounded text-xs font-medium capitalize",
                            RUNTIME_COLORS[r] || "text-cs-muted bg-cs-border"
                          )}
                        >
                          {r}
                        </span>
                      ))}
                    </div>
                    <span className="flex-1 text-sm truncate text-cs-muted">
                      {firstPrompt.slice(0, 60)}{firstPrompt.length > 60 ? "…" : ""}
                    </span>
                    <span className="text-xs text-cs-muted font-mono">
                      {formatDuration(totalDur)}
                    </span>
                    <span className="text-xs text-cs-muted font-mono w-20 text-right">
                      {totalTokIn} / {totalTokOut}
                    </span>
                    <span className="text-xs text-cs-muted w-20 text-right">
                      {formatTime(item.newestAt)}
                    </span>
                  </button>
                  {isOpen && (
                    <div className="border-t border-cs-accent/20 bg-cs-bg/40 px-3 py-2 space-y-2">
                      {item.logs.map((log) => {
                        const StatusIcon = STATUS_ICONS[log.status as keyof typeof STATUS_ICONS] || Clock;
                        const isTurnExpanded = expandedId === log.id;
                        return (
                          <div
                            key={log.id}
                            className="border border-cs-border/60 rounded-md bg-cs-card overflow-hidden"
                          >
                            <button
                              onClick={() => setExpandedId(isTurnExpanded ? null : log.id)}
                              className="w-full flex items-center gap-3 p-2.5 hover:bg-cs-border/30 transition-colors text-left"
                            >
                              <div className="flex items-center gap-2">
                                {isTurnExpanded ? (
                                  <ChevronDown size={12} className="text-cs-muted" />
                                ) : (
                                  <ChevronRight size={12} className="text-cs-muted" />
                                )}
                                <StatusIcon
                                  size={14}
                                  className={STATUS_COLORS[log.status as keyof typeof STATUS_COLORS] || "text-cs-muted"}
                                />
                              </div>
                              <span
                                className={cn(
                                  "px-1.5 py-0.5 rounded text-xs font-medium capitalize",
                                  RUNTIME_COLORS[log.runtime] || "text-cs-muted bg-cs-border"
                                )}
                              >
                                {log.runtime}
                              </span>
                              <span className="flex-1 text-sm truncate text-cs-muted">
                                {log.prompt ? log.prompt.slice(0, 80) + (log.prompt.length > 80 ? "…" : "") : <span className="italic">no prompt</span>}
                              </span>
                              <span className="text-xs text-cs-muted font-mono">{formatDuration(log.durationMs)}</span>
                              <span className="text-xs text-cs-muted font-mono w-20 text-right">{formatTokens(log.tokensIn, log.tokensOut)}</span>
                              <span className="text-xs text-cs-muted w-20 text-right">{formatTime(log.createdAt)}</span>
                            </button>
                            {isTurnExpanded && (
                              <div className="border-t border-cs-border/60 p-3 space-y-3 bg-cs-bg/60">
                                {log.prompt && (
                                  <div>
                                    <label className="text-xs text-cs-muted uppercase font-medium">Prompt</label>
                                    <pre className="mt-1 p-2 rounded bg-cs-card text-xs overflow-x-auto whitespace-pre-wrap">{log.prompt}</pre>
                                  </div>
                                )}
                                {log.status === "error" && log.errorMessage ? (
                                  <div>
                                    <label className="text-xs text-red-400 uppercase font-medium">Error</label>
                                    <pre className="mt-1 p-2 rounded bg-red-500/10 border border-red-500/30 text-xs text-red-400 overflow-x-auto whitespace-pre-wrap">{log.errorMessage}</pre>
                                  </div>
                                ) : log.response ? (
                                  <div>
                                    <label className="text-xs text-cs-muted uppercase font-medium">Response</label>
                                    <pre className="mt-1 p-2 rounded bg-cs-card text-xs overflow-x-auto whitespace-pre-wrap max-h-48 overflow-y-auto">{log.response}</pre>
                                  </div>
                                ) : null}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            }

            // Solo (non-session) log — keep original flat rendering.
            const log = item.log;
            const flatIndex = filteredLogs.indexOf(log);
            const StatusIcon = STATUS_ICONS[log.status as keyof typeof STATUS_ICONS] || Clock;
            const isExpanded = expandedId === log.id;
            const isNew = isLive && flatIndex < newLogCount;

            return (
              <div
                key={log.id}
                className={cn(
                  "border border-cs-border rounded-lg bg-cs-card overflow-hidden transition-all",
                  isNew && "ring-2 ring-green-400/50 bg-green-400/5"
                )}
              >
                {/* Header row */}
                <button
                  onClick={() => setExpandedId(isExpanded ? null : log.id)}
                  className="w-full flex items-center gap-4 p-3 hover:bg-cs-border/30 transition-colors text-left"
                >
                  <div className="flex items-center gap-2">
                    {isExpanded ? (
                      <ChevronDown size={14} className="text-cs-muted" />
                    ) : (
                      <ChevronRight size={14} className="text-cs-muted" />
                    )}
                    <StatusIcon
                      size={16}
                      className={STATUS_COLORS[log.status as keyof typeof STATUS_COLORS] || "text-cs-muted"}
                    />
                  </div>

                  <span
                    className={cn(
                      "px-2 py-0.5 rounded text-xs font-medium capitalize",
                      RUNTIME_COLORS[log.runtime] || "text-cs-muted bg-cs-border"
                    )}
                  >
                    {log.runtime}
                  </span>

                  <span className="flex-1 text-sm truncate">
                    {log.skillName ? (
                      <span className="text-cs-accent">/{log.skillName}</span>
                    ) : log.prompt ? (
                      <span className="text-cs-muted">{log.prompt.slice(0, 50)}...</span>
                    ) : (
                      <span className="text-cs-muted italic">No prompt</span>
                    )}
                  </span>

                  <span className="text-xs text-cs-muted font-mono">
                    {formatDuration(log.durationMs)}
                  </span>

                  <span className="text-xs text-cs-muted font-mono w-20 text-right">
                    {formatTokens(log.tokensIn, log.tokensOut)}
                  </span>

                  {/* v2.4.5 — Tier 2 audit badge. Null = no tools offered;
                       0 = tools offered but model chose prompt-only;
                       >0 = model verified via N tool calls. */}
                  {log.toolCallsCount != null && (
                    <span
                      className={cn(
                        "text-[10px] font-mono px-1.5 py-0.5 rounded border",
                        log.toolCallsCount === 0
                          ? "border-amber-500/40 text-amber-400 bg-amber-500/10"
                          : "border-cs-accent/40 text-cs-accent bg-cs-accent/10"
                      )}
                      title={
                        log.toolCallsCount === 0
                          ? "Prompt-only: tools were offered but the model didn't use them"
                          : `Verified via ${log.toolCallsCount} tool call(s)`
                      }
                    >
                      {log.toolCallsCount === 0
                        ? "prompt-only"
                        : `${log.toolCallsCount}×tools`}
                    </span>
                  )}

                  <span className="text-xs text-cs-muted w-20 text-right">
                    {formatTime(log.createdAt)}
                  </span>
                </button>

                {/* Expanded content */}
                {isExpanded && (
                  <div className="border-t border-cs-border p-4 space-y-4 bg-cs-bg">
                    {/* Prompt */}
                    {log.prompt && (
                      <div>
                        <label className="text-xs text-cs-muted uppercase font-medium">Prompt</label>
                        <pre className="mt-1 p-3 rounded bg-cs-card text-sm overflow-x-auto whitespace-pre-wrap">
                          {log.prompt}
                        </pre>
                      </div>
                    )}

                    {/* Response or Error */}
                    {log.status === "error" && log.errorMessage ? (
                      <div>
                        <label className="text-xs text-red-400 uppercase font-medium">Error</label>
                        <pre className="mt-1 p-3 rounded bg-red-500/10 border border-red-500/30 text-sm text-red-400 overflow-x-auto whitespace-pre-wrap">
                          {log.errorMessage}
                        </pre>
                      </div>
                    ) : log.response ? (
                      <div>
                        <label className="text-xs text-cs-muted uppercase font-medium">Response</label>
                        <pre className="mt-1 p-3 rounded bg-cs-card text-sm overflow-x-auto whitespace-pre-wrap max-h-64 overflow-y-auto">
                          {log.response}
                        </pre>
                      </div>
                    ) : null}

                    {/* v2.4.5 — Tool-call audit (Tier 2 reviews).
                         Lists every read_file / grep / git_log invocation
                         the LLM made before producing its reply, so the
                         human can tell "verified" from "guessed". */}
                    {log.toolCallsCount != null && (() => {
                      const entries = parseToolCallsSummary(log.toolCallsSummary);
                      return (
                        <div>
                          <label className="text-xs text-cs-muted uppercase font-medium">
                            Tool calls ({log.toolCallsCount})
                          </label>
                          {entries.length === 0 ? (
                            <p className="mt-1 text-xs text-amber-400">
                              Tools were offered but the model didn't invoke any —
                              its reply is based on the prompt alone.
                            </p>
                          ) : (
                            <ol className="mt-1 space-y-1">
                              {entries.map((e, i) => (
                                <li
                                  key={i}
                                  className={cn(
                                    "text-xs font-mono p-2 rounded border",
                                    e.isError
                                      ? "border-red-500/30 bg-red-500/5 text-red-300"
                                      : "border-cs-border bg-cs-card text-cs-muted"
                                  )}
                                >
                                  <span className="text-cs-accent">{e.name}</span>
                                  {" "}
                                  <span className="opacity-75">{e.argsBrief}</span>
                                  {e.isError && (
                                    <span className="ml-2 text-red-400">ERROR</span>
                                  )}
                                </li>
                              ))}
                            </ol>
                          )}
                        </div>
                      );
                    })()}

                    {/* Metadata */}
                    <div className="flex items-center gap-6 text-xs text-cs-muted">
                      <span>Duration: {formatDuration(log.durationMs)}</span>
                      <span>Tokens: {log.tokensIn || 0} in / {log.tokensOut || 0} out</span>
                      <span>Time: {new Date(log.createdAt).toLocaleString()}</span>
                      <span className="font-mono opacity-50">{log.id}</span>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
