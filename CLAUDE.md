# ATO (Open Source)

Multi-LLM control panel for AI coding tools. Supports Claude, Codex, OpenClaw, and Hermes. MIT licensed.

## Structure

```
apps/desktop/        # Tauri 2.x desktop app (Rust + React)
packages/core/       # Shared types, token utils, config paths (no I/O)
packages/db/         # Database abstraction (SQLite for desktop)
services/mcp-server/ # Standalone MCP server for Claude Code (stdio)
```

## Commands

- `npm run dev:desktop` — Start Tauri desktop app in dev mode
- `npm run dev:mcp` — Start MCP server in dev mode
- `npm run build:desktop` — Build desktop app for distribution
- `npm run build` — Build all packages

## Desktop App

Tauri 2.x with:
- **Rust backend**: SQLite (rusqlite), multi-runtime CLI dispatch, file watcher (notify crate)
- **React frontend**: Vite + TailwindCSS + Recharts + Zustand
- **i18n**: English, Portuguese, Spanish (react-i18next)
- **Theme**: Dark (#0a0a0f) + cyan/mint (#00FFB2) accent
- **Offline-first**: Works without internet, all data in local SQLite

## Multi-Agent Runtime

Supported runtimes: Claude, Codex, OpenClaw (SSH), Hermes.
- `detect_agent_runtimes` — auto-detect installed CLIs
- `prompt_agent(runtime, prompt, config?)` — unified dispatch to any runtime
- Per-node runtime selection in automation workflows
- Runtime-specific config: OpenClaw (SSH), Codex (API key), Hermes (endpoint)

## MCP Server Tools

- `get_context_usage` — Context window breakdown
- `list_skills` / `toggle_skill` — Manage skills
- `get_usage_stats` — Token/cost analytics
- `get_mcp_status` — MCP server health

## Open Source vs Closed Source

**Open source (this repo)**: Skills manager, marketplace, multi-agent runtime, subagents, automation builder, cron scheduling, context visualizer, hooks, MCP dashboard, config, i18n.

**Closed source (separate repo, paid)**: Cron health monitoring dashboard (7-day timeline, silent failure detection, alert banners, auto-retry), usage analytics (cloud), cloud sync, team features, push notifications.

## Security

- Desktop is local-first. No network calls unless sync explicitly enabled.
- Use parameterized SQL queries only.
- SSH for OpenClaw uses key-based auth (paths only, no key contents stored).
- Validate all inputs with zod schemas.
