import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as os from "node:os";
import matter from "gray-matter";
import { runtimePaths } from "../runtime-paths.js";

// v1.3.0+ — Agents-as-MCP (T7 follow-up).
//
// Exposes ATO-managed agents as MCP tools so any runtime that speaks MCP can
// invoke any agent we've created — regardless of which runtime owns it.
// Cross-runtime dispatch via MCP, no per-CLI wrappers, no per-runtime caveats.
//
// Tools:
//   list_agents()                       → enumerate all created agents
//   run_agent({slug, prompt})           → dispatch to the agent's native runtime
//
// Discovery: agent files live on disk under known per-runtime paths. We scan
// those paths instead of reading ATO's SQLite — keeps the MCP server
// dependency-free and works as soon as the user has created any agent file.

const execFileAsync = promisify(execFile);

type Runtime = "claude" | "codex" | "gemini" | "openclaw" | "hermes";

interface DiscoveredAgent {
  slug: string;
  runtime: Runtime;
  filePath: string;
  description?: string;
  model?: string;
  /** Set when this entry is actually a multi-agent group (F4). */
  isGroup?: boolean;
}

interface GroupFile {
  slug: string;
  displayName: string;
  description?: string;
  runtime: Runtime;
  routerConfig?: {
    rules?: Array<{
      if?: { keyword?: string[]; regex?: string };
      then?: string;
    }>;
    llmFallback?: { enabled?: boolean; model?: string };
  };
  members?: Array<{ agent: string; role: "router" | "child"; position: number }>;
}

const HOME = os.homedir();

// Per-runtime agent file location patterns. Mirrors apps/desktop/src-tauri/src/
// commands.rs `agent_file_path()`.
const AGENT_DIRS: Record<Runtime, { dir: string; pattern: "flat-md" | "dir-with-file" | "flat-yaml"; fileName?: string }> = {
  claude:   { dir: path.join(HOME, ".claude/agents"),   pattern: "flat-md" },
  codex:    { dir: path.join(HOME, ".codex/agents"),    pattern: "dir-with-file", fileName: "AGENTS.md" },
  gemini:   { dir: path.join(HOME, ".gemini/agents"),   pattern: "flat-yaml" },
  openclaw: { dir: path.join(HOME, ".openclaw/agents"), pattern: "dir-with-file", fileName: "SOUL.md" },
  hermes:   { dir: path.join(HOME, ".hermes/agents"),   pattern: "dir-with-file", fileName: "AGENT.md" },
};

async function discoverAgentsFor(runtime: Runtime): Promise<DiscoveredAgent[]> {
  const cfg = AGENT_DIRS[runtime];
  let entries: string[] = [];
  try {
    entries = await fs.readdir(cfg.dir);
  } catch {
    return [];
  }

  const out: DiscoveredAgent[] = [];
  for (const entry of entries) {
    const fullPath = path.join(cfg.dir, entry);
    let stat;
    try {
      stat = await fs.stat(fullPath);
    } catch {
      continue;
    }

    if (cfg.pattern === "flat-md" && stat.isFile() && entry.endsWith(".md")) {
      const slug = entry.replace(/\.md$/, "");
      const meta = await readAgentMetaSafe(fullPath);
      out.push({ slug, runtime, filePath: fullPath, ...meta });
    } else if (cfg.pattern === "flat-yaml" && stat.isFile() && entry.endsWith(".yaml")) {
      const slug = entry.replace(/\.yaml$/, "");
      out.push({ slug, runtime, filePath: fullPath });
    } else if (cfg.pattern === "dir-with-file" && stat.isDirectory() && cfg.fileName) {
      const inner = path.join(fullPath, cfg.fileName);
      try {
        await fs.access(inner);
      } catch {
        continue;
      }
      const meta = await readAgentMetaSafe(inner);
      out.push({ slug: entry, runtime, filePath: inner, ...meta });
    }
  }
  return out;
}

async function readAgentMetaSafe(file: string): Promise<{ description?: string; model?: string }> {
  try {
    const content = await fs.readFile(file, "utf8");
    const parsed = matter(content);
    const fm = parsed.data as { description?: string; model?: string };
    return {
      description: typeof fm.description === "string" ? fm.description : undefined,
      model: typeof fm.model === "string" ? fm.model : undefined,
    };
  } catch {
    return {};
  }
}

async function discoverAllAgents(): Promise<DiscoveredAgent[]> {
  const runtimes: Runtime[] = ["claude", "codex", "gemini", "openclaw", "hermes"];
  const lists = await Promise.all(runtimes.map(discoverAgentsFor));
  const agents = lists.flat();
  const groups = await discoverGroups();
  return [...agents, ...groups];
}

/** v1.4.0 F4 — discover multi-agent groups in `~/.ato/groups/`. */
async function discoverGroups(): Promise<DiscoveredAgent[]> {
  const dir = path.join(HOME, ".ato", "groups");
  let entries: string[] = [];
  try {
    entries = await fs.readdir(dir);
  } catch {
    return [];
  }
  const out: DiscoveredAgent[] = [];
  for (const slug of entries) {
    const file = path.join(dir, slug, "group.json");
    try {
      const raw = await fs.readFile(file, "utf8");
      const parsed = JSON.parse(raw) as GroupFile;
      out.push({
        slug: parsed.slug ?? slug,
        runtime: parsed.runtime,
        filePath: file,
        description: parsed.description,
        isGroup: true,
      });
    } catch {
      // skip malformed groups
    }
  }
  return out;
}

async function loadGroupFile(slug: string): Promise<GroupFile | null> {
  const file = path.join(HOME, ".ato", "groups", slug, "group.json");
  try {
    const raw = await fs.readFile(file, "utf8");
    return JSON.parse(raw) as GroupFile;
  } catch {
    return null;
  }
}

async function findAgent(slug: string): Promise<DiscoveredAgent | null> {
  const all = await discoverAllAgents();
  // Exact slug match wins; ties go to runtime priority order.
  return all.find((a) => a.slug === slug) ?? null;
}

/** Decide which child a prompt routes to. Mirrors Rust `route_prompt_to_child`
 *  logic so cross-runtime invocations behave identically. */
async function routePromptToChild(
  group: GroupFile,
  prompt: string
): Promise<{ childSlug: string; reason: string }> {
  const children = (group.members ?? []).filter((m) => m.role === "child");
  if (children.length === 0) {
    throw new Error(`Group "${group.slug}" has no children to route to`);
  }
  const lower = prompt.toLowerCase();
  for (const rule of group.routerConfig?.rules ?? []) {
    const thenSlug = rule.then ?? "";
    if (rule.if?.keyword) {
      for (const kw of rule.if.keyword) {
        if (kw && lower.includes(kw.toLowerCase())) {
          if (children.some((c) => c.agent === thenSlug)) {
            return { childSlug: thenSlug, reason: `rule: keyword '${kw}' matched` };
          }
        }
      }
    }
    if (rule.if?.regex) {
      // Literal substring match for now (matches Rust behavior in Wave 3.1).
      if (rule.if.regex && prompt.includes(rule.if.regex)) {
        if (children.some((c) => c.agent === thenSlug)) {
          return {
            childSlug: thenSlug,
            reason: `rule: pattern '${rule.if.regex}' matched (literal)`,
          };
        }
      }
    }
  }

  // LLM fallback
  if (group.routerConfig?.llmFallback?.enabled) {
    const descriptions = children.map((c) => `- ${c.agent}`).join("\n");
    const classifierPrompt = `You are a router. Pick the single agent slug that should handle the user's message.\nAvailable agents:\n${descriptions}\nUser message: ${prompt}\nReply with ONLY the slug — nothing else.`;
    try {
      const reply = await dispatchByRuntime(group.runtime, classifierPrompt);
      const pick = reply.trim().split("\n")[0]?.trim() ?? "";
      const matched = children.find((c) => c.agent === pick);
      if (matched) {
        return { childSlug: matched.agent, reason: "llm-fallback" };
      }
    } catch (err) {
      console.error("router LLM fallback failed:", err);
    }
  }

  // Default
  return { childSlug: children[0].agent, reason: "default: first child" };
}

async function dispatchByRuntime(runtime: Runtime, prompt: string): Promise<string> {
  switch (runtime) {
    case "claude":
      return claudeDispatch("__router__", prompt);
    case "codex":
      return codexDispatch("__router__", prompt);
    case "gemini":
      return geminiDispatch("__router__", prompt);
    case "openclaw":
      return openclawDispatch("__router__", prompt);
    case "hermes":
      return hermesDispatch("__router__", prompt);
  }
}

// ---------------------------------------------------------------------------
// Per-runtime dispatch — mirrors apps/desktop/src-tauri/src/commands.rs
// `prompt_agent`. We can't call Tauri from here, so this re-implements the
// invocation patterns for each runtime in Node.
// ---------------------------------------------------------------------------

async function dispatch(agent: DiscoveredAgent, userPrompt: string): Promise<string> {
  const startedAt = Date.now();
  let response = "";
  let errorMessage: string | undefined;
  let routedTo: string | undefined;

  try {
    // v1.4.0 F4: groups route through their router to a child agent.
    if (agent.isGroup) {
      const group = await loadGroupFile(agent.slug);
      if (!group) throw new Error(`Group ${agent.slug} could not be loaded`);
      const { childSlug, reason } = await routePromptToChild(group, userPrompt);
      routedTo = childSlug;
      // Resolve the child to a real DiscoveredAgent and dispatch through the
      // standard per-runtime path. Recurses for nested groups.
      const child = await findAgent(childSlug);
      if (!child) {
        throw new Error(`Routed to "${childSlug}" but that agent isn't on disk`);
      }
      response = await dispatch(child, userPrompt);
      // Annotate the routing decision in the response so users can see what
      // happened. (Pure-noise observability; doesn't change the model output.)
      void reason; // logged via the trace below
      return response;
    }

    switch (agent.runtime) {
      case "claude":
        response = await claudeDispatch(agent.slug, userPrompt);
        break;
      case "codex":
        response = await codexDispatch(agent.slug, userPrompt);
        break;
      case "gemini":
        response = await geminiDispatch(agent.slug, userPrompt);
        break;
      case "openclaw":
        response = await openclawDispatch(agent.slug, userPrompt);
        break;
      case "hermes":
        response = await hermesDispatch(agent.slug, userPrompt);
        break;
    }
    return response;
  } catch (err) {
    errorMessage = err instanceof Error ? err.message : String(err);
    throw err;
  } finally {
    // Cross-runtime observability: append to ~/.ato/agent-logs.jsonl so every
    // dispatch — whether it came from a desktop Run button, an MCP-routed
    // call from another runtime, or a cron job — shows up in one timeline.
    void appendAgentLog({
      ts: new Date(startedAt).toISOString(),
      durationMs: Date.now() - startedAt,
      runtime: agent.runtime,
      slug: agent.slug,
      filePath: agent.filePath,
      promptPreview: userPrompt.slice(0, 200),
      responsePreview: response.slice(0, 200),
      ok: !errorMessage,
      error: errorMessage,
      source: agent.isGroup ? "mcp:run_agent:group" : "mcp:run_agent",
      routedTo,
    });
  }
}

interface AgentLogLine {
  ts: string;
  durationMs: number;
  runtime: Runtime;
  slug: string;
  filePath: string;
  promptPreview: string;
  responsePreview: string;
  ok: boolean;
  error?: string;
  source: string;
  /** v1.4.0 F4 — set when this dispatch was a group routed through its router. */
  routedTo?: string;
}

async function appendAgentLog(entry: AgentLogLine): Promise<void> {
  try {
    const dir = path.join(HOME, ".ato");
    await fs.mkdir(dir, { recursive: true });
    const line = JSON.stringify(entry) + "\n";
    await fs.appendFile(path.join(dir, "agent-logs.jsonl"), line, "utf8");
  } catch {
    // Logging failures must never break the agent call.
  }
}

async function claudeDispatch(slug: string, prompt: string): Promise<string> {
  const cliPath = await runtimePaths.getPath("claude");
  if (!cliPath) throw new Error("Claude Code CLI not found. Install it first.");
  // Native @-mention dispatch.
  const fullPrompt = `@${slug} ${prompt}`;
  const { stdout } = await execFileAsync(cliPath, ["--print", fullPrompt], {
    timeout: 5 * 60 * 1000, // 5 min hard cap
    maxBuffer: 10 * 1024 * 1024,
  });
  return stdout;
}

async function codexDispatch(slug: string, prompt: string): Promise<string> {
  const cliPath = await runtimePaths.getPath("codex");
  if (!cliPath) throw new Error("Codex CLI not found. Install it first.");
  const fullPrompt = `[acting as the "${slug}" agent] ${prompt}`;
  const { stdout } = await execFileAsync(cliPath, ["--print", fullPrompt], {
    timeout: 5 * 60 * 1000,
    maxBuffer: 10 * 1024 * 1024,
  });
  return stdout;
}

async function geminiDispatch(slug: string, prompt: string): Promise<string> {
  // Gemini CLI binary isn't tracked by runtimePaths (no enum entry); resolve
  // via shell PATH instead. Same pattern as the desktop's `which_cli` fallback.
  const fullPrompt = `[acting as the "${slug}" agent] ${prompt}`;
  const { stdout } = await execFileAsync("gemini", ["-p", fullPrompt], {
    timeout: 5 * 60 * 1000,
    maxBuffer: 10 * 1024 * 1024,
  });
  return stdout;
}

async function hermesDispatch(slug: string, prompt: string): Promise<string> {
  const cliPath = await runtimePaths.getPath("hermes");
  if (!cliPath) throw new Error("Hermes CLI not found.");
  const fullPrompt = `[acting as the "${slug}" agent] ${prompt}`;
  const { stdout } = await execFileAsync(cliPath, ["--execute", fullPrompt], {
    timeout: 5 * 60 * 1000,
    maxBuffer: 10 * 1024 * 1024,
  });
  return stdout;
}

async function openclawDispatch(slug: string, prompt: string): Promise<string> {
  // OpenClaw is SSH-based. Read the saved gateway config from ~/.ato/.
  let cfg: {
    sshHost?: string;
    sshPort?: number | string;
    sshUser?: string;
    sshKeyPath?: string;
  } = {};
  try {
    const raw = await fs.readFile(path.join(HOME, ".ato", "runtimes", "openclaw.json"), "utf8");
    cfg = JSON.parse(raw);
  } catch {
    throw new Error(
      "OpenClaw gateway config not found. Configure SSH host/user/key in ATO Settings → Runtimes first."
    );
  }
  if (!cfg.sshHost || !cfg.sshUser) {
    throw new Error("OpenClaw config missing sshHost/sshUser.");
  }
  const args: string[] = [];
  if (cfg.sshKeyPath) args.push("-i", cfg.sshKeyPath);
  args.push(
    "-p", String(cfg.sshPort ?? 22),
    "-o", "ConnectTimeout=5",
    "-o", "StrictHostKeyChecking=no",
    "-o", "BatchMode=yes",
    `${cfg.sshUser}@${cfg.sshHost}`,
    `openclaw exec '@${slug} ${prompt.replace(/'/g, "'\\''")}'`
  );
  const { stdout } = await execFileAsync("ssh", args, {
    timeout: 5 * 60 * 1000,
    maxBuffer: 10 * 1024 * 1024,
  });
  return stdout;
}

// ---------------------------------------------------------------------------
// Tool registration
// ---------------------------------------------------------------------------

export function registerAgentTools(server: McpServer) {
  server.tool(
    "list_agents",
    "List all ATO-managed agents available on this machine, across every runtime (Claude, Codex, Gemini, OpenClaw, Hermes). Returns slug, runtime, description, model, and file path for each.",
    {
      runtime: z
        .enum(["claude", "codex", "gemini", "openclaw", "hermes"])
        .optional()
        .describe("Optional: filter to a single runtime."),
    },
    async ({ runtime }) => {
      const all = runtime ? await discoverAgentsFor(runtime) : await discoverAllAgents();
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(
              {
                count: all.length,
                agents: all.map((a) => ({
                  slug: a.slug,
                  runtime: a.runtime,
                  description: a.description ?? null,
                  model: a.model ?? null,
                  filePath: a.filePath,
                })),
              },
              null,
              2
            ),
          },
        ],
      };
    }
  );

  server.tool(
    "run_agent",
    "Dispatch a prompt to one of your ATO-managed agents and return its response. The agent runs on its native runtime (Claude / Codex / Gemini / Hermes / OpenClaw via SSH) — you don't need to know or care which one. Use list_agents first to discover what's available.",
    {
      slug: z.string().describe("The agent's slug (e.g. 'email-monitor-and-responder')."),
      prompt: z.string().describe("What you want the agent to do."),
    },
    async ({ slug, prompt }) => {
      const agent = await findAgent(slug);
      if (!agent) {
        return {
          content: [
            {
              type: "text",
              text: JSON.stringify(
                {
                  error: "AGENT_NOT_FOUND",
                  message: `No agent named "${slug}" found in any runtime's agents directory.`,
                  hint: "Use list_agents to see available agents.",
                },
                null,
                2
              ),
            },
          ],
          isError: true,
        };
      }

      try {
        const response = await dispatch(agent, prompt);
        return {
          content: [
            {
              type: "text",
              text: response.trim() || "(agent returned no output)",
            },
          ],
        };
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        return {
          content: [
            {
              type: "text",
              text: JSON.stringify(
                {
                  error: "DISPATCH_FAILED",
                  agent: { slug: agent.slug, runtime: agent.runtime },
                  message: msg,
                },
                null,
                2
              ),
            },
          ],
          isError: true,
        };
      }
    }
  );
}
