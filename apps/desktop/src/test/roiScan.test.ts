import { describe, it, expect } from "vitest";
import { analyzeRoiScan, formatRoiUsd } from "@/lib/roiScan";
import type {
  CostRecommendation,
  RegressionRow,
} from "@/lib/cloudAgentTraces";

// PR-C (2026-05-21) — Pure-function tests for the Day-1 ROI scan analysis.
// The tile renders these outputs verbatim; assertions here lock the headline
// math (sum, top-rec selection, top-regression ranking) without spinning up
// React Query / Tauri / the DOM.

function mkRec(o: Partial<CostRecommendation> = {}): CostRecommendation {
  return {
    agent_slug: "writer",
    current_runtime: "claude",
    current_runs: 100,
    current_cost_per_run: 0.01,
    current_ok_rate: 0.92,
    current_eval_score: null,
    suggested_runtime: "minimax",
    suggested_runs: 80,
    suggested_cost_per_run: 0.003,
    suggested_ok_rate: 0.88,
    suggested_eval_score: null,
    savings_per_run_usd: 0.007,
    savings_window_usd: 0.7,
    savings_pct: 70,
    projected_monthly_usd: 4.2,
    ...o,
  };
}

function mkReg(o: Partial<RegressionRow> = {}): RegressionRow {
  return {
    change_id: "c1",
    agent_slug: "writer",
    field: "model",
    old_value: "sonnet-4-6",
    new_value: "sonnet-4-7",
    changed_at: "2026-05-15T00:00:00Z",
    before_runs: 50,
    before_ok_rate: 0.91,
    before_p95_ms: 1200,
    before_cost_per_run: 0.01,
    before_eval_score: null,
    before_eval_count: 0,
    after_runs: 50,
    after_ok_rate: 0.74,
    after_p95_ms: 1500,
    after_cost_per_run: 0.012,
    after_eval_score: null,
    after_eval_count: 0,
    ok_delta_pp: -17,
    p95_delta_pct: 25,
    cost_delta_pct: 20,
    eval_delta_pp: null,
    severity: "regression",
    failing_trace_ids: [],
    ...o,
  } as RegressionRow;
}

describe("analyzeRoiScan", () => {
  it("returns isEmpty=true on no data", () => {
    const r = analyzeRoiScan([], []);
    expect(r.isEmpty).toBe(true);
    expect(r.monthlySavingsUsd).toBe(0);
    expect(r.topRec).toBeNull();
    expect(r.topRegression).toBeNull();
  });

  it("sums monthly savings across recommendations", () => {
    const r = analyzeRoiScan(
      [
        mkRec({ projected_monthly_usd: 3 }),
        mkRec({ projected_monthly_usd: 7 }),
        mkRec({ projected_monthly_usd: 0.5 }),
      ],
      []
    );
    expect(r.monthlySavingsUsd).toBeCloseTo(10.5);
    expect(r.recCount).toBe(3);
  });

  it("picks the highest projected_monthly_usd as topRec", () => {
    const r = analyzeRoiScan(
      [
        mkRec({ agent_slug: "small", projected_monthly_usd: 1.0 }),
        mkRec({ agent_slug: "big", projected_monthly_usd: 9.0, savings_pct: 65 }),
        mkRec({ agent_slug: "medium", projected_monthly_usd: 4.0 }),
      ],
      []
    );
    expect(r.topRec?.agentSlug).toBe("big");
    expect(r.topRec?.monthlySavingsUsd).toBe(9.0);
    expect(r.topRec?.savingsPct).toBe(65);
  });

  it("filters non-finite and non-positive savings from recs", () => {
    const r = analyzeRoiScan(
      [
        mkRec({ projected_monthly_usd: NaN }),
        mkRec({ projected_monthly_usd: 0 }),
        mkRec({ projected_monthly_usd: 2.5 }),
      ],
      []
    );
    expect(r.recCount).toBe(1);
    expect(r.monthlySavingsUsd).toBeCloseTo(2.5);
  });

  it("counts only severity=regression rows", () => {
    const r = analyzeRoiScan(
      [],
      [
        mkReg({ severity: "regression" }),
        mkReg({ severity: "improvement" }),
        mkReg({ severity: "neutral" }),
        mkReg({ severity: "regression" }),
      ]
    );
    expect(r.regressionCount).toBe(2);
  });

  it("ranks topRegression by larger of ok-drop and cost-increase", () => {
    // cost-heavy's 40% cost increase outweighs its tiny 2pp ok-drop;
    // ok-heavy's 25pp ok-drop outweighs its tiny 5% cost bump. The 40
    // wins the head-to-head — the tile surfaces the loudest single
    // signal, not a composite.
    const r = analyzeRoiScan(
      [],
      [
        mkReg({ agent_slug: "cost-heavy", ok_delta_pp: -2, cost_delta_pct: 40 }),
        mkReg({ agent_slug: "ok-heavy", ok_delta_pp: -25, cost_delta_pct: 5 }),
      ]
    );
    expect(r.topRegression?.agentSlug).toBe("cost-heavy");
    expect(r.topRegression?.deltaMagnitude).toBe(40);
  });

  it("falls back to ok-drop when there is no cost data", () => {
    const r = analyzeRoiScan(
      [],
      [
        mkReg({ agent_slug: "small-drop", ok_delta_pp: -5, cost_delta_pct: 0 }),
        mkReg({ agent_slug: "big-drop", ok_delta_pp: -25, cost_delta_pct: 0 }),
      ]
    );
    expect(r.topRegression?.agentSlug).toBe("big-drop");
    expect(r.topRegression?.deltaMagnitude).toBe(25);
  });

  it("isEmpty=false when only regressions exist", () => {
    const r = analyzeRoiScan([], [mkReg()]);
    expect(r.isEmpty).toBe(false);
    expect(r.recCount).toBe(0);
    expect(r.regressionCount).toBe(1);
  });
});

describe("formatRoiUsd", () => {
  it("guards zero / negative / NaN", () => {
    expect(formatRoiUsd(0)).toBe("$0");
    expect(formatRoiUsd(-5)).toBe("$0");
    expect(formatRoiUsd(NaN)).toBe("$0");
  });

  it("shows two decimals under $1", () => {
    expect(formatRoiUsd(0.42)).toBe("$0.42");
    expect(formatRoiUsd(0.09)).toBe("$0.09");
  });

  it("rounds to whole dollars in mid-range", () => {
    expect(formatRoiUsd(4.2)).toBe("$4");
    expect(formatRoiUsd(99.4)).toBe("$99");
  });

  it("comma-formats large amounts", () => {
    expect(formatRoiUsd(1234.5)).toBe("$1,235");
  });
});
