// ============================================================
// Claude Code Config File Path Constants & Parsing
// ============================================================

import { z } from 'zod';

/**
 * Claude Code config path templates using {home} placeholder.
 */
export const CLAUDE_PATHS = {
  globalConfig: '{home}/.claude.json',
  globalSettings: '{home}/.claude/settings.json',
  personalSkills: '{home}/.claude/skills/',
  logs: '{home}/.claude/logs/',
  projectDir: '.claude/',
  projectSettings: '.claude/settings.json',
  projectSkills: '.claude/skills/',
  claudeMd: 'CLAUDE.md',
} as const;

/**
 * Resolve config paths by replacing {home} with the actual home directory.
 */
export function resolveConfigPaths(homeDir: string): Record<string, string> {
  const resolved: Record<string, string> = {};
  for (const [key, template] of Object.entries(CLAUDE_PATHS)) {
    resolved[key] = template.replace('{home}', homeDir);
  }
  return resolved;
}

/**
 * Safely parse a JSON string. Returns the fallback value on failure.
 */
export function parseJsonSafe<T>(json: string, fallback: T): T {
  try {
    return JSON.parse(json) as T;
  } catch {
    return fallback;
  }
}

// ============================================================
// Zod Validation Schemas
// ============================================================

export const claudeConfigSchema = z.record(z.unknown());

export const claudeSettingsSchema = z.object({
  permissions: z.record(z.unknown()).optional(),
  env: z.record(z.string()).optional(),
}).passthrough();

export const mcpConfigSchema = z.object({
  mcpServers: z.record(
    z.object({
      command: z.string().optional(),
      args: z.array(z.string()).optional(),
      env: z.record(z.string()).optional(),
      url: z.string().optional(),
      transport: z.enum(['stdio', 'http', 'streamable-http']).optional(),
    }),
  ).optional(),
}).passthrough();
