/**
 * ATO wrapper for the OpenAI SDK
 *
 * Usage:
 *   import OpenAI from 'openai';
 *   import { wrapOpenAI } from '@ato/sdk/openai';
 *
 *   const client = wrapOpenAI(new OpenAI());
 *   // All calls are now auto-traced
 *   const res = await client.chat.completions.create({ model: 'gpt-4o', ... });
 */

import { getClient, generateTraceId } from './client.js';
import { calculateCost } from './pricing.js';
import type { AtoTrace } from './types.js';

type OpenAIClient = {
  chat: {
    completions: {
      create: (...args: any[]) => Promise<any>;
    };
  };
  responses?: {
    create: (...args: any[]) => Promise<any>;
  };
  [key: string]: any;
};

/**
 * Wrap an OpenAI client to auto-capture traces
 */
export function wrapOpenAI<T extends OpenAIClient>(client: T): T {
  const originalCreate = client.chat.completions.create.bind(client.chat.completions);

  // Wrap chat.completions.create
  client.chat.completions.create = async function (...args: any[]) {
    const params = args[0] || {};
    const start = Date.now();

    // If streaming, handle differently
    if (params.stream) {
      return handleStream(originalCreate, params, args, start);
    }

    try {
      const result = await originalCreate(...args);
      const duration = Date.now() - start;

      const inputTokens = result.usage?.prompt_tokens || 0;
      const outputTokens = result.usage?.completion_tokens || 0;
      const cachedTokens = result.usage?.prompt_tokens_details?.cached_tokens || 0;

      const trace: AtoTrace = {
        id: generateTraceId(),
        provider: 'openai',
        model: result.model || params.model || 'unknown',
        inputTokens,
        outputTokens,
        cachedTokens,
        totalTokens: inputTokens + outputTokens,
        costUsd: calculateCost(result.model || params.model, inputTokens, outputTokens, cachedTokens),
        durationMs: duration,
        status: 'success',
        metadata: {
          finishReason: result.choices?.[0]?.finish_reason,
          maxTokens: params.max_tokens || params.max_completion_tokens,
          temperature: params.temperature,
          toolCalls: result.choices?.[0]?.message?.tool_calls?.length,
          systemFingerprint: result.system_fingerprint,
        },
        timestamp: new Date().toISOString(),
      };

      getClient().capture(trace);
      return result;
    } catch (err: any) {
      const duration = Date.now() - start;

      const trace: AtoTrace = {
        id: generateTraceId(),
        provider: 'openai',
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
          errorCode: err.code,
        },
        timestamp: new Date().toISOString(),
      };

      getClient().capture(trace);
      throw err;
    }
  } as any;

  // Wrap responses.create if it exists (newer OpenAI API)
  if (client.responses?.create) {
    const originalResponses = client.responses.create.bind(client.responses);

    client.responses.create = async function (...args: any[]) {
      const params = args[0] || {};
      const start = Date.now();

      try {
        const result = await originalResponses(...args);
        const duration = Date.now() - start;

        const inputTokens = result.usage?.input_tokens || 0;
        const outputTokens = result.usage?.output_tokens || 0;

        const trace: AtoTrace = {
          id: generateTraceId(),
          provider: 'openai',
          model: result.model || params.model || 'unknown',
          inputTokens,
          outputTokens,
          cachedTokens: 0,
          totalTokens: inputTokens + outputTokens,
          costUsd: calculateCost(result.model || params.model, inputTokens, outputTokens),
          durationMs: duration,
          status: 'success',
          metadata: { api: 'responses' },
          timestamp: new Date().toISOString(),
        };

        getClient().capture(trace);
        return result;
      } catch (err: any) {
        const duration = Date.now() - start;

        getClient().capture({
          id: generateTraceId(),
          provider: 'openai',
          model: params.model || 'unknown',
          inputTokens: 0,
          outputTokens: 0,
          cachedTokens: 0,
          totalTokens: 0,
          costUsd: 0,
          durationMs: duration,
          status: 'error',
          error: err.message || String(err),
          metadata: { api: 'responses', errorType: err.constructor?.name },
          timestamp: new Date().toISOString(),
        });

        throw err;
      }
    } as any;
  }

  return client;
}

/**
 * Handle streaming responses — collect chunks and capture trace at end
 */
async function handleStream(
  originalCreate: (...args: any[]) => Promise<any>,
  params: any,
  args: any[],
  start: number,
): Promise<any> {
  const result = await originalCreate(...args);

  // For async iterables (streaming), wrap to capture usage at end
  if (result && Symbol.asyncIterator in result) {
    const originalIterator = result[Symbol.asyncIterator].bind(result);
    let totalInputTokens = 0;
    let totalOutputTokens = 0;
    let model = params.model || 'unknown';
    let finishReason = '';

    result[Symbol.asyncIterator] = async function* () {
      for await (const chunk of originalIterator()) {
        // Accumulate usage from chunks (OpenAI sends usage in last chunk with stream_options)
        if (chunk.usage) {
          totalInputTokens = chunk.usage.prompt_tokens || 0;
          totalOutputTokens = chunk.usage.completion_tokens || 0;
        }
        if (chunk.model) model = chunk.model;
        if (chunk.choices?.[0]?.finish_reason) {
          finishReason = chunk.choices[0].finish_reason;
        }
        yield chunk;
      }

      // Capture trace after stream completes
      const duration = Date.now() - start;
      getClient().capture({
        id: generateTraceId(),
        provider: 'openai',
        model,
        inputTokens: totalInputTokens,
        outputTokens: totalOutputTokens,
        cachedTokens: 0,
        totalTokens: totalInputTokens + totalOutputTokens,
        costUsd: calculateCost(model, totalInputTokens, totalOutputTokens),
        durationMs: duration,
        status: 'success',
        metadata: { streaming: true, finishReason },
        timestamp: new Date().toISOString(),
      });
    };
  }

  return result;
}

export { init, getClient } from './client.js';
export { calculateCost, MODEL_PRICING } from './pricing.js';
export type { AtoTrace, AtoConfig } from './types.js';
