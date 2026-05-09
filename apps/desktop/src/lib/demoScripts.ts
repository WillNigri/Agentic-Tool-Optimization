import type { DemoScript } from "@/stores/useDemoStore";

// v1.5.0 — Canonical recordable demo scripts.
//
// FULL_TOUR is the marketing video script. A first-time user:
//   1. Lands on Home, sees the value prop
//   2. Goes to Agents, opens Templates wizard for visual context
//   3. Switches to Quick path and creates ONE agent by hand (animated typing
//      in the form fields), saves it
//   4. We seed two more specialized agents via direct backend create
//   5. Build a group out of the three with keyword router rules
//   6. Show the agents list populated with everything
//   7. Land in the chat pane: pick the freshly-created reviewer agent,
//      ask it for a code review → real streamed response
//   8. Switch runtime to Codex mid-thread, ask for the prior topic → Codex
//      pulls the Claude history (the cross-runtime money shot)
//
// HERO is the cross-runtime climax on its own (~60s).
// SHORT is single streamed response with markdown (~20s).
//
// Cmd+Shift+D plays HERO. Bottom-right pill picks any. Esc stops.

const REVIEWER_SYSTEM_PROMPT = `You are a code reviewer. When given a diff or PR description, surface real issues with surgical specificity.

Priorities, in order:
1. Correctness — does the code do what it claims? Edge cases that silently break?
2. Security — input validation, secrets handling, injection risks.
3. Tests — is the new behavior tested meaningfully?
4. Clarity — names, structure, comments where the WHY isn't obvious.

Style:
- Lead with the highest-severity issue. One concern per comment.
- Quote the exact line. Suggest a concrete fix.
- Skip nitpicks unless they compound into real readability problems.
- If the code is good, say so plainly.`;

const SECURITY_SYSTEM_PROMPT = `You are a security-focused code reviewer. Only flag security issues — input validation, secrets, auth boundaries, injection, deserialization, race conditions on auth.

Be specific. No generic warnings. If you don't see a security issue, say "No security issues found" and stop.`;

const WRITER_SYSTEM_PROMPT = `You are a code writer. When asked to write code, return ONLY the code in a fenced code block — no preamble, no explanation, no trailing commentary.

Rules:
- Pick the language the user asked for. If unclear, default to Python.
- Include a docstring or comment for non-obvious functions.
- Add inline comments only where the WHY isn't obvious from the code.
- Don't write tests unless asked.
- Don't write markdown around the code block — just the fenced block.

Output format:
\`\`\`<language>
<your code>
\`\`\``;

export const FULL_TOUR_SCRIPT: DemoScript = {
  id: "full-tour",
  label: "Full tour — first-time user creates agents from scratch",
  shortDescription: "Build 3 agents + a group, then run them — end to end",
  steps: [
    // ── Cleanup: wipe anything left over from a prior demo run so the
    //    "create from scratch" story is honest every time. Silent. ────────
    {
      kind: "cleanup",
      runtime: "claude",
      agentSlugs: ["code-reviewer", "code-writer", "security-reviewer", "perf-reviewer", "acme-support", "support-bot"],
      groupSlugs: ["code-review-team", "write-and-review"],
      apiKeyNames: ["[DEMO] Anthropic"],
    },
    // v2.0.0 — pre-seed a fake Anthropic key so the External wizard shows
    // the "key on file" success state during the demo. Cleanup above
    // wipes it on next run. The key value is a placeholder; nothing
    // actually dispatches to the network during the demo.
    {
      kind: "seedDemoApiKey",
      provider: "anthropic",
      name: "[DEMO] Anthropic",
      value: "sk-ant-demo-DO-NOT-USE-this-is-a-placeholder-key",
    },
    // ── Open: Home ───────────────────────────────────────────────────────
    // Collapse the chat pane while we tour sections so each one gets full
    // vertical space — the chat reopens later for the hero workflow.
    { kind: "setChatPaneOpen", open: false },
    { kind: "navigate", section: "home" },
    {
      kind: "subtitle",
      textKey: "demo.subtitles.intro",
      text: "ATO — the GUI for daily agentic work.",
      durationMs: 2400,
    },
    {
      kind: "subtitle",
      textKey: "demo.subtitles.oneWorkspace",
      text: "One workspace for Claude, Codex, Gemini, OpenClaw, Hermes, Ollama.",
      durationMs: 2800,
    },
    { kind: "wait", ms: 500 },

    // ── Show three creation paths: Templates teaser → Guided chat → Quick
    { kind: "navigate", section: "agents" },
    {
      kind: "subtitle",
      textKey: "demo.subtitles.threeWays",
      text: "Three ways to create an agent. Let's see all of them.",
      durationMs: 2400,
    },
    { kind: "openWizard", path: "templates" },
    {
      kind: "subtitle",
      text: "Templates — production-quality starters across engineering, writing, ops, and data.",
      durationMs: 2400,
    },
    { kind: "wait", ms: 800 },
    // ── Guided (chat) path teaser. We deliberately just show the goal
    //    submission and the assistant kicking off — we don't hold long
    //    enough to enter MCP install / auth phases, which can stall while
    //    the LLM is still thinking. Close the wizard before that happens.
    { kind: "openWizard", path: "guided" },
    {
      kind: "subtitle",
      text: "Or describe what you want — ATO drives the conversation.",
      durationMs: 2400,
    },
    {
      kind: "typeGuidedGoal",
      text: "Help me build an agent that reviews my pull requests for security issues",
    },
    { kind: "wait", ms: 350 },
    { kind: "submitGuidedGoal" },
    // Dwell long enough to actually capture the assistant's first response.
    // claude --print can take 8-20s for the first turn; if it lands earlier
    // we just sit on it for a moment, which reads fine in the recording.
    {
      kind: "subtitle",
      text: "Thinking… ATO is drafting clarifying questions.",
      durationMs: 2200,
    },
    { kind: "wait", ms: 14000 },
    {
      kind: "subtitle",
      text: "Once it has enough, ATO scaffolds the agent. We'll go faster with the form.",
      durationMs: 3000,
    },
    { kind: "wait", ms: 800 },
    // Switch to Quick path to show the form-based hand-build.
    { kind: "openWizard", path: "quick" },
    {
      kind: "subtitle",
      text: "Or skip the chat — go straight to the form. Same end state.",
      durationMs: 2600,
    },
    { kind: "wait", ms: 600 },

    // ── Create agent #1 BY HAND — animated typing into the form fields ──
    {
      kind: "subtitle",
      text: "Name → runtime → system prompt. Save.",
      durationMs: 2200,
    },
    { kind: "setAgentField", field: "runtime", value: "claude" },
    { kind: "setAgentField", field: "model", value: "claude-sonnet-4-6" },
    { kind: "wait", ms: 200 },
    { kind: "typeAgentField", field: "name", text: "code-reviewer" },
    { kind: "wait", ms: 350 },
    {
      kind: "typeAgentField",
      field: "description",
      text: "Reviews code diffs for correctness, security, tests, and clarity.",
      charsPerSec: 38,
    },
    { kind: "wait", ms: 350 },
    {
      kind: "typeAgentField",
      field: "systemPrompt",
      text: REVIEWER_SYSTEM_PROMPT,
      charsPerSec: 80,
    },
    { kind: "wait", ms: 400 },
    // Context files — these live in <context>, not in the system prompt.
    {
      kind: "subtitle",
      text: "Context files: loaded every turn into <context>, not into the system prompt.",
      durationMs: 3000,
    },
    {
      kind: "setAgentField",
      field: "contextFiles",
      value: ["~/CLAUDE.md", "~/.claude/CONVENTIONS.md"],
    },
    { kind: "wait", ms: 1200 },
    // Scroll the modal so the Save button is actually in frame before we
    // narrate it — a viewer of the recording shouldn't have to imagine
    // where the click lands.
    { kind: "scrollIntoView", id: "quick-form-save", block: "center" },
    { kind: "wait", ms: 400 },
    {
      kind: "subtitle",
      textKey: "demo.subtitles.oneClickSave",
      text: "One click to save — file written, agent registered, ready to run.",
      durationMs: 2400,
    },
    { kind: "highlight", id: "quick-form-save", durationMs: 1500 },
    { kind: "submitAgentForm" },
    // Brief dwell on the "Agent created" success card so it registers.
    { kind: "wait", ms: 1200 },
    // Close the wizard so the agent list shows underneath, with the new
    // agent now in it.
    { kind: "closeWizard" },
    { kind: "wait", ms: 900 },
    {
      kind: "subtitle",
      text: "There it is — code-reviewer, listed and ready.",
      durationMs: 2400,
    },
    { kind: "wait", ms: 800 },

    // ── Seed three more agents via the backend (fast, no animation) ─────
    //    A writer + two specialized reviewers. Together they form the
    //    write→review workflow we'll route through a group.
    {
      kind: "subtitle",
      text: "Now two more — a code writer + a security reviewer. The group below will chain them.",
      durationMs: 2800,
    },
    {
      kind: "createAgent",
      spec: {
        displayName: "code-writer",
        runtime: "claude",
        model: "claude-sonnet-4-6",
        description: "Writes code to spec — code only, no commentary.",
        systemPrompt: WRITER_SYSTEM_PROMPT,
        goal: "Write code on request, code-only output",
      },
    },
    // Cross-runtime: writer is Claude, reviewer is Codex. Sequential
    // dispatch hits each child on its own runtime — Claude → Codex.
    {
      kind: "createAgent",
      spec: {
        displayName: "security-reviewer",
        runtime: "codex",
        model: "gpt-4.1",
        description: "Reviews code for security issues only.",
        systemPrompt: SECURITY_SYSTEM_PROMPT,
        goal: "Surface security-only review notes",
      },
    },
    { kind: "wait", ms: 600 },
    // v2.0 — the External agent is created LIVE through the wizard so
    // the viewer sees the kind picker, AuthRequirements panel, and the
    // External-specific success-state CTAs. The walkthrough of the
    // Knowledge / Context / Deploy / Raw tabs happens later, AFTER the
    // creation, when we click "Generate deploy bundle →" from the
    // success state.
    { kind: "wait", ms: 400 },

    // ── Build the group ──────────────────────────────────────────────────
    {
      kind: "subtitle",
      textKey: "demo.subtitles.bundleGroup",
      text: "Bundle them into a group. Router decides who handles what.",
      durationMs: 2800,
    },
    // ── Two group types — both created so viewers see both patterns ─────
    {
      kind: "subtitle",
      textKey: "demo.subtitles.twoGroupTypes",
      text: "Two group types: routed (router picks one) and automation (pipeline).",
      durationMs: 3000,
    },
    // 1. Sequential automation pipeline — built BY HAND through the form.
    //    Sequential makes a stronger visual story than routed because the
    //    chat segment that follows fires BOTH children (writer → reviewer)
    //    and the viewer has just watched the same group being assembled.
    //    Routed groups are still supported, but the demo focuses on the
    //    pattern that produces 2 answers from 1 prompt.
    { kind: "setSubTab", storageKey: "ato.subtab.agents", tabId: "groups" },
    { kind: "wait", ms: 600 },
    { kind: "subtitle", text: "Click + New group.", durationMs: 1800 },
    { kind: "clickByDemoId", id: "group-new" },
    { kind: "wait", ms: 700 },
    {
      kind: "subtitle",
      text: "Name → type → children → save. Sequential pipeline runs each child in order.",
      durationMs: 2800,
    },
    {
      kind: "autoFillGroupForm",
      spec: {
        displayName: "write-and-review",
        runtime: "claude",
        description: "Automation: writer → security review, all from one prompt.",
        dispatchKind: "sequential",
        childSlugs: ["code-writer", "security-reviewer"],
      },
    },
    // Dwell on the populated form so the viewer can read the children
    // before the next step (scroll-to-save) snaps the page to the header.
    {
      kind: "subtitle",
      text: "Two children, in order: code-writer first, then security-reviewer. One prompt fires both.",
      durationMs: 3000,
    },
    { kind: "wait", ms: 800 },
    { kind: "scrollIntoView", id: "group-save", block: "center" },
    { kind: "wait", ms: 400 },
    {
      kind: "subtitle",
      text: "Save the group — written to disk, ready to dispatch.",
      durationMs: 2200,
    },
    { kind: "highlight", id: "group-save", durationMs: 1500 },
    { kind: "clickByDemoId", id: "group-save" },
    { kind: "wait", ms: 1500 },
    // Reset Agents sub-tab back to "mine" so the next phase shows the
    // populated agent list rather than the groups list.
    { kind: "setSubTab", storageKey: "ato.subtab.agents", tabId: "mine" },
    { kind: "wait", ms: 600 },

    // ── Tour: Insights ──────────────────────────────────────────────────
    { kind: "navigate", section: "insights" },
    {
      kind: "subtitle",
      text: "Every dispatch is traced. Per-agent latency, success rate, evaluators.",
      durationMs: 3000,
    },
    { kind: "wait", ms: 1000 },

    // ── Hero: automation pipeline — ONE prompt runs the whole workflow ──
    //    Pick the SEQUENTIAL group. One prompt fires both children:
    //    writer (Claude) produces code, then reviewer (Codex) reviews it.
    //    Chat shows the transcript with each agent + runtime labelled.
    { kind: "navigate", section: "home" },
    // Bring the chat pane back for the actual workflow demo.
    { kind: "setChatPaneOpen", open: true },
    {
      kind: "subtitle",
      textKey: "demo.subtitles.automationPipeline",
      text: "Now the automation pipeline — one prompt, the whole workflow runs.",
      durationMs: 2800,
    },
    { kind: "newThread" },
    { kind: "setRuntime", runtime: "claude" },
    { kind: "selectChatGroup", slug: "write-and-review" },
    {
      kind: "subtitle",
      text: "Sequential pipeline: code-writer (Claude) → security-reviewer (Codex).",
      durationMs: 3000,
    },
    {
      kind: "type",
      text: "Write a Python function to fetch a user by id from a SQL database, then review it.",
    },
    { kind: "wait", ms: 350 },
    { kind: "send" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Two messages back: Claude wrote the code, Codex reviewed it. One prompt.",
      durationMs: 3500,
    },
    { kind: "wait", ms: 1200 },

    // ── Cross-runtime mid-thread ─────────────────────────────────────────
    // Pipeline above ended on Codex (security-reviewer's runtime). Swap
    // BACK to Claude here — Claude inherits the full history and can
    // summarize what Codex just flagged. Beatriz feedback 2026-05-07.
    {
      kind: "subtitle",
      textKey: "demo.subtitles.swapMidThread",
      text: "Now swap back to Claude mid-thread — the full history travels.",
      durationMs: 2400,
    },
    { kind: "selectChatGroup", slug: null },
    { kind: "selectAgent", slug: null },
    { kind: "setRuntime", runtime: "claude" },
    { kind: "wait", ms: 500 },
    {
      kind: "type",
      text: "Summarize what we just did, including the security issue Codex flagged.",
    },
    { kind: "wait", ms: 300 },
    { kind: "send" },
    { kind: "wait", ms: 800 },
    {
      kind: "subtitle",
      text: "Claude remembers what it wrote, what Codex flagged. One thread, two runtimes.",
      durationMs: 3500,
    },
    { kind: "wait", ms: 800 },

    // ── Schedule it ──────────────────────────────────────────────────────
    //    Show that the workflow can also run on a cron — same agent, no
    //    babysitting. Seed the cron job programmatically and navigate to
    //    Runs → Schedules so it appears in the list.
    {
      kind: "subtitle",
      textKey: "demo.subtitles.scheduleUnattended",
      text: "And you can schedule it to run unattended.",
      durationMs: 2400,
    },
    {
      kind: "createCronJob",
      name: "Daily security review",
      description: "Fires @security-reviewer (Codex) over yesterday's diff every weekday morning.",
      schedule: "0 9 * * 1-5",
      runtime: "codex",
      // The cron dispatches THROUGH the agent — variables, hooks, system
      // prompt, all fire on every run. The "prompt" here is just the
      // message the agent receives, not a re-spec of the agent.
      agentSlug: "security-reviewer",
      prompt: "Review the diff from `git log -p -1` for security issues. Skip if no diff.",
    },
    // Collapse chat first so the cron monitor isn't squeezed under the
    // chat pane — without this the Calendar view is barely visible.
    { kind: "setChatPaneOpen", open: false },
    { kind: "navigate", section: "runs" },
    { kind: "setSubTab", storageKey: "ato.subtab.runs", tabId: "schedules" },
    { kind: "wait", ms: 1200 },
    {
      kind: "subtitle",
      text: "Same agents. Now on a cron — listed in the schedule monitor.",
      durationMs: 2800,
    },
    { kind: "wait", ms: 1000 },
    // Switch to the calendar view so viewers see the visual layout of
    // upcoming runs, not just a list row.
    { kind: "clickByDemoId", id: "cron-view-calendar" },
    { kind: "wait", ms: 900 },
    {
      kind: "subtitle",
      text: "Calendar view: past runs (green/red) + upcoming runs at a glance.",
      durationMs: 3200,
    },
    { kind: "wait", ms: 1200 },
    {
      kind: "subtitle",
      textKey: "demo.subtitles.wakeFromSleep",
      text: "Wake-from-sleep on macOS, Linux, and Windows — jobs fire even when ATO is closed.",
      durationMs: 3600,
    },
    { kind: "wait", ms: 1200 },

    // ── v2.0 — External agents: live creation + tab walkthrough ────────
    // The cross-runtime + scheduling story above is the developer-facing
    // surface. v2.0 turns ATO into the place where you build agents for
    // OTHER PEOPLE — customer chatbots that deploy to your customers' own
    // infrastructure. We CREATE the external agent live through the
    // wizard so the viewer sees the kind picker, AuthRequirements panel,
    // and the External-specific success-state CTAs. Then we click
    // "Generate deploy bundle →" to land on Deploy and walk through the
    // four power tabs (Knowledge / Context / Deploy / Raw).
    { kind: "setChatPaneOpen", open: false },
    { kind: "navigate", section: "agents" },
    { kind: "setSubTab", storageKey: "ato.subtab.agents", tabId: "mine" },
    { kind: "wait", ms: 600 },
    {
      kind: "subtitle",
      text: "v2.0 — agents you build for your CUSTOMERS, not just yourself.",
      durationMs: 3000,
    },
    // Open the Quick wizard for a hand-built External agent.
    { kind: "openWizard", path: "quick" },
    { kind: "wait", ms: 700 },
    {
      kind: "subtitle",
      text: "First decision in the wizard: Internal (you) vs External (your customers).",
      durationMs: 3200,
    },
    // Highlight + click the External kind tile so the AuthRequirements
    // panel shows below it.
    { kind: "highlight", id: "agent-kind-external", durationMs: 1500 },
    { kind: "clickByDemoId", id: "agent-kind-external" },
    { kind: "wait", ms: 700 },
    {
      kind: "subtitle",
      text: "External: read-only by default, no shell, no filesystem writes — safe for customer-facing deploys.",
      durationMs: 3800,
    },
    {
      kind: "subtitle",
      text: "AuthRequirements panel: ATO checks for chat-provider keys you can deploy with. Anthropic key on file ✓.",
      durationMs: 4000,
    },
    // Type the rest of the form — name, description, system prompt with
    // a template variable so the prompt animation is meaningful.
    { kind: "setAgentField", field: "runtime", value: "claude" },
    { kind: "setAgentField", field: "model", value: "claude-sonnet-4-6" },
    { kind: "wait", ms: 200 },
    { kind: "typeAgentField", field: "name", text: "support-bot" },
    { kind: "wait", ms: 350 },
    {
      kind: "typeAgentField",
      field: "description",
      text: "Customer support chatbot for {company_name}. RAG-backed. Deploys to Cloudflare Worker.",
      charsPerSec: 42,
    },
    { kind: "wait", ms: 350 },
    {
      kind: "typeAgentField",
      field: "systemPrompt",
      text:
        "You are the customer support agent for {company_name}. " +
        "Answer using the policies in the <context> block when available. " +
        "If you don't know, say so and offer to escalate.",
      charsPerSec: 90,
    },
    { kind: "wait", ms: 600 },
    { kind: "scrollIntoView", id: "quick-form-save", block: "center" },
    { kind: "wait", ms: 400 },
    {
      kind: "subtitle",
      text: "Save — the agent record is written, ready to wire knowledge + deploy.",
      durationMs: 2800,
    },
    { kind: "highlight", id: "quick-form-save", durationMs: 1200 },
    { kind: "submitAgentForm" },
    { kind: "wait", ms: 1200 },
    // Success state — show the External-specific CTAs the viewer hasn't
    // seen on Internal saves earlier in the demo.
    {
      kind: "subtitle",
      text: "Created. External agents get two CTAs: Add knowledge, or jump straight to Generate deploy bundle.",
      durationMs: 4200,
    },
    { kind: "wait", ms: 600 },
    // Click "Generate deploy bundle →" — this calls openAgentDetail()
    // which routes us into the AgentDetail on the Deploy tab.
    { kind: "highlight", id: "quick-success-deploy", durationMs: 1500 },
    { kind: "clickByDemoId", id: "quick-success-deploy" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "External agents get four power tabs: Knowledge, Context, Deploy, and Raw. Let's walk through each.",
      durationMs: 4000,
    },

    // ── 1) Knowledge — static RAG ──────────────────────────────────────
    { kind: "highlight", id: "agent-tab-knowledge", durationMs: 1200 },
    { kind: "clickByDemoId", id: "agent-tab-knowledge" },
    { kind: "wait", ms: 1000 },
    {
      kind: "subtitle",
      text: "Knowledge — STATIC text the agent should always know. FAQs, policies, product docs.",
      durationMs: 3800,
    },
    {
      kind: "subtitle",
      text: "Drop .md / .txt → ATO chunks + embeds locally. Auto-routes to OpenAI / Voyage / Gemini / Cohere / Ollama.",
      durationMs: 4500,
    },
    {
      kind: "subtitle",
      text: "Test retrieval right here. The deployed agent runs the same cosine-similarity search per request, baked into the bundle.",
      durationMs: 4500,
    },

    // ── 2) Context — live data per turn ────────────────────────────────
    { kind: "highlight", id: "agent-tab-context", durationMs: 1200 },
    { kind: "clickByDemoId", id: "agent-tab-context" },
    { kind: "wait", ms: 1000 },
    {
      kind: "subtitle",
      text: "Context — LIVE data per turn. DB queries, CRM lookups, API calls, MCP tools.",
      durationMs: 3800,
    },
    {
      kind: "subtitle",
      text: "New: fire-mode picker. Always (every turn) / On keyword / LLM decides (cheap classifier ~$0.0001/turn).",
      durationMs: 4500,
    },
    {
      kind: "subtitle",
      text: "So 'fetch billing history' only fires when the user actually mentions billing. No wasted API calls.",
      durationMs: 4500,
    },

    // ── 3) Deploy — generate the bundle ────────────────────────────────
    { kind: "highlight", id: "agent-tab-deploy", durationMs: 1200 },
    { kind: "clickByDemoId", id: "agent-tab-deploy" },
    { kind: "wait", ms: 1200 },
    {
      kind: "subtitle",
      text: "Deploy — pick a target. Cloudflare Worker, Vercel Edge, Docker image, Node script.",
      durationMs: 4000,
    },
    {
      kind: "subtitle",
      text: "Pick a chat provider — 9 supported (Anthropic / OpenAI / Gemini / Groq / Mistral / DeepSeek / xAI / Together / Fireworks).",
      durationMs: 4500,
    },
    {
      kind: "subtitle",
      text: "Inline knowledge for RAG → chunks + embeddings baked into the worker.js. Self-contained, no external vector DB.",
      durationMs: 4500,
    },
    {
      kind: "subtitle",
      text: "Save bundle → folder full of files ready to wrangler deploy / vercel deploy / docker build.",
      durationMs: 4000,
    },

    // ── 4) Raw — power-user surface ────────────────────────────────────
    { kind: "highlight", id: "agent-tab-raw", durationMs: 1200 },
    { kind: "clickByDemoId", id: "agent-tab-raw" },
    { kind: "wait", ms: 1000 },
    {
      kind: "subtitle",
      text: "Raw — advanced view. Full SQLite state as JSON: variables, hooks, knowledge sources, memory, role models.",
      durationMs: 4500,
    },
    {
      kind: "subtitle",
      text: "Internal agents also see the on-disk file editor with hash-checked save + auto-backup.",
      durationMs: 4000,
    },

    // Closing
    {
      kind: "subtitle",
      text: "Customer's API key. Customer's infra. Your IDE. ATO never holds inference compute.",
      durationMs: 4500,
    },
    { kind: "wait", ms: 600 },
    // Close the AgentDetail overlay so the closing subtitles render on
    // top of the regular dashboard, not over the detail modal.
    { kind: "clickByDemoId", id: "agent-detail-close" },
    { kind: "wait", ms: 500 },
    { kind: "navigate", section: "home" },
    { kind: "wait", ms: 600 },

    // ── Close ────────────────────────────────────────────────────────────
    {
      kind: "subtitle",
      textKey: "demo.subtitles.closeBuildBundle",
      text: "Build the agents. Bundle them into a workflow. Run on demand or on a schedule.",
      durationMs: 3400,
    },
    {
      kind: "subtitle",
      textKey: "demo.subtitles.closeBranding",
      text: "ATO. Local-first. MIT licensed. agentictool.ai",
      durationMs: 3200,
    },
  ],
};

export const HERO_SCRIPT: DemoScript = {
  id: "hero",
  label: "Hero — cross-runtime mid-thread",
  shortDescription: "Claude → Codex in the same conversation",
  steps: [
    {
      kind: "subtitle",
      text: "Persistent threads. Streaming responses. Markdown rendering.",
      durationMs: 2200,
    },
    { kind: "navigate", section: "home" },
    { kind: "newThread" },
    { kind: "setRuntime", runtime: "claude" },
    { kind: "wait", ms: 400 },
    {
      kind: "type",
      text: "Walk me through how SSL handshake works. Include a small JSON example.",
    },
    { kind: "wait", ms: 350 },
    { kind: "send" },
    { kind: "wait", ms: 1200 },
    {
      kind: "subtitle",
      text: "Now switch runtime mid-thread.",
      durationMs: 1800,
    },
    { kind: "setRuntime", runtime: "codex" },
    { kind: "wait", ms: 400 },
    {
      kind: "type",
      text: "What was the most recent technical topic I asked about in this conversation?",
    },
    { kind: "wait", ms: 300 },
    { kind: "send" },
    { kind: "wait", ms: 800 },
    {
      kind: "subtitle",
      text: "Same thread. Different runtime. The history travels.",
      durationMs: 4000,
    },
  ],
};

export const SHORT_SCRIPT: DemoScript = {
  id: "short",
  label: "Short — streaming + markdown only",
  shortDescription: "One streamed Claude response with code block",
  steps: [
    { kind: "subtitle", text: "Streaming responses. Markdown rendering.", durationMs: 2000 },
    { kind: "navigate", section: "home" },
    { kind: "newThread" },
    { kind: "setRuntime", runtime: "claude" },
    { kind: "wait", ms: 300 },
    {
      kind: "type",
      text: "Give me a Rust function that reverses a string. Just the code in a fenced code block.",
    },
    { kind: "wait", ms: 300 },
    { kind: "send" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Tokens stream live. Code blocks have a Copy button.",
      durationMs: 3500,
    },
  ],
};

// ─────────────────────────────────────────────────────────────────────
// v2.1 standalone verification scripts.
//
// Each script demos ONE feature so you can verify it works without
// running the FULL_TOUR. They aren't shown in marketing video — these
// are for product QA.
//
// Pattern (REAL OPS — Beatriz feedback 2026-05-09):
//   1. Cleanup any leftover state from a previous run.
//   2. Create the agents / groups the demo needs (REAL writes).
//   3. Fire the actual dispatch / open the actual UI (REAL reads).
//   4. Narrate via subtitle so the operator knows what to watch for.
//   5. Cleanup at the END so the next run is also clean and the
//      operator's account doesn't accumulate demo cruft.
//
// Cloud trace data: requires cloud sign-in (Settings → Cloud).
// Local-only API key fallbacks: a fake [DEMO] Anthropic key is seeded
// for the embed-key demo because the user may not have a real
// Anthropic API key configured locally; that's the only mock these
// scripts use. Everything else is real.
// ─────────────────────────────────────────────────────────────────────

export const LIVE_RUNS_SCRIPT: DemoScript = {
  id: "v21-live-runs",
  label: "v2.1 — Live runs panel",
  shortDescription: "Fire a slow dispatch, watch it appear in Insights → Live with kill button",
  steps: [
    { kind: "subtitle", text: "v2.1 ops layer — Live runs panel.", durationMs: 2000 },
    // Open chat first + type the prompt so we have it ready. THEN
    // navigate to Live BEFORE sending — otherwise the dispatch
    // completes before the demo gets there and the row clears.
    { kind: "navigate", section: "home" },
    { kind: "setChatPaneOpen", open: true },
    { kind: "newThread" },
    { kind: "setRuntime", runtime: "claude" },
    { kind: "wait", ms: 400 },
    {
      kind: "type",
      // Long enough that claude --print takes ~15-30s to finish so
      // there's time to navigate + read the live row before the
      // registry clears.
      text:
        "Write a detailed 250-word essay on coffee culture in Brazil. " +
        "Cover the history, regional differences, and the role of cafés " +
        "in social life. Be thorough.",
    },
    { kind: "wait", ms: 400 },
    // Navigate to Live FIRST so we're already watching the panel
    // when send fires. The chat pane sits at the bottom across views
    // so the prompt stays visible.
    { kind: "navigate", section: "insights" },
    { kind: "setSubTab", storageKey: "ato.subtab.insights", tabId: "live" },
    { kind: "wait", ms: 600 },
    {
      kind: "subtitle",
      text: "Empty until something fires. Watch this panel — sending the prompt now.",
      durationMs: 3200,
    },
    // Now send. The demo runner awaits completion, but the panel is
    // already on screen and the live row populates within ~500ms of
    // the spawn. The 15s+ Claude duration gives plenty of time to
    // read it.
    { kind: "send" },
    {
      kind: "subtitle",
      text: "There — agent slug, runtime, workspace, elapsed. Kill button per row, no terminal-buffer hunting.",
      durationMs: 5500,
    },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Row clears when the dispatch returns. Try it again with a stuck process — Kill works.",
      durationMs: 4000,
    },
    // Live runs script doesn't create persistent state — the dispatch
    // is the only side effect and that's a real one-shot we want to
    // see. Nothing to clean up.
  ],
};

export const CONFIG_HISTORY_SCRIPT: DemoScript = {
  id: "v21-config-history",
  label: "v2.1 — Config history (impact ledger)",
  shortDescription: "Edit an agent, see the change recorded in the History tab",
  steps: [
    {
      kind: "cleanup",
      runtime: "claude",
      agentSlugs: ["history-demo"],
    },
    {
      kind: "createAgent",
      spec: {
        displayName: "history-demo",
        runtime: "claude",
        model: "claude-sonnet-4-6",
        description: "Throwaway agent for the config-history demo.",
        systemPrompt: "Demo agent.",
        goal: "config-history demo",
      },
    },
    { kind: "subtitle", text: "v2.1 — Configuration impact ledger.", durationMs: 2000 },
    { kind: "navigate", section: "agents" },
    { kind: "setSubTab", storageKey: "ato.subtab.agents", tabId: "mine" },
    { kind: "wait", ms: 800 },
    {
      kind: "subtitle",
      text: "Open the agent we just created.",
      durationMs: 1800,
    },
    // Configure button is inside the expanded row body, so we have to
    // click the row to expand it first. Otherwise clickByDemoId
    // silently no-ops because the configure button isn't in the DOM.
    { kind: "clickByDemoId", id: "agent-row-history-demo" },
    { kind: "wait", ms: 600 },
    { kind: "highlight", id: "agent-configure-history-demo", durationMs: 1200 },
    { kind: "clickByDemoId", id: "agent-configure-history-demo" },
    { kind: "wait", ms: 1200 },
    {
      kind: "subtitle",
      text: "Genesis change was recorded automatically. Every model swap, prompt edit, hook add lands here.",
      durationMs: 4200,
    },
    { kind: "highlight", id: "agent-tab-history", durationMs: 1200 },
    { kind: "clickByDemoId", id: "agent-tab-history" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Each row: field changed, old → new diff (expand), actor, timestamp. Pro+ + cloud login required.",
      durationMs: 5000,
    },
    { kind: "wait", ms: 1500 },
    // Cleanup so reruns start fresh + your real agent list isn't
    // polluted with demo cruft.
    {
      kind: "cleanup",
      runtime: "claude",
      agentSlugs: ["history-demo"],
    },
  ],
};

export const PIPELINE_VIEWER_SCRIPT: DemoScript = {
  id: "v21-pipeline-viewer",
  label: "v2.1 — Pipeline trace visualizer",
  shortDescription: "Fire a sequential group, click the pipeline link, see Claude → Codex flow",
  steps: [
    {
      kind: "cleanup",
      runtime: "claude",
      agentSlugs: ["pipe-writer", "pipe-reviewer"],
      groupSlugs: ["pipe-demo"],
    },
    {
      kind: "createAgent",
      spec: {
        displayName: "pipe-writer",
        runtime: "claude",
        model: "claude-sonnet-4-6",
        description: "Writes one short paragraph.",
        systemPrompt: "Write ONE short paragraph on the topic given. No commentary.",
        goal: "pipeline writer",
      },
    },
    {
      kind: "createAgent",
      spec: {
        displayName: "pipe-reviewer",
        runtime: "codex",
        model: "gpt-4.1",
        description: "Reviews the paragraph.",
        systemPrompt: "Review the previous paragraph for clarity. Reply in 2 short sentences.",
        goal: "pipeline reviewer",
      },
    },
    { kind: "subtitle", text: "v2.1 — Pipeline trace visualizer.", durationMs: 2000 },
    // Skip the form-driven group creation. The form save raced the
    // chat pane's group-list cache (selectChatGroup ran before
    // pipe-demo was in the dropdown), and the dispatch ended up
    // going to bare Claude with no agent. Programmatic createGroup
    // bypasses both the form validation AND the cache timing.
    {
      kind: "createGroup",
      spec: {
        displayName: "pipe-demo",
        runtime: "claude",
        description: "Sequential demo: writer → reviewer.",
        dispatchKind: "sequential",
        childSlugs: ["pipe-writer", "pipe-reviewer"],
      },
    },
    { kind: "wait", ms: 800 },
    {
      kind: "subtitle",
      text: "Group seeded. Fire it from chat — both children run, traces share a parent_run_id.",
      durationMs: 3500,
    },
    { kind: "navigate", section: "home" },
    { kind: "setChatPaneOpen", open: true },
    { kind: "newThread" },
    // Select the group BEFORE typing so the dispatch path is locked
    // in. wait long enough for React Query to refresh the group list
    // after createGroup mutated SQLite.
    { kind: "wait", ms: 1500 },
    { kind: "selectChatGroup", slug: "pipe-demo" },
    { kind: "wait", ms: 800 },
    {
      kind: "type",
      text: "Topic: why morning routines matter for software engineers.",
    },
    { kind: "wait", ms: 300 },
    { kind: "send" },
    // Group dispatch (writer + reviewer) takes 15-30s. Wait for both
    // stages to complete + traces to upload. In mock mode, the
    // upload short-circuits — the pipeline visualizer falls back to
    // the canonical fixture parent_id which has 2 mock stages.
    { kind: "wait", ms: 2500 },
    { kind: "navigate", section: "insights" },
    // Pipelines sub-tab — purpose-built for multi-stage dispatches
    // grouped by parent_run_id. Works for ANY agent kind (internal
    // pipe-writer/reviewer included), unlike External which is
    // strict to deployed bundles.
    { kind: "setSubTab", storageKey: "ato.subtab.insights", tabId: "pipelines" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Pipelines: every multi-stage dispatch grouped by parent_run_id.",
      durationMs: 2800,
    },
    { kind: "wait", ms: 600 },
    {
      kind: "subtitle",
      text: "Click the first pipeline to see the per-stage flow.",
      durationMs: 2400,
    },
    { kind: "highlight", id: "pipeline-row-first", durationMs: 1200 },
    { kind: "clickByDemoId", id: "pipeline-row-first" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Numbered stages: 1️⃣ writer → 2️⃣ reviewer with handoff arrows + per-stage timing + files.",
      durationMs: 4500,
    },
    { kind: "wait", ms: 1500 },
    // Cleanup the agents + group so reruns start fresh.
    {
      kind: "cleanup",
      runtime: "claude",
      agentSlugs: ["pipe-writer", "pipe-reviewer"],
      groupSlugs: ["pipe-demo"],
    },
  ],
};

export const EMBED_KEY_SCRIPT: DemoScript = {
  id: "v21-embed-key",
  label: "v2.1 — Deploy bundle embed key",
  shortDescription: "Open Deploy tab on an external agent, reveal ATO_TRACE_KEY",
  steps: [
    {
      kind: "cleanup",
      runtime: "claude",
      agentSlugs: ["embed-demo"],
      apiKeyNames: ["[DEMO] Anthropic"],
    },
    {
      kind: "seedDemoApiKey",
      provider: "anthropic",
      name: "[DEMO] Anthropic",
      value: "sk-ant-demo-DO-NOT-USE-this-is-a-placeholder-key",
    },
    {
      kind: "createAgent",
      spec: {
        displayName: "embed-demo",
        runtime: "claude",
        model: "claude-sonnet-4-6",
        description: "External agent for embed-key demo.",
        systemPrompt: "Customer support agent for {company_name}.",
        goal: "embed key demo",
        kind: "external",
      },
    },
    { kind: "subtitle", text: "v2.1 — Deploy tab + embed key.", durationMs: 2000 },
    { kind: "navigate", section: "agents" },
    { kind: "setSubTab", storageKey: "ato.subtab.agents", tabId: "mine" },
    { kind: "wait", ms: 600 },
    // Configure button is inside the expanded row, so click the row
    // first to open it. Otherwise the configure click is a no-op.
    { kind: "clickByDemoId", id: "agent-row-embed-demo" },
    { kind: "wait", ms: 600 },
    { kind: "highlight", id: "agent-configure-embed-demo", durationMs: 1200 },
    { kind: "clickByDemoId", id: "agent-configure-embed-demo" },
    { kind: "wait", ms: 1200 },
    { kind: "highlight", id: "agent-tab-deploy", durationMs: 1200 },
    { kind: "clickByDemoId", id: "agent-tab-deploy" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Toggling 'Stream traces' surfaces the embed key panel below.",
      durationMs: 3000,
    },
    { kind: "highlight", id: "deploy-forward-traces", durationMs: 1200 },
    { kind: "clickByDemoId", id: "deploy-forward-traces" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Reveal toggles masked → real key.",
      durationMs: 2400,
    },
    { kind: "highlight", id: "embed-key-reveal", durationMs: 1000 },
    { kind: "clickByDemoId", id: "embed-key-reveal" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Copy → paste as ATO_TRACE_KEY env var on the deployed bundle.",
      durationMs: 3200,
    },
    { kind: "highlight", id: "embed-key-copy", durationMs: 1000 },
    { kind: "clickByDemoId", id: "embed-key-copy" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Free or signed-out users see the upgrade hint instead.",
      durationMs: 2800,
    },
    { kind: "wait", ms: 1200 },
    // Cleanup the agent + the [DEMO] Anthropic key so reruns start
    // fresh and your real keystore isn't polluted with demo data.
    {
      kind: "cleanup",
      runtime: "claude",
      agentSlugs: ["embed-demo"],
      apiKeyNames: ["[DEMO] Anthropic"],
    },
  ],
};

export const COMPARE_TRACES_SCRIPT: DemoScript = {
  id: "v21-compare-traces",
  label: "v2.1 — Compare traces (eval workbench)",
  shortDescription: "Fire 2 real dispatches on the same agent, compare them",
  steps: [
    {
      kind: "cleanup",
      runtime: "claude",
      agentSlugs: ["compare-demo"],
    },
    {
      kind: "createAgent",
      spec: {
        // Plain internal agent now — Compare lives in its own sub-tab
        // and is kind-agnostic, so we don't have to dress an internal
        // agent up as external just to satisfy a panel filter.
        displayName: "compare-demo",
        runtime: "claude",
        model: "claude-sonnet-4-6",
        description: "Throwaway agent for the trace-compare demo.",
        systemPrompt: "Reply with one short paragraph on the topic given.",
        goal: "compare-demo",
      },
    },
    { kind: "subtitle", text: "v2.1 — Eval workbench (compare).", durationMs: 2000 },
    {
      kind: "subtitle",
      text: "Need 2 traces of the same agent to compare. Firing both now.",
      durationMs: 2800,
    },
    // Two real dispatches via the chat pane against the same agent
    // → two real traces upload to cloud → drill-down has 2 rows to
    // compare. This takes ~10s per dispatch with claude --print.
    { kind: "navigate", section: "home" },
    { kind: "setChatPaneOpen", open: true },
    { kind: "newThread" },
    { kind: "selectAgent", slug: "compare-demo" },
    { kind: "wait", ms: 800 },
    { kind: "type", text: "Topic: morning routines for software engineers." },
    { kind: "wait", ms: 300 },
    { kind: "send" },
    { kind: "wait", ms: 1500 },
    { kind: "newThread" },
    { kind: "selectAgent", slug: "compare-demo" },
    { kind: "wait", ms: 600 },
    { kind: "type", text: "Topic: evening routines for software engineers." },
    { kind: "wait", ms: 300 },
    { kind: "send" },
    // Wait for both trace uploads to land in cloud (Pro+ users with
    // sign-in only — the trace-upload pipe is best-effort silent for
    // signed-out users; in that case Compare modal will show the
    // baseline trace but no candidates).
    { kind: "wait", ms: 4000 },
    { kind: "navigate", section: "insights" },
    // Compare sub-tab — kind-agnostic eval workbench. Lists agents
    // with ≥2 cloud traces and opens the diff modal directly.
    { kind: "setSubTab", storageKey: "ato.subtab.insights", tabId: "compare" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Compare: any agent with ≥2 cloud traces lands here.",
      durationMs: 2400,
    },
    { kind: "highlight", id: "compare-agent-first", durationMs: 1200 },
    { kind: "clickByDemoId", id: "compare-agent-first" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Modal opens with baseline already filled. Pick a comparison from the candidate list.",
      durationMs: 4000,
    },
    { kind: "highlight", id: "compare-candidate-first", durationMs: 1200 },
    { kind: "clickByDemoId", id: "compare-candidate-first" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Diff: duration delta, cost delta, ok-status change, files only-in-baseline / only-in-comparison.",
      durationMs: 5000,
    },
    { kind: "wait", ms: 1500 },
    // Cleanup so reruns start fresh.
    {
      kind: "cleanup",
      runtime: "claude",
      agentSlugs: ["compare-demo"],
    },
  ],
};

export const INSIGHTS_TOUR_SCRIPT: DemoScript = {
  id: "v21-insights-tour",
  label: "v2.1 — Insights tour (Live → External → Regressions → Cost)",
  shortDescription: "Walk every v2.1 sub-tab in Insights so you can verify each renders",
  steps: [
    { kind: "subtitle", text: "v2.1 — Insights tour. Walking every sub-tab.", durationMs: 2400 },
    { kind: "navigate", section: "insights" },
    { kind: "setSubTab", storageKey: "ato.subtab.insights", tabId: "live" },
    { kind: "wait", ms: 1000 },
    {
      kind: "subtitle",
      text: "Live — in-flight dispatches. Phase 4.",
      durationMs: 2400,
    },
    { kind: "setSubTab", storageKey: "ato.subtab.insights", tabId: "external" },
    { kind: "wait", ms: 1000 },
    {
      kind: "subtitle",
      text: "External — per-agent trace metrics + drill-down with files / pipeline / compare. Phases 3 + 7 + 9.",
      durationMs: 4500,
    },
    { kind: "setSubTab", storageKey: "ato.subtab.insights", tabId: "regressions" },
    { kind: "wait", ms: 1000 },
    {
      kind: "subtitle",
      text: "Regressions — model swap → eval drop, joined automatically. Phase 5.",
      durationMs: 4000,
    },
    { kind: "setSubTab", storageKey: "ato.subtab.insights", tabId: "cost" },
    { kind: "wait", ms: 1000 },
    {
      kind: "subtitle",
      text: "Cost — per-(agent, runtime) cost-per-success with outlier flagging. Phase 8.",
      durationMs: 4000,
    },
    { kind: "wait", ms: 1500 },
  ],
};

export const DEMO_SCRIPTS: DemoScript[] = [
  FULL_TOUR_SCRIPT,
  HERO_SCRIPT,
  SHORT_SCRIPT,
  // v2.1 standalone verification scripts.
  LIVE_RUNS_SCRIPT,
  CONFIG_HISTORY_SCRIPT,
  PIPELINE_VIEWER_SCRIPT,
  EMBED_KEY_SCRIPT,
  COMPARE_TRACES_SCRIPT,
  INSIGHTS_TOUR_SCRIPT,
];
