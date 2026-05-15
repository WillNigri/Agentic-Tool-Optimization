# ATO (Open Source)

The GUI for creating, managing, and observing AI agents — across Claude Code, Codex / OpenAI Agents SDK, Gemini CLI / ADK, OpenClaw, Hermes, and Ollama. MIT licensed. Local-first.

Positioning as of v1.3.0 (The GUI Pivot): "the place where you create an agent, not just manage one." Three audiences served by one app: non-technical first-timers (guided chat wizard), power users (Quick form + ⌘K + terminal), teams (Pro/Team tier in `ato-cloud`). See `PRD.md` and `ROADMAP.md` for full positioning.

## Structure

```
apps/desktop/        # Tauri 2.x desktop app (Rust + React)
packages/core/       # Shared types, token utils, config paths (no I/O)
packages/db/         # Database abstraction (SQLite for desktop)
services/mcp-server/ # Standalone MCP server for AI coding tools (stdio, 8 tools)
```

## Commands

- `npm run dev:desktop` — Start Tauri desktop app in dev mode
- `npm run dev:mcp` — Start MCP server in dev mode
- `npx tauri build` — Build desktop app for distribution
- `npm run build` — Build all packages

## Desktop App

Tauri 2.x with:
- **Rust backend**: SQLite (rusqlite), multi-runtime CLI dispatch, recursive skill scanning, context estimation, file watcher (notify crate)
- **React frontend**: Vite + TailwindCSS + Recharts + Zustand + TanStack React Query
- **i18n**: English, Portuguese, Spanish (react-i18next)
- **Theme**: Dark (#0a0a0f) + cyan/mint (#00FFB2) accent
- **Offline-first**: Works without internet, all data in local SQLite
- **withGlobalTauri**: Enabled in tauri.conf.json for Tauri API access

## Multi-Agent Runtime (Two-Way)

Supported runtimes: Claude, Codex, OpenClaw (SSH), Hermes.

**Outbound**: `promptAgent(runtime, prompt, config?)` — unified dispatch
**Inbound**: `queryAgentStatus(runtime)` — deep health check (version, auth, connectivity)
**Logging**: All executions auto-logged to `~/.ato/agent-logs.jsonl`
**Detection**: Searches common paths + npx cache + user's shell PATH + manual override

## Skill Directories Per Runtime

- Claude: `~/.claude/skills/`, `.claude/skills/`, `/etc/claude/skills/`
- Codex: `~/.codex/skills/`, `.agents/skills/`, `.codex/skills/` ($CODEX_HOME)
- OpenClaw: `~/.openclaw/skills/`, workspace/skills/, AGENTS.md, SOUL.md, TOOLS.md ($OPENCLAW_HOME)
- Hermes: `~/.hermes/skills/` (with category subdirs), SOUL.md, memories/

## MCP Server Tools

- `get_context_usage` — Context window breakdown
- `list_skills` / `toggle_skill` — Manage skills
- `get_usage_stats` — Token/cost analytics
- `get_mcp_status` — MCP server health
- `get_runtime_status` — Health check for any runtime
- `get_all_runtime_statuses` — All runtimes at once
- `get_agent_logs` — Execution logs (filterable by runtime)

## Key Implementation Details

- **Project paths**: `project_root()` walks up from CWD to find `.git` or `.claude/` (Tauri CWD is apps/desktop/)
- **Skills are on-demand**: NOT counted in context total (loaded only when triggered)
- **Automation flows**: Auto-detected from `## Step N:` and `## Phase N:` headers in SKILL.md
- **Approval dialog**: When agent response contains SKILL.md content, shows approval UI
- **Setup wizard**: First-launch onboarding, persisted to localStorage

## Information Architecture (v1.3.0+)

The desktop app sidebar has **6 top-level sections** (collapsed from 24 in v1.2.x). Every screen lives under one of these:

1. **Home** — landing; Create Agent CTA, recent agents, recent runs, alerts.
2. **Agents** — list, detail (Config / Skills / MCPs / Permissions), + New Agent wizard.
3. **Skills & MCPs** — Skills tab, MCPs tab (with install registry), Marketplace tab.
4. **Runs** — Live (active sessions) / History (logs) / Schedules (cron) / Automations (workflows) / Hooks.
5. **Insights** — Health, Analytics, Context, Audit Log.
6. **Settings** — Runtimes (merged), Models, API Keys, Secrets, Env, Cloud (auth+sync+teams+notifications), Projects.

**Persistent surfaces**: Command palette (⌘K), Chat/Terminal pane (bottom, expandable), Project switcher (sidebar top).

## Create Agent flow (v1.3.0+)

Two paths share the same end-state. Guided (chat) is default for non-technical users; Quick (form) is default for power users.

**File-writing contract per runtime** (all writes go through hash check + auto-backup + audit log):

| Runtime | Path |
|---|---|
| Claude Code | `~/.claude/agents/<slug>.md` (global) or `<project>/.claude/agents/<slug>.md` (project) |
| Codex / OpenAI Agents SDK | `~/.codex/agents/<slug>/` directory + `AGENTS.md` |
| Gemini CLI / ADK | `<project>/.gemini/agents/<slug>.yaml` (root_agent.yaml entry) |
| OpenClaw | `~/.openclaw/agents/<slug>/SOUL.md` + `TOOLS.md` |
| Hermes | `~/.hermes/agents/<slug>/` |

**Auth resolution order** for the wizard's "AI suggests a stack" step (subscriptions OR keys, both first-class):
1. Active CLI subscription — shell out (`claude --print`, `codex --print`, `gemini -p`)
2. Active API key in `LlmApiKeys` — direct provider SDK
3. Local Ollama on `localhost:11434`
4. Pro-tier `/agent-suggest` on `ato-cloud` (fallback)

Drafts persisted to `~/.ato/agent-drafts/<id>.json` so users can leave and resume.

## Embedded Terminal (v1.3.0+)

Replaces chat-only PromptBar with an expandable bottom pane, two modes:

- **Chat mode** — same as today's PromptBar (`promptAgent()`).
- **Terminal mode** — full xterm shell scoped to active project CWD.

**Stack**:
- Frontend: `xterm.js` + `@xterm/addon-fit` + `@xterm/addon-web-links`
- Backend: `portable-pty` Rust crate, spawned via Tauri command, default shell `$SHELL` (Unix) / `pwsh.exe` (Windows)
- PATH from login-shell spawn (existing pattern).
- Persistent across navigation; collapsible; resizable.

## Open Source vs Closed Source

**Open source (this repo)**: Skills manager + marketplace, multi-agent runtime, subagents, **Create Agent wizard (v1.3.0)**, **MCP install UI (v1.3.0)**, **embedded terminal (v1.3.0)**, automation builder, cron scheduling, context visualizer, hooks, ATO MCP server (8 tools), config editor, command palette, i18n.

**Closed source (`ato-cloud`, paid)**: Real-time cron monitoring, silent failure detection, push notifications, usage analytics (cloud), cloud sync of agents + skills, team workspaces, hosted `/agent-suggest` fallback, MCP registry mirror, hosted LLM-as-judge evaluators, agent-trace upload + retention, cross-runtime config-change ledger, provider-key + provider-usage tracking, mesh-relay (paid-tier WebSocket router for laptop↔server dispatch), SSO + audit retention (Enterprise).

## Security

- Desktop is local-first. No network calls unless sync explicitly enabled.
- Use parameterized SQL queries only.
- SSH for OpenClaw uses key-based auth (paths only, no key contents stored).
- Validate all inputs with zod schemas.
- Tauri apps get user's full PATH via login shell spawn.
