# ATO - Product Requirements Document

## Vision Statement

ATO (Agentic Tool Optimization) is **the GUI for creating, managing, and observing AI agents** — across every runtime (Claude Code, Codex / OpenAI Agents SDK, Gemini CLI / ADK, OpenClaw, Hermes) and every model provider (Anthropic, OpenAI, Google, Ollama). One desktop app that takes a user from "I want an agent that does X" to a working, running agent in under two minutes.

**The "full GUI" promise**: anything you can do with an AI coding agent on the command line, you can do here — create it, configure it, run it, watch it work, debug it. Without leaving the app, and without editing JSON by hand.

### Target Users

ATO serves three audiences with one product, via two front-doors that share the same primitives:

1. **Non-technical / first-agent users ("normies")** — Have heard of AI agents, may have installed Claude Code or ChatGPT desktop, want a real agent for a real job (review my PRs, summarize my emails, watch my server logs). Need a chat-style guided wizard, sane defaults, and zero JSON.
2. **Power users / multi-agent developers** — Already running 2-5 agents across runtimes. Need fast switching, a real terminal, command palette, and bulk operations. Get the Quick (form) onboarding and ⌘K.
3. **Teams / enterprise** — Multiple developers, shared skill libraries, audit trail, SSO, cross-runtime policies. Pro/Team tier features in `ato-cloud`.

**Business Model**: Open-core. MIT-licensed desktop app + optional Pro subscription for real-time monitoring, cloud sync, hosted suggestions, and team features (closed source, separate repo `ato-cloud`).

**The locked principle (2026-05-25)**: *customers can run primitives free; we charge for the codified automation we package on top.* Same model as GitLab, Sentry, Supabase. You can write your own bash loop calling `ato dispatch`, set up your own launchd plist, hand-prompt your own diagnose LLM — we don't lock that path. We charge for the one-click button that codifies our methodology + the safety net we wrap around it (holdouts, statistical guarantees, automatic rollback, cross-device sync). See [`docs/tiers.md`](./docs/tiers.md) for the full Free / Pro / Team / Enterprise inventory.

> Note: "ATO" is the short name; "Agentic Tool Optimization" is the full name.

---

## Information Architecture

The desktop app is organized into **6 top-level sections** in the sidebar (collapsed from 24 in v1.2.x). Every screen lives under one of these.

| # | Section | Purpose | Sub-tabs / panels |
|---|---|---|---|
| 1 | **Home** | Landing page; create new agents; see what's happening | Create Agent CTA, Recent Agents, Recent Runs, Alerts |
| 2 | **Agents** | List, create, configure, and inspect agents | Agents list, Agent detail (Config, Skills, MCPs, Permissions), + New Agent |
| 3 | **Skills & MCPs** | Manage capabilities — skills (SKILL.md) and MCP servers | Skills, MCPs, Marketplace |
| 4 | **Runs** | Everything execution-related | Live (active sessions), History (logs), Schedules (cron), Automations (workflows), Hooks |
| 5 | **Insights** | Health, costs, audit, context | Health, Analytics, Context, Audit Log |
| 6 | **Settings** | Configuration | Runtimes, Models, API Keys, Secrets, Env, Cloud (auth/sync/teams/notifications), Projects |

**Persistent surfaces** (not in the sidebar):
- **Command palette (⌘K)** — search any action, page, agent, skill, project.
- **Chat / Terminal pane** — bottom of the screen, expandable. Two modes: chat (send to runtime) or terminal (full xterm shell scoped to active project CWD).
- **Project switcher** — sticky at the top of the sidebar.

---

## Architecture

### Desktop-First, Multi-Runtime

```
ATO/
├── apps/
│   └── desktop/                # Tauri 2.x (Rust + React) — offline-first
│       ├── src-tauri/          # Rust backend: SQLite, multi-runtime CLI dispatch,
│       │                       #   file watcher, skill scanning, context estimation
│       └── src/                # React frontend: dashboard UI
│           ├── components/     # UI components (Skills, Cron, Automation, etc.)
│           │   ├── automation/ # Visual workflow builder
│           │   └── cron/       # Cron monitoring + calendar
│           ├── stores/         # Zustand state (automation, cron)
│           ├── lib/            # API layer, cron utils, skill-to-workflow parser
│           └── i18n/           # EN, PT, ES translations
├── services/
│   └── mcp-server/             # Standalone MCP server (stdio) — 8 tools
├── packages/
│   ├── core/                   # Shared types, token utils, config paths (no I/O)
│   └── db/                     # Database abstraction (SQLite for desktop)
└── .github/workflows/          # CI: multi-platform release builds
```

### Data Flow

1. **Desktop (default, offline)**: Tauri reads config files from all runtime directories (`~/.claude/`, `~/.codex/`, `~/.openclaw/`, `~/.hermes/`) → stores in local SQLite → renders in React UI
2. **CLI dispatch**: User actions route through `promptAgent(runtime, prompt)` → dispatches to correct CLI (`claude --print`, `codex --print`, SSH `openclaw exec`, `hermes --execute`)
3. **Inbound status**: `queryAgentStatus(runtime)` checks health, version, auth for each runtime
4. **Execution logging**: All agent calls auto-logged to `~/.ato/agent-logs.jsonl`
5. **MCP bridge**: Standalone MCP server exposes ATO data to any MCP client

### Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop Runtime | Tauri 2.x (Rust backend, ~5MB installer) |
| Desktop DB | SQLite via rusqlite (local, offline) |
| Desktop Frontend | React 18 + Vite + TailwindCSS |
| State Management | Zustand |
| Charts | Recharts |
| MCP Server | @modelcontextprotocol/sdk (stdio transport) |
| i18n | react-i18next (English, Portuguese, Spanish) |
| CI/CD | GitHub Actions (4-platform builds) |
| Data Fetching | TanStack React Query |

---

## Supported Runtimes

| Runtime | Provider | Outbound | Inbound | Skill Dirs | Config |
|---------|----------|----------|---------|------------|--------|
| **Claude** | Anthropic | `claude --print` | MCP + auth check | `~/.claude/skills/`, `.claude/skills/` | `~/.claude/settings.json`, `CLAUDE.md` |
| **Codex** | OpenAI | `codex --print` | version + API key | `~/.codex/skills/`, `.agents/skills/` | `~/.codex/config.toml`, `AGENTS.md` |
| **OpenClaw** | OpenClaw | SSH `exec` | SSH version + status | `~/.openclaw/skills/`, workspace | `~/.openclaw/openclaw.json`, `SOUL.md` |
| **Hermes** | NousResearch | `hermes --execute` | version + endpoint | `~/.hermes/skills/` (categories) | `~/.hermes/config.yaml`, `SOUL.md` |

### Runtime Detection

ATO auto-detects installed CLIs by searching:
1. User-configured path override (`~/.ato/{runtime}-path`)
2. Common install paths (`/usr/local/bin`, `/opt/homebrew/bin`, `~/.npm-global/bin`, etc.)
3. npx cache (`~/.npm/_npx/**/node_modules/.bin/`)
4. User's full shell PATH (spawns login shell to get real PATH)

Falls back to manual path input in Setup Wizard when auto-detect fails.

---

## Security

- **Local-first**: No network calls unless cloud sync explicitly enabled
- **Parameterized SQL**: All queries use parameterized statements
- **Input validation**: Zod schemas on all boundaries
- **SSH for OpenClaw**: Key-based auth (paths only, no key contents stored)
- **No secrets in repo**: .env files gitignored, no hardcoded credentials
- **Shell PATH isolation**: Tauri apps get user's full PATH via login shell

---

## Feature Specifications

### F1: Home + Create Agent (v1.3.0)

**Home page** is the new landing destination after splash. Replaces the old SetupWizard as the front door. Contains:

- Single primary **"Create Agent"** CTA
- **Recent Agents** carousel (most-recently-used)
- **Recent Runs** strip (last 5 executions across all agents)
- **Alerts** card (failed crons, runtime offline, token-budget hits)

If no runtime is connected, Home prompts the user to connect one via the new Settings → Runtimes flow (which replaces the old SetupWizard's runtime-detection step).

**Create Agent** is a full-page wizard with two tab-toggled paths:

#### Path A — Guided (chat, default)
Single-pane chat. Each turn either asks one question or offers a card to confirm. State persisted as a draft (`~/.ato/agent-drafts/<id>.json`).

1. *"What do you want your agent to help with?"* (free text)
2. AI proposes runtime + model + 0–3 skills + 0–3 MCPs as confirmable cards.
3. *"Where will this agent live?"* (project picker)
4. Confirm → ATO writes agent files to disk + creates SQLite record + opens `Agents → [new agent]` with "Open in Terminal" + "Run a test" buttons.

The "AI proposes a stack" step uses the user's own runtime as the suggestion engine (resolution order):
- Active CLI subscription (`claude --print`, `codex --print`, `gemini -p`) — VS-Code-style, zero key setup
- Active API key (`LlmApiKeys`) — direct provider SDK
- Local Ollama
- Pro-tier hosted `/agent-suggest` (fallback)

#### Path B — Quick (form, default for power users)
One scrollable form: name, runtime, model, project, skills (multi-select), MCPs (multi-select), system prompt (CodeMirror), permissions. Single "Create" button. Both paths land at the same agent record.

#### File-writing contract per runtime
| Runtime | Agent files written |
|---|---|
| Claude Code | `~/.claude/agents/<slug>.md` (or `<project>/.claude/agents/<slug>.md` for project scope) |
| Codex / OpenAI Agents SDK | `~/.codex/agents/<slug>/` directory + `AGENTS.md` |
| Gemini CLI / ADK | `<project>/.gemini/agents/<slug>.yaml` (root_agent.yaml entry) |
| OpenClaw | `~/.openclaw/agents/<slug>/SOUL.md` + `TOOLS.md` |
| Hermes | `~/.hermes/agents/<slug>/` |

All writes go through the existing safety pipeline (hash check, auto-backup, audit log).

### F2: Skills Manager

Per-runtime skill management:
- **Runtime filter tabs**: All / Claude / Codex / OpenClaw / Hermes (with skill counts)
- **Scope hierarchy**: Enterprise > Personal > Project > Plugin (Claude), Personal > Project (Codex/OpenClaw), Personal (Hermes)
- **Recursive scanning**: Handles nested directories (gstack-style `skills/gstack/qa/SKILL.md`)
- **Drag-and-drop priority**: Reorder skills within scope groups (persisted to localStorage)
- **Conflict detection**: Keyword overlap analysis (Jaccard similarity >30%)
- **Frontmatter editor**: All Claude Code fields (`user-invocable`, `allowed-tools`, `model`, `context: fork`)
- **Create/Edit/Delete**: Writes to correct runtime directory based on selected runtime + scope
- **AI-powered creation**: "Generate with AI" section — describe what you want, pick runtime, AI writes SKILL.md
- **In-app approval dialog**: When agent generates a skill file, shows yellow approval banner with preview, scope selector, "Approve & Save" button
- **Auto-improve**: Send skill to its own runtime for rewrite, diff preview, Apply/Discard
- **Share/Publish**: Private sharing via link, publish to marketplace

### F3: Skills Marketplace

Community skill catalog:
- 9 categories: library-reference, product-verification, data-fetching, business-process, code-scaffolding, code-quality, ci-cd, runbooks, infra-ops
- Search, category filter, install counts, ratings
- One-click install to `~/.{runtime}/skills/`
- Publish your skills with category/tags metadata

### F4: Context Visualizer

Per-runtime context breakdown:
- **Runtime tabs**: Claude / Codex / OpenClaw / Hermes
- **"Not connected"** state for uninstalled runtimes
- **Always-loaded items** counted in total: system prompts, CLAUDE.md/AGENTS.md/SOUL.md, MCP schemas, conversation
- **Skills shown as "on-demand"**: NOT counted in total (loaded only when triggered)
- **Color warnings** at 75% (yellow) and 90% (red)
- **Dependencies tab**: Runtime-specific file paths and token counts
- **Permissions tab**: Runtime-specific tool permissions (Claude: Read/Write/Bash/etc., Codex: shell/file_read/etc.)

### F5: Automation Builder

Visual workflow editor with auto-detection:
- **Auto-generates flows from skill content**: Parses `## Step N:` and `## Phase N:` headers from SKILL.md files (works with gstack, custom skills, any skill pack)
- **Per-node runtime selection**: Mix Claude + Codex in the same workflow
- **Service integrations**: GitHub, Slack, Gmail, Postgres, Notion, Linear
- **Decision nodes**: Conditional branching
- **Prompt serialization**: Converts workflow to structured prompt with `@runtime` per step
- **Run button**: Dispatches to correct runtime via `promptAgent()`

### F6: Subagents Manager

Create and manage subagents with runtime selection:
- **Runtime selector**: 4 colored buttons (Claude/Codex/OpenClaw/Hermes)
- **Runtime-specific config**: OpenClaw SSH fields, Codex API key, Hermes endpoint
- **RuntimeBadge** on cards for quick identification
- Assign skills, allowed tools, model override, custom instructions

### F7: Cron Monitor

Scheduled agent job management:
- **List view**: Job cards with schedule, runtime badge, status, 7-day timeline
- **Calendar view**: Google Calendar-style monthly grid with color-coded execution status
  - Green = success (click to see output)
  - Red = failed (click to see error details)
  - Gray = scheduled for future
- **Create/Edit**: Cron expression with live validation + human-readable preview
- **Manual trigger**: "Run Now" button
- **Auto-retry**: Retry failed executions
- **Smart failure detection**: Silent failures, chronic warnings, alert dedup
- **Alert banner**: Unacknowledged alerts shown at top with dismiss button
- **Sidebar badge**: Red pulse dot when alerts exist

### F8: Hooks Manager

Shell commands on Claude Code events:
- Events: PreToolUse, PostToolUse, Notification, Stop, SubagentStop
- Configure: command, matcher (regex/exact), timeout, scope (global/project)
- Starts clean (no mock data)

### F9a: ATO MCP Server (we expose ATO as an MCP)

Standalone MCP server with 8 tools — this is what we *publish* so other agents can read ATO state:
- `get_context_usage` — Context window breakdown
- `list_skills` / `toggle_skill` — Manage skills
- `get_usage_stats` — Token/cost analytics from JSONL logs
- `get_mcp_status` — MCP server configuration
- `get_runtime_status` — Health check for any runtime
- `get_all_runtime_statuses` — Health check all runtimes at once
- `get_agent_logs` — Execution logs (filterable by runtime)

### F9b: MCP Manager (we install MCPs into the user's agents) — v1.3.0

The other half of MCP. `Skills & MCPs → MCPs` tab lets the user install and manage MCP servers that *their* agents will consume.

- **Registry browser** — curated list (filesystem, github, postgres, slack, brave-search, gmail, calendar, etc.) sourced from `GET /mcp-registry` on `ato-cloud`. Search, category filter, install counts.
- **One-click install** — writes the MCP entry into the active runtime's MCP config (`.mcp.json` for Claude, `codex.json` for Codex, etc.) AND runs the install command in the embedded terminal so the user can see what happened.
- **Custom install** — manual form: name, command, args, env vars (stdio) or URL (SSE/HTTP).
- **Per-agent enable/disable** — toggle which installed MCPs are exposed to which agent.
- **Tool discovery** — for each running MCP, show available tools (existing `discoverMcpServerTools` in `McpDashboard`).

### F10: Prompt Bar

Persistent chat input at bottom of every page:
- **Runtime selector dropdown**: Claude / Codex / OpenClaw / Hermes
- **Stateless execution**: Each call uses `--print` mode (no session persistence)
- **Skill detection**: When response contains SKILL.md content, shows approval dialog
- **Auto-instruction**: Skill requests prepend "return content only, no file writes"
- **Response shows runtime badge**: Which agent responded
- **Send button color** matches selected runtime

### F11: Configuration

Unified view of config files across all runtimes:
- Claude: settings.json, settings.local.json, CLAUDE.md, skills/
- Codex: config.toml, AGENTS.md, skills/
- OpenClaw: openclaw.json, AGENTS.md, SOUL.md, TOOLS.md, skills/
- Hermes: config.yaml, SOUL.md, skills/, memories/
- Shows exists/missing status for each file

### F12: Usage Analytics

Token consumption tracking:
- Today/week/month summary cards
- Burn rate (tokens/hour, cost/hour)
- 30-day usage chart
- Parses Claude Code JSONL logs

### F13: i18n

Full internationalization:
- English (en), Portuguese (pt), Spanish (es)
- All UI strings via react-i18next
- Language switcher in sidebar

### F14: Embedded Terminal (v1.3.0)

Replaces today's chat-only `PromptBar` with an expandable bottom pane that has two modes (toggle in the header):

- **Chat mode** — current behavior (send to runtime via `promptAgent()`, show approval dialog for SKILL.md content).
- **Terminal mode** — full xterm.js shell, scoped to the active project's CWD, inheriting the user's PATH (Tauri login-shell PATH spawn). The chat agent can stream commands here when it wants to "show its work."

**Stack**:
- Frontend: `xterm.js` + `@xterm/addon-fit` + `@xterm/addon-web-links`
- Backend: `portable-pty` Rust crate, spawned via Tauri command. Default shell: `$SHELL` on Unix, `pwsh.exe` on Windows.
- Persistent across page navigation; collapsible; resizable.

**Why it matters for positioning**: this is the single feature that lets us tell the same story to both audiences. Normies see "the agent did this thing in a terminal." Tech people see "an actual shell I can use."

---

## Design System

- **Theme**: Dark (#0a0a0f background, #16161e cards, #2a2a3a borders)
- **Accent**: Cyan/mint (#00FFB2) for primary actions
- **Runtime colors**: Claude (#f97316 orange), Codex (#22c55e green), OpenClaw (#06b6d4 cyan), Hermes (#a855f7 purple)
- **Typography**: System font for UI, monospace for code/paths
- **Status**: Green=healthy, Red=failed, Yellow=warning, Gray=paused

---

## Deployment

### Desktop
- Tauri builds for macOS (Apple Silicon + Intel), Windows, Linux
- GitHub Actions CI: auto-builds on version tag push
- `npm run dev:desktop` for development
- `npx tauri build` for production

### MCP Server
- `npm run dev:mcp` for development
- `npx ato-mcp` for standalone use

---

## Data Storage

| Data | Location |
|------|----------|
| Skills (Claude) | `~/.claude/skills/`, `.claude/skills/` |
| Skills (Codex) | `~/.codex/skills/`, `.agents/skills/` |
| Skills (OpenClaw) | `~/.openclaw/skills/` |
| Skills (Hermes) | `~/.hermes/skills/` |
| Workflows | `~/.ato/workflows/*.json` |
| Cron jobs | `~/.ato/cron-jobs.json` |
| Agent logs | `~/.ato/agent-logs.jsonl` |
| Runtime paths | `~/.ato/{runtime}-path` |
| Database | `~/.ato/local.db` (SQLite) |
| Setup state | `localStorage (ato-setup)` |

---

## Open Source vs Pro

### Open Source (MIT, this repo)
All platform features: Skills Manager, Marketplace, Multi-Agent Runtime, Subagents, Automation Builder, Cron Scheduling, Context Visualizer, Hooks, MCP Server, Config, i18n, Setup Wizard, Prompt Bar.

### Pro (Closed Source, Paid)
- Real-time cron health monitoring dashboard
- Silent failure detection + push notifications
- Usage analytics across all runtimes (cloud aggregation)
- Cloud sync (skills, workflows, cron jobs across machines)
- Team workspaces with access controls
- SLA tracking for scheduled jobs
- Slack/email alert integration

---

## Success Metrics

### v0.3.0 Criteria (Achieved)
- [x] Multi-LLM platform (Claude, Codex, OpenClaw, Hermes)
- [x] Two-way communication with all runtimes
- [x] Skills Marketplace with AI-powered creation
- [x] Cron Monitor with calendar view
- [x] Auto-detect automation flows from skill content
- [x] Per-runtime context visualization
- [x] Setup wizard for first-time configuration
- [x] In-app approval dialog for file writes
- [x] No mock data in production
- [x] gstack compatibility (recursive skill scanning)
- [x] GitHub Actions multi-platform builds
- [x] i18n (EN, PT, ES)
