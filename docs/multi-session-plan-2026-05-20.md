# Multi-session plan — v2.7.10 + v2.7.11

Date: 2026-05-20
Driver: Will, with PR plan informed by Felipe Tavares' v2.7.7 review (`ATO_REVIEW_2026-05-20.md`) + the v2.7.9 deferred docket.

The intent: hand each chunk to a separate Claude session in its own terminal. Each chunk is scoped tightly so multiple sessions can run in parallel without stepping on each other.

---

## Strict file-ownership matrix — no two TIER 1 sessions touch the same file

Each TIER 1 session has an **exclusive write set**. If a session's prompt asks it to touch a file outside this set, that's a bug in the prompt — the session must instead stub/comment and surface the issue back to you.

| Session | EXCLUSIVE write set | Read-only |
|---|---|---|
| **S1** MiniMax tighten | `apps/cli/src/api_dispatch_tools.rs`<br>`apps/desktop/src-tauri/src/api_dispatch_tools.rs` | everything else |
| **S2** Default prompt | `apps/desktop/src-tauri/src/schema.rs` (additive ALTER only)<br>`apps/desktop/src-tauri/src/commands/mod.rs` (only `create_agent` + a new `update_agent_default_prompt` fn — DOES NOT touch `prompt_agent_inner` or `prompt_api_provider`)<br>`apps/desktop/src/lib/agents.ts`<br>`apps/desktop/src/lib/agentConversation.ts`<br>`apps/desktop/src/components/AgentDetail/*` (or wherever the agent edit UI lives) | everything else |
| **S3** Run-detail UI | `apps/desktop/src/components/SessionsList/SingleRunDetailView.tsx`<br>`apps/desktop/src/components/SessionsList/_helpers.ts` (if new types are needed)<br>NEW: `apps/desktop/src/components/SessionsList/PermissionEventsPanel.tsx` | everything else |
| **S4** .deb updater | NEW: `apps/desktop/src-tauri/src/installer_detect.rs`<br>NEW: `apps/desktop/src/components/UpdateBanner.tsx`<br>`apps/desktop/src-tauri/src/lib.rs` (only the `mod installer_detect;` line) | everything else — DO NOT touch the existing updater command unless extending it via new module surface |
| **S5** Empty-MCPs warning | `apps/desktop/src/components/CreateAgentWizard/QuickPath.tsx` AND/OR `GuidedPath.tsx` (only the MCP-selection section)<br>NEW: `apps/desktop/src/components/CreateAgentWizard/EmptyMcpsWarning.tsx` | everything else |

**S2's caveat**: it adds a field to `prompt_agent_inner` semantically (use default_prompt when prompt arg is empty), but the implementation is **placeholder for now** — write a TODO comment in `prompt_agent_inner` and STOP. Real wiring happens in a later session that holds the `prompt_agent_inner` lock. This keeps the file ownership clean.

**Backend-touching session (S2) must NOT modify**:
- `prompt_agent_inner` (~commands/mod.rs:807 region)
- `prompt_api_provider` (~commands/mod.rs:1300+ region)
- `dispatch_war_room`, `dispatch_into_session*` in `sessions_view/write.rs`
- Any existing Tauri command handlers other than `create_agent`

If S2's task requires touching one of those, it leaves a TODO + signals to you.

### TIER 2 sessions own `commands/mod.rs::prompt_agent_inner` + `prompt_api_provider` (after TIER 1 merges)

| Session | EXCLUSIVE write set |
|---|---|
| **S6** PATH resolver | `apps/desktop/src-tauri/src/commands/mod.rs::get_user_path` only (around line 530). DO NOT touch other functions in that file. |
| **S7** MCP-tool gating | `apps/desktop/src-tauri/src/commands/mod.rs::prompt_api_provider` only (around line 1300+). New helper module `apps/desktop/src-tauri/src/commands/mcp_dispatch.rs`. DO NOT modify `prompt_agent_inner` or `get_user_path`. |
| **S8** Pre-trust | `apps/desktop/src-tauri/src/commands/mod.rs::prompt_agent_inner` claude branch only. DO NOT modify other branches or unrelated fns. |

S6 + S7 + S8 can run in parallel because each owns a different fn in mod.rs. Their merges will conflict at the FILE level (same file, different regions) — git's 3-way merge handles this automatically since the regions don't overlap.

### TIER 3 — refactor; sequential

S10 (async loop dedup) inherently touches both `api_dispatch_tools.rs` files — must run after S1 (and ideally after any TIER 2 work that touches the loop).

Each session gets its own git worktree. You merge them back after each session's pre-push gate passes.

---

## How to launch each session

In a fresh terminal:
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git worktree add ../ato-S<N> -b session-S<N>
cd ../ato-S<N>
claude  # or open Claude Code in this worktree
```

Then paste the **session prompt** for that session (below). Each session prompt is self-contained — assumes Claude has zero prior context.

After the session finishes:
```bash
# In the worktree
git push origin session-S<N>
# Open a PR or merge directly into main
git checkout main && git merge session-S<N> && git push
git worktree remove ../ato-S<N>
```

---

## TIER 1 — run all 5 in parallel

### S1 — MiniMax prose-prefix tolerance + strict-match tightening

**Files:** `apps/cli/src/api_dispatch_tools.rs`, `apps/desktop/src-tauri/src/api_dispatch_tools.rs`.

**Branch:** `session-S1-minimax-tighten`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S1 (a git worktree on branch
session-S1-minimax-tighten).

Task: tighten v2.7.9's MiniMax content-as-args detector
(parse_minimax_content_as_tool_calls in apps/cli/src/api_dispatch_tools.rs
+ desktop mirror).

Two changes:

1. PROSE-PREFIX TOLERANCE. Currently the detector requires the ENTIRE
   trimmed content to be a JSON object (optionally inside a single
   ```json fence). Real MiniMax emissions sometimes look like:
   "I'll call read_file: ```json\n{...}\n```"
   Detect this pattern: if content contains exactly one ```json (or
   ```)-fenced block, extract that block's contents and parse. Reject
   if multiple fenced blocks (ambiguous).

2. STRICT-MATCH TIGHTENING (claude's PR-A risk 4a). Currently when
   schema has no `required` field, the match becomes very loose
   (only checks extra-keys). Add: when `required` is absent AND obj
   is non-empty, require at least one key to intersect with
   `properties`. Empty obj with absent `required` still matches.

Write 4 new unit tests:
- prose_prefix_then_fenced_json_matches
- multiple_fenced_blocks_returns_empty (ambiguous)
- empty_object_no_required_still_matches (preserves edge case)
- nonempty_object_no_required_no_property_intersection_returns_empty

Mirror in desktop's api_dispatch_tools.rs. Run cargo test from
apps/cli to confirm all pass.

When done: commit with message
"polish(minimax): prose-prefix tolerance + tighter strict-match"
and push the branch.
```

---

### S2 — Felipe P5: Default dispatch prompt field on agents

**Files:** Frontend agent-edit screens + `agentConversation.ts` + agents schema (additive column) + minor backend `create_agent` + `update_agent_*` changes.

**Branch:** `session-S2-default-prompt`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S2 (git worktree).

Felipe's v2.7.7 review (docs/dogfood-v2.7.9.md or
~/Downloads/ATO_REVIEW_2026-05-20.md) P5: agents have no "default
dispatch prompt" field. Every Run requires typing a prompt. Closes
his most common WhatsApp complaint.

Scope:
1. Schema: add column `default_prompt TEXT` to `agents` table in
   apps/desktop/src-tauri/src/schema.rs (additive ALTER TABLE,
   same pattern as permissions_migrated_at).
2. Backend: extend create_agent + add update_agent_default_prompt
   Tauri command in apps/desktop/src-tauri/src/commands/mod.rs.
3. Backend: in prompt_agent_inner (commands/mod.rs:807) — when the
   incoming prompt arg is empty AND agent_slug is set AND
   agent.default_prompt is non-empty, use default_prompt as the
   prompt body. Persona prepend still applies on top.
4. Frontend: extend AgentSpec in agentConversation.ts with
   defaultPrompt?: string. Add a textarea to the agent-detail edit
   UI (find existing agent edit component). Save via new command.
5. Frontend: PromptBar + FirstChatWizard — when agent has
   default_prompt and user hits Send/Run without typing, use the
   default. Show a small "(default)" indicator in the dispatch
   button when this would fire.

Write tests:
- Rust: `create_agent_with_default_prompt_persists`,
  `prompt_agent_inner_uses_default_when_blank` (intercept Command::get_args).
- Frontend: vitest verifying defaultPrompt round-trips through
  agentConversation.parse.

Run pre-commit gate (cargo check + vitest) before committing.

Commit message:
"feat(agents): default dispatch prompt field — Felipe P5"

Push branch.
```

---

### S3 — Run-detail UI for denied/advisory events

**Files:** `apps/desktop/src/components/SessionsList/SingleRunDetailView.tsx`, related styles.

**Branch:** `session-S3-run-detail-denied`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S3 (git worktree).

v2.7.8 plumbed agent permissions through to dispatch but the
denied/advisory events have no UI surface. When a permission BLOCKS
a tool call (or the gate marks a label as advisory on a codex
sandbox), the user sees nothing.

Scope:
1. Read apps/desktop/src/components/SessionsList/SingleRunDetailView.tsx.
   Add a "Permission events" section near the existing
   tool_calls_summary rendering.
2. Parse tool_calls_summary JSON (already includes is_error per
   audit). When is_error AND content matches "blocked by agent
   policy", render with a red "DENIED" badge + the rule that fired.
3. New backend column or JSON field: extend execution_logs writes in
   apps/cli/src/commands/dispatch.rs persist site to ALSO record
   advisory_only labels when codex sandbox demotion fires. Add a
   small "advisory_labels" JSON column or pack into existing
   tool_calls_summary.
4. UI badges: "ALLOWED" (green, default), "DENIED" (red, when
   blocked), "ADVISORY" (yellow, codex-style structural limit).

Write vitest tests for the parsing function that takes a
tool_calls_summary JSON string and returns the categorized events.

Run pre-commit + vitest gate before committing.

Commit message:
"feat(ui): run-detail surfaces denied + advisory tool events"

Push branch.
```

---

### S4 — Felipe P2: `.deb` auto-update detection

**Files:** `apps/desktop/src-tauri/src/updater.rs` (if exists) or new module.

**Branch:** `session-S4-deb-update`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S4 (git worktree).

Felipe's v2.7.7 review P2: Tauri Updater tries to replace AppImage,
but on .deb installs the binary is /usr/bin/ato-desktop (root-owned).
Update fails silently with EACCES. Felipe ran v2.4.8 for 6 days
unaware v2.7.7 had shipped.

Scope:
1. Find the existing Tauri Updater integration. Search for
   "updater" or "tauri_plugin_updater" in apps/desktop/src-tauri/src/.
2. Detect installation method on Linux:
   - .deb → binary at /usr/bin/ato-desktop, package owner = root,
     `dpkg -S /usr/bin/ato-desktop` succeeds
   - AppImage → binary path matches *.AppImage or APPIMAGE env var
   - snap → binary path under /snap/
3. When install_method == "deb", REPLACE the "Install update" button
   with text + a copy-clipboard button:
   "Update available: v{version}. Run:
    sudo dpkg -r ato && wget <release-url> -O /tmp/ato.deb && sudo dpkg -i /tmp/ato.deb"
4. Use gh API or the existing release-check to grab the latest .deb
   download URL.

Test: mock the install_method detection function and verify the UI
renders the right message per method. (No way to test the dpkg
command without root, so verify only the message generation.)

Commit message:
"fix(updater): .deb install detection + manual upgrade instructions"

Push branch.
```

---

### S5 — Felipe P6: Empty MCPs warning in agent creation

**Files:** `apps/desktop/src/components/CreateAgentWizard/*.tsx` (specifically the MCP-selection step).

**Branch:** `session-S5-empty-mcps-warning`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S5 (git worktree).

Felipe's v2.7.7 review P6: when creating an agent and reaching the
MCP selection step, the list is empty if no MCPs have been registered
under Skills & MCPs. New users create agents without MCPs without
realizing.

Scope:
1. In the MCP selection step of CreateAgentWizard (find the relevant
   .tsx file in apps/desktop/src/components/CreateAgentWizard/),
   detect when the list of available MCPs is empty.
2. When empty, render a warning card:
   "No MCPs registered yet. MCPs give your agent tools like gmail,
    filesystem, github. [Browse catalog →] (link to Skills & MCPs
    tab) or skip — you can attach MCPs later."
3. Use useUiStore's setSection/setSubTab to route the link.

Vitest: render the wizard with mocked empty MCP list, assert the
warning + the link both appear.

Commit message:
"feat(wizard): warn when no MCPs registered yet — Felipe P6"

Push branch.
```

---

## TIER 2 — run AFTER TIER 1 merges (all touch commands/mod.rs)

### S6 — Felipe P1: PATH resolver for nvm/pyenv/rbenv

**Files:** `apps/desktop/src-tauri/src/commands/mod.rs::get_user_path` + MCP spawn sites.

**Branch:** `session-S6-path-resolver`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S6 (git worktree). main has all
of TIER 1's merges.

Felipe's v2.7.7 review P1: 100% of catalog MCPs that use `npx` fail
on WSL with nvm because ATO spawns child processes without the nvm
PATH. Error: `No such file or directory (os error 2)` or
`EOF while parsing a value at line 1 column 0`.

Scope:
1. Read apps/desktop/src-tauri/src/commands/mod.rs::get_user_path
   (around line 530). It spawns a login shell to capture PATH. On
   WSL with nvm, the login shell's PATH may not include
   ~/.nvm/versions/node/<version>/bin if nvm is sourced lazily.
2. Extend the function: after the login-shell capture, ALSO
   explicitly probe for and append common version-manager paths if
   they exist:
     - $HOME/.nvm/versions/node/*/bin (newest version)
     - $HOME/.pyenv/shims
     - $HOME/.rbenv/shims
     - $HOME/.local/bin
   For nvm: glob ~/.nvm/versions/node/*/bin, sort descending,
   take the first match.
3. Surface a "Generate wrapper" button on the MCP connection error
   screen — when the error message contains "No such file or
   directory" or "EOF while parsing", offer to write a wrapper
   script to ~/.local/bin/<mcp>-wrapper.sh that exports the right
   PATH then execs the original command. The wrapper template is in
   Felipe's review under P1.
4. Tests: assert the PATH after extension includes
   ~/.nvm/versions/node/*/bin if that directory exists on the test
   host. Skip the test if it doesn't (use #[cfg_attr(not(target_os
   = "linux"), ignore)] or runtime check).

Commit message:
"fix(mcp): auto-extend PATH with nvm/pyenv/rbenv shims — Felipe P1"

Push branch.
```

---

### S7 — MCP-tool gating (PR-B reborn from v2.7.9)

**Files:** `apps/desktop/src-tauri/src/commands/mod.rs::prompt_api_provider` + execution path for MCP tool calls.

**Branch:** `session-S7-mcp-tool-gating`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S7 (git worktree).

PR-B from v2.7.9 was implemented and reverted after a war-room review
found two ship blockers:
1. discover_mcp_server_tools is sync, called from async Tauri command
   → blocks tokio worker. Fix: wrap in tokio::task::spawn_blocking.
2. MCP tools were OFFERED to the model but execute_call returns
   "unknown tool". Fix: implement MCP-server tools/call round-trip.

Scope:
1. Add tokio::task::spawn_blocking wrap around discover_mcp_server_tools
   in a new load_agent_mcp_tools helper inside prompt_api_provider in
   apps/desktop/src-tauri/src/commands/mod.rs.
2. Cache discovery results in-memory keyed on (agent_slug,
   mcps_json_hash). Cache invalidates when agent.mcps column changes.
3. Implement an MCP execution path: when a tool call's name matches
   an MCP-discovered tool (not in review_tools::registry()),
   re-spawn the MCP server, send JSON-RPC tools/call with the
   model's args, parse the response, return as ToolResult. Wire into
   the dispatch_with_tools loop in
   apps/desktop/src-tauri/src/api_dispatch_tools.rs — add an
   execute_mcp_tool branch alongside the existing review_tools
   branch.
4. Tests: assert spawn_blocking returns to the async runtime cleanly;
   mock MCP server returning a tools/call response; assert ToolResult
   carries the response.

This is the v2.7.10 hero PR. Run all gates carefully.

Commit message:
"feat(mcp): tool-call gating + execution for desktop API dispatch"

Push branch.
```

---

### S8 — Felipe P3: Pre-trust directories on project registration

**Files:** `apps/desktop/src-tauri/src/commands/mod.rs::prompt_agent_inner` (claude branch).

**Branch:** `session-S8-pre-trust`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S8 (git worktree).

Felipe's v2.7.7 review P3: Claude Code prompts "Trust this folder?"
on every session. Friction on every dispatch.

Scope:
1. In apps/desktop/src-tauri/src/commands/mod.rs::prompt_agent_inner
   (claude branch around line 833), add `--dangerously-skip-permissions`
   to the claude --print invocation ONLY when the workspace is a
   project registered in ATO's projects table.
2. Add a Settings toggle "Trust ATO-registered projects" (default
   ON). When OFF, don't pass the flag.
3. The toggle's value reads from a new settings.json key
   "trust_registered_projects". Default true.

Tests: verify the spawn args include the flag when the dispatch is
inside a registered project; verify they don't when the toggle is
off.

Commit message:
"feat(claude): pre-trust registered projects to skip per-session prompt — Felipe P3"

Push branch.
```

---

### S9 — Felipe P4: Swap "Run" and "Quick Test" hierarchy

**Files:** Frontend AgentDetail / AgentCard components.

**Branch:** `session-S9-run-vs-quicktest`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S9 (git worktree).

Felipe's v2.7.7 review P4: user clicks "Run" expecting "execute the
agent and give me the result." Actual behavior is "open interactive
terminal for me to type in." Quick Test does what users expect from
Run.

Scope:
1. Find the Run button and Quick Test button in the agent UI (likely
   apps/desktop/src/components/AgentDetail/* or
   apps/desktop/src/components/AgentCard.tsx).
2. Make "Run" be the dispatch path (current Quick Test behavior). If
   agent.default_prompt (from S2) is non-empty, use that. Otherwise
   show a prompt input modal.
3. Rename "Open in Shell" or "Run" (whichever was the interactive)
   to "Interactive session" (Felipe P10).
4. Use the default_prompt added in S2 as the auto-fired prompt when
   user clicks Run.

This task depends on S2 landing first (default_prompt field).

Tests: vitest assertions about which Tauri command gets invoked for
each button click.

Commit message:
"refactor(ui): Run = dispatch, Interactive session = shell — Felipe P4"

Push branch.
```

---

## TIER 3 — refactor; ship last

### S10 — Extract async loop body (CLI/desktop dedup)

**Branch:** `session-S10-loop-dedup`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S10 (git worktree).

v2.7.8 PR-3b shipped a parallel implementation of dispatch_with_tools
in apps/cli/src/api_dispatch_tools.rs (blocking) and
apps/desktop/src-tauri/src/api_dispatch_tools.rs (async). The
Conversation struct + per-flavor helpers (build_request_body,
parse_tool_calls, append_*, extract_*) are ~95% identical. v2.7.9
added more shared helpers (parse_minimax_content_as_tool_calls,
minimax_content_matches_schema, strip_fenced_json_block) that landed
in BOTH files. Drift risk.

Codex's PR-3b review recommended: extract loop body into a flavor-
agnostic async helper that takes a send_round async closure. CLI
wraps in a blocking shim, desktop calls natively.

Scope:
1. Create packages/ato-tools-loop crate (or extend ato-review-tools).
2. Move Conversation + helpers there.
3. Move dispatch_with_tools generic core there, parameterized over
   an async fn(request_body) -> Result<response_payload>.
4. CLI dispatch_with_tools becomes ~20-line blocking shim using
   tokio::runtime::Handle::block_on (or build an ephemeral runtime).
5. Desktop dispatch_with_tools becomes ~20-line async caller.

All existing tests should pass with zero modification (they test
parse/serialize behavior which moves into the shared crate).

Run cargo test from BOTH apps/cli AND packages/* to confirm.

Commit message:
"refactor(api-tools): extract shared async loop core; CLI uses blocking shim"

Push branch.
```

---

### S11 — Migration UI toast for pre-v2.7.8 agents

**Branch:** `session-S11-migration-toast`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S11 (git worktree).

v2.7.8 PR-6 made permissions_migrated_at opt-in to preserve pre-
v2.7.8 dispatch behaviour. Today users have to SQL-stamp the column
or recreate the agent. A one-time toast on agent-edit closes the
gap.

Scope:
1. In the agent-detail / agent-edit React component, useEffect on
   mount: check if agent.permissions_migrated_at is null AND
   agent.permissions is non-null.
2. If yes, render a toast/banner: "This agent's permissions aren't
   enforced yet. Review and enable enforcement → [Enable]"
3. Clicking Enable: invoke a new Tauri command
   stamp_permissions_migration({agent_id}) that UPDATEs
   permissions_migrated_at = datetime('now') and recordChangeFor.
4. Dismiss button stores a flag in localStorage so the toast doesn't
   re-render every navigation.

Vitest: render with a non-migrated agent, assert toast appears;
click Enable, assert invoke is called with the right slug.

Commit message:
"feat(agents): one-time migration toast for pre-v2.7.8 agents"

Push branch.
```

---

### S12 — Tauri-webdriver pre-tag smoke pass

**Branch:** `session-S12-webdriver-smoke`

**Session prompt:**
```
Working in /Users/beatriznigri/ato-S12 (git worktree).

Both 2026-05-19 war-room reviewers picked mandatory pre-tag dogfood
smoke as a v2.7.8 deliverable (skipped to ship). Worth ~1 day of
focused investment.

Scope:
1. Add @tauri-apps/test or webdriverio dev-dep (research which works
   best on macOS + Linux dev environments).
2. Write scripts/pretag-dogfood-smoke.ts: drives a Tauri-webdriver
   session through the 7-step golden path from the audit doc:
   - Cold launch ATO desktop
   - Open FirstChatWizard from Home → mounts
   - Open FirstChatWizard from PromptBar → mounts
   - Create session without initial turn → no ghost row in Sessions
   - Create war-room → dispatch fires, seats persist
   - Toggle runtime readiness with wizard + PromptBar both open
     → no z-index stacking
   - Final assertions: zero 0-msg ghost rows, no hidden modals
3. Wire into .githooks/pre-push to run ONLY for v*.*.* tag commits
   (skip on regular pushes).
4. Failure = block tag push.

This is a process investment — make sure it's reliable BEFORE wiring
into the hook (else releases get unblockable).

Commit message:
"chore(release): mandatory pre-tag dogfood smoke via Tauri-webdriver"

Push branch.
```

---

## Suggested sequencing

**Today / first session:** Launch all of TIER 1 in 5 parallel terminals. Each is ~1-3 hours of focused work; total wall time ~3 hours.

**After all TIER 1 PRs merge:** Launch TIER 2 in 4 parallel terminals (S6/S7/S8/S9). S9 needs S2 (default_prompt). S7 is the v2.7.10 hero.

**TIER 3 is a clean-up batch:** Ship as v2.7.11 or v2.7.12.

**Release cadence:**
- **v2.7.10**: S1 + S2 + S3 + S4 + S5 + S6 + S7 + S8 + S9 (everything Felipe + ours, except refactor)
- **v2.7.11**: S10 + S11 + S12 (refactor + nice-to-have)

Or split:
- **v2.7.10**: Hero = S7 (MCP). Plus S1, S6, S8 (small backend wins).
- **v2.7.11**: Felipe UX bundle = S2 + S4 + S5 + S9 (all the wizard / agent UX wins).
- **v2.7.12**: S3 + S10 + S11 + S12 (run-detail, refactor, polish).

---

## Cross-session coordination rules

1. **No two sessions touch the same file at the same time.** Conflict map above shows TIER 1 is safe; TIER 2 must wait.
2. **Each session opens the relevant memory files first** to inherit context (especially `~/.claude/projects/-Users-beatriznigri/memory/MEMORY.md` and the v2.7.8 + v2.7.9 project memories).
3. **Each session uses ATO war-rooms for code review** before final commit. Use claude + google + minimax (codex still rate-limited until 2026-05-24). Pattern from the v2.7.9 dogfood doc.
4. **Pre-commit / pre-push hooks must stay green.** No `--no-verify`.
5. **Never auto-push to main.** Each session pushes its branch; you (the human) merge after review.

---

## Felipe's review items NOT covered above (intentionally)

- **P7 — Docker WSL** — environment configuration, not ATO bug. Documented for completeness.
- **F5 — Cron scheduling in UI** — separate roadmap item, fits with the existing automations work.
- **F6 — Alert notifications** — cloud feature (paid tier).
- **F7 — Run diff view** — nice-to-have, low priority.
- **F8 — Re-run with edited variables** — fits with the existing variable system, separate session.
- **F9 — MCP catalog filter by runtime** — fits with PR-3a follow-up; v2.7.11.

---

**Generated:** 2026-05-20 by Claude during the v2.7.9 ship session.
**Driver:** Will Nigri.
**Inputs:** Felipe Tavares' `ATO_REVIEW_2026-05-20.md` + audit doc (`docs/audits/agent-permissions-plumb-through-2026-05-20.md`) + v2.7.9 release notes.

---

## LAUNCH SCRIPTS — copy-paste these into separate terminals

> **Repo root:** `/Users/beatriznigri/Agentic-Tool-Optimization` (referenced as `$REPO` below for brevity — replace if pasting).
>
> **Worktree pattern:** Each session lives in `../ato-S<N>/` as a sibling directory to the main checkout. Each on its own branch. They share `~/.ato/local.db` (read-only from the sessions; only TIER 2's S7 writes to it for caching).

### Shared "Operating instructions" — included in every session prompt

Every session prompt below ends with this block. Don't strip it.

```
=== OPERATING INSTRUCTIONS ===

You are running in an isolated git worktree on a session branch.
You have FILE-OWNERSHIP CONSTRAINTS — see docs/multi-session-plan-
2026-05-20.md (in this worktree). Touching files outside your
exclusive write set is FORBIDDEN; instead leave a TODO comment and
surface the conflict to the user.

USE ATO FOR PLANNING + CODE REVIEW (don't dispatch Claude sub-Agents
for these — the standing rule is in the user's memory at
~/.claude/projects/-Users-beatriznigri/memory/feedback_war_room_in_ato.md).

The ATO CLI is at /Users/beatriznigri/Agentic-Tool-Optimization/apps/cli/target/release/ato.
The keychain master key is in macOS keychain; export it first:

    export ATO_MASTER_KEY_B64="$(security find-generic-password -s ato-desktop -a master_key_v1 -w)"

WAR-ROOM FOR PLANNING (use BEFORE writing code on a non-trivial task):

    WID=$(uuidgen)
    ATO=/Users/beatriznigri/Agentic-Tool-Optimization/apps/cli/target/release/ato
    $ATO dispatch claude  "<your prompt>" --war-room-id $WID --war-room-round 1 &
    $ATO dispatch google  "<your prompt>" --war-room-id $WID --war-room-round 1 &
    $ATO dispatch minimax "<your prompt>" --war-room-id $WID --war-room-round 1 &
    wait
    sqlite3 ~/.ato/local.db "SELECT runtime, response FROM execution_logs WHERE war_room_id='$WID';"

Codex is rate-limited until 2026-05-24 — DO NOT dispatch codex.
Use claude + google + minimax (3 seats is the standard quorum).

WAR-ROOM FOR CODE REVIEW (use AFTER your first implementation pass):

    Same pattern. Reference the actual file paths + line numbers
    you changed. Demand cite-the-code answers.

DOGFOOD VIA CLI (after code changes, before commit):

    cd /Users/beatriznigri/Agentic-Tool-Optimization/apps/cli
    cargo build --release
    # Then test with a real dispatch:
    $ATO dispatch <runtime> --agent <slug> "<test prompt>"

PRE-COMMIT GATE: runs automatically on git commit. cargo check +
vitest. NEVER --no-verify; fix the underlying issue.

PRE-PUSH GATE: runs automatically on git push. Adds 18-cmd CLI smoke
+ vite build + cargo build --release. Also NEVER --no-verify.

TESTS:
- Rust: cd apps/cli && cargo test
- Crate: cd packages/<crate> && cargo test
- Frontend: cd apps/desktop && npx vitest run --reporter=dot
- TS typecheck: cd apps/desktop && npx tsc --noEmit

MEMORY FILES TO READ FIRST:
- ~/.claude/projects/-Users-beatriznigri/memory/MEMORY.md (index)
- ~/.claude/projects/-Users-beatriznigri/memory/project_v2_7_8_agent_perms_shipped.md
- ~/.claude/projects/-Users-beatriznigri/memory/feedback_war_room_in_ato.md
- ~/.claude/projects/-Users-beatriznigri/memory/feedback_dev_build_keychain.md

COMMIT MESSAGE FORMAT:
    <type>(<scope>): <one-line summary>

    <body — 2-3 paragraphs explaining why, war-rooms used, tests added>

    Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>

When done: commit on your session branch. DO NOT push to main or
merge. DO push your branch. The user merges.
```

---

### Terminal A — Session S1 (MiniMax tighten)

**Setup (run once):**
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git worktree add ../ato-S1 -b session-S1-minimax-tighten
cd ../ato-S1
```

**Open Claude Code in this worktree, then paste this prompt:**
```
Session S1 — MiniMax content-as-args tighten.

Files you OWN (exclusive write):
  apps/cli/src/api_dispatch_tools.rs
  apps/desktop/src-tauri/src/api_dispatch_tools.rs

Touching anything else = STOP and surface.

TASK:
1. PROSE-PREFIX TOLERANCE. v2.7.9's parse_minimax_content_as_tool_calls
   requires the ENTIRE trimmed content to be JSON (optionally fenced).
   Real MiniMax sometimes emits "I'll call read_file: ```json {...} ```".
   Detect: if content has exactly ONE fenced block (```json or bare ```),
   parse that block's contents. Reject if multiple fenced blocks
   (ambiguous).

2. STRICT-MATCH TIGHTENING. When schema lacks `required`, the match
   becomes loose (only extra-keys check fires). Add: when `required`
   is absent AND obj is non-empty, require at least one obj key to
   intersect with `properties`. Empty obj + absent required still
   matches (preserves edge case).

Mirror BOTH changes in CLI + desktop versions verbatim.

Write 4 new unit tests in apps/cli/src/api_dispatch_tools.rs tests/
mod (the CLI is the canonical test location; desktop is a verbatim
port):
- prose_prefix_then_fenced_json_matches
- multiple_fenced_blocks_returns_empty
- empty_object_no_required_still_matches
- nonempty_object_no_required_no_property_intersection_returns_empty

VERIFY:
  cd /Users/beatriznigri/ato-S1/apps/cli && cargo test
  cd /Users/beatriznigri/ato-S1/apps/desktop/src-tauri && cargo check

Before committing, run a war-room (claude + google + minimax) on
your diff. Use the pattern in the operating instructions.

COMMIT:
  polish(minimax): prose-prefix tolerance + tighter strict-match

[paste operating instructions block here]
```

**When the session finishes — merge script (run from $REPO main):**
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git fetch origin
git checkout main
git merge --no-ff origin/session-S1-minimax-tighten -m "merge S1: minimax tighten"
git push
git worktree remove ../ato-S1
git branch -d session-S1-minimax-tighten
```

---

### Terminal B — Session S2 (Default dispatch prompt)

**Setup:**
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git worktree add ../ato-S2 -b session-S2-default-prompt
cd ../ato-S2
```

**Session prompt:**
```
Session S2 — Felipe P5: agents need a default dispatch prompt field.

Files you OWN (exclusive write):
  apps/desktop/src-tauri/src/schema.rs (additive ALTER TABLE only)
  apps/desktop/src-tauri/src/commands/mod.rs (ONLY create_agent fn +
    a NEW update_agent_default_prompt fn — DO NOT modify
    prompt_agent_inner, prompt_api_provider, or any other existing fn)
  apps/desktop/src/lib/agents.ts
  apps/desktop/src/lib/agentConversation.ts
  apps/desktop/src/components/AgentDetail/* (or wherever agent edit
    UI lives — find it via grep -rn "update_agent_" apps/desktop/src)

Touching anything else = STOP and surface (especially prompt_agent_inner
— S9 holds the lock for the actual "auto-fire on Run" wiring).

CONTEXT:
Felipe (early user) said in his v2.7.7 review: "no field to configure
a prompt that fires automatically when an agent is dispatched." His
monitoring agents (VPS health, telemetry) can't be one-click — every
run needs a manually-typed prompt.

TASK:
1. Schema: ALTER TABLE agents ADD COLUMN default_prompt TEXT (in
   schema.rs, same idempotent ALTER pattern as permissions_migrated_at).
2. Backend Rust:
   - Extend create_agent to accept Option<String> default_prompt,
     persist via INSERT.
   - New Tauri command update_agent_default_prompt(id, value).
   - Register in lib.rs's invoke_handler.
3. Frontend TS:
   - Add defaultPrompt?: string to AgentSpec in agentConversation.ts
     and round-trip through parse/serialize.
   - Add Agent.defaultPrompt to apps/desktop/src/lib/agents.ts and a
     wrapper updateAgentDefaultPrompt(id, value).
   - Add a <textarea> in the agent-edit UI with placeholder
     "Optional: prompt that fires automatically when this agent is
     dispatched. Leave blank for interactive prompts."

DO NOT WIRE THE "USE DEFAULT WHEN BLANK" LOGIC IN PROMPT_AGENT_INNER.
That's S9's job. Leave a TODO comment near prompt_agent_inner that
says "S9: when prompt is empty AND agent has default_prompt, use
default_prompt." But DO NOT change the function.

VERIFY:
  cd /Users/beatriznigri/ato-S2/apps/desktop/src-tauri && cargo check
  cd /Users/beatriznigri/ato-S2/apps/desktop && npx tsc --noEmit && npx vitest run --reporter=dot

War-room review on the diff before committing.

COMMIT:
  feat(agents): default dispatch prompt field — Felipe P5

[paste operating instructions block here]
```

**Merge:**
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git fetch origin && git checkout main
git merge --no-ff origin/session-S2-default-prompt -m "merge S2: default prompt field"
git push && git worktree remove ../ato-S2 && git branch -d session-S2-default-prompt
```

---

### Terminal C — Session S3 (Run-detail UI for denied events)

**Setup:**
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git worktree add ../ato-S3 -b session-S3-run-detail-denied
cd ../ato-S3
```

**Session prompt:**
```
Session S3 — Run-detail UI surfaces denied + advisory tool events.

Files you OWN (exclusive write):
  apps/desktop/src/components/SessionsList/SingleRunDetailView.tsx
  apps/desktop/src/components/SessionsList/_helpers.ts (only if NEW
    types are needed; don't modify existing fns)
  NEW: apps/desktop/src/components/SessionsList/PermissionEventsPanel.tsx

Touching anything else = STOP and surface.

CONTEXT:
v2.7.8 plumbed agent permissions through to dispatch but denied /
advisory events have no UI surface. tool_calls_summary already
includes is_error per call (see audit doc); we just don't render
it specifically when denial is the reason.

TASK:
1. New component PermissionEventsPanel that takes a parsed
   tool_calls_summary array and groups events into:
   - allowed (default; small green icon)
   - denied (is_error: true AND content contains "blocked by agent
     policy")
   - advisory (codex-style; matched against an "advisory_only"
     marker — for v2.7.11 the marker doesn't exist yet, so default
     this category to empty)
2. Embed PermissionEventsPanel inside SingleRunDetailView near the
   existing tool_calls rendering.
3. Add a unit test (vitest) for the parsing function:
   - parses well-formed JSON
   - handles malformed JSON (returns empty)
   - correctly categorizes a denied event

This is a frontend-only session. No Rust changes.

VERIFY:
  cd /Users/beatriznigri/ato-S3/apps/desktop && npx tsc --noEmit && npx vitest run --reporter=dot

War-room review on the diff before committing. For frontend code
review, claude + google are the strongest signal; minimax can spot-
check the JSX shape.

COMMIT:
  feat(ui): run-detail surfaces denied + advisory tool events

[paste operating instructions block here]
```

**Merge:**
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git fetch origin && git checkout main
git merge --no-ff origin/session-S3-run-detail-denied -m "merge S3: run-detail denied UI"
git push && git worktree remove ../ato-S3 && git branch -d session-S3-run-detail-denied
```

---

### Terminal D — Session S4 (.deb auto-update detection)

**Setup:**
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git worktree add ../ato-S4 -b session-S4-deb-update
cd ../ato-S4
```

**Session prompt:**
```
Session S4 — Felipe P2: .deb auto-update detection.

Files you OWN (exclusive write):
  NEW: apps/desktop/src-tauri/src/installer_detect.rs
  NEW: apps/desktop/src/components/UpdateBanner.tsx
  apps/desktop/src-tauri/src/lib.rs (ONLY add `pub mod
    installer_detect;` near the existing `pub mod api_dispatch;`
    line — DO NOT modify anything else)

Touching anything else = STOP and surface. Do not modify the
existing Tauri Updater integration.

CONTEXT:
Felipe ran ATO v2.4.8 for 6 days on his WSL2 Ubuntu machine without
knowing v2.7.7 had shipped. Reason: the Tauri Updater is configured
to swap an AppImage but his binary is at /usr/bin/ato-desktop
(root-owned, EACCES on auto-swap). The "OK" button on the update
modal silently fails.

TASK:
1. Create apps/desktop/src-tauri/src/installer_detect.rs exporting:
   - InstallMethod enum: Deb | AppImage | Snap | Unknown | NonLinux
   - pub fn detect_install_method() -> InstallMethod
     - On macOS / Windows: NonLinux.
     - On Linux: read /proc/self/exe symlink target.
       - If path starts with /usr/bin/ato-desktop AND `dpkg -S` of
         that path succeeds AND maps to package "ato" → Deb.
       - If path ends with .AppImage OR env APPIMAGE is set → AppImage.
       - If path under /snap/ → Snap.
       - Else Unknown.
   - pub fn manual_update_command(method: InstallMethod, version: &str,
     release_url: &str) -> Option<String>:
     - Deb → returns the sudo dpkg -r/wget/dpkg -i command string
       (per Felipe's workaround in his review)
     - AppImage → None (auto-update should work)
     - Snap → returns "sudo snap refresh ato"
     - Unknown → None
     - NonLinux → None
2. Tauri command: pub fn get_install_method() -> Result<String, String>
   returning JSON. Register in lib.rs invoke_handler.
3. Frontend: new UpdateBanner component that, when invoked,:
   - Calls get_install_method
   - If method == "deb" AND an update is available, replaces the
     normal "Install update" button with a CODE BLOCK containing the
     manual_update_command output + a "Copy" button
   - Else falls through to the default updater UI (don't replace it)
4. Wire UpdateBanner into the existing update-modal display location
   (find it via grep -rn "checkForUpdate\|update.available"
   apps/desktop/src).

Unit tests:
- detect_install_method returns NonLinux on macOS (use cfg!(target_os))
- manual_update_command returns the expected dpkg string for Deb

VERIFY:
  cd /Users/beatriznigri/ato-S4/apps/desktop/src-tauri && cargo test installer_detect
  cd /Users/beatriznigri/ato-S4/apps/desktop && npx tsc --noEmit && npx vitest run --reporter=dot

War-room review before commit.

COMMIT:
  fix(updater): detect .deb install + show manual upgrade command — Felipe P2

[paste operating instructions block here]
```

**Merge:**
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git fetch origin && git checkout main
git merge --no-ff origin/session-S4-deb-update -m "merge S4: .deb update detection"
git push && git worktree remove ../ato-S4 && git branch -d session-S4-deb-update
```

---

### Terminal E — Session S5 (Empty-MCPs warning)

**Setup:**
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git worktree add ../ato-S5 -b session-S5-empty-mcps-warning
cd ../ato-S5
```

**Session prompt:**
```
Session S5 — Felipe P6: warn when no MCPs registered during agent
creation.

Files you OWN (exclusive write):
  apps/desktop/src/components/CreateAgentWizard/QuickPath.tsx (only
    the MCP-selection section)
  apps/desktop/src/components/CreateAgentWizard/GuidedPath.tsx (only
    the MCP-selection section)
  NEW: apps/desktop/src/components/CreateAgentWizard/EmptyMcpsWarning.tsx

Touching anything else = STOP.

CONTEXT:
Felipe P6: "When creating a new agent and reaching the MCP selection
field, the list is empty if no MCPs have been registered beforehand
— even official catalog MCPs." New users create agents without MCPs
unknowingly.

TASK:
1. Find the MCP selection step in CreateAgentWizard. Both QuickPath
   and GuidedPath likely have one — handle both.
2. Detect: when the list of available MCPs is empty AND user is on
   the MCP step.
3. New EmptyMcpsWarning component renders:
   - Yellow/orange warning card
   - Title: "No MCPs registered"
   - Body: "MCPs give your agent tools like gmail, github,
     filesystem. Browse the catalog or skip and add them later."
   - Primary CTA: "Browse catalog" → calls useUiStore's setSection
     ("skills") + setSubTab("ato.subtab.skills", "mcps") via the
     existing patterns
   - Secondary action: "Skip for now" (just dismisses the warning
     for this session)
4. Render the warning above the empty selector instead of the empty
   selector.

This is a frontend-only session. No backend changes.

VERIFY:
  cd /Users/beatriznigri/ato-S5/apps/desktop && npx tsc --noEmit && npx vitest run --reporter=dot

For testing the warning: render the wizard with a mocked empty MCP
list, assert the warning appears.

War-room review before commit.

COMMIT:
  feat(wizard): warn when no MCPs registered yet — Felipe P6

[paste operating instructions block here]
```

**Merge:**
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization
git fetch origin && git checkout main
git merge --no-ff origin/session-S5-empty-mcps-warning -m "merge S5: empty MCPs warning"
git push && git worktree remove ../ato-S5 && git branch -d session-S5-empty-mcps-warning
```

---

## After TIER 1 merges — TIER 2 setup

Wait for all 5 TIER 1 PRs to land on `main`. Then for each of S6/S7/S8, do the same `git worktree add ../ato-S<N> -b session-S<N>-...` pattern with the prompts in the TIER 2 / TIER 3 task descriptions above. The same operating-instructions block applies.

Critically: **never run TIER 2 sessions until TIER 1 has merged**, or the worktrees won't share the TIER 1 changes and merges will conflict.

---

## TL;DR cadence (what you do)

1. Open 5 terminals.
2. Copy-paste the setup + Claude Code prompt for S1–S5, one per terminal.
3. While the 5 sessions run, you do other things. Each will war-room with claude/google/minimax for plan + review, then commit on its branch.
4. When each session pings you "done", run its merge script. Pre-push gate validates.
5. When all 5 TIER 1 are on main, kick off TIER 2 (3 more parallel sessions).
6. After TIER 2 lands, TIER 3 is a single session you can run when convenient.

**Total wall time**: 3-5 hours for TIER 1, another 2-3 hours for TIER 2, ~2 hours for TIER 3 — vs. ~20 hours sequential.
