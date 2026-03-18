# ATO — Agentic Tool Optimization

The **multi-LLM control panel** for AI coding tools. One dashboard to manage **Claude**, **Codex**, **OpenClaw**, and **Hermes** — skills, subagents, automation workflows, and cron scheduling across all runtimes.

**MIT Licensed** | **Offline-first** | **macOS, Windows, Linux**

---

## Supported Runtimes

| Runtime | Provider | Outbound (send) | Inbound (status) | Transport |
|---------|----------|-----------------|-------------------|-----------|
| **Claude** | Anthropic | `claude --print` | MCP tools + `--version` + auth check | Local CLI |
| **Codex** | OpenAI | `codex --print` | `--version` + `--help` + API key detection | Local CLI |
| **OpenClaw** | OpenClaw | SSH `openclaw exec` | SSH `openclaw --version` + `openclaw status` | SSH remote |
| **Hermes** | Hermes | `hermes --execute` | `--version` + `/health` endpoint probe | CLI + HTTP |

**Full two-way communication** with all runtimes. ATO auto-detects installed CLIs, verifies health (auth, connectivity, API keys), logs all executions to `~/.ato/agent-logs.jsonl`, and exposes status via both MCP tools and Tauri commands. Mix runtimes in the same workflow — run code review with Claude, then deploy with Codex, all in one pipeline.

---

## Open Source vs Pro

ATO follows an open-core model. The platform, multi-agent runtime, and all local tooling ship MIT. Monitoring, analytics, and cloud sync are closed source (separate repo, paid).

### Open Source (this repo)

| Feature | Description |
|---------|-------------|
| **Skills Manager** | 4-scope hierarchy, conflict detection, frontmatter editor, directory skills |
| **Skills Marketplace** | Browse, install, publish, share community skills across 9 categories |
| **Multi-Agent Runtime** | Claude, Codex, OpenClaw, Hermes — unified `promptAgent()` dispatch |
| **Subagents Manager** | Create subagents with runtime selection + runtime-specific config |
| **Automation Builder** | n8n-style visual workflows with per-node runtime selection |
| **Cron Scheduling** | Create & trigger cron jobs with any runtime, expression validation |
| **Context Visualizer** | Real-time context window breakdown with token budget warnings |
| **Hooks Manager** | Shell commands on Claude Code events (Pre/Post ToolUse, Stop) |
| **MCP Dashboard** | Monitor MCP server connections, tools, restart |
| **Configuration** | Unified view of all config files |
| **i18n** | English, Portuguese, Spanish |

### Pro (closed source, separate repo)

| Feature | Description |
|---------|-------------|
| **Cron Health Monitor** | 7-day execution timeline, smart failure detection, alert banners |
| **Silent Failure Detection** | Catches jobs that should have run but didn't |
| **Alert System** | Chronic warning detection, dedup, auto-retry |
| **Usage Analytics** | Token consumption, cost tracking, burn rate across all runtimes |
| **Cloud Sync** | Sync skills, workflows, cron jobs across machines |
| **Team Features** | Shared workspaces, access controls, collaboration |
| **Push Notifications** | Slack/email alerts for cron failures and SLA breaches |

---

## What it does

### Skills Manager
- **4-scope hierarchy**: Enterprise > Personal > Project > Plugin — with visual priority arrows
- **Description conflict analyzer**: Detects skills with overlapping descriptions that could cause auto-invocation of the wrong skill
- **Official Anthropic standards**: 500-line guideline counter, all frontmatter fields (`user-invocable`, `disable-model-invocation`, `argument-hint`, `context: fork`, `allowed-tools`, `model`)
- **Allowed tools grid**: See all 10 Claude tools at a glance — highlighted if enabled, faded if restricted
- **Drag-and-drop priority**: Reorder skills within scope groups to control which skill wins when descriptions overlap
- **Support file links**: Automatically extracts relative markdown links, shows referenced files
- **Substitution reference**: `$ARGUMENTS`, `${CLAUDE_SKILL_DIR}`, `${CLAUDE_SESSION_ID}` shown in edit mode
- **Directory structure**: View scripts/, references/, assets/ subdirectories

### Skills Marketplace
- **9 categories**: library-reference, product-verification, data-fetching, business-process, code-scaffolding, code-quality, ci-cd, runbooks, infra-ops
- **Install**: One-click install to `~/.claude/skills/`
- **Publish**: Share your skills with the community
- **Private sharing**: Share with specific users via link/JSON
- **Auto-improve**: Run autoresearch prompt on any skill, review diff, apply or discard

### Multi-Agent Runtime
- **Unified dispatch**: `promptAgent(runtime, prompt, config?)` routes to the correct CLI
- **Auto-detection**: `detect_agent_runtimes` checks `which` for each CLI
- **Runtime-specific config**:
  - **Claude**: Local CLI, no extra config needed
  - **Codex**: Optional API key path
  - **OpenClaw**: SSH host, port, user, key path — executes on remote host
  - **Hermes**: Optional endpoint URL

### Subagents Manager
- Create subagents with **runtime selection** — assign Claude, Codex, OpenClaw, or Hermes
- Runtime-specific configuration fields appear based on selection
- Assign skills, allowed tools, model override, and custom instructions per subagent
- Agent types: General Purpose, Explorer, Planner, Custom
- Runtime badge on each card for quick identification

### Automation Builder (n8n-style)
- Visual drag-and-drop workflow editor with SVG bezier connections
- **Per-node runtime selection**: Mix Claude and Codex in the same workflow
- Service integrations: GitHub, Slack, Gmail, Postgres, Notion, Linear (each with brand colors)
- Decision nodes with conditional branching
- Animated data flow indicators
- Workflow persistence to `~/.ato/workflows/`
- Prompt serialization: converts workflow to structured prompt with `@runtime` per step

### Cron Scheduling
- Standard 5-field cron expressions with validation
- **Any runtime**: Schedule Claude, Codex, OpenClaw, or Hermes jobs
- Human-readable schedule preview ("Every day at 7:00 AM")
- Link cron jobs to automation workflows
- Manual "Run Now" trigger
- Retry failed executions
- Persistence to `~/.ato/cron-jobs.json`

### Context Visualizer
- Horizontal bar chart of token usage by category (system prompts, skills, MCP schemas, CLAUDE.md, conversation)
- Dependencies viewer: click any dependency to view its content
- Permissions viewer: all tool permissions at a glance
- Usage percentage bar with color warnings at 75% and 90%

### Hooks Manager
- Shell hooks by event: PreToolUse, PostToolUse, Notification, Stop, SubagentStop
- Color-coded by event type
- Configure command, matcher (regex/exact), timeout, scope (global/project)

### MCP Server Dashboard
- Status cards (connected/disconnected/error counts)
- Expand for tool list, environment, permissions, connection config
- Restart button per server

### MCP Prompt Bar
- Persistent input at the bottom of every page
- Query Claude Code via MCP tools without leaving the dashboard

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

Opens at `http://localhost:5173` — full UI, no Rust/Tauri required.

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

### Connect ATO MCP to Claude Code

Add the ATO MCP server to your Claude Code settings so Claude can read your dashboard data (context, skills, usage, runtime status):

```bash
# Option 1: Add to ~/.claude/settings.json
cat <<EOF >> ~/.claude/settings.json
{
  "mcpServers": {
    "ato": {
      "command": "npx",
      "args": ["tsx", "/path/to/Agentic-Tool-Optimization/services/mcp-server/src/index.ts"]
    }
  }
}
EOF

# Option 2: If you built the MCP server
cd services/mcp-server && npm run build
# Then add to settings.json:
# "ato": { "command": "node", "args": ["/path/to/services/mcp-server/dist/index.js"] }
```

Once connected, Claude Code gets these tools:
- `get_context_usage` — See what's consuming your context window
- `list_skills` / `toggle_skill` — Manage skills from the CLI
- `get_usage_stats` — Token consumption and cost data
- `get_mcp_status` — Check configured MCP servers
- `get_runtime_status` — Health check any runtime (Claude/Codex/OpenClaw/Hermes)
- `get_all_runtime_statuses` — Health check all runtimes at once
- `get_agent_logs` — Read agent execution history

### Runtime Setup

ATO auto-detects installed runtimes. Install the ones you want to use:

```bash
# Claude (Anthropic) — required for core functionality
npm install -g @anthropic-ai/claude-code

# Codex (OpenAI) — optional
npm install -g @openai/codex

# OpenClaw — requires SSH access to a host running OpenClaw
# Configure host/port/user/key in ATO subagent or cron job settings

# Hermes — install per Hermes documentation
```

---

## Architecture

```
apps/desktop/               # Tauri 2.x desktop app (Rust + React)
  src/
    components/
      automation/            # Visual workflow builder (types, canvas, nodes)
      cron/                  # Cron monitoring types
    stores/
      useAutomationStore.ts  # Workflow state (Zustand)
      useCronStore.ts        # Cron jobs, executions, alerts (Zustand)
    lib/
      tauri-api.ts           # Frontend → Rust command bridge
      cron-utils.ts          # Cron parser, validator, human-readable
      cron-health.ts         # Smart failure detection logic
      marketplace-mock.ts    # Community skill catalog (mock)
      skill-similarity.ts    # Conflict detection algorithm
    i18n/locales/            # EN, PT, ES translations
  src-tauri/src/lib.rs       # Rust backend (SQLite, CLI dispatch, file I/O)

packages/core/               # Shared types, token utils, config paths (no I/O)
packages/db/                 # Database abstraction (SQLite for desktop)
services/mcp-server/         # Standalone MCP server for Claude Code (stdio)
```

### Tech Stack
- **Rust backend**: SQLite (rusqlite), multi-runtime CLI dispatch, file watcher (notify)
- **React frontend**: Vite + TailwindCSS + Recharts + Zustand
- **Data fetching**: TanStack React Query
- **Icons**: Lucide React
- **Theme**: Dark (#0a0a0f) + cyan/mint (#00FFB2) accent

### MCP Server Tools
- `get_context_usage` — Context window breakdown
- `list_skills` / `toggle_skill` — Manage skills
- `get_usage_stats` — Token/cost analytics
- `get_mcp_status` — MCP server health
- `get_runtime_status` — Health check for any runtime (claude/codex/openclaw/hermes)
- `get_all_runtime_statuses` — Health check all runtimes at once
- `get_agent_logs` — Read agent execution logs (filterable by runtime)

### Tauri Commands (Rust → Frontend)

| Command | Description |
|---------|-------------|
| `detect_agent_runtimes` | Check which CLIs are installed |
| `prompt_agent` | Dispatch prompt to any runtime (auto-logs) |
| `prompt_claude` | Direct Claude CLI invocation |
| `query_agent_status` | Deep health check for a single runtime |
| `query_all_agent_statuses` | Fast status check for all runtimes |
| `append_agent_log` | Write structured execution log entry |
| `get_agent_logs` | Read execution logs (filterable by runtime) |
| `get_local_skills` / `get_skill_detail` | Scan & read skills |
| `create_skill` / `update_skill` / `delete_skill` | Skill CRUD |
| `list_workflows` / `save_workflow` / `delete_workflow` | Workflow CRUD |
| `list_cron_jobs` / `save_cron_job` / `delete_cron_job` | Cron CRUD |
| `trigger_cron_job` | Execute a cron job immediately |
| `get_cron_history` | Fetch execution history |
| `get_context_estimate` | Token breakdown by category |
| `get_local_config` | MCP servers from settings.json |

---

## Data Storage

All data is local by default. No network calls unless cloud sync is explicitly enabled.

| Data | Location |
|------|----------|
| Skills | `~/.claude/skills/`, `.claude/skills/` |
| Workflows | `~/.ato/workflows/*.json` |
| Cron jobs | `~/.ato/cron-jobs.json` |
| Cron history | `~/.ato/cron-history.json` |
| Agent logs | `~/.ato/agent-logs.jsonl` |
| Database | `~/.ato/local.db` (SQLite) |
| Config | `~/.claude/settings.json` |

## Security

- **Local-first**: No network calls unless sync is explicitly enabled
- **Parameterized SQL**: All queries use parameterized statements
- **Input validation**: Zod schemas on all boundaries
- **SSH for OpenClaw**: Key-based auth, no passwords stored
- **No secrets in repo**: .env files gitignored, no hardcoded credentials
- **Paths only**: Runtime configs store key file paths, not key contents

---

## Downloads

See [Releases](https://github.com/WillNigri/Agentic-Tool-Optimization/releases) for pre-built installers:
- macOS (Apple Silicon + Intel) — `.dmg`
- Windows — `.exe`
- Linux — `.AppImage` / `.deb`

---

## License

MIT — see [LICENSE](LICENSE)

Monitoring dashboard, cloud sync, and analytics are closed source (separate repo, paid subscription).
