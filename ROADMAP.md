# ATO Roadmap

## Released

### v0.3.0 — Multi-LLM Platform
- Multi-runtime support: Claude Code, Codex, OpenClaw, Hermes
- Two-way communication with all runtimes (send prompts + get status)
- Skills Manager with per-runtime tabs and recursive scanning
- Skills Marketplace (browse, install, publish, share, auto-improve)
- AI-powered skill creation with in-app approval dialog
- Automation Builder with auto-detection from skill content
- Cron Monitor with Google Calendar view and click-to-inspect
- Per-runtime Context Visualizer (skills shown as on-demand)
- Setup Wizard for first-time runtime configuration
- Subagents Manager with runtime selection
- MCP server with 8 tools including runtime status
- GitHub Actions CI for macOS, Windows, Linux
- i18n: English, Portuguese, Spanish

### v0.4.0 — Monitoring & Analytics
- Real-time log viewer with file watcher
- Background health polling for all runtimes
- Usage analytics dashboard with execution metrics
- Latency/uptime charts per runtime
- Cost tracking per runtime with burn rate visualization

### v0.5.0 — Cloud Sync & Collaboration
- Cloud backend (ato-cloud) with PostgreSQL
- GitHub OAuth login
- Team workspaces with shared skill libraries
- Team member management (invite, roles, permissions)
- Team skills sharing and collaboration
- Activity logs for audit trail
- Skill sync across devices

### v0.6.0 — Deeper Runtime Integration
- Live context tracking from runtime session logs (reads Claude session JSONL)
- Real MCP tool discovery (JSON-RPC protocol to running MCP servers)
- Config editor with write support (FileViewer with save functionality)
- Hooks read/write from actual settings files (HooksManager + Tauri commands)

### v0.7.0 — Marketplace Backend
- Marketplace service with PostgreSQL schema
- Skill submissions with versioning (semver)
- Search, filter, and discovery endpoints
- Ratings and reviews with helpfulness voting
- Skill packs (collections) with import/export as JSON
- Update notifications for installed skills

### v0.8.0 — Advanced Automation
- Webhook triggers (inbound) with path/method/secret configuration
- Parallel node execution with group tracking
- Error handling nodes (try-catch, retry with exponential backoff)
- Variables and data passing between nodes (set, get, transform, jq expressions)
- Workflow templates (4 built-in: Webhook to Slack, Parallel Deploy, Error Handling, Data Transform)
- New node types: parallel, try-catch, retry, variable, template
- Enhanced execution state with runId, trigger payload, parallel groups, retry tracking

### v0.5.5 — Notifications & Integrations
- Notifications service with provider abstraction (Tauri backend)
- Slack webhook integration (Block Kit formatting)
- Discord webhook integration (embed support)
- Telegram bot integration (Markdown formatting)
- Email notifications (SMTP - placeholder, requires lettre crate)
- Notification preferences per event type (8 event types)
- Desktop UI for managing notification channels (existing component, now connected to backend)
- SQLite persistence for channel configurations
- Test notification functionality

---

### v1.0.0 — Production Ready (Released April 2026)
- SDK (`@ato-sdk/js`), web dashboard, cost tracking
- LLM API key management, audit logging, agent monitor
- SSO, rate limiting, Homebrew tap

### v1.1.0 — Projects Dashboard + Multi-Runtime (Released April 2026)
- Projects Dashboard with 7 Claude sections + multi-runtime switcher
- 6 runtimes: Claude Code, Codex/OpenAI Agents SDK, Gemini CLI/ADK, OpenClaw, Hermes
- Ollama provider: auto-detect, model picker, copy endpoint
- CodeMirror 6 editor with conflict detection, auto-backup, audit logging
- Sandbox config + approval policies (editable with write-back)
- File watcher, token chart, backup/restore, i18n (EN/PT/ES)
- 46 tests (35 Rust + 11 frontend), CI/CD, code splitting

### v1.2.0 — Agent Command Center (In Progress)
- Visual workspace canvas: drag nodes, zoom in/out, pan
- Live execution visualization: agent activity pulses nodes, animated edge dots
- Skill palette: drag-to-install from marketplace with suggestions
- Command palette (⌘K): search nodes, skills, actions
- Multi-select batch operations on skill nodes
- Grid + Canvas dual view mode
- Strategy game-inspired UX: semantic zoom, animated transitions

---

### v1.3.0 — The GUI Pivot (Released May 2026)
**Goal: become the place where you create an agent, not just manage one.** Repositioning ATO from "multi-runtime control panel" to "the GUI for creating, managing, and observing AI agents" — for non-technical users, power users, and teams alike.

- **IA collapse: 24 sidebar entries → 6 sections** (`Home`, `Agents`, `Skills & MCPs`, `Runs`, `Insights`, `Settings`)
- **New `Home` page** with "Create Agent" CTA, recent agents, recent runs, alerts
- **Create Agent — Guided (chat path)**: multi-turn LLM-driven wizard with required questions (domain, tone/style, filesystem scope, permissions, optional skills); draft persistence; runtime/model/skill/MCP suggestion cards; conversation runs on the user-picked runtime
- **Create Agent — Quick (form path)**: one-page form with all fields visible (project picker, skills/MCPs multi-select, draft auto-save)
- Agent record in SQLite + file-writing for `~/.claude/agents/`, `~/.codex/agents/`, `~/.gemini/agents/`, `~/.openclaw/agents/`, `~/.hermes/agents/`
- **MCP install UI** in `McpDashboard`: registry browser + custom install + path-scoping picker for filesystem MCP (folders the agent can access, with a native folder picker)
- **Embedded terminal**: full xterm.js + portable-pty shell — Chat / Shell modes
- **Subscriptions OR API keys** — first-class auth dual: VS-Code-style detection of `claude` / `codex` / `gemini` CLI logins, OR stored API keys, user's choice per runtime
- **Run loop** (the F5 of agents): per-runtime invocation matrix; "Run" button on every agent card opens an interactive shell scoped to that agent (real persistent session, full memory); "Quick test" dialog for stateless single-shot
- **Cross-runtime dispatch via MCP**: ATO's MCP server now exposes `list_agents` + `run_agent` tools, so any runtime configured with `ato` MCP can natively invoke any ATO-managed agent regardless of which runtime owns it. Resolves the "Codex/Gemini @-mention isn't native" caveat at the protocol layer. Cross-runtime calls auto-log to `~/.ato/agent-logs.jsonl`.
- **Runtime parity matrix** on Customize Overview — honest table of what each runtime supports for create/install/run, with notes about the limits we can't paper over
- Merge Configuration + Runtime Settings → `Settings → Runtimes`
- Fold Cloud Sync / Teams / Skill Sync / Notifications under `Settings → Cloud`
- Demote Workspace canvas to a sub-view of `Settings → Projects`
- Command palette (⌘K) — promoted from v1.2.0
- i18n strings for all new copy (EN/PT/ES)

### v1.4.0 — Production-Grade Agent Authoring (Released May 2026)
**Goal: turn ATO from "agents-as-static-files" into "agents-as-context-engineered-systems."** Driven by industry consensus on what makes production agents survive vs. demos: dynamic context, specialization, observability. This is also where Free / Pro / Team / Enterprise tier gating becomes visible across the product.

**The seven context-engineering primitives:**
- **F1. Dynamic prompts with variables** — `{var}` syntax with resolvers (static / env / project-path on Free; file / db-query / mcp-call / computed on Pro). Variables tab on every agent.
- **F2. Pre-call context hooks** *(Pro)* — ordered list of resolvers that fire before each turn and inject results into a `<context>` block in the user message. CRM / DB / file / webhook / MCP-call / computed.
- **F3. Conversation summarizers** — per-agent Memory tab. Summarize-when-N + keep-last-K + summarizer-model. Free has fixed defaults; Pro is tunable.
- **F4. Multi-agent groups (router + children)** — first-class object: a router + N specialized child agents (Free up to 3 children; Pro unlimited). Visual graph editor reusing the AutomationFlow canvas patterns. Routers support rules + LLM-classifier + hybrid mode. MCP `run_agent` transparently dispatches group slugs through the router.
- **F5. Per-task model selection** *(Pro)* — agents gain `roleModels: { router, summarizer, response, evaluator }` so cheap-fast models handle routing/classification while advanced models handle the response.
- **F6. Tracing + observability dashboard** — Insights → Agent observability: success rate, latency, token cost, last 100 runs (Free); 30/90/∞-day cloud retention (Pro/Team/Enterprise). Trace explorer shows the full sequence (variables → hooks → router → child → response → tool calls).
- **F7. Evaluators** *(Pro)* — manual + scheduled batch only (never live). Heuristic (substring/regex/length/tool-was-called) + LLM-as-judge. Quality scores show in the dashboard.
- **F8. Tool description quality** — "Improve description for this agent" button uses the agent's runtime to rewrite MCP descriptions in context of the agent's actual goal.

**Plus the original v1.4 polish items, all shipping in the same release:**
- Agent templates / blueprints (5 starters: PR reviewer, doc writer, codebase explainer, data analyst, devops helper)
- Skill version history + rollback (DB schema bump)
- Global search across agents / skills / projects / secrets / audit (powers ⌘K)
- Configuration export/backup (.zip of all configs + restore flow)
- Runtime comparison surface (lift the buried `RuntimeComparisonModal` to `Settings → Runtimes → Compare`)
- Bulk skill operations (multi-select enable/disable)

**Tier gating UX:** Pro features are visible to Free users with a small crown lock badge + "Upgrade to Pro" tooltip — Linear / Notion / Figma pattern. Discovery sells; hidden features can't drive upgrades.

**Cloud-side pairing (in `ato-cloud`):**
- Migration `008_v1_4_0_observability.sql` — `agent_traces`, `agent_evaluations`, `agent_groups` (synced).
- New route `POST /agent-traces` (`requireTier('pro')`) — receives traces, persists, computes aggregates.
- Tier checks expand on existing `requireTier` middleware.

**Detailed ticket-by-ticket build plan**: see `docs/V1.4.0-IMPLEMENTATION.md`.

### v1.5.0 — Daily Workspace (Released May 2026)
**Goal: turn ATO from "control panel for agents" into "the place where you do agentic work."** The pivot from configuration GUI to daily workspace.

- **Persistent chat threads** — SQLite-backed `chat_threads` + `chat_messages` tables; conversations survive restart, listed in a dropdown with msg count + last activity, scoped optionally to active project, rename via double-click, delete via hover trash
- **Multi-runtime mid-thread** — switch Claude → Codex → Gemini in the same conversation. Full thread history travels to whichever runtime answers next, regardless of agent selection
- **Streaming responses** — `prompt_agent_stream` / `prompt_agent_with_history_stream` Rust commands stream stdout via `tokio::process::Command` + `tauri::ipc::Channel<StreamEvent>`. Tokens appear live with a blinking cyan caret
- **Syntax-highlighted markdown** — `react-markdown` + `remark-gfm` + `rehype-highlight`; assistant messages render as proper markdown (headings, GFM tables, fenced code blocks with hover-revealed Copy button), user/error/attachment stay raw
- **File attachments** — paperclip pick or drag-drop a text file (≤32KB, binary refused); contents wrap in `<attachment>` block and join history
- **Per-thread sticky agent** — picking an agent persists it to the thread's `agent_id`; switching threads restores the agent
- **Runtime mid-thread for no-agent path** — frontend stitches thread history into a single framed prompt so cross-runtime swaps without an agent still carry context

### v1.6.0 — Intelligence Layer (Planned)
- **Automations tab repurpose — group pipelines as flow nodes** ([detailed plan](docs/V1.6.0-AUTOMATIONS-REPURPOSE.md))
  - Today the Runs → Automations tab visualizes skill-derived flow charts (parsed from `## Step N` / `## Phase N` headers in SKILL.md files). Useful but narrow — and v1.5 groups now own the word "automation."
  - v1.6 turns it into the canonical visualization for **everything that runs without a human in the loop**: routed groups, sequential pipelines, scheduled cron jobs, hooks, and skill flows — all on the same canvas. Each node is a real agent / runtime / tool with live status (idle / running / errored).
  - Sequential group "Claude → Codex" becomes a left-to-right flow with arrows showing data flow; routed group becomes a fan-out from the router node; cron jobs anchor at the left edge with a clock icon.
  - Click a node → Insights opens that agent's trace explorer for the last N runs.
- Real-time collaborative workspace (WebSocket via ato-cloud)
- Team cursors (Figma-style)
- Cross-runtime policy enforcement templates
- Hosted terminal sessions for Team tier (cloud)
- Proactive suggestions ("Your project is missing X")
- Cost optimization alerts from SDK traces
- Agent performance benchmarking across runtimes
- **HALO integration** — feed traces from `~/.ato/agent-logs.jsonl` into Context Labs' HALO RLM engine (MIT, on PyPI), surface harness-improvement reports as one-click inline diffs

### v1.7.0+ — Future
- Cron-driven evaluator scheduling
- `mcp-call` variable / hook resolver (embedded MCP client)
- Trace-retention enforcement on cloud (Pro=30d / Team=90d / Enterprise=∞)
- Search across persistent threads
- Mobile companion (read-only)

---

## Future Runtime Support

As new AI coding agents emerge:
- Cursor
- Windsurf / Codeium
- Aider
- Continue.dev
- Custom agents via plugin API
