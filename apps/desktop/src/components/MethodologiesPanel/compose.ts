// v2.10 PR-8 — TypeScript port of apps/cli/src/methodology/compose.rs.
//
// Same Student's t-table, same Welford-style sample SD, same 95% CI
// math the CLI uses for `runs show`. Keeping the math here lets the
// React panel render meaningful per-cell statistics without a round-
// trip to the CLI for every detail view.
//
// Pure functions. No DOM, no Tauri imports — testable from a Vitest
// unit suite if/when we add one for this module.

export interface Stats {
  n: number;
  mean: number;
  sd: number;
  ciLo: number;
  ciHi: number;
}

export interface CellSummary {
  promptIdx: number;
  model: string;
  condition: string;
  n: number;
  successN: number;
  errorN: number;
  cost: Stats;
  tokensOut: Stats;
  durationMs: Stats;
  score: Stats | null;
  passedAt05: number | null;
  groundingVerdicts: Record<string, number>;
}

interface DispatchInput {
  variant_cell: string;
  score: number | null;
  cost_usd: number | null;
  tokens_in: number | null;
  tokens_out: number | null;
  duration_ms: number | null;
  status: string | null;
  grounding_verdict: string | null;
}

interface VariantCellShape {
  prompt_idx?: number;
  model?: string;
  condition?: string;
}

// Student's t critical at the two-sided 95% level, df = 1..29.
// df ≥ 30 falls back to the normal-approximation z = 1.96.
const T_TABLE_95: number[] = [
  0.0, 12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228,
  2.201, 2.179, 2.16, 2.145, 2.131, 2.12, 2.11, 2.101, 2.093, 2.086, 2.08,
  2.074, 2.069, 2.064, 2.06, 2.056, 2.052, 2.048, 2.045,
];

export function tCritical95(df: number): number {
  if (df <= 0) return Number.POSITIVE_INFINITY;
  if (df < T_TABLE_95.length) return T_TABLE_95[df];
  return 1.96;
}

export function mean(xs: number[]): number {
  if (xs.length === 0) return 0;
  return xs.reduce((a, b) => a + b, 0) / xs.length;
}

/** Sample standard deviation (n-1 denominator). Returns 0 when n < 2. */
export function sampleSd(xs: number[]): number {
  if (xs.length < 2) return 0;
  const m = mean(xs);
  const sumSq = xs.reduce((acc, x) => acc + (x - m) ** 2, 0);
  return Math.sqrt(sumSq / (xs.length - 1));
}

// Abramowitz & Stegun 7.1.26 erf approximation (~1.5e-7 accuracy).
// Mirrors apps/cli/src/methodology/compose.rs::erf_approx so UI numbers
// match the CLI verbatim.
export function erfApprox(x: number): number {
  const a1 = 0.254_829_592;
  const a2 = -0.284_496_736;
  const a3 = 1.421_413_741;
  const a4 = -1.453_152_027;
  const a5 = 1.061_405_429;
  const p = 0.327_591_1;
  const sign = x < 0 ? -1 : 1;
  const xa = Math.abs(x);
  const t = 1 / (1 + p * xa);
  const y =
    1 -
    (((((a5 * t + a4) * t + a3) * t + a2) * t + a1) * t) *
      Math.exp(-xa * xa);
  return sign * y;
}

/** Standard normal CDF Φ(z). */
export function normalCdf(z: number): number {
  return 0.5 * (1 + erfApprox(z / Math.SQRT2));
}

/** Two-sided p approximation for Welch t. Returns null when df < 30.
 *  Code-review finding #2 (PR-9): the normal-CDF approximation
 *  under-states t-distribution tail mass below df=30 enough to flip
 *  "borderline significant" calls. Tight cutoff at 30 so callers
 *  fall back to the CI-disjoint heuristic at smaller samples. */
export function welchPValueApprox(t: number, df: number): number | null {
  if (df < 30 || !Number.isFinite(t)) return null;
  const p = 2 * (1 - normalCdf(Math.abs(t)));
  return Math.max(0, Math.min(1, p));
}

export function stats(xs: number[]): Stats {
  const n = xs.length;
  const m = mean(xs);
  const sd = sampleSd(xs);
  if (n < 2) {
    return { n, mean: m, sd, ciLo: m, ciHi: m };
  }
  const t = tCritical95(n - 1);
  const se = sd / Math.sqrt(n);
  return { n, mean: m, sd, ciLo: m - t * se, ciHi: m + t * se };
}

function parseVariantCell(raw: string): VariantCellShape {
  try {
    return JSON.parse(raw) as VariantCellShape;
  } catch {
    return {};
  }
}

export function composeCells(dispatches: DispatchInput[]): CellSummary[] {
  const groups = new Map<string, DispatchInput[]>();
  const keyToCoords = new Map<
    string,
    { promptIdx: number; model: string; condition: string }
  >();
  for (const d of dispatches) {
    const cell = parseVariantCell(d.variant_cell);
    const promptIdx = cell.prompt_idx ?? 0;
    const model = cell.model ?? "(unknown)";
    const condition = cell.condition ?? "default";
    const key = `${promptIdx}::${model}::${condition}`;
    if (!groups.has(key)) {
      groups.set(key, []);
      keyToCoords.set(key, { promptIdx, model, condition });
    }
    groups.get(key)!.push(d);
  }
  const cells: CellSummary[] = [];
  for (const [key, rows] of groups) {
    const coords = keyToCoords.get(key)!;
    const costs = rows
      .map((r) => r.cost_usd)
      .filter((v): v is number => v !== null);
    const tokens = rows
      .map((r) => r.tokens_out)
      .filter((v): v is number => v !== null);
    const durations = rows
      .map((r) => r.duration_ms)
      .filter((v): v is number => v !== null);
    const scores = rows
      .map((r) => r.score)
      .filter((v): v is number => v !== null);
    const verdicts: Record<string, number> = {};
    for (const r of rows) {
      const v = r.grounding_verdict ?? "not_enforced";
      verdicts[v] = (verdicts[v] ?? 0) + 1;
    }
    cells.push({
      promptIdx: coords.promptIdx,
      model: coords.model,
      condition: coords.condition,
      n: rows.length,
      successN: rows.filter((r) => r.status === "success").length,
      errorN: rows.filter((r) => r.status !== null && r.status !== "success")
        .length,
      cost: stats(costs),
      tokensOut: stats(tokens),
      durationMs: stats(durations),
      score: scores.length > 0 ? stats(scores) : null,
      passedAt05:
        scores.length > 0 ? scores.filter((s) => s >= 0.5).length : null,
      groundingVerdicts: verdicts,
    });
  }
  cells.sort((a, b) => {
    if (a.promptIdx !== b.promptIdx) return a.promptIdx - b.promptIdx;
    if (a.condition !== b.condition) return a.condition.localeCompare(b.condition);
    return a.model.localeCompare(b.model);
  });
  return cells;
}
