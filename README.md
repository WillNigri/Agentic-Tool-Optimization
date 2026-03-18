# ATO — Agentic Tool Optimization

The control panel for AI coding tools. A desktop dashboard and MCP server that gives you full visibility and control over Claude Code's skills, subagents, hooks, automations, context window, MCP servers, and usage analytics.

**MIT Licensed** | **Offline-first** | **macOS, Windows, Linux**

---

## What it does

ATO replaces guessing with seeing. Instead of wondering what's loaded in your context, which skills might conflict, or how much you're spending — you get a real-time dashboard.

### Skills Manager
- **4-scope hierarchy**: Enterprise > Personal > Project > Plugin — with visual priority arrows
- **Description conflict analyzer**: Detects skills with overlapping descriptions that could cause Claude to auto-invoke the wrong one (since skills don't require /commands)
- **Official Anthropic standards**: 500-line guideline counter, all frontmatter fields (`user-invocable`, `disable-model-invocation`, `argument-hint`, `context: fork`, `allowed-tools`, `model`)
- **Allowed tools grid**: See all 10 Claude tools at a glance — highlighted if enabled, faded if restricted. Toggle in edit mode
- **Support file links**: Automatically extracts relative markdown links from skill content, shows referenced files
- **Substitution reference**: `$ARGUMENTS`, `${CLAUDE_SKILL_DIR}`, `${CLAUDE_SESSION_ID}` shown in edit mode
- **Directory structure**: View scripts/, references/, assets/ subdirectories for directory-based skills

### Subagents Manager
- Create and manage subagents **with skill access** — the key differentiator from traditional subagents
- Assign skills, allowed tools, model override, and custom instructions per subagent
- Agent types: General Purpose, Explorer, Planner, Custom

### Hooks Manager
- Manage shell hooks by event type: PreToolUse, PostToolUse, Notification, Stop, SubagentStop
- Color-coded by event type for quick scanning
- Configure command, matcher (regex/exact), timeout, and scope (global/project)
- Inline expand-to-edit — no modal needed

### Automation Flow (n8n-style)
- Visual flow diagrams of user-configured automations
- External MCP service nodes: Gmail, Slack, GitHub, Linear, Postgres, Notion — each with brand colors
- SVG bezier connections with animated data flow indicators
- Pan, zoom (scroll wheel + buttons), click-to-inspect nodes
- Workflow switcher: toggle between multiple automations
- Right panel: connected services, run stats, searchable node list
- Example workflows: PR Review Pipeline, Daily Email Digest, DB Migration Guard, Standup Bot

### Context Visualizer
- **Breakdown chart**: Horizontal bar chart of token usage by category (system prompts, skills, MCP schemas, CLAUDE.md, conversation, file reads)
- **Dependencies viewer**: Click any dependency to view its content — CLAUDE.md, skill files, settings.json
- **Permissions viewer**: All tool permissions at a glance (allowed/ask/denied) with scope info
- Usage percentage bar with color warnings at 75% and 90%

### MCP Server Dashboard
- Status overview cards (connected/disconnected/error counts)
- Click to expand: full tool list with descriptions, environment variables, permissions, connection config
- Restart button per server

### Usage Analytics
- Today / This Week / This Month summary cards
- Burn rate: tokens/hour, cost/hour, estimated time to limit
- 30-day line chart with input/output token trends

### Configuration
- Unified view of all Claude Code config files
- Click any existing file to view its contents with line numbers

### MCP Prompt Bar
- Persistent input at the bottom of every page
- Query Claude Code via MCP tools without leaving the dashboard
- Available commands: context usage, skills list, MCP server status, usage stats, skill toggles
- Expandable chat history with tool invocation labels

### Internationalization
- Full i18n: English, Portuguese, Spanish
- Language switcher in sidebar

---

## Quick Start

### Browser dev mode (no backend needed)

```bash
git clone https://github.com/WillNigri/Agentic-Tool-Optimization.git
cd Agentic-Tool-Optimization
npm install
npm run dev:desktop
```

Opens at `http://localhost:5173` with mock data — full UI, no Rust/Tauri required.

### Desktop app (full Tauri build)

Requires [Rust](https://rustup.rs/) and platform dependencies for [Tauri 2](https://v2.tauri.app/start/prerequisites/).

```bash
npm install
npm run dev -w apps/desktop -- -- tauri dev
```

### MCP server only

```bash
npm run dev:mcp
```

Or install from npm (when published):

```bash
npx ato-mcp
```

---

## Architecture

```
apps/desktop/        # Tauri 2.x desktop app (Rust + React)
packages/core/       # Shared types, token utils, config paths (no I/O)
packages/db/         # Database abstraction (SQLite for desktop)
services/mcp-server/ # Standalone MCP server for Claude Code (stdio)
```

### Desktop Tech Stack
- **Rust backend**: SQLite (rusqlite), file watcher (notify)
- **React frontend**: Vite + TailwindCSS + Recharts + Zustand
- **Data fetching**: TanStack React Query
- **Icons**: Lucide React
- **Theme**: Dark (#0a0a0f) + cyan/mint (#00FFB2) accent

### MCP Server Tools
- `get_context_usage` — Context window breakdown
- `list_skills` / `toggle_skill` — Manage skills
- `get_usage_stats` — Token/cost analytics
- `get_mcp_status` — MCP server health

---

## Cloud Sync (Optional)

The desktop app can optionally sync to a cloud backend (closed source, separate repo).
Toggle in Settings: OFF = pure local, ON = syncs to cloud.
The desktop app works fully offline without cloud sync.

---

## Security

- **Local-first**: No network calls unless sync is explicitly enabled
- **Parameterized SQL**: All queries use parameterized statements
- **Input validation**: Zod schemas on all boundaries
- **No secrets in repo**: .env files gitignored, no hardcoded credentials

---

## Downloads

See [Releases](https://github.com/WillNigri/Agentic-Tool-Optimization/releases) for pre-built installers:
- macOS (Apple Silicon + Intel) — `.dmg`
- Windows — `.exe`
- Linux — `.AppImage` / `.deb`

---

## License

MIT — see [LICENSE](LICENSE)
