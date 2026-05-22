import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { TrendingDown, Sparkles, AlertTriangle, AlertCircle, ArrowRight } from "lucide-react";
import {
  getCostRecommendationsLocal,
  getRegressionsLocal,
} from "@/lib/localInsights";
import { analyzeRoiScan, formatRoiUsd } from "@/lib/roiScan";

// PR-C (2026-05-21) — Day-1 ROI scan tile.
//
// Mounted on Home below the recent-agents section. Reads local cost
// recommendations + regressions (no cloud sign-in needed) and renders one of
// three states:
//
//   1. idle / loading  — "Scanning your dispatches…" (first paint never blocks
//      on this; see the requestIdleCallback gate below).
//   2. has data        — "ATO saw $X/mo of cost wins. Open Insights →"
//   3. empty / error   — "Run an agent to see your day-1 ROI scan" CTA.
//
// War-room de8ffb6d-8b39-4b5c-a2e9-6665e6e7e9f3, R1 3/3 LOCK locked:
//   - Q1: requestIdleCallback wrapper around useQuery `enabled` so heavy
//     ~/.ato/agent-logs.jsonl reads (multi-MB on long-running users) never
//     delay first paint. setTimeout(0) fallback for Safari.
//   - Q2: honest empty state — no synthetic numbers, no fixture demo.
//   - Q5: Roi (PascalCase code) + ROI (user-facing copy).

interface IdleCallbackHandle {
  cancel: () => void;
}

function scheduleIdle(cb: () => void): IdleCallbackHandle {
  if (typeof window === "undefined") {
    cb();
    return { cancel: () => {} };
  }
  // Safari (and any non-modern host) lacks requestIdleCallback; fall back
  // to a 0ms timer so the work still defers past the current paint.
  type IdleHost = Window & {
    requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => number;
    cancelIdleCallback?: (handle: number) => void;
  };
  const host = window as IdleHost;
  if (typeof host.requestIdleCallback === "function") {
    const id = host.requestIdleCallback(cb, { timeout: 2000 });
    return { cancel: () => host.cancelIdleCallback?.(id) };
  }
  const id = window.setTimeout(cb, 0);
  return { cancel: () => window.clearTimeout(id) };
}

/** Hook: returns true once the browser has been idle at least once after
 *  mount. The ROI scan queries are gated on this so they never block the
 *  first paint of Home. */
function useIdleEnabled(): boolean {
  const [enabled, setEnabled] = useState(false);
  useEffect(() => {
    const handle = scheduleIdle(() => setEnabled(true));
    return () => handle.cancel();
  }, []);
  return enabled;
}

interface Props {
  /** Optional click handler that opens Insights → Regressions / Usage.
   *  Falls back to a no-op so the tile still renders in isolation tests. */
  onOpenInsights?: () => void;
}

export default function RoiScanTile({ onOpenInsights }: Props) {
  const { t } = useTranslation();
  const enabled = useIdleEnabled();

  const { data: costData, isLoading: costLoading, isError: costError } = useQuery({
    queryKey: ["roi-scan-cost"],
    queryFn: () => getCostRecommendationsLocal({ days: 30 }),
    enabled,
    staleTime: 60_000,
  });

  const { data: regData, isLoading: regLoading, isError: regError } = useQuery({
    queryKey: ["roi-scan-regressions"],
    queryFn: () => getRegressionsLocal({ days: 30 }),
    enabled,
    staleTime: 60_000,
  });

  // Show the loading skeleton only after the idle gate fires — before that
  // the tile renders the same compact "Scanning…" frame, which keeps the
  // layout stable but tells the user something will arrive.
  const loading = !enabled || costLoading || regLoading;
  // PR-C code-review (war-room round 2) — gemini MAJOR finding: a failed
  // Tauri invoke would render the empty-state CTA ("Run an agent…") which
  // misleads users into thinking they have no data when really the scan
  // couldn't read ~/.ato/local.db. Surfacing a distinct error chip keeps
  // the empty CTA honest. Rare in practice (the Rust commands gracefully
  // return source="local-no-schema" instead of throwing) but the path
  // exists, so we cover it.
  const errored = enabled && (costError || regError);

  const result = (() => {
    if (!costData || !regData) return null;
    return analyzeRoiScan(costData.recommendations, regData.regressions);
  })();

  return (
    <section
      className="rounded-lg border border-cs-border bg-cs-card p-4"
      aria-label={t("home.roiScan.aria", "Day-1 ROI scan")}
    >
      <header className="flex items-center gap-2 mb-3">
        <span className="inline-flex items-center justify-center w-7 h-7 rounded-md bg-cs-accent/10 text-cs-accent">
          <Sparkles size={14} />
        </span>
        <h2 className="text-sm font-medium text-cs-text">
          {t("home.roiScan.title", "Day-1 ROI scan")}
        </h2>
        <span className="ml-auto text-[10px] uppercase tracking-wide text-cs-muted">
          {t("home.roiScan.window", "Last 30 days")}
        </span>
      </header>

      {loading ? (
        <p className="text-xs text-cs-muted" data-testid="roi-scan-loading">
          {t("home.roiScan.loading", "Scanning your dispatches…")}
        </p>
      ) : errored ? (
        <div
          className="flex items-start gap-2 rounded-md border border-cs-warning/40 bg-cs-warning/10 px-3 py-2"
          data-testid="roi-scan-error"
        >
          <AlertCircle size={14} className="text-cs-warning shrink-0 mt-0.5" />
          <p className="text-xs text-cs-text">
            {t(
              "home.roiScan.error",
              "Couldn't read local insights — Tauri command failed. Check that ~/.ato/local.db is readable."
            )}
          </p>
        </div>
      ) : result === null || result.isEmpty ? (
        <button
          type="button"
          onClick={onOpenInsights}
          data-testid="roi-scan-empty"
          className="w-full text-left rounded-md border border-dashed border-cs-border bg-cs-bg-raised/40 px-4 py-3 hover:border-cs-accent/40 hover:bg-cs-accent/5 transition"
        >
          <p className="text-sm text-cs-text">
            {t(
              "home.roiScan.empty",
              "Run an agent to see your day-1 ROI scan."
            )}
          </p>
          <p className="mt-0.5 text-xs text-cs-muted">
            {t(
              "home.roiScan.emptyHint",
              "ATO measures cost wins + regressions across every dispatch."
            )}
          </p>
        </button>
      ) : (
        <RoiSummary
          result={result}
          onOpenInsights={onOpenInsights}
          t={t}
        />
      )}
    </section>
  );
}

interface SummaryProps {
  result: NonNullable<ReturnType<typeof analyzeRoiScan>>;
  onOpenInsights?: () => void;
  t: (key: string, def: string, vars?: Record<string, unknown>) => string;
}

function RoiSummary({ result, onOpenInsights, t }: SummaryProps) {
  const { monthlySavingsUsd, topRec, regressionCount, topRegression } = result;
  const usd = formatRoiUsd(monthlySavingsUsd);

  return (
    <div className="space-y-3">
      {topRec && monthlySavingsUsd > 0 && (
        <div
          className="flex items-start gap-3"
          data-testid="roi-scan-savings"
        >
          <TrendingDown size={16} className="text-cs-accent shrink-0 mt-0.5" />
          <div className="flex-1 min-w-0">
            <p className="text-sm text-cs-text">
              {t(
                "home.roiScan.savingsHeadline",
                "ATO saw {{usd}}/mo of cost wins on your dispatches.",
                { usd }
              )}
            </p>
            <p className="mt-0.5 text-xs text-cs-muted truncate">
              {t(
                "home.roiScan.savingsDetail",
                "Top swap: @{{slug}} {{from}} → {{to}} ({{pct}}% cheaper).",
                {
                  slug: topRec.agentSlug,
                  from: topRec.currentRuntime,
                  to: topRec.suggestedRuntime,
                  pct: Math.round(topRec.savingsPct),
                }
              )}
            </p>
          </div>
        </div>
      )}

      {topRegression && (
        <div
          className="flex items-start gap-3"
          data-testid="roi-scan-regression"
        >
          <AlertTriangle size={16} className="text-cs-warning shrink-0 mt-0.5" />
          <div className="flex-1 min-w-0">
            <p className="text-sm text-cs-text">
              {regressionCount === 1
                ? t(
                    "home.roiScan.regressionSingular",
                    "1 config change hurt @{{slug}} ({{field}}).",
                    {
                      slug: topRegression.agentSlug,
                      field: topRegression.field,
                    }
                  )
                : t(
                    "home.roiScan.regressionPlural",
                    "{{count}} config changes hurt success rate — worst on @{{slug}}.",
                    {
                      count: regressionCount,
                      slug: topRegression.agentSlug,
                    }
                  )}
            </p>
            <p className="mt-0.5 text-xs text-cs-muted">
              {t(
                "home.roiScan.regressionHint",
                "Open Insights → Regressions to see the before/after deltas."
              )}
            </p>
          </div>
        </div>
      )}

      {onOpenInsights && (
        <button
          type="button"
          onClick={onOpenInsights}
          className="inline-flex items-center gap-1 text-xs font-medium text-cs-accent hover:text-cs-accent-hover"
          data-testid="roi-scan-open-insights"
        >
          {t("home.roiScan.openInsights", "Open Insights")}
          <ArrowRight size={12} />
        </button>
      )}
    </div>
  );
}
