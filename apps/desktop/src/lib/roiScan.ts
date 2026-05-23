import type { CostRecommendation, RegressionRow } from "@/lib/cloudAgentTraces";

// PR-C (2026-05-21) — Day-1 ROI scan analysis.
//
// Pure function over already-fetched local insights (cost recommendations +
// regressions). The tile reads ~/.ato/agent-logs.jsonl via the existing
// `compute_cost_recommendations_local` and `compute_regressions_local` Tauri
// commands — this file just turns the two arrays into the one-line headline
// + supporting numbers the tile renders.
//
// "Day-1 ROI" framing: a user installs ATO, dispatches a few agents, then
// sees the Home tile light up with "ATO saw $X/mo of cost wins". The number
// is real — it comes from their own dispatches; the algorithm is the same
// one that powers Insights → Usage. We just promote the top recommendation
// to the landing page so the value lands at first glance instead of waiting
// for the user to drill into the Insights section.
//
// War-room de8ffb6d-8b39-4b5c-a2e9-6665e6e7e9f3, R1 3/3 LOCK. Q2 locked the
// honest empty state: when `recommendations` is empty we render
// "Run an agent to see your day-1 ROI scan" — no synthetic data.

export interface RoiScanTopRec {
  agentSlug: string;
  currentRuntime: string;
  suggestedRuntime: string;
  monthlySavingsUsd: number;
  savingsPct: number;
}

export interface RoiScanTopRegression {
  agentSlug: string;
  field: string;
  /** Worst-direction percentage delta. For ok-rate this is the drop in
   *  percentage points expressed positively (a 91% → 74% drop is 17).
   *  For cost it's the percent increase. The tile sorts by this value
   *  so the headline always carries the most actionable regression. */
  deltaMagnitude: number;
}

export interface RoiScanResult {
  /** Sum of `projected_monthly_usd` across every "real" cost recommendation
   *  (gates: ≥30% cheaper AND ok-rate within 10pp). The local Rust command
   *  already applies these gates; we just sum what it returns. */
  monthlySavingsUsd: number;
  /** Count of cost recommendations after gating. */
  recCount: number;
  /** Highest single recommendation by `projected_monthly_usd`. Drives the
   *  one-line headline ("Swap @writer claude → minimax for $4.20/mo"). */
  topRec: RoiScanTopRec | null;
  /** Count of regressions flagged severity="regression" (improvements and
   *  neutrals are excluded — only actionable drops surface here). */
  regressionCount: number;
  /** Worst regression by the larger of ok-rate drop (pp) and cost increase
   *  (pct) — single loudest axis wins, no composite score. Powers the
   *  "+ N config changes hurt success rate" supporting line. */
  topRegression: RoiScanTopRegression | null;
  /** True when there is nothing to show. The tile shows the empty-state
   *  CTA in this case. Distinct from `monthlySavingsUsd === 0` because
   *  zero savings with valid scan data still counts as "ran the scan." */
  isEmpty: boolean;
}

/** Analyse a snapshot of local insights into the one summary the Home tile
 *  needs to render. Pure — no I/O, no React, no hooks. Tested in
 *  `test/roiScan.test.ts`. */
export function analyzeRoiScan(
  recommendations: readonly CostRecommendation[],
  regressions: readonly RegressionRow[]
): RoiScanResult {
  const recs = recommendations.filter(
    (r) => Number.isFinite(r.projected_monthly_usd) && r.projected_monthly_usd > 0
  );

  const monthlySavingsUsd = recs.reduce(
    (sum, r) => sum + r.projected_monthly_usd,
    0
  );

  const topRec = recs.length === 0
    ? null
    : recs.reduce((best, r) =>
        r.projected_monthly_usd > best.projected_monthly_usd ? r : best
      );

  const realRegressions = regressions.filter((r) => r.severity === "regression");
  // Rank by the larger of the two damage axes — ok-rate drop (already in
  // percentage points) vs cost increase (already in percent). The two are
  // on roughly the same scale, so a plain max is enough. We negate the
  // ok-rate delta because regressions report negative pp deltas (-17 means
  // success dropped 17 points) and we want a positive magnitude.
  const ranked = realRegressions
    .map((r) => ({
      row: r,
      magnitude: Math.max(
        -r.ok_delta_pp,
        Number.isFinite(r.cost_delta_pct) ? r.cost_delta_pct : 0
      ),
    }))
    .sort((a, b) => b.magnitude - a.magnitude);

  const worst = ranked[0] ?? null;

  return {
    monthlySavingsUsd,
    recCount: recs.length,
    topRec: topRec
      ? {
          agentSlug: topRec.agent_slug,
          currentRuntime: topRec.current_runtime,
          suggestedRuntime: topRec.suggested_runtime,
          monthlySavingsUsd: topRec.projected_monthly_usd,
          savingsPct: topRec.savings_pct,
        }
      : null,
    regressionCount: realRegressions.length,
    topRegression: worst
      ? {
          agentSlug: worst.row.agent_slug,
          field: worst.row.field,
          deltaMagnitude: worst.magnitude,
        }
      : null,
    isEmpty: recs.length === 0 && realRegressions.length === 0,
  };
}

/** Format a USD figure for the tile. Sub-dollar amounts get two decimals so
 *  "$0.42/mo" doesn't truncate to "$0/mo"; anything ≥ $1 rounds to whole
 *  dollars to keep the headline scannable. */
export function formatRoiUsd(usd: number): string {
  if (!Number.isFinite(usd) || usd <= 0) return "$0";
  if (usd < 1) return `$${usd.toFixed(2)}`;
  if (usd < 100) return `$${usd.toFixed(0)}`;
  return `$${Math.round(usd).toLocaleString("en-US")}`;
}
