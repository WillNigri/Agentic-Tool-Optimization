/**
 * Runtime path discovery and caching.
 * Optimizes CLI detection by caching paths and searching in parallel.
 */

import { execFile } from "node:child_process";
import { promisify } from "node:util";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as os from "node:os";

const execFileAsync = promisify(execFile);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type RuntimeName = "claude" | "codex" | "hermes";

interface CachedPath {
  path: string;
  discoveredAt: number;
  source: "common_path" | "which" | "manual";
}

interface PathCache {
  version: number;
  paths: Partial<Record<RuntimeName, CachedPath>>;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const CACHE_VERSION = 1;
const CACHE_TTL_MS = 24 * 60 * 60 * 1000; // 24 hours
const CACHE_FILE = path.join(os.homedir(), ".ato", "runtime-paths.json");

// Common installation paths to check (in order of priority)
const COMMON_PATHS: Record<RuntimeName, string[]> = {
  claude: [
    "/usr/local/bin/claude",
    "/opt/homebrew/bin/claude",
    path.join(os.homedir(), ".npm-global/bin/claude"),
    path.join(os.homedir(), ".local/bin/claude"),
    "/usr/bin/claude",
  ],
  codex: [
    "/usr/local/bin/codex",
    "/opt/homebrew/bin/codex",
    path.join(os.homedir(), ".npm-global/bin/codex"),
    path.join(os.homedir(), ".local/bin/codex"),
    "/usr/bin/codex",
  ],
  hermes: [
    "/usr/local/bin/hermes",
    "/opt/homebrew/bin/hermes",
    path.join(os.homedir(), ".npm-global/bin/hermes"),
    path.join(os.homedir(), ".local/bin/hermes"),
    "/usr/bin/hermes",
  ],
};

// ---------------------------------------------------------------------------
// Path Cache Implementation
// ---------------------------------------------------------------------------

class RuntimePathCache {
  private memoryCache: PathCache = { version: CACHE_VERSION, paths: {} };
  private initialized = false;
  private initializing: Promise<void> | null = null;

  /**
   * Initialize cache from disk.
   */
  async initialize(): Promise<void> {
    if (this.initialized) return;
    if (this.initializing) return this.initializing;

    this.initializing = this._loadFromDisk();
    await this.initializing;
    this.initializing = null;
    this.initialized = true;
  }

  private async _loadFromDisk(): Promise<void> {
    try {
      const content = await fs.readFile(CACHE_FILE, "utf-8");
      const data = JSON.parse(content) as PathCache;

      // Check version compatibility
      if (data.version === CACHE_VERSION) {
        this.memoryCache = data;
      }
    } catch {
      // File doesn't exist or is invalid, start fresh
      this.memoryCache = { version: CACHE_VERSION, paths: {} };
    }
  }

  /**
   * Save cache to disk.
   */
  private async _saveToDisk(): Promise<void> {
    try {
      const dir = path.dirname(CACHE_FILE);
      await fs.mkdir(dir, { recursive: true });
      await fs.writeFile(CACHE_FILE, JSON.stringify(this.memoryCache, null, 2));
    } catch {
      // Ignore write errors
    }
  }

  /**
   * Check if a cached path is still valid.
   */
  private isCacheValid(cached: CachedPath): boolean {
    const age = Date.now() - cached.discoveredAt;
    return age < CACHE_TTL_MS;
  }

  /**
   * Check if a file exists and is executable.
   */
  private async isExecutable(filePath: string): Promise<boolean> {
    try {
      await fs.access(filePath, fs.constants.X_OK);
      return true;
    } catch {
      return false;
    }
  }

  /**
   * Search common paths in parallel.
   */
  private async searchCommonPaths(runtime: RuntimeName): Promise<string | null> {
    const paths = COMMON_PATHS[runtime] || [];

    // Check all paths in parallel
    const results = await Promise.all(
      paths.map(async (p) => {
        const exists = await this.isExecutable(p);
        return exists ? p : null;
      })
    );

    // Return first valid path
    return results.find((p) => p !== null) || null;
  }

  /**
   * Fall back to 'which' command.
   */
  private async whichFallback(runtime: RuntimeName): Promise<string | null> {
    try {
      const { stdout } = await execFileAsync("which", [runtime], {
        timeout: 5000,
      });
      const p = stdout.trim();
      return p || null;
    } catch {
      return null;
    }
  }

  /**
   * Get the path for a runtime, using cache if available.
   */
  async getPath(runtime: RuntimeName): Promise<string | null> {
    await this.initialize();

    const cached = this.memoryCache.paths[runtime];

    // Check if we have a valid cached path
    if (cached && this.isCacheValid(cached)) {
      // Verify the path still exists
      if (await this.isExecutable(cached.path)) {
        return cached.path;
      }
      // Path no longer valid, clear it
      delete this.memoryCache.paths[runtime];
    }

    // Discover path: common paths first (fast), then 'which' (slower)
    let discoveredPath = await this.searchCommonPaths(runtime);
    let source: CachedPath["source"] = "common_path";

    if (!discoveredPath) {
      discoveredPath = await this.whichFallback(runtime);
      source = "which";
    }

    if (discoveredPath) {
      // Cache the discovered path
      this.memoryCache.paths[runtime] = {
        path: discoveredPath,
        discoveredAt: Date.now(),
        source,
      };
      // Save to disk (don't await, fire and forget)
      this._saveToDisk();
    }

    return discoveredPath;
  }

  /**
   * Get all cached paths at once (parallel discovery).
   */
  async getAllPaths(): Promise<Record<RuntimeName, string | null>> {
    const runtimes: RuntimeName[] = ["claude", "codex", "hermes"];

    const results = await Promise.all(
      runtimes.map(async (runtime) => ({
        runtime,
        path: await this.getPath(runtime),
      }))
    );

    return Object.fromEntries(
      results.map((r) => [r.runtime, r.path])
    ) as Record<RuntimeName, string | null>;
  }

  /**
   * Manually set a path for a runtime.
   */
  async setPath(runtime: RuntimeName, cliPath: string): Promise<boolean> {
    await this.initialize();

    if (!(await this.isExecutable(cliPath))) {
      return false;
    }

    this.memoryCache.paths[runtime] = {
      path: cliPath,
      discoveredAt: Date.now(),
      source: "manual",
    };

    await this._saveToDisk();
    return true;
  }

  /**
   * Clear cached path for a runtime (forces re-discovery).
   */
  async clearPath(runtime: RuntimeName): Promise<void> {
    await this.initialize();
    delete this.memoryCache.paths[runtime];
    await this._saveToDisk();
  }

  /**
   * Clear all cached paths.
   */
  async clearAll(): Promise<void> {
    await this.initialize();
    this.memoryCache.paths = {};
    await this._saveToDisk();
  }

  /**
   * Force refresh all paths.
   */
  async refreshAll(): Promise<Record<RuntimeName, string | null>> {
    await this.initialize();
    this.memoryCache.paths = {};
    return this.getAllPaths();
  }

  /**
   * Get cache statistics.
   */
  async stats(): Promise<{
    cached_paths: number;
    paths: Record<string, { path: string; age_ms: number; source: string } | null>;
    cache_file: string;
  }> {
    await this.initialize();

    const now = Date.now();
    const paths: Record<string, { path: string; age_ms: number; source: string } | null> = {};

    for (const runtime of ["claude", "codex", "hermes"] as RuntimeName[]) {
      const cached = this.memoryCache.paths[runtime];
      if (cached) {
        paths[runtime] = {
          path: cached.path,
          age_ms: now - cached.discoveredAt,
          source: cached.source,
        };
      } else {
        paths[runtime] = null;
      }
    }

    return {
      cached_paths: Object.keys(this.memoryCache.paths).length,
      paths,
      cache_file: CACHE_FILE,
    };
  }
}

// Export singleton instance
export const runtimePaths = new RuntimePathCache();
