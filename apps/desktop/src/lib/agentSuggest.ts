import { promptAgent, queryAllAgentStatuses, type AgentRuntime } from "@/lib/tauri-api";
import { getStoredTokens } from "@/lib/cloud-api";

// v1.3.0 — Agent stack suggester (T3).
// Given a free-text goal, returns a suggested {runtime, model, description, systemPrompt}.
// Resolution order (subscriptions OR keys, both first-class — see plan):
//   1. Active CLI subscription via promptAgent(runtime, ...)
//   2. (T-future) stored API key direct call
//   3. (T-future) local Ollama
//   4. (T-future) Pro-tier hosted /agent-suggest on ato-cloud
//   5. None → throw NO_RUNTIME_AVAILABLE; UI directs user to Settings → Runtimes.

export type StackSuggestion = {
  runtime: AgentRuntime;
  model: string;
  displayName: string;
  description: string;
  systemPrompt: string;
  reasoning: string;
  /**
   * Registry IDs of MCP servers the agent needs to be functional end-to-end.
   * (e.g., ["gmail", "calendar"] for an email assistant). Match the entries in
   * `data/mcp-registry-fallback.json`.
   */
  recommendedMcps: string[];
};

export class NoRuntimeError extends Error {
  constructor() {
    super("NO_RUNTIME_AVAILABLE");
    this.name = "NoRuntimeError";
  }
}

const SUGGEST_PROMPT = (goal: string) => `You are helping a user create an AI agent that can ACTUALLY DO ITS JOB end-to-end.

Their goal: "${goal}"

Critical: an agent without the right MCPs is useless — it can talk about the task but can't perform it. Pick MCPs from this catalog (use these exact IDs):
  - filesystem  — read/write local files
  - github      — GitHub issues, PRs, code search (needs GITHUB_PERSONAL_ACCESS_TOKEN)
  - postgres    — read-only Postgres queries (needs DATABASE_URL)
  - sqlite      — query a SQLite db file
  - brave-search — web search (needs BRAVE_API_KEY, free 2k/mo)
  - fetch       — fetch a URL and convert to markdown
  - slack       — read/post Slack messages (needs SLACK_BOT_TOKEN, SLACK_TEAM_ID)
  - gmail       — read/search/send Gmail (OAuth flow on first run)
  - calendar    — Google Calendar (OAuth flow on first run)
  - memory      — persistent key-value memory across sessions
  - time        — time/timezone helpers

Reply with ONLY valid JSON in this exact shape (no markdown fences, no commentary):

{
  "displayName": "<3-5 word agent name in Title Case>",
  "description": "<1 sentence describing what this agent does>",
  "model": "<recommended model id, e.g. claude-sonnet-4-6 or gpt-4.1>",
  "systemPrompt": "<the agent's system prompt — 3-8 sentences, reference specific MCP tools when useful (e.g., 'use the gmail tool to fetch new messages')>",
  "recommendedMcps": ["<id from the catalog>", ...],
  "reasoning": "<1 sentence on why this stack fits the goal>"
}`;

const RUNTIME_DEFAULT_MODEL: Record<AgentRuntime, string> = {
  claude: "claude-sonnet-4-6",
  codex: "gpt-4.1",
  gemini: "gemini-2.0-flash-exp",
  openclaw: "claude-sonnet-4-6",
  hermes: "hermes-3",
};

/**
 * Walk the resolution order until one method succeeds. Throws NoRuntimeError
 * when nothing is configured.
 */
export async function suggestStack(goal: string): Promise<StackSuggestion> {
  if (!goal.trim()) throw new Error("goal cannot be empty");

  // Step 1: try CLI subscription on any healthy runtime.
  const statuses = await queryAllAgentStatuses().catch(() => []);
  const ready = statuses.find((s) => s.available && s.healthy);
  if (ready) {
    const runtime = ready.runtime as AgentRuntime;
    try {
      const text = await promptAgent(runtime, SUGGEST_PROMPT(goal));
      const parsed = parseSuggestionJson(text);
      return {
        runtime,
        model: parsed.model || RUNTIME_DEFAULT_MODEL[runtime],
        displayName: parsed.displayName,
        description: parsed.description,
        systemPrompt: parsed.systemPrompt,
        reasoning: parsed.reasoning,
        recommendedMcps: parsed.recommendedMcps,
      };
    } catch (err) {
      // Fall through to the next strategy on failure.
      console.warn("agentSuggest: CLI subscription path failed", err);
    }
  }

  // Step 4: hosted Pro fallback. We only attempt this when the user is
  // signed into ATO Pro; the cloud route requires it. Auth headers come from
  // the user's stored session — in this Tauri app we do not have a global
  // auth store wired here yet, so we attempt unauthenticated and the API
  // gateway will respond 401 → cleanly fall through.
  try {
    const cloud = await fetchHostedSuggest(goal, statuses.map((s) => s.runtime));
    if (cloud) return cloud;
  } catch (err) {
    console.warn("agentSuggest: hosted fallback failed", err);
  }

  // Steps 2-3 (API key direct, Ollama) land in T3.b.
  throw new NoRuntimeError();
}

const HOSTED_SUGGEST_URL =
  (typeof import.meta !== "undefined" &&
    (import.meta as ImportMeta & { env?: Record<string, string> }).env?.VITE_AGENT_SUGGEST_URL) ||
  "https://api.agentictool.ai/api/agent-suggest";

async function fetchHostedSuggest(
  goal: string,
  availableRuntimes: string[]
): Promise<StackSuggestion | null> {
  const token = readJwt();
  if (!token) return null; // not signed in → skip silently
  const res = await fetch(HOSTED_SUGGEST_URL, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({ goal, availableRuntimes }),
  });
  if (!res.ok) return null;
  const body = (await res.json()) as {
    success?: boolean;
    data?: {
      runtime: AgentRuntime;
      model: string;
      displayName: string;
      description: string;
      systemPrompt: string;
      reasoning: string;
      recommendedMcps?: string[];
    };
  };
  if (!body?.success || !body.data) return null;
  return {
    ...body.data,
    recommendedMcps: body.data.recommendedMcps ?? [],
  };
}

function readJwt(): string | null {
  // Cloud auth (CloudAuth + LoginModal) stores tokens via cloud-api's
  // storeTokens() under the `ato_cloud_tokens` key. Reuse the same accessor
  // so we automatically pick up refreshed tokens.
  return getStoredTokens()?.accessToken ?? null;
}

type RawSuggestion = {
  displayName: string;
  description: string;
  model: string;
  systemPrompt: string;
  reasoning: string;
  recommendedMcps: string[];
};

function parseSuggestionJson(raw: string): RawSuggestion {
  // Models often wrap the JSON in prose or fences. Pull the first {...} block.
  const match = raw.match(/\{[\s\S]*\}/);
  if (!match) throw new Error("Suggestion model returned no JSON object");
  const obj = JSON.parse(match[0]) as Partial<RawSuggestion>;
  if (!obj.displayName || !obj.systemPrompt) {
    throw new Error("Suggestion JSON missing displayName / systemPrompt");
  }
  return {
    displayName: obj.displayName,
    description: obj.description ?? "",
    model: obj.model ?? "",
    systemPrompt: obj.systemPrompt,
    reasoning: obj.reasoning ?? "",
    recommendedMcps: Array.isArray(obj.recommendedMcps)
      ? obj.recommendedMcps.filter((s): s is string => typeof s === "string")
      : [],
  };
}
