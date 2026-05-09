import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Loader2,
  AlertCircle,
  DollarSign,
  TrendingUp,
  Cpu,
  Cloud,
  Zap,
} from "lucide-react";
import { getCostBenchmarks, type CostBenchmarkRow } from "@/lib/cloudAgentTraces";
import { useFeatureFlag } from "@/lib/tier";
import { useAuthStore } from "@/hooks/useAuth";
import { cn } from "@/lib/utils";

// v2.1.0 Phase 8 — Cost benchmarks (descriptive, not prescriptive).
//
// Pitch: "@triage costs $0.014/run. Your median is $0.003. Look at it."
//
// Why descriptive:
//   - Real cross-runtime "switching @triage from GPT-4 to Haiku saves
//     $312/mo with no quality drop" requires shadow-evaluation across
//     runtimes, which is a v2.2 feature.
//   - Outlier flagging is purely statistical (cost vs your own
//     median), so it's honest at any data volume.
//
// Anti-pattern avoided: claiming we know which model "should" be used
// based on prompt similarity scores or other ML magic. We don't have
// the data to back that up; calling it out wrong destroys trust.

export default function CostBenchmarksPanel() {
  const { t } = useTranslation();
  const isPro = useFeatureFlag("cloud-traces");
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const accessToken = useAuthStore((s) => s.accessToken);
  const canQuery = isCloudUser && accessToken;
  const [days, setDays] = useState<7 | 30 | 90>(30);

  const query = useQuery({
    queryKey: ["cost-benchmarks", days],
    queryFn: () => getCostBenchmarks({ days }),
    enabled: !!canQuery && isPro,
    staleTime: 60_000,
  });

  if (!isPro) {
    return (
      <Empty
        icon={<Zap size={20} />}
        title={t("insights.cost.proRequired", "Cost benchmarks are a Pro feature")}
        body={t(
          "insights.cost.proBody",
          "Aggregates trace cost data across all your agents so you can spot expensive outliers. Pro tier unlocks it.",
        )}
      />
    );
  }
  if (!canQuery) {
    return (
      <Empty
        icon={<Cloud size={20} />}
        title={t("insights.cost.signInRequired", "Sign in to see cost benchmarks")}
        body={t(
          "insights.cost.signInBody",
          "Cost benchmarks live on ato-cloud — needs a cloud login. Settings → Cloud → Sign in.",
        )}
      />
    );
  }
  if (query.isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-cs-muted">
        <Loader2 size={18} className="animate-spin mr-2" />
        {t("insights.cost.loading", "Crunching cost data…")}
      </div>
    );
  }
  if (query.isError) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <span>
          {t("insights.cost.error", "Couldn't load cost benchmarks")}: {String(query.error)}
        </span>
      </div>
    );
  }

  const rows = query.data?.rows ?? [];
  const median = query.data?.medianCostPerOk ?? 0;
  const outliers = rows.filter((r) => r.is_outlier);
  const totalSpend = rows.reduce((acc, r) => acc + r.total_cost_usd, 0);

  return (
    <div className="space-y-4">
      <header className="flex items-center justify-between gap-3">
        <div>
          <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
            <DollarSign size={14} className="text-cs-accent" />
            {t("insights.cost.title", "Cost benchmarks")}
          </h3>
          <p className="mt-0.5 text-[11px] text-cs-muted">
            {t(
              "insights.cost.subtitle",
              "Per-(agent, runtime) cost-per-success over the window. Outliers flagged at 2× your median, sorted descending. Doesn't claim what's optimal — surfaces the spread so you can decide.",
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

      {/* Summary bar — total spend + median + outlier count. */}
      <div className="grid grid-cols-3 gap-2 text-[11px]">
        <SummaryStat
          label={t("insights.cost.totalSpend", "Total spend")}
          value={`$${totalSpend.toFixed(2)}`}
        />
        <SummaryStat
          label={t("insights.cost.medianPerOk", "Median $/success")}
          value={median > 0 ? `$${median.toFixed(4)}` : "—"}
        />
        <SummaryStat
          label={t("insights.cost.outliers", "Outliers (≥2× median)")}
          value={String(outliers.length)}
          accent={outliers.length > 0}
        />
      </div>

      {rows.length === 0 ? (
        <Empty
          icon={<DollarSign size={20} />}
          title={t("insights.cost.empty", "No cost data yet")}
          body={t(
            "insights.cost.emptyBody",
            "Either traces in this window don't carry cost_usd (most CLI runtimes don't auto-report cost — needs API-key dispatch with cost telemetry) or there's not enough sample size yet.",
          )}
        />
      ) : (
        <ul className="space-y-1.5">
          {rows.map((r) => (
            <BenchmarkRow key={`${r.agent_slug}|${r.runtime}`} row={r} median={median} />
          ))}
        </ul>
      )}
    </div>
  );
}

function BenchmarkRow({ row, median }: { row: CostBenchmarkRow; median: number }) {
  const { t } = useTranslation();
  const okRate = row.runs > 0 ? row.ok_runs / row.runs : 0;
  // How many multiples of median is this row's cost-per-success.
  // Anchors the outlier badge to a real number, not just a binary flag.
  const multiple = median > 0 ? row.cost_per_ok / median : 0;
  return (
    <li
      className={cn(
        "rounded-lg border p-3",
        row.is_outlier
          ? "border-cs-danger/40 bg-cs-danger/5"
          : "border-cs-border bg-cs-bg-raised/40",
      )}
    >
      <div className="flex items-center gap-2">
        <Cpu size={11} className="text-cs-muted shrink-0" />
        <code className="font-mono text-sm text-cs-text font-medium">@{row.agent_slug}</code>
        <span className="text-[10px] uppercase tracking-wide text-cs-muted">{row.runtime}</span>
        {row.is_outlier && (
          <span
            className="inline-flex items-center gap-1 rounded-full border border-cs-danger/40 bg-cs-danger/10 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide text-cs-danger"
            title={t(
              "insights.cost.outlierTitle",
              "{{m}}× the median cost-per-success across your agents — candidate for a cheaper runtime/model.",
              { m: multiple.toFixed(1) },
            )}
          >
            <TrendingUp size={9} />
            {multiple.toFixed(1)}× median
          </span>
        )}
        <span className="ml-auto font-mono text-cs-muted text-[10px]">
          {row.runs} {t("insights.cost.runsShort", "runs")}
        </span>
      </div>
      <div className="mt-2 grid grid-cols-4 gap-2 text-[11px]">
        <Stat
          label={t("insights.cost.costPerOk", "$/success")}
          value={`$${row.cost_per_ok.toFixed(4)}`}
          accent={row.is_outlier ? "danger" : "neutral"}
        />
        <Stat
          label={t("insights.cost.costPerRun", "$/run")}
          value={`$${row.cost_per_run.toFixed(4)}`}
        />
        <Stat
          label={t("insights.cost.successRate", "OK rate")}
          value={`${(okRate * 100).toFixed(0)}%`}
        />
        <Stat
          label={t("insights.cost.totalSpendShort", "Spend")}
          value={`$${row.total_cost_usd.toFixed(2)}`}
        />
      </div>
    </li>
  );
}

function SummaryStat({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div
      className={cn(
        "rounded-md border p-2",
        accent
          ? "border-cs-danger/40 bg-cs-danger/5"
          : "border-cs-border bg-cs-bg-raised/40",
      )}
    >
      <div className="text-[10px] uppercase tracking-wide text-cs-muted">{label}</div>
      <div
        className={cn(
          "mt-0.5 font-mono text-sm",
          accent ? "text-cs-danger" : "text-cs-text",
        )}
      >
        {value}
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: "danger" | "neutral";
}) {
  return (
    <div className="rounded border border-cs-border bg-cs-bg p-2">
      <div className="text-[9px] uppercase tracking-wide text-cs-muted">{label}</div>
      <div
        className={cn(
          "mt-0.5 font-mono",
          accent === "danger" ? "text-cs-danger" : "text-cs-text",
        )}
      >
        {value}
      </div>
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
