# ATO — Run any AI on your actual task

> **Run any AI on your actual task — see which one solved it cheapest and best, with receipts.**
>
> One command across Claude, Codex, Gemini, and 7 more runtimes. Local-first. MIT licensed.

```bash
brew install willnigri/ato/ato
```

<!-- terminal cast: ato dispatch claude "refactor this function" && ato dispatch codex "refactor this function" && ato review --consensus -->

[![Homebrew](https://img.shields.io/badge/homebrew-install-blue?logo=homebrew)](https://github.com/WillNigri/homebrew-ato) · [![Direct download](https://img.shields.io/badge/direct-download-green?logo=github)](https://github.com/WillNigri/Agentic-Tool-Optimization/releases) · [![MCP server](https://img.shields.io/badge/mcp-server-purple)](#mcp-server)

**Why now:** AI model performance varies wildly by task and budget. Manual comparison wastes time and leaves no audit trail. ATO makes runtime comparison a single command — with receipts (cost, tokens, diff, tool calls).

---

## What you'd use it for

| Use case | Example |
|---|---|
| **Code review across runtimes** | `ato review --consensus` — Claude, Codex, Gemini argue the diff, cite tool calls, surface disagreements inline |
| **Cost comparison** | Same prompt → 3 runtimes → table shows duration, tokens, $ per run. Cheapest model that solved it wins. |
| **Replay across runtimes** | Failing example from a regression? One click replays it against any runtime/model and diffs side-by-side. |
| **Strategy debates / pre-mortems** | Drop your LLMs into a war-room session and have them argue with each other while you push back. Every claim is cited against live files. |
| **Architecture decisions** | *"Postgres + queue vs. event-sourced — debate it against our existing services."* Tool-verified across N runtimes. |
| **Security & compliance audits** | Same primitive, scoped to threat-model files. Every "this looks fine" gets a tool-call citation. |

The decision-making engine and the audit trail are the same across all of them. Code review is the most-validated workflow today; the war-room patterns ride the same rails.

```bash
ato review --consensus \
  --reviewer @security-specialist \
  --reviewer @perf-reviewer \
  --reviewer google \
  --out review.md
```

Each reviewer runs in the same session — turn #2 sees #1's findings via history replay, no prompt re-pasting. Function-calling tools (`read_file`, `grep`, `git_log`) let the model walk the live repo instead of guessing. The audit log records which tool calls each LLM made, so the GUI can badge a reply `verified via 2 tool calls` vs `prompt-only`.

---

## 5-minute first run

After install, the desktop app drops you into a pre-loaded demo session with two LLMs comparing a refactor side-by-side. Want to skip straight to your own work?

```bash
# zero-config demo — uses your first 2 configured runtimes, or falls back to local Ollama
ato demo-compare

# real workflow
ato dispatch claude "your prompt here"
ato dispatch codex  "your prompt here"
ato review --consensus
```

Or from the GUI: ⌘K → "demo compare". Every run lands in `~/.ato/local.db` with cost, tokens, and tool-call trace.

> **Coding agent? Read [`AGENTS.md`](./AGENTS.md) first.** Every meaningful operation is reachable from the local CLI (`ato <command>`) or the stdio MCP server. The agent reads `AGENTS.md`, discovers ATO's surface, and operates it alongside the human. Local SQLite means zero network round-trip; agents never have to leave the machine.

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

### CLI only

```bash
brew install willnigri/ato/ato
ato --help
```

The CLI talks to the same `~/.ato/local.db` as the desktop app. Use either, both, or your coding agent shelling out — same data.

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

Exposes `run_agent` and 24 other tools — any MCP-aware runtime (Claude Code, Cursor agent mode, etc.) can dispatch to any ATO-managed agent regardless of native runtime. Cross-runtime by protocol, not by hack.

### SDK — narrow scope

```bash
npm install @ato-sdk/js
```

`@ato-sdk/js` is a **trace forwarder for ATO-authored agents deployed outside the desktop app** (Cloudflare Worker / Vercel / Docker / Node bundles). It is **not** a general-purpose LLM observability SDK.

If you have an existing production stack and want general LLM observability, use [Langfuse](https://langfuse.com), [Helicone](https://www.helicone.ai), [LangSmith](https://smith.langchain.com), [Arize Phoenix](https://phoenix.arize.com), or [Braintrust](https://www.braintrust.dev). They're built for that job. ATO is **complementary** to them — see [Relationship to other tools](#relationship-to-other-tools).

[Full SDK docs](docs/SDK.md).

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

Plus 15+ API providers: Anthropic, OpenAI, Google AI, Mistral, Groq, **xAI/Grok**, Together, Fireworks, DeepSeek, Qwen, MiniMax, Kimi, GLM, Yi, OpenRouter.

---

## Three audiences, one app

- **First-time users** — chat-style guided wizard ("describe what you want") suggests runtime, model, skills, MCPs. Or pick a starter template. Working agent in under two minutes.
- **Power users** — Quick form, command palette (⌘K), embedded `portable-pty` terminal, persistent threads, drag-drop file attachments, streaming responses, sequential automation pipelines.
- **AI coding agents** — every meaningful operation reachable from a local CLI (`ato <command>`) or a stdio MCP server. The agent reads [`AGENTS.md`](./AGENTS.md), discovers ATO's surface, and operates it alongside the human.

Bring your own auth: ATO rides your existing logged-in CLI subscriptions (Claude Code, Codex, Gemini CLI) the way VS Code rides your GitHub login — *or* you can use stored API keys. Your choice, per runtime.

---

## Local-first by design

ATO writes everything to `~/.ato/` on the developer's machine:

- **`~/.ato/local.db`** — SQLite database with every dispatch, replay, config change, agent definition, chat thread, skill registration. Agents can `sqlite3` query it directly for fast reads.
- **`~/.ato/agent-logs.jsonl`** — append-only JSONL log; grep-friendly.
- **`~/.ato/workflows/`**, **`~/.ato/cron-jobs.json`**, **`~/.ato/backups/`** — workflows, schedules, auto-backups.

Sign-in is **optional** and only unlocks cloud-side features (cross-device trace history, hosted LLM-as-judge, team workspaces). Every core operation — dispatch, replay, compare, file attribution, configuration ledger — works fully offline.

[**Sign in to unlock cloud sync, evaluators, and trace retention — free during beta →**](https://agentictool.ai/signup)

*Built in the open — star the repo if comparing AI runtimes sounds useful: [github.com/WillNigri/Agentic-Tool-Optimization](https://github.com/WillNigri/Agentic-Tool-Optimization)*

---

## What's in the box

### The compare-and-decide loop

- **`ato dispatch <runtime>`** — fire the same prompt at any runtime (CLI or API), record cost, tokens, duration, tool calls into `~/.ato/local.db`.
- **`ato review --consensus`** — multi-LLM code review. Each reviewer sees prior turns via session history, surfaces disagreements inline, cites tool calls.
- **`ato compare <run-a> <run-b>`** — post-hoc side-by-side of two execution_logs rows: duration delta, cost delta, response diff.
- **`ato demo-compare`** — zero-config first-run demo. Uses your first 2 configured runtimes, falls back to local Ollama, then to stubbed responses. Always shows the cost-comparison table.
- **`ato sessions`** — sticky multi-turn conversations. Cross-runtime by `--session <id>` + `--tag-bridge`. Auto-closes with coordinator-generated title/summary/tags.
- **Replay across runtimes** — failing example from a regression? One click re-dispatches the prompt against an alternative runtime + diffs side-by-side.

### Agent authoring

- **Variables** — `{user_name}` style templates with resolvers: static, env var, project path, file (Free) + db-query, computed expressions, MCP call (sign-in).
- **Pre-call context hooks** — ordered list of resolvers that fire before each turn and inject results into the user message inside `<context>...</context>` tags.
- **Conversation summarizers** — per-agent memory policy (`summarizeAfter`, `keepLastK`, custom summarizer model).
- **Multi-agent groups** — **Routed** (router picks one child per prompt) and **Sequential automation pipeline** (children run in `position` order, cross-runtime chains like Claude → Codex → Gemini work natively).
- **Per-task models** — distinct models for routing / summarizing / responding / evaluating.
- **Observability** — per-agent metrics (run count, p50/p95 latency, success rate), trace explorer with full sequence.
- **Evaluators** — heuristic kinds (contains / not-contains / length-range / tool-called) run locally; LLM-as-judge runs cloud-side (sign-in). Manual + scheduled batch — never live on every dispatch.

### Daily workspace

- Persistent chat threads (SQLite), streaming responses, syntax-highlighted markdown, file attachments.
- Multi-runtime mid-thread — switch Claude → Codex → Gemini in the same conversation.
- Embedded shell — real interactive PTY via `xterm.js` + `portable-pty`.
- i18n — English, Português, Español.

### Skills, MCPs, projects

- **Skills Manager** — per-runtime tabs, scope grouping, drag-to-prioritize, conflict detection, AI-powered creation.
- **MCP install UI** — curated registry (filesystem, github, postgres, slack, brave-search, gmail, calendar, …) with one-click install.
- **Projects dashboard** — memory hierarchy, skills, subagents, commands, hooks, permissions, MCPs. File watcher auto-refreshes.

### Settings

- Runtimes setup (CLI paths, SSH config, status checks) + Compare tab (per-runtime feature/config matrix).
- API Keys / Secrets / Environment — encrypted locally, OS-keychain-backed where applicable.
- Cloud — auth, teams, sync, notifications.
- Backup — JSON export/import of all your config.

### Cross-cutting

- **Command palette ⌘K** — global search across agents, skills, MCPs, projects.
- **i18n** — EN, PT, ES.

---

## Relationship to other tools

ATO is **complementary** to production-observability tools like [Langfuse](https://langfuse.com), [Helicone](https://www.helicone.ai), [LangSmith](https://smith.langchain.com), [Arize Phoenix](https://phoenix.arize.com), and [Braintrust](https://www.braintrust.dev) — not a competitor.

- **Those tools** instrument *deployed production stacks* via SDKs and log *end-user conversations* in real time.
- **ATO** covers the *developer side* of the same agent — what you dispatched while building, what you replayed across runtimes, what regressed after a config change, what each dispatch cost, which agent touched which files.

Most production teams use one from each camp: Langfuse / Helicone for production user-conversation logging, plus ATO for the developer/multi-runtime side. Production tools catch regressions in real user traffic; ATO catches regressions before you ship and lets you replay any failing example against an alternative runtime in one click.

vs. **Cursor / Continue / Cody** — those are *authoring* (write code with an AI in your editor). ATO is *operations* (dispatch, compare, replay).

vs. **Claude Code / Codex / Gemini CLI directly** — we're the GUI/orchestrator above them. They're the runtimes that do the work.

vs. **`/ultrareview`, CodeRabbit, Greptile** — code review is one of ATO's surfaces, not the headline. We're cross-runtime by default; they're tied to one provider.

---

## Architecture

```
apps/
  desktop/                 # Tauri 2.x desktop app (Rust + React)
  cli/                     # ato CLI binary (Rust)

packages/
  sdk/                     # @ato-sdk/js — narrow trace forwarder
  core/                    # Shared types, token utils
  db/                      # Database adapters

services/
  mcp-server/              # Standalone MCP server (25 tools)
```

### Cloud Backend (optional, sign-in for cloud features)

```
api.agentictool.ai
├── API Gateway       # Routing, JWT auth, rate limiting
├── Auth              # Register, login, GitHub OAuth, SSO/OIDC
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

# CLI
cd apps/cli && cargo run -- --help

# MCP server
npm run dev:mcp
```

Requires [Rust](https://rustup.rs/) and [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/).

---

## Engineering

- **CI/CD:** GitHub Actions runs `cargo check` + `cargo test` + `vitest run` + `vite build` on every PR
- **66+ Rust unit tests** + frontend Vitest tests
- **Code splitting:** Sidebar sections lazy-loaded via `React.lazy`
- **Accessibility:** ARIA labels on navigation, dialogs, dashboard tabs
- **Modular Rust:** types separate from commands

## Security

- **Local-first** — no network calls unless sync explicitly enabled
- **Parameterized SQL** — all queries
- **API keys** — AES-256-GCM encrypted locally under a macOS-keychain-backed master key
- **SSH** — OpenClaw uses key-based auth (paths only)
- **Validation** — all inputs validated with Zod / serde
- **Conflict detection** — content hashing prevents overwriting concurrent edits
- **Auto-backup** — every file write creates a timestamped backup, restorable from the UI
- **Audit trail** — every file write logged with diff stats and backup path

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE)

---

**[Website](https://agentictool.ai)** | **[Sign in (free during beta)](https://agentictool.ai/signup)** | **[SDK Docs](docs/SDK.md)** | **[Roadmap](ROADMAP.md)**

**MIT Licensed** | **Local-first** | **macOS, Windows, Linux**
