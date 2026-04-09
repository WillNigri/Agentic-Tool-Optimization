# ATO — Agentic Tool Optimization

The **multi-LLM control panel** for AI coding tools. One dashboard to manage **Claude Code**, **Codex**, **OpenClaw**, and **Hermes** — skills, API keys, costs, agent monitoring, and automation across all runtimes.

**[Website](https://agentictool.ai)** | **[Web Dashboard](https://app.agentictool.ai)** | **[SDK Docs](docs/SDK.md)** | **[npm](https://www.npmjs.com/package/@ato-sdk/js)**

**MIT Licensed** | **Offline-first** | **macOS, Windows, Linux**

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

Works with **Anthropic**, **OpenAI**, and **Claude Agent SDK**:

```typescript
import { wrapOpenAI } from '@ato-sdk/js/openai';
import { wrapAgent } from '@ato-sdk/js/agent';
```

Built-in pricing for **60+ models** across 7 providers. [Full SDK docs](docs/SDK.md).

### MCP Server (16 tools)

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

---

## What You Get

### Cost Dashboard
Per-model, per-provider, per-day cost breakdowns. See burn rate, daily timeline, team-wide spend. Auto-calculated from SDK traces.

### LLM API Key Management
Store, rotate, and scope API keys for Anthropic, OpenAI, Google, Mistral, Groq, Cohere, Together, Fireworks. Encrypted locally, never sent to any server.

### Agent Monitor
Track active agent sessions, token consumption, error rates, and runtime health across all your AI coding tools. Basic stats free, real-time 3s auto-refresh in Pro.

### Audit Log
Complete trail of every action — skill changes, key rotations, config updates, cron triggers. Filterable, exportable to JSON.

### Skills Manager + Marketplace
Per-runtime tabs (Claude / Codex / OpenClaw / Hermes). Browse marketplace, AI-powered skill creation, conflict detection, recursive directory scanning.

### Automation Builder
Visual workflow editor. Auto-detects flows from skill headers. Per-node runtime selection — mix Claude + Codex in one workflow.

### Cron Monitor
Google Calendar view with color-coded execution status. Click any day for output/error. Smart failure detection, manual trigger, auto-retry.

### Context Visualizer
Per-runtime context breakdown. Skills marked on-demand vs always-loaded. Color warnings at 75% and 90% usage.

### Subagents Manager
Create subagents with runtime selection. Runtime-specific config (SSH for OpenClaw, API keys for Codex). Assign skills, tools, model overrides.

---

## Supported Runtimes

| Runtime | Provider | Send Prompts | Health Check | Skills Directory |
|---------|----------|:---:|:---:|------------------|
| **Claude** | Anthropic | `claude --print` | MCP + auth | `~/.claude/skills/` |
| **Codex** | OpenAI | `codex --print` | version + API | `~/.codex/skills/` |
| **OpenClaw** | OpenClaw | SSH `openclaw exec` | SSH health | `~/.openclaw/skills/` |
| **Hermes** | NousResearch | `hermes --execute` | version + /health | `~/.hermes/skills/` |

---

## Open Source vs Pro

| | Free (this repo) | Pro ([app.agentictool.ai](https://app.agentictool.ai)) |
|---|---|---|
| **Dashboard** | Desktop app | Desktop + Web dashboard |
| **Skills** | Manager, marketplace, AI creation | + Cloud sync, team sharing |
| **API Keys** | Local encrypted storage | + Team-wide key management |
| **Monitoring** | Basic stats, manual refresh | Real-time (3s), smart alerts, charts |
| **Cost Tracking** | Local via SDK | Cloud aggregation, per-team breakdown |
| **Audit Log** | Local | Team-wide, cloud |
| **Automation** | Builder, cron scheduling | + Push notifications (Slack/Discord/Email) |
| **Auth** | GitHub OAuth | + SSO (Google, Okta, Microsoft Entra) |
| **Teams** | — | Workspaces, roles, activity logs |
| **SDK** | Full (MIT) | — |
| **MCP Server** | 16 tools (MIT) | — |

---

## SDK

Auto-capture LLM traces with zero code changes. Supports:

| Provider | Wrapper | Import |
|----------|---------|--------|
| Anthropic | `wrapAnthropic(client)` | `@ato-sdk/js/anthropic` |
| OpenAI | `wrapOpenAI(client)` | `@ato-sdk/js/openai` |
| Claude Agent SDK | `wrapAgent(agent)` | `@ato-sdk/js/agent` |
| Any provider | `capture(trace)` | `@ato-sdk/js` |

Each call automatically records: model, tokens (input/output/cached), cost (USD), duration, status, errors, metadata.

**60+ models priced**: Claude (Opus, Sonnet, Haiku), GPT-4o/4.1/o1/o3/o4-mini, Gemini, Mistral, Groq, Cohere.

[Full SDK documentation](docs/SDK.md)

---

## Architecture

```
apps/
  desktop/                 # Tauri 2.x desktop app (Rust + React)
  web/                     # Web dashboard (Vite + React, Vercel)

packages/
  sdk/                     # @ato-sdk/js — auto-trace LLM calls
  core/                    # Shared types, token utils

services/
  mcp-server/              # Standalone MCP server (16 tools)
```

### Cloud Backend (separate repo, Pro)

```
api.agentictool.ai         # 7 microservices on Railway
├── API Gateway (3000)     # Routing, JWT auth, tiered rate limiting
├── Auth (3001)            # Register, login, GitHub OAuth, SSO/OIDC
├── Skills (3002)          # CRUD, filesystem sync
├── Analytics (3003)       # Token tracking, cost aggregation, burn rate
├── MCP Monitor (3004)     # MCP server health monitoring
├── Teams (3005)           # Workspaces, roles, activity logs
└── Notifications (3006)   # Email (SMTP), Slack, Discord, Telegram
```

### Data Storage (desktop — all local)

| Data | Location |
|------|----------|
| Database | `~/.ato/local.db` (SQLite) |
| Agent logs | `~/.ato/agent-logs.jsonl` |
| Workflows | `~/.ato/workflows/` |
| Cron jobs | `~/.ato/cron-jobs.json` |

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
| **v1.0.0** | SDK (`@ato-sdk/js`), web dashboard, cost tracking, LLM API key management, audit logging, agent monitor, SSO, rate limiting, Homebrew tap |
| v0.8.0 | Agent Configuration Manager, advanced automation |
| v0.7.0 | Marketplace backend, dynamic workflows |
| v0.6.0 | Deeper runtime integration (OpenClaw, Hermes) |
| v0.5.5 | Notifications & integrations (Slack, Discord, Telegram, Email) |
| v0.3.0 | Multi-LLM support, marketplace, cron, automation builder |

---

## Security

- **Local-first** — no network calls unless sync explicitly enabled
- **Parameterized SQL** — all queries use parameterized statements
- **API keys** — encrypted locally, never sent externally
- **SSH** — OpenClaw uses key-based auth (paths only, not key contents)
- **Validation** — all inputs validated with Zod schemas
- **Rate limiting** — tiered (global/auth/API/sensitive) on cloud endpoints

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE)
