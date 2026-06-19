# ATO — Agent Playbook

> **Audience.** This doc is for **LLMs driving ATO** — Claude in Claude Code, Codex in the Codex CLI, Gemini, a future agent over MCP. If you're a *human* reading this, the same content applies, you just won't get the second-person framing.
>
> **Why this file exists.** ATO already gives API providers (Gemini / OpenAI / MiniMax / Anthropic) a function-calling tool loop — `provider_supports_tools()` in `apps/cli/src/api_dispatch_tools.rs:243`. But every LLM that's driven ATO so far has *missed* this and reached for one-shot text instead. The on-disk help text mentions the flag; the human-facing docs explain the policy; nothing tells **you** specifically how to invoke it. This file does.
>
> **Receipts are the whole pitch.** The product story (blog Parts 1-7 below) is "every AI follows your rules" — *receipts* of what tools each model actually called, not vibes. If you dispatch one-shot text when grounded mode was available, you're skipping the receipt and breaking the pitch.

## The blog you should read first (background)

The v2.9 grounded-mode build log, in order:

| Part | What it covers | URL |
|---|---|---|
| Part 1 | PR-1: `GroundingPolicy` module, dispatch flag surface, verdict computation. Bench: +1 upgrade (gemini hallucination captured), -1 regression (claude false negative). | https://agentictool.ai/posts/we-used-ato-to-test-ato-part-1.html |
| Parts 2-3-4 | Closing the claude false-negative by parsing `--output-format json` tool-use; routing **every API-provider dispatch through `api_dispatch_tools.rs`** when grounding is non-`off` (this is the headline change); parserless runtimes refuse-with-options. | https://agentictool.ai/posts/we-used-ato-to-test-ato-parts-2-3-4.html |
| Part 5 | The n=150 corpus methodology, why it's credible. | https://agentictool.ai/posts/we-used-ato-to-test-ato-part-5.html |
| Part 6 | v2.10 methodology runner shipped; first end-to-end "receipts plus runner" demo. | https://agentictool.ai/posts/we-used-ato-to-test-ato-part-6.html |
| Part 7 | v2.11 learning loop — Goodhart-defense via holdouts; cross-LLM diagnose validates the design for $0.086. | https://agentictool.ai/posts/we-used-ato-to-test-ato-part-7.html |

Part 3 is the load-bearing one for tool access on API providers.

## The two-binary trap (read this BEFORE you dispatch)

There are two `ato` binaries on a typical install:

| Binary | Version (today) | Can dispatch claude/codex/gemini/openai/minimax? | Has `--require-tools`? | Can read keychain-stored API keys? |
|---|---|---|---|---|
| `/Applications/ATO.app/Contents/MacOS/ato` (**PROD**) | 2.7.4 (until next prod ship) | ✓ — all of them, including gemini via Google API | ✗ if pre-v2.9 — flag errors as unknown | ✓ |
| `/opt/homebrew/bin/ato` (often symlinks to a dev `cargo build --release` artifact) | whatever was last built | ✓ if your build is current | ✓ if v2.9+ | ✗ — adhoc-sign breaks the keychain ACL and CAN silently rotate the master key, orphaning every stored ciphertext |

**HARD RULE (Will, 2026-06-10):** for any dispatch that needs API keys (i.e. any LLM dispatch), shell through `/Applications/ATO.app/Contents/MacOS/ato`. Not the PATH-resolved one. If you rebuild the dev binary and use it for dispatches, you will at minimum hit "Failed to decrypt the stored API key" and at worst rotate the master key and break every encrypted row. The 2026-05-17 incident is in `memory/feedback_dev_build_keychain.md`.

**What this means for war-rooms TODAY (v2.7.4 prod binary):**

- `claude` (CLI), `codex` (CLI), `gemini` (Google API fallback) all dispatch fine — verified in the v2.14 Loop Composer war-rooms.
- `--require-tools` will be rejected by the v2.7.4 binary as an unknown flag, so gemini runs text-only (no function-calling loop). That's fine for **strategy / scope / positioning / pricing** seats — they don't need to walk code.
- For **code-review** seats today: use `codex` (its CLI runs its own tool loop, no flag needed). If you have a strong reason to want gemini on a code review specifically, inline the source bytes in the prompt as a fallback and name the methodology gap in the audit trail.
- Shipping a new prod app at v2.13/v2.14 unlocks `--require-tools` on gemini/openai/minimax/anthropic via the prod binary. That's a desirable backlog item (see v2.14.x list below), but it is NOT a precondition for using gemini in war-rooms today.

```bash
# ALWAYS this, never `ato dispatch ...` directly:
export PROD_ATO=/Applications/ATO.app/Contents/MacOS/ato
"$PROD_ATO" dispatch claude --human "your prompt"
```

## When to pass `--require-tools` (i.e. give the LLM tool access)

`ato dispatch` has these grounded-mode flags:

- `--require-tools <comma-list>` — model MUST call at least one of these tools (e.g. `read_file,grep`). Tightens only; always accepted.
- `--require-paths <comma-list>` — model MUST `read_file` at least one path matching these globs.
- `--mode-override strict|soft|off` — temporary policy override for this dispatch.

When ANY of those are set on a dispatch to an API provider (`gemini` / `openai` / `minimax` / `anthropic`), `with_tools` flips to `true` and the dispatch routes through `api_dispatch_tools.rs::dispatch_with_tools()`. The provider's function-calling loop fires; the model can call `read_file`, `grep`, `git_log`; every call lands in `execution_logs.tool_calls_summary` natively.

### Decision table — should you pass `--require-tools`?

| Dispatch purpose | Pass `--require-tools`? |
|---|---|
| Code review, security audit, PR diff scrutiny | **YES — `--require-tools read_file,grep`**. The seat needs to walk the source. |
| War-room seat reviewing a code chunk or design doc that cites files | **YES** |
| War-room seat on strategy / scope / positioning / pricing | NO — pure priors voice |
| Methodology run cell (single-shot prompt → score) | NO — the methodology runner has its own grounded-mode story |
| Adversarial challenge / "10-star reframe" | NO — value is unblocked priors, not file access |

### The dispatch that codex-the-CLI-binary already does for free

When you dispatch to `codex` or `claude` (CLI runtimes), tool access is **always on** — the CLI itself runs the tool loop. You don't need `--require-tools` for them. The flag is specifically for the **API-only fallback path** where the CLI binary isn't installed and ATO routes to the provider's HTTP API.

Cheat sheet for the war-room (CEO is `claude` in your Claude Code session):

```bash
WR=$(uuidgen)

# Codex seat — tool-capable native CLI. No --require-tools needed.
"$PROD_ATO" dispatch codex --war-room-id "$WR" --human "<brief>"

# Gemini seat — CLI usually not installed; ATO falls back to Google API.
# Pass --require-tools so the API path fires the tool loop. Without this,
# gemini receives one-shot text and can't read the source you're asking about.
"$PROD_ATO" dispatch gemini --require-tools read_file,grep --war-room-id "$WR" --human "<brief>"

# Code-touching seat where you also want git context:
"$PROD_ATO" dispatch gemini --require-tools read_file,grep,git_log --require-paths "apps/cli/src/**" --war-room-id "$WR" --human "<brief>"
```

## How to read receipts after a dispatch

Receipts answer: *did the model actually use tools, or did it hallucinate?*

```bash
"$PROD_ATO" dispatches show <execution_log_id> --human
```

Look for:

- `grounded: verified | ungrounded | not_enforced` — the verdict
- `tool_calls_summary` — every tool call the model made, with arguments
- `required_tools` / `required_paths` — what the policy demanded
- `unmet_rules` — empty when verified, populated when ungrounded

For a war-room or loop run, the full set of dispatches all share a `war_room_id` (or `loop_run_id` via `loop_run_steps.execution_log_id`) — so you can scan them as a bundle:

```bash
"$PROD_ATO" dispatches list --war-room "$WR" --human
# Each row shows: model, status, tool-call count, badge.
```

**If a seat returns a confident-sounding reply but its receipt shows zero tool calls and your dispatch wanted code review, the reply is unverified.** Surface this in the war-room synthesis explicitly — that's the whole pitch.

## When you're driving a Loop (v2.14)

Loop steps of kind `dispatch` (and `review`, `war_room`, etc.) write a `loop_run_steps` row. The row's `execution_log_id` joins to `execution_logs` for the underlying receipt. Today (v2.14.0) the loop's dispatch handler doesn't pass `--require-tools` automatically — that's a v2.14.x backlog item. Until then, the LOOP DESIGNER (you, configuring the node) is responsible for putting the grounded-mode flags into the node's config so the executor passes them through. See `apps/cli/src/commands/loops.rs::handle_dispatch` for what fields are read from the node's `config.params`.

## v2.14.x backlog (so you know what's missing — don't waste a session re-discovering)

1. **Ship a new prod app build at v2.13/v2.14** so the prod binary recognizes `--require-tools` / `--require-paths` / `--mode-override`. The v2.7.4 prod binary already dispatches every provider (gemini, openai, minimax, anthropic, claude, codex) fine — what's missing is the grounded-mode flag surface. Nice-to-have, not blocking.
2. **`ato runtimes health` should label the dispatch route** — today says "gemini missing" when the CLI binary isn't on PATH, even though the Google API fallback works fine. Should say: `gemini: api-fallback (google API), tool-loop available when --require-tools is set`.
3. **`ato providers status`** — one-shot truth table of every provider: working / dead / no-key / over-quota. The current way to discover this is to dispatch and read the error, which is wasteful.
4. **`--tools` shorthand on `ato dispatch`** — flip `with_tools=true` without requiring a `--require-tools` policy. Many dispatches just want "let the model use tools if it wants" without enforcing a minimum.
5. **Loop executor should pass grounded-mode flags through** from the node's config to `dispatch::run()`. Today the handler reads `runtime / prompt / model / agent_slug` but ignores any grounded-mode fields in `config.params`.
6. **Receipt surface in loop_run_steps** — the `tool_calls_summary` should be visible in `ato loop runs show` as a per-step badge. Today the runtime captures `execution_log_id` but the human renderer doesn't follow the FK.
7. **Update the `ato-warroom` skill** — the current SKILL.md (in `ato-cloud/.claude/skills/ato-warroom/`) tells agents that "API-only providers can only reason from what's in the prompt." That sentence shipped before v2.9 grounded mode and is now wrong. Replace with a reference to this playbook.

## Team sharing & real-time participation (v2.18.7, Team tier)

If you are operating inside a Team workspace you can share any war-room, session, or chat with your team and then have every teammate's machine receive live updates as new turns are appended — no refresh, both directions.

### What "share" means

- A shared item renders in every teammate's Sessions feed (Team filter) as the **same rich card** as a local one — title, summary, tags, coordinator, seats, runtime badges — plus a 👥 TEAM badge and "shared by <name>". The owner sees one card, not a duplicate.
- Appending a turn to a shared item (via CLI or the UI) immediately pushes the update live to all connected members via HTTP → Postgres NOTIFY → WebSocket.
- Requires **Team tier** and a signed-in prod binary (`ato login`). Always shell through `/Applications/ATO.app/Contents/MacOS/ato` — not the PATH-resolved dev build — so the keychain auth is available.

### CLI commands

**Share a war-room / session / chat into a team:**

```bash
PROD_ATO=/Applications/ATO.app/Contents/MacOS/ato

"$PROD_ATO" war-rooms share <id> --team <slug>
"$PROD_ATO" sessions  share <id> --team <slug>
"$PROD_ATO" chats     share <id> --team <slug>
```

Each command builds a full snapshot so the card renders identically to a UI share.

**Inspect and remove shares:**

```bash
"$PROD_ATO" war-rooms list-shared --team <slug>
"$PROD_ATO" war-rooms unshare <id> --team <slug>
```

**Append a turn programmatically (appears live for all teammates):**

```bash
"$PROD_ATO" war-rooms append-event <id> --team <slug> \
    --kind dispatch_result \
    --json '{"seat":"claude","summary":"Recommends the migration. Receipts attached."}'
```

`--kind` accepts the same event kinds used internally (e.g. `dispatch_result`, `close_summary`). The payload in `--json` is free-form but should match the kind's expected shape so the desktop card renders it correctly.

### Typical agent workflow

```bash
PROD_ATO=/Applications/ATO.app/Contents/MacOS/ato
WR=$(uuidgen)

# 1. Run the war-room as usual.
"$PROD_ATO" dispatch claude --war-room-id "$WR" --human "scope the migration risk"
"$PROD_ATO" dispatch codex  --war-room-id "$WR" --human "scope the migration risk"

# 2. Close with a coordinator summary (subscription preferred, API key fallback).
"$PROD_ATO" war-rooms close "$WR" --coordinator claude --human

# 3. Share into the team so everyone sees the rich card live.
"$PROD_ATO" war-rooms share "$WR" --team my-team

# 4. Append a follow-up turn — teammates see it appear immediately.
"$PROD_ATO" war-rooms append-event "$WR" --team my-team \
    --kind dispatch_result \
    --json '{"seat":"you","summary":"Migration approved. Scheduling for Friday."}'
```

### Coordinator close summaries (all tiers)

`ato war-rooms close`, `ato sessions close`, and `ato chats close` all accept `--coordinator` to generate a logged summary. The default prefers the active Claude Code subscription (no API key or billing charge) and falls back to an API key if the subscription is unusable:

```bash
# Default — subscription first, API key fallback:
"$PROD_ATO" war-rooms close $WR --human

# Explicitly name a subscription-based coordinator:
"$PROD_ATO" war-rooms close $WR --coordinator claude  --human
"$PROD_ATO" war-rooms close $WR --coordinator codex  --human
"$PROD_ATO" war-rooms close $WR --coordinator gemini --human
```

The desktop close dialog also exposes a Subscription vs API picker for the same choice.

## Related docs

- `docs/grounding.md` — the design doc (human-facing — agent wizard UI, policy schema, verdict computation)
- `docs/methodology-runner.md` — what the runner dispatches through
- `docs/eval-methodology.md` — how the v2.9 bench validates grounded-mode behavior
- `CLAUDE.md` — repo-level playbook (start here if you're new to the repo)
- `memory/feedback_dev_build_keychain.md` (in the driver's `~/.claude/projects/...`) — the dev-build trap
- `memory/feedback_war_room_seats.md` — what providers we actually have working access to

## TL;DR

1. **Use `/Applications/ATO.app/Contents/MacOS/ato`** for any dispatch. Never the PATH-resolved one.
2. **For API-provider seats that need to walk code** (gemini, openai, minimax, anthropic API paths), pass `--require-tools read_file,grep`. Without it the seat is one-shot text.
3. **For CLI-native runtimes** (claude, codex CLI), tools are always on.
4. **Read receipts** with `ato dispatches show <id> --human`. Look at `tool_calls_summary` and `grounded` verdict.
5. **In war-rooms, name in the audit trail** which seats had tools active and which didn't.
6. **When a feature you need isn't in the prod binary**, that's a ship-a-new-prod-app task, not a "switch to the dev binary" temptation.
7. **To share a war-room/session/chat with your team** (Team tier), use `ato war-rooms share <id> --team <slug>`. Teammates see a live rich card. Append turns programmatically with `ato war-rooms append-event <id> --team <slug> --kind <kind> --json <payload>`.
8. **Close summaries** prefer your active Claude Code subscription — no API key billing. Pass `--coordinator claude|codex|gemini` to be explicit.
