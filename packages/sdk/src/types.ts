/**
 * Core types for ATO SDK traces
 */

export interface AtoTrace {
  id: string;
  provider: string;       // 'anthropic' | 'openai' | 'google' | 'mistral' | etc.
  model: string;
  inputTokens: number;
  outputTokens: number;
  cachedTokens: number;
  totalTokens: number;
  costUsd: number;
  durationMs: number;
  status: 'success' | 'error';
  error?: string;
  metadata: Record<string, unknown>;
  timestamp: string;
  sessionId?: string;
  userId?: string;
  tags?: string[];
}

export interface AtoConfig {
  /** ATO Cloud API key (from app.agentictool.ai/settings) */
  apiKey?: string;

  /** ATO Cloud endpoint (default: https://api.agentictool.ai) */
  endpoint?: string;

  /** Send traces in batches for performance (default: true) */
  batching?: boolean;

  /** Batch flush interval in ms (default: 5000) */
  flushInterval?: number;

  /** Max batch size before auto-flush (default: 50) */
  maxBatchSize?: number;

  /** Log traces to console for debugging (default: false) */
  debug?: boolean;

  /** Default tags applied to all traces */
  defaultTags?: string[];

  /** Default metadata applied to all traces */
  defaultMetadata?: Record<string, unknown>;

  /** Session ID for grouping traces */
  sessionId?: string;

  /** User ID for attribution */
  userId?: string;

  /** Disable sending to cloud (local logging only) */
  localOnly?: boolean;
}

export interface BatchPayload {
  traces: AtoTrace[];
  sdk: string;
  sdkVersion: string;
  sentAt: string;
}
