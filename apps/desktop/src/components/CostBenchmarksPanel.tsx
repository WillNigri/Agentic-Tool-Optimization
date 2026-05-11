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
  Lightbulb,
  ArrowRight,
} from "lucide-react";
import {
  getCostBenchmarks,
  getCostRecommendations,
  type CostBenchmarkRow,
  type CostRecommendation,
} from "@/lib/cloudAgentTraces";
// v2.3.2 Phase 2.x — local-mode fallback for cost recommendations.
// Benchmarks (the per-(agent, runtime) cost table) stays cloud-only
// for now since the local aggregator hasn't been ported yet.
import { getCostRecommendationsLocal } from "@/lib/localInsights";
import { useFeatureFlag } from "@/lib/tier";
import { useAuthStore } from "@/hooks/useAuth";
import { asNumber } from "@/lib/pricing";
import { cn } from "@/lib/utils";

// v2.1.0 Phase 8 — Usage benchmarks (descriptive, not prescriptive).
//
// Pitch: "@triage runs cost $0.014/call when you dispatch via API key.
// Your CLI subscription runs don't report cost — only call counts."
//
// Why descriptive:
//   - Real cross-runtime "switching @triage from GPT-4 to Haiku saves
//     $312/mo with no quality drop" requires shadow-evaluation across
//     runtimes, which is a v2.2 feature.
//   - Outlier flagging is purely statistical (cost vs your own
//     median), so it's honest at any data volume.
//   - Subscription dispatches (Claude Code, Codex CLI, Gemini CLI)
//     don't surface per-request cost, so we DON'T fake it. Beatriz
//     feedback 2026-05-09: "we can have X tokens used, Y number of
//     calls — that is different from something it does not make
//     sense like actual cost since it's a subscription."
//
// Anti-pattern avoided: claiming we know which model "should" be used
// based on prompt similarity scores or other ML magic. We don't have
// the data to back that up; calling it out wrong destroys trust.
// Same for fake cost numbers on subscription rows.

export default function CostBenchmarksPanel() {
  const { t } = useTranslation();
  const isPro = useFeatureFlag("cloud-traces");
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const accessToken = useAuthStore((s) => s.accessToken);
  // Mock mode: short-circuit auth so local dev can verify the UI
  // without sign-in. Real prod still needs cloud login.
  const mock = import.meta.env.VITE_USE_MOCK_CLOUD === "true";
  const canQuery = mock || (isCloudUser && accessToken);
  const [days, setDays] = useState<7 | 30 | 90>(30);

  const query = useQuery({
    queryKey: ["cost-benchmarks", days],
    queryFn: () => getCostBenchmarks({ days }),
    enabled: !!canQuery && isPro,
    staleTime: 60_000,
  });

  // Cost recommendations — prescriptive companion. Renders only when
  // the user has historical data on multiple runtimes for the same
  // agent AND the alt is meaningfully cheaper at preserved quality.
  // v2.3.2 Phase 2.x — local fallback so signed-out users still get
  // recs from their own machine's dispatches.
  const recsCloudEligible = !!canQuery && isPro;
  const recsCloudQuery = useQuery({
    queryKey: ["cost-recommendations-cloud", days],
    queryFn: () => getCostRecommendations({ days }),
    enabled: recsCloudEligible,
    staleTime: 60_000,
  });
  const recsLocalQuery = useQuery({
    queryKey: ["cost-recommendations-local", days],
    queryFn: () => getCostRecommendationsLocal({ days }),
    enabled: !recsCloudEligible || recsCloudQuery.isError,
    staleTime: 60_000,
  });
  const recsUsingLocal =
    !recsCloudEligible || recsCloudQuery.isError || !recsCloudQuery.data;
  const recs: CostRecommendation[] = recsUsingLocal
    ? recsLocalQuery.data?.recommendations ?? []
    : recsCloudQuery.data?.recommendations ?? [];
  const recsMode: "cloud" | "local" | "local-no-schema" =
    !recsUsingLocal
      ? "cloud"
      : (recsLocalQuery.data?.source ?? "local") === "local-no-schema"
        ? "local-no-schema"
        : "local";

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
  // v2.1.9 — PG DECIMAL columns serialize as strings. Coerce on every
  // arithmetic/.toFixed read so the panel doesn't crash when real
  // cloud cost values land (was latent before v2.1.4 wired uploads).
  const totalSpend = rows.reduce((acc, r) => acc + asNumber(r.total_cost_usd), 0);
  // v2.1 — split rows by whether their dispatches actually reported
  // cost. Subscription runs (CLI: claude code, codex, gemini-cli)
  // come back with cost_usd = 0; we hide cost columns for those and
  // show a "subscription" badge instead.
  const totalCalls = rows.reduce((acc, r) => acc + r.runs, 0);
  const apiRows = rows.filter((r) => asNumber(r.cost_per_run) > 0);
  const subscriptionRows = rows.filter((r) => asNumber(r.cost_per_run) === 0);

  return (
    <div className="space-y-4">
      <header className="flex items-center justify-between gap-3">
        <div>
          <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
            <DollarSign size={14} className="text-cs-accent" />
            {t("insights.cost.title", "Usage benchmarks")}
          </h3>
          <p className="mt-0.5 text-[11px] text-cs-muted">
            {t(
              "insights.cost.subtitle",
              "Per-(agent, runtime) usage over the window. Calls + latency + success rate are always shown. Cost only when traces report it (API-key dispatches); CLI subscription runs don't surface per-request cost — we don't pretend to know.",
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

      {/* v2.1 Phase 5 — Cost recommendations. Surface above the
          benchmarks because they're the actionable bit. Render-nothing
          when no recs in the window so the panel stays clean.
          v2.3.2 Phase 2.x — cloud preferred, local fallback. The
          local-no-schema case renders nothing too. */}
      {recsMode !== "local-no-schema" && (
        <RecommendationsSection
          recs={recs}
          windowDays={days}
          mode={recsMode}
        />
      )}

      {/* Summary bar. Show total calls always; cost stats only when
          some row actually reported cost. */}
      <div
        className={cn(
          "grid gap-2 text-[11px]",
          apiRows.length > 0 ? "grid-cols-4" : "grid-cols-2",
        )}
      >
        <SummaryStat
          label={t("insights.cost.totalCalls", "Total calls")}
          value={totalCalls.toLocaleString()}
        />
        <SummaryStat
          label={t("insights.cost.apiVsSub", "API / subscription rows")}
          value={`${apiRows.length} / ${subscriptionRows.length}`}
        />
        {apiRows.length > 0 && (
          <>
            <SummaryStat
              label={t("insights.cost.totalSpend", "API spend (window)")}
              value={`$${totalSpend.toFixed(2)}`}
            />
            <SummaryStat
              label={t("insights.cost.outliers", "Outliers (≥2× median)")}
              value={String(outliers.length)}
              accent={outliers.length > 0}
            />
          </>
        )}
      </div>

      {rows.length === 0 ? (
        <Empty
          icon={<DollarSign size={20} />}
          title={t("insights.cost.empty", "No usage data yet")}
          body={t(
            "insights.cost.emptyBody",
            "Either no traces in this window, or below the minimum sample size for the rollup.",
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
  // v2.1 — split rendering by whether we have real cost data. API
  // dispatches surface cost via provider response → cost_per_run > 0.
  // Subscription dispatches don't surface cost; we show calls + p50
  // + ok rate + a "subscription" badge instead of fake $0.0000 cells.
  const hasCostData = asNumber(row.cost_per_run) > 0;
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
        {!hasCostData && (
          <span
            className="inline-flex items-center gap-1 rounded-full border border-cs-border bg-cs-bg-raised px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide text-cs-muted"
            title={t(
              "insights.cost.subscriptionTitle",
              "These dispatches went via a CLI subscription (Claude Code, Codex, Gemini CLI) which doesn't surface per-request cost. Calls + latency are real.",
            )}
          >
            {t("insights.cost.subscriptionBadge", "subscription")}
          </span>
        )}
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
          {row.runs} {t("insights.cost.runsShort", "calls")}
        </span>
      </div>
      <div className="mt-2 grid grid-cols-4 gap-2 text-[11px]">
        {/* Always real, always shown. */}
        <Stat
          label={t("insights.cost.calls", "Calls")}
          value={row.runs.toLocaleString()}
        />
        <Stat
          label={t("insights.cost.p50", "p50 latency")}
          value={`${row.p50_ms}ms`}
        />
        <Stat
          label={t("insights.cost.successRate", "OK rate")}
          value={`${(okRate * 100).toFixed(0)}%`}
        />
        {hasCostData ? (
          <Stat
            label={t("insights.cost.costPerOk", "$/success")}
            value={`$${asNumber(row.cost_per_ok).toFixed(4)}`}
            accent={row.is_outlier ? "danger" : "neutral"}
          />
        ) : (
          <Stat
            label={t("insights.cost.costPerOk", "$/success")}
            value="—"
          />
        )}
      </div>
      {/* Second row of cost-only stats for API rows that have real
          data. Hidden entirely for subscription rows so the layout
          stays honest about what we know. */}
      {hasCostData && (
        <div className="mt-2 grid grid-cols-2 gap-2 text-[11px]">
          <Stat
            label={t("insights.cost.costPerRun", "$/call")}
            value={`$${asNumber(row.cost_per_run).toFixed(4)}`}
          />
          <Stat
            label={t("insights.cost.totalSpendShort", "Total spend")}
            value={`$${asNumber(row.total_cost_usd).toFixed(2)}`}
          />
        </div>
      )}
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

// v2.1 Phase 5 — Cost recommendations section. Prescriptive: surfaces
// concrete swaps based on the user's own historical data (not synthetic
// benchmarks, not predictions). Render-nothing when empty so the
// underlying benchmarks panel stays as the single source of cost truth.
function RecommendationsSection({
  recs,
  windowDays,
  mode,
}: {
  recs: CostRecommendation[];
  windowDays: number;
  // v2.3.2 Phase 2.x — surfaces which data source the recommendations
  // come from so the user knows whether this includes cross-device
  // aggregation (cloud) or just this machine's dispatches (local).
  mode: "cloud" | "local" | "local-no-schema";
}) {
  const { t } = useTranslation();
  if (recs.length === 0) return null;
  return (
    <section className="rounded-lg border border-cs-accent/30 bg-cs-accent/5 p-3 space-y-2">
      <header className="flex items-center gap-2">
        <Lightbulb size={13} className="text-cs-accent shrink-0" />
        <h4 className="text-[12px] font-medium text-cs-text">
          {t("insights.cost.recsTitle", "Cost recommendations")}
        </h4>
        <span
          title={
            mode === "cloud"
              ? "Cross-device aggregation (Pro)"
              : "Computed from this machine's dispatches only. Sign in for cross-device aggregation."
          }
          className={cn(
            "inline-flex items-center gap-1 rounded-full border px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide",
            mode === "cloud"
              ? "border-cs-accent/40 bg-cs-accent/10 text-cs-accent"
              : "border-cs-border bg-cs-bg-raised text-cs-muted",
          )}
        >
          {mode === "cloud" ? "cloud" : "local"}
        </span>
        <span className="text-[10px] text-cs-muted">
          {t(
            "insights.cost.recsSubtitle",
            "Same-agent swaps where you already have data on both sides — quality preserved.",
          )}
        </span>
      </header>
      <ul className="space-y-1.5">
        {recs.map((r, i) => (
          <RecRow key={`${r.agent_slug}-${r.suggested_runtime}-${i}`} rec={r} windowDays={windowDays} />
        ))}
      </ul>
    </section>
  );
}

function RecRow({ rec, windowDays }: { rec: CostRecommendation; windowDays: number }) {
  const { t } = useTranslation();
  const evalShown = rec.current_eval_score !== null && rec.suggested_eval_score !== null;
  return (
    <li className="rounded-md border border-cs-border bg-cs-bg p-2.5">
      <div className="flex items-center gap-2 flex-wrap">
        <code className="font-mono text-sm text-cs-text font-medium">@{rec.agent_slug}</code>
        <span className="inline-flex items-center gap-1 text-[11px] text-cs-muted">
          <span className="font-mono text-cs-text">{rec.current_runtime}</span>
          <ArrowRight size={11} className="text-cs-accent shrink-0" />
          <span className="font-mono text-cs-accent">{rec.suggested_runtime}</span>
        </span>
        <span className="ml-auto inline-flex items-baseline gap-1 font-mono text-[11px]">
          <span className="text-cs-accent font-medium">
            -{asNumber(rec.savings_pct).toFixed(0)}%
          </span>
          <span className="text-cs-muted">
            {t("insights.cost.recsPerCall", "/ call")}
          </span>
        </span>
      </div>
      <div className="mt-1 grid grid-cols-3 gap-2 text-[10px] text-cs-muted">
        <RecStat
          label={t("insights.cost.recsCurrent", "Current")}
          value={`$${asNumber(rec.current_cost_per_run).toFixed(4)}`}
          sub={`${rec.current_runs} ${t("insights.cost.recsRuns", "runs")} · ok ${(asNumber(rec.current_ok_rate) * 100).toFixed(0)}%${evalShown ? ` · eval ${asNumber(rec.current_eval_score).toFixed(2)}` : ""}`}
        />
        <RecStat
          label={t("insights.cost.recsAlternative", "Alternative")}
          value={`$${asNumber(rec.suggested_cost_per_run).toFixed(4)}`}
          sub={`${rec.suggested_runs} ${t("insights.cost.recsRuns", "runs")} · ok ${(asNumber(rec.suggested_ok_rate) * 100).toFixed(0)}%${evalShown ? ` · eval ${asNumber(rec.suggested_eval_score).toFixed(2)}` : ""}`}
        />
        <RecStat
          label={t("insights.cost.recsProjMonthly", "Projected /mo")}
          value={`$${asNumber(rec.projected_monthly_usd).toFixed(2)}`}
          sub={t(
            "insights.cost.recsProjAtVolume",
            "at this {{n}}d volume",
            { n: windowDays },
          )}
        />
      </div>
    </li>
  );
}

function RecStat({ label, value, sub }: { label: string; value: string; sub: string }) {
  return (
    <div className="rounded border border-cs-border bg-cs-bg-raised/30 p-1.5">
      <div className="text-[9px] uppercase tracking-wide text-cs-muted">{label}</div>
      <div className="mt-0.5 font-mono text-cs-text">{value}</div>
      <div className="text-[9px] text-cs-muted">{sub}</div>
    </div>
  );
}
