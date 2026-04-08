/**
 * ATO Client — manages trace collection and cloud sync
 */

import type { AtoConfig, AtoTrace, BatchPayload } from './types.js';

const SDK_VERSION = '0.1.0';
const DEFAULT_ENDPOINT = 'https://api.agentictool.ai';
const DEFAULT_FLUSH_INTERVAL = 5000;
const DEFAULT_MAX_BATCH_SIZE = 50;

let globalClient: AtoClient | null = null;

export class AtoClient {
  private config: Required<
    Pick<AtoConfig, 'batching' | 'flushInterval' | 'maxBatchSize' | 'debug' | 'localOnly'>
  > & AtoConfig;
  private queue: AtoTrace[] = [];
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(config: AtoConfig = {}) {
    this.config = {
      endpoint: DEFAULT_ENDPOINT,
      batching: true,
      flushInterval: DEFAULT_FLUSH_INTERVAL,
      maxBatchSize: DEFAULT_MAX_BATCH_SIZE,
      debug: false,
      localOnly: false,
      ...config,
    };

    if (this.config.batching && !this.config.localOnly) {
      this.timer = setInterval(() => this.flush(), this.config.flushInterval);
      // Don't keep the process alive just for flushing
      if (this.timer && typeof this.timer === 'object' && 'unref' in this.timer) {
        this.timer.unref();
      }
    }
  }

  /**
   * Record a trace
   */
  capture(trace: AtoTrace): void {
    // Apply defaults
    if (this.config.defaultTags) {
      trace.tags = [...(trace.tags || []), ...this.config.defaultTags];
    }
    if (this.config.defaultMetadata) {
      trace.metadata = { ...this.config.defaultMetadata, ...trace.metadata };
    }
    if (this.config.sessionId && !trace.sessionId) {
      trace.sessionId = this.config.sessionId;
    }
    if (this.config.userId && !trace.userId) {
      trace.userId = this.config.userId;
    }

    if (this.config.debug) {
      console.log('[ato]', JSON.stringify(trace, null, 2));
    }

    if (this.config.localOnly) return;

    this.queue.push(trace);

    if (!this.config.batching || this.queue.length >= this.config.maxBatchSize) {
      this.flush();
    }
  }

  /**
   * Flush pending traces to ATO Cloud
   */
  async flush(): Promise<void> {
    if (this.queue.length === 0) return;

    const traces = this.queue.splice(0);
    const payload: BatchPayload = {
      traces,
      sdk: '@ato/sdk',
      sdkVersion: SDK_VERSION,
      sentAt: new Date().toISOString(),
    };

    try {
      const headers: Record<string, string> = {
        'Content-Type': 'application/json',
      };
      if (this.config.apiKey) {
        headers['Authorization'] = `Bearer ${this.config.apiKey}`;
      }

      const res = await fetch(`${this.config.endpoint}/api/analytics/ingest`, {
        method: 'POST',
        headers,
        body: JSON.stringify(payload),
      });

      if (!res.ok && this.config.debug) {
        console.error('[ato] Failed to send traces:', res.status, await res.text());
      }
    } catch (err) {
      if (this.config.debug) {
        console.error('[ato] Failed to send traces:', err);
      }
      // Put traces back in queue for retry
      this.queue.unshift(...traces);
      // Cap queue to prevent memory leak
      if (this.queue.length > 1000) {
        this.queue.splice(0, this.queue.length - 500);
      }
    }
  }

  /**
   * Shutdown — flush remaining traces
   */
  async shutdown(): Promise<void> {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
    await this.flush();
  }

  getConfig(): AtoConfig {
    return { ...this.config };
  }
}

/**
 * Initialize the global ATO client
 */
export function init(config: AtoConfig = {}): AtoClient {
  globalClient = new AtoClient(config);
  return globalClient;
}

/**
 * Get or create the global client
 */
export function getClient(): AtoClient {
  if (!globalClient) {
    globalClient = new AtoClient();
  }
  return globalClient;
}

/**
 * Generate a unique trace ID
 */
export function generateTraceId(): string {
  return `ato_${Date.now()}_${Math.random().toString(36).slice(2, 10)}`;
}
