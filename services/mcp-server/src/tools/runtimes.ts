import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import * as os from "node:os";
import * as path from "node:path";
import * as fs from "node:fs/promises";
import { cache, CACHE_KEYS, CACHE_TTL } from "../cache.js";
import { runtimePaths } from "../runtime-paths.js";

const execFileAsync = promisify(execFile);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface RuntimeStatus {
  runtime: string;
  available: boolean;
  version: string | null;
  path: string | null;
  healthy: boolean;
  details: Record<string, unknown>;
}

interface RuntimeLog {
  timestamp: string;
  level: string;
  message: string;
  runtime: string;
  jobId?: string;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function runCommand(
  cmd: string,
  args: string[],
  timeoutMs = 10000
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  try {
    const { stdout, stderr } = await execFileAsync(cmd, args, {
      timeout: timeoutMs,
    });
    return { stdout: stdout.trim(), stderr: stderr.trim(), exitCode: 0 };
  } catch (err: unknown) {
    const e = err as { stdout?: string; stderr?: string; code?: number };
    return {
      stdout: (e.stdout || "").trim(),
      stderr: (e.stderr || "").trim(),
      exitCode: e.code || 1,
    };
  }
}

async function sshCommand(
  host: string,
  port: number,
  user: string,
  keyPath: string | undefined,
  remoteCmd: string,
  timeoutMs = 15000
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const args: string[] = [];
  if (keyPath) args.push("-i", keyPath);
  args.push(
    "-p", String(port),
    "-o", "ConnectTimeout=5",
    "-o", "StrictHostKeyChecking=no",
    "-o", "BatchMode=yes",
    `${user}@${host}`,
    remoteCmd
  );
  return runCommand("ssh", args, timeoutMs);
}

// ---------------------------------------------------------------------------
// Runtime-specific status checks (using cached paths)
// ---------------------------------------------------------------------------

async function getClaudeStatus(): Promise<RuntimeStatus> {
  // Use cached path instead of calling 'which' every time
  const cliPath = await runtimePaths.getPath("claude");
  if (!cliPath) {
    return { runtime: "claude", available: false, version: null, path: null, healthy: false, details: {} };
  }

  // Get version
  const versionResult = await runCommand(cliPath, ["--version"]);
  const version = versionResult.exitCode === 0 ? versionResult.stdout.split("\n")[0] : null;

  // Check if authenticated by running a minimal command
  const authResult = await runCommand(cliPath, ["--print", "respond with OK"], 15000);
  const healthy = authResult.exitCode === 0 && !authResult.stderr.includes("not logged in");

  return {
    runtime: "claude",
    available: true,
    version,
    path: cliPath,
    healthy,
    details: {
      authenticated: healthy,
      stderr: authResult.stderr || undefined,
    },
  };
}

async function getCodexStatus(): Promise<RuntimeStatus> {
  // Use cached path instead of calling 'which' every time
  const cliPath = await runtimePaths.getPath("codex");
  if (!cliPath) {
    return { runtime: "codex", available: false, version: null, path: null, healthy: false, details: {} };
  }

  const versionResult = await runCommand(cliPath, ["--version"]);
  const version = versionResult.exitCode === 0 ? versionResult.stdout.split("\n")[0] : null;

  // Check Codex health — try --help or a simple version check
  const healthResult = await runCommand(cliPath, ["--help"]);
  const healthy = healthResult.exitCode === 0;

  // Check for API key
  const apiKeyEnv = process.env.OPENAI_API_KEY ? "set" : "not set";
  const configDir = path.join(os.homedir(), ".config", "codex");
  let hasConfigFile = false;
  try {
    await fs.access(configDir);
    hasConfigFile = true;
  } catch { /* no config dir */ }

  return {
    runtime: "codex",
    available: true,
    version,
    path: cliPath,
    healthy,
    details: {
      apiKeyEnv,
      hasConfigDir: hasConfigFile,
    },
  };
}

async function getOpenClawStatus(config?: Record<string, unknown>): Promise<RuntimeStatus> {
  const host = (config?.sshHost as string) || "";
  const port = (config?.sshPort as number) || 22;
  const user = (config?.sshUser as string) || "root";
  const keyPath = config?.sshKeyPath as string | undefined;

  if (!host) {
    return {
      runtime: "openclaw",
      available: false,
      version: null,
      path: null,
      healthy: false,
      details: { error: "No SSH host configured" },
    };
  }

  // Test SSH connectivity + check OpenClaw version
  const versionResult = await sshCommand(host, port, user, keyPath, "openclaw --version 2>/dev/null || echo NOT_FOUND");
  const available = versionResult.exitCode === 0 && !versionResult.stdout.includes("NOT_FOUND");
  const version = available ? versionResult.stdout.split("\n")[0] : null;

  // Health check — can we reach the host?
  const healthy = versionResult.exitCode === 0 && !versionResult.stderr.includes("Connection refused");

  // Get running jobs if healthy
  let runningJobs = 0;
  if (healthy) {
    const jobsResult = await sshCommand(host, port, user, keyPath, "openclaw status --json 2>/dev/null || echo '{}'");
    try {
      const status = JSON.parse(jobsResult.stdout);
      runningJobs = status.running_jobs || 0;
    } catch { /* parse failure */ }
  }

  return {
    runtime: "openclaw",
    available,
    version,
    path: `${user}@${host}:${port}`,
    healthy,
    details: {
      sshHost: host,
      sshPort: port,
      sshUser: user,
      sshReachable: versionResult.exitCode === 0,
      runningJobs,
    },
  };
}

async function getHermesStatus(config?: Record<string, unknown>): Promise<RuntimeStatus> {
  const endpoint = (config?.endpoint as string) || "";
  // Use cached path instead of calling 'which' every time
  const cliPath = await runtimePaths.getPath("hermes");

  // Check CLI availability
  if (!cliPath && !endpoint) {
    return { runtime: "hermes", available: false, version: null, path: null, healthy: false, details: {} };
  }

  let version: string | null = null;
  let healthy = false;

  if (cliPath) {
    const versionResult = await runCommand(cliPath, ["--version"]);
    version = versionResult.exitCode === 0 ? versionResult.stdout.split("\n")[0] : null;

    const healthResult = await runCommand(cliPath, ["--help"]);
    healthy = healthResult.exitCode === 0;
  }

  // If endpoint configured, check HTTP health
  let endpointHealthy = false;
  if (endpoint) {
    try {
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), 5000);
      const res = await fetch(`${endpoint}/health`, { signal: controller.signal }).catch(() => null);
      clearTimeout(timeoutId);
      endpointHealthy = res !== null && res.ok;
    } catch { /* endpoint unreachable */ }
  }

  return {
    runtime: "hermes",
    available: !!cliPath || endpointHealthy,
    version,
    path: cliPath || endpoint || null,
    healthy: healthy || endpointHealthy,
    details: {
      cliAvailable: !!cliPath,
      endpoint: endpoint || undefined,
      endpointHealthy: endpoint ? endpointHealthy : undefined,
    },
  };
}

// ---------------------------------------------------------------------------
// Log reading
// ---------------------------------------------------------------------------

async function getAtoLogs(runtime?: string, limit = 50): Promise<RuntimeLog[]> {
  const logsPath = path.join(os.homedir(), ".ato", "agent-logs.jsonl");
  const logs: RuntimeLog[] = [];

  try {
    const content = await fs.readFile(logsPath, "utf-8");
    const lines = content.split("\n").filter((l) => l.trim());

    for (const line of lines) {
      try {
        const entry = JSON.parse(line) as RuntimeLog;
        if (!runtime || entry.runtime === runtime) {
          logs.push(entry);
        }
      } catch { /* skip */ }
    }
  } catch {
    // No log file yet
  }

  return logs.slice(-limit);
}

// ---------------------------------------------------------------------------
// MCP Tool Registration
// ---------------------------------------------------------------------------

export function registerRuntimeTools(server: McpServer): void {
  // Query status of any runtime
  server.tool(
    "get_runtime_status",
    "Check the health and availability of an AI coding agent runtime (claude, codex, openclaw, hermes). Uses cached CLI paths for faster detection.",
    {
      runtime: z.enum(["claude", "codex", "openclaw", "hermes"]).describe("Which runtime to check"),
      config: z.string().optional().describe("JSON config for runtime (e.g. SSH config for openclaw)"),
    },
    async ({ runtime, config }) => {
      try {
        const parsedConfig = config ? JSON.parse(config) : undefined;

        let status: RuntimeStatus;
        switch (runtime) {
          case "claude":
            status = await getClaudeStatus();
            break;
          case "codex":
            status = await getCodexStatus();
            break;
          case "openclaw":
            status = await getOpenClawStatus(parsedConfig);
            break;
          case "hermes":
            status = await getHermesStatus(parsedConfig);
            break;
        }

        return {
          content: [{ type: "text", text: JSON.stringify(status, null, 2) }],
        };
      } catch (error) {
        return {
          content: [{ type: "text", text: JSON.stringify({ error: String(error) }) }],
          isError: true,
        };
      }
    }
  );

  // Query all runtimes at once
  server.tool(
    "get_all_runtime_statuses",
    "Check health and availability of all configured AI coding agent runtimes at once. Uses cached CLI paths and parallel execution for faster results. Results are cached for 30 seconds.",
    {},
    async () => {
      try {
        const statuses = await cache.getOrSet(
          CACHE_KEYS.ALL_RUNTIME_STATUSES,
          CACHE_TTL.RUNTIME_STATUS,
          async () => {
            const [claude, codex, openclaw, hermes] = await Promise.all([
              getClaudeStatus(),
              getCodexStatus(),
              getOpenClawStatus(),
              getHermesStatus(),
            ]);
            return { claude, codex, openclaw, hermes };
          }
        );

        return {
          content: [{
            type: "text",
            text: JSON.stringify(statuses, null, 2),
          }],
        };
      } catch (error) {
        return {
          content: [{ type: "text", text: JSON.stringify({ error: String(error) }) }],
          isError: true,
        };
      }
    }
  );

  // Read agent execution logs
  server.tool(
    "get_agent_logs",
    "Read execution logs for agent runtimes from ~/.ato/agent-logs.jsonl. Optionally filter by runtime. Results are cached for 10 seconds.",
    {
      runtime: z.enum(["claude", "codex", "openclaw", "hermes"]).optional().describe("Filter by runtime"),
      limit: z.number().optional().describe("Max entries to return (default 50)"),
    },
    async ({ runtime, limit }) => {
      try {
        const effectiveLimit = limit || 50;
        const cacheKey = `${CACHE_KEYS.AGENT_LOGS}:${runtime || "all"}:${effectiveLimit}`;

        const logs = await cache.getOrSet(
          cacheKey,
          CACHE_TTL.AGENT_LOGS,
          () => getAtoLogs(runtime, effectiveLimit)
        );

        return {
          content: [{ type: "text", text: JSON.stringify(logs, null, 2) }],
        };
      } catch (error) {
        return {
          content: [{ type: "text", text: JSON.stringify({ error: String(error) }) }],
          isError: true,
        };
      }
    }
  );

  // Runtime path cache management tools
  server.tool(
    "get_runtime_path_cache",
    "Returns statistics about the runtime path cache, showing cached CLI paths and their ages.",
    {},
    async () => {
      try {
        const stats = await runtimePaths.stats();
        return {
          content: [{ type: "text", text: JSON.stringify(stats, null, 2) }],
        };
      } catch (error) {
        return {
          content: [{ type: "text", text: JSON.stringify({ error: String(error) }) }],
          isError: true,
        };
      }
    }
  );

  server.tool(
    "refresh_runtime_paths",
    "Forces re-discovery of all runtime CLI paths. Use this if you've installed or moved a CLI tool.",
    {},
    async () => {
      try {
        const paths = await runtimePaths.refreshAll();
        // Also invalidate runtime status cache
        cache.invalidate(CACHE_KEYS.ALL_RUNTIME_STATUSES);
        return {
          content: [{
            type: "text",
            text: JSON.stringify({
              success: true,
              message: "Runtime paths refreshed",
              paths,
            }, null, 2),
          }],
        };
      } catch (error) {
        return {
          content: [{ type: "text", text: JSON.stringify({ error: String(error) }) }],
          isError: true,
        };
      }
    }
  );

  server.tool(
    "set_runtime_path",
    "Manually set the path for a runtime CLI. Useful when the CLI is installed in a non-standard location.",
    {
      runtime: z.enum(["claude", "codex", "hermes"]).describe("Which runtime to set the path for"),
      path: z.string().describe("Absolute path to the CLI executable"),
    },
    async ({ runtime, path: cliPath }) => {
      try {
        const success = await runtimePaths.setPath(runtime, cliPath);
        if (success) {
          // Invalidate runtime status cache
          cache.invalidate(CACHE_KEYS.ALL_RUNTIME_STATUSES);
          return {
            content: [{
              type: "text",
              text: JSON.stringify({
                success: true,
                message: `Path for ${runtime} set to ${cliPath}`,
              }),
            }],
          };
        } else {
          return {
            content: [{
              type: "text",
              text: JSON.stringify({
                success: false,
                error: `Path ${cliPath} does not exist or is not executable`,
              }),
            }],
            isError: true,
          };
        }
      } catch (error) {
        return {
          content: [{ type: "text", text: JSON.stringify({ error: String(error) }) }],
          isError: true,
        };
      }
    }
  );
}
