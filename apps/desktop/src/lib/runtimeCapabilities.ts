import type { AgentRuntime } from "@/lib/tauri-api";
import { loadRuntimeConfig } from "@/lib/tauri-api";

// v1.3.0 — Runtime capability matrix.
// The honest truth about what ATO can do for each runtime today. Used by
// Run buttons, Quick Test, install UIs, and the Customize overview to:
//   - dispatch correctly per runtime (different shells, mention syntax, etc.)
//   - hide buttons / show clear "not yet" messaging when something isn't wired
//   - surface a parity matrix to the user so they know exactly what works

export type AgentInvocation =
  /** Native @-mention dispatch in interactive CLI (Claude). */
  | { kind: "mention"; shellCommand: string; mentionFormat: (slug: string) => string }
  /** No native dispatch — prefix the user's prompt so the agent is mentioned in
   *  free text. Works as a soft "agent context" hint when the runtime doesn't
   *  support real subagent dispatch. */
  | { kind: "prompt-prefix"; shellCommand: string; promptPrefix: (slug: string) => string }
  /** SSH-streamed shell — we build the `ssh user@host` command from the
   *  runtime's stored config. No automatic follow-up (the user continues by
   *  running `openclaw exec '...'` once they're in). */
  | { kind: "ssh" }
  /** Runtime needs a separate launcher — show the user how to do it. */
  | { kind: "manual"; instructions: string };

export type RuntimeCapability = {
  /** Display label. */
  label: string;
  /** Color dot used everywhere. */
  dotClass: string;
  /** Can ATO write the agent file to disk for this runtime today? */
  canCreateAgent: boolean;
  /** Can ATO write an MCP server entry into this runtime's config today? */
  canInstallMcp: boolean;
  /** How does ATO invoke a created agent for this runtime? */
  invocation: AgentInvocation;
  /** Short note explaining the limit. Empty when full parity. */
  note?: string;
  /** Where the runtime stores its MCP config (used in error messages). */
  mcpConfigPath?: string;
};

// Partial — covers CLI runtimes. API providers fall back at the call site
// (see getRuntimeCapability below which already handles undefined).
const CAPABILITIES: Partial<Record<AgentRuntime, RuntimeCapability>> = {
  claude: {
    label: "Claude Code",
    dotClass: "bg-orange-500",
    canCreateAgent: true,
    canInstallMcp: true,
    mcpConfigPath: "~/.claude/settings.json",
    invocation: {
      kind: "mention",
      shellCommand: "claude",
      mentionFormat: (slug) => `@${slug} `,
    },
  },
  codex: {
    label: "Codex / OpenAI Agents SDK",
    dotClass: "bg-green-500",
    canCreateAgent: true,
    canInstallMcp: true,
    mcpConfigPath: "~/.codex/config.toml",
    // Codex CLI doesn't expose Claude-style @-mention dispatch yet; the AGENTS.md
    // file shapes context but the user invokes by asking. Best we can do today
    // is preface the prompt with the agent name so Codex picks up the context.
    invocation: {
      kind: "prompt-prefix",
      shellCommand: "codex",
      promptPrefix: (slug) => `[acting as the "${slug}" agent] `,
    },
    note: "No native @-mention dispatch — ATO prefixes prompts with the agent name as context.",
  },
  gemini: {
    label: "Gemini CLI / ADK",
    dotClass: "bg-blue-500",
    canCreateAgent: true,
    canInstallMcp: true,
    mcpConfigPath: "~/.gemini/settings.json",
    // ADK uses root_agent.yaml + sub-agents; the CLI launches them as a tree.
    // For now we drop the user into `gemini` and prefix the prompt.
    invocation: {
      kind: "prompt-prefix",
      shellCommand: "gemini",
      promptPrefix: (slug) => `[acting as the "${slug}" agent] `,
    },
    note: "Gemini ADK dispatches agents via root_agent.yaml — ATO drops into the CLI and prefixes prompts.",
  },
  openclaw: {
    label: "OpenClaw",
    dotClass: "bg-cyan-400",
    canCreateAgent: true,
    canInstallMcp: true,
    mcpConfigPath: "~/.openclaw/openclaw.json",
    invocation: { kind: "ssh" },
    note: "Run-in-shell streams an SSH session into the gateway; once you're in, use `openclaw exec '<prompt>'` to dispatch.",
  },
  hermes: {
    label: "Hermes",
    dotClass: "bg-purple-500",
    canCreateAgent: true,
    canInstallMcp: true,
    mcpConfigPath: "~/.hermes/config.yaml",
    // Hermes has --execute (non-interactive) which we already use. Most builds
    // also expose a `hermes` interactive REPL — try that and let the user
    // restart if their build doesn't have it.
    invocation: {
      kind: "prompt-prefix",
      shellCommand: "hermes",
      promptPrefix: (slug) => `[acting as the "${slug}" agent] `,
    },
    note: "MCP entries are written to config.yaml. Run-in-shell uses Hermes's interactive REPL.",
  },
};

export function getRuntimeCapability(runtime: AgentRuntime): RuntimeCapability {
  // CAPABILITIES.claude is always present (asserted by the literal above);
  // the `!` keeps the return type non-nullable for callers.
  return CAPABILITIES[runtime] ?? CAPABILITIES.claude!;
}

export function listRuntimeCapabilities(): { runtime: AgentRuntime; cap: RuntimeCapability }[] {
  return (Object.keys(CAPABILITIES) as AgentRuntime[])
    .map((rt) => {
      const cap = CAPABILITIES[rt];
      return cap ? { runtime: rt, cap } : null;
    })
    .filter((x): x is { runtime: AgentRuntime; cap: RuntimeCapability } => x !== null);
}

/** Build a free-text prompt to send via promptAgent() (single-shot mode).
 *  For mention-capable runtimes we use the @<slug> form; for others we
 *  prefix with an agent-context hint. */
export function buildPromptForAgent(
  runtime: AgentRuntime,
  slug: string,
  userText: string
): string {
  const inv = getRuntimeCapability(runtime).invocation;
  if (inv.kind === "mention") return inv.mentionFormat(slug) + userText;
  if (inv.kind === "prompt-prefix") return inv.promptPrefix(slug) + userText;
  return userText;
}

/** Args for requestShell() so the embedded terminal opens the right CLI and
 *  queues the right keystrokes. Async because SSH-mode runtimes (OpenClaw)
 *  need to read their stored config to build the connection command.
 *  Returns null when the runtime can't be driven from a local shell. */
export async function shellRequestForAgent(
  runtime: AgentRuntime,
  slug: string
): Promise<{ initialCommand: string; followUpKeys?: string; followUpDelayMs?: number } | null> {
  const inv = getRuntimeCapability(runtime).invocation;
  if (inv.kind === "manual") return null;
  if (inv.kind === "ssh") {
    return buildSshShellRequest(runtime, slug);
  }
  return {
    initialCommand: inv.shellCommand,
    followUpKeys:
      inv.kind === "mention" ? inv.mentionFormat(slug) : inv.promptPrefix(slug),
    followUpDelayMs: 1500,
  };
}

async function buildSshShellRequest(
  runtime: AgentRuntime,
  slug: string
): Promise<{ initialCommand: string; followUpKeys?: string; followUpDelayMs?: number } | null> {
  try {
    const raw = await loadRuntimeConfig(runtime);
    if (!raw) return null;
    const cfg = JSON.parse(raw) as {
      sshHost?: string;
      sshPort?: number | string;
      sshUser?: string;
      sshKeyPath?: string;
    };
    if (!cfg.sshHost || !cfg.sshUser) return null;
    const parts = ["ssh"];
    if (cfg.sshKeyPath) parts.push("-i", shellQuote(cfg.sshKeyPath));
    if (cfg.sshPort) parts.push("-p", String(cfg.sshPort));
    // -t forces a pseudo-tty so the user gets an interactive remote shell.
    parts.push("-t", `${cfg.sshUser}@${cfg.sshHost}`);
    // Pre-type the openclaw exec command (without enter) so the user only has
    // to add the prompt body.
    return {
      initialCommand: parts.join(" "),
      followUpKeys: `openclaw exec '@${slug} `,
      followUpDelayMs: 2500, // SSH handshake takes longer than a local CLI boot
    };
  } catch {
    return null;
  }
}

function shellQuote(s: string): string {
  // Bare-minimum POSIX quoting — wrap if there's whitespace or a quote.
  if (/^[A-Za-z0-9_\-./~@%:+,]+$/.test(s)) return s;
  return `'${s.replace(/'/g, "'\\''")}'`;
}
