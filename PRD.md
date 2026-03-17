# ATO - Product Requirements Document (Updated)

## Vision Statement

ATO (Agentic Tool Optimization) is an open-source desktop dashboard and MCP server that makes Claude Code's hidden internals visible and manageable. It's "the control panel for AI coding tools" — giving developers real-time visibility into context consumption, skills hierarchy, MCP connections, and usage analytics.

**Target Users**: Technical developers who use Claude Code daily and want to optimize their workflow, understand what's consuming their context window, and manage skills/MCP servers without editing JSON files manually.

**Business Model**: Open source (MIT) desktop app + optional cloud sync for teams (freemium SaaS hosted on Railway via agenticsearchoptimization.ai).

> Note: "ATO" is the short name; "Agentic Tool Optimization" is the full name.

---

## Architecture (Implemented)

### Hybrid Desktop + Cloud

```
ATO/
├── apps/
│   └── desktop/            # Tauri 2.x (Rust + React) — offline-first
│       ├── src-tauri/      # Rust backend: SQLite, file watcher, commands
│       └── src/            # React frontend: dashboard UI
├── services/               # Cloud sync backend (Railway)
│   ├── api-gateway/        # Express proxy (port 3000), JWT auth, rate limit
│   ├── auth/               # Registration, login, bcrypt + JWT
│   ├── skills/             # Skills CRUD + filesystem scan
│   ├── analytics/          # Token tracking, burn rate, cost projections
│   ├── mcp-monitor/        # MCP server health monitoring
│   └── mcp-server/         # Standalone MCP server (stdio) for Claude Code
├── packages/
│   ├── core/               # Shared types, token utils, config paths (no I/O)
│   ├── db/                 # Database abstraction: SQLite + PostgreSQL
│   └── shared/             # Cloud-specific auth/validation helpers
├── database/migrations/    # PostgreSQL schema
├── Dockerfile              # Railway single-container deployment
└── docker-compose.yml      # Local dev PostgreSQL
```

### Data Flow

1. **Desktop (default, offline)**: Tauri reads `~/.claude/` files via `notify` crate file watcher → stores in local SQLite via `rusqlite` → renders in React UI
2. **Cloud sync (opt-in)**: User enables sync in Settings → desktop pushes data to Railway microservices → PostgreSQL → accessible across devices/team

### Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop Runtime | Tauri 2.x (Rust backend, ~5MB installer) |
| Desktop DB | SQLite via rusqlite (local, offline) |
| Desktop Frontend | React 18 + Vite + TailwindCSS |
| Cloud Backend | Node.js 20 + Express + TypeScript |
| Cloud DB | PostgreSQL 16 (Railway) |
| Auth | bcrypt (12 rounds) + JWT (access + refresh tokens) |
| MCP Server | @modelcontextprotocol/sdk (stdio transport) |
| i18n | react-i18next (English, Portuguese, Spanish) |
| State Management | Zustand |
| Charts | Recharts |
| Deployment | Railway (cloud), Tauri builds (desktop) |

---

## Security (Implemented)

- **Passwords**: bcrypt with 12 rounds. NEVER stored as plain text. NEVER returned in API responses.
- **Tokens**: JWT access tokens (15min) + refresh tokens (7 days). Refresh tokens hashed before DB storage.
- **SQL**: All queries parameterized. No string interpolation in SQL ever.
- **Input Validation**: All inputs validated with zod schemas before processing.
- **Desktop**: Local-first. No network calls unless sync explicitly enabled by user.
- **API Gateway**: Rate limiting (100 req/15min), CORS, helmet security headers.
- **Secrets**: All credentials via environment variables, never committed.

---

## Feature Specifications (Implemented)

### F1: Context Visualizer (P0 - Implemented)

Real-time breakdown of Claude Code's context window:
- Overall progress bar with color shifts at 75% (yellow) and 90% (red)
- Category breakdown chart (system prompts, skills, MCP schemas, CLAUDE.md, conversation, file reads)
- Token estimates from local file sizes
- Desktop: reads files directly via Tauri commands
- Cloud: ingested via analytics service

### F2: Skills Manager (P0 - Implemented)

Visual management of Claude Code skills:
- Lists skills from `~/.claude/skills/` (personal) and `.claude/skills/` (project)
- Parses YAML frontmatter from SKILL.md files
- Token count estimation per skill
- One-click enable/disable (renames file with `.disabled` extension)
- Search and filter
- Conflict detection via keyword overlap (Jaccard similarity >30%)

### F3: Usage Analytics (P0 - Implemented)

Token consumption and cost tracking:
- Today/week/month summary cards
- Burn rate calculation (tokens/hour, cost/hour)
- Time-to-limit estimation
- 30-day usage chart (input + output tokens)
- Parses Claude Code JSONL logs from `~/.claude/logs/`
- Stored in SQLite (desktop) or PostgreSQL (cloud)

### F4: MCP Status Dashboard (P1 - Implemented)

MCP server connection monitoring:
- Lists all configured MCP servers from `~/.claude.json` and `.claude/settings.json`
- Status indicators (green=connected, red=error, yellow=warning)
- Transport type, tool count display
- Restart/reconnect buttons

### F5: Config Editor (P1 - Implemented)

Unified view of Claude Code configuration:
- Lists all config file locations with exists/missing status
- Global vs project scope indication
- Read-only view (editing planned for Phase 2)

### F6: MCP Server for Claude Code (P0 - Implemented)

Standalone MCP server exposable directly to Claude Code:
- Transport: stdio
- Tools: `get_context_usage`, `list_skills`, `toggle_skill`, `get_usage_stats`, `get_mcp_status`
- Install: `npx ato-mcp`

### F7: Cloud Sync (P2 - Architecture Implemented)

Optional sync to Railway backend:
- Toggle in Settings: OFF = pure local, ON = syncs to cloud
- GitHub OAuth login (planned)
- Team skill sharing (planned)
- Cross-device usage dashboards (planned)
- Self-hostable backend via Docker

### F8: i18n (Implemented)

Full internationalization:
- English (en), Portuguese (pt), Spanish (es)
- All UI strings via react-i18next translation keys
- Language switcher in sidebar
- Persisted to localStorage

---

## Design System (Implemented)

- **Theme**: Dark (#0a0a0f background, #16161e cards, #2a2a3a borders)
- **Accent**: Cyan/mint (#00FFB2) for primary actions, active states, highlights
- **Typography**: Inter for UI, JetBrains Mono for code/numbers
- **Status colors**: Green (#00FFB2) connected, Red (#FF4466) error, Yellow (#FFB800) warning
- **Components**: Cards with subtle borders, toggle switches, progress bars, status dots
- **Inspired by**: CodeAuto (sidebar + workflow), ASO_ (dark + cyan/mint aesthetic)

---

## Deployment (Implemented)

### Desktop
- Tauri builds for macOS, Windows, Linux (~5MB installer)
- `npm run dev:desktop` for development
- `npm run build:desktop` for distribution

### Cloud (Railway)
- Single Dockerfile running all microservices behind API gateway
- PostgreSQL addon on Railway
- Health check at `/api/health`
- Configured via `railway.json`

### ASO Integration
- ATO listed as a tool in the ASO directory (agenticsearchoptimization.ai)
- Category: Observability
- Includes llms.txt for agent discovery
- Hosted at `ato.agenticsearchoptimization.ai` (subdomain)

---

## Database Schema

### PostgreSQL (Cloud) — `database/migrations/001_initial_schema.sql`
- `users` — id, email, password_hash (bcrypt), name, timestamps
- `refresh_tokens` — id, user_id, token_hash, expires_at, revoked_at
- `sessions` — id, user_id, token counts, cost, model, metadata
- `usage_records` — id, user_id, timestamp, model, input/output/cache tokens, cost
- `skills` — id, user_id, name, file_path, source, content, token_count, enabled, content_hash
- `mcp_servers` — id, user_id, name, transport, command, status, tool_count
- Auto-updating `updated_at` triggers

### SQLite (Desktop) — `packages/db/src/sqlite.ts`
- Same schema adapted for SQLite syntax
- Additional `settings` table (key-value store for sync config, language, etc.)
- `userId` always 'local' for desktop use
- WAL journal mode for performance

---

## Success Metrics

### MVP Success Criteria
- [x] Installs and runs on macOS (Tauri desktop)
- [x] Offline-first with local SQLite
- [x] Scans and displays skills from `~/.claude/skills/`
- [x] Parses Claude Code logs for usage analytics
- [x] MCP server integration via stdio
- [x] i18n for EN, PT, ES
- [x] Cloud sync architecture ready (Railway)
- [ ] < 5MB installer size (Tauri target)
- [ ] < 50MB memory usage at idle

### Growth Metrics (Post-Launch)
- GitHub stars (target: 1,000 in first month)
- Desktop downloads
- MCP server installations
- Cloud sync signups
- ASO directory listing traffic

---

## File Count Summary

| Area | Files | Lines of Code |
|------|-------|---------------|
| Desktop (Tauri + React) | 37 | ~2,500 |
| Cloud Services | 24 | ~1,800 |
| Packages (core/db/shared) | 18 | ~2,200 |
| Config/Deploy | 14 | ~500 |
| **Total** | **93** | **~6,000** |
