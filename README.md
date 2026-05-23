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

## Why ATO

Claude Code, Codex, Gemini CLI already come with tools. They can grep your repo, read your files, run your tests, edit code. **That's the default now** — chat is a commodity and so are hands.

What you can't do without a cockpit:

- **Set rules across every runtime at once.** Read-only here. No network there. Repo-scoped everywhere. ATO speaks the permission flag each runtime understands (`--allowedTools` for Claude, the equivalent for Codex / Gemini) so one config governs them all.
- **Watch every tool call.** Per-dispatch receipts: prompt, runtime, model, every `read_file` / `grep` / `git_log` with arguments, every byte returned, files written via mtime-snapshot diff. Verified-via-N-tool-calls vs prompt-only badges per seat so you know which findings were checked against the code and which are vibes.
- **Compare what each AI actually did.** Side-by-side replay across runtimes. File attribution per dispatch. "Claude touched 3 files, Codex touched 5 — these two diverge here."
- **Kill a runaway.** Live runs registry, one-click kill, across every runtime.
- **Bring runtimes that don't have hands yet.** Hermes and OpenClaw are scaffolds — you wire their tools through ATO. Same receipts, same rules.

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
- **Automation pipelines** — sequential + routed (classifier picks the model) multi-stage dispatches. Claude → Codex → Gemini chains. Visual graph editor.
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

| | Free | Pro ($29/mo) |
|---|:---:|:---:|
| `ato review` with `read_file` / `grep` / `git_log` | ✓ | ✓ |
| War-rooms · sessions · cross-runtime bridge | ✓ | ✓ |
| Tool-call passthrough + permission rules | ✓ | ✓ |
| Replay · file attribution · live runs · kill | ✓ | ✓ |
| Receipts (local SQLite, AES-256 at rest) | ✓ | ✓ |
| MCP server (17 tools) | ✓ | ✓ |
| Cost optimizer (`optimize recommend`) | ✓ | ✓ |
| Tauri desktop · embedded terminal · skills marketplace | ✓ | ✓ |
| LAN mesh (mDNS peer discovery) | coming v2.9 | coming v2.9 |
| Quality scoring | heuristic pass/fail (regex + status-exit checks on the response) | **LLM judge — a second model rates each reply on our API key, not yours** |
| Cloud trace retention + regression alerts | — | 30-day cross-device |
| Scheduled evaluators | — | hourly / daily / weekly |
| Cloud-relay mesh (NAT traversal) | — | ✓ |
| Team sessions + shared war-rooms | — | ✓ |
| Priority support | — | ✓ |

**Free**: you fly the cockpit. **Pro**: ATO replays your prompts overnight, scores quality, and tells you what to switch.

**[Start free →](https://github.com/WillNigri/Agentic-Tool-Optimization/releases/latest)** · **[ATO Pro →](https://agentictool.ai/pro)**

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
