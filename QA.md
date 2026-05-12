# ATO Pre-Push QA Checklist

**Run this before every push to `main` / before tagging a release.**

Bug found in production is a bug we should have caught here. The whole point of this doc is to make "did you actually try it?" a yes-or-no question, not a vibes call. Every checkbox has an exact command or click sequence + a pass/fail criterion. If a step is genuinely not applicable to your change, write `n/a` and a one-line reason — don't skip silently.

Format conventions:
- `[ ]` — not run
- `[x]` — passed
- `[!]` — failed (must fix before push)
- `[~]` — partially run / known caveat

Sections you can skip:
- If you touched ONLY backend Rust → still run §0, §1, §10. Skip §3 (GUI) only if no Tauri command shape changed.
- If you touched ONLY frontend → still run §0, §3. Skip §1 (CLI) only if no command was added/changed.
- Documentation-only changes (md / comments) → run §0 only.

There is no shortcut on §0. It runs always.

---

## §0a Cross-runtime review (`ato-review` skill)

Triggered before any commit that touches a public surface (CLI subcommand /
Tauri command / MCP tool / schema migration / `Command::new` / IPC boundary)
or exceeds ~50 LOC of behavior change. The
[`ato-review`](.claude/skills/ato-review/SKILL.md) skill at the repo root
codifies the procedure. Summary:

- [ ] Diff dispatched to a reviewer runtime via `ato dispatch <reviewer> --session review/<branch>`.
- [ ] Findings parsed; each one applied OR explicitly deferred with a one-line justification.
- [ ] Commit body includes the `### Dogfood + review process` section listing the reviewer + applied/deferred findings.

If the change doesn't trip the skill's trigger heuristics, note "trivial — skill skipped" in the commit body so the next reader sees it was a decision, not an oversight.

## §0 Pre-flight (always runs)

- [ ] `cargo build --manifest-path apps/cli/Cargo.toml -p ato` succeeds with zero new warnings (existing warnings tolerated).
- [ ] `cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml` succeeds.
- [ ] `cargo test --manifest-path apps/cli/Cargo.toml -p ato` — all unit tests pass.
- [ ] `cd apps/desktop && npx vite build` — frontend builds with no new TS errors.
- [ ] `cd services/mcp-server && npm run build` — MCP server compiles.
- [ ] `git status` — no unintended files staged. `git diff --stat HEAD` reviewed top-to-bottom.
- [ ] Version bumped in all four manifests if user-visible behavior changed:
  - `apps/cli/Cargo.toml`
  - `apps/desktop/src-tauri/Cargo.toml`
  - `apps/desktop/package.json`
  - `apps/desktop/src-tauri/tauri.conf.json`
- [ ] `ROADMAP.md` updated if a phase changed status (Planned → Shipped, etc).

---

## §1 CLI surface

Build once before this section: `cargo build --manifest-path apps/cli/Cargo.toml -p ato`. Run commands against `./apps/cli/target/debug/ato`.

### §1.1 Dispatch + persistence

- [ ] `ato dispatch claude "say hi" --human` — completes, persists row in `execution_logs`. Verify with `ato dispatches list --limit 1`.
- [ ] `ato dispatch claude "say hi"` (no `--human`) emits valid JSON parseable by `python3 -c "import sys,json; json.load(sys.stdin)"`.
- [ ] `ato dispatch <api-provider> "..."` with a configured key (MiniMax/Grok/etc) — completes, persists. (Skip if no key configured.)
- [ ] `ato dispatch <missing-runtime> "..."` — fails cleanly with "not found on PATH" / "no API key" message, no panic.

### §1.2 Sessions (Phase 6 Slice A + A.2)

- [ ] `ato sessions new --runtime claude --title qa` → returns id. Save it as `$SID`.
- [ ] `ato dispatch claude "remember the number 42" --session $SID --human` — succeeds.
- [ ] `ato dispatch claude "what number" --session $SID --human` — claude recalls 42 via `--resume`.
- [ ] `sqlite3 ~/.ato/local.db "SELECT turn_index, role, runtime FROM session_turns WHERE session_id='$SID' ORDER BY turn_index"` — 4 rows (user/assistant × 2).
- [ ] `ato sessions list --human` — session appears.
- [ ] `ato sessions get $SID` — returns the record.
- [ ] **Cross-runtime continuation**: with a session anchored to claude, run `ato dispatch minimax "what number did i tell you?" --session $SID --human`. The note "anchored to 'claude'; this turn runs 'minimax' via history replay" must appear, AND minimax must recall "42" from the replayed history.
- [ ] `ato sessions delete $SID` — succeeds.

### §1.3 Cross-runtime bridge (Phase 6 Slice B)

- [ ] `ato dispatch claude "..." --tag-bridge` (no `--session`) — fails with "requires --session".
- [ ] **Real bridge round-trip**:
  1. Create a fresh session
  2. Seed an assistant turn with `@minimax please respond` via sqlite3
  3. Run `ato bridge --session $SID --max-rounds 2 --human`
  4. Expect: "Bridge round 1 of 2: @claude → @minimax", a real response, then either round 2 firing or "no @-mention in last turn".
- [ ] **`[CONSENSUS]` termination**: seed a turn with literal `[CONSENSUS]` on a line by itself, run bridge, expect "✓ [CONSENSUS] reached" exit.
- [ ] **Unresolvable mention**: seed `@bogus_runtime`, expect "didn't resolve to a known runtime; stopping".
- [ ] **Self-reference guard**: seed `@claude` on a claude-anchored session, expect "didn't resolve" (self gets filtered).
- [ ] **Round cap**: seed `@minimax` and chain-write so each response has another mention; verify loop bails at `--max-rounds`.
- [ ] **Code-fence stripping**: seed a turn with `` ```@codex inside``` `` in a code block followed by `@minimax` outside — only minimax triggers.
- [ ] `cargo test bridge` — all 6 bridge unit tests pass.

### §1.4 SSH remote runtimes (Phase 6.x-J)

- [ ] `ato runtimes add-remote --name qa-fake --host nonexistent.invalid --user u --runtime claude --binary-path claude --human` — succeeds.
- [ ] `ato runtimes list-remote --human` — shows qa-fake.
- [ ] `ato dispatch qa-fake "hi" --human` — fails cleanly with SSH connect error, persists `error` row in execution_logs.
- [ ] `ato runtimes remove-remote --name qa-fake --human` — succeeds, list now empty.
- [ ] **Real SSH** (if you have a configured remote): dispatch completes, response captured into execution_logs, run row uses the remote slug as the runtime field.
- [ ] `ato dispatch <remote-slug> "..." --session $SID` — fails with the documented "sessions aren't supported on remote runtimes yet" bail.

### §1.5 Runtime health (Phase 6.x-I)

- [ ] `ato runtimes health --human` — output structured per-runtime with status / detail / fix_command.
- [ ] `ato runtimes health` (JSON) — parses as `[{runtime, binary_path, status, detail, fix_command}, ...]`.
- [ ] For each runtime installed on this machine, status is one of `ok / unsigned / revoked / quarantined / unknown`.
- [ ] Missing runtimes get `status: "missing"` + an install fix_command when known (claude/codex/gemini) or none for hermes/openclaw.
- [ ] `cargo test runtimes` — 4 unit tests pass.

### §1.6 Runtime quotas

- [ ] `ato runtimes status --human` — runs without error (may be empty if no quota events captured).

### §1.7 Posts / activity feed

- [ ] `ato posts add "QA test" --human` — persists, returns the post id as `$PID`.
- [ ] `ato posts list --limit 5 --human` — recent posts visible, QA post on top.
- [ ] `ato posts get $PID` — returns the record.
- [ ] `ato posts add "approve me" --kind approval_request` → save id as `$AID`.
- [ ] `ato posts pending --human` — shows $AID.
- [ ] `ato posts approve $AID --notes "approved by QA" --human` — succeeds.
- [ ] `ato posts pending --human` — no longer shows $AID.
- [ ] `ato posts deny <other-id>` on a different approval_request — succeeds (use a freshly created one).
- [ ] `ato posts tail` runs in background, emits one JSONL line per new post (test by `ato posts add` in another shell).

### §1.8 Events bus

- [ ] `ato events recent --limit 5 --human` — recent events shown.
- [ ] `ato events recent --type dispatch_started` — type filter works.
- [ ] `ato events watch` (in another shell) emits a JSONL line when `ato dispatch ...` fires.

### §1.9 Replays + compare

- [ ] `ato replays list --human` — runs without error.
- [ ] Pick a recent dispatch id from `ato dispatches list`. Run `ato replay start <id> --runtime <alt-runtime>`. Verify job created.
- [ ] `ato replay get <job-id>` — returns job state.
- [ ] `ato compare <run-a> <run-b>` — shows side-by-side diff.

### §1.10 Regressions + cost

- [ ] `ato regressions list --human` — runs without error.
- [ ] `ato cost recommendations --human` — runs without error.

### §1.9b API streaming (Phase 6.x-F)

- [ ] `ato dispatch minimax "count to 10 one per line" --stream --human` — chunks land in stdout as they arrive, marked by a "[streaming from <provider> — chunks below]" header. Full response also appears in the final JSON envelope.
- [ ] `ato dispatch minimax "..." --stream` (no `--human`) — outputs only the final JSON envelope; intermediate chunks suppressed so scripts get a single parseable object.
- [ ] Tokens captured: `sqlite3 ~/.ato/local.db "SELECT tokens_in, tokens_out FROM execution_logs ORDER BY created_at DESC LIMIT 1"` — both non-NULL after a successful streamed dispatch.
- [ ] CLI runtime ignores the flag: `ato dispatch claude "..." --stream` runs as a normal buffered dispatch (CLI streaming is a future slice).
- [ ] Bridge loop doesn't stream individual turns even when triggered from a flag-aware command — internal calls pass stream=false.

### §1.10b Eval-score ratchet (Phase 6.x-K)

- [ ] `ato ratchet lock --target runtime:claude --days 30 --human` — succeeds when there's recent data, persists a floor.
- [ ] `ato ratchet lock --target agent:nonexistent` — fails cleanly with "no dispatches in the last N days" (no panic, exit code != 0).
- [ ] `ato ratchet list --human` — shows locked floors with target, threshold, window, locked-at date.
- [ ] `ato ratchet check --human` — exit 0 when current rate ≥ floor-tolerance for every target.
- [ ] **CI-gate fail**: synthesize a breach by manually tightening a lock (`UPDATE eval_ratchets SET threshold=0.0`) + inserting an error row, then run `ato ratchet check`. Exit code must be 1, output must show "✗ FAIL" for that target.
- [ ] `ato ratchet status --human` — same info as check, but exit 0 even on breach (informational).
- [ ] `ato ratchet check --target runtime:<r>` — only checks that target; unaffected ratchets aren't reported.
- [ ] `ato ratchet unlock --target ...` — removes the lock, subsequent `list` shows it gone.
- [ ] `parse_target` rejects typos (`agnet:foo`, bare `foo`, `agent:`) — `cargo test ratchet`.

### §1.11 Recipes

- [ ] `ato recipes templates --human` — lists built-in templates.
- [ ] `ato recipes install <template-slug> --as qa-recipe` — installs.
- [ ] `ato recipes list --human` — shows qa-recipe.
- [ ] `ato recipes enable qa-recipe` — enables.
- [ ] `ato recipes runs qa-recipe` — shows audit log (may be empty initially).
- [ ] `ato recipes disable qa-recipe` — disables.
- [ ] `ato recipes delete qa-recipe` — removes.

### §1.12 Skills + agents

- [ ] `ato skills list --human` — runs.
- [ ] `ato agents list --human` — runs, shows the agents in `~/.claude/agents/` and friends.

### §1.13 Kill + setup-path

- [ ] `ato kill <fake-run-id>` — returns "no such run" or similar, no panic.
- [ ] `ato setup-path --check` — reports current state without modifying anything.

---

## §2 MCP server (50 tools)

Build first: `cd services/mcp-server && npm run build`.

The MCP server is a stdio process. For headless verification, the equivalent CLI command's exit code = the MCP tool's success / failure since each tool is `runAtoCli(args)`.

- [ ] **Compile check**: `tsc` exits 0.
- [ ] **Flag-name parity audit** — for every tool that builds an argv, grep the CLI's `--help` to confirm each flag name exists on the underlying subcommand. Specifically check tools added recently:
  - `post_message` → `ato posts add --as <kind> --slug <slug> --kind <kind>`
  - `approve_request` → `ato posts approve <id> --notes <note>`
  - `deny_request` → `ato posts deny <id> --notes <note>`
  - `sessions_new` → `ato sessions new --runtime <r> --as <slug> --title <t>`
  - `bridge_run` → `ato bridge --session <id> --max-rounds <n>`
  - `add_remote_runtime` → `ato runtimes add-remote --name --host --runtime --port --user --key-path --binary-path --extra-args`
  - `runtime_health` → `ato runtimes health`
  - `list_recent_events` → `ato events recent --limit --type`
- [ ] **Smoke a couple of tools via stdio** (if you have an MCP client like `mcp-inspector` or Claude Desktop with the server installed):
  - Call `list_pending_approvals` — returns valid JSON.
  - Call `runtime_health` — returns the same array `ato runtimes health` does.
- [ ] **Register/unregister flow** — for a small backend change to a tool, install the dev MCP server, list tools, confirm the new tool appears in the list.

---

## §3 Desktop GUI

Launch the app via `npm run dev:desktop` from repo root. Wait for the window to render.

### §3.1 Sidebar + navigation

- [ ] Six sections render: Home / Agents / Skills & MCPs / Runs / Insights / Settings.
- [ ] Project switcher renders at the top of the sidebar.
- [ ] Pending-approvals badge on Runs shows a count when `ato posts pending` returns non-empty.
- [ ] Persistent bottom pane has "Chat" + "Shell" mode toggle.

### §3.2 Home

- [ ] **Runtime health banner**: when at least one runtime returns `revoked / quarantined / unknown`, banner renders red with the specific reason. Otherwise hidden.
- [ ] **Banner false-positive guard**: unsigned `#!/usr/bin/env node` shims (npm-installed claude / openclaw / codex) do NOT trigger the banner.
- [ ] **"Run fix" button** (when a real `revoked` or `quarantined` row exists): clicking it
  1. shows "Running…"
  2. on success: banner disappears (or shrinks to the success toast) within 5 min refetch, success toast says "Fixed @<runtime>"
  3. on failure: red `Failed: <error>` line appears under the row, no app crash
  4. allowlist enforced: a tampered `fix_command` (not `npm install -g` or `xattr -d com.apple.quarantine`) is refused with "unrecognized fix shape"
- [ ] **First-run prompt**: when no runtime is detected, "Connect a runtime" section renders + "Open Settings" button works.
- [ ] **Build an agent CTA**: "Start with chat" opens guided wizard; "Quick setup" opens form wizard.
- [ ] **Recent agents**: shows real agents from `listAgents()`.

### §3.3 Agents

- [ ] List renders with all configured agents.
- [ ] Click an agent → detail page renders Config / Skills / MCPs / Permissions / History / Evaluators tabs.
- [ ] New Agent wizard (guided) completes a smoke test: pick runtime, name, description → writes file to `~/.claude/agents/<slug>.md` (or equivalent per runtime).
- [ ] New Agent wizard (quick form) — same end state, different surface.

### §3.4 Skills & MCPs

- [ ] Skills tab — list of local skills renders per runtime.
- [ ] Toggle a skill on/off — persists.
- [ ] MCPs tab — install registry visible.
- [ ] Install one registry MCP — completes, appears in the installed list.

### §3.5 Runs

- [ ] Live tab — fire `ato dispatch claude "..."` from a terminal, the run appears here within 1s.
- [ ] History tab — past runs visible, click one → detail panel with prompt + response.
- [ ] Schedules — cron jobs visible.
- [ ] Automations — recipe runs visible.
- [ ] Hooks — list visible.
- [ ] Kill button on a long-running dispatch — actually terminates the child process.

### §3.6 Insights

- [ ] Health — runtimes' health pulse renders.
- [ ] Analytics — Recharts plots render with real data.
- [ ] Context — context-usage breakdown visible per runtime.
- [ ] Audit Log — recent events visible.
- [ ] Regressions panel — local mode renders when not signed in; cloud mode renders when Pro+signed-in.
- [ ] Compare modal — opens for two selected runs, side-by-side renders.

### §3.7 Activity feed

- [ ] Feed pane shows recent posts.
- [ ] Compose box: post a message → appears immediately (Tauri event listener, <100ms).
- [ ] Approval request → renders Approve / Deny buttons.
- [ ] Approve / Deny click writes the decision row + the partial UNIQUE index prevents a double-decision (try clicking both — second click should be blocked).

### §3.8 Settings

- [ ] Runtimes — health badges per runtime, manual path override works.
- [ ] Models — model picker for each runtime, persisted.
- [ ] API Keys — add a key for a non-CLI provider (Grok / DeepSeek / etc.), verify dispatch to that provider then works.
- [ ] Secrets — add / list / delete works.
- [ ] Env — add / list / delete works.
- [ ] Cloud — sign-in flow (when applicable).
- [ ] Projects — create / switch / delete works.

### §3.9 Persistent pane

- [ ] Chat mode — send a prompt, see streaming-ish response (today it's batch; flag if it suddenly hangs).
- [ ] Shell mode — `xterm` renders a working shell scoped to project CWD. PATH includes the user's login-shell PATH.
- [ ] Resize the pane — sticks.
- [ ] Navigate between sections — pane state preserved.

### §3.10 Command palette (⌘K)

- [ ] ⌘K opens palette.
- [ ] Typing a query filters across runs, agents, skills.
- [ ] Selecting a result navigates correctly.

### §3.11 i18n

- [ ] EN / PT / ES toggle in sidebar works; key strings on Home update without reload.

---

## §4 Cross-cutting smoke tests

- [ ] **Dispatch a real prompt end-to-end** through the GUI's chat → verify execution_logs row + Live tab + activity feed + (if signed in) cloud trace upload.
- [ ] **Sign-in (Pro)** — sign in once, navigate to Insights → Regressions, verify cloud-mode badge.
- [ ] **Sign-out** — local fallback kicks in cleanly.
- [ ] **Tier gate** — non-Pro user trying to access a Pro feature sees the UpgradePrompt component.

---

## §5 Known unverified-this-session items

Each release should expand this list with what was NOT re-tested. Carry forward to the next release's QA so we don't forget.

- [x] **MCP tools via a real client** — *verified v2.3.51 via `services/mcp-server/scripts/qa-stdio-smoke.mjs`*. Spawns the built MCP server, runs the JSON-RPC handshake, lists 52 tools, invokes `runtime_health` + `list_pending_approvals` and asserts response shapes. **Caught a real PATH-resolution gap**: the MCP server's spawn env can't find the dev `ato` binary without `ATO_CLI_PATH` set; documented in the smoke test. Re-run with `cd services/mcp-server && npm run build && node scripts/qa-stdio-smoke.mjs` before releases that change MCP surface.
- [x] **"Run fix" button against a real revoked binary** — *verified v2.3.51 via `runtime_health::xattr_removes_quarantine_end_to_end` integration test*. Creates a temp file, sets `com.apple.quarantine`, calls the underlying `runtime_health_run_fix` function directly (same function body the Tauri IPC invokes; the IPC layer adds JSON arg deserialization on top), asserts the xattr is removed. macOS-only (`#[cfg(target_os = "macos")]`); runs in `cargo test`. Does NOT exercise the Tauri JSON-deserialization layer — to test that end-to-end, open the desktop, set up a real quarantined runtime binary, click the banner's "Run fix" button.
- [~] **Slice B `[CONSENSUS]` real-LLM termination** — *anecdotally verified v2.3.46 dogfood* (MiniMax replied `[CONSENSUS]` on its own line; loop exited round 1). Not a deterministic regression test: a future model version could phrase the same agreement differently and this property would silently regress. Follow-up: add a synthetic test that feeds a literal `[CONSENSUS]` turn into the parser directly (already covered by `consensus_must_be_on_own_line`) AND a session-level test with a mock-LLM fixture that always emits the standalone-line form to lock the integration path.
- [~] **Recipe templates fire on triggers** — *partially verified*. `skillify-successful-replays` has 7 past runs in `ops_recipe_runs` (1 done, 6 failed-action — still proves the events_log → engine-poll → action chain works). `auto-replay-regression-failures` is enabled but no real regression event has fired yet on this machine; trigger plumbing is the same code path so it'll fire when one comes in. Manual procedure to force-verify: insert a synthetic `regression_detected` row into `events_log`, run the desktop, wait 2s, check `ops_recipe_runs` for a new claim.
- [ ] **Cron schedules** fire at their scheduled time on macOS launchd + Linux cron + Windows schtasks. Time-bound and side-effect-heavy (modifies user's launchd). Manual procedure on macOS: open desktop → Runs → Schedules → create "every minute" schedule → verify `~/Library/LaunchAgents/ai.agenticsearchoptimization.cron.*.plist` exists and `launchctl list | grep ato` shows it loaded → wait 60s → check `execution_logs` for the dispatched row.

---

## §6 Pre-push checklist (5-minute version)

When the change is small and §1–§5 feels excessive:

- [ ] §0 ran clean
- [ ] The specific feature you touched dogfooded once end-to-end
- [ ] No new untested public surface (CLI subcommand / Tauri command / MCP tool) — or §5 updated with what you skipped
- [ ] Version bumped if user-visible
- [ ] Commit message names the feature + the dogfood action you ran

---

## §7 Doc maintenance

This file is the contract. When you ship a new feature:
1. Add a check for it in the appropriate section above.
2. Update §5 with anything you couldn't test in the release session.
3. Bump the doc's stamp below.

**Last full pass**: 2026-05-12 (Phase 6.x cluster + MCP expansion).
**Maintainer**: see `git log QA.md`.
