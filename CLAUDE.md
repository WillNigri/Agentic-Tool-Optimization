# ATO (Open Source)

Multi-LLM control panel for AI coding tools. Supports Claude Code, Codex, OpenClaw, and Hermes. MIT licensed.

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

## Open Source vs Closed Source

**Open source (this repo)**: Skills manager + marketplace, multi-agent runtime, subagents, automation builder, cron scheduling, context visualizer, hooks, MCP server (8 tools), config editor, setup wizard, prompt bar, i18n.

**Closed source (separate repo, paid)**: Real-time cron monitoring, silent failure detection, push notifications, usage analytics (cloud), cloud sync, team workspaces.

## Security

- Desktop is local-first. No network calls unless sync explicitly enabled.
- Use parameterized SQL queries only.
- SSH for OpenClaw uses key-based auth (paths only, no key contents stored).
- Validate all inputs with zod schemas.
- Tauri apps get user's full PATH via login shell spawn.
