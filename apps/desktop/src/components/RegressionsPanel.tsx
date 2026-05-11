import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Loader2,
  AlertCircle,
  TrendingDown,
  TrendingUp,
  Minus,
  Cpu,
  FileText,
  GitCommit,
  Server,
  Sparkles,
  Cloud,
  Zap,
  XCircle,
  Clock,
  RotateCw,
} from "lucide-react";
import {
  getRegressions,
  getTraceById,
  type RegressionRow,
  type CloudAgentTrace,
} from "@/lib/cloudAgentTraces";
// v2.3.2 Phase 2.x — local-mode fallback so signed-out users still see
// regressions detected from their own machine's dispatches. Same
// algorithm, no cloud round-trip.
import { getRegressionsLocal } from "@/lib/localInsights";
import { useFeatureFlag } from "@/lib/tier";
import { useAuthStore } from "@/hooks/useAuth";
import { asNumber } from "@/lib/pricing";
import { cn } from "@/lib/utils";
// v2.2.0 — failing-example → replay-on-alt-runtime wiring. Reuses the
// ReplayPicker + ReplayResultPanel components originally built for
// TraceCompareModal so the UX is identical wherever replay surfaces.
import { ReplayPicker, ReplayResultPanel } from "./TraceCompareModal";

// v2.1.0 Phase 5 — Cross-runtime regression detection.
//
// The headline pitch: "Switching @reviewer from sonnet-4-6 → 4-7
// dropped success rate from 91% → 74% across 412 conversations."
//
// For every model / role_models / system_prompt / runtime change in
// the window, the cloud computes aggregate stats for the trace window
// before AND after, returns them with severity tagged. We render
// regressions first (most actionable), then improvements, then
// neutral. Each card shows the before→after deltas in plain English
// so users can act on it without re-doing the math.
//
// Why this works on top of v1.4 evaluators:
//   - Evaluators give per-run scores; this gives per-EDIT impact.
//   - Same trace data, different aggregation lens.
//   - No retraining or extra config — fires the moment you have
//     enough traces on each side of any logged change.

const FIELD_ICONS: Record<string, typeof FileText> = {
  model: Cpu,
  role_models: Cpu,
  system_prompt: FileText,
  runtime: Server,
};

export default function RegressionsPanel() {
  const { t } = useTranslation();
  const isPro = useFeatureFlag("cloud-traces");
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const accessToken = useAuthStore((s) => s.accessToken);
  const mock = import.meta.env.VITE_USE_MOCK_CLOUD === "true";
  // v2.3.2 Phase 2.x — cloud is preferred (cross-device aggregation),
  // but local-mode is the fallback so signed-out users still get value.
  // Auth state decides which path; both query types return the same
  // shape so the rendering below doesn't fork.
  const cloudEligible = !!(mock || (isCloudUser && accessToken)) && isPro;
  const [days, setDays] = useState<7 | 30 | 90>(30);
  const [showAll, setShowAll] = useState(false);
  const [openDrill, setOpenDrill] = useState<RegressionRow | null>(null);

  const cloudQuery = useQuery({
    queryKey: ["regressions-cloud", days],
    queryFn: () => getRegressions({ days }),
    enabled: cloudEligible,
    staleTime: 60_000,
  });

  const localQuery = useQuery({
    queryKey: ["regressions-local", days],
    queryFn: () => getRegressionsLocal({ days }),
    // Run local only when cloud isn't eligible OR when the cloud query
    // errored (fallback). The latter case handles transient cloud
    // outages cleanly — local data is always available.
    enabled: !cloudEligible || cloudQuery.isError,
    staleTime: 60_000,
  });

  // Pick whichever source has data. Cloud wins when eligible AND
  // successful; otherwise fall back to local.
  const usingLocal =
    !cloudEligible || cloudQuery.isError || !cloudQuery.data;
  const query = usingLocal ? localQuery : cloudQuery;
  const data = query.data;
  const mode: "cloud" | "local" | "local-no-schema" =
    !usingLocal
      ? "cloud"
      : (data as { source?: string } | undefined)?.source === "local-no-schema"
        ? "local-no-schema"
        : "local";

  if (query.isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-cs-muted">
        <Loader2 size={18} className="animate-spin mr-2" />
        {t("insights.regressions.loading", "Comparing windows…")}
      </div>
    );
  }
  if (query.isError) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <span>{t("insights.regressions.error", "Couldn't load regressions")}: {String(query.error)}</span>
      </div>
    );
  }

  // Bail early when the local schema isn't there yet — clearer signal
  // than "0 regressions detected."
  if (mode === "local-no-schema") {
    return (
      <Empty
        icon={<Cloud size={20} />}
        title={t(
          "insights.regressions.schemaNotReady",
          "Regression schema not migrated yet",
        )}
        body={t(
          "insights.regressions.schemaNotReadyBody",
          "Local-mode regressions need the v2.3.2 schema. This usually means the desktop hasn't restarted since v2.3.2 landed. Reload the app and try again.",
        )}
      />
    );
  }

  // Cloud-side window/min-samples might differ slightly from local;
  // pick whichever the active source returned.
  const windowHours = (data as any)?.windowHours ?? (data as any)?.window_hours ?? 168;
  const minSamples = (data as any)?.minSamples ?? (data as any)?.min_samples ?? 20;
  const all = query.data?.regressions ?? [];
  const regressions = all.filter((r) => r.severity === "regression");
  const improvements = all.filter((r) => r.severity === "improvement");
  const neutral = all.filter((r) => r.severity === "neutral");
  const visible = showAll ? all : [...regressions, ...improvements];

  return (
    <div className="space-y-4">
      <header className="flex items-center justify-between gap-3">
        <div>
          <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
            <GitCommit size={14} className="text-cs-accent" />
            {t("insights.regressions.title", "Regression detector")}
            <ModeBadge mode={mode} />
          </h3>
          <p className="mt-0.5 text-[11px] text-cs-muted">
            {t(
              "insights.regressions.subtitle",
              "Every config change with enough traces on both sides — sorted regressions first. Window: {{h}}h before vs after each change, min {{n}} samples per side.",
              { h: windowHours, n: minSamples },
            )}
          </p>
        </div>
        <div className="inline-flex rounded-md border border-cs-border bg-cs-bg-raised p-0.5">
          {([7, 30, 90] as const).map((d) => (
            <button
              key={d}
              type="button"
              onClick={() => setDays(d)}
              className={cn(
                "rounded px-3 py-1.5 text-[11px] font-medium transition",
                days === d ? "bg-cs-accent/15 text-cs-accent" : "text-cs-muted hover:text-cs-text",
              )}
            >
              {d}d
            </button>
          ))}
        </div>
      </header>

      {/* Summary bar */}
      <div className="flex items-center gap-3 rounded-md border border-cs-border bg-cs-bg-raised/40 px-3 py-2 text-[11px]">
        <span className="inline-flex items-center gap-1 text-cs-danger">
          <TrendingDown size={11} />
          <strong>{regressions.length}</strong>{" "}
          {t("insights.regressions.summaryRegressions", "regression")}
          {regressions.length === 1 ? "" : "s"}
        </span>
        <span className="inline-flex items-center gap-1 text-cs-accent">
          <TrendingUp size={11} />
          <strong>{improvements.length}</strong>{" "}
          {t("insights.regressions.summaryImprovements", "improvement")}
          {improvements.length === 1 ? "" : "s"}
        </span>
        <span className="inline-flex items-center gap-1 text-cs-muted ml-auto">
          <Minus size={11} />
          <strong>{neutral.length}</strong> {t("insights.regressions.neutral", "neutral")}
          <button
            type="button"
            onClick={() => setShowAll((v) => !v)}
            className="ml-2 text-cs-muted hover:text-cs-accent underline-offset-2 hover:underline"
          >
            {showAll
              ? t("insights.regressions.hideNeutral", "hide neutral")
              : t("insights.regressions.showAll", "show all")}
          </button>
        </span>
      </div>

      {visible.length === 0 ? (
        <Empty
          icon={<Sparkles size={20} />}
          title={t("insights.regressions.empty", "No detectable regressions in this window")}
          body={t(
            "insights.regressions.emptyBody",
            "Either your edits haven't moved the needle (good!), or there aren't enough traces on each side of recent changes yet. Reduce the window or accumulate more traces.",
          )}
        />
      ) : (
        <ul className="space-y-2">
          {visible.map((r) => (
            <RegressionCard
              key={r.change_id}
              row={r}
              onDrillIn={() => setOpenDrill(r)}
            />
          ))}
        </ul>
      )}
      {openDrill && (
        <RegressionDrillModal
          row={openDrill}
          onClose={() => setOpenDrill(null)}
        />
      )}
    </div>
  );
}

function RegressionCard({
  row,
  onDrillIn,
}: {
  row: RegressionRow;
  onDrillIn: () => void;
}) {
  const { t } = useTranslation();
  const Icon = FIELD_ICONS[row.field] ?? FileText;
  const severityClasses =
    row.severity === "regression"
      ? "border-cs-danger/40 bg-cs-danger/5"
      : row.severity === "improvement"
        ? "border-cs-accent/40 bg-cs-accent/5"
        : "border-cs-border bg-cs-bg-raised/40";
  // v2.1 Phase 5b — eval score is null when neither side ran an evaluator.
  // Render "—" rather than a misleading 0pp delta in that case.
  const hasEval = row.eval_delta_pp !== null;
  const failingCount = row.failing_trace_ids?.length ?? 0;
  return (
    <li className={cn("rounded-lg border p-3", severityClasses)}>
      <div className="flex items-start gap-2">
        <Icon size={14} className="text-cs-muted shrink-0 mt-0.5" />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <code className="font-mono text-sm text-cs-text font-medium">@{row.agent_slug}</code>
            <span className="text-[10px] uppercase tracking-wide text-cs-muted">{row.field}</span>
            <SeverityBadge severity={row.severity} />
            <time className="ml-auto text-[10px] font-mono text-cs-muted" dateTime={row.changed_at}>
              {new Date(row.changed_at).toLocaleString()}
            </time>
          </div>
          <p className="mt-1 text-[11px] text-cs-muted">
            {summarizeChange(row, t)}
          </p>
          <div
            className={cn(
              "mt-2 grid gap-2 text-[11px]",
              hasEval ? "grid-cols-4" : "grid-cols-3",
            )}
          >
            {/* v2.1.9 — coerce every PG-string-numeric column with
                asNumber before .toFixed/comparison so "0.014200"
                doesn't crash and "0.91" > 0 doesn't string-compare. */}
            <DeltaStat
              label={t("insights.regressions.successRate", "Success rate")}
              before={`${(asNumber(row.before_ok_rate) * 100).toFixed(0)}%`}
              after={`${(asNumber(row.after_ok_rate) * 100).toFixed(0)}%`}
              delta={`${asNumber(row.ok_delta_pp) >= 0 ? "+" : ""}${asNumber(row.ok_delta_pp).toFixed(1)}pp`}
              good={asNumber(row.ok_delta_pp) >= 0}
            />
            {hasEval && (
              <DeltaStat
                label={t("insights.regressions.evalScore", "Eval score")}
                before={asNumber(row.before_eval_score).toFixed(2)}
                after={asNumber(row.after_eval_score).toFixed(2)}
                delta={`${asNumber(row.eval_delta_pp) >= 0 ? "+" : ""}${asNumber(row.eval_delta_pp).toFixed(1)}pp`}
                good={asNumber(row.eval_delta_pp) >= 0}
              />
            )}
            <DeltaStat
              label={t("insights.regressions.p95Latency", "p95 latency")}
              before={`${row.before_p95_ms}ms`}
              after={`${row.after_p95_ms}ms`}
              delta={`${asNumber(row.p95_delta_pct) >= 0 ? "+" : ""}${asNumber(row.p95_delta_pct).toFixed(0)}%`}
              good={asNumber(row.p95_delta_pct) <= 0}
            />
            <DeltaStat
              label={t("insights.regressions.costPerRun", "Cost / run")}
              before={`$${asNumber(row.before_cost_per_run).toFixed(4)}`}
              after={`$${asNumber(row.after_cost_per_run).toFixed(4)}`}
              delta={`${asNumber(row.cost_delta_pct) >= 0 ? "+" : ""}${asNumber(row.cost_delta_pct).toFixed(0)}%`}
              good={asNumber(row.cost_delta_pct) <= 0}
            />
          </div>
          <div className="mt-1.5 flex items-center gap-2 text-[10px] text-cs-muted">
            <span>
              {t("insights.regressions.sampleSize", "{{n}} runs before · {{m}} after", {
                n: row.before_runs,
                m: row.after_runs,
              })}
            </span>
            {failingCount > 0 && (
              <button
                type="button"
                onClick={onDrillIn}
                className="ml-auto inline-flex items-center gap-1 text-cs-warn hover:text-cs-text underline-offset-2 hover:underline"
              >
                {t(
                  "insights.regressions.viewFailing",
                  "View {{n}} failing example{{s}} →",
                  { n: failingCount, s: failingCount === 1 ? "" : "s" },
                )}
              </button>
            )}
          </div>
        </div>
      </div>
    </li>
  );
}

// v2.3.2 Phase 2.x — surfaces which data source the panel is showing.
// Cloud = cross-device aggregation (Pro). Local = this machine's
// SQLite. Local-no-schema = migration hasn't run yet.
function ModeBadge({ mode }: { mode: "cloud" | "local" | "local-no-schema" }) {
  if (mode === "cloud") {
    return (
      <span
        title="Cross-device aggregation (Pro)"
        className="inline-flex items-center gap-1 rounded-full border border-cs-accent/40 bg-cs-accent/10 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide text-cs-accent"
      >
        <Cloud size={9} /> cloud
      </span>
    );
  }
  return (
    <span
      title="Computed from this machine's dispatches only. Sign in for cross-device aggregation."
      className="inline-flex items-center gap-1 rounded-full border border-cs-border bg-cs-bg-raised px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide text-cs-muted"
    >
      local
    </span>
  );
}

function SeverityBadge({ severity }: { severity: RegressionRow["severity"] }) {
  if (severity === "regression") {
    return (
      <span className="inline-flex items-center gap-1 rounded-full border border-cs-danger/40 bg-cs-danger/10 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide text-cs-danger">
        <TrendingDown size={9} />
        regression
      </span>
    );
  }
  if (severity === "improvement") {
    return (
      <span className="inline-flex items-center gap-1 rounded-full border border-cs-accent/40 bg-cs-accent/10 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide text-cs-accent">
        <TrendingUp size={9} />
        improvement
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1 rounded-full border border-cs-border bg-cs-bg-raised px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide text-cs-muted">
      <Minus size={9} />
      neutral
    </span>
  );
}

function DeltaStat({
  label,
  before,
  after,
  delta,
  good,
}: {
  label: string;
  before: string;
  after: string;
  delta: string;
  good: boolean;
}) {
  return (
    <div className="rounded border border-cs-border bg-cs-bg p-2">
      <div className="text-[9px] uppercase tracking-wide text-cs-muted">{label}</div>
      <div className="mt-0.5 flex items-baseline gap-1.5 font-mono">
        <span className="text-cs-muted text-[10px]">{before}</span>
        <span className="text-cs-muted text-[10px]">→</span>
        <span className="text-cs-text">{after}</span>
      </div>
      <div className={cn("mt-0.5 text-[10px] font-mono", good ? "text-cs-accent" : "text-cs-danger")}>
        {delta}
      </div>
    </div>
  );
}

/** Render a one-line change summary the user can read at a glance.
 *  Falls back to the raw old→new JSON when the field shape is exotic. */
function summarizeChange(row: RegressionRow, t: ReturnType<typeof useTranslation>["t"]): string {
  const oldStr = stringifyValue(row.old_value);
  const newStr = stringifyValue(row.new_value);
  if (row.field === "model" || row.field === "runtime") {
    return t("insights.regressions.summaryModel", "Switched {{field}} {{from}} → {{to}}", {
      field: row.field,
      from: oldStr || "?",
      to: newStr,
    });
  }
  if (row.field === "system_prompt") {
    return t(
      "insights.regressions.summaryPrompt",
      "System prompt edited (length {{from}} → {{to}} chars)",
      { from: oldStr.length, to: newStr.length },
    );
  }
  return t("insights.regressions.summaryGeneric", "Field {{field}} changed", { field: row.field });
}

function stringifyValue(v: unknown): string {
  if (v === null || v === undefined) return "";
  if (typeof v === "string") return v;
  try {
    return JSON.stringify(v);
  } catch {
    return String(v);
  }
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

// v2.1 Phase 5b — Drill-down modal showing the actual failing post-change
// traces. The aggregate delta tells you something dropped; this tells you
// WHICH conversations failed so you can read the prompts + errors and
// decide whether to roll back or just patch the prompt.
//
// v2.2.0 — each failing example gains a "Replay on alt runtime" button.
// Closes the loop the strategy audit flagged as the highest-leverage
// connection: regression detector tells you what broke, replay tells
// you whether the alternative would have been right. Picker + result
// panel reuse the components from TraceCompareModal.
function RegressionDrillModal({
  row,
  onClose,
}: {
  row: RegressionRow;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  // Picker state: which trace's "Replay on…" picker is open.
  const [pickerTrace, setPickerTrace] = useState<CloudAgentTrace | null>(null);
  // Active replay state: which job is running + the trace it replays.
  const [activeReplay, setActiveReplay] = useState<{
    jobId: string;
    trace: CloudAgentTrace;
  } | null>(null);
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
              <TrendingDown size={14} className="text-cs-danger shrink-0" />
              {t("insights.regressions.drillTitle", "Failing examples after the change")}
            </h3>
            <p className="mt-1 text-[11px] text-cs-muted">
              <code className="font-mono text-cs-text">@{row.agent_slug}</code>
              {" · "}
              {summarizeChange(row, t)}
              {" · "}
              {new Date(row.changed_at).toLocaleString()}
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="shrink-0 rounded-md border border-cs-border bg-cs-bg px-2 py-1 text-[11px] text-cs-muted hover:text-cs-text"
          >
            {t("common.close", "Close")}
          </button>
        </header>
        <div className="flex-1 min-h-0 overflow-y-auto p-4 space-y-3">
          {/* Active replay surfaces at the top so the side-by-side is
              the first thing the user sees after triggering one. */}
          {activeReplay && (
            <ReplayResultPanel
              jobId={activeReplay.jobId}
              baselineTrace={activeReplay.trace}
              onClear={() => setActiveReplay(null)}
            />
          )}
          <ul className="space-y-2">
            {row.failing_trace_ids.map((traceId) => (
              <FailingTraceRow
                key={traceId}
                traceId={traceId}
                onReplay={(trace) => setPickerTrace(trace)}
              />
            ))}
          </ul>
        </div>
      </div>
      {/* Replay picker submodal — opens above the drill modal with a
          higher z-index so the click-outside-to-close behaviour works. */}
      {pickerTrace && (
        <ReplayPicker
          baselineTrace={pickerTrace}
          baselineTraceId={pickerTrace.id}
          onClose={() => setPickerTrace(null)}
          onStarted={(jobId) => {
            setActiveReplay({ jobId, trace: pickerTrace });
            setPickerTrace(null);
          }}
        />
      )}
    </div>
  );
}

function FailingTraceRow({
  traceId,
  onReplay,
}: {
  traceId: string;
  onReplay: (trace: CloudAgentTrace) => void;
}) {
  const { t } = useTranslation();
  const traceQuery = useQuery({
    queryKey: ["trace-by-id", traceId],
    queryFn: () => getTraceById(traceId),
    staleTime: 60_000,
  });
  if (traceQuery.isLoading) {
    return (
      <li className="flex items-center gap-2 rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3 text-[11px] text-cs-muted">
        <Loader2 size={12} className="animate-spin" />
        {t("insights.regressions.loadingTrace", "Loading trace…")}
      </li>
    );
  }
  if (traceQuery.isError || !traceQuery.data) {
    return (
      <li className="rounded-lg border border-cs-danger/40 bg-cs-danger/5 p-3 text-[11px] text-cs-danger">
        <code className="font-mono">{traceId.slice(0, 8)}…</code>{" "}
        {t("insights.regressions.traceLoadError", "couldn't load")}
      </li>
    );
  }
  const tr: CloudAgentTrace = traceQuery.data;
  return (
    <li className="rounded-lg border border-cs-danger/30 bg-cs-danger/5 p-3">
      <div className="flex items-center gap-2">
        <XCircle size={11} className="text-cs-danger shrink-0" />
        <code className="font-mono text-[11px] text-cs-text">{tr.id.slice(0, 8)}…</code>
        <span className="text-[10px] uppercase tracking-wide text-cs-muted">{tr.runtime}</span>
        <span className="ml-auto inline-flex items-center gap-1 text-[10px] font-mono text-cs-muted">
          <Clock size={9} />
          {tr.duration_ms}ms
        </span>
        {/* v2.2.0 — Replay this failing example on a different runtime.
            Closes the regression → replay loop without leaving the
            modal: dispatch goes through prompt_agent_inner, the result
            panel surfaces at the top of this drill view. */}
        <button
          type="button"
          onClick={() => onReplay(tr)}
          className="inline-flex items-center gap-1 rounded-md border border-cs-accent/40 bg-cs-accent/10 px-1.5 py-0.5 text-[10px] font-medium text-cs-accent hover:bg-cs-accent/20"
          title={t(
            "insights.regressions.replayHint",
            "Re-run this prompt against a different runtime to check if it would have passed",
          )}
        >
          <RotateCw size={9} />
          {t("insights.regressions.replay", "Replay on…")}
        </button>
      </div>
      {tr.prompt_summary && (
        <div className="mt-1.5 rounded border border-cs-border bg-cs-bg px-2 py-1 text-[11px] text-cs-text">
          <span className="text-cs-muted uppercase tracking-wide text-[9px] mr-1.5">
            {t("insights.regressions.prompt", "prompt")}
          </span>
          {tr.prompt_summary}
        </div>
      )}
      {tr.error && (
        <div className="mt-1.5 rounded border border-cs-danger/40 bg-cs-danger/10 px-2 py-1 text-[11px] text-cs-danger break-words">
          <span className="uppercase tracking-wide text-[9px] mr-1.5">
            {t("insights.regressions.error", "error")}
          </span>
          {tr.error}
        </div>
      )}
      <div className="mt-1 text-[10px] text-cs-muted">
        {new Date(tr.started_at).toLocaleString()}
      </div>
    </li>
  );
}
