// ============================================================
// Token Estimation Utilities
// ============================================================

/**
 * Rough token estimate based on character count.
 * Approximation: ~4 characters per token for English text.
 */
export function estimateTokens(text: string): number {
  return Math.ceil(text.length / 4);
}

/**
 * Pricing table for Claude models.
 * Costs are in USD per million tokens.
 */
export const MODEL_PRICING: Record<
  string,
  { inputPerMillion: number; outputPerMillion: number }
> = {
  'claude-sonnet': { inputPerMillion: 3.0, outputPerMillion: 15.0 },
  'claude-opus': { inputPerMillion: 15.0, outputPerMillion: 75.0 },
  'claude-haiku': { inputPerMillion: 0.8, outputPerMillion: 4.0 },
};

/**
 * Calculate the cost of a request given model name and token counts.
 * Returns cost in cents. Falls back to claude-sonnet pricing if model is unknown.
 */
export function calculateCost(
  model: string,
  inputTokens: number,
  outputTokens: number,
): number {
  const pricing = MODEL_PRICING[model] ?? MODEL_PRICING['claude-sonnet']!;
  const dollars =
    (inputTokens * pricing.inputPerMillion) / 1_000_000 +
    (outputTokens * pricing.outputPerMillion) / 1_000_000;
  return dollars * 100;
}

/**
 * Format a token count for display: "1.2K", "45.3K", "1.2M", etc.
 */
export function formatTokenCount(count: number): string {
  if (count < 1_000) {
    return String(count);
  }
  if (count < 1_000_000) {
    const k = count / 1_000;
    return `${k % 1 === 0 ? k.toFixed(0) : k.toFixed(1)}K`;
  }
  const m = count / 1_000_000;
  return `${m % 1 === 0 ? m.toFixed(0) : m.toFixed(1)}M`;
}

/**
 * Format a cost in cents for display: "$0.45", "$12.30", etc.
 */
export function formatCost(cents: number): string {
  const dollars = cents / 100;
  return `$${dollars.toFixed(2)}`;
}
