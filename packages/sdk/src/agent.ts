/**
 * ATO wrapper for the Claude Agent SDK
 *
 * Usage:
 *   import { Agent } from 'claude-agent-sdk';
 *   import { wrapAgent } from '@ato-sdk/js/agent';
 *
 *   const agent = wrapAgent(new Agent({ model: 'claude-sonnet-4-6' }));
 *   // All agent runs are now auto-traced
 *   const result = await agent.run('Fix the failing tests');
 */

import { getClient, generateTraceId } from './client.js';
import { calculateCost } from './pricing.js';
import type { AtoTrace } from './types.js';

type AgentInstance = {
  run: (...args: any[]) => any;
  [key: string]: any;
};

/**
 * Wrap a Claude Agent SDK instance to auto-capture traces
 */
export function wrapAgent<T extends AgentInstance>(agent: T): T {
  const originalRun = agent.run.bind(agent);

  agent.run = async function (...args: any[]) {
    const prompt = typeof args[0] === 'string' ? args[0] : args[0]?.prompt || '';
    const start = Date.now();
    let totalInputTokens = 0;
    let totalOutputTokens = 0;
    let totalCachedTokens = 0;
    let model = (agent as any).model || 'claude-sonnet-4-6';
    let toolCalls = 0;
    let status: 'success' | 'error' = 'success';
    let error: string | undefined;

    try {
      const result = originalRun(...args);

      // Handle async iterable (streaming agent loop)
      if (result && Symbol.asyncIterator in result) {
        const originalIterator = result[Symbol.asyncIterator].bind(result);

        result[Symbol.asyncIterator] = async function* () {
          for await (const message of originalIterator()) {
            // Accumulate usage from messages
            if (message.type === 'usage' || message.usage) {
              const usage = message.usage || message;
              totalInputTokens += usage.input_tokens || 0;
              totalOutputTokens += usage.output_tokens || 0;
              totalCachedTokens += usage.cache_read_input_tokens || 0;
            }

            if (message.type === 'tool_use' || message.type === 'tool_call') {
              toolCalls++;
            }

            if (message.model) {
              model = message.model;
            }

            yield message;
          }

          // Capture trace after agent loop completes
          captureAgentTrace({
            start, model, prompt, totalInputTokens, totalOutputTokens,
            totalCachedTokens, toolCalls, status, error,
          });
        };

        return result;
      }

      // Handle promise (non-streaming)
      const awaited = await result;

      // Extract usage from result
      if (awaited?.usage) {
        totalInputTokens = awaited.usage.input_tokens || 0;
        totalOutputTokens = awaited.usage.output_tokens || 0;
        totalCachedTokens = awaited.usage.cache_read_input_tokens || 0;
      }

      if (awaited?.model) model = awaited.model;
      if (awaited?.tool_calls) toolCalls = awaited.tool_calls;

      captureAgentTrace({
        start, model, prompt, totalInputTokens, totalOutputTokens,
        totalCachedTokens, toolCalls, status, error,
      });

      return awaited;
    } catch (err: any) {
      status = 'error';
      error = err.message || String(err);

      captureAgentTrace({
        start, model, prompt, totalInputTokens, totalOutputTokens,
        totalCachedTokens, toolCalls, status, error,
      });

      throw err;
    }
  } as any;

  return agent;
}

function captureAgentTrace(params: {
  start: number;
  model: string;
  prompt: string;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCachedTokens: number;
  toolCalls: number;
  status: 'success' | 'error';
  error?: string;
}) {
  const duration = Date.now() - params.start;

  const trace: AtoTrace = {
    id: generateTraceId(),
    provider: 'anthropic-agent',
    model: params.model,
    inputTokens: params.totalInputTokens,
    outputTokens: params.totalOutputTokens,
    cachedTokens: params.totalCachedTokens,
    totalTokens: params.totalInputTokens + params.totalOutputTokens,
    costUsd: calculateCost(params.model, params.totalInputTokens, params.totalOutputTokens, params.totalCachedTokens),
    durationMs: duration,
    status: params.status,
    error: params.error,
    metadata: {
      type: 'agent-session',
      prompt: params.prompt.slice(0, 200),
      toolCalls: params.toolCalls,
    },
    timestamp: new Date().toISOString(),
  };

  getClient().capture(trace);
}

export { init, getClient } from './client.js';
export { calculateCost, MODEL_PRICING } from './pricing.js';
export type { AtoTrace, AtoConfig } from './types.js';
