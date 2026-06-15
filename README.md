# ATO — The cockpit where every AI follows your rules

> **Your AIs already have hands. ATO is the seat you fly them from.**
> Set the rules. Watch every tool call. Kill the runaway. Compare what each one actually did to your code. Multi-runtime. Local-first. MIT.

```
$ ato review --reviewer claude --reviewer codex --reviewer gemini --against main
# illustrative output — your numbers (cost, duration, tool calls) will vary by runtime, model, and diff size

review session 7F3A1B6E · 3 reviewers · your rules: read-only, repo-scoped, killable

  CLAUDE     🔧 verified via 4 tool calls
             flagged 2 issues — XSS in src/render.ts:142, auth bypass in api/login.ts:87
             read_file ×3 · grep ×1 · 6.4s · $0.024 · 0 files written (read-only mode)

  CODEX      🔧 verified via 6 tool calls
             flagged 3 issues incl. SQL injection in db/queries.ts:64
             ← caught one claude missed by grep'ing for raw string concatenation
             read_file ×4 · grep ×2 · 7.1s · $0.004 · 0 files written

  GEMINI     ⚠️ prompt-only — didn't open the code
             flagged 2 issues from the diff alone (XSS×2, CSRF)
             5.9s · $0.001 · 0 tool calls · downweight in synthesis

  closer:    4 unique findings, 1 disputed.
             tags: security, sql-injection. cost: $0.029 total.
             every tool call, every byte read, archived to ~/.ato/local.db
             paste-ready transcript at .ato/reviews/7F3A1B6E.md
```

Three AIs all *could* have walked the code. **You see which ones did, and which one just replied.** That's the cockpit.

---

## In 30 seconds

```bash
# macOS
brew install willnigri/ato/ato && ato demo-war-room

# Linux
curl -fsSL https://agentictool.ai/install.sh | sh && ato demo-war-room

# Desktop app (macOS · Windows · Linux)
```

**[Download →](https://github.com/WillNigri/Agentic-Tool-Optimization/releases/latest)** (Tauri 2.x · MIT)

`demo-war-room` runs without API keys — picks your configured runtimes, falls back to Ollama. First receipt in 30 seconds.

---

## What's new in v2.18.0

- **Browse your team workspaces from any browser.** Sign in to your cloud account on the web — every shared session, war-room, chat, loop, and mission renders with the same fidelity as the desktop. Read-only by default; mobile-responsive.
- **Pair your browser to your desktop.** v2.17 tether: X25519 DH + AEAD, fingerprint-verified pairing. Your laptop becomes a secure oracle for the page you're looking at — no plaintext through the cloud relay.
- **Create + manage teams from the web.** New "+ New team", invite by email, role changes, danger-zone delete. Account page with profile + plan + sign-out. (LLM keys, runtimes, and skills still live in the desktop where the OS keychain is.)
- **`ato war-rooms sweep`** — auto-closes idle war-rooms with a coordinator-summary, single-JSON envelope output, clap-layer validators. Wire to cron and one-shot R1 reviews self-close.
- **`ato subagent log`** — Claude Code's Agent (Task) tool dispatches now show up in execution_logs alongside outer-session work. Canonical `auth_mode` / `billing_surface` vocab so analytics group cleanly. Git commit SHA captured per receipt.
- **Web sign-in + Onboarding redesigned.** Minimal centered sign-in card. Onboarding walks users through install / `.env` / wrap-the-client with explanations of *why* each step matters.
- **Deprecated Google models auto-filtered** from the chat picker (no more `gemini-2.0-flash-001` 404s).
- See [CHANGELOG.md](CHANGELOG.md) for the full list.

---

## Why ATO

Claude Code, Codex, Gemini CLI already come with tools. They can grep your repo, read your files, run your tests, edit code. **That's the default now** — chat is a commodity and so are hands.

What you can't do without a cockpit:

- **Set rules across every runtime at once.** Read-only here. No network there. Repo-scoped everywhere. ATO speaks the permission flag each runtime understands (`--allowedTools` for Claude, the equivalent for Codex / Gemini) so one config governs them all.
- **Watch every tool call.** Per-dispatch receipts: prompt, runtime, model, every `read_file` / `grep` / `git_log` with arguments, every byte returned, files written via mtime-snapshot diff. Verified-via-N-tool-calls vs prompt-only badges per seat so you know which findings were checked against the code and which are vibes.
- **Compare what each AI actually did.** Side-by-side replay across runtimes. File attribution per dispatch. "Claude touched 3 files, Codex touched 5 — these two diverge here."
- **Kill a runaway.** Live runs registry, one-click kill, across every runtime.
- **Bring API models in as full teammates.** Claude Code, Codex, Gemini CLI, Hermes, and OpenClaw already have hands — they ship with their own coding tool layer. The runtimes that don't are API providers without a first-party coding agent — Grok, MiniMax, DeepSeek, Qwen, GLM, Yi, Kimi — they hit a prompt-in, text-out endpoint with no built-in `read_file` / `grep` / `bash`. ATO wraps them with the same tool loop the CLI runtimes use, so a Grok or DeepSeek call can review code alongside Claude Code under identical rules + receipts. One war room, every seat with hands.

All of it local. AES-256 at rest, OS-keychain master key with a rotation ledger (v2.7.14+). No cloud round-trip unless you opt in.

---

## What you do in the cockpit

### 1. Multi-LLM code review — with shared findings

`ato review` dispatches your diff + full file context + git log to N reviewers in a shared session. Each one can call `read_file`, `grep`, `git_log` to walk the live repo under your rules. The second reviewer sees the first's findings — real consensus, not parallel monologues.

```bash
ato review --reviewer claude --reviewer codex --reviewer @security-specialist \
           --against main --consensus
```

Untrusted-file-content guard: tool returns are wrapped in `<UNTRUSTED_FILE_CONTENT>` so a file under review can't hijack the reviewer with prompt injection.

### 2. War-rooms — N LLMs, one shared room

Fire one prompt at N LLMs sharing a `--war-room-id`. A closer summarizes every reply with title, tags, category, and who agreed.

```bash
WR=$(uuidgen)
ato dispatch claude  "should we ship the Postgres migration before the freeze?" --war-room-id $WR
ato dispatch codex   "should we ship the Postgres migration before the freeze?" --war-room-id $WR
ato dispatch gemini  "should we ship the Postgres migration before the freeze?" --war-room-id $WR
ato war-rooms close  $WR --human
```

The room becomes one card in the GUI; click any seat to see its tool calls.

### 3. Sticky sessions with `@runtime` bridge

Multi-turn chats that span runtimes. `@gemini what do you think?` mid-thread bridges Gemini in; the bridge loops until `[CONSENSUS]` or the round cap.

```bash
ato sessions new --runtime claude --title "auth-rewrite"
ato dispatch claude "..." --session <id> --tag-bridge --max-rounds 3
```

### 4. Receipts that the pilot can actually read

Local SQLite at `~/.ato/local.db`. Every dispatch persists prompt, runtime, model, tokens, cost, duration, **files touched (mtime diff)**, every tool call with arguments, session id. The GUI renders them paste-ready.

```bash
ato dispatches list --human
ato files-touched <run-id>          # cross-run lineage per file
ato traces show <run-id>            # full transcript + tool-call audit
```

### 5. MCP server — 17 tools, your coding agent drives ATO

Claude Code, Codex, Cursor — any MCP-aware runtime can drive ATO directly: `run_agent`, `list_agents`, `get_context_usage`, `get_usage_stats`, `get_mcp_status`, skill management, runtime health, agent logs, cache. The runtime calls into the cockpit instead of you opening tabs.

```bash
npx ato-mcp
# add to ~/.claude/settings.json mcpServers
```

### And the rest of the cockpit

- **Replay any past trace** against a different runtime/model — side-by-side diff with duration + cost delta.
- **Compare workbench** — diff any two cloud traces of the same agent (duration, cost, files, ok-status).
- **Live runs registry + kill** — every in-flight dispatch with agent, runtime, workspace, elapsed; one-click kill.
- **Cross-runtime regression detection** — flags *"success rate dropped 17pp after the model swap"* by joining the config-change ledger with trace windows.
- **Cost optimizer** — `ato optimize recommend --human`. Concrete swaps with quality guards (≥30% cheaper, ok-rate within 10pp, eval-score within 5pp).
- **Loop Composer** (v2.14) — visual + CLI graph editor for **LLM workflow loops**. First-class node types: dispatch, methodology run, diagnose, apply (Goodhart-defended), review, war-room, score. Persisted SQLite, scriptable via `ato loop run <slug>` for headless boxes and MCP agents. *"Weekly: run methodology X, diagnose failures, apply the patch, re-run on the holdout, alert on regression"* is one loop. Reframed from Automations — same node-graph muscle, but the palette is LLM-aware. Reads the moment as Peter Steinberger put it: design loops that prompt your agents, not prompts in isolation.
- **Embedded terminal** — xterm.js + portable-pty, scoped to active project, persistent across navigation.
- **Dynamic prompts** — resolvers: env / file / SQL / MCP / computed JS.
- **Agent wizard + skills marketplace** — writes the right file per runtime (`~/.claude/agents/`, `~/.codex/agents/`, `<proj>/.gemini/agents/`, `~/.openclaw/agents/`, `~/.hermes/agents/`).
- **SSH remote runtimes** — `ato runtimes add-remote` for laptop→server dispatches (v2.3.32).
- **Eval-score ratchet** — `ato ratchet check` for CI gates (v2.6.x).
- **External agent deploys** — bundle generators for Cloudflare Worker, Vercel Edge, Docker, Node + embed widget; customer's API key.

---

## After ~20 real dispatches: ATO recommends switches

```bash
ato optimize recommend --human
```

```
Cost Recommendations (based on YOUR data)

  1. Switch CLAUDE → GOOGLE on @code-writer
     Savings: 85% per round ($0.0245 → $0.0037)
     Evidence: head-to-head rounds from your local trace database
     Quality guard: ok-rate within 4pp · eval-score within 2pp · PASS
```

Computed from rounds where both runtimes answered the same prompt and a judge scored both. Not vibes. **Receipts.**

This is **chapter 2** — what happens once the cockpit has data on YOUR work. The cockpit is the daily-use loop; cost recs are the payoff.

---

## Free vs Pro

**The principle**: you can run every primitive yourself for free. We charge for the codified automation we package on top. Same model as GitLab, Sentry, Supabase. Full mapping in [`docs/tiers.md`](./docs/tiers.md).

| | Free | Pro ($29/mo) | Team ($49/mo) |
|---|:---:|:---:|:---:|
| `ato review` with `read_file` / `grep` / `git_log` | ✓ | ✓ | ✓ |
| War-rooms · sessions · cross-runtime bridge | ✓ | ✓ | ✓ |
| Tool-call passthrough + permission rules | ✓ | ✓ | ✓ |
| Replay · file attribution · live runs · kill | ✓ | ✓ | ✓ |
| Receipts (local SQLite, AES-256 at rest) | ✓ | ✓ | ✓ |
| MCP server (27 tools incl. 10 methodology) | ✓ | ✓ | ✓ |
| Cost optimizer (`optimize recommend`) | ✓ | ✓ | ✓ |
| Tauri desktop · embedded terminal · skills marketplace | ✓ | ✓ | ✓ |
| **Methodology runner** (`create / run / adopt / score / runs / margin / calibrate`) | ✓ | ✓ | ✓ |
| Methodology Insights panel (per-cell stats + Welch t + p-values + 95% CI) | ✓ | ✓ | ✓ |
| Workspaces (local, multi-namespace) | ✓ | ✓ | ✓ |
| LAN mesh (mDNS peer discovery) | ✓ | ✓ | ✓ |
| Quality scoring | regex / structural / your own LLM-judge with your key | **`methodology diagnose`: codified learning loop** | (Pro features) |
| **`methodology schedule create`** (auto-rerun on cron) | — *(DIY with crontab)* | ✓ | ✓ |
| **`methodology diagnose`** (read failing cells → propose agent change → A/B test) | — *(DIY with `ato dispatch`)* | ✓ | ✓ |
| Cloud trace retention + regression alerts | — | 30-day cross-device | ✓ |
| Scheduled evaluators (cron-driven cloud evals) | — | ✓ | ✓ |
| Cloud sync of methodologies + runs across devices | — | ✓ | ✓ |
| Cloud-relay mesh (NAT traversal) | — | ✓ | ✓ |
| Auto-revert watch (7-day Langfuse trace monitor) | — | ✓ | ✓ |
| Auto-PR after A/B wins | — | ✓ | ✓ |
| Team workspaces (multi-user shared agents + skills) | — | — | ✓ |
| Encrypted provider key store (cron usage-poller) | — | — | ✓ |
| Priority support | — | ✓ | ✓ |

**Why this split**: every Pro row is automation we built on top of the free primitives. You can build the same loop with `ato dispatch` + bash + your own LLM prompts — you just don't get OUR button. The principle is *we charge for the codified workflow, not the underlying capability.*

**[Start free →](https://github.com/WillNigri/Agentic-Tool-Optimization/releases/latest)** · **[ATO Pro →](https://agentictool.ai/pro)** · **[Full tier mapping](./docs/tiers.md)**

---

## Supported runtimes

**CLI (tools built-in):** Claude Code · Codex · Gemini CLI · Ollama
**CLI (bring-your-own toolset, same rules):** OpenClaw · Hermes
**API:** OpenAI · Google AI · Anthropic · Mistral · Groq · xAI · DeepSeek · Qwen · MiniMax · OpenRouter · Together · Fireworks · Kimi · GLM · Yi

ATO rides your existing CLI logins the way VS Code rides your GitHub login — or use stored API keys. **Bring your own keys. ATO never holds inference compute.**

---

## Complementary, not competing

ATO is your **local cockpit** — the dev-workflow side of multi-runtime AI work. For production SDK observability on shipped apps, use [Langfuse](https://langfuse.com), [Helicone](https://www.helicone.ai), or [LangSmith](https://smith.langchain.com). Most production teams run one from each camp — they cover different sides of the same agent.

For IDE-embedded coding (Cursor / Continue), ATO sits next to them: you keep your IDE, you fire a war-room when one model isn't enough.

---

## Docs

**[CLI Reference](AGENTS.md)** · **[Architecture](docs/architecture.md)** · **[Contributing](CONTRIBUTING.md)** · **[Roadmap](ROADMAP.md)**

---

**[Website](https://agentictool.ai)** · **[ATO Pro](https://agentictool.ai/pro)** · **[GitHub](https://github.com/WillNigri/Agentic-Tool-Optimization)** · MIT Licensed
