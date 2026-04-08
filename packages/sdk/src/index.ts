/**
 * @ato/sdk — Auto-capture LLM traces for ATO
 *
 * Quick start:
 *   import { init } from '@ato/sdk';
 *   init({ apiKey: 'your-ato-api-key' });
 *
 * Then wrap your LLM client:
 *   import { wrapAnthropic } from '@ato/sdk/anthropic';
 *   import { wrapOpenAI } from '@ato/sdk/openai';
 *
 * Or capture traces manually:
 *   import { capture } from '@ato/sdk';
 *   capture({ provider: 'custom', model: 'my-model', ... });
 */

export { init, getClient, AtoClient, generateTraceId } from './client.js';
export { calculateCost, MODEL_PRICING } from './pricing.js';
export type { ModelPricing } from './pricing.js';
export type { AtoTrace, AtoConfig, BatchPayload } from './types.js';

import { getClient } from './client.js';
import type { AtoTrace } from './types.js';

/**
 * Capture a trace manually (for custom providers)
 */
export function capture(trace: AtoTrace): void {
  getClient().capture(trace);
}

/**
 * Flush all pending traces
 */
export function flush(): Promise<void> {
  return getClient().flush();
}

/**
 * Shutdown the SDK (flush + cleanup)
 */
export function shutdown(): Promise<void> {
  return getClient().shutdown();
}
