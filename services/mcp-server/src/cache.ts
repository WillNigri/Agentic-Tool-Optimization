/**
 * TTL-based in-memory cache for MCP tools.
 * Reduces latency by avoiding repeated file I/O and glob scans.
 */

interface CacheEntry<T> {
  value: T;
  expiresAt: number;
}

class TTLCache {
  private cache = new Map<string, CacheEntry<unknown>>();

  /**
   * Get a cached value, or execute the factory function and cache the result.
   * @param key - Cache key
   * @param ttlMs - Time-to-live in milliseconds
   * @param factory - Async function to produce the value if not cached
   */
  async getOrSet<T>(
    key: string,
    ttlMs: number,
    factory: () => Promise<T>
  ): Promise<T> {
    const now = Date.now();
    const entry = this.cache.get(key) as CacheEntry<T> | undefined;

    if (entry && entry.expiresAt > now) {
      return entry.value;
    }

    const value = await factory();
    this.cache.set(key, { value, expiresAt: now + ttlMs });
    return value;
  }

  /**
   * Get a cached value if it exists and hasn't expired.
   */
  get<T>(key: string): T | undefined {
    const entry = this.cache.get(key) as CacheEntry<T> | undefined;
    if (!entry) return undefined;
    if (entry.expiresAt <= Date.now()) {
      this.cache.delete(key);
      return undefined;
    }
    return entry.value;
  }

  /**
   * Set a cached value with TTL.
   */
  set<T>(key: string, value: T, ttlMs: number): void {
    this.cache.set(key, { value, expiresAt: Date.now() + ttlMs });
  }

  /**
   * Invalidate specific cache keys by prefix.
   */
  invalidateByPrefix(prefix: string): number {
    let count = 0;
    for (const key of this.cache.keys()) {
      if (key.startsWith(prefix)) {
        this.cache.delete(key);
        count++;
      }
    }
    return count;
  }

  /**
   * Invalidate specific cache key.
   */
  invalidate(key: string): boolean {
    return this.cache.delete(key);
  }

  /**
   * Clear all cached entries.
   */
  clear(): void {
    this.cache.clear();
  }

  /**
   * Get cache statistics.
   */
  stats(): { size: number; keys: string[] } {
    return {
      size: this.cache.size,
      keys: Array.from(this.cache.keys()),
    };
  }
}

// Singleton instance for the MCP server
export const cache = new TTLCache();

// Cache key prefixes for different tools
export const CACHE_KEYS = {
  CONTEXT_USAGE: "context_usage",
  SKILLS_LIST: "skills_list",
  USAGE_STATS: "usage_stats",
  RUNTIME_STATUS: "runtime_status",
  ALL_RUNTIME_STATUSES: "all_runtime_statuses",
  AGENT_LOGS: "agent_logs",
} as const;

// Default TTLs in milliseconds
export const CACHE_TTL = {
  CONTEXT_USAGE: 30_000, // 30 seconds - context rarely changes mid-session
  SKILLS_LIST: 60_000, // 60 seconds - skills don't change often
  USAGE_STATS: 300_000, // 5 minutes - logs don't need real-time
  RUNTIME_STATUS: 30_000, // 30 seconds - status checks are expensive
  AGENT_LOGS: 10_000, // 10 seconds - logs may update frequently
} as const;
