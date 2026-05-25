// v2.10 PR-8 — compose.ts unit tests. Mirrors the Rust suite in
// apps/cli/src/methodology/compose.rs so the UI math stays aligned with
// the CLI's authoritative composer.

import { describe, expect, it } from "vitest";
import {
  composeCells,
  erfApprox,
  mean,
  normalCdf,
  sampleSd,
  stats,
  tCritical95,
  welchPValueApprox,
} from "@/components/MethodologiesPanel/compose";

function dispatch(
  promptIdx: number,
  model: string,
  condition: string,
  cost: number,
  opts: { score?: number | null; status?: string; verdict?: string } = {},
) {
  return {
    variant_cell: JSON.stringify({
      prompt_idx: promptIdx,
      model,
      condition,
    }),
    score: opts.score ?? null,
    cost_usd: cost,
    tokens_in: 100,
    tokens_out: cost * 1000,
    duration_ms: cost * 10000,
    status: opts.status ?? "success",
    grounding_verdict: opts.verdict ?? null,
  };
}

describe("mean", () => {
  it("returns 0 for empty input", () => {
    expect(mean([])).toBe(0);
  });
  it("matches known mean", () => {
    expect(mean([1, 2, 3, 4, 5])).toBeCloseTo(3);
  });
});

describe("sampleSd", () => {
  it("returns 0 for singleton (avoids NaN)", () => {
    expect(sampleSd([42])).toBe(0);
  });
  it("matches textbook value", () => {
    // SD of [2,4,4,4,5,5,7,9] ≈ 2.138 (sample, n-1 denominator)
    expect(sampleSd([2, 4, 4, 4, 5, 5, 7, 9])).toBeCloseTo(2.138, 3);
  });
});

describe("erfApprox + normalCdf + welchPValueApprox", () => {
  it("erf at 0 is 0", () => {
    expect(erfApprox(0)).toBeCloseTo(0, 6);
  });
  it("normalCdf at 0 is 0.5", () => {
    expect(normalCdf(0)).toBeCloseTo(0.5, 5);
  });
  it("matches published z→p table values", () => {
    expect(normalCdf(1.96)).toBeCloseTo(0.975, 3);
    expect(normalCdf(-1.96)).toBeCloseTo(0.025, 3);
  });
  it("returns null for df < 10", () => {
    expect(welchPValueApprox(2.0, 5)).toBeNull();
    expect(welchPValueApprox(2.0, 9.99)).toBeNull();
  });
  it("translates t=1.96 / df=30 to p ≈ 0.05", () => {
    const p = welchPValueApprox(1.96, 30);
    expect(p).not.toBeNull();
    expect(Math.abs((p as number) - 0.05)).toBeLessThan(0.01);
  });
  it("clamps to near-zero at large |t|", () => {
    expect(welchPValueApprox(10, 30)).toBeLessThan(1e-6);
  });
});

describe("tCritical95", () => {
  it("returns table value at df=5", () => {
    expect(tCritical95(5)).toBeCloseTo(2.571, 3);
  });
  it("falls back to z=1.96 above df=29", () => {
    expect(tCritical95(30)).toBe(1.96);
    expect(tCritical95(1000)).toBe(1.96);
  });
});

describe("stats", () => {
  it("widens CI with more variance", () => {
    const tight = stats([10, 10, 10, 10, 10]);
    const wide = stats([1, 10, 20, 30, 40]);
    expect(wide.ciHi - wide.ciLo).toBeGreaterThan(tight.ciHi - tight.ciLo);
  });
});

describe("composeCells", () => {
  it("groups rows by (prompt, model, condition)", () => {
    const cells = composeCells([
      dispatch(0, "claude-sonnet-4-6", "soft", 0.01),
      dispatch(0, "claude-sonnet-4-6", "soft", 0.02),
      dispatch(0, "claude-opus-4-7", "soft", 0.1),
    ]);
    expect(cells).toHaveLength(2);
    const opus = cells.find((c) => c.model === "claude-opus-4-7");
    expect(opus?.n).toBe(1);
    expect(opus?.cost.mean).toBeCloseTo(0.1);
  });

  it("computes score stats when scores present", () => {
    const cells = composeCells([
      dispatch(0, "claude", "soft", 0.01, { score: 1.0 }),
      dispatch(0, "claude", "soft", 0.02, { score: 0.5 }),
      dispatch(0, "claude", "soft", 0.03, { score: 0.0 }),
    ]);
    expect(cells[0].score).not.toBeNull();
    expect(cells[0].score?.mean).toBeCloseTo(0.5, 3);
    expect(cells[0].passedAt05).toBe(2); // 1.0 and 0.5 pass; 0.0 fails
  });

  it("returns null score stats when no rubric ran yet", () => {
    const cells = composeCells([
      dispatch(0, "claude", "off", 0.01),
      dispatch(0, "claude", "off", 0.02),
    ]);
    expect(cells[0].score).toBeNull();
    expect(cells[0].passedAt05).toBeNull();
  });

  it("counts grounding verdicts per cell", () => {
    const cells = composeCells([
      dispatch(0, "claude", "strict", 0.01, { verdict: "compliant" }),
      dispatch(0, "claude", "strict", 0.02, { verdict: "compliant" }),
      dispatch(0, "claude", "strict", 0.03, { verdict: "violation" }),
    ]);
    expect(cells[0].groundingVerdicts.compliant).toBe(2);
    expect(cells[0].groundingVerdicts.violation).toBe(1);
  });

  it("tolerates malformed variant_cell JSON without throwing", () => {
    const cells = composeCells([
      {
        variant_cell: "not-json",
        score: null,
        cost_usd: 0.01,
        tokens_in: 1,
        tokens_out: 1,
        duration_ms: 1,
        status: "success",
        grounding_verdict: null,
      },
    ]);
    expect(cells).toHaveLength(1);
    expect(cells[0].model).toBe("(unknown)");
  });

  it("sorts cells deterministically", () => {
    const cells = composeCells([
      dispatch(1, "claude", "off", 0.01),
      dispatch(0, "claude", "off", 0.01),
      dispatch(0, "anthropic", "off", 0.01),
    ]);
    expect(cells[0].promptIdx).toBe(0);
    expect(cells[0].model).toBe("anthropic");
    expect(cells[1].model).toBe("claude");
    expect(cells[2].promptIdx).toBe(1);
  });
});
