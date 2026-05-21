import { promptAgent, queryAllAgentStatuses, getSkills, type AgentRuntime } from "@/lib/tauri-api";
import { getStoredTokens } from "@/lib/cloud-api";

// v1.3.0 T3.c — Multi-turn agent-creation conversation.
//
// The LLM runs the wizard. Each turn we send the full history; the LLM decides
// whether to ask another clarifying question or emit the final spec (`review`).
// Frontend renders different UI per turn type. The user can reply with free
// text or click a suggestion chip; we append the message and re-prompt.

export type Turn =
  | { role: "user"; content: string }
  | { role: "assistant"; type: "ask"; text: string; suggestions?: string[] }
  | { role: "assistant"; type: "review"; text: string; spec: AgentSpec };

export type CredentialRequest = {
  /** Display label, e.g. "GitHub Personal Access Token". */
  label: string;
  /** Env var name the MCP needs, e.g. "GITHUB_PERSONAL_ACCESS_TOKEN". */
  envVar: string;
  /** "key" = paste a token; "oauth" = OAuth flow on first MCP run. */
  kind: "key" | "oauth";
  note?: string;
};

export type Permissions = {
  /** 1-line summary of what the agent IS allowed to do (for the review card). */
  summary: string;
  /** Semantic action labels (e.g., "read_emails", "draft_replies", "send_emails"). */
  allowed: string[];
  /** Semantic action labels the agent must ASK before doing (e.g., "send_emails"). */
  requireApproval: string[];
  /** Actions the agent must NEVER do under any circumstance. */
  denied: string[];
};

export type AgentSpec = {
  displayName: string;
  description: string;
  runtime: AgentRuntime;
  model: string;
  systemPrompt: string;
  recommendedMcps: string[];
  recommendedSkills: string[];
  credentials: CredentialRequest[];
  permissions: Permissions;
  reasoning: string;
  /** v2.7.9 Felipe P5 — optional dispatch-time prompt that fires
   *  automatically when the agent is run without one. Used by
   *  monitoring agents (VPS health, telemetry) where every run is
   *  the same prompt. Empty/undefined preserves interactive behavior. */
  defaultPrompt?: string;
};

export class NoRuntimeError extends Error {
  constructor() {
    super("NO_RUNTIME_AVAILABLE");
    this.name = "NoRuntimeError";
  }
}

const MCP_CATALOG_FOR_LLM = `
MCP catalog (use these exact IDs in recommendedMcps):
  - filesystem  — read/write local files
  - github      — GitHub issues/PRs/code search (needs GITHUB_PERSONAL_ACCESS_TOKEN, kind: key)
  - postgres    — read-only Postgres queries (needs DATABASE_URL, kind: key)
  - sqlite      — query a SQLite db file
  - brave-search — web search (needs BRAVE_API_KEY, kind: key, free 2k/mo)
  - fetch       — fetch URL → markdown
  - slack       — Slack messages (needs SLACK_BOT_TOKEN + SLACK_TEAM_ID, kind: key)
  - gmail       — Gmail (kind: oauth, browser flow on first run)
  - calendar    — Google Calendar (kind: oauth, browser flow on first run)
  - memory      — persistent memory across sessions
  - time        — time/timezone helpers
`;

const SYSTEM_PROMPT = (skillsList: string, runtime: AgentRuntime) => `You are the conductor of an agent-creation wizard. Your job: ask short, concrete clarifying questions until you have enough to spec a working agent that can ACTUALLY do its job, then emit a review.

You are creating an agent for runtime: ${runtime}.

${MCP_CATALOG_FOR_LLM}

User's installed skills (you can suggest enabling some in recommendedSkills — match by name):
${skillsList || "(none installed)"}

PROTOCOL — every turn you reply with ONE valid JSON object, no markdown fences, no commentary, exactly one of:

  { "type": "ask", "text": "<short question, 1 sentence>", "suggestions": ["short", "answer", "chips"] }

  { "type": "review", "text": "<1-line summary>", "spec": {
      "displayName": "<3-5 word Title Case name>",
      "description": "<1 sentence>",
      "runtime": "${runtime}",
      "model": "<model id>",
      "systemPrompt": "<3-8 sentence agent instructions, mention MCP tools by id>",
      "recommendedMcps": ["<id from catalog>", ...],
      "recommendedSkills": ["<exact skill name from list>", ...],
      "credentials": [
        { "label": "...", "envVar": "...", "kind": "key" | "oauth", "note": "..." }
      ],
      "permissions": {
        "summary": "<1-line plain-English summary of what the agent IS allowed to do>",
        "allowed": ["<action_label>", ...],
        "requireApproval": ["<action_label that must be confirmed>", ...],
        "denied": ["<action_label that is never allowed>", ...]
      },
      "reasoning": "<1 sentence>"
  }}

Rules:
- Ask 3-7 questions before review. Cover at minimum: domain specifics (e.g., which provider for email), tone/style/voice (especially for content-producing agents), permissions, optional skills. Then emit review.
- Don't ask about auth or credentials — surface them in the spec.
- "suggestions" are clickable chips (3-6 short options); always include them when there's a clear shortlist.

REQUIRED QUESTIONS — for any agent, ask all of these (in any order that flows naturally):
  1. Domain specifics. Email → "Which email provider? (Gmail / Outlook / Other)". Code → "Which language(s) / repo(s)?". Data → "Which database / table?".
  2. Tone & style — only for agents that produce content (write, draft, summarize). Ask "How should it write?" with chips like "Professional", "Casual / friendly", "Direct & terse", "Match my existing style", "Formal".
  3. Context — anything specific to this user. Email → "Personal, work, or both?", "Anything to always prioritize or always ignore (newsletters, specific senders)?".
  4. Filesystem scope — IF you're recommending the "filesystem" MCP, ask "Which folders should the agent be able to read/write?". Chips: "This project only", "My Documents", "My Desktop", "Custom paths". The user picks a path scope at install time, but ask first so they understand the agent isn't getting whole-disk access.
  5. Permissions — "Should the agent take actions automatically, or ask for approval?". Chips for an email agent: "Read only", "Read + draft replies (no auto-send)", "Auto-send routine replies, ask for sensitive ones", "Full access (read, draft, send, delete)". Translate the answer into allowed / requireApproval / denied.
  6. Optional skills — if the user has skills installed (see list above), ask "Want any of your installed skills enabled for this agent?" with chips for the most relevant matches. If no skills are installed, skip this question.

REVIEW SHAPE:
- Reference MCPs and skills in systemPrompt when useful (e.g., "use gmail to fetch new messages"). REINFORCE the permissions in the system prompt — e.g., "Never send emails without explicit user approval."
- Credentials must mirror the MCP catalog auth notes exactly. For OAuth MCPs (gmail, calendar), use kind: "oauth" with note: "Browser-based Google OAuth flow on first run — no API key needed".
- Permission action labels should be specific verbs related to the MCPs you chose. For gmail: read_emails, search_emails, draft_replies, send_emails, delete_emails. For github: read_repos, comment_on_issues, create_issues, merge_prs. For slack: read_messages, send_messages, react_to_messages. Be concrete.
`;

export async function startConversation(
  goal: string,
  runtimeOverride?: AgentRuntime
): Promise<{
  runtime: AgentRuntime;
  history: Turn[];
  next: Extract<Turn, { role: "assistant" }>;
}> {
  const runtime = runtimeOverride ?? (await pickRuntime());
  const skills = await getSkills().catch(() => []);
  const skillsList = skills
    .map((s) => `  - ${s.name} (${s.runtime}): ${s.description}`)
    .slice(0, 30)
    .join("\n");

  const history: Turn[] = [{ role: "user", content: goal }];
  const next = await callLLM(runtime, skillsList, history);
  return { runtime, history, next };
}

/** Returns runtimes that are detected + healthy (i.e., ready to drive the wizard). */
export async function listReadyRuntimes(): Promise<AgentRuntime[]> {
  const statuses = await queryAllAgentStatuses().catch(() => []);
  return statuses
    .filter((s) => s.available && s.healthy)
    .map((s) => s.runtime as AgentRuntime);
}

export async function continueConversation(
  runtime: AgentRuntime,
  history: Turn[],
  userReply: string
): Promise<Extract<Turn, { role: "assistant" }>> {
  const skills = await getSkills().catch(() => []);
  const skillsList = skills
    .map((s) => `  - ${s.name} (${s.runtime}): ${s.description}`)
    .slice(0, 30)
    .join("\n");
  const newHistory: Turn[] = [...history, { role: "user", content: userReply }];
  return callLLM(runtime, skillsList, newHistory);
}

async function pickRuntime(): Promise<AgentRuntime> {
  const statuses = await queryAllAgentStatuses().catch(() => []);
  const ready = statuses.find((s) => s.available && s.healthy);
  if (ready) return ready.runtime as AgentRuntime;
  // Hosted Pro fallback only works if signed in. Otherwise: no runtime.
  if (getStoredTokens()?.accessToken) return "claude" as AgentRuntime;
  throw new NoRuntimeError();
}

async function callLLM(
  runtime: AgentRuntime,
  skillsList: string,
  history: Turn[]
): Promise<Extract<Turn, { role: "assistant" }>> {
  // Render the conversation as a transcript for the local CLI.
  // (CLIs are stateless --print mode, so we always send the full transcript.)
  const transcript = history
    .map((t) => {
      if (t.role === "user") return `USER: ${t.content}`;
      if (t.type === "ask")
        return `ASSISTANT: ${JSON.stringify({ type: "ask", text: t.text, suggestions: t.suggestions ?? [] })}`;
      return `ASSISTANT: ${JSON.stringify({ type: "review", text: t.text, spec: t.spec })}`;
    })
    .join("\n\n");

  const fullPrompt = `${SYSTEM_PROMPT(skillsList, runtime)}\n\nConversation so far:\n\n${transcript}\n\nReply with the next JSON object (ask or review only). Do not echo prior turns.`;

  const text = await promptAgent(runtime, fullPrompt);
  return parseAssistantTurn(text);
}

function parseAssistantTurn(raw: string): Extract<Turn, { role: "assistant" }> {
  const match = raw.match(/\{[\s\S]*\}/);
  if (!match) throw new Error("Wizard model returned no JSON");
  const obj = JSON.parse(match[0]) as Record<string, unknown>;
  const type = obj.type;
  if (type === "ask") {
    if (typeof obj.text !== "string") throw new Error("ask: missing text");
    return {
      role: "assistant",
      type: "ask",
      text: obj.text,
      suggestions: Array.isArray(obj.suggestions)
        ? obj.suggestions.filter((s): s is string => typeof s === "string")
        : undefined,
    };
  }
  if (type === "review") {
    const spec = obj.spec as Partial<AgentSpec> | undefined;
    if (!spec || typeof spec.displayName !== "string" || typeof spec.systemPrompt !== "string") {
      throw new Error("review: incomplete spec");
    }
    return {
      role: "assistant",
      type: "review",
      text: typeof obj.text === "string" ? obj.text : "",
      spec: {
        displayName: spec.displayName,
        description: spec.description ?? "",
        runtime: (spec.runtime ?? "claude") as AgentRuntime,
        model: spec.model ?? "claude-sonnet-4-6",
        systemPrompt: spec.systemPrompt,
        recommendedMcps: Array.isArray(spec.recommendedMcps)
          ? spec.recommendedMcps.filter((s): s is string => typeof s === "string")
          : [],
        recommendedSkills: Array.isArray(spec.recommendedSkills)
          ? spec.recommendedSkills.filter((s): s is string => typeof s === "string")
          : [],
        credentials: Array.isArray(spec.credentials)
          ? (spec.credentials.filter(
              (c) =>
                c &&
                typeof (c as CredentialRequest).label === "string" &&
                typeof (c as CredentialRequest).envVar === "string"
            ) as CredentialRequest[])
          : [],
        permissions: parsePermissions(spec.permissions),
        reasoning: spec.reasoning ?? "",
        defaultPrompt:
          typeof spec.defaultPrompt === "string" && spec.defaultPrompt.trim().length > 0
            ? spec.defaultPrompt
            : undefined,
      },
    };
  }
  throw new Error(`Unknown assistant turn type: ${String(type)}`);
}

function parsePermissions(raw: unknown): Permissions {
  const empty: Permissions = { summary: "", allowed: [], requireApproval: [], denied: [] };
  if (!raw || typeof raw !== "object") return empty;
  const r = raw as Record<string, unknown>;
  const arr = (k: string): string[] =>
    Array.isArray(r[k]) ? (r[k] as unknown[]).filter((s): s is string => typeof s === "string") : [];
  return {
    summary: typeof r.summary === "string" ? r.summary : "",
    allowed: arr("allowed"),
    requireApproval: arr("requireApproval"),
    denied: arr("denied"),
  };
}
