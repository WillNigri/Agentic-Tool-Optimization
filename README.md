# ATO — Agentic Tool Optimization

The **multi-LLM control panel** for AI coding tools. One dashboard to manage **Claude Code**, **Codex**, **OpenClaw**, and **Hermes** — skills, subagents, automation workflows, and cron scheduling across all runtimes.

**MIT Licensed** | **Offline-first** | **macOS, Windows, Linux**

---

## Supported Runtimes

| Runtime | Provider | Send Prompts | Get Status | Skills Directory |
|---------|----------|:---:|:---:|------------------|
| **Claude** | Anthropic | `claude --print` | MCP + auth check | `~/.claude/skills/` |
| **Codex** | OpenAI | `codex --print` | version + API key | `~/.codex/skills/` |
| **OpenClaw** | OpenClaw | SSH `openclaw exec` | SSH health + status | `~/.openclaw/skills/` |
| **Hermes** | NousResearch | `hermes --execute` | version + `/health` | `~/.hermes/skills/` |

Full two-way communication with all runtimes. Auto-detects installed CLIs, verifies health, logs executions, and exposes status via MCP. Mix runtimes in the same workflow.

---

## Features

### Skills Manager
- **Per-runtime tabs** — filter by Claude / Codex / OpenClaw / Hermes
- Reads real files from each runtime's skill directories
- Recursive scanning (supports gstack-style nested directories)
- Drag-and-drop priority ordering within scope groups
- Conflict detection when skill descriptions overlap
- Create, edit, delete skills — writes to correct runtime directory

### AI Skill Creation
- Describe what you want in plain text → AI generates the SKILL.md
- In-app approval dialog — preview content, choose scope, click "Approve & Save"
- Works with any connected runtime (Claude, Codex, etc.)

### Skills Marketplace
- Browse community skills across 9 categories
- One-click install, publish your own, share with users
- Auto-improve: AI rewrites skills with diff preview

### Automation Builder
- **Auto-detects flows from installed skills** — parses `## Step` and `## Phase` headers
- Works with gstack (`/ship`, `/qa`, `/review`, `/design-review`, etc.) and any skill pack
- Per-node runtime selection — mix Claude + Codex in one workflow
- Visual drag-and-drop editor with decision branching
- Run button dispatches to correct runtime

### Cron Monitor
- **Google Calendar view** with color-coded execution status
- Click any day → see the job's output (green) or error (red)
- List view with 7-day execution timeline
- Create jobs with cron expression validation + human-readable preview
- Smart failure detection: silent failures, chronic warnings, alert dedup
- Manual "Run Now" trigger + auto-retry

### Context Visualizer
- **Per-runtime breakdown** — each LLM has different files loaded
- Skills shown as "on-demand" — NOT counted in always-loaded total
- "Not connected" state for uninstalled runtimes
- Color warnings at 75% and 90% usage

### Subagents Manager
- Create subagents with runtime selection (Claude/Codex/OpenClaw/Hermes)
- Runtime-specific config (SSH for OpenClaw, API key for Codex, endpoint for Hermes)
- Assign skills, tools, model override, custom instructions

### Prompt Bar
- Runtime selector dropdown (switch between Claude/Codex/OpenClaw/Hermes)
- Responses show which runtime answered
- Auto-detects skill content → shows approval dialog for file saves

### Setup Wizard
- Runs on first launch — connect your runtimes, verify health, start using
- Auto-detects installed CLIs + manual path fallback

### MCP Server (8 Tools)
- `get_context_usage` — Context window breakdown
- `list_skills` / `toggle_skill` — Manage skills
- `get_usage_stats` — Token/cost analytics
- `get_mcp_status` — MCP server configuration
- `get_runtime_status` — Health check any runtime
- `get_all_runtime_statuses` — Health check all runtimes
- `get_agent_logs` — Execution logs (filterable by runtime)

---

## Open Source vs Pro

| | Open Source (this repo) | Pro (paid, separate repo) |
|---|---|---|
| **Skills** | Manager, marketplace, AI creation, sharing | — |
| **Automation** | Builder, auto-detect from skills, per-node runtime | — |
| **Cron** | Scheduling, calendar view, manual trigger | Real-time monitoring, push alerts |
| **Context** | Per-runtime breakdown, on-demand skills | — |
| **Analytics** | Local log parsing | Cloud aggregation, cross-runtime |
| **Sync** | — | Cloud sync across machines |
| **Teams** | — | Workspaces, access controls |
| **Alerts** | — | Silent failure detection, Slack/email |

---

## Quick Start

### Desktop App

```bash
git clone https://github.com/WillNigri/Agentic-Tool-Optimization.git
cd Agentic-Tool-Optimization/apps/desktop
npm install
npx tauri dev
```

Requires [Rust](https://rustup.rs/) and [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/).

### Pre-built Installers

Download from [Releases](https://github.com/WillNigri/Agentic-Tool-Optimization/releases):
- macOS Apple Silicon — `.dmg`
- macOS Intel — `.dmg`
- Windows — `.exe` / `.msi`
- Linux — `.AppImage` / `.deb`

### Connect ATO MCP to Claude Code

```json
// Add to ~/.claude/settings.json
{
  "mcpServers": {
    "ato": {
      "command": "npx",
      "args": ["tsx", "/path/to/services/mcp-server/src/index.ts"]
    }
  }
}
```

### Runtime Setup

```bash
# Claude (Anthropic)
npm install -g @anthropic-ai/claude-code

# Codex (OpenAI)
npm install -g @openai/codex

# OpenClaw — configure SSH in ATO Setup Wizard

# Hermes — install per NousResearch docs
```

---

## Architecture

```
apps/desktop/               # Tauri 2.x (Rust + React)
  src/components/            # Skills, Cron, Automation, Context, etc.
  src/stores/                # Zustand (automation, cron)
  src/lib/                   # API layer, cron utils, skill-to-workflow parser
  src-tauri/src/lib.rs       # Rust: multi-runtime dispatch, skill scanning, context

services/mcp-server/         # Standalone MCP server (8 tools)
packages/core/               # Shared types (no I/O)
```

### Data Storage (all local)

| Data | Location |
|------|----------|
| Claude skills | `~/.claude/skills/`, `.claude/skills/` |
| Codex skills | `~/.codex/skills/`, `.agents/skills/` |
| OpenClaw skills | `~/.openclaw/skills/` |
| Hermes skills | `~/.hermes/skills/` |
| Workflows | `~/.ato/workflows/` |
| Cron jobs | `~/.ato/cron-jobs.json` |
| Agent logs | `~/.ato/agent-logs.jsonl` |
| Database | `~/.ato/local.db` |

---

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the full plan.

| Version | Focus | Status |
|---------|-------|--------|
| v0.3.0 | Multi-LLM platform, marketplace, cron, automation | **Released** |
| v0.4.0 | Real-time monitoring (Pro) | Planned |
| v0.5.0 | Cloud sync & teams | Planned |
| v0.6.0 | Deeper runtime integration | Planned |
| v0.7.0 | Marketplace backend | Planned |
| v0.8.0 | Advanced automation | Planned |
| v1.0.0 | Production ready + code signing | Planned |

---

## Security

- Local-first. No network calls unless sync explicitly enabled.
- Parameterized SQL queries only.
- SSH for OpenClaw uses key-based auth (paths only).
- All inputs validated with zod schemas.

## License

MIT — see [LICENSE](LICENSE)
