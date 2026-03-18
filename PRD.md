# ATO - Product Requirements Document

## Vision Statement

ATO (Agentic Tool Optimization) is the **multi-LLM control panel** for AI coding tools. One desktop dashboard to manage **Claude Code**, **Codex**, **OpenClaw**, and **Hermes** — skills, subagents, automation workflows, cron scheduling, and context visualization across all runtimes with full two-way communication.

**Target Users**: Developers who use one or more AI coding agents and want a unified dashboard to manage skills, monitor context consumption, create automations, schedule agent tasks, and control everything from one place — instead of editing config files manually for each tool.

**Business Model**: Open-core. MIT-licensed desktop app + optional Pro subscription for real-time monitoring, cloud sync, and team features (closed source, separate repo).

> Note: "ATO" is the short name; "Agentic Tool Optimization" is the full name.

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

### F1: Setup Wizard (First Launch)

4-step onboarding flow:
1. **Welcome** — introduces ATO as multi-LLM control panel
2. **Connect Runtimes** — toggle on/off each runtime, auto-detect CLIs, runtime-specific config (SSH for OpenClaw, API key for Codex, endpoint for Hermes), manual path fallback
3. **Verify** — parallel health check all enabled runtimes (version, auth, connectivity)
4. **Done** — summary + what's next

Persisted to `localStorage`. Runs only on first launch.

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

### F9: MCP Server

Standalone MCP server with 8 tools:
- `get_context_usage` — Context window breakdown
- `list_skills` / `toggle_skill` — Manage skills
- `get_usage_stats` — Token/cost analytics from JSONL logs
- `get_mcp_status` — MCP server configuration
- `get_runtime_status` — Health check for any runtime
- `get_all_runtime_statuses` — Health check all runtimes at once
- `get_agent_logs` — Execution logs (filterable by runtime)

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
