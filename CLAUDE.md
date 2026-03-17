# ATO (Open Source)

Desktop dashboard and MCP server for AI coding tool visibility. MIT licensed.

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
- **Rust backend**: SQLite (rusqlite), file watcher (notify crate)
- **React frontend**: Vite + TailwindCSS + Recharts + Zustand
- **i18n**: English, Portuguese, Spanish (react-i18next)
- **Theme**: Dark (#0a0a0f) + cyan/mint (#00FFB2) accent
- **Offline-first**: Works without internet, all data in local SQLite

## MCP Server Tools

- `get_context_usage` — Context window breakdown
- `list_skills` / `toggle_skill` — Manage skills
- `get_usage_stats` — Token/cost analytics
- `get_mcp_status` — MCP server health

## Cloud Sync (Optional)

The desktop app can optionally sync to a cloud backend (closed source, separate repo).
Toggle in Settings: OFF = pure local, ON = syncs to cloud.

## Security

- Desktop is local-first. No network calls unless sync explicitly enabled.
- Use parameterized SQL queries only.
- Validate all inputs with zod schemas.
