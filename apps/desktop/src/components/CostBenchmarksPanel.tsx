import { useMemo, useState } from "react";
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
  Eye,
  EyeOff,
} from "lucide-react";
import {
  getCostBenchmarks,
  getCostRecommendations,
  getProviderUsage,
  type CostBenchmarkRow,
  type CostRecommendation,
  type ProviderUsageRow,
} from "@/lib/cloudAgentTraces";
// v2.3.2 Phase 2.x — local-mode fallback for cost recommendations.
// Benchmarks (the per-(agent, runtime) cost table) stays cloud-only
// for now since the local aggregator hasn't been ported yet.
// v2.6 PR-A — billing-surface summary is local-only (passive
// observations only exist on this machine until PR-B ships cloud
// polling).
import {
  getCostRecommendationsLocal,
  getBillingSurfaceSummary,
  type BillingSurfaceRow,
} from "@/lib/localInsights";
import { billingSurfaceLabel } from "@/lib/activeRuns";
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

  // v2.6 PR-A — local observatory summary. Always-on regardless of
  // cloud auth because passive observations + ATO's own dispatches
  // both land in execution_logs on this machine.
  const summaryDays = days <= 7 ? 7 : 30;
  const summaryQuery = useQuery({
    queryKey: ["billing-surface-summary", summaryDays],
    queryFn: () => getBillingSurfaceSummary({ days: summaryDays }),
    staleTime: 30_000,
  });
  // Group-by toggle: by-agent (v2.1 cloud benchmarks) OR by-billing-
  // surface (v2.6 PR-A local rollup) OR by-cloud-provider (v2.6 PR-B
  // chunk 5 — what each provider's own usage API says was billed).
  // Persisted in component state only — short session-scoped pref,
  // not worth a settings round-trip.
  const [groupBy, setGroupBy] = useState<
    "agent" | "billing_surface" | "cloud_provider"
  >("agent");

  // v2.6 PR-B chunk 5 — cloud-polled provider usage. Only fires when
  // the user has cloud auth (Pro+ tier) AND has actually selected the
  // cloud_provider view, so the daily-aggregate fetch stays lazy.
  const cloudProviderQuery = useQuery({
    queryKey: ["cloud-provider-usage", days],
    queryFn: () => getProviderUsage({ days }),
    enabled: !!canQuery && isPro && groupBy === "cloud_provider",
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
  const summary = summaryQuery.data;
  // Surface rows for the by-billing-surface view. Coalesce passive +
  // active under the same surface so the user sees "Claude Code
  // Subscription: 47 calls" regardless of who fired them.
  const surfaceRows = useMemo(() => {
    if (!summary) return [];
    const map = new Map<string, BillingSurfaceRow>();
    for (const r of summary.rows) {
      const existing = map.get(r.billing_surface);
      if (!existing) {
        map.set(r.billing_surface, { ...r, dispatch_kind: "merged" });
      } else {
        existing.runs += r.runs;
        existing.tokens_in += r.tokens_in;
        existing.tokens_out += r.tokens_out;
        existing.cost_usd += r.cost_usd;
        existing.duration_seconds += r.duration_seconds;
      }
    }
    return Array.from(map.values());
  }, [summary]);
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

      {/* v2.6 PR-A — "Last 7 days at a glance" header card. Three big
          numbers + the blind-spot caveat. Always rendered (even when
          empty) because the candor about what we *can't* see is the
          positioning differentiator. */}
      <GlanceCard
        days={summaryQuery.data?.days ?? summaryDays}
        subscriptionHours={summaryQuery.data?.subscription_hours ?? 0}
        apiSpend={summaryQuery.data?.api_spend_usd ?? 0}
        totalRuns={summaryQuery.data?.total_runs ?? 0}
        loading={summaryQuery.isLoading}
      />

      {/* Group-by toggle. By-agent = v2.1 cloud benchmarks. By-billing-
          surface = v2.6 PR-A local rollup including passive observations.
          By-cloud-provider = v2.6 PR-B chunk 5: what each provider's own
          usage API says was billed (authoritative; includes activity
          from non-observable surfaces like phone apps + web UI). */}
      <div className="inline-flex rounded-md border border-cs-border bg-cs-bg-raised p-0.5 text-[11px]">
        {(
          [
            ["agent", t("insights.cost.groupByAgent", "By agent")],
            [
              "billing_surface",
              t("insights.cost.groupBySurface", "By billing surface"),
            ],
            [
              "cloud_provider",
              t("insights.cost.groupByCloudProvider", "By cloud provider"),
            ],
          ] as const
        ).map(([key, label]) => (
          <button
            key={key}
            type="button"
            onClick={() => setGroupBy(key)}
            className={cn(
              "rounded px-2.5 py-1 font-medium transition",
              groupBy === key
                ? "bg-cs-accent/15 text-cs-accent"
                : "text-cs-muted hover:text-cs-text",
            )}
          >
            {label}
          </button>
        ))}
      </div>

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

      {groupBy === "billing_surface" ? (
        surfaceRows.length === 0 ? (
          <Empty
            icon={<Eye size={20} />}
            title={t("insights.cost.surfaceEmpty", "No observed sessions yet")}
            body={t(
              "insights.cost.surfaceEmptyBody",
              "Fire a dispatch from ATO or run any supported CLI (Claude Code, Codex) in another terminal — rows will appear here as they happen.",
            )}
          />
        ) : (
          <ul className="space-y-1.5">
            {surfaceRows.map((r) => (
              <SurfaceRow key={r.billing_surface} row={r} />
            ))}
          </ul>
        )
      ) : groupBy === "cloud_provider" ? (
        cloudProviderQuery.isLoading ? (
          <div className="flex items-center justify-center h-24 text-cs-muted">
            <Loader2 size={16} className="animate-spin mr-2" />
            {t("insights.cost.cloudProviderLoading", "Loading cloud-reported usage…")}
          </div>
        ) : cloudProviderQuery.isError ? (
          // Network / 5xx must NOT silently fall through to the empty
          // state — that would make a real failure indistinguishable
          // from "no data yet" (review fixup: MiniMax #1).
          <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
            <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
            <span>
              {t(
                "insights.cost.cloudProviderError",
                "Couldn't load cloud-reported usage",
              )}
              : {String(cloudProviderQuery.error)}
            </span>
          </div>
        ) : (cloudProviderQuery.data?.rows ?? []).length === 0 ? (
          <Empty
            icon={<Cloud size={20} />}
            title={t("insights.cost.cloudProviderEmpty", "No cloud-polled data yet")}
            body={t(
              "insights.cost.cloudProviderEmptyBody",
              "Add a provider API key in Settings → Cloud → Provider Keys. ato-cloud polls each provider's usage API daily, then surfaces the authoritative totals here — including activity from phone apps and web UI that the local watcher can't see.",
            )}
          />
        ) : (
          <ul className="space-y-1.5">
            {(cloudProviderQuery.data?.rows ?? []).map((r) => (
              <CloudProviderRow key={r.provider} row={r} requestedDays={days} />
            ))}
          </ul>
        )
      ) : rows.length === 0 ? (
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

// v2.6 PR-A — "Last 7 days at a glance" header card.
function GlanceCard({
  days,
  subscriptionHours,
  apiSpend,
  totalRuns,
  loading,
}: {
  days: number;
  subscriptionHours: number;
  apiSpend: number;
  totalRuns: number;
  loading: boolean;
}) {
  const { t } = useTranslation();
  return (
    <section className="rounded-lg border border-cs-border bg-cs-bg-raised/30 p-3">
      <header className="flex items-center justify-between">
        <h4 className="flex items-center gap-1.5 text-[12px] font-medium text-cs-text">
          <Eye size={12} className="text-cs-accent" />
          {t("insights.cost.glanceTitle", "Last {{n}} days at a glance", { n: days })}
        </h4>
        {loading && <Loader2 size={11} className="animate-spin text-cs-muted" />}
      </header>
      <div className="mt-2 grid grid-cols-3 gap-2 text-[11px]">
        <GlanceStat
          label={t("insights.cost.glanceSubHours", "Subscription hours")}
          value={subscriptionHours >= 1 ? `${subscriptionHours.toFixed(1)}h` : `${Math.round(subscriptionHours * 60)}m`}
          hint={t(
            "insights.cost.glanceSubHint",
            "Wall-clock on flat-rate CLIs (Claude Code, Codex, Gemini CLI).",
          )}
        />
        <GlanceStat
          label={t("insights.cost.glanceApiSpend", "API spend")}
          value={`$${apiSpend.toFixed(2)}`}
          hint={t(
            "insights.cost.glanceApiHint",
            "Per-token spend across ATO + observed sessions on this machine.",
          )}
        />
        <GlanceStat
          label={t("insights.cost.glanceTotalRuns", "Total runs")}
          value={totalRuns.toLocaleString()}
          hint={t(
            "insights.cost.glanceTotalHint",
            "ATO dispatches + passively observed CLI turns.",
          )}
        />
      </div>
      {/* Blind-spot line. The candor IS the differentiator — surfaces
          we can't observe, surfaced honestly so the user knows what
          this number is NOT counting. */}
      <p className="mt-2 flex items-start gap-1.5 text-[10px] text-cs-muted">
        <EyeOff size={11} className="text-cs-muted/80 shrink-0 mt-0.5" />
        <span>
          {t(
            "insights.cost.blindSpot",
            "Blind spot: phone Claude / ChatGPT apps and claude.ai web on consumer plans don't surface a usage API — not observable from here. Connect provider keys in Settings → Cloud (Pro) for cross-device totals once v2.6 PR-B ships.",
          )}
        </span>
      </p>
    </section>
  );
}

function GlanceStat({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint: string;
}) {
  return (
    <div
      className="rounded-md border border-cs-border bg-cs-bg p-2"
      title={hint}
    >
      <div className="text-[9px] uppercase tracking-wide text-cs-muted">{label}</div>
      <div className="mt-0.5 font-mono text-base text-cs-text">{value}</div>
    </div>
  );
}

// v2.6 PR-A — by-billing-surface row. Subscription surfaces show
// duration + call count + "—" for cost (no per-call $ to attribute);
// API surfaces show spend + tokens.
function SurfaceRow({ row }: { row: BillingSurfaceRow }) {
  const { t } = useTranslation();
  const isSubscription =
    row.billing_surface.endsWith("_subscription") ||
    row.billing_surface === "ollama_local";
  return (
    <li className="rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3">
      <div className="flex items-center gap-2 flex-wrap">
        <Cpu size={11} className="text-cs-muted shrink-0" />
        <span className="text-sm text-cs-text font-medium">
          {billingSurfaceLabel(
            row.billing_surface as Parameters<typeof billingSurfaceLabel>[0],
          )}
        </span>
        <span
          className={cn(
            "inline-flex items-center rounded-full border px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide",
            isSubscription
              ? "border-cs-border bg-cs-bg-raised text-cs-muted"
              : "border-cs-accent/30 bg-cs-accent/10 text-cs-accent",
          )}
        >
          {isSubscription
            ? t("insights.cost.surfaceFlat", "flat-rate")
            : t("insights.cost.surfacePerToken", "per-token")}
        </span>
        <span className="ml-auto font-mono text-cs-muted text-[10px]">
          {row.runs.toLocaleString()} {t("insights.cost.runsShort", "calls")}
        </span>
      </div>
      <div className="mt-2 grid grid-cols-4 gap-2 text-[11px]">
        <Stat
          label={t("insights.cost.calls", "Calls")}
          value={row.runs.toLocaleString()}
        />
        <Stat
          label={t("insights.cost.tokensIn", "Tokens in")}
          value={row.tokens_in.toLocaleString()}
        />
        <Stat
          label={t("insights.cost.tokensOut", "Tokens out")}
          value={row.tokens_out.toLocaleString()}
        />
        {isSubscription ? (
          <Stat
            label={t("insights.cost.spend", "Spend")}
            value="—"
          />
        ) : (
          <Stat
            label={t("insights.cost.spend", "Spend")}
            value={`$${row.cost_usd.toFixed(2)}`}
          />
        )}
      </div>
    </li>
  );
}

// v2.6 PR-B chunk 5 — by-cloud-provider row. Renders one provider's
// authoritative usage as reported by its own usage API (polled daily
// by ato-cloud's usage-poller). Differs from SurfaceRow in two ways:
//   1. Source is the provider's books, not local observations — so
//      this row INCLUDES activity the local watcher cannot see (phone,
//      web UI, other machines).
//   2. Cost is per-token where the provider returns it; null where it
//      doesn't (OpenAI's /v1/usage doesn't include cost; rendered as
//      "—" with a tooltip).
function CloudProviderRow({
  row,
  requestedDays,
}: {
  row: ProviderUsageRow;
  requestedDays: number;
}) {
  const { t } = useTranslation();
  // Mirror the cost-null guard for tokens/requests too. The TypeScript
  // interface says number | string but PG decimal serialization and
  // future provider-adapter regressions could legitimately produce
  // null — rendering "0" would silently mislead. (Review fixup:
  // MiniMax #2.)
  const cost = row.total_cost_usd == null ? null : asNumber(row.total_cost_usd);
  const requests = row.total_requests == null ? null : asNumber(row.total_requests);
  const tokensIn = row.total_tokens_in == null ? null : asNumber(row.total_tokens_in);
  const tokensOut = row.total_tokens_out == null ? null : asNumber(row.total_tokens_out);
  const partialWindow = row.rows_polled > 0 && row.rows_polled < requestedDays;
  return (
    <li className="rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3">
      <div className="flex items-center gap-2 flex-wrap">
        <Cloud size={11} className="text-cs-accent shrink-0" />
        <span className="text-sm text-cs-text font-medium font-mono">
          {row.provider}
        </span>
        <span
          className="inline-flex items-center rounded-full border border-cs-accent/30 bg-cs-accent/10 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide text-cs-accent"
          title={t(
            "insights.cost.cloudProviderBadgeTitle",
            "Provider-side ground truth — polled from the provider's own usage API. Includes activity from sources the local watcher can't see (phone apps, web UI, other machines).",
          )}
        >
          {t("insights.cost.cloudProviderBadge", "provider-reported")}
        </span>
        <span
          className={cn(
            "ml-auto font-mono text-[10px]",
            partialWindow ? "text-cs-warning" : "text-cs-muted",
          )}
          title={
            partialWindow
              ? t(
                  "insights.cost.partialWindowTitle",
                  "Only {{polled}} of the requested {{requested}} days have been polled — provider key added recently, or the cron missed runs during an outage.",
                  { polled: row.rows_polled, requested: requestedDays },
                )
              : undefined
          }
        >
          {partialWindow
            ? t("insights.cost.partialWindow", "{{polled}}/{{requested}} days polled", {
                polled: row.rows_polled,
                requested: requestedDays,
              })
            : `${row.rows_polled} ${t("insights.cost.daysShort", "days polled")}`}
        </span>
      </div>
      <div className="mt-2 grid grid-cols-4 gap-2 text-[11px]">
        <Stat
          label={t("insights.cost.requests", "Requests")}
          value={requests === null ? "—" : requests.toLocaleString()}
        />
        <Stat
          label={t("insights.cost.tokensIn", "Tokens in")}
          value={tokensIn === null ? "—" : tokensIn.toLocaleString()}
        />
        <Stat
          label={t("insights.cost.tokensOut", "Tokens out")}
          value={tokensOut === null ? "—" : tokensOut.toLocaleString()}
        />
        {cost === null ? (
          <Stat
            label={t("insights.cost.spend", "Spend")}
            value="—"
          />
        ) : (
          <Stat
            label={t("insights.cost.spend", "Spend")}
            value={`$${cost.toFixed(2)}`}
          />
        )}
      </div>
    </li>
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
