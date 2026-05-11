# ATO — Agentic Tool Optimization

**The local-first developer-workflow operations platform for multi-runtime AI agents — used by humans and their coding agents together.** Static system prompts are the floor, not the ceiling. ATO makes it trivial to build agents whose prompts resolve from files, env vars, databases, or other LLMs at fire time. Multi-runtime by protocol. Local-first. MIT.

> **Coding agent? Read [`AGENTS.md`](./AGENTS.md) first.** ATO is built to be driven by both humans (via the GUI) and AI coding agents (via CLI and MCP). The AGENTS.md doc covers everything an AI agent needs to know to operate ATO on a developer's behalf.

```ts
// Your system prompt:
You are a context-aware assistant for {user_name} working on {project_name}.
Today is {today}. The project root is {project_root}.
// All four resolve at every turn — env var, computed JS, project path, current date.
// Plus a pre-call hook injects the latest CHANGELOG.md into <context> on each call.
```

This is what production-grade agents look like — and ATO makes it a 5-minute setup, not weeks of plumbing. **Pick the [Production-grade Agent template](apps/desktop/src/lib/agentTemplates.ts) on first launch to see the dynamic pattern wired end-to-end.**

Two group types make agents collaborate:

- **Routed groups** — single prompt → router picks the right specialist child (keyword rules + LLM-classifier fallback).
- **Sequential automations** — single prompt → children run in order, each agent's output flows into the next as input. **Each child runs on its own runtime**, so you can chain Claude → Codex → Gemini in a single pipeline.

Supported: **Claude Code**, **Codex / OpenAI Agents SDK**, **Gemini CLI / ADK**, **OpenClaw**, **Hermes**, **Ollama** as native CLIs — plus 15+ API providers including Anthropic, OpenAI, Google AI, Mistral, Groq, **xAI/Grok**, Together, Fireworks, DeepSeek, Qwen, MiniMax, Kimi, GLM, Yi.

### Three audiences, one app

- **First-time users** — chat-style guided wizard ("describe what you want") suggests runtime, model, skills, MCPs. Or pick a starter template. Working agent in under two minutes.
- **Power users** — Quick form, command palette (⌘K), embedded `portable-pty` terminal, persistent threads, drag-drop file attachments, streaming responses with syntax-highlighted markdown, sequential automation pipelines.
- **Teams** — cloud sync, shared agents, team-wide observability, SSO, audit retention via the optional Pro / Team tier.
- **AI coding agents** *(new)* — every meaningful operation is reachable from a local CLI (`ato <command>`) or a stdio MCP server. The agent reads [`AGENTS.md`](./AGENTS.md), discovers ATO's surface, and operates it alongside the human. Local SQLite means zero network round-trip; agents never have to leave the machine.

Bring your own auth: ATO rides your existing logged-in CLI subscriptions (Claude Code, Codex, Gemini CLI) the way VS Code rides your GitHub login — *or* you can use stored API keys. Your choice, per runtime.

### Local-first by design

ATO writes everything to `~/.ato/` on the developer's machine:

- **`~/.ato/local.db`** — SQLite database with every dispatch, replay, config change, agent definition, chat thread, skill registration. Agents can `sqlite3` query it directly for fast reads.
- **`~/.ato/agent-logs.jsonl`** — append-only JSONL log; grep-friendly.
- **`~/.ato/workflows/`**, **`~/.ato/cron-jobs.json`**, **`~/.ato/backups/`** — workflows, schedules, auto-backups.

Sign-in is **optional** and only matters for cloud sync features (cross-device trace history, team workspaces). Every core operation — dispatch, replay, compare, file attribution, configuration ledger — works fully offline.

### Relationship to other tools

**ATO is the developer-workflow operations layer for multi-runtime AI agents.** It is *complementary* to production-observability tools like [Langfuse](https://langfuse.com), [Helicone](https://www.helicone.ai), [LangSmith](https://smith.langchain.com), [Arize Phoenix](https://phoenix.arize.com), and [Braintrust](https://www.braintrust.dev) — not a competitor.

- **Those tools** instrument *deployed production stacks* via SDKs and log *end-user conversations* in real time.
- **ATO** covers the *developer side* of the same agent — what you dispatched while building, what you replayed across runtimes, what regressed after a config change, what each dispatch cost, which agent touched which files.

Most production teams use one from each camp: a Langfuse / Helicone for production user-conversation logging, plus ATO for the developer/multi-runtime side. The two views fit together: production tools catch regressions in real user traffic; ATO catches regressions before you ship, and lets you replay any failing example against an alternative runtime in one click.

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

### SDK — narrow scope

```bash
npm install @ato-sdk/js
```

`@ato-sdk/js` is a **trace forwarder for ATO-authored agents deployed outside the desktop app** (Cloudflare Worker / Vercel / Docker / Node bundles). It is **not** a general-purpose LLM observability SDK.

If you have an existing production stack and want general LLM observability, use [Langfuse](https://langfuse.com), [Helicone](https://www.helicone.ai), [LangSmith](https://smith.langchain.com), [Arize Phoenix](https://phoenix.arize.com), or [Braintrust](https://www.braintrust.dev). They're built for that job. ATO is **complementary** to them — see [Relationship to other tools](#relationship-to-other-tools) below.

[Full SDK docs](docs/SDK.md).

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

### Production-Ready Agents (v1.5.5)

- **Production-grade Agent template** — fifth starter ships pre-wired with 4 variables (env / computed / project-path / Date), a pre-call file hook, and a memory policy. Click it once → see the dynamic-prompt pattern end-to-end without manually configuring anything.
- **Dynamic-prompt messaging** — the wizard now spells out that prompts adapt at fire time. Empty states on Variables / Context tabs teach the resolver kinds instead of just saying "no items yet."
- **Cron jobs wake from sleep** on every desktop OS — launchd on macOS, systemd-user timers on Linux, Task Scheduler on Windows. Your scheduled agents fire even when ATO is closed.
- **Demo Tab-to-pause** — viewing the in-app demo? Tab pauses, Tab resumes, Esc stops. Catch a long subtitle without restarting from scratch.

### Daily workspace (v1.5.0–1.5.4)

- **Persistent chat threads** — conversations survive app restart, scoped optionally to projects, listed in a dropdown with msg count + last activity.
- **Multi-runtime mid-thread** — switch Claude → Codex → Gemini in the same conversation. The full thread history travels to whichever runtime answers next.
- **Streaming responses** — tokens appear as they're generated, with a blinking caret. No more 20-second blocking waits.
- **Sequential pipeline messages** — when a Claude → Codex pipeline returns, the messages stagger in with a "stage 1 / 2" badge so you can read each step as it arrives.
- **Syntax-highlighted markdown** — assistant replies render as proper markdown: headings, lists, GFM tables, fenced code blocks with copy buttons. Inline code in cyan.
- **File attachments** — paperclip pick or drag-drop a text file (`.md`, `.json`, `.ts`, `.py`, etc.); contents join the conversation as context.
- **Embedded shell** — real interactive PTY via `xterm.js` + `portable-pty`, scoped to active project, persists across navigation.
- **i18n** — English, Português, Español. Demo subtitles localized too.

### Production-grade agent authoring (v1.4)

Every principle from the [context engineering literature](https://nigri.substack.com/p/context-engineering-2026), as a first-class UI:

- **Variables** — `{user_name}` style templates with resolvers: static, env var, project path, file (Free) + db-query, computed expressions, MCP call (Pro).
- **Pre-call context hooks** — ordered list of resolvers that fire before each turn and inject results into the user message inside `<context>...</context>` tags.
- **Conversation summarizers** — per-agent memory policy (`summarizeAfter`, `keepLastK`, custom summarizer model). Long sessions auto-compact.
- **Multi-agent groups** — two types: **Routed** (router picks one child per prompt — keyword rules + LLM-classifier fallback) and **Sequential automation pipeline** (children run in `position` order, each agent's output feeds the next as input; cross-runtime chains like Claude → Codex → Gemini work natively).
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

### SDK (trace forwarder)

`@ato-sdk/js` is narrow-scoped: it forwards traces from agents you authored in ATO and deployed externally (Cloudflare Worker / Vercel / Docker / Node bundles) back to your ATO Insights dashboard. It is not a drop-in observability SDK for arbitrary production stacks.

| Provider | Wrapper | Import |
|----------|---------|--------|
| Anthropic | `wrapAnthropic(client)` | `@ato-sdk/js/anthropic` |
| OpenAI | `wrapOpenAI(client)` | `@ato-sdk/js/openai` |
| Claude Agent SDK | `wrapAgent(agent)` | `@ato-sdk/js/agent` |
| Custom provider in a bundle | `capture(trace)` | `@ato-sdk/js` |

Each forwarded trace records: model, tokens (input/output/cached), cost (USD), duration, status, errors, metadata. Built-in pricing for 60+ models. [Full SDK documentation](docs/SDK.md).

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
| Routed groups (router picks one) | up to 3 children | unlimited | unlimited + shared | unlimited |
| Sequential automation pipelines | up to 3 stages | unlimited | unlimited + shared | unlimited |
| Cross-runtime children in pipelines | ✅ | ✅ | ✅ | ✅ |
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
| **v2.2.1** | **Regression → Replay** — failing examples in the regression drill modal now have a one-click "Replay on…" button that re-dispatches the prompt against an alternative runtime + diffs side-by-side. Closes the loop the strategy alignment audit flagged as highest-leverage. |
| **v2.2.0** | **Real cost capture on the dispatch path** — Rust dispatch path computes tokens + cost at finish and persists them on `execution_logs` + `replay_jobs`. Compare / Cost Recs / Replay panels read the captured value instead of recomputing per render. |
| **v2.1.x** | **Replay infrastructure + deep regression detection + cost recommendations** — replay any cloud trace against a different runtime / model; configuration ledger joined with trace stats surfaces regressions with failing-example drill-down; cost-rec layer surfaces same-agent model swaps when historical data justifies them |
| **v2.0.0** | **External agents + hosted deployment** — Internal-vs-External agent toggle; deploy bundles for Cloudflare Worker / Vercel / Docker / Node across 9 chat-LLM providers; knowledge ingestion + RAG; embed widget; trace sink forwarding to Langfuse + OTLP webhook (complementary boundary baked into v2.0); Apple Developer signing + notarization |
| **v1.5.0–1.5.5** | **Daily workspace** — persistent chat threads (SQLite), streaming responses, syntax-highlighted markdown, file attachments, multi-runtime mid-thread, per-thread sticky agent, production-grade agent template with welcome tour, dynamic-prompt empty-state CTAs |
| **v1.4.0** | **Production-grade agent authoring** — Variables (F1), Context Hooks (F2), Summarizers (F3), Multi-agent Groups + Router + Graph Editor (F4), Per-task Models (F5), Observability + Trace Explorer (F6), Evaluators (F7), Tool Description Rewrite (F8); Pro tier gating; agent templates (5 starters); skill version history; bulk skill ops; runtime comparison tab; configuration export/import |
| **v1.3.0** | **The GUI Pivot** — IA collapse (24 → 6 sections), Home page, Create Agent (Guided + Quick), MCP install UI, embedded terminal (xterm + portable-pty), command palette (⌘K), subscriptions-or-keys auth model |
| **v1.2.0** | Visual workspace canvas, live execution visualization, skill palette, multi-select batch ops |
| **v1.1.0** | Projects dashboard, 6 runtimes (+ Gemini + OpenAI Agents SDK), Ollama provider, CodeMirror editor with conflict detection + inline lint, sandbox/policies management, backup/restore, file watcher, token chart, i18n (EN/PT/ES) |
| **v1.0.0** | SDK (`@ato-sdk/js` — narrow-scoped trace forwarder for ATO-authored agents deployed externally; not general-purpose LLM observability), web dashboard, cost tracking, LLM API key management, audit logging, agent monitor, SSO, Homebrew tap |

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
