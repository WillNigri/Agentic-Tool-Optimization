/**
 * LLM model pricing (per 1M tokens)
 * Updated: April 2026
 */

export interface ModelPricing {
  input: number;   // $ per 1M input tokens
  output: number;  // $ per 1M output tokens
  cached?: number; // $ per 1M cached input tokens
}

// Prices in USD per 1M tokens
export const MODEL_PRICING: Record<string, ModelPricing> = {
  // Anthropic
  'claude-opus-4-6':         { input: 15.00, output: 75.00, cached: 1.50 },
  'claude-sonnet-4-6':       { input: 3.00,  output: 15.00, cached: 0.30 },
  'claude-haiku-4-5':        { input: 0.80,  output: 4.00,  cached: 0.08 },
  'claude-sonnet-4-5':       { input: 3.00,  output: 15.00, cached: 0.30 },
  'claude-3-5-sonnet':       { input: 3.00,  output: 15.00, cached: 0.30 },
  'claude-3-5-haiku':        { input: 0.80,  output: 4.00,  cached: 0.08 },
  'claude-3-opus':           { input: 15.00, output: 75.00, cached: 1.50 },
  'claude-3-sonnet':         { input: 3.00,  output: 15.00 },
  'claude-3-haiku':          { input: 0.25,  output: 1.25 },

  // OpenAI
  'gpt-4o':                  { input: 2.50,  output: 10.00, cached: 1.25 },
  'gpt-4o-mini':             { input: 0.15,  output: 0.60,  cached: 0.075 },
  'gpt-4-turbo':             { input: 10.00, output: 30.00 },
  'gpt-4':                   { input: 30.00, output: 60.00 },
  'gpt-3.5-turbo':           { input: 0.50,  output: 1.50 },
  'o1':                      { input: 15.00, output: 60.00, cached: 7.50 },
  'o1-mini':                 { input: 3.00,  output: 12.00, cached: 1.50 },
  'o1-pro':                  { input: 150.00, output: 600.00 },
  'o3':                      { input: 10.00, output: 40.00, cached: 2.50 },
  'o3-mini':                 { input: 1.10,  output: 4.40,  cached: 0.55 },
  'o4-mini':                 { input: 1.10,  output: 4.40,  cached: 0.275 },
  'gpt-4.1':                 { input: 2.00,  output: 8.00,  cached: 0.50 },
  'gpt-4.1-mini':            { input: 0.40,  output: 1.60,  cached: 0.10 },
  'gpt-4.1-nano':            { input: 0.10,  output: 0.40,  cached: 0.025 },

  // Google
  'gemini-2.5-pro':          { input: 1.25,  output: 10.00 },
  'gemini-2.5-flash':        { input: 0.15,  output: 0.60 },
  'gemini-2.0-flash':        { input: 0.10,  output: 0.40 },
  'gemini-1.5-pro':          { input: 1.25,  output: 5.00 },
  'gemini-1.5-flash':        { input: 0.075, output: 0.30 },

  // Mistral
  'mistral-large':           { input: 2.00,  output: 6.00 },
  'mistral-small':           { input: 0.20,  output: 0.60 },
  'codestral':               { input: 0.30,  output: 0.90 },

  // Groq (inference pricing)
  'llama-3.3-70b':           { input: 0.59,  output: 0.79 },
  'llama-3.1-8b':            { input: 0.05,  output: 0.08 },
  'mixtral-8x7b':            { input: 0.24,  output: 0.24 },

  // Cohere
  'command-r-plus':          { input: 2.50,  output: 10.00 },
  'command-r':               { input: 0.15,  output: 0.60 },
};

/**
 * Calculate cost for a given model and token usage
 */
export function calculateCost(
  model: string,
  inputTokens: number,
  outputTokens: number,
  cachedTokens = 0,
): number {
  // Try exact match first, then prefix match
  const pricing = MODEL_PRICING[model] || Object.entries(MODEL_PRICING).find(
    ([key]) => model.startsWith(key)
  )?.[1];

  if (!pricing) return 0;

  const inputCost = ((inputTokens - cachedTokens) / 1_000_000) * pricing.input;
  const outputCost = (outputTokens / 1_000_000) * pricing.output;
  const cachedCost = pricing.cached ? (cachedTokens / 1_000_000) * pricing.cached : 0;

  return inputCost + outputCost + cachedCost;
}
