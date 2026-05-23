# Grounded mode — agent-level harness for tool consultation

> **Problem.** A war-room or `ato dispatch` today can return a confident reply that never opened the code. The user sees "claude flagged 2 issues" and cannot tell whether claude actually read the diff or hallucinated from the title. The cockpit framing — *"every AI follows your rules"* — falls apart if the rules are advisory only.
>
> **Solution.** Make grounding a first-class property of an agent. The user defines, at agent-create time, *what an agent must consult before answering* — and ATO enforces it at every dispatch, surfaces verified-vs-prompt-only badges in receipts, and refuses to mark a dispatch "verified" without proof.

This is the harness that gives the README + website pitch real teeth. Without it, "verified-via-N-tool-calls" is a marketing claim. With it, every dispatch's receipt earns or fails that badge by checked behavior.

## What a grounded agent looks like (user-facing)

In the agent wizard (Quick form **and** chat wizard), after persona + runtime + model:

```
Grounding (how this agent reads context before answering)
  ( ) Off       — answers from prompt only (default; matches today's behavior)
  ( ) Soft      — agent is asked to list what it consulted before answering;
                  GUI shows "consulted: N items" on every receipt
  (•) Strict    — agent MUST make at least N tool calls / consult at least
                  these paths before its reply is marked verified.
                  Ungrounded replies surface a ⚠ banner.

Required tools     [ read_file ] [ grep ] [ git_log ]              (+ add)
Required paths     README.md  ·  apps/cli/src/commands/**/*.rs    (+ add)
Min tool calls     [ 2 ]
On miss            ( ) warn only    (•) mark "ungrounded"   ( ) re-dispatch
```

For `ato review` and war-rooms, the bundled reviewer agents (`@security-specialist`, `@code-reviewer`, the default war-room seats) ship with `grounding_mode: strict` and `required_tools: [read_file, grep]`. The same checkbox lets a user override per-agent.

## What this looks like in dispatch (CLI + receipts)

```bash
$ ato dispatch claude "review src/auth.ts for SQL injection" --agent @security-specialist
```

The dispatch:

1. Loads the agent's grounding policy.
2. Prepends a system block: *"You MUST call `read_file` and `grep` before answering. List every file you opened. If the prompt does not require code access, say `NO_CODE_REQUIRED` and skip."*
3. Runs the underlying runtime (claude CLI, codex CLI, gemini CLI, or API provider's function-calling loop).
4. Parses the response for tool-use markers (claude CLI emits `<tool_use>` blocks; API providers emit `tool_calls` arrays).
5. Counts tool calls against the policy; records the count + the tool names + the file paths each tool was called on, into the execution log.
6. Sets `dispatch.grounded` to `verified | ungrounded | skipped` based on the policy match.
7. Renders the receipt with a 🔧 / ⚠ badge so a human reading `ato dispatches list` or the GUI sees the verdict instantly.

The receipt the user actually sees:

```
$ ato dispatches show <id> --human

dispatch 7F3A1B6E
  agent:        @security-specialist
  runtime:      claude
  grounded:     ✓ verified   (4 tool calls — read_file ×3, grep ×1)
  tools called:
    read_file("src/auth.ts")                  152 bytes
    read_file("src/middleware/session.ts")    438 bytes
    read_file("tests/auth.spec.ts")            91 bytes
    grep("WHERE.+session_id", "src/**")         3 matches
  duration:     6.4s · $0.024 · 0 files written
  reply:        flagged 2 issues — XSS in render.ts:142, ...
```

And the inverse:

```
dispatch A3B91F02
  agent:        @drive-by-reviewer
  runtime:      gemini
  grounded:     ⚠ ungrounded   (0 tool calls; policy required ≥2)
  reply:        flagged 2 issues from the diff alone
  note:         downweighted in synthesis · re-dispatch suggested
```

## Schema

Single migration on `agents`:

```sql
ALTER TABLE agents ADD COLUMN grounding_mode TEXT NOT NULL DEFAULT 'off';
  -- 'off' | 'soft' | 'strict'

ALTER TABLE agents ADD COLUMN grounding_required_tools TEXT;
  -- JSON array, e.g. '["read_file","grep"]'

ALTER TABLE agents ADD COLUMN grounding_required_paths TEXT;
  -- JSON array of file globs the dispatch must touch via tool call

ALTER TABLE agents ADD COLUMN grounding_min_tool_calls INTEGER NOT NULL DEFAULT 0;

ALTER TABLE agents ADD COLUMN grounding_on_miss TEXT NOT NULL DEFAULT 'mark';
  -- 'warn' | 'mark' | 'redispatch'
```

Per-dispatch evidence captured in `execution_logs`:

```sql
ALTER TABLE execution_logs ADD COLUMN grounded TEXT;
  -- 'verified' | 'ungrounded' | 'skipped' | NULL (when grounding_mode='off')

ALTER TABLE execution_logs ADD COLUMN tool_calls_summary TEXT;
  -- JSON array of {tool, args_summary, bytes_returned}, capped at ~32KB
```

No new tables. The `tool_calls_summary` is a lossy summary; full tool-call audit lives in the runtime-native session history (claude CLI history file, API provider conversation), accessible via existing `ato traces show` paths.

## Dispatch-time enforcement (where it lives)

The hook lives in **one place**: `apps/cli/src/commands/dispatch.rs`, in the `prompt_agent_inner` (or equivalent) that already builds the per-runtime command. The same path is shared by `ato dispatch`, `ato war-rooms` (which is `dispatch` + tag), `ato review`, `ato sessions ... dispatch`, MCP `run_agent`, and the GUI's `promptAgent`. One place to change.

```rust
// pseudo
let policy = agent.grounding_policy()?;
let augmented_prompt = policy.prepend_system_note(prompt);
let response = run_runtime(runtime, augmented_prompt, ...)?;
let evidence = parse_tool_calls(&response, runtime)?;
let verdict = policy.evaluate(&evidence);
execution_log.grounded = Some(verdict);
execution_log.tool_calls_summary = Some(evidence.summary_json());
```

Per-runtime parsers (`parse_tool_calls`):

- **claude CLI** — already emits `<tool_use>` blocks in `--print` mode; trivial regex.
- **codex CLI** — emits tool-call records when `--print` is used with function-calling enabled.
- **gemini CLI** — Slice-B native; emits structured `tool_call` JSON.
- **API providers** (anthropic / openai / google / mistral / groq / xai / deepseek / qwen / minimax / openrouter / together / fireworks / kimi / glm / yi) — the API response's `tool_calls` field is already captured in `api_dispatch_tools.rs`; just bubble the count up.
- **Ollama / OpenClaw / Hermes** — runtimes without native tool calls fall back to `grounded: 'skipped'` with a one-time warning "this runtime doesn't expose tool-call telemetry; soft mode is the strictest option here."

## GUI surface

1. **Agent detail page** — new "Grounding" tab next to "Config / Skills / MCPs / Permissions". Lists the policy, recent verified-vs-ungrounded counts, the last 5 ungrounded dispatches as drill-down.
2. **Live runs registry** — column "Grounding" with the 🔧 ✓ / ⚠ badge (or em-dash for off).
3. **Sessions feed** — each turn shows the grounding badge inline next to the runtime chip.
4. **War-room close card** — closer summary states *"3/3 reviewers walked the code"* or *"1/3 reviewer didn't open the code — downweighted"*.
5. **Insights → Health** — a "Grounding rate" panel per agent (% of dispatches that hit `verified`) with a regression alert when the rate drops 10pp.

## CLI ergonomics

```bash
# Force grounded mode on for a one-off dispatch (no agent required)
ato dispatch claude "..." --grounded
ato dispatch claude "..." --require-tools read_file,grep --min-tool-calls 2

# Create an agent with strict grounding from the start
ato agents create @auth-reviewer \
  --runtime claude \
  --grounding strict \
  --require-tools read_file,grep \
  --require-paths "src/auth/**,tests/auth/**" \
  --min-tool-calls 2

# Inspect grounding health
ato agents grounding @auth-reviewer
  ok-rate: 94% verified · 6% ungrounded over last 100 dispatches
  last ungrounded: 2026-05-22T14:03Z · gemini · 0 tool calls

# Ratchet integration — CI gate on grounding rate
ato ratchet check --agent @auth-reviewer --min-grounded-pct 95
```

## Why this matters strategically

This is the harness that lets ATO's pitch — *"every AI follows your rules"* — be measurably true. Without it, the user has no way to tell a tool-using reply from a hallucinated one in a war-room card, and the cockpit framing leaks. With it:

- Receipts earn their "verified" badge by **observed** behavior, not the agent's word.
- The user defines the rules **once** at agent-create time; every downstream dispatch enforces them — including dispatches fired by MCP from a coding agent the user isn't even watching.
- Cross-runtime comparison becomes honest: *"claude with grounding vs gemini without"* is now a measurable head-to-head, not a vibes argument.
- It gives `ato review`, `ato war-rooms`, and `ato sessions` their integrity story. The closer summary can say *"2/3 reviewers walked the code"* and **mean it**.

It is the **architectural answer** to "we want users to be in the cockpit and have agents follow their rules." The agent's rules are now durable, persisted, and enforced — not a hope-prompt in a system message.

## Roll-out (4 PRs, ~2 days each)

**PR-1 — schema + soft mode.** Migration adds the 5 columns on `agents` + 2 on `execution_logs`. CLI flag `--grounded` accepts `soft` only. Prompt prepend in `dispatch.rs`. GUI badge renders on dispatches. No enforcement; this is observation-only and ships first to gather a baseline of "what % of current dispatches would have passed strict?".

**PR-2 — strict mode + claude CLI parser.** Add `--require-tools` / `--min-tool-calls` flags. Implement `parse_tool_calls` for claude CLI. `grounded` column gets populated with `verified` / `ungrounded`. Closer summary on `ato review` and `ato war-rooms close` references the verdict.

**PR-3 — API-provider parsers + agent wizard UI.** Bubble the API providers' existing `tool_calls` arrays into the same path. New "Grounding" tab in the agent detail page. Bundled reviewer agents (`@security-specialist`, `@code-reviewer`) ship with `strict + read_file,grep + min:2`.

**PR-4 — ratchet + health panel.** `ato ratchet check --min-grounded-pct N` for CI gates. Insights → Health gets the "Grounding rate" panel + regression alert. Documentation lands in `AGENTS.md`, `README.md`, and an `agentictool.ai` blog post explaining the user-facing rules.

Total: ~8 days of focused work for a feature that turns the cockpit pitch into a measurable invariant. P0 work item for v2.9.

## Open questions for sign-off

1. **Default for new agents.** `off` (no behavior change for existing flows) or `soft` (every new agent at least lists what it consulted, no enforcement)? Argument for `soft`: the receipt is more useful, the perf hit is zero, the UI signal is immediate. Argument for `off`: zero risk of false positives in the first week.

2. **Re-dispatch on miss.** Strict-mode + `on_miss: redispatch` is tempting but doubles cost on flaky runtimes. Default `on_miss: mark` (flag and surface, but don't re-run). User flips to `redispatch` for high-stakes review agents.

3. **Hermes / OpenClaw / Ollama.** These runtimes don't natively expose tool-call telemetry. Auto-downgrade their policies to `soft`, or refuse `strict` mode at create time and tell the user "this runtime doesn't support strict grounding yet"? Lean toward the latter — explicit is honest.

4. **Storage cap.** `tool_calls_summary` capped at 32KB per dispatch — enough for 50–100 tool calls of summary lines. Anything beyond goes to the runtime-native history, which `ato traces show` already surfaces. Raise the cap only if real usage hits it.
