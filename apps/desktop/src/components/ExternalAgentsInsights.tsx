import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Loader2,
  AlertCircle,
  Globe,
  CheckCircle2,
  XCircle,
  Clock,
  DollarSign,
  Zap,
  Database,
  ExternalLink,
  Sparkles,
} from "lucide-react";
import {
  getAgentTraceMetrics,
  getAgentTraces,
  getTracesByFile,
  getPipelineTraces,
  canQueryCloudTraces,
  type CloudAgentTrace,
  type CloudAgentTraceMetric,
} from "@/lib/cloudAgentTraces";
import { listAgents, type Agent } from "@/lib/agents";
import TraceCompareModal from "@/components/TraceCompareModal";
import { listConfigChanges, type ConfigChange } from "@/lib/cloudConfigChanges";
import { useFeatureFlag } from "@/lib/tier";
import { cn } from "@/lib/utils";

// v2.0.0 Wave 5 — External Agents dashboard.
//
// Surfaces traces flowing back from the deployed Cloudflare Worker /
// Vercel Edge / Docker / Node bundles. Read-only view; the heavy
// lifting (POST /agent-traces from the bundles, retention enforcement,
// per-tier limits) lives on ato-cloud.
//
// Three slices:
//   1. Per-agent metric cards (run count, success rate, p50/p95
//      latency, total cost over the window)
//   2. Drill-down for a selected agent: recent traces with status,
//      runtime, latency, error if any
//   3. Empty states for the various blocked paths (no cloud login,
//      free tier, no traces yet)

type Days = 7 | 30 | 90;

export default function ExternalAgentsInsights() {
  const { t } = useTranslation();
  const isPro = useFeatureFlag("cloud-traces");
  const canQuery = canQueryCloudTraces();
  const [windowDays, setWindowDays] = useState<Days>(30);
  const [selectedSlug, setSelectedSlug] = useState<string | null>(null);
  // v2.1.0 — when set, opens the FileHistoryModal showing every run
  // that touched this path across all agents. Twitter ask: "who
  // changed this file and why?" — this is the answer.
  const [openFile, setOpenFile] = useState<string | null>(null);
  // v2.1.0 Phase 7 — when set, opens the PipelineModal showing every
  // stage of a multi-agent dispatch (Claude → Codex → Gemini) keyed
  // by parent_run_id. The trace list shows a "↪ pipeline" link on
  // any trace that has a parent.
  const [openPipeline, setOpenPipeline] = useState<string | null>(null);
  // v2.1.0 Phase 9 — Eval workbench (compare). When set, opens the
  // TraceCompareModal with this trace as the baseline; the modal
  // lets the user pick a comparison from same-slug recent traces.
  const [openCompare, setOpenCompare] = useState<{ id: string; slug: string | null } | null>(null);

  // Local agent list — used to mark which slugs are EXTERNAL (not just
  // any agent the cloud has traces for) so we surface only the
  // customer-facing dispatch surface here. Internal-agent traces are
  // shown in the existing Agents tab via local jsonl.
  const { data: agents = [] } = useQuery({
    queryKey: ["agents-for-external-insights"],
    queryFn: () => listAgents(),
    staleTime: 30_000,
  });
  const externalAgents = agents.filter((a) => a.kind === "external");
  const externalSlugs = new Set(externalAgents.map((a) => a.slug));

  const metricsQuery = useQuery({
    queryKey: ["cloud-agent-trace-metrics", windowDays],
    queryFn: () => getAgentTraceMetrics(windowDays),
    enabled: canQuery && isPro,
    staleTime: 30_000,
    refetchInterval: 60_000,
  });

  const detailQuery = useQuery({
    queryKey: ["cloud-agent-traces", selectedSlug],
    queryFn: () => getAgentTraces(selectedSlug ?? undefined, 50),
    enabled: canQuery && isPro && !!selectedSlug,
    staleTime: 15_000,
  });

  // v2.1.0 — config changes for the selected agent, in the same window
  // as the trace list. Merged into the timeline so the operator can
  // see "p95 spiked AFTER this model swap, before this prompt edit."
  const changesQuery = useQuery({
    queryKey: ["cloud-config-changes", selectedSlug, windowDays],
    queryFn: () =>
      listConfigChanges({ agentSlug: selectedSlug!, days: windowDays, limit: 100 }),
    enabled: canQuery && isPro && !!selectedSlug,
    staleTime: 15_000,
  });

  // ── Empty / blocked states ─────────────────────────────────────────
  if (!isPro) {
    return (
      <Empty
        icon={<Zap size={20} />}
        title={t("insights.external.proRequired", "External agent traces are a Pro feature")}
        body={t(
          "insights.external.proBody",
          "Traces from your deployed Cloudflare / Vercel / Docker / Node bundles get streamed back here so you can see how customers actually use the agent. Cloud sign-up gives you Pro free during the alpha.",
        )}
      />
    );
  }
  if (!canQuery) {
    return (
      <Empty
        icon={<Globe size={20} />}
        title={t("insights.external.signInRequired", "Sign in to see deployed-agent traces")}
        body={t(
          "insights.external.signInBody",
          "Settings → Cloud → Sign in. Trace data lives on ato-cloud so it's accessible across all your machines + survives a desktop reinstall.",
        )}
      />
    );
  }
  // In mock mode we skip the "no external agents" gate so the
  // fixture metrics render even when the user hasn't created any
  // external agents locally yet. Production builds keep the gate.
  const isMock = import.meta.env.VITE_USE_MOCK_CLOUD === "true";
  if (!isMock && externalAgents.length === 0) {
    return (
      <Empty
        icon={<Globe size={20} />}
        title={t("insights.external.noExternal", "No external agents yet")}
        body={t(
          "insights.external.noExternalBody",
          "External agents are designed for customer-facing deployment. Create one via + New Agent → External, then deploy a bundle to start collecting traces.",
        )}
      />
    );
  }
  if (metricsQuery.isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-cs-muted">
        <Loader2 size={18} className="animate-spin mr-2" />
        {t("insights.external.loading", "Loading metrics…")}
      </div>
    );
  }
  if (metricsQuery.isError) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <span>
          {t("insights.external.error", "Couldn't load trace metrics from cloud")}: {String(metricsQuery.error)}
        </span>
      </div>
    );
  }

  const allMetrics = metricsQuery.data?.metrics ?? [];
  // Filter to ONLY external-agent slugs the user owns locally — keeps
  // the surface focused on deployed-agent observability. In mock
  // mode the local agent list won't match fixture slugs, so we skip
  // the filter and show every metric the mock returns.
  const externalMetrics = isMock
    ? allMetrics
    : allMetrics.filter((m) => externalSlugs.has(m.agent_slug));

  // ── Render ────────────────────────────────────────────────────────
  return (
    <div className="space-y-4">
      {/* Window selector + summary */}
      <header className="flex items-center justify-between gap-3">
        <div>
          <h3 className="text-sm font-medium text-cs-text">
            {t("insights.external.title", "External agent traces")}
          </h3>
          <p className="mt-0.5 text-[11px] text-cs-muted">
            {t(
              "insights.external.subtitle",
              "Live data from your deployed Cloudflare / Vercel / Docker / Node bundles, streamed via the agent_traces pipeline.",
            )}
          </p>
        </div>
        <div className="inline-flex rounded-md border border-cs-border bg-cs-bg-raised p-0.5">
          {([7, 30, 90] as const).map((d) => (
            <button
              key={d}
              type="button"
              onClick={() => setWindowDays(d)}
              className={cn(
                "rounded px-3 py-1.5 text-[11px] font-medium transition",
                windowDays === d
                  ? "bg-cs-accent/15 text-cs-accent"
                  : "text-cs-muted hover:text-cs-text",
              )}
            >
              {t("insights.external.daysLabel", "{{d}}d", { d })}
            </button>
          ))}
        </div>
      </header>

      {externalMetrics.length === 0 ? (
        <NoTracesYet externalAgents={externalAgents} />
      ) : (
        <ul className="space-y-2">
          {externalMetrics.map((m, i) => (
            <MetricCard
              key={m.agent_slug}
              metric={m}
              agent={externalAgents.find((a) => a.slug === m.agent_slug)}
              selected={selectedSlug === m.agent_slug}
              isFirst={i === 0}
              onSelect={() => setSelectedSlug((s) => (s === m.agent_slug ? null : m.agent_slug))}
            />
          ))}
        </ul>
      )}

      {/* Drill-down */}
      {selectedSlug && (
        <section className="rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3 space-y-2">
          <div className="flex items-center justify-between">
            <h4 className="text-xs font-semibold text-cs-text">
              {t("insights.external.recent", "Recent traces — @{{slug}}", { slug: selectedSlug })}
            </h4>
            <button
              type="button"
              onClick={() => setSelectedSlug(null)}
              className="text-[11px] text-cs-muted hover:text-cs-text"
            >
              {t("common.close", "Close")}
            </button>
          </div>
          {detailQuery.isLoading ? (
            <div className="text-xs text-cs-muted">
              <Loader2 size={11} className="inline animate-spin mr-1" />
              {t("insights.external.loadingTraces", "Loading traces…")}
            </div>
          ) : (
            <MergedTimeline
              traces={detailQuery.data?.traces ?? []}
              changes={changesQuery.data?.changes ?? []}
              onFileClick={setOpenFile}
              onPipelineClick={setOpenPipeline}
              onCompareClick={(id, slug) => setOpenCompare({ id, slug })}
            />
          )}
        </section>
      )}

      {openFile && (
        <FileHistoryModal path={openFile} onClose={() => setOpenFile(null)} />
      )}
      {openPipeline && (
        <PipelineModal
          parentRunId={openPipeline}
          onClose={() => setOpenPipeline(null)}
          onFileClick={setOpenFile}
        />
      )}
      {openCompare && (
        <TraceCompareModal
          baselineTraceId={openCompare.id}
          agentSlug={openCompare.slug}
          onClose={() => setOpenCompare(null)}
        />
      )}
    </div>
  );
}

// ── Sub-components ─────────────────────────────────────────────────────

function MetricCard({
  metric,
  agent,
  selected,
  isFirst,
  onSelect,
}: {
  metric: CloudAgentTraceMetric;
  agent: Agent | undefined;
  selected: boolean;
  isFirst?: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  const successRate = metric.run_count > 0 ? metric.ok_count / metric.run_count : 0;
  return (
    <li>
      <button
        type="button"
        // v2.1.0+ — demo IDs let standalone verification scripts click
        // the first card without knowing the slug ahead of time.
        // `agent-metric-<slug>` is stable for known agents; -first
        // works in tests / demos with arbitrary trace data.
        data-demo-id={isFirst ? "agent-metric-first" : `agent-metric-${metric.agent_slug}`}
        onClick={onSelect}
        className={cn(
          "w-full text-left rounded-lg border p-3 transition-colors",
          selected
            ? "border-cs-accent bg-cs-accent/10"
            : "border-cs-border bg-cs-bg-raised/40 hover:border-cs-accent/40",
        )}
      >
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <Globe size={12} className="text-cs-accent" />
              <code className="font-mono text-sm text-cs-text">{metric.agent_slug}</code>
              {agent?.runtime && (
                <span className="text-[10px] uppercase tracking-wide text-cs-muted">
                  {agent.runtime}
                </span>
              )}
            </div>
            {agent?.description && (
              <p className="mt-0.5 text-[11px] text-cs-muted truncate">{agent.description}</p>
            )}
          </div>
          <SuccessBadge rate={successRate} />
        </div>
        <div className="mt-3 grid grid-cols-2 md:grid-cols-4 gap-2 text-[11px]">
          <Stat
            icon={<Database size={10} />}
            label={t("insights.external.runs", "Runs")}
            value={metric.run_count.toLocaleString()}
          />
          <Stat
            icon={<Clock size={10} />}
            label={t("insights.external.latency", "p50 / p95")}
            value={`${metric.p50_ms}ms / ${metric.p95_ms}ms`}
          />
          <Stat
            icon={<XCircle size={10} />}
            label={t("insights.external.fails", "Failures")}
            value={metric.fail_count.toLocaleString()}
          />
          <Stat
            icon={<DollarSign size={10} />}
            label={t("insights.external.cost", "Cost")}
            value={`$${metric.cost_usd.toFixed(2)}`}
          />
        </div>
      </button>
    </li>
  );
}

function SuccessBadge({ rate }: { rate: number }) {
  const pct = Math.round(rate * 100);
  const ok = rate >= 0.95;
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium",
        ok
          ? "border border-cs-accent/40 bg-cs-accent/10 text-cs-accent"
          : "border border-cs-warn/40 bg-cs-warn/10 text-cs-text",
      )}
    >
      {ok ? <CheckCircle2 size={10} /> : <AlertCircle size={10} />}
      {pct}% ok
    </span>
  );
}

function Stat({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return (
    <div className="rounded border border-cs-border bg-cs-bg p-2">
      <div className="flex items-center gap-1 text-cs-muted">
        {icon}
        <span className="text-[10px] uppercase tracking-wide">{label}</span>
      </div>
      <div className="mt-0.5 font-mono text-cs-text">{value}</div>
    </div>
  );
}

// v2.1.0 — Merged timeline of traces + config changes. Renders both
// in chronological order so a reviewer can see "model swap → 6h of
// degraded p95 → prompt edit → recovery" as one coherent story.
function MergedTimeline({
  traces,
  changes,
  onFileClick,
  onPipelineClick,
  onCompareClick,
}: {
  traces: CloudAgentTrace[];
  changes: ConfigChange[];
  onFileClick?: (path: string) => void;
  onPipelineClick?: (parentRunId: string) => void;
  onCompareClick?: (traceId: string, agentSlug: string | null) => void;
}) {
  const { t } = useTranslation();
  if (traces.length === 0 && changes.length === 0) {
    return (
      <p className="text-[11px] text-cs-muted">
        {t("insights.external.noActivity", "No traces or config changes in this window.")}
      </p>
    );
  }

  // Tag and merge by timestamp, descending (newest first — matches the
  // trace list's existing order so the reviewer doesn't have to mentally
  // re-orient when changes appear).
  type Row =
    | { kind: "trace"; at: string; data: CloudAgentTrace }
    | { kind: "change"; at: string; data: ConfigChange };
  const rows: Row[] = [
    ...traces.map((d) => ({ kind: "trace" as const, at: d.started_at, data: d })),
    ...changes.map((d) => ({ kind: "change" as const, at: d.changed_at, data: d })),
  ].sort((a, b) => b.at.localeCompare(a.at));

  // Find the index of the first trace row (skipping any change rows
  // that appear before it). Demo scripts clicking "first trace" use
  // -first demo IDs, so this single source of truth keeps them stable
  // regardless of what change events landed in the merged stream.
  const firstTraceIndex = rows.findIndex((r) => r.kind === "trace");
  return (
    <ul className="space-y-1">
      {rows.map((row, i) =>
        row.kind === "trace" ? (
          <TraceRow
            key={`t-${row.data.id}`}
            trace={row.data}
            isFirst={i === firstTraceIndex}
            onFileClick={onFileClick}
            onPipelineClick={onPipelineClick}
            onCompareClick={onCompareClick}
          />
        ) : (
          <ChangeRow key={`c-${row.data.id}`} change={row.data} />
        ),
      )}
    </ul>
  );
}

function TraceRow({
  trace: tr,
  isFirst,
  onFileClick,
  onPipelineClick,
  onCompareClick,
}: {
  trace: CloudAgentTrace;
  isFirst?: boolean;
  onFileClick?: (path: string) => void;
  onPipelineClick?: (parentRunId: string) => void;
  onCompareClick?: (traceId: string, agentSlug: string | null) => void;
}) {
  const { t } = useTranslation();
  const [showFiles, setShowFiles] = useState(false);
  const origin =
    tr.metadata && typeof (tr.metadata as { origin?: unknown }).origin === "string"
      ? (tr.metadata as { origin: string }).origin
      : null;
  const files = tr.files_touched ?? [];
  const fileCount = files.length;
  // v2.1 Phase 10 — embed-side analytics. When the deployed bundle
  // forwarded an embedSession (widget tracked TTFM, turn count, page
  // URL) it lives at metadata.embedSession. Surface as a small badge
  // so the operator can see customer-facing engagement signals
  // alongside the technical trace data.
  const embedSession =
    tr.metadata && typeof (tr.metadata as { embedSession?: unknown }).embedSession === "object"
      ? ((tr.metadata as { embedSession: Record<string, unknown> }).embedSession ?? null)
      : null;
  const ttfmMs =
    embedSession && typeof embedSession.msToFirstMessage === "number"
      ? (embedSession.msToFirstMessage as number)
      : null;
  const turn =
    embedSession && typeof embedSession.turn === "number"
      ? (embedSession.turn as number)
      : null;
  // v2.1.0+ Concurrent attribution — when this run overlapped with
  // another in the same workspace, mtime-based file attribution is
  // ambiguous. The capture layer (active_runs.rs) records the peers;
  // here we surface it as a warning badge so the user doesn't trust
  // the file list as authoritative.
  const concurrentRuns =
    (tr.metadata && Array.isArray((tr.metadata as { concurrentRuns?: unknown }).concurrentRuns)
      ? (tr.metadata as { concurrentRuns: Array<{ agent_slug?: string | null }> }).concurrentRuns
      : []) ?? [];
  const overlapCount = concurrentRuns.length;
  return (
    <li className="rounded border border-cs-border bg-cs-bg text-[11px]">
      <div className="flex items-center gap-2 px-2 py-1">
        {tr.ok ? (
          <CheckCircle2 size={10} className="text-cs-accent shrink-0" />
        ) : (
          <XCircle size={10} className="text-cs-danger shrink-0" />
        )}
        <code className="font-mono text-cs-muted shrink-0">
          {new Date(tr.started_at).toLocaleTimeString()}
        </code>
        <span className="text-[10px] uppercase tracking-wide text-cs-muted shrink-0">
          {tr.runtime}
        </span>
        <span className="font-mono text-cs-muted shrink-0">{tr.duration_ms}ms</span>
        {tr.error && <span className="text-cs-danger truncate flex-1">{tr.error}</span>}
        {!tr.error && origin && (
          <span className="text-cs-muted truncate flex-1">{origin}</span>
        )}
        {/* Right-side controls cluster. Single ml-auto on the wrapper
            so badges/buttons inside don't fight each other for the
            right edge. */}
        <div className="ml-auto flex items-center gap-1.5 shrink-0">
          {embedSession && (
            <span
              className="inline-flex items-center gap-1 rounded-sm border border-cs-accent/30 bg-cs-accent/5 px-1.5 py-0.5 font-mono text-[10px] text-cs-accent"
              title={t(
                "insights.external.embedSessionTitle",
                "Embed session: turn {{turn}}{{ttfm}}{{url}}",
                {
                  turn: turn ?? "?",
                  ttfm: ttfmMs !== null ? `, TTFM ${(ttfmMs / 1000).toFixed(1)}s` : "",
                  url:
                    embedSession.url && typeof embedSession.url === "string"
                      ? `, on ${embedSession.url}`
                      : "",
                },
              )}
            >
              💬 {turn !== null ? `turn ${turn}` : "embed"}
              {ttfmMs !== null && ttfmMs < 60000
                ? ` · ${(ttfmMs / 1000).toFixed(1)}s`
                : ""}
            </span>
          )}
          {tr.parent_run_id && onPipelineClick && (
            <button
              type="button"
              data-demo-id={isFirst ? "trace-pipeline-first" : undefined}
              onClick={() => onPipelineClick(tr.parent_run_id!)}
              className="inline-flex items-center gap-1 rounded-sm border border-cs-accent/40 bg-cs-accent/10 px-1.5 py-0.5 font-mono text-[10px] text-cs-accent hover:bg-cs-accent/20"
              title={t("insights.external.pipelineTitle", "Open the full pipeline view for this dispatch")}
            >
              ↪ {t("insights.external.pipelineLabel", "pipeline")}
            </button>
          )}
          {onCompareClick && (
            <button
              type="button"
              data-demo-id={isFirst ? "trace-compare-first" : undefined}
              onClick={() => onCompareClick(tr.id, tr.agent_slug)}
              className="inline-flex items-center gap-1 rounded-sm border border-cs-border bg-cs-bg-raised px-1.5 py-0.5 font-mono text-[10px] text-cs-muted hover:text-cs-accent hover:border-cs-accent/40"
              title={t("insights.external.compareTitle", "Open this trace side-by-side with another run")}
            >
              ↔ {t("insights.external.compareLabel", "compare")}
            </button>
          )}
          {overlapCount > 0 && (
            <span
              className="inline-flex items-center gap-1 rounded-sm border border-cs-warn/40 bg-cs-warn/10 px-1.5 py-0.5 font-mono text-[10px] text-cs-warn"
              title={t(
                "insights.external.overlapTitle",
                "Overlapped with {{n}} other run{{p}} in the same workspace — file attribution ambiguous.",
                { n: overlapCount, p: overlapCount === 1 ? "" : "s" },
              )}
            >
              ⚠ {t("insights.external.overlapBadge", "ambiguous ×{{n}}", { n: overlapCount })}
            </span>
          )}
          {fileCount > 0 && (
            <button
              type="button"
              onClick={() => setShowFiles((v) => !v)}
              className={cn(
                "inline-flex items-center gap-1 rounded-sm border bg-cs-bg-raised px-1.5 py-0.5 font-mono text-[10px] hover:text-cs-accent",
                overlapCount > 0
                  ? "border-cs-warn/40 text-cs-warn"
                  : "border-cs-border text-cs-muted",
              )}
              title="Files touched during this dispatch"
            >
              📁 {fileCount} {fileCount === 1 ? "file" : "files"}
            </button>
          )}
        </div>
      </div>
      {showFiles && fileCount > 0 && (
        <ul className="border-t border-cs-border bg-cs-bg-raised/40 px-3 py-1.5 space-y-0.5">
          {overlapCount > 0 && (
            <li className="text-[10px] text-cs-warn pb-1 border-b border-cs-border/40 mb-1">
              {t(
                "insights.external.overlapDetail",
                "⚠ This run overlapped with: {{peers}}. Any of those agents may have written some of these files; mtime-based attribution can't disambiguate concurrent dispatches.",
                {
                  peers: concurrentRuns
                    .map((p) => `@${p.agent_slug ?? "ad-hoc"}`)
                    .join(", "),
                },
              )}
            </li>
          )}
          {files.map((f) => (
            <li key={f} className="flex items-center gap-1.5 font-mono text-[10px]">
              <span className="text-cs-text truncate flex-1">{f}</span>
              {onFileClick && (
                <button
                  type="button"
                  onClick={() => onFileClick(f)}
                  className="shrink-0 text-cs-muted hover:text-cs-accent transition-colors"
                  title={t("insights.external.fileHistory", "Show every run that touched this file")}
                >
                  {t("insights.external.fileHistoryLabel", "history →")}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

function ChangeRow({ change }: { change: ConfigChange }) {
  // Visually distinct: full-width bar with accent background so the
  // operator's eye picks it out from the noise of trace rows.
  const summary =
    typeof change.new_value === "string"
      ? change.new_value
      : change.new_value !== null && change.new_value !== undefined
        ? JSON.stringify(change.new_value)
        : "changed";
  return (
    <li className="flex items-center gap-2 rounded border border-cs-accent/30 bg-cs-accent/5 px-2 py-1 text-[11px]">
      <Sparkles size={10} className="text-cs-accent shrink-0" />
      <code className="font-mono text-cs-accent shrink-0">
        {new Date(change.changed_at).toLocaleTimeString()}
      </code>
      <span className="text-[10px] uppercase tracking-wide text-cs-accent shrink-0">
        {change.field}
      </span>
      <span className="text-cs-text truncate flex-1">{summary}</span>
      <span className="text-[10px] text-cs-muted shrink-0 truncate max-w-[140px]">
        {change.changed_by}
      </span>
    </li>
  );
}

function NoTracesYet({ externalAgents }: { externalAgents: Agent[] }) {
  const { t } = useTranslation();
  return (
    <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 text-center text-sm text-cs-muted">
      <Globe size={20} className="mx-auto mb-2 text-cs-muted" />
      <p className="text-cs-text font-medium mb-1">
        {t("insights.external.noTraces", "No traces in this window")}
      </p>
      <p className="text-[12px] mb-3">
        {t(
          "insights.external.noTracesBody",
          "You have {{n}} external agent{{plural}} but none have streamed traces back yet. The deployed bundle POSTs to /api/agent-traces only when ATO_TRACE_KEY is set as an env var.",
          { n: externalAgents.length, plural: externalAgents.length === 1 ? "" : "s" },
        )}
      </p>
      <a
        href="https://github.com/WillNigri/Agentic-Tool-Optimization#external-agents"
        target="_blank"
        rel="noreferrer"
        className="inline-flex items-center gap-1 text-[11px] text-cs-accent hover:underline"
      >
        <ExternalLink size={11} />
        {t("insights.external.docsLink", "How to enable trace forwarding")}
      </a>
    </div>
  );
}

function Empty({
  icon,
  title,
  body,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
}) {
  return (
    <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 text-center text-sm">
      <div className="mx-auto mb-2 flex h-10 w-10 items-center justify-center rounded-full bg-cs-accent/10 text-cs-accent">
        {icon}
      </div>
      <p className="text-cs-text font-medium mb-1">{title}</p>
      <p className="text-[12px] text-cs-muted">{body}</p>
    </div>
  );
}

// v2.1.0 — File history modal. Answers "who changed this file and why,
// across every dispatch in every agent." Twitter ask: Timur Yessenov.
//
// Each row in the modal is a trace where this file appears in
// files_touched. Sorted newest-first. Shows agent slug + runtime +
// timestamp + duration + error if any so the operator can pivot from
// "what changed in this file?" to "what was that run trying to do?"
// without leaving the dashboard.
function FileHistoryModal({
  path,
  onClose,
}: {
  path: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const query = useQuery({
    queryKey: ["traces-by-file", path],
    queryFn: () => getTracesByFile(path, 200),
    staleTime: 15_000,
  });

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-2xl max-h-[80vh] overflow-hidden rounded-lg border border-cs-border bg-cs-bg-raised shadow-2xl flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-start justify-between gap-3 border-b border-cs-border p-4">
          <div className="min-w-0">
            <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
              <Database size={14} className="text-cs-accent shrink-0" />
              {t("insights.fileHistory.title", "File history")}
            </h3>
            <code className="mt-1 block font-mono text-[11px] text-cs-muted truncate" title={path}>
              {path}
            </code>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="shrink-0 rounded-md border border-cs-border bg-cs-bg px-2 py-1 text-[11px] text-cs-muted hover:text-cs-text"
          >
            {t("common.close", "Close")}
          </button>
        </header>

        <div className="flex-1 min-h-0 overflow-y-auto p-4">
          {query.isLoading ? (
            <div className="flex items-center justify-center h-32 text-cs-muted text-xs">
              <Loader2 size={14} className="animate-spin mr-2" />
              {t("insights.fileHistory.loading", "Loading file history…")}
            </div>
          ) : query.isError ? (
            <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
              <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
              <span>
                {t("insights.fileHistory.error", "Couldn't load history")}: {String(query.error)}
              </span>
            </div>
          ) : (query.data?.traces ?? []).length === 0 ? (
            <p className="text-xs text-cs-muted text-center py-8">
              {t(
                "insights.fileHistory.empty",
                "No traces touched this file yet. Either it was edited before v2.1.0 (when attribution was added) or by a runtime path that doesn't track files yet.",
              )}
            </p>
          ) : (
            <ol className="space-y-1.5">
              {(query.data?.traces ?? []).map((tr) => (
                <li
                  key={tr.id}
                  className="rounded-md border border-cs-border bg-cs-bg p-2.5 text-[11px]"
                >
                  <div className="flex items-center gap-2 mb-1">
                    {tr.ok ? (
                      <CheckCircle2 size={11} className="text-cs-accent shrink-0" />
                    ) : (
                      <XCircle size={11} className="text-cs-danger shrink-0" />
                    )}
                    <code className="font-mono text-cs-text font-medium">
                      @{tr.agent_slug}
                    </code>
                    <span className="text-[10px] uppercase tracking-wide text-cs-muted shrink-0">
                      {tr.runtime}
                    </span>
                    <span className="ml-auto font-mono text-cs-muted shrink-0">
                      {new Date(tr.started_at).toLocaleString()}
                    </span>
                  </div>
                  <div className="flex items-center gap-3 text-cs-muted text-[10px]">
                    <span>
                      <Clock size={9} className="inline mr-0.5" />
                      {tr.duration_ms}ms
                    </span>
                    {tr.routed_to && (
                      <span>
                        →{" "}
                        <code className="font-mono text-cs-text">@{tr.routed_to}</code>
                      </span>
                    )}
                    {tr.cost_usd !== null && tr.cost_usd > 0 && (
                      <span>
                        <DollarSign size={9} className="inline" />
                        {tr.cost_usd.toFixed(4)}
                      </span>
                    )}
                    {tr.source && (
                      <span className="font-mono">{tr.source}</span>
                    )}
                  </div>
                  {tr.error && (
                    <div className="mt-1 text-cs-danger break-words">{tr.error}</div>
                  )}
                  {/* v2.1.0+ — "why" answer. Brief prompt summary lets
                       the reviewer see what the dispatch was trying to
                       do, not just that it touched the file. */}
                  {tr.prompt_summary && (
                    <div className="mt-1.5 rounded border border-cs-border bg-cs-bg-raised/60 px-2 py-1 text-[10px] text-cs-text">
                      <span className="text-cs-muted uppercase tracking-wide text-[9px] mr-1.5">
                        {t("insights.fileHistory.promptLabel", "prompt")}
                      </span>
                      {tr.prompt_summary}
                    </div>
                  )}
                  {/* Other files this same dispatch touched — gives
                       context for "what was that run actually doing?"
                       without making the user open another modal. */}
                  {tr.files_touched && tr.files_touched.length > 1 && (
                    <details className="mt-1.5">
                      <summary className="cursor-pointer text-[10px] text-cs-muted hover:text-cs-text">
                        {t("insights.fileHistory.otherFiles", "{{n}} other files in this dispatch", {
                          n: tr.files_touched.length - 1,
                        })}
                      </summary>
                      <ul className="mt-1 ml-3 space-y-0.5">
                        {tr.files_touched
                          .filter((f) => f !== path)
                          .slice(0, 20)
                          .map((f) => (
                            <li key={f} className="font-mono text-[10px] text-cs-muted truncate">
                              {f}
                            </li>
                          ))}
                      </ul>
                    </details>
                  )}
                </li>
              ))}
            </ol>
          )}
        </div>
      </div>
    </div>
  );
}

// v2.1.0 Phase 7 — Pipeline trace visualizer.
//
// Multi-agent dispatches (sequential pipelines, routed groups) emit
// one trace per stage with a shared parent_run_id. This modal queries
// /api/agent-traces/pipeline/<id> and renders the chain as a flow:
// Claude → Codex → Gemini, with per-stage status, runtime, latency,
// files touched, and prompt summary.
//
// Why this view earns its keep beyond the linear trace list:
//   - In the per-agent drill-down each stage shows up as a separate
//     row, mixed with unrelated traces. The flow view groups them.
//   - The handoff itself is the interesting bit (what runtime took
//     over from what); the per-row layout buries it.
function PipelineModal({
  parentRunId,
  onClose,
  onFileClick,
}: {
  parentRunId: string;
  onClose: () => void;
  onFileClick?: (path: string) => void;
}) {
  const { t } = useTranslation();
  const query = useQuery({
    queryKey: ["pipeline-traces", parentRunId],
    queryFn: () => getPipelineTraces(parentRunId),
    staleTime: 15_000,
  });

  const stages = query.data?.stages ?? [];
  const totalDuration = stages.reduce((acc, s) => acc + (s.duration_ms ?? 0), 0);
  // Group dispatch metadata is on each stage's metadata; pull from
  // the first stage so the header can summarize "@write-and-review,
  // 3 stages, 2.3s total".
  const groupSlug =
    stages[0]?.metadata && typeof (stages[0].metadata as { groupSlug?: unknown }).groupSlug === "string"
      ? (stages[0].metadata as { groupSlug: string }).groupSlug
      : null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-3xl max-h-[85vh] overflow-hidden rounded-lg border border-cs-border bg-cs-bg-raised shadow-2xl flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-start justify-between gap-3 border-b border-cs-border p-4">
          <div className="min-w-0">
            <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
              <Sparkles size={14} className="text-cs-accent shrink-0" />
              {t("insights.pipeline.title", "Pipeline trace")}
            </h3>
            <div className="mt-1 flex items-center gap-3 text-[11px] text-cs-muted">
              {groupSlug && (
                <code className="font-mono text-cs-text">@{groupSlug}</code>
              )}
              <span>
                {stages.length}{" "}
                {stages.length === 1
                  ? t("insights.pipeline.stage", "stage")
                  : t("insights.pipeline.stages", "stages")}
              </span>
              <span className="font-mono">
                {totalDuration > 1000 ? `${(totalDuration / 1000).toFixed(1)}s` : `${totalDuration}ms`} total
              </span>
              <code className="font-mono text-[10px] text-cs-muted truncate" title={parentRunId}>
                {parentRunId.slice(0, 8)}…
              </code>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="shrink-0 rounded-md border border-cs-border bg-cs-bg px-2 py-1 text-[11px] text-cs-muted hover:text-cs-text"
          >
            {t("common.close", "Close")}
          </button>
        </header>

        <div className="flex-1 min-h-0 overflow-y-auto p-4">
          {query.isLoading ? (
            <div className="flex items-center justify-center h-32 text-cs-muted text-xs">
              <Loader2 size={14} className="animate-spin mr-2" />
              {t("insights.pipeline.loading", "Loading pipeline…")}
            </div>
          ) : query.isError ? (
            <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
              <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
              <span>
                {t("insights.pipeline.error", "Couldn't load pipeline")}: {String(query.error)}
              </span>
            </div>
          ) : stages.length === 0 ? (
            <p className="text-xs text-cs-muted text-center py-8">
              {t(
                "insights.pipeline.empty",
                "No stages found for this dispatch. Either the upload is still in flight or the parent_run_id is invalid.",
              )}
            </p>
          ) : (
            <ol className="space-y-2">
              {stages.map((stage, i) => (
                <PipelineStageRow
                  key={stage.id}
                  stage={stage}
                  index={i}
                  isLast={i === stages.length - 1}
                  onFileClick={onFileClick}
                />
              ))}
            </ol>
          )}
        </div>
      </div>
    </div>
  );
}

function PipelineStageRow({
  stage,
  index,
  isLast,
  onFileClick,
}: {
  stage: CloudAgentTrace;
  index: number;
  isLast: boolean;
  onFileClick?: (path: string) => void;
}) {
  const { t } = useTranslation();
  const files = stage.files_touched ?? [];
  return (
    <li>
      <div
        className={cn(
          "rounded-lg border p-3",
          stage.ok
            ? "border-cs-border bg-cs-bg-raised/40"
            : "border-cs-danger/40 bg-cs-danger/5",
        )}
      >
        <div className="flex items-center gap-2">
          {/* Stage number — visual anchor for "this is step N." */}
          <span className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-cs-accent/15 text-[10px] font-mono font-bold text-cs-accent">
            {index + 1}
          </span>
          {stage.ok ? (
            <CheckCircle2 size={11} className="text-cs-accent shrink-0" />
          ) : (
            <XCircle size={11} className="text-cs-danger shrink-0" />
          )}
          <code className="font-mono text-sm text-cs-text font-medium">
            @{stage.agent_slug}
          </code>
          <span className="text-[10px] uppercase tracking-wide text-cs-muted shrink-0">
            {stage.runtime}
          </span>
          <span className="ml-auto font-mono text-[10px] text-cs-muted shrink-0">
            <Clock size={9} className="inline mr-0.5" />
            {stage.duration_ms}ms
          </span>
        </div>
        {stage.prompt_summary && index === 0 && (
          <div className="mt-1.5 rounded border border-cs-border bg-cs-bg px-2 py-1 text-[11px] text-cs-text">
            <span className="text-cs-muted uppercase tracking-wide text-[9px] mr-1.5">
              {t("insights.pipeline.userPrompt", "user")}
            </span>
            {stage.prompt_summary}
          </div>
        )}
        {stage.error && (
          <div className="mt-1.5 text-[11px] text-cs-danger break-words">{stage.error}</div>
        )}
        {files.length > 0 && (
          <details className="mt-1.5">
            <summary className="cursor-pointer text-[10px] text-cs-muted hover:text-cs-text">
              📁 {files.length} {files.length === 1 ? "file" : "files"} touched
            </summary>
            <ul className="mt-1 ml-3 space-y-0.5">
              {files.map((f) => (
                <li key={f} className="flex items-center gap-1.5 text-[10px]">
                  <span className="font-mono text-cs-text truncate flex-1">{f}</span>
                  {onFileClick && (
                    <button
                      type="button"
                      onClick={() => onFileClick(f)}
                      className="shrink-0 font-mono text-cs-muted hover:text-cs-accent"
                    >
                      history →
                    </button>
                  )}
                </li>
              ))}
            </ul>
          </details>
        )}
      </div>
      {/* Arrow between stages — visual handoff. Only render between
          adjacent stages so the last one isn't followed by a dangler. */}
      {!isLast && (
        <div className="flex justify-center my-1 text-cs-accent text-sm font-mono">↓</div>
      )}
    </li>
  );
}
