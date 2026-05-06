# ATO — Agentic Tool Optimization

**The GUI for daily agentic work.** Persistent multi-runtime conversations, production-grade agent authoring, and observability — across **Claude Code**, **Codex / OpenAI Agents SDK**, **Gemini CLI / ADK**, **OpenClaw**, **Hermes**, and **Ollama**. Without editing JSON. Without leaving the app.

Switch from Claude to Codex mid-conversation. Variables, hooks, and memory policies travel. Threads persist across restart. Markdown renders. Tokens stream.

### Three audiences, one app

- **First-time users** — chat-style guided wizard ("describe what you want") suggests runtime, model, skills, MCPs. Or pick a starter template. Working agent in under two minutes.
- **Power users** — Quick form, command palette (⌘K), embedded `portable-pty` terminal, persistent threads, drag-drop file attachments, streaming responses with syntax-highlighted markdown.
- **Teams** — cloud sync, shared agents, team-wide observability, SSO, audit retention via the optional Pro / Team tier.

Bring your own auth: ATO rides your existing logged-in CLI subscriptions (Claude Code, Codex, Gemini CLI) the way VS Code rides your GitHub login — *or* you can use stored API keys. Your choice, per runtime.

**[Website](https://agentictool.ai)** | **[Web Dashboard](https://app.agentictool.ai)** | **[SDK Docs](docs/SDK.md)** | **[npm](https://www.npmjs.com/package/@ato-sdk/js)**

**MIT Licensed** | **Local-first** | **macOS, Windows, Linux**

---

## Install

### Desktop App

```bash
# Homebrew (macOS)
brew tap WillNigri/ato
brew install --cask ato

# Or download from GitHub Releases
# macOS (Apple Silicon + Intel), Windows (.exe), Linux (.AppImage, .deb)
```

**[Download latest release](https://github.com/WillNigri/Agentic-Tool-Optimization/releases/latest)**

### SDK (auto-trace LLM costs)

```bash
npm install @ato-sdk/js
```

```typescript
import { init } from '@ato-sdk/js';
import { wrapAnthropic } from '@ato-sdk/js/anthropic';
import Anthropic from '@anthropic-ai/sdk';

init({ apiKey: 'your-ato-key' });
const client = wrapAnthropic(new Anthropic());

// Every call is now auto-traced with costs
const msg = await client.messages.create({
  model: 'claude-sonnet-4-6',
  max_tokens: 1024,
  messages: [{ role: 'user', content: 'Hello' }],
});
```

Works with **Anthropic**, **OpenAI**, and **Claude Agent SDK**. Built-in pricing for **60+ models** across 7 providers. [Full SDK docs](docs/SDK.md).

### MCP Server

```json
{
  "mcpServers": {
    "ato": {
      "command": "npx",
      "args": ["tsx", "services/mcp-server/src/index.ts"]
    }
  }
}
```

The MCP server exposes `run_agent` — any MCP-aware runtime can dispatch to any ATO-managed agent regardless of native runtime. Cross-runtime by protocol, not by hack.

---

## What's in the box

### Daily workspace (v1.5)

- **Persistent chat threads** — conversations survive app restart, scoped optionally to projects, listed in a dropdown with msg count + last activity.
- **Multi-runtime mid-thread** — switch Claude → Codex → Gemini in the same conversation. The full thread history travels to whichever runtime answers next.
- **Streaming responses** — tokens appear as they're generated, with a blinking caret. No more 20-second blocking waits.
- **Syntax-highlighted markdown** — assistant replies render as proper markdown: headings, lists, GFM tables, fenced code blocks with copy buttons. Inline code in cyan.
- **File attachments** — paperclip pick or drag-drop a text file (`.md`, `.json`, `.ts`, `.py`, etc.); contents join the conversation as context.
- **Embedded shell** — real interactive PTY via `xterm.js` + `portable-pty`, scoped to active project, persists across navigation.

### Production-grade agent authoring (v1.4)

Every principle from the [context engineering literature](https://nigri.substack.com/p/context-engineering-2026), as a first-class UI:

- **Variables** — `{user_name}` style templates with resolvers: static, env var, project path, file (Free) + db-query, computed expressions, MCP call (Pro).
- **Pre-call context hooks** — ordered list of resolvers that fire before each turn and inject results into the user message inside `<context>...</context>` tags.
- **Conversation summarizers** — per-agent memory policy (`summarizeAfter`, `keepLastK`, custom summarizer model). Long sessions auto-compact.
- **Multi-agent groups** — router + N children. Visual graph editor with router-in-the-middle, hover-to-inspect rules. Routing via keyword rules + LLM-classifier fallback.
- **Per-task models** — distinct models for routing / summarizing / responding / evaluating. Cheap fast for routing, advanced for response.
- **Observability** — per-agent metrics (run count, p50/p95 latency, success rate), trace explorer with full sequence (variables → hooks → router → response).
- **Evaluators** — heuristic kinds (contains / not-contains / length-range / tool-called) run locally; LLM-as-judge runs Pro cloud-side. Manual + scheduled batch — never live on every dispatch.
- **Tool description rewrite** — per-MCP-tool button that asks your runtime to rewrite the description for your specific use-case.

### Cross-runtime dispatch (agents-as-MCPs)

The MCP server exposes `mcp__ato__run_agent("<slug>", "<prompt>")`. Any MCP-aware runtime can dispatch to any ATO-managed agent. Slug points at a single agent or a group — groups route through their router transparently. This is how cross-runtime works: not via a fragile shim, but as a standard MCP tool.

### Create Agent (3 paths)

- **Guided** — chat wizard: describe goal → ATO suggests runtime/model/skills/MCPs/permissions as confirmable cards.
- **Quick** — one-page form, all fields visible, draft auto-saved.
- **Templates** — 5 production-quality starters (PR Reviewer, Doc Writer, Codebase Explainer, Data Analyst, DevOps Helper). Pick → form pre-filled → customize → save.

All paths write through the same safety pipeline (hash check, auto-backup, audit log) to the right place per runtime.

### Skills, MCPs, projects

- **Skills Manager** — per-runtime tabs, scope grouping (enterprise/personal/project/plugin), drag-to-prioritize, conflict detection (similar-description warnings), AI-powered creation.
- **Skill version history** — every edit auto-snapshots; drawer shows prior versions; restore is itself reversible.
- **Bulk skill ops** — multi-select toolbar: enable/disable/delete N at once.
- **Marketplace** — browse curated + community skills.
- **MCP install UI** — curated registry (filesystem, github, postgres, slack, brave-search, gmail, calendar, …) with one-click install.
- **Projects dashboard** — click a project, see everything: memory hierarchy, skills, subagents, commands, hooks, permissions, MCPs. File watcher auto-refreshes.

### Settings

- **Runtimes** — Setup tab (CLI paths, SSH config, status checks) + **Compare tab** (per-runtime feature/config matrix).
- **Models** — model config per runtime/project.
- **API Keys / Secrets / Environment** — encrypted local storage, OS-keychain-backed where applicable.
- **Cloud** — auth, teams, sync, notifications.
- **Backup** — JSON export/import of all your config (agents, hooks, variables, groups, projects, env, model configs, secrets metadata).

### Cross-cutting

- **Command palette ⌘K** — global search across agents, skills, MCPs, projects, plus quick navigation.
- **Tier gating** — Pro features are visible to Free users with a crown lock badge + upgrade tooltip. Discovery sells; hiding doesn't.
- **i18n** — EN, PT, ES (react-i18next).

### SDK

Auto-capture LLM traces with zero code changes:

| Provider | Wrapper | Import |
|----------|---------|--------|
| Anthropic | `wrapAnthropic(client)` | `@ato-sdk/js/anthropic` |
| OpenAI | `wrapOpenAI(client)` | `@ato-sdk/js/openai` |
| Claude Agent SDK | `wrapAgent(agent)` | `@ato-sdk/js/agent` |
| Any provider | `capture(trace)` | `@ato-sdk/js` |

Each call records: model, tokens (input/output/cached), cost (USD), duration, status, errors, metadata. Built-in pricing for 60+ models. [Full SDK documentation](docs/SDK.md).

---

## Supported Runtimes

| Runtime | Provider | Config Files | Skills Directory |
|---------|----------|-------------|------------------|
| **Claude Code** | Anthropic | `CLAUDE.md`, `.claude/settings.json`, `.mcp.json` | `~/.claude/skills/` |
| **Codex / OpenAI Agents SDK** | OpenAI | `AGENTS.md`, `.codex/config.toml`, `codex.json` | `~/.codex/skills/` |
| **Gemini CLI / ADK** | Google | `GEMINI.md`, `.gemini/settings.json`, `root_agent.yaml` | `.gemini/agents/` |
| **OpenClaw** | OpenClaw | `SOUL.md`, `TOOLS.md`, `openclaw.json` | `~/.openclaw/skills/` |
| **Hermes** | NousResearch | `SOUL.md`, `config.yaml`, `memories/*.md` | `~/.hermes/skills/` |
| **Ollama** | local | auto-detect `localhost:11434` | n/a |

---

## Free vs Pro vs Team vs Enterprise

| | Free (this repo) | Pro $29/seat/mo | Team $49/seat/mo | Enterprise $99+/seat/yr |
|---|---|---|---|---|
| Single-agent create / run / shell / Quick Test | ✅ | ✅ | ✅ | ✅ |
| Cross-runtime MCP dispatch (`run_agent`) | ✅ | ✅ | ✅ | ✅ |
| Persistent multi-runtime threads | ✅ | ✅ | ✅ | ✅ |
| Streaming responses + markdown | ✅ | ✅ | ✅ | ✅ |
| Variables — basic resolvers | ✅ | ✅ | ✅ | ✅ |
| Variables — db-query / computed / MCP-call | – | ✅ | ✅ | ✅ |
| Pre-call context hooks | – | ✅ | ✅ | ✅ |
| Tunable summarizer policy | – | ✅ | ✅ | ✅ |
| Multi-agent groups | up to 3 children | unlimited | unlimited + shared | unlimited |
| Visual group graph editor | view-only | edit | edit + collab | edit + audit |
| Per-task model selection | – | ✅ | ✅ | ✅ |
| Local trace history | last 100 runs | unlimited | unlimited | unlimited |
| Cloud trace retention | – | 30 days | 90 days | unlimited |
| Observability dashboard | basic counts | full per-agent | + team aggregates | + SLA dashboards |
| LLM-as-judge evaluators | – | ✅ | ✅ | ✅ |
| Cron / Schedules | up to 3 jobs | unlimited | unlimited | unlimited + SLA |
| Cloud sync of agents | – | ✅ | ✅ | ✅ |
| Team workspaces / shared agents | – | – | ✅ | ✅ |
| SSO / Audit retention | – | – | – | ✅ |

The OSS desktop is fully functional standalone — Pro adds cloud-side capabilities (suggest fallback, hosted judge, trace retention, sync). Sign-in is optional.

---

## Architecture

```
apps/
  desktop/                 # Tauri 2.x desktop app (Rust + React)
  web/                     # Web dashboard (Vite + React)

packages/
  sdk/                     # @ato-sdk/js — auto-trace LLM calls
  core/                    # Shared types, token utils
  db/                      # Database adapters

services/
  mcp-server/              # Standalone MCP server with `run_agent`
```

### Cloud Backend (separate repo, Pro+)

```
api.agentictool.ai
├── API Gateway       # Routing, JWT auth, tiered rate limiting
├── Auth              # Register, login, GitHub OAuth, SSO/OIDC, tier
├── Skills            # CRUD, agent-suggest, agent-traces, agent-evaluators/judge
├── Analytics         # Token tracking, cost aggregation, burn rate
├── MCP Monitor       # MCP server health monitoring
├── Teams             # Workspaces, roles, activity logs
└── Notifications     # Email (SMTP), Slack, Discord, Telegram
```

### Data Storage (desktop — all local)

| Data | Location |
|------|----------|
| Database | `~/.ato/local.db` (SQLite) |
| Agent logs / traces | `~/.ato/agent-logs.jsonl` |
| Workflows | `~/.ato/workflows/` |
| Cron jobs | `~/.ato/cron-jobs.json` |
| File backups | `~/.ato/backups/` (auto-pruned >30 days) |

---

## Quick Start (Development)

```bash
git clone https://github.com/WillNigri/Agentic-Tool-Optimization.git
cd Agentic-Tool-Optimization

# Desktop app
cd apps/desktop && npm install && npx tauri dev

# MCP server
npm run dev:mcp

# SDK development
cd packages/sdk && npm run dev
```

Requires [Rust](https://rustup.rs/) and [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/).

---

## Version History

| Version | Highlights |
|---------|-----------|
| **v1.5.0** | **Daily workspace** — persistent threads (SQLite), streaming responses, syntax-highlighted markdown rendering, file attachments, multi-runtime mid-thread (history travels with or without an agent), per-thread sticky agent, project scoping, in-thread runtime swap |
| **v1.4.0** | **Production-grade agent authoring** — Variables (F1), Context Hooks (F2), Summarizers (F3), Multi-agent Groups + Router + Graph Editor (F4), Per-task Models (F5), Observability + Trace Explorer (F6), Evaluators (F7), Tool Description Rewrite (F8); Pro tier gating; agent templates (5 starters); skill version history; bulk skill ops; runtime comparison tab; configuration export/import |
| **v1.3.0** | **The GUI Pivot** — IA collapse (24 → 6 sections), Home page, Create Agent (Guided + Quick), MCP install UI, embedded terminal (xterm + portable-pty), command palette (⌘K), subscriptions-or-keys auth model |
| **v1.2.0** | Visual workspace canvas, live execution visualization, skill palette, multi-select batch ops |
| **v1.1.0** | Projects dashboard, 6 runtimes (+ Gemini + OpenAI Agents SDK), Ollama provider, CodeMirror editor with conflict detection + inline lint, sandbox/policies management, backup/restore, file watcher, token chart, i18n (EN/PT/ES) |
| **v1.0.0** | SDK (`@ato-sdk/js`), web dashboard, cost tracking, LLM API key management, audit logging, agent monitor, SSO, Homebrew tap |

---

## Engineering

- **CI/CD**: GitHub Actions runs `cargo check` + `cargo test` + `vitest run` + `vite build` on every PR
- **66+ Rust unit tests** + frontend Vitest tests
- **Code splitting**: Sidebar sections lazy-loaded via `React.lazy`
- **Accessibility**: ARIA labels on navigation, dialogs, dashboard tabs
- **Modular Rust**: types separate from commands

## Security

- **Local-first** — no network calls unless sync explicitly enabled
- **Parameterized SQL** — all queries
- **API keys** — encrypted locally, never sent externally
- **SSH** — OpenClaw uses key-based auth (paths only)
- **Validation** — all inputs validated with Zod / serde
- **Conflict detection** — content hashing prevents overwriting concurrent edits
- **Auto-backup** — every file write creates a timestamped backup, restorable from the UI
- **Audit trail** — every file write logged with diff stats and backup path
- **db-query resolver** — opens SQLite read-only; rejects anything that isn't `SELECT/WITH`
- **computed resolver** — constrained expression grammar, not arbitrary JS

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE)
