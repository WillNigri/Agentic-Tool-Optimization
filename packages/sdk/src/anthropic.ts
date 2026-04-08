/**
 * ATO wrapper for the Anthropic SDK
 *
 * Usage:
 *   import Anthropic from '@anthropic-ai/sdk';
 *   import { wrapAnthropic } from '@ato/sdk/anthropic';
 *
 *   const client = wrapAnthropic(new Anthropic());
 *   // All calls are now auto-traced
 *   const msg = await client.messages.create({ model: 'claude-sonnet-4-6', ... });
 */

import { getClient, generateTraceId } from './client.js';
import { calculateCost } from './pricing.js';
import type { AtoTrace } from './types.js';

type AnthropicClient = {
  messages: {
    create: (...args: any[]) => Promise<any>;
    stream: (...args: any[]) => any;
  };
  [key: string]: any;
};

/**
 * Wrap an Anthropic client to auto-capture traces
 */
export function wrapAnthropic<T extends AnthropicClient>(client: T): T {
  const originalCreate = client.messages.create.bind(client.messages);
  const originalStream = client.messages.stream?.bind(client.messages);

  // Wrap messages.create
  client.messages.create = async function (...args: any[]) {
    const params = args[0] || {};
    const start = Date.now();
    let trace: AtoTrace;

    try {
      const result = await originalCreate(...args);
      const duration = Date.now() - start;

      const inputTokens = result.usage?.input_tokens || 0;
      const outputTokens = result.usage?.output_tokens || 0;
      const cachedTokens = result.usage?.cache_read_input_tokens || 0;

      trace = {
        id: generateTraceId(),
        provider: 'anthropic',
        model: result.model || params.model || 'unknown',
        inputTokens,
        outputTokens,
        cachedTokens,
        totalTokens: inputTokens + outputTokens,
        costUsd: calculateCost(result.model || params.model, inputTokens, outputTokens, cachedTokens),
        durationMs: duration,
        status: 'success',
        metadata: {
          stopReason: result.stop_reason,
          maxTokens: params.max_tokens,
          temperature: params.temperature,
          system: params.system ? '[present]' : undefined,
          toolUse: params.tools?.length ? params.tools.length : undefined,
        },
        timestamp: new Date().toISOString(),
      };

      getClient().capture(trace);
      return result;
    } catch (err: any) {
      const duration = Date.now() - start;

      trace = {
        id: generateTraceId(),
        provider: 'anthropic',
        model: params.model || 'unknown',
        inputTokens: 0,
        outputTokens: 0,
        cachedTokens: 0,
        totalTokens: 0,
        costUsd: 0,
        durationMs: duration,
        status: 'error',
        error: err.message || String(err),
        metadata: {
          errorType: err.constructor?.name,
          statusCode: err.status,
        },
        timestamp: new Date().toISOString(),
      };

      getClient().capture(trace);
      throw err;
    }
  } as any;

  // Wrap messages.stream if available
  if (originalStream) {
    client.messages.stream = function (...args: any[]) {
      const params = args[0] || {};
      const start = Date.now();
      const stream = originalStream(...args);

      // Hook into the finalMessage event
      const originalOn = stream.on?.bind(stream);
      if (originalOn) {
        stream.on = function (event: string, handler: any) {
          if (event === 'finalMessage' || event === 'message') {
            const wrappedHandler = (message: any) => {
              const duration = Date.now() - start;
              const inputTokens = message.usage?.input_tokens || 0;
              const outputTokens = message.usage?.output_tokens || 0;
              const cachedTokens = message.usage?.cache_read_input_tokens || 0;

              getClient().capture({
                id: generateTraceId(),
                provider: 'anthropic',
                model: message.model || params.model || 'unknown',
                inputTokens,
                outputTokens,
                cachedTokens,
                totalTokens: inputTokens + outputTokens,
                costUsd: calculateCost(message.model || params.model, inputTokens, outputTokens, cachedTokens),
                durationMs: duration,
                status: 'success',
                metadata: { streaming: true, stopReason: message.stop_reason },
                timestamp: new Date().toISOString(),
              });

              handler(message);
            };
            return originalOn(event, wrappedHandler);
          }
          return originalOn(event, handler);
        } as any;
      }

      return stream;
    } as any;
  }

  return client;
}

export { init, getClient } from './client.js';
export { calculateCost, MODEL_PRICING } from './pricing.js';
export type { AtoTrace, AtoConfig } from './types.js';
