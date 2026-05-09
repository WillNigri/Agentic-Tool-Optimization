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
} from "lucide-react";
import { getRegressions, type RegressionRow } from "@/lib/cloudAgentTraces";
import { useFeatureFlag } from "@/lib/tier";
import { useAuthStore } from "@/hooks/useAuth";
import { cn } from "@/lib/utils";

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
  const canQuery = isCloudUser && accessToken;
  const [days, setDays] = useState<7 | 30 | 90>(30);
  const [showAll, setShowAll] = useState(false);

  const query = useQuery({
    queryKey: ["regressions", days],
    queryFn: () => getRegressions({ days }),
    enabled: !!canQuery && isPro,
    staleTime: 60_000,
  });

  if (!isPro) {
    return (
      <Empty
        icon={<Zap size={20} />}
        title={t("insights.regressions.proRequired", "Regression detection is a Pro feature")}
        body={t(
          "insights.regressions.proBody",
          "Compares trace stats before and after every config change so you spot quality drops the moment you have enough data. Pro tier unlocks it.",
        )}
      />
    );
  }
  if (!canQuery) {
    return (
      <Empty
        icon={<Cloud size={20} />}
        title={t("insights.regressions.signInRequired", "Sign in to see regressions")}
        body={t(
          "insights.regressions.signInBody",
          "Regression detection joins your config-change ledger with cloud trace data — needs a cloud login. Settings → Cloud → Sign in.",
        )}
      />
    );
  }
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
          </h3>
          <p className="mt-0.5 text-[11px] text-cs-muted">
            {t(
              "insights.regressions.subtitle",
              "Every config change with enough traces on both sides — sorted regressions first. Window: {{h}}h before vs after each change, min {{n}} samples per side.",
              { h: query.data?.windowHours ?? 168, n: query.data?.minSamples ?? 20 },
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
            <RegressionCard key={r.change_id} row={r} />
          ))}
        </ul>
      )}
    </div>
  );
}

function RegressionCard({ row }: { row: RegressionRow }) {
  const { t } = useTranslation();
  const Icon = FIELD_ICONS[row.field] ?? FileText;
  const severityClasses =
    row.severity === "regression"
      ? "border-cs-danger/40 bg-cs-danger/5"
      : row.severity === "improvement"
        ? "border-cs-accent/40 bg-cs-accent/5"
        : "border-cs-border bg-cs-bg-raised/40";
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
          <div className="mt-2 grid grid-cols-3 gap-2 text-[11px]">
            <DeltaStat
              label={t("insights.regressions.successRate", "Success rate")}
              before={`${(row.before_ok_rate * 100).toFixed(0)}%`}
              after={`${(row.after_ok_rate * 100).toFixed(0)}%`}
              delta={`${row.ok_delta_pp >= 0 ? "+" : ""}${row.ok_delta_pp.toFixed(1)}pp`}
              good={row.ok_delta_pp >= 0}
            />
            <DeltaStat
              label={t("insights.regressions.p95Latency", "p95 latency")}
              before={`${row.before_p95_ms}ms`}
              after={`${row.after_p95_ms}ms`}
              delta={`${row.p95_delta_pct >= 0 ? "+" : ""}${row.p95_delta_pct.toFixed(0)}%`}
              good={row.p95_delta_pct <= 0}
            />
            <DeltaStat
              label={t("insights.regressions.costPerRun", "Cost / run")}
              before={`$${row.before_cost_per_run.toFixed(4)}`}
              after={`$${row.after_cost_per_run.toFixed(4)}`}
              delta={`${row.cost_delta_pct >= 0 ? "+" : ""}${row.cost_delta_pct.toFixed(0)}%`}
              good={row.cost_delta_pct <= 0}
            />
          </div>
          <div className="mt-1.5 text-[10px] text-cs-muted">
            {t("insights.regressions.sampleSize", "{{n}} runs before · {{m}} after", {
              n: row.before_runs,
              m: row.after_runs,
            })}
          </div>
        </div>
      </div>
    </li>
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
