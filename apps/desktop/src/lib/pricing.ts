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

export const PRICING_PER_M_TOKENS: Record<string, { in: number; out: number }> = {
  // Anthropic
  "claude-opus-4-7": { in: 15, out: 75 },
  "claude-opus-4-6": { in: 15, out: 75 },
  "claude-sonnet-4-6": { in: 3, out: 15 },
  "claude-sonnet-4-5": { in: 3, out: 15 },
  "claude-haiku-4-5-20251001": { in: 1, out: 5 },
  // OpenAI
  "gpt-4.1": { in: 2.5, out: 10 },
  "gpt-4o": { in: 2.5, out: 10 },
  "gpt-4o-mini": { in: 0.15, out: 0.6 },
  // Google
  "gemini-2.0-flash": { in: 0.1, out: 0.4 },
  "gemini-1.5-pro": { in: 1.25, out: 5 },
  "gemini-1.5-flash": { in: 0.075, out: 0.3 },
};

/** Default model per runtime — used when the dispatch didn't specify a
 *  model override. Aligns with what the runtime CLIs default to. */
export const DEFAULT_MODEL_PER_RUNTIME: Record<string, string> = {
  claude: "claude-sonnet-4-6",
  codex: "gpt-4.1",
  gemini: "gemini-2.0-flash",
};

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
