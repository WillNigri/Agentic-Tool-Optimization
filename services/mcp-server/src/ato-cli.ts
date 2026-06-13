// v2.3.4 Phase 3 — Shared helper for shelling out to the `ato` CLI.
//
// Phase 3's MCP tool expansion calls `ato <subcommand>` rather than
// re-implementing SQLite queries in TypeScript. The CLI is the canonical
// implementation; the MCP server is a thin protocol adapter. Benefits:
//   - One algorithm, two surfaces (CLI + MCP)
//   - Schema migrations only have to be tested in one place
//   - Tools stay <30 lines each
//
// Discovery order for the binary:
//   1. ATO_CLI_PATH env var (explicit override; useful for tests / CI)
//   2. `ato` on PATH (user ran `ato setup-path` after install — most cases)
//   3. /Applications/ATO.app/Contents/Resources/binaries/ato-* (macOS Tauri
//      sidecar path, picked up when the user hasn't run setup-path yet)
//
// Errors are surfaced as MCP tool errors with the CLI's stderr inline,
// so the agent calling the tool can read what went wrong.

import { execFile } from "node:child_process";
import { promisify } from "node:util";
import * as fs from "node:fs/promises";
import * as path from "node:path";

const execFileAsync = promisify(execFile);
// Spread caller env LAST so an explicitly-set ATO_CLIENT_SURFACE /
// ATO_INITIATOR_KIND (test harness, parent process override) wins
// over the MCP defaults — preserves PR-B's env-first contract.
const atoCliEnv = {
  ATO_CLIENT_SURFACE: "mcp_stdio",
  ATO_INITIATOR_KIND: "mcp",
  ...process.env,
};

let resolvedCliPath: string | null = null;

async function resolveCliPath(): Promise<string> {
  if (resolvedCliPath) return resolvedCliPath;

  const envOverride = process.env.ATO_CLI_PATH;
  if (envOverride) {
    try {
      await fs.access(envOverride);
      resolvedCliPath = envOverride;
      return envOverride;
    } catch {
      // fall through to other candidates
    }
  }

  // Try `ato` on PATH first — the most common shape after the user has
  // run `ato setup-path` or installed via Homebrew with the post-install
  // symlink. execFile with a bare name will use PATH automatically; we
  // probe by trying `ato --version`.
  try {
    await execFileAsync("ato", ["--version"], { env: atoCliEnv });
    resolvedCliPath = "ato";
    return "ato";
  } catch {
    // Continue to Tauri sidecar fallback
  }

  // macOS Tauri sidecar fallback. The .app bundle includes the binary
  // under Resources/binaries/ with a host-triple suffix. If we're on
  // macOS and the bundle path exists, use the right binary.
  if (process.platform === "darwin") {
    const candidates = [
      "/Applications/ATO.app/Contents/Resources/binaries/ato-aarch64-apple-darwin",
      "/Applications/ATO.app/Contents/Resources/binaries/ato-x86_64-apple-darwin",
    ];
    for (const p of candidates) {
      try {
        await fs.access(p);
        resolvedCliPath = p;
        return p;
      } catch {
        continue;
      }
    }
  }

  throw new Error(
    "Could not find the `ato` CLI. Install ATO and run `ato setup-path`, or set the ATO_CLI_PATH environment variable to the binary's full path.",
  );
}

/**
 * Run an ato CLI subcommand and parse its JSON output. Throws on
 * non-zero exit with the stderr inlined so MCP callers see what
 * went wrong instead of a generic "command failed".
 */
export async function runAtoCli<T = unknown>(args: string[]): Promise<T> {
  const bin = await resolveCliPath();
  try {
    const { stdout } = await execFileAsync(bin, args, {
      env: atoCliEnv,
      // 10MB is plenty; trace dumps cap at 64KB per row already.
      maxBuffer: 10 * 1024 * 1024,
      timeout: 60_000,
    });
    const trimmed = stdout.trim();
    if (!trimmed) {
      // Some subcommands legitimately emit nothing (setup-path --quiet,
      // a successful no-op update). Return null so callers can decide.
      return null as unknown as T;
    }
    return JSON.parse(trimmed) as T;
  } catch (err) {
    // execFile rejects with a result that has .stderr / .code on it.
    // Surface those so the MCP tool error is actionable.
    const e = err as { stderr?: string; message?: string; code?: number };
    const detail = e.stderr?.trim() || e.message || "Unknown error";
    throw new Error(`ato ${args.join(" ")} failed: ${detail}`);
  }
}

/** Same as runAtoCli but returns the raw stdout (no JSON.parse).
 *  Used by tools whose output is plain text (rare). */
export async function runAtoCliRaw(args: string[]): Promise<string> {
  const bin = await resolveCliPath();
  const { stdout } = await execFileAsync(bin, args, {
    env: atoCliEnv,
    maxBuffer: 10 * 1024 * 1024,
    timeout: 60_000,
  });
  return stdout.trim();
}

/** Reset cached path. Mostly for tests. */
export function resetCliPathCache() {
  resolvedCliPath = null;
}
