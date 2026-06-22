// v2.1.4 — Cost capture for desktop dispatches.
//
// CLI dispatches (claude --print, codex exec, gemini -p) don't return
// token usage in stdout. We can't get exact numbers without parsing
// runtime-specific JSON output flags (claude --output-format json,
// etc.) — that's a bigger change. For now we estimate tokens from
// character count and surface the cost with an "est." badge so the
// number is honest about its precision.
//
// The pricing table is shared with deployBundleGenerators/shared.ts
// (which uses an inline copy for the IIFE bundle output — its self-
// containment is the whole point of bundles). Drift between the two
// is a real risk; treat this file as the source of truth and update
// shared.ts when prices change.

// Pricing per million tokens (input, output) in USD.
//
// SINGLE SOURCE OF TRUTH: `packages/ato-pricing/src/lib.rs::MODEL_PRICES`.
// The table below is GENERATED from it into `pricing-table.generated.ts` and
// re-exported here, so the JS frontend can never drift from the Rust crate
// again (the old hand-kept mirror went stale — missing gemini-3.x, opus-4-8,
// and entire providers — silently under-reporting cost). To change a rate,
// edit the Rust table and regenerate:
//   cargo test -p ato-pricing write_generated_pricing_ts -- --ignored
// (a drift-guard test fails CI if the committed file is out of date.)
//
// Original bug (2026-05-15): Settings → Dispatch Cost showed $0.00 for google
// dispatches AI Studio billed for ~R$8.57 — the model wasn't in the table, so
// cost stored NULL and COALESCE'd to $0. The single source prevents recurrence.
export { PRICING_PER_M_TOKENS } from "./pricing-table.generated";
import { PRICING_PER_M_TOKENS } from "./pricing-table.generated";

/** Default model per runtime — used when the dispatch didn't specify a
 *  model override. Aligns with what the runtime CLIs default to. */
export const DEFAULT_MODEL_PER_RUNTIME: Record<string, string> = {
  claude: "claude-sonnet-4-6",
  codex: "gpt-4.1",
  gemini: "gemini-2.5-flash",
};

/** Returns true when the model has a row in PRICING_PER_M_TOKENS. The UI
 *  uses this to distinguish "$0.00 because the model is free/unpriced
 *  by us" from "$0.00 because we don't know what it costs." The pricing
 *  comment at the top of this file calls this distinction out explicitly. */
export function isModelPriced(model: string | null | undefined): boolean {
  if (!model) return false;
  return PRICING_PER_M_TOKENS[model] !== undefined;
}

/** Estimate token count from text. The 4-chars-per-token rule is the
 *  standard rough heuristic — within ~15% of real tokenizer counts for
 *  English prose, more off for code (which is denser). Acceptable for
 *  cost-comparison purposes; not for billing. */
export function estimateTokens(text: string | null | undefined): number {
  if (!text) return 0;
  return Math.ceil(text.length / 4);
}

/** Compute estimated USD cost for a (model, prompt, response) tuple.
 *  Returns 0 when the model isn't in our pricing table — caller should
 *  treat 0 as "unknown" rather than "free." */
export function estimateCostUsd(
  model: string | null | undefined,
  prompt: string | null | undefined,
  response: string | null | undefined,
): number {
  const m = model && PRICING_PER_M_TOKENS[model] ? model : null;
  if (!m) return 0;
  const prices = PRICING_PER_M_TOKENS[m];
  const promptTokens = estimateTokens(prompt);
  const responseTokens = estimateTokens(response);
  const cost =
    (promptTokens / 1_000_000) * prices.in +
    (responseTokens / 1_000_000) * prices.out;
  // Round to 6 decimals — fractional cents matter at scale; truncating
  // earlier loses precision for cheap models.
  return Math.round(cost * 1_000_000) / 1_000_000;
}

/** v2.1.9 — Defensive number coercion for cloud values that come back
 *  as strings. PostgreSQL `DECIMAL(N,M)` columns serialize as strings
 *  through node-postgres ("0.014200" not 0.0142), but our TS types
 *  say `number`. Calling `.toFixed()` on the string crashes the panel.
 *  This helper makes the boundary safe — pass anything that should be
 *  a number, get a number back. */
export function asNumber(v: unknown, fallback = 0): number {
  if (typeof v === "number" && Number.isFinite(v)) return v;
  if (typeof v === "string") {
    const n = Number(v);
    if (Number.isFinite(n)) return n;
  }
  return fallback;
}

/** Convenience: estimate token + cost in one call. */
export function estimateUsage(
  runtime: string,
  modelOverride: string | null | undefined,
  prompt: string | null | undefined,
  response: string | null | undefined,
): { promptTokens: number; responseTokens: number; costUsd: number; model: string | null } {
  const model = modelOverride || DEFAULT_MODEL_PER_RUNTIME[runtime] || null;
  return {
    promptTokens: estimateTokens(prompt),
    responseTokens: estimateTokens(response),
    costUsd: estimateCostUsd(model, prompt, response),
    model,
  };
}
