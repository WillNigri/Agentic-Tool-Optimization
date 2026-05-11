# AGENTS.md — Using ATO from a Coding Agent

> This file is for **AI coding agents** (Claude Code, Codex, Cursor agent mode, Aider, OpenClaw, Hermes, and similar). If you're a human, see [`README.md`](./README.md) instead.
>
> ATO is the local-first developer-workflow operations platform for multi-runtime AI agents. Humans and their coding agents share the same cockpit. This doc tells you (the agent) how to use it.

---

## 1. What ATO is, in one paragraph

ATO sits on the developer's machine. It traces every dispatch (Claude / Codex / Gemini / OpenClaw / Hermes / Ollama / 16+ API providers), joins config changes against trace stats to surface regressions, captures real per-million-token cost, supports cross-runtime replay, and persists everything in a local SQLite database. You operate it through a CLI, an MCP server, or by reading the SQLite database directly. No cloud round-trip is required for any core operation.

## 2. The three actor surfaces

You can drive ATO three ways, in order of recommendation:

| Surface | Best for | How |
|---|---|---|
| **CLI** | Most agent operations. Lower latency than MCP, composable with shell, every coding agent already knows how to shell out. | `ato <command> [flags]` → JSON to stdout |
| **MCP (stdio)** | When you're in an MCP-enabled harness (Claude Code, etc.) and tool-call integration is already wired | Connect to `ato-mcp` over stdio; call tools by name |
| **Direct SQLite** | Fast read-only queries when you want zero overhead | `sqlite3 ~/.ato/local.db "SELECT ..."` |

**Default to CLI.** It is the surface ATO is most optimized for agent use. MCP is for harness integration. Direct SQLite is for power reads.

## 3. Local file layout — where data lives

ATO writes everything to `~/.ato/`. Read these directly if you need to:

| Path | Contents |
|---|---|
| `~/.ato/local.db` | SQLite database. Tables include `execution_logs`, `agent_traces`, `replay_jobs`, `agent_config_changes`, `agents`, `agent_variables`, `agent_hooks`, `chat_threads`, `chat_messages`, more. |
| `~/.ato/agent-logs.jsonl` | Append-only JSONL log of every dispatch. Useful for grep-style queries. |
| `~/.ato/workflows/` | Saved automation workflows (JSON). |
| `~/.ato/cron-jobs.json` | Scheduled cron jobs. |
| `~/.ato/backups/` | Auto-backups of config files before any write. Auto-pruned at 30 days. |
| `~/.ato/skills/` | User-authored skills (your own SKILL.md files can go here, or per-runtime under `~/.claude/skills/`, `~/.codex/skills/`, etc.) |
| `~/.ato/recipes/` | User-authored ops recipes (trigger→action workflows). Reserved for v2.3+. |

## 4. Setup — how to make ATO usable in this session

If the developer hasn't already set ATO up:

```bash
# Check if ATO is installed
ato --version

# If not, the human runs:
brew tap WillNigri/ato && brew install --cask ato      # macOS
# or downloads from agentictool.ai
```

**First-run PATH setup.** After installing ATO from a desktop bundle (DMG, AppImage, NSIS, .deb), the CLI is shipped inside the bundle but **may not be on PATH yet**. The human runs:

```bash
ato setup-path
# Symlinks the CLI binary to /usr/local/bin/ato (or ~/.local/bin/ato as fallback).
# Outputs JSON describing what it did. Idempotent — safe to run repeatedly.
```

If your `which ato` already returns a path, this is a no-op. If `ato` isn't on PATH after setup, you're a coding agent reading this who can't use ATO yet — surface this to the human:

> "ATO's CLI isn't on PATH. Run `ato setup-path` from a terminal where the desktop bundle is reachable (or use `/Applications/ATO.app/Contents/Resources/binaries/ato-<your-arch> setup-path` on macOS to bootstrap)."

Once installed, you can use the CLI immediately. No sign-in required for local operations.

For MCP, the human needs to add ATO's MCP server to their harness config once:

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

After that, every MCP-aware coding agent the human runs has ATO's tools available.

## 5. CLI reference

All commands output JSON to stdout by default. Add `--human` for human-readable formatting. Add `--quiet` to suppress non-essential output. Add `--db /custom/path.db` to point at a non-default SQLite path (rarely needed).

> **Status legend:** `[v2.1]` = shipped today. `[v2.3+]` = on the roadmap; not yet available. Use `ato --version` to check what's live on this machine. If a command isn't there yet, fall back to the equivalent MCP tool or read SQLite directly.

### Observation

```bash
# Recent dispatches (default: last 20)
ato dispatches recent [--limit N] [--runtime claude|codex|...] [--status success|error]
# [v2.3+]

# Active dispatches (currently running)
ato runs live
# [v2.3+]

# Specific run by ID
ato runs get <run-id>
# [v2.3+]

# Configuration changes for an agent (the ledger)
ato config-changes list --agent <slug> [--since 7d]
# [v2.3+]

# Regressions detected (joins config changes × trace stats)
ato regressions list [--days 7|30|90] [--severity regression|improvement]
# [v2.3+] — local-mode planned; today requires cloud sign-in via the GUI

# Failing examples for a specific regression
ato regressions failing-examples <change-id>
# [v2.3+]

# Cost recommendations (when historical multi-runtime data justifies a swap)
ato cost recommendations [--agent <slug>]
# [v2.3+]

# File attribution for a specific dispatch
ato files-touched <run-id>
# [v2.3+]

# Replay history for a trace
ato replays for-trace <trace-id>
# [v2.3+]
```

### Operations

```bash
# Dispatch a prompt to a runtime
ato dispatch <runtime> "<prompt>" [--model <model>] [--agent <slug>]
# [v2.3+]

# Start a replay of a past trace on a different runtime
ato replay start <trace-id> --runtime <target> [--model <model>]
# [v2.3+]

# Get the result of a replay (poll until done)
ato replay get <job-id> [--wait]
# [v2.3+]

# Kill a stuck dispatch
ato kill <run-id>
# [v2.3+]

# Bulk kill (kill all matching pattern)
ato kill --all [--runtime <runtime>] [--older-than 5m]
# [v2.3+]

# Compare two traces
ato compare <trace-id-a> <trace-id-b>
# [v2.3+]
```

### Authoring (write things back)

```bash
# Draft a skill from a successful replay
ato skills draft --from-replay <job-id> [--out ~/.claude/skills/my-skill/SKILL.md]
# [v2.3+]

# Create a new agent
ato agents create --slug <slug> --runtime <runtime> --system-prompt "<prompt>"
# [v2.3+]

# Update an agent's model
ato agents update <slug> --model <model>
# [v2.3+]

# Create an ops recipe (trigger → action workflow)
ato recipes create --name <name> --trigger <event> --action <action>
# [v2.3+]
```

### Events (long-lived subscriptions)

```bash
# Watch ATO events. Blocks until ^C or --until fires.
ato events watch [--type regression|dispatch_failed|replay_done|cost_threshold]
# [v2.3+]
# Emits one JSON event per line to stdout. Pipe to your handler:
# ato events watch --type regression | while read line; do ... done
```

### Reading the database directly

For zero-overhead reads, just query SQLite:

```bash
# Most recent dispatches
sqlite3 -json ~/.ato/local.db "
  SELECT id, runtime, status, duration_ms, cost_usd_estimated, created_at
  FROM execution_logs
  ORDER BY created_at DESC
  LIMIT 20
"

# Configuration changes since yesterday
sqlite3 -json ~/.ato/local.db "
  SELECT agent_slug, field, old_value, new_value, changed_at
  FROM agent_config_changes
  WHERE changed_at > datetime('now', '-1 day')
  ORDER BY changed_at DESC
"
```

The SQLite schema is documented at the top of `apps/desktop/src-tauri/src/lib.rs`.

## 6. MCP server reference

Connect over stdio. Tools below are available today (v2.1.x) or planned. Use `tools/list` to see what's actually exposed on the running server.

### Currently exposed (v2.1.x)

| Tool | What it does |
|---|---|
| `get_context_usage` | Context window breakdown for the current project |
| `list_skills` | Per-runtime skill listing |
| `toggle_skill` | Enable/disable a skill |
| `get_usage_stats` | Token / cost analytics |
| `get_mcp_status` | MCP server health |
| `get_runtime_status` | Health check for any single runtime |
| `get_all_runtime_statuses` | Health check across all runtimes |
| `get_agent_logs` | Execution logs (filterable by runtime) |
| `run_agent` | Dispatch any ATO-managed agent regardless of native runtime |

### Planned expansion (v2.3+)

Mirrors the CLI reference. Each CLI subcommand has an equivalent MCP tool:

- **Observation:** `get_recent_dispatches`, `get_active_runs`, `get_regressions`, `get_failing_examples`, `get_config_changes`, `get_cost_recommendations`, `get_file_attribution`, `get_replay_history`
- **Operations:** `start_dispatch`, `start_replay`, `get_replay_job`, `kill_run`, `bulk_kill`, `compare_traces`
- **Authoring:** `draft_skill_from_replay`, `create_agent`, `update_agent_config`, `create_ops_recipe`
- **Events:** `subscribe_to_events`, `acknowledge_event`

## 7. Common recipes — patterns to copy

These are example workflows you can run on the human's behalf when given the goal. They show how to compose the primitives.

### Recipe: "Skillify a regression"

The human shipped a config change; quality dropped on N examples. The replay shows the previous runtime would have handled some of them correctly. Make the fix structural so it can't recur.

```bash
# 1. Find the most recent regression
REG=$(ato regressions list --severity regression --days 7 | jq '.[0]')
CHANGE_ID=$(echo $REG | jq -r '.change_id')
AGENT=$(echo $REG | jq -r '.agent_slug')

# 2. Get the failing examples
ato regressions failing-examples $CHANGE_ID > failing.json

# 3. Replay the first failing example on the runtime that was working before
PROMPT=$(jq -r '.[0].prompt' failing.json)
OLD_RUNTIME=$(echo $REG | jq -r '.old_value')
JOB_ID=$(ato replay start $(jq -r '.[0].trace_id' failing.json) --runtime $OLD_RUNTIME)

# 4. Wait for replay
RESULT=$(ato replay get $JOB_ID --wait)

# 5. If replay succeeded, draft a skill that routes this prompt pattern
#    back to the old runtime
if [ $(echo $RESULT | jq -r '.status') = "done" ]; then
  ato skills draft --from-replay $JOB_ID --out ~/.claude/skills/route-${AGENT}/SKILL.md
fi

# 6. Tell the human in the activity feed
echo "{\"type\":\"agent_post\", \"text\":\"Drafted skill from regression of @${AGENT}. Review at ~/.claude/skills/route-${AGENT}/SKILL.md.\"}" | ato events post
```

### Recipe: "Watch this dev session for regressions"

The human is about to make a series of edits. Stand by and notify them if anything goes wrong.

```bash
# Subscribe to regression events, exit after 60 minutes
timeout 3600 ato events watch --type regression | while read event; do
  AGENT=$(echo $event | jq -r '.agent_slug')
  DELTA=$(echo $event | jq -r '.eval_delta_pp')
  echo "{\"type\":\"agent_post\",\"text\":\"⚠️ Regression on @${AGENT}: eval score dropped ${DELTA}pp. Want me to investigate?\"}" | ato events post
done
```

### Recipe: "Check if there's a cheaper way before this dispatch"

Before running an expensive prompt, see whether ATO's cost recommendations suggest a swap.

```bash
ato cost recommendations --agent $AGENT_SLUG | jq '.[] | select(.savings_per_month_usd > 10)'
```

## 8. Safety — what's destructive

| Operation | Destructive? | Approval pattern |
|---|---|---|
| `dispatches recent`, `runs live`, all `get_*` reads | No | Just go |
| `dispatch`, `replay start` | Spends real LLM tokens. Mildly destructive (cost). | Run, but log the cost back to the human |
| `kill <run-id>`, `bulk kill` | Cancels in-flight work. Hard to undo. | **Ask the human first** unless it's a clearly-stuck run (older than the configured threshold) |
| `skills draft` to disk | Writes a file. Auto-backed up before overwrite. | Run; tell the human where you wrote |
| `agents create`, `agents update` | Writes to SQLite + per-runtime config file. Audit-logged + auto-backed-up. | Run unless the human said don't |
| `recipes create` | Adds an automation that fires on future events. Could amplify mistakes. | **Ask the human first.** Show them the recipe; let them approve. |
| Direct SQLite `UPDATE`/`DELETE` | Bypasses audit + backup. | **Don't.** Use CLI or MCP tools so the change is logged. |

Every write through the CLI or MCP automatically appends to `~/.ato/agent-logs.jsonl` with timestamp, actor, operation, and (when applicable) the diff. The human can grep this any time to see what their agent did.

## 9. How ATO fits with other tools (so you don't double-instrument)

- **Langfuse / Helicone / LangSmith / Phoenix / Braintrust:** these are *production observability* SDKs. They log end-user conversations from a deployed app. ATO is the *developer-workflow* side. Use both: Langfuse logs your live users; ATO is the cockpit for what you build and replay during development. Don't try to send Langfuse data to ATO or vice versa; they cover different sides of the same agent.
- **Cursor / Continue / Cody:** these are authoring tools (write code with AI in your editor). ATO is operations (operate the agents). Coexist.
- **Aider:** single-runtime CLI. ATO sits above it for multi-runtime workflows. They coexist.
- **Hermes Agent's skill_manage:** Hermes creates skills autonomously; ATO is the verification layer (run tests against historical traces, surface regressions). Complementary.

## 10. Asking for help

If a command isn't documented here, try:

```bash
ato --help
ato <command> --help
```

If the CLI doesn't have what you need, fall back to MCP (`tools/list`) or read SQLite directly. If neither has it, the operation probably doesn't exist yet — tell the human, and don't fabricate output.

If you (the agent) are uncertain whether an operation is safe in the human's context, **ask before acting.** ATO is built for human + agent co-piloting; the human is one message away.

---

*Spec version: 2026-05-11. Reflects ATO v2.2.1 (current) + the v2.3 platform-expansion plan. Commands marked `[v2.3+]` are documented for the agent's planning purposes but not yet shipped.*
