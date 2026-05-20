# v2.7.9 dogfood — step-by-step verification

This doc walks you through proving the **MiniMax tool-call loop** end-to-end, plus regression checks for everything v2.7.8 already shipped.

Date: 2026-05-20
Scope: v2.7.9 = PR-A (MiniMax content-as-args). PR-B (MCP-tool gating) deferred to v2.7.10 — see "Deferred" section.

## What v2.7.9 ships

**One credibility-load-bearing fix:** MiniMax-M2.7-highspeed emits function-call arguments as plain JSON in `message.content` rather than the OpenAI `choices[0].message.tool_calls[]` shape. Until v2.7.9, this meant MiniMax received tools in its prompt but the loop never engaged — the model's args-as-content was treated as text and the user saw a "hallucinated" code block (the model intended to call a tool, the loop just never saw the call).

v2.7.9 closes that gap. The loop now:

1. **Detects content-as-args** (`apps/cli/src/api_dispatch_tools.rs::parse_minimax_content_as_tool_calls` + mirror in `apps/desktop/src-tauri/src/api_dispatch_tools.rs`): parse `message.content` as JSON, strict-match against offered tools' `input_schema`, require exactly one match (zero or multiple → treat as plain text — claude's #2 risk fix).
2. **Strips fenced JSON blocks** (`​```json ... ​````): models often wrap intended tool args in fences. Stripped before parsing.
3. **Synthesizes history-echo `tool_calls[]`** when payload lacks them but content matched a tool. Otherwise the next round's `tool_result` would reference a `tool_call_id` that doesn't exist in any prior assistant message — claude's #3 risk fix.

**Tests:** 9 new unit tests pin: strict match, fenced-block stripping, missing-required, extra-keys, ambiguous, non-JSON, JSON array, native `tool_calls` precedence, synthesized assistant message has `tool_calls[]`.

**Total tests as of v2.7.9:** 69 CLI + 4 review-tools + 10 perms + 20 frontend = **103 green**.

## Verifying the fix — three terminals + the desktop GUI

> All commands use absolute paths and don't depend on `cd` or env vars.

### Step 1 — Fresh terminal: confirm the CLI binary is current
```bash
ls -la /Users/beatriznigri/Agentic-Tool-Optimization/apps/cli/target/release/ato
```
mtime should be `20 Mai 19:14` (or later, after the PR-A rebuild). If older:
```bash
cd /Users/beatriznigri/Agentic-Tool-Optimization/apps/cli && cargo build --release
```

### Step 2 — Confirm a MiniMax-runtime agent has migrated tool permissions

Pick (or create) an agent on the `minimax` runtime, then stamp tool perms + migration:
```bash
# Check what minimax agents you have
sqlite3 /Users/beatriznigri/.ato/local.db "SELECT slug, runtime, permissions, permissions_migrated_at FROM agents WHERE runtime='minimax';"
```

Stamp one — e.g. `distribution-fixer`:
```bash
sqlite3 /Users/beatriznigri/.ato/local.db "UPDATE agents SET permissions = '[\"allow:read_file\",\"allow:grep\",\"allow:git_log\"]', permissions_migrated_at = datetime('now') WHERE slug='distribution-fixer' AND runtime='minimax';"
```

Verify:
```bash
sqlite3 /Users/beatriznigri/.ato/local.db "SELECT slug, runtime, permissions, permissions_migrated_at FROM agents WHERE slug='distribution-fixer' AND runtime='minimax';"
```
Should show non-NULL permissions + non-NULL `permissions_migrated_at`.

### Step 3 — Open the desktop GUI

1. Make sure an **active project** is selected in the left sidebar.
2. Create a new session: **Runs → Sessions → + New session** → runtime `claude`, blank title, blank agent. Create.
3. In the session, change runtime dropdown from `claude` to **minimax**.
4. From the **agent picker** next to it, choose your migrated agent (e.g. `distribution-fixer`).
5. Paste this prompt:
   ```
   Use your read_file tool to read apps/cli/src/main.rs lines 80 to 95.
   Quote those exact lines back to me verbatim inside a fenced code block.
   Then in one sentence explain what they do.
   ```
6. Click **Send**.

### Step 4 — Verify the loop fired

In a terminal:
```bash
sqlite3 /Users/beatriznigri/.ato/local.db "SELECT runtime, agent_slug, tool_calls_count, substr(tool_calls_summary,1,200), tokens_in, tokens_out FROM execution_logs ORDER BY created_at DESC LIMIT 1;"
```

**Expected (PR-A success):**
- `runtime=minimax`
- `agent_slug=distribution-fixer` (or whichever you picked)
- `tool_calls_count > 0` (typically `1`)
- `tool_calls_summary` shows `[{"name":"read_file","args_brief":"{...path:apps/cli/src/main.rs...}","is_error":false}]`
- `tokens_in` is high (~1500+ tokens — file content fed back to the model in round 2)
- `tokens_out` is the model's verbatim quote + brief explanation

**In the GUI**, the response should contain the **actual verbatim content** of `apps/cli/src/main.rs:80-95`:
```
    /// Dispatch a prompt to a runtime
    Dispatch {
        /// Runtime: claude, codex, gemini, openclaw, hermes
        runtime: String,
        ...
```

If you see that exact text quoted back, **the MiniMax content-as-args fix is working end-to-end**.

### Step 5 — If something doesn't match expected

The PR-A path has fallbacks at every layer. Common failure modes:

| Symptom | Likely cause | Fix |
|---|---|---|
| `tool_calls_count` is NULL | Tool loop didn't engage. Check `permissions` + `permissions_migrated_at` on the agent. | Re-stamp via Step 2. |
| `tool_calls_count = 0` | Loop engaged but model declined to call any tool. Prompt didn't explicitly require file reads. | Use the prompt in Step 3 (it explicitly says "Use your read_file tool"). |
| Response is hallucinated code (not actual file) | MiniMax model didn't emit JSON in `content`. The model may have produced prose with embedded code. The content-as-args detector requires the ENTIRE content to be a JSON object (optionally inside a `​```json` fence). | Re-prompt with clearer "use the tool" instruction. v2.7.10 may add prose-prefix tolerance. |
| `tool_calls_count > 0` but second-round content is wrong | History-echo synthesis bug. Run `cargo test minimax_synthesized_assistant_message_has_tool_calls` to confirm test passes. | If test passes but live behaviour fails, paste the execution_logs row here. |

## Regression checks (everything v2.7.8 already shipped)

Quick smoke that you haven't broken anything from v2.7.8:

```bash
# 18-cmd pre-push smoke
cd /Users/beatriznigri/Agentic-Tool-Optimization && .githooks/pre-push 2>&1 | tail -5
```
Expect: `✓ pre-push gate passed (18 CLI commands)`.

```bash
# Anthropic + devex regression (v2.7.8 hero path)
# In the desktop GUI: war-room with anthropic + devex agent + same read prompt.
# tool_calls_count=1 expected.
```

```bash
# Cross-runtime mirror agent (v2.7.8 dogfood fix)
# Confirm devex on google still works through gemini auto-fallback:
sqlite3 /Users/beatriznigri/.ato/local.db "SELECT slug, runtime, permissions IS NOT NULL FROM agents WHERE slug='devex';"
```
Should show two rows (gemini + google), one of them migrated. PR-3c cross-runtime fallback picks the migrated one regardless of which the dispatch routes through.

## Deferred to v2.7.10

**PR-B (Desktop MCP-tool gating)** was implemented and reverted after the v2.7.9 war-room (`A803A3C3-…` planning + review). Two real ship blockers caught by claude + google reviewers:

1. **Sync MCP discovery inside an async Tauri command blocks the tokio worker.** `discover_mcp_server_tools` is a multi-second stdio handshake; calling it directly in `prompt_api_provider` (async fn) blocks all parallel commands on the worker thread. Fix: wrap each MCP discovery in `tokio::task::spawn_blocking`.

2. **Offering MCP tools to the model without an execution path is deceptive UX.** Today's `review_tools::execute_call` returns "unknown tool" for any name not in its built-in registry. Offering an `MCP-discovered.tool` to the model and then erroring on the call wastes tokens and breaks the user-visible promise. Fix: implement MCP-server round-trip execution (stdio re-spawn, send `tools/call` JSON-RPC, parse result, append).

Both fixes are scoped for v2.7.10. The kept piece of PR-B work:
- `ato_review_tools::ToolDef.name` / `.description` changed from `&'static str` to `String` (needed for runtime MCP tool catalogues). Internal-only, no behaviour change today.

## What the war-rooms revealed about each LLM (Will's "compare them" ask)

I ran two war-rooms during v2.7.9:

### Planning war-room (`A803A3C3-…`, 2026-05-20 20:09 UTC)
4 seats requested: claude, codex, google, minimax. Codex was rate-limited (returns until 2026-05-24). The 3 working seats:

| Seat | Verdict |
|---|---|
| **Claude** | Most thorough. Caught the unique blocker (history-echo bug — payload echoes verbatim but lacks tool_calls[], so next round's tool_results dangle). Predicted PR-B would need an MCP shared crate, ~600 LOC if extracted vs ~200 LOC if duplicated. Gave concrete algorithm pseudocode. |
| **Google** | Comprehensive but generic. Same algorithm but didn't catch the history-echo bug. Listed risks well but flagged "schema matching uniqueness" rather than the specific dangling tool_call_id failure mode. |
| **MiniMax** | Provided a working pseudocode in Rust + good risk table. Caught the schema-collision case and the CLI/desktop-port-sync risk. **Did not catch the history-echo bug.** Ironic because MiniMax is the runtime being fixed; if it had been able to read the code (which it couldn't because the fix is still pending for its tool loop on the LIVE binary), it might have caught more. |

**Takeaway:** Claude is the deepest reviewer for design-doc-style planning. Google is a good consensus check. MiniMax is fine for "spot risks" but misses subtle integration bugs.

### Code-review war-room (`v279_review_wr`, 2026-05-20 22:17 UTC)
Same 3 seats. Both claude and google identified the SAME two ship-blockers for PR-B:
1. Sync call in async context → tokio worker blocked.
2. MCP tools offered but unexecutable → deceptive UX.

Both gave **DON'T SHIP** on PR-B. Both said SHIP on PR-A.

MiniMax tried to use a `ReadMultipleFiles` tool in its response — which isn't a real tool offered to it. This confirms the v2.7.9 fix's importance: MiniMax IS attempting tool calls; the model emission shape just doesn't match OpenAI's. Once v2.7.9 ships and MiniMax has tool surface like the other API providers, its reviews will be much more grounded.

**Takeaway:** For code review with strict-correctness needs, claude is the strongest signal. Google is a great parallel sanity check. MiniMax will be more useful for code review once v2.7.9 ships (its tool loop becomes real).

## v2.7.10 docket

- **PR-B with proper MCP execution** — spawn_blocking wrap + MCP-server `tools/call` round-trip.
- **MiniMax tool reliability follow-ups** — content-as-args is a fallback; explore tightening the strict-match (require non-empty `required` OR keys-intersect-properties when `required` is absent — claude's risk 4a from review).
- **Extract async loop body** — CLI's blocking dispatch_with_tools and desktop's async port share the same Conversation logic. Pull both into a shared async core + CLI block_on shim.
- **Run-detail UI for denied/advisory events** — surface the agent's permission rule that fired when a tool was denied.
- **Tauri-webdriver pre-tag smoke pass** — automate the 7-step golden path so we catch regressions before pushing tag commits.
- **MiniMax content-as-args prose-prefix tolerance** — handle "I'll call read_file: ```json {...} ```" style emission where prose precedes the fenced block.

---

**War-room provenance for v2.7.9:**
- Planning: `A803A3C3-0C75-4A7A-98AB-7778D06318CE`
- Code review: war_room_id captured at `/tmp/v279_review_wr`

**Audit doc reference:** `docs/audits/agent-permissions-plumb-through-2026-05-20.md` (still the canonical v2.7.8+ design doc).
