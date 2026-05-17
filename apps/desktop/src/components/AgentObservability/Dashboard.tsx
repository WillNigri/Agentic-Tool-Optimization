import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Activity,
  Loader2,
  AlertCircle,
  Clock,
  Search,
  CheckCircle2,
  XCircle,
  ChevronDown,
  ChevronRight,
  RefreshCw,
} from "lucide-react";
import {
  getAgentMetrics,
  readAgentTraces,
  type AgentTraceLine,
  type AgentTraceFilter,
} from "@/lib/agentObservability";
import { cn } from "@/lib/utils";
import TraceExplorer from "./TraceExplorer";

// v1.4.0 F6 — Agent observability dashboard.
// Free: reads ~/.ato/agent-logs.jsonl (no cap on local lines, just the local
// file's contents). Pro/Team/Enterprise unlock cloud retention + aggregations
// across devices — that's a separate Wave 4.x integration.

const RUNTIME_DOT: Record<string, string> = {
  claude: "bg-orange-500",
  codex: "bg-green-500",
  gemini: "bg-blue-500",
  openclaw: "bg-cyan-400",
  hermes: "bg-purple-500",
};

export default function Dashboard() {
  const { t } = useTranslation();
  const [filter, setFilter] = useState<AgentTraceFilter>({ status: "all", limit: 100 });
  const [search, setSearch] = useState("");
  const [exploringTrace, setExploringTrace] = useState<AgentTraceLine | null>(null);
  const [expandedAgent, setExpandedAgent] = useState<string | null>(null);

  // v1.6.0 — soft-handoff from Automations canvas. When the user
  // clicks "View runs" on a flow node, AutomationFlow drops the
  // agent slug here; we pick it up on mount, expand that row, and
  // clear the hint so refresh-without-handoff doesn't re-trigger.
  useEffect(() => {
    try {
      const slug = localStorage.getItem("ato.insights.preselectAgentSlug");
      if (slug) {
        setExpandedAgent(slug);
        localStorage.removeItem("ato.insights.preselectAgentSlug");
      }
    } catch {
      // localStorage unavailable — non-fatal.
    }
  }, []);

  const { data: metrics, isLoading, error, refetch, isFetching } = useQuery({
    queryKey: ["agent-metrics", filter],
    queryFn: () => getAgentMetrics(filter),
    staleTime: 5_000,
    refetchInterval: 30_000,
  });

  const { data: traces = [] } = useQuery({
    queryKey: ["agent-traces", filter, expandedAgent],
    queryFn: () =>
      readAgentTraces({
        ...filter,
        agentSlug: expandedAgent ?? filter.agentSlug,
      }),
    staleTime: 5_000,
    enabled: !!metrics,
  });

  const filteredTraces = search.trim()
    ? traces.filter((t) =>
        [t.slug, t.responsePreview, t.promptPreview, t.error]
          .filter(Boolean)
          .some((s) => (s as string).toLowerCase().includes(search.toLowerCase()))
      )
    : traces;

  return (
    <div className="space-y-5">
      <header className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Activity size={16} className="text-cs-accent" />
            <h3 className="text-sm font-medium text-cs-text">
              {t("observability.title", "Agent observability")}
            </h3>
          </div>
          <p className="mt-1 text-xs text-cs-muted max-w-2xl">
            {t(
              "observability.subtitle",
              "Every dispatch — from the desktop Run button, Quick Test, MCP run_agent, group routing, ato dispatch CLI, or cron — lands in the local execution_logs table. This dashboard reads it directly with full agent / runtime / cost / status detail. Cloud retention + cross-device aggregation are Pro features."
            )}
          </p>
        </div>
        <button
          type="button"
          onClick={() => refetch()}
          disabled={isFetching}
          className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-bg-raised px-2.5 py-1 text-xs text-cs-text hover:border-cs-hover disabled:opacity-50 shrink-0"
        >
          <RefreshCw size={11} className={isFetching ? "animate-spin" : ""} />
          {t("common.refresh", "Refresh")}
        </button>
      </header>

      {/* Filters */}
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => setFilter((f) => ({ ...f, status: "all" }))}
          className={chipClass(filter.status === "all" || !filter.status)}
        >
          {t("observability.filter.all", "All")}
        </button>
        <button
          type="button"
          onClick={() => setFilter((f) => ({ ...f, status: "ok" }))}
          className={chipClass(filter.status === "ok")}
        >
          <CheckCircle2 size={11} />
          {t("observability.filter.ok", "Successful")}
        </button>
        <button
          type="button"
          onClick={() => setFilter((f) => ({ ...f, status: "error" }))}
          className={chipClass(filter.status === "error")}
        >
          <XCircle size={11} />
          {t("observability.filter.error", "Errors")}
        </button>
        <div className="relative flex-1 min-w-[200px]">
          <Search size={11} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-cs-muted" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("observability.searchPlaceholder", "Search traces…")}
            className="w-full rounded-md border border-cs-border bg-cs-bg pl-7 pr-2.5 py-1 text-xs text-cs-text focus:border-cs-accent focus:outline-none"
          />
        </div>
      </div>

      {error && (
        <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
          <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
          <span>{error instanceof Error ? error.message : String(error)}</span>
        </div>
      )}

      {/* Top-line metrics */}
      {metrics && (
        <div className="grid grid-cols-4 gap-3">
          <MetricCard
            label={t("observability.totalRuns", "Total runs")}
            value={metrics.totalRuns.toString()}
          />
          <MetricCard
            label={t("observability.successRate", "Success rate")}
            value={metrics.totalRuns > 0 ? `${Math.round(metrics.successRate * 100)}%` : "—"}
            tone={
              metrics.totalRuns === 0
                ? "neutral"
                : metrics.successRate >= 0.95
                ? "good"
                : metrics.successRate >= 0.8
                ? "ok"
                : "bad"
            }
          />
          <MetricCard
            label={t("observability.p50", "p50 latency")}
            value={metrics.p50LatencyMs !== null ? `${metrics.p50LatencyMs}ms` : "—"}
          />
          <MetricCard
            label={t("observability.p95", "p95 latency")}
            value={metrics.p95LatencyMs !== null ? `${metrics.p95LatencyMs}ms` : "—"}
          />
        </div>
      )}

      {isLoading && !metrics ? (
        <div className="flex items-center justify-center h-32">
          <Loader2 size={20} className="animate-spin text-cs-muted" />
        </div>
      ) : metrics?.totalRuns === 0 ? (
        <EmptyState />
      ) : (
        <>
          {/* Per-agent table */}
          {metrics && metrics.perAgent.length > 0 && (
            <section>
              <h4 className="text-[10px] uppercase tracking-wide text-cs-muted mb-2">
                {t("observability.perAgent", "Per agent")}
              </h4>
              <div className="rounded-lg border border-cs-border bg-cs-card overflow-hidden">
                {metrics.perAgent.map((a) => (
                  <button
                    key={a.slug}
                    type="button"
                    onClick={() =>
                      setExpandedAgent((cur) => (cur === a.slug ? null : a.slug))
                    }
                    className={cn(
                      "w-full flex items-center gap-3 px-3 py-2 text-left text-xs hover:bg-cs-bg-raised transition border-t first:border-t-0 border-cs-border",
                      expandedAgent === a.slug && "bg-cs-bg-raised"
                    )}
                  >
                    {expandedAgent === a.slug ? (
                      <ChevronDown size={11} className="text-cs-muted" />
                    ) : (
                      <ChevronRight size={11} className="text-cs-muted" />
                    )}
                    {a.runtime && (
                      <span
                        className={cn(
                          "inline-block w-1.5 h-1.5 rounded-full shrink-0",
                          RUNTIME_DOT[a.runtime] ?? "bg-cs-muted"
                        )}
                      />
                    )}
                    <code className="font-mono text-cs-text shrink-0">{a.slug}</code>
                    <span className="text-cs-muted">
                      {a.totalRuns} {t("observability.runs", "runs")}
                    </span>
                    <span
                      className={cn(
                        "ml-auto text-[10px]",
                        a.successRate >= 0.95
                          ? "text-cs-accent"
                          : a.successRate >= 0.8
                          ? "text-cs-warning"
                          : "text-cs-danger"
                      )}
                    >
                      {Math.round(a.successRate * 100)}%
                    </span>
                    {a.p50LatencyMs !== null && (
                      <span className="text-cs-muted text-[10px] tabular-nums">
                        p50 {a.p50LatencyMs}ms
                      </span>
                    )}
                  </button>
                ))}
              </div>
            </section>
          )}

          {/* Trace list */}
          <section>
            <h4 className="text-[10px] uppercase tracking-wide text-cs-muted mb-2">
              {t("observability.recentTraces", "Recent traces")}
              {expandedAgent && (
                <span className="ml-2 text-cs-text">
                  ({t("observability.scoped", "scoped to")} {expandedAgent})
                </span>
              )}
            </h4>
            <div className="space-y-1.5">
              {filteredTraces.length === 0 ? (
                <p className="rounded-md border border-dashed border-cs-border bg-cs-bg-raised/40 p-4 text-xs text-cs-muted text-center">
                  {t("observability.noMatching", "No traces match the current filters.")}
                </p>
              ) : (
                filteredTraces.map((trace, i) => (
                  <TraceRow key={i} trace={trace} onClick={() => setExploringTrace(trace)} />
                ))
              )}
            </div>
          </section>
        </>
      )}

      {exploringTrace && (
        <TraceExplorer trace={exploringTrace} onClose={() => setExploringTrace(null)} />
      )}
    </div>
  );
}

function MetricCard({
  label,
  value,
  tone = "neutral",
}: {
  label: string;
  value: string;
  tone?: "neutral" | "good" | "ok" | "bad";
}) {
  const toneClass =
    tone === "good"
      ? "text-cs-accent"
      : tone === "ok"
      ? "text-cs-warning"
      : tone === "bad"
      ? "text-cs-danger"
      : "text-cs-text";
  return (
    <div className="rounded-lg border border-cs-border bg-cs-card p-3">
      <div className="text-[10px] uppercase tracking-wide text-cs-muted">{label}</div>
      <div className={cn("mt-1 text-xl font-semibold tabular-nums", toneClass)}>{value}</div>
    </div>
  );
}

function TraceRow({ trace, onClick }: { trace: AgentTraceLine; onClick: () => void }) {
  const ok = trace.ok !== false;
  return (
    <button
      type="button"
      onClick={onClick}
      className="w-full flex items-center gap-3 rounded-md border border-cs-border bg-cs-card px-3 py-2 text-left text-xs hover:border-cs-hover transition"
    >
      {ok ? (
        <CheckCircle2 size={12} className="text-cs-accent shrink-0" />
      ) : (
        <XCircle size={12} className="text-cs-danger shrink-0" />
      )}
      {/* 2026-05-17 — slug fallback. Generalist dispatches (no
          --agent flag) have a NULL agent_slug; show "(generalist)"
          so the row is informative rather than "unknown". */}
      <code className="font-mono text-cs-text shrink-0">
        {trace.slug ?? <span className="text-cs-muted italic">generalist</span>}
      </code>
      {trace.runtime && (
        <span
          className={cn(
            "inline-block w-1.5 h-1.5 rounded-full shrink-0",
            RUNTIME_DOT[trace.runtime] ?? "bg-cs-muted"
          )}
        />
      )}
      {trace.routedTo && (
        <span className="text-[10px] text-cs-muted">
          → <code className="font-mono">{trace.routedTo}</code>
        </span>
      )}
      <span className="flex-1 truncate text-cs-muted">
        {trace.error ?? trace.responsePreview ?? trace.promptPreview ?? ""}
      </span>
      {trace.durationMs !== undefined && (
        <span className="text-cs-muted tabular-nums shrink-0 inline-flex items-center gap-1">
          <Clock size={10} />
          {trace.durationMs}ms
        </span>
      )}
      {trace.ts && (
        <span className="text-[10px] text-cs-muted shrink-0">
          {new Date(trace.ts).toLocaleString()}
        </span>
      )}
    </button>
  );
}

function EmptyState() {
  const { t } = useTranslation();
  return (
    <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-8 text-center">
      <Activity size={28} className="mx-auto text-cs-muted mb-3" />
      <h3 className="text-sm font-medium text-cs-text">
        {t("observability.emptyTitle", "No traces yet")}
      </h3>
      <p className="mt-1 text-xs text-cs-muted max-w-md mx-auto">
        {t(
          "observability.emptyBody",
          "Traces appear here as soon as you dispatch an agent — Run, Quick Test, MCP run_agent, or group routing. Each dispatch writes one line to ~/.ato/agent-logs.jsonl."
        )}
      </p>
    </div>
  );
}

function chipClass(active: boolean): string {
  return cn(
    "inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-[11px] font-medium transition",
    active
      ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
      : "border-cs-border bg-cs-bg-raised text-cs-muted hover:text-cs-text"
  );
}
