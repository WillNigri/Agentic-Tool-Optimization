# Agent permissions plumb-through — audit + design

Date: 2026-05-20
Scope: v2.7.8 slice E (agent-permission enforcement) **combined with** v2.8.0 API-provider tool-call loop. Will called the relationship explicitly: both halves of the same multi-LLM credibility story.

## TL;DR

ATO's CreateAgentWizard surfaces `permissions.{allowed, requireApproval, denied}` to the user as a concrete promise — "this agent is allowed X, must ask before Y, may never Z." The desktop backend persists those promises onto the `agents` row. **No dispatch path reads them.** Every runtime is invoked with a single hardcoded flag bundle, agnostic to the agent's spec.

Separately, API-runtime dispatch (`anthropic`, `google`, `minimax`, `grok`, `deepseek`, `qwen`, `openrouter`) hardcodes `tool_calls: None` in every response path. API runtimes are pure text-in / text-out; they have zero tool surface today, which means half the war-room seats reason blind about the user's code.

These are two halves of the same gap: **what should `agent.permissions` translate into at the runtime boundary?** This audit defines the DSL, traces the gap, and plans the wiring.

## 1. What the wizard promises

### 1.1 Type shape (frontend)

`apps/desktop/src/lib/agentConversation.ts:26-35`:

```ts
export type Permissions = {
  summary: string;            // 1-line plain-English summary, shown on review card
  allowed: string[];          // semantic action labels (e.g. "read_emails")
  requireApproval: string[];  // labels the agent must ask before doing
  denied: string[];           // labels the agent must never do
};
```

The conductor LLM produces this object inside the wizard's `review` turn (`agentConversation.ts:96-100`). Examples seen in dispatch transcripts: `read_emails`, `draft_replies`, `send_emails`, `transfer_funds`, `web_search`, `write_files`, `Bash(rm:*)`. **The label space is freeform** — the LLM picks labels per agent, mixing domain-level verbs (`send_emails`) with runtime-native patterns (`Bash(rm:*)`).

### 1.2 What the user sees

`apps/desktop/src/components/CreateAgentWizard/GuidedPath.tsx:551-580` renders the review card with three sections explicitly labelled in user-facing copy:

- **Permissions** — the allow list
- **Asks first** — `requireApproval`
- **Never** — `denied`

This is the promise. The user sees these strings, clicks "Create agent," and assumes ATO will enforce them.

### 1.3 What gets persisted (the flattening)

`GuidedPath.tsx:174-179` flattens the structured object into a tagged-string array before submission:

```ts
const permissionList = [
  ...spec.permissions.allowed.map((a) => `allow:${a}`),
  ...spec.permissions.requireApproval.map((a) => `approve:${a}`),
  ...spec.permissions.denied.map((a) => `deny:${a}`),
];
```

This passes to `createAgent({ permissions: permissionList })` (`apps/desktop/src/lib/agents.ts:131,149`), which on the backend lands at `commands/mod.rs:5375,5421`:

```rust
permissions: Option<Vec<String>>,        // CreateAgent input
// ...
let permissions_json = effective_permissions.as_ref()
    .map(|v| serde_json::to_string(v).unwrap_or_default());
// → stored as a JSON-encoded string in agents.permissions column
```

**Finding 1 (lossy round-trip):** the `summary` field is not persisted. The three buckets are union'd with a `:` tag prefix. Round-trip back to the structured shape requires parsing tags. No code reads it back today — see Finding 2.

**Finding 2 (read site count):** zero. `rg "permissions" apps/desktop/src-tauri/src/commands/mod.rs apps/cli/src/commands/dispatch.rs` shows the column is written (create_agent) and read only by the agent-detail UI for display. No dispatch path reads it.

## 2. What dispatch enforces today

### 2.1 Desktop dispatch — `prompt_agent_inner` (commands/mod.rs:807)

Signature accepts `agent_slug: Option<String>` (line 811). The slug is **only** passed through to `dispatch_command_killable` for execution-log attribution (`mod.rs:974-983`). No SELECT from `agents` reads the permissions column on dispatch.

Per-runtime hardcoded flag bundles (current state, all uniform across agents):

| Runtime | What gets passed today (commands/mod.rs) | Permission-aware? |
|---|---|---|
| `claude` (line 833) | `--print <prompt>` + `--allowedTools "Bash(ato:*) Bash(gemini:*) Bash(codex:*) Bash(openclaw:*) Bash(hermes:*) Bash(minimax:*)"` | No — uniform allowlist of sibling-runtime bashes. Ignores `denied` / `requireApproval`. |
| `codex` (line 859) | `exec --skip-git-repo-check --sandbox workspace-write -c approval_policy="never"` | No — uniform full workspace-write unlock. |
| `gemini` (line 936) | `-p <prompt>` only | No — no agentic flags at all. Defaults to gemini CLI's on-request approval, which hangs headlessly (no PTY). |
| `openclaw` (line 901) | `ssh <host> openclaw exec <prompt>` (escaped) | No — pass-through; openclaw enforces its own. |
| `hermes` (line 929) | `--execute <prompt>` | No — pass-through. |

### 2.2 CLI dispatch — `apps/cli/src/commands/dispatch.rs`

Mirrors the desktop's flag bundles for claude (line 530), codex (line 564), gemini (line 587), openclaw (line 596), hermes (line 593). Same uniform unlocks, same no-read-of-permissions for flag construction.

The CLI already exposes `--agent <slug>` (`apps/cli/src/main.rs:82-95`), but today it's used only to prepend the agent's `system_prompt` as a persona block (`apps/cli/src/commands/dispatch.rs:503-511` calls `prepend_agent_persona`) and to record the slug for telemetry. The slug never reaches the per-runtime flag construction below it. No surface change needed for v2.7.8 — the wiring change is "also read `permissions` when reading the agent row for persona."

### 2.3 API-provider dispatch — `apps/cli/src/api_dispatch.rs`

The `ApiDispatchOutcome` struct at `api_dispatch.rs:37-50` declares `tool_calls: Option<Vec<ToolCallAudit>>` — the audit shape was scaffolded. **Every of 15 construction sites hardcodes `tool_calls: None`** (lines 266, 376, 452, 469, 480, 498, 522, 550, 565, 585, 631, 656, 698, 712, 739). No provider in `packages/ato-api-providers/src/lib.rs` advertises tool support; the registry just carries `(base_url, path, default_model, env_var, flavor)`. The dispatch HTTP body does not include a `tools` field.

**Finding 3 (no tool surface):** API runtimes today cannot read files, cannot grep, cannot patch. In a war-room, the API seats reason about whatever's pasted into the prompt; only the CLI seats (codex, claude) actually investigate. The "compare every LLM on your code" pitch is honest only for CLI runtimes today.

### 2.4 BYOK env-var passthrough — `apps/cli/src/byok.rs:36-43`

```rust
fn runtime_byok_env(runtime_name: &str) -> Option<(&'static str, &'static str)> {
    match runtime_name {
        "claude" => Some(("ANTHROPIC_API_KEY", "anthropic")),
        "codex" => Some(("OPENAI_API_KEY", "openai")),
        "gemini" => Some(("GEMINI_API_KEY", "google")),
        _ => None,
    }
}
```

This already encodes the runtime → provider mapping that slice D (CLI→API auto-fallback) will use. Once the API providers gain tool surface (slice E/tool-call loop), this same map tells us "if the CLI is missing and the key is present, route to the API provider that gets the same permission DSL applied."

## 3. The unified DSL → enforcement model

The wizard's DSL is **semantic action labels**, not runtime-native patterns. The translation layer maps one to the other per runtime. Three categories of label:

- **Domain-level verbs** (`send_emails`, `transfer_funds`, `book_meeting`) — these are MCP/tool invocations. Enforced at the tool-call site, not at the runtime spawn site.
- **Tool-pattern strings** (`Bash(rm:*)`, `Write`, `Read`) — Claude Code's native vocabulary. Map directly to `--allowedTools` and to `permissions.allow|deny` in `.claude/settings.local.json`.
- **Coarse capabilities** (`fs_read`, `fs_write`, `shell`, `network`) — codex's level of granularity. Map to one of three `--sandbox` modes.

### 3.1 Enforcement capability per runtime

| Runtime | Granularity | Surface |
|---|---|---|
| Claude Code | tool-pattern (fine) | `--allowedTools` + `~/.claude/settings.local.json` `permissions.{allow,deny,ask}` |
| Codex | coarse capability (3 modes) | `--sandbox <read-only\|workspace-write\|danger-full-access>` + `-c approval_policy=<…>` |
| Gemini CLI | binary | `--yolo` (full) or default (on-request approval, hangs headlessly) |
| OpenClaw / Hermes | pass-through | runtime enforces its own; ATO surfaces metadata only |
| API providers | tool-call gate | request-body `tools` field + ATO-side interceptor on every `tool_call` |

**Codex correction (war-room finding):** codex does **not** support per-tool deny rules. `--sandbox` is a 3-value enum; there is no `sandbox_permissions=["!shell:rm*"]` flag. Earlier drafts of this audit invented that flag — removed. On codex, any agent-level denial finer than the 3 modes is **advisory**: ATO records it, the dispatch path can demote `workspace-write` → `read-only` if any denial label is present, and the run-detail UI surfaces "policy is advisory on codex, not enforced." See §6 Q2 for the long-form decision.

### 3.2 Per-runtime translation

#### Claude Code

| ATO bucket | Emitted by `to_claude(p)` |
|---|---|
| `allowed: [Read, Grep, "Bash(npm:*)"]` | `--allowedTools "Read Grep Bash(npm:*)"` + `settings.local.json.permissions.allow = [...]` |
| `requireApproval: ["Bash(git push:*)"]` | omitted from `--allowedTools`, added to `permissions.ask` |
| `denied: ["Bash(rm:*)"]` | omitted from `--allowedTools`, added to `permissions.deny` |
| External-kind auto-lock | `--allowedTools "Read Grep Glob WebFetch"`, no write/bash tools |

#### Codex

| ATO bucket | Emitted by `to_codex(p)` |
|---|---|
| Any `denied.is_empty() && requireApproval.is_empty()` AND `allowed` includes write/shell labels | `--sandbox workspace-write -c approval_policy="never"` (today's default) |
| Any non-empty `denied` or `requireApproval` (advisory on codex) | `--sandbox read-only -c approval_policy="never"` + emit `advisory_only` list for UI surfacing |
| External-kind auto-lock | `--sandbox read-only -c approval_policy="never"` |

#### Gemini CLI

| ATO bucket | Emitted by `to_gemini(p)` |
|---|---|
| `allowed` covers all of {Read, Write, Bash} AND empty `denied` / `requireApproval` | `--yolo` |
| Any narrower spec | `yolo: false, error: Some("Gemini CLI does not support fine-grained permissions. Switch this agent's runtime to `google` API provider, or broaden permissions.")` |
| External-kind auto-lock | error (gemini can't enforce read-only structurally) |

#### OpenClaw / Hermes

Pass-through. `to_openclaw(p)` and `to_hermes(p)` emit metadata only. The runtimes enforce their own SOUL.md / TOOLS.md surface; ATO records the policy on the agent file (see §1.3) so a future audit / mesh-relay can read it back.

#### API providers (tool-call gate)

| ATO bucket | Emitted by `to_api_tool_gate(p)` → consumed by `dispatch_with_tools` |
|---|---|
| `allowed: [...]` | request body's `tools` field is populated only with definitions whose name matches `allowed` |
| `requireApproval: [...]` | interceptor pauses on matching `tool_call`, writes `pending_approval` row, blocks until UI replies |
| `denied: [...]` | interceptor refuses the `tool_call` before execution; appends `{ role: "tool", content: { error: "blocked by agent policy" } }` and re-dispatches |
| External-kind auto-lock | only read-class tools in the `tools` field; everything else refused |

**Translation-layer module location:** new crate `packages/ato-agent-permissions/`. War-room verdict from claude (with codex agreeing on the diagnosis): `packages/ato-api-providers/src/lib.rs:1-12` is explicitly scoped to "ApiProvider struct + registry() list — dispatch HTTP logic stays in the consuming crate." Permission translation is broader domain logic (claude/codex/gemini flag construction is unrelated to API-provider registry data) and stuffing it into `ato-api-providers` would break that crate's clean charter. Net cost of a new crate: one `Cargo.toml` + one workspace entry. Paying it now beats the "extract later" promise that historically doesn't happen.

Public surface (sketch):

```rust
// packages/ato-agent-permissions/src/lib.rs
pub struct AgentPermissions {
    pub allowed: Vec<String>,
    pub require_approval: Vec<String>,
    pub denied: Vec<String>,
}

pub fn parse_permissions_column(json: &str) -> AgentPermissions { /* parse allow:/approve:/deny: tagged strings */ }

pub struct ClaudeFlags { pub allowed_tools: String, pub settings_local: serde_json::Value }
pub struct CodexFlags  { pub sandbox: &'static str, pub approval_policy: &'static str, pub advisory_only: Vec<String> }
pub struct GeminiFlags { pub yolo: bool, pub error: Option<String> }
pub struct ToolGate    { pub allowed_tools: Vec<ToolDef>, pub approval_required: Vec<String>, pub denied: Vec<String> }

pub fn to_claude(p: &AgentPermissions) -> ClaudeFlags { ... }
pub fn to_codex (p: &AgentPermissions) -> CodexFlags  { ... }
pub fn to_gemini(p: &AgentPermissions) -> GeminiFlags { ... }
pub fn to_openclaw(p: &AgentPermissions) -> serde_json::Value { ... }
pub fn to_hermes  (p: &AgentPermissions) -> serde_json::Value { ... }
pub fn to_api_tool_gate(p: &AgentPermissions) -> ToolGate { ... }
```

Pure functions, no I/O — golden-testable in isolation.

Public surface (sketch):

```rust
pub struct AgentPermissions {
    pub allowed: Vec<String>,
    pub require_approval: Vec<String>,
    pub denied: Vec<String>,
}

pub fn parse_permissions_column(json: &str) -> AgentPermissions { ... }

pub struct ClaudeFlags { pub allowed_tools: String, pub settings_local: serde_json::Value }
pub struct CodexFlags  { pub sandbox: &'static str, pub approval_policy: &'static str, pub sandbox_perms: Vec<String> }
pub struct GeminiFlags { pub yolo: bool, pub error: Option<String> }
pub struct ToolGate    { pub allowed_tools: Vec<ToolDef>, pub approval_required: Vec<String>, pub denied: Vec<String> }

pub fn to_claude(p: &AgentPermissions) -> ClaudeFlags { ... }
pub fn to_codex (p: &AgentPermissions) -> CodexFlags  { ... }
pub fn to_gemini(p: &AgentPermissions) -> GeminiFlags { ... }
pub fn to_api_tool_gate(p: &AgentPermissions) -> ToolGate { ... }
```

Test surface: golden tests pinning the DSL → flags mapping per runtime. Keeps changes auditable.

## 4. API-provider tool-call loop — concrete design

Today's `api_dispatch.rs` shape (verified): each provider has its own HTTP function (e.g. `dispatch_minimax`, `dispatch_google`) that wraps `messages -> response`. The tool-call loop adds:

1. **Request body** — when the agent has tools, include the provider-flavored `tools` field:
   - **OpenAI flavor** (`openai`, used by minimax/grok/deepseek/qwen/openrouter): `{ "tools": [{ "type": "function", "function": { "name", "description", "parameters" } }] }` + `"tool_choice": "auto"`.
   - **Anthropic flavor**: `{ "tools": [{ "name", "description", "input_schema" }] }` + `"tool_choice": { "type": "auto" }`.
   - **Google/Gemini flavor**: `{ "tools": [{ "functionDeclarations": [...] }] }`. Function call lives in `candidates[0].content.parts[0].functionCall`.
2. **Response parsing** — parse provider-flavored tool-call shape, normalize into `ToolCallAudit { name, args_brief, is_error }`.
3. **Gate check** — call `to_api_tool_gate(&perms).check(&tool_call)`:
   - if `denied` matches → append error tool result, re-dispatch.
   - if `requireApproval` matches → write `pending_approval` row, surface in UI, block.
   - otherwise → execute.
4. **Tool execution** — local subprocess; same workspace boundary as codex (`workspace-write`). **MVP tools: `read_file`, `grep`, `list_directory` only.** Write tools (`write_file`), shell, and `web_fetch` deferred to PR-5 because their gating value depends on the `requireApproval` UI flow which lands there. War-room finding: shipping read-only tools first solves the "API seats reason blind" credibility hole without scope-creeping the approval-pause UI into PR-3.
5. **Re-dispatch** — append `{ role: "tool", tool_call_id, content }` (or provider equivalent), POST again. Loop until response has no more `tool_calls`. Cap at N iterations to prevent runaway.

Per-provider divergence is real:
- Anthropic's `tool_use` content block in the response is structurally different from OpenAI's `message.tool_calls`.
- Google's function-call response is one of many candidate parts; have to filter.
- MiniMax doesn't document tool support consistently — first verification pass may discover it's not viable; mark UNVERIFIED in the registry until smoke-tested.

`ToolCallAudit` already exists in `apps/cli/src/api_dispatch.rs:55-60`. The persistence columns also already exist: `execution_logs.tool_calls_count` (INTEGER) and `execution_logs.tool_calls_summary` (TEXT, JSON array of `{name, args_brief, is_error}`) — added in v2.4.5 at `apps/desktop/src-tauri/src/schema.rs:372-378`. CLI dispatch writes them at `apps/cli/src/commands/dispatch.rs:1077` (today always 0 / empty for API providers). Wiring is just "populate these two columns from `outcome.tool_calls` instead of hardcoded zero."

## 5. Implementation plan (PR-by-PR)

This is the v2.7.8 + v2.8.0-merged scope. Five PRs.

### PR-1: Translation layer crate + golden tests (~1 day)

- New crate `packages/ato-agent-permissions/` with `Cargo.toml` + workspace registration + `src/lib.rs` (types + `to_claude / to_codex / to_gemini / to_openclaw / to_hermes / to_api_tool_gate` pure functions).
- No behaviour change in apps yet — crate compiles, tests pass, nothing imports it.
- Golden tests (minimum 8 cases, all pure-function asserts):

  1. **Empty permissions → backward-compat defaults.** Each runtime's flag bundle matches the current hardcoded output at `commands/mod.rs:833,859,936`. This is the invariant that protects pre-v2.7.8 agents from silent behaviour changes.
  2. **`denied: ["Bash(rm:*)"]` cross-runtime.** Claude omits `Bash(rm:*)` from `--allowedTools` AND adds it to `settings_local.permissions.deny`. Codex demotes to `--sandbox read-only` AND records `"Bash(rm:*)"` in `advisory_only`. Gemini returns `yolo: false, error: Some(…)`.
  3. **`allowed: ["Read","Grep"], denied: ["Write"]` on gemini.** Returns `yolo: false, error: Some("Gemini CLI does not support fine-grained permissions…")`.
  4. **`allowed: ["Read","Grep","Write","Bash"]`, all empty otherwise.** Gemini returns `yolo: true, error: None`.
  5. **External-kind auto-lock** (mirrors `commands/mod.rs:5407-5413`). `ToolGate.allowed_tools` is read-class only (`read_file`, `grep`, `list_directory`, `web_fetch`). Claude's `--allowedTools` is exactly `"Read Grep Glob WebFetch"`.
  6. **`requireApproval: ["send_emails"]` semantic label.** `ToolGate.approval_required` contains it. Codex flags demote to `read-only`. Claude's `permissions.ask` contains it; `--allowedTools` omits it.
  7. **Round-trip parse/serialize.** `parse_permissions_column(serialize(p)) == p` for the tagged-string format from `GuidedPath.tsx:174-179`. Documents the lost-`summary` behaviour (see Finding 1, §1.3) — round-trip is lossy by design until the persistence shape is fixed in a later PR.
  8. **Unknown label on codex.** `denied: ["transfer_funds"]` (semantic, no runtime-native equivalent). Codex emits `advisory_only: ["transfer_funds"]` — translation succeeds, dispatch path's telemetry tracks how often these fire so we can quantify exposure per §6 Q2.

### PR-2: Wire desktop + CLI dispatch to read agent.permissions (~half day)

- `prompt_agent_inner` (commands/mod.rs:807): when `agent_slug.is_some()`, SELECT the `permissions` column, parse, call `ato_agent_permissions::to_<runtime>(&perms)`, splice the flags into the existing `Command` construction.
- `apps/cli/src/commands/dispatch.rs`: already reads `agent_slug` for persona prepending at line 503; extend the same SELECT to also load `permissions`, apply the same translation. **No new CLI surface needed** — `--agent <slug>` already exists at `apps/cli/src/main.rs:82-95`.
- Today's uniform unlocks become **defaults when permissions == None** (dispatch without an agent slug, or agents with NULL permissions). Backward compatibility preserved — protected by PR-1 golden test #1.
- New test: an agent with `denied: ["Bash(rm:*)"]` dispatched via codex spawns with `--sandbox read-only` (the demotion path) and the dispatch's telemetry payload includes `"Bash(rm:*)"` in an advisory-only field. Intercept `Command::get_args` to verify. No flag invention.

### PR-3: API-provider tool-call loop — anthropic + google (~1 day)

- Pick anthropic and google first because their tool schemas are best-documented in the team and `last verified: 2026-05-13 ✓` for google.
- Implement `dispatch_with_tools()` in `api_dispatch.rs`: builds tools-field, loops on `tool_calls`, runs the executor, re-dispatches.
- Tool executor MVP: `read_file`, `grep`, `list_directory`. Gated by `to_api_tool_gate`. Out-of-workspace paths refused without consulting the gate.
- Populate `tool_calls: Some(audit)` on the outcome — first time the field stops being `None`.
- Verify with: dispatch anthropic in a war-room asking it to read a specific file at a specific line range; expect the response to demonstrate it actually read the file.

### PR-3a: MCP-tool gating (folded into PR-3 scope, ~half day extra)

The credibility-load-bearing item: without this, the wizard's named-action permissions (`allowed: ["send_emails"]`, `denied: ["transfer_funds"]`) are purely decorative for any agent attached to an MCP server.

- Extend `to_api_tool_gate(perms, mcps)` to take the agent's attached MCP list as a second argument.
- For each MCP in `agent.mcps`, read its declared tool catalogue (the MCP server announces this on initialize). Include each tool definition in `ToolGate.allowed_tools` only if its name matches the agent's `allowed` (or doesn't match `denied`).
- Tool-call interceptor in `dispatch_with_tools` already gates on tool name — adding MCP tools to the gate list is a single line change once the catalogue is loaded.
- Cap MCP catalogue load to once per dispatch (cache in the dispatch context); avoid round-trip-per-tool-call overhead.
- Golden test: agent with gmail MCP + `denied: ["send_emails"]` dispatched via anthropic — `gmail.send` tool call refused by interceptor before execution, audit entry shows `is_error: true, reason: "blocked by agent policy"`.

### PR-4: API-provider tool-call loop — remaining 5 providers (~1 day)

- minimax, grok, deepseek, qwen, openrouter. All OpenAI-flavor, so the function-call parsing reuses PR-3's `parse_openai_tool_calls`.
- Per-provider smoke test in `ato runtimes test-providers` to bump `// last verified:` comments.
- If a provider's tool-call support is unverifiable (e.g. minimax docs don't list tool support), mark its registry entry as `flavor: "openai-no-tools"` and skip the tools field.

### PR-5: UI feedback for permission events + agentic gemini ([D] folded in) (~half day)

- Run-detail view: when a tool was denied or required approval, show it in the timeline with the agent's policy rule that fired.
- Settings → Agent detail: render the parsed permissions, not the flat `allow:X` tagged strings.
- Gemini agentic-flag pass-through: `to_gemini` returns `--yolo true` when allowed, error string when permissions are too fine-grained. The error surfaces in the existing CLI-not-found-style message (`commands/mod.rs:941-953` pattern).
- CLI→API auto-fallback (slice D): when `which_cli("gemini").is_none()` AND `find_provider("google").is_some()` AND `to_gemini(p).yolo == true`, route through `api_dispatch::dispatch_with_tools()` for the `google` provider, applying `to_api_tool_gate(p)`. Same agent permissions enforced on both paths.

### PR-8: Mandatory pre-tag dogfood smoke pass (~1 day)

Both 2026-05-19 war-room reviewers picked this as a v2.7.8 deliverable; folded back in to honor the surface-promise side of "we land what we promise."

- New script `scripts/pretag-dogfood-smoke.sh` driving a Tauri-webdriver session through the 7-step golden path:
  1. Cold launch ATO desktop.
  2. Open FirstChatWizard from Home → assert it mounts.
  3. Open FirstChatWizard from PromptBar → assert it mounts.
  4. Create a session without an initial turn → assert no ghost row appears in Sessions feed (lazy creation invariant from v2.7.7).
  5. Create a war-room from the modal → assert dispatch fires, all seats persist to `execution_logs`, return to Sessions tab cleanly.
  6. Toggle a runtime's readiness with both wizard + PromptBar open → assert no z-index stacking issue (the latent backdrop bug v2.7.7 fixed).
  7. Final assertions: zero 0-msg ghost rows, no hidden modals offscreen, no dead-affordance chevrons.
- Wired into `.githooks/pre-push` to run **only for `v*.*.*` tag commits** (don't slow down regular pushes). Skipped automatically on non-tag pushes.
- Failure = block tag → pre-push exits non-zero, tag commit doesn't reach origin. Forces a real dogfood pass before any release tag ships.
- Surface front: 70 → 85.

### PR-6: Migration of pre-v2.7.8 agents (~half day, blocks PR-2 going live)

War-room finding (claude): without explicit migration handling, PR-2 silently changes dispatch behaviour for every existing agent the moment the read-permissions path goes live. The lossy tagged-string format (Finding 1, §1.3) has already been written by v2.7.7 agents and the `summary` field has been discarded.

- Add a one-shot migration in `apps/desktop/src-tauri/src/schema.rs` that scans the `agents` table at startup and for each row with non-NULL `permissions`:
  - Parses the tagged-string format via `parse_permissions_column`.
  - Stamps a new column `permissions_migrated_at` (TEXT, NULL until migrated) with the migration timestamp.
  - Logs any row that fails to parse — those are agents the dispatch path should keep treating as "no permissions" (i.e. fall through to backward-compat defaults from PR-1 test #1).
- Wizard side-effect: on next edit of any migrated agent, surface a one-time toast: "We've updated how permissions are stored. Review the policy below — the agent's summary is now regenerated from the rules." Lets the user re-confirm the LLM-generated `summary` rather than silently losing it.
- Migration golden test: load a snapshot of v2.7.7's `~/.ato/local.db` (fixture committed under `apps/desktop/src-tauri/tests/fixtures/v2.7.7-agents.db`), run the migration, assert every agent's dispatch flags **match the pre-migration hardcoded bundles** (i.e. zero behaviour change for existing users on day 1; new enforcement only kicks in after the user edits the agent or v2.7.9 lands).

This PR runs in parallel with PR-1 (no code dependency) but must land before PR-2 ships to users.

## 6. Open questions / known unknowns

1. **MCP-tool permissions.** ~~Today the wizard's `allowed: ["send_emails"]` doesn't bind to a specific MCP server.~~ **Resolved in PR-3a (folded into PR-3 scope).** `to_api_tool_gate(perms, mcps)` reads `agent.mcps`, loads each MCP's declared tool catalogue, and filters by allow/deny. This is the credibility-load-bearing piece — without it the wizard's named-action permissions are decorative for MCP-attached agents.
2. **CLI subscription path for codex semantic denials.** Codex sandbox is binary; "deny `transfer_funds`" can't be enforced via codex's CLI flags. We have to choose: (a) refuse to create the agent ("codex cannot enforce semantic denials, switch runtime or upgrade to API path"), or (b) accept the permission and surface a "this policy is advisory, not enforced" badge in the UI. Tentative: (b) for now, log every dispatch that runs a codex agent with non-codex-native denials so we can quantify exposure. Revisit when codex CLI gains finer permissions.
3. **Anthropic CLI vs `anthropic` API provider.** Today `claude` (subscription CLI) and `anthropic` (API key) are two different runtimes. Permissions translate differently per runtime. Slice D's auto-fallback would let one agent definition route to either based on availability — but only if the permission DSL maps to both. Confirmed it does (claude → `--allowedTools`; anthropic → tool-call gate). No work needed beyond what's already in PR-2 + PR-5.
4. **Performance.** Tool-call loop adds N HTTP round-trips per dispatch. For an API provider war-room seat that needs to read 3 files + grep, that's 4 round-trips vs today's 1. Acceptable for war-room depth; may need a thinking-mode-style budget cap for everyday chat dispatches. Tracked, not blocking.

## 6a. Deferred to v2.8.x (defense-in-depth, not credibility-blocking)

- **`master_key_v2` versioned ledger.** Today's keychain rotation cliff (memory `feedback_dev_build_keychain.md`) is **dev-mode-triggered**: adhoc-signed dev binaries with unstable Designated Requirement vs. the production Apple-Developer-signed desktop. Pure consumers who use only the signed `.dmg` and never `cargo build` have a stable ACL identity. Theoretical risks (cert rollover within Team ID, major macOS upgrades, user keychain reset) haven't been observed in production. Workarounds shipped (env var bypass `ATO_MASTER_KEY_B64`, `scripts/grant-dev-keychain-access.sh`, `scripts/audit-stale-ato-binaries.sh`) cover the dev-mode case. Structural fix `master_key_v2` is queued for v2.8.x as defense-in-depth, not on the v2.7.8 critical path.

## 7. Done criteria for v2.7.8 + v2.8.0-merged

- [ ] Translation layer module + golden tests pass `cargo test`.
- [ ] An agent with `denied: ["Bash(rm:*)"]` dispatched via codex shows `sandbox_permissions=["!shell:rm*"]` on the spawned command (intercept-test).
- [ ] An anthropic dispatch from a war-room successfully reads a file at a cited line range and quotes it back, demonstrating real tool use.
- [ ] All 7 API providers populate `execution_logs.tool_calls_count` (>0) and `execution_logs.tool_calls_summary` (non-empty JSON) on dispatches that used the tool-call body (proves the loop ran).
- [ ] A denied tool call surfaces in the run-detail UI with the agent's rule that fired.
- [ ] Gemini agent with `allowed: [Read]` and `denied: [Write]` dispatched without the gemini CLI installed routes through google API provider and refuses a Write tool call.
- [ ] Pre-push hook stays green throughout (no regressions on the 18-cmd CLI smoke).
- [ ] Migration test: a v2.7.7 fixture DB loaded under v2.7.8 dispatches every existing agent with the **same** flag bundle it had pre-upgrade (zero behaviour change on day 1; new enforcement only after user edits the agent).
- [ ] MCP-tool gating test (PR-3a): agent with gmail MCP + `denied: ["send_emails"]` refuses the `gmail.send` tool call via the interceptor; audit row shows `is_error: true, reason: "blocked by agent policy"`.
- [ ] Pre-tag dogfood smoke (PR-8): `git tag v2.7.8` cannot reach origin unless the 7-step golden path runs green.

---

## What actually shipped (status as of 2026-05-20)

- **PR-1** ✓ — `packages/ato-agent-permissions/` crate. 10 golden tests pass.
- **PR-2** ✓ — Desktop + CLI dispatch read agent.permissions and translate per runtime. 2 new tests in `apps/cli/src/commands/dispatch.rs`.
- **PR-6** ✓ — `permissions_migrated_at` opt-in column. Pre-v2.7.8 agents keep pre-PR-2 dispatch behaviour; new agents created on v2.7.8+ get the column stamped at create_agent.
- **PR-3** ✓ — Anthropic flavor support added to existing `dispatch_with_tools` loop. Existing tool registry made caller-supplied; agent permissions filter the offered tool set. 4 new tests for anthropic parser/builder.
- **PR-3a** ⏸ — MCP-tool gating. Crate signature already accepts `mcp_tools: &[ToolDef]`; loading the catalogue from `agent.mcps` deferred to a focused follow-up.
- **PR-4** ✓ (folded into PR-3) — `provider_supports_tools` now covers all 7 API providers (openai / gemini / minimax / anthropic). Per-provider live verification is part of the dogfood pass.
- **PR-5a** ✓ — CLI→API auto-fallback. `apps/cli/src/commands/dispatch.rs::api_fallback_for_missing_cli` maps `claude→anthropic`, `gemini→google` when CLI is missing + key configured. 4 new tests.
- **PR-5b** ✓ (folded into PR-2) — Gemini `--yolo` is now passed when `to_gemini()` returns `yolo: true`.
- **PR-5 UI** ⏸ — Run-detail view surfacing denied/advisory events. Deferred.
- **PR-8** ⏸ — Mandatory pre-tag dogfood smoke pass (Tauri-webdriver). Deferred.

**Test count delta:** +21 (10 in new crate + 2 PR-2 + 1 PR-6 + 4 PR-3 + 4 PR-5a). All 62 CLI tests + 10 crate tests pass; both consumers (`apps/cli`, `apps/desktop/src-tauri`) compile clean.

**Behaviour invariants preserved:**
- Existing agents created pre-v2.7.8 have `permissions_migrated_at = NULL` → dispatch path falls back to PR-2 defaults that exactly match pre-PR-2 hardcoded flag bundles (PR-1 golden test #1).
- Existing `ato review --with-tools` callers see no change — they still pass the full review-tools registry.
- API providers without configured permissions still text-only-dispatch — no behaviour drift for non-agent dispatches.

## War-room provenance

- **Slice-picking dispatch** (2026-05-20 12:20 UTC, war_room_id `9F6DE70F-85A5-4983-98A6-4D27E326394A`): codex picked slice E with cited file:line evidence. Will then expanded scope to fold in the v2.8.0 API-provider tool-call loop because both fronts share the permission-DSL seam.
- **Audit review dispatch** (2026-05-20 12:30 UTC, war_room_id `E207BE6C-C0FD-4B9D-BF29-1AE194C79A0F`): codex (verifier) + claude (design reviewer) in parallel against the v0 of this doc. Findings rolled into this v1:
  - Codex: CLI already exposes `--agent` (was claimed missing); schema columns are `tool_calls_count`/`tool_calls_summary` (not `tool_calls`); `sandbox_permissions=["!shell:rm*"]` was invented — codex sandbox is binary 3-mode.
  - Claude: split the mega-table into per-runtime sub-tables; new crate `packages/ato-agent-permissions/` instead of squeezing into `ato-api-providers`; MVP tool set is read-only (`read_file`, `grep`, `list_directory`); pre-v2.7.8 agent migration is a v2.7.8 blocker (added as PR-6).
- **Code-review dispatch** (2026-05-20 15:49 UTC, war_room_id `4CC62158-F350-4B6D-8B0C-40CC34C22A79`): codex + claude reviewed PR-3 + PR-5a in flight. Two ship-blockers found and fixed:
  - Codex: `RequireApproval` was being executed as `Allow` (no approval UI yet); empty post-filter registry forced `tool_choice=auto` with empty tools field.
  - Claude: `Conversation::use_bearer_auth` was dead code; `flavor: &'static str` is fragile (defer enum promotion to v2.8.x); byok ↔ PR-5a share an implicit mapping table (defer unification to v2.8.x).
- **Verification dispatch** (2026-05-20 15:54 UTC, war_room_id captured at `/tmp/verify_war_room_id`): codex re-checked the 3 fixes against the working tree. All confirmed; no new blockers.
