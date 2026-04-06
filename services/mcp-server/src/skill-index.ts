/**
 * Incremental skill index with file watching.
 * Maintains an in-memory index of skills and updates incrementally on file changes.
 */

import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as os from "node:os";
import * as crypto from "node:crypto";
import { glob } from "glob";
import matter from "gray-matter";
import chokidar, { type FSWatcher } from "chokidar";
import { cache, CACHE_KEYS } from "./cache.js";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface SkillMetadata {
  name: string;
  description: string;
  file_path: string;
  token_count: number;
  enabled: boolean;
  content_hash: string;
  mtime: number;
}

interface SkillFile {
  path: string;
  content: string;
  mtime: number;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function estimateTokens(content: string): number {
  return Math.ceil(content.length / 4);
}

function contentHash(content: string): string {
  return crypto.createHash("md5").update(content).digest("hex").slice(0, 12);
}

async function readSkillFile(filePath: string): Promise<SkillFile | null> {
  try {
    const [content, stat] = await Promise.all([
      fs.readFile(filePath, "utf-8"),
      fs.stat(filePath),
    ]);
    return { path: filePath, content, mtime: stat.mtimeMs };
  } catch {
    return null;
  }
}

function parseSkillMetadata(file: SkillFile): SkillMetadata {
  const parsed = matter(file.content);
  const frontmatter = parsed.data as Record<string, unknown>;

  const baseName = path.basename(file.path);
  const enabled = !baseName.endsWith(".disabled");
  const name =
    (frontmatter.name as string) ||
    baseName.replace(/\.disabled$/, "").replace(/\.md$/, "");
  const description = (frontmatter.description as string) || "";

  return {
    name,
    description,
    file_path: file.path,
    token_count: estimateTokens(file.content),
    enabled,
    content_hash: contentHash(file.content),
    mtime: file.mtime,
  };
}

// ---------------------------------------------------------------------------
// Skill Index Singleton
// ---------------------------------------------------------------------------

class SkillIndex {
  private skills = new Map<string, SkillMetadata>();
  private watcher: FSWatcher | null = null;
  private initialized = false;
  private initializing: Promise<void> | null = null;
  private watchDirs: string[] = [];

  /**
   * Get the skill directories to watch.
   */
  private getSkillDirs(): string[] {
    const homeDir = os.homedir();
    return [
      path.join(homeDir, ".claude", "skills"),
      path.join(process.cwd(), ".claude", "skills"),
    ];
  }

  /**
   * Initialize the index by scanning all skill directories.
   */
  async initialize(): Promise<void> {
    if (this.initialized) return;
    if (this.initializing) return this.initializing;

    this.initializing = this._doInitialize();
    await this.initializing;
    this.initializing = null;
    this.initialized = true;
  }

  private async _doInitialize(): Promise<void> {
    this.watchDirs = this.getSkillDirs();

    // Full scan on initialization
    for (const dir of this.watchDirs) {
      await this.scanDirectory(dir);
    }

    // Start file watcher
    this.startWatcher();
  }

  /**
   * Scan a single directory for skill files.
   */
  private async scanDirectory(dir: string): Promise<void> {
    try {
      const files = await glob("**/*.md{,.disabled}", {
        cwd: dir,
        absolute: true,
      });

      for (const filePath of files) {
        await this.indexFile(filePath);
      }
    } catch {
      // Directory may not exist, skip
    }
  }

  /**
   * Index or update a single file.
   */
  private async indexFile(filePath: string): Promise<boolean> {
    const file = await readSkillFile(filePath);
    if (!file) {
      // File doesn't exist or is unreadable, remove from index
      this.skills.delete(filePath);
      return false;
    }

    const existing = this.skills.get(filePath);

    // Skip if file hasn't changed (same mtime)
    if (existing && existing.mtime === file.mtime) {
      return false;
    }

    // Parse and index the skill
    const metadata = parseSkillMetadata(file);

    // Check if content actually changed (hash comparison)
    if (existing && existing.content_hash === metadata.content_hash) {
      // Only mtime changed, update mtime but don't invalidate cache
      this.skills.set(filePath, { ...existing, mtime: file.mtime });
      return false;
    }

    this.skills.set(filePath, metadata);
    return true;
  }

  /**
   * Start watching skill directories for changes.
   */
  private startWatcher(): void {
    if (this.watcher) return;

    const watchPaths = this.watchDirs.filter((dir) => {
      try {
        // Check if directory exists synchronously for watcher setup
        return true; // chokidar handles non-existent paths gracefully
      } catch {
        return false;
      }
    });

    if (watchPaths.length === 0) return;

    this.watcher = chokidar.watch(watchPaths, {
      ignored: (filePath: string) => {
        // Only watch .md and .md.disabled files
        const ext = path.extname(filePath);
        if (ext === ".md") return false;
        if (filePath.endsWith(".md.disabled")) return false;
        // Ignore directories (let them be traversed)
        return !filePath.includes(".");
      },
      persistent: true,
      ignoreInitial: true, // Don't emit events for existing files
      awaitWriteFinish: {
        stabilityThreshold: 100,
        pollInterval: 50,
      },
    });

    this.watcher
      .on("add", (filePath) => this.onFileChange(filePath, "add"))
      .on("change", (filePath) => this.onFileChange(filePath, "change"))
      .on("unlink", (filePath) => this.onFileChange(filePath, "unlink"));
  }

  /**
   * Handle file change events.
   */
  private async onFileChange(
    filePath: string,
    event: "add" | "change" | "unlink"
  ): Promise<void> {
    let changed = false;

    if (event === "unlink") {
      changed = this.skills.delete(filePath);
    } else {
      changed = await this.indexFile(filePath);
    }

    if (changed) {
      // Invalidate related caches
      cache.invalidate(CACHE_KEYS.SKILLS_LIST);
      cache.invalidate(CACHE_KEYS.CONTEXT_USAGE);
    }
  }

  /**
   * Get all indexed skills.
   */
  async getSkills(): Promise<SkillMetadata[]> {
    await this.initialize();
    return Array.from(this.skills.values());
  }

  /**
   * Get a single skill by file path.
   */
  async getSkill(filePath: string): Promise<SkillMetadata | undefined> {
    await this.initialize();
    return this.skills.get(filePath);
  }

  /**
   * Force re-index a specific file (e.g., after toggle).
   */
  async reindexFile(filePath: string): Promise<void> {
    await this.initialize();
    await this.indexFile(filePath);
  }

  /**
   * Handle skill toggle - update index for old and new paths.
   */
  async handleToggle(oldPath: string, newPath: string): Promise<void> {
    await this.initialize();

    // Remove old entry
    this.skills.delete(oldPath);

    // Index new path
    await this.indexFile(newPath);

    // Invalidate caches
    cache.invalidate(CACHE_KEYS.SKILLS_LIST);
    cache.invalidate(CACHE_KEYS.CONTEXT_USAGE);
  }

  /**
   * Force a full rescan of all directories.
   */
  async rescan(): Promise<void> {
    this.skills.clear();
    for (const dir of this.watchDirs) {
      await this.scanDirectory(dir);
    }

    // Invalidate caches
    cache.invalidate(CACHE_KEYS.SKILLS_LIST);
    cache.invalidate(CACHE_KEYS.CONTEXT_USAGE);
  }

  /**
   * Stop the file watcher.
   */
  async stop(): Promise<void> {
    if (this.watcher) {
      await this.watcher.close();
      this.watcher = null;
    }
  }

  /**
   * Get index statistics.
   */
  stats(): {
    skill_count: number;
    enabled_count: number;
    disabled_count: number;
    total_tokens: number;
    watch_dirs: string[];
    watcher_active: boolean;
  } {
    const skills = Array.from(this.skills.values());
    return {
      skill_count: skills.length,
      enabled_count: skills.filter((s) => s.enabled).length,
      disabled_count: skills.filter((s) => !s.enabled).length,
      total_tokens: skills.reduce((sum, s) => sum + s.token_count, 0),
      watch_dirs: this.watchDirs,
      watcher_active: this.watcher !== null,
    };
  }
}

// Export singleton instance
export const skillIndex = new SkillIndex();
