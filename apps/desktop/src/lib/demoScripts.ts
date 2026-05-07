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

const PERF_SYSTEM_PROMPT = `You are a performance-focused code reviewer. Only flag real performance issues — N+1 queries, unbounded loops, blocking I/O on hot paths, allocations in tight loops.

Be specific. Estimate the cost ("this is N queries where 1 would do"). If you don't see a perf issue, say "No performance issues found" and stop.`;

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
      agentSlugs: ["code-reviewer", "code-writer", "security-reviewer", "perf-reviewer"],
      groupSlugs: ["code-review-team", "write-and-review"],
    },
    // ── Open: Home ───────────────────────────────────────────────────────
    // Collapse the chat pane while we tour sections so each one gets full
    // vertical space — the chat reopens later for the hero workflow.
    { kind: "setChatPaneOpen", open: false },
    { kind: "navigate", section: "home" },
    {
      kind: "subtitle",
      text: "ATO — the GUI for daily agentic work.",
      durationMs: 2400,
    },
    {
      kind: "subtitle",
      text: "One workspace for Claude, Codex, Gemini, OpenClaw, Hermes, Ollama.",
      durationMs: 2800,
    },
    { kind: "wait", ms: 500 },

    // ── Show three creation paths: Templates teaser → Guided chat → Quick
    { kind: "navigate", section: "agents" },
    {
      kind: "subtitle",
      text: "Three ways to create an agent. Let's see all of them.",
      durationMs: 2400,
    },
    { kind: "openWizard", path: "templates" },
    {
      kind: "subtitle",
      text: "Templates — five production-quality starters.",
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
      text: "Now three more — a code writer + two specialized reviewers.",
      durationMs: 2600,
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
    {
      kind: "createAgent",
      spec: {
        displayName: "perf-reviewer",
        runtime: "claude",
        model: "claude-sonnet-4-6",
        description: "Reviews code for performance issues only.",
        systemPrompt: PERF_SYSTEM_PROMPT,
        goal: "Surface performance-only review notes",
      },
    },
    { kind: "wait", ms: 600 },

    // ── Build the group ──────────────────────────────────────────────────
    {
      kind: "subtitle",
      text: "Bundle them into a group. Router decides who handles what.",
      durationMs: 2800,
    },
    // ── Two group types — both created so viewers see both patterns ─────
    {
      kind: "subtitle",
      text: "Two group types: routed (router picks one) and automation (pipeline).",
      durationMs: 3000,
    },
    // 1. Routed group — built BY HAND through the form so the recording
    //    shows the same "watching it being built" UX as the agent quick
    //    form. Open Groups sub-tab, click + New, animate fields, save.
    { kind: "setSubTab", storageKey: "ato.subtab.agents", tabId: "groups" },
    { kind: "wait", ms: 600 },
    { kind: "subtitle", text: "Click + New group.", durationMs: 1800 },
    { kind: "clickByDemoId", id: "group-new" },
    { kind: "wait", ms: 700 },
    {
      kind: "subtitle",
      text: "Name → type → children → routing rule → save.",
      durationMs: 2400,
    },
    {
      kind: "autoFillGroupForm",
      spec: {
        displayName: "code-review-team",
        runtime: "claude",
        description: "Routed: router picks one specialist per prompt.",
        dispatchKind: "routed",
        childSlugs: ["perf-reviewer", "code-reviewer"],
        routerRule: {
          keywords: ["performance", "perf", "slow", "N+1", "latency", "hot path"],
          thenSlug: "perf-reviewer",
        },
      },
    },
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
    // 2. Sequential automation: one prompt fires the whole pipeline.
    //    Writer creates code → security reviewer reviews it → done.
    {
      kind: "createGroup",
      spec: {
        displayName: "write-and-review",
        runtime: "claude",
        description: "Automation: writer → security review, all from one prompt.",
        dispatchKind: "sequential",
        childSlugs: ["code-writer", "security-reviewer"],
      },
    },
    { kind: "wait", ms: 600 },

    // ── Show the group: Agents → Groups → open code-review-team in Graph
    { kind: "setSubTab", storageKey: "ato.subtab.agents", tabId: "groups" },
    { kind: "wait", ms: 1000 },
    {
      kind: "subtitle",
      text: "Three specialists. One router. Visible in the Groups tab.",
      durationMs: 2400,
    },
    { kind: "wait", ms: 800 },
    { kind: "selectGroup", slug: "code-review-team" },
    { kind: "wait", ms: 1500 },
    {
      kind: "subtitle",
      text: "Form view shows the rules. Switch to Graph to see the routing.",
      durationMs: 2800,
    },
    { kind: "wait", ms: 1500 },
    // Reset Agents sub-tab back to "mine" so the next phase shows the
    // populated agent list rather than the group editor.
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
    {
      kind: "subtitle",
      text: "Now swap to Codex mid-thread — the full history travels.",
      durationMs: 2400,
    },
    { kind: "selectChatGroup", slug: null },
    { kind: "selectAgent", slug: null },
    { kind: "setRuntime", runtime: "codex" },
    { kind: "wait", ms: 500 },
    {
      kind: "type",
      text: "Summarize what we just did, including the security issue Claude flagged.",
    },
    { kind: "wait", ms: 300 },
    { kind: "send" },
    { kind: "wait", ms: 800 },
    {
      kind: "subtitle",
      text: "Codex remembers what Claude wrote, what the reviewer flagged. One thread.",
      durationMs: 3500,
    },
    { kind: "wait", ms: 800 },

    // ── Schedule it ──────────────────────────────────────────────────────
    //    Show that the workflow can also run on a cron — same agent, no
    //    babysitting. Seed the cron job programmatically and navigate to
    //    Runs → Schedules so it appears in the list.
    {
      kind: "subtitle",
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
      text: "Wake-from-sleep on macOS, Linux, and Windows — jobs fire even when ATO is closed.",
      durationMs: 3600,
    },
    { kind: "wait", ms: 1200 },

    // ── Close ────────────────────────────────────────────────────────────
    {
      kind: "subtitle",
      text: "Build the agents. Bundle them into a workflow. Run on demand or on a schedule.",
      durationMs: 3400,
    },
    {
      kind: "subtitle",
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

export const DEMO_SCRIPTS: DemoScript[] = [FULL_TOUR_SCRIPT, HERO_SCRIPT, SHORT_SCRIPT];
