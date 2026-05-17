# ATO OSS Schema — `~/.ato/local.db`

> **What this is:** the canonical reference for ATO's local SQLite database. The schema is the user-facing API for every agent / human / script that wants to read or write ATO's state. This doc names every table, what it holds, and which other tables it relates to (even when the FK isn't formally declared).
>
> **What this is NOT:** the cloud-side schema. That lives in `ato-cloud/docs/SCHEMA.md` (sibling file, parallel structure). The bridge between OSS and cloud is documented in [`docs/CROSS_REPO_DATA_CONTRACT.md`](CROSS_REPO_DATA_CONTRACT.md).

---

## Quick navigation

| Domain | Tables | Purpose |
|---|---|---|
| **Auth / identity** (local-side cache) | `secrets`, `llm_api_keys`, `env_vars` | Encrypted local credentials + env-var snapshots |
| **Sessions + Dispatch** | `sessions`, `session_turns`, `execution_logs`, `live_runs` | Multi-turn conversations + per-dispatch audit trail |
| **Agents + Persona** | `agents`, `agent_groups`, `agent_group_members`, `agent_variables`, `agent_hooks`, `agent_evaluators`, `agent_knowledge_chunks`, `agent_config_changes` | Persistent agent records + their attached metadata |
| **Skills + MCPs** | `skill_toggles`, `skill_versions` | Per-runtime skill enablement + snapshot history |
| **Replay + Compare** | `replay_jobs`, `eval_ratchets` | Re-dispatch + score-ratchet rails |
| **Runtimes** | `runtime_preferences`, `runtime_quotas`, `remote_runtimes`, `health_checks` | Runtime configuration + health |
| **Ops + Automation** | `ops_recipes`, `ops_recipe_runs`, `cron_alerts`, `notification_channels` | User-authored automation + cron job alerting |
| **Mesh (LAN peers)** | `mesh_peers`, `mesh_invites`, `mesh_discovered` | Phase 7.0 LAN mesh of trusted machines |
| **Chat threads** | `chat_threads`, `chat_messages` | Daily-workspace chat history (legacy v1.5.0 surface) |
| **Projects** | `projects`, `profile_snapshots` | Project metadata + file-watcher state |
| **Telemetry / audit** | `audit_logs`, `events_log`, `activity_posts`, `watcher_state` | Operational logs |
| **Settings** | `settings`, `model_configs` | App-level configuration |
| **Views** | `v_session_audit`, `v_recent_dispatches`, `v_session_cost_summary`, `v_cost_by_agent_runtime`, `v_orphaned_session_turns`, `v_orphaned_execution_logs` | Audit-trail joins centralized (see `packages/ato-db-views`) |

Total: **40 tables**, **38 indexes**, **6 views**. Schema lives in inline `ALTER TABLE` migrations in `apps/desktop/src-tauri/src/lib.rs` (canonical) and mirror migrations in `apps/cli/src/db.rs::open_readwrite` (for CLI-only users).

---

## Domain: Sessions + Dispatch (the audit-trail core)

These five tables are the load-bearing audit trail. Every meaningful action ATO records ends up here.

### `sessions`

One row per conversation / decision / war-room / work block. The unit users see in the Sessions tab.

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PRIMARY KEY | UUID. Pass to `--session <id>`. |
| `runtime` | TEXT NOT NULL | Anchor runtime (where the session was created). Marked `★` in the UI. |
| `agent_slug` | TEXT | Optional anchor agent. Distinct from per-turn personas. |
| `title` | TEXT | User-set title at creation. |
| `auto_title` | TEXT | Coordinator-generated at close; preferred in UI. |
| `summary` | TEXT | Coordinator-generated paragraph at close. |
| `tags_json` | TEXT (JSON array) | Coordinator-generated topic tags. |
| `project_id` | TEXT | Inferred project. Free-form string today; controlled vocab in roadmap. |
| `category` | TEXT (CHECK) | Controlled vocabulary: `Business` / `Marketing` / `Dev` / `Frontend` / `Backend` / `Design` / `Security` / `Compliance` / `Ops` / `Other`. NULL allowed (back-fill safety). Populated by the coordinator at close in PR 3. Indexed via `idx_sessions_category_lastused` for filter dropdowns. |
| `team` | TEXT | Free-form owner/team label (founder / frontend / backend / etc.). NULL allowed. Indexed via `idx_sessions_team_lastused`. Multi-tenant story land later — single-user installs use this as a "band" label per Will's findability theme. |
| `status` | TEXT | `'open'` or `'closed'`. |
| `created_at`, `last_used_at`, `closed_at` | TEXT (RFC3339) | Lifecycle timestamps. |
| `turn_count` | INTEGER | Cached count of `session_turns` rows. |

**Relations** (stringly-typed, not FK-declared):
- `sessions.id` ← `session_turns.session_id` (1:N)
- `sessions.id` ← `execution_logs.session_id` (1:N)

### `session_turns`

One row per turn (user prompt OR assistant response) in a session.

| Column | Type | Notes |
|---|---|---|
| `session_id` | TEXT NOT NULL | FK to `sessions.id` (stringly-typed). |
| `turn_index` | INTEGER NOT NULL | 0-based monotonic per session. |
| `role` | TEXT NOT NULL | `'user'` or `'assistant'`. |
| `text` | TEXT NOT NULL | Full turn body. |
| `runtime` | TEXT NOT NULL | Runtime that produced this turn. Differs from `sessions.runtime` for cross-runtime sessions. |
| `agent_slug` | TEXT | Persona if dispatched with `--agent`; NULL for generalist. |
| `created_at` | TEXT NOT NULL | RFC3339. |
| `sender_peer_id` | TEXT | Mesh peer when the turn came from another machine; NULL local. |

PK: `(session_id, turn_index)`. Index on `(session_id, turn_index ASC)`.

### `execution_logs`

One row per dispatch — the canonical record of every `ato dispatch` invocation. Source of truth for cost / tokens / duration / agent_slug / auth_mode.

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PRIMARY KEY | UUID. |
| `runtime` | TEXT NOT NULL | claude / codex / gemini / openclaw / hermes / anthropic / google / minimax / grok / deepseek / qwen / openrouter / etc. |
| `agent_slug` | TEXT | Slug passed via `--agent`. NULL for generalist dispatches. Written by all three dispatch paths since commit `8170ed5`. |
| `model` | TEXT | Provider-side model id (`claude-sonnet-4-6`, `gemini-2.5-flash`, etc.). |
| `prompt` | TEXT NOT NULL | Truncated to 64KB. |
| `response` | TEXT | Truncated to 64KB. |
| `tokens_in`, `tokens_out` | INTEGER | From provider usage block when available; otherwise chars/4 estimate. |
| `duration_ms` | INTEGER | Wall-clock dispatch duration. |
| `cost_usd_estimated` | REAL | From `packages/ato-pricing::cost_from_tokens`. NULL when model isn't in the pricing table. |
| `status` | TEXT NOT NULL | `'success'` / `'error'` / `'killed'`. |
| `error_message` | TEXT | Trimmed error. |
| `session_id` | TEXT | FK to `sessions.id` (stringly-typed). NULL for one-off dispatches. |
| `cloud_trace_id` | TEXT | FK to `ato-cloud:agent_traces.id` after upload (cross-repo bridge). |
| `auth_mode` | TEXT | `'subscription'` / `'api_key'` / `'env_var'`. Authoritative per-row billing mode. |
| `dispatch_kind` | TEXT NOT NULL DEFAULT 'active' | `'active'` (normal) / `'passive'` (observer mode, Tier 1). |
| `billing_surface`, `provider_session_id` | TEXT | Tier 1 passive observer fields. |
| `sequence_within_session` | INTEGER | Insertion order within a session. |
| `tool_calls_count`, `tool_calls_summary` | INTEGER / TEXT | Tool-call audit (function-calling Tier 2). |
| `skill_name` | TEXT | Skill that was active during dispatch, if any. |
| `created_at` | TEXT NOT NULL | RFC3339. |

Index on `(agent_slug, created_at DESC)`. The dispatch path writes to this table from THREE places (`commands/dispatch.rs:478`, `:840`, `:1077`) — CLI runtime / API provider / remote runtime paths.

### `live_runs`

In-flight dispatches. Rows are inserted on dispatch start, removed on completion (via a Drop guard in dispatch.rs).

### `replay_jobs`

Re-dispatch jobs queued from the Replay surface. Independent of execution_logs.

---

## Domain: Agents + Persona

### `agents`

One row per agent record. Created via `ato agents create --from-file`, the GUI Create Agent wizard, or direct SQL.

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PRIMARY KEY | UUID. |
| `slug` | TEXT NOT NULL | Per-runtime slug (e.g., `positioning`, `ceo`). |
| `runtime` | TEXT NOT NULL | Which runtime this agent is scoped to. |
| `display_name` | TEXT NOT NULL | Human-readable. |
| `description` | TEXT | Short blurb. |
| `system_prompt` | TEXT | The persona content prepended on `--agent <slug>` dispatches. **Today this is the only field the dispatch path reads** — see `Agent ⇄ Skill linkage` roadmap entry. |
| `model` | TEXT | Default model override. |
| `permissions`, `skills`, `mcps` | TEXT (JSON) | Schema present; dispatch path doesn't read them yet. Roadmap. |
| `goal`, `file_path` | TEXT | Optional. |
| `role_models_json`, `memory_policy_json` | TEXT (JSON) | Per-task models + memory policy. Schema-present, not dispatch-read. |
| `project_id` | TEXT | Project this agent belongs to (optional). |
| `kind` | TEXT NOT NULL DEFAULT 'internal' | `'internal'` (ATO-managed) / `'external'` (deploy-bundle agent). |
| `created_at`, `last_used_at` | TEXT | Timestamps. |

UNIQUE on `(runtime, slug)`. Indexes on `runtime`, `last_used_at DESC`, `project_id`.

### `agent_groups` + `agent_group_members`

Routed groups and sequential pipelines. These DO have proper FK declarations (`agent_group_members.agent_id → agents.id`, `agent_group_members.group_id → agent_groups.id`).

### `agent_variables`, `agent_hooks`, `agent_evaluators`, `agent_knowledge_chunks`

Per-agent metadata for the dynamic-prompt + observability surfaces. Most populated by the GUI Create Agent wizard; the dispatch path doesn't all of them yet.

### `agent_config_changes`

Configuration ledger — auto-logged on every model swap, prompt edit, role-models change. Joined with `execution_logs` for the regression-detection surface.

---

## Domain: Skills + MCPs

### `skill_toggles`

Per-(runtime, skill_path) enabled/disabled state. Skills themselves live on disk (`~/.claude/skills/<name>/SKILL.md`, etc.) — ATO reads them but doesn't store content.

### `skill_versions`

Snapshot history of skill file edits. Every save creates a version row; restoring is itself reversible.

---

## Domain: Runtimes

### `runtime_preferences`

Per-runtime CLI path overrides + auth modes.

### `runtime_quotas`

Cached `"rate limited until <ts>"` errors so dispatches can short-circuit.

### `remote_runtimes`

SSH config for OpenClaw + Phase 6.x-J remote dispatches.

### `health_checks`

Per-runtime health polling results. **Has a 7-day retention** via `DELETE FROM health_checks WHERE checked_at < datetime('now', '-7 days')` in the health_poller — runs on every poll cycle, so the table stays bounded even at high poll frequencies.

---

## Domain: Ops + Automation

### `ops_recipes` + `ops_recipe_runs`

User-authored trigger→action workflows. v2.3.0 Phase 7 — see `ROADMAP-INTERNAL.md`.

### `cron_alerts`, `notification_channels`

Schedule job results + delivery channels.

---

## Domain: Mesh (LAN peers, Phase 7.0)

### `mesh_peers`

Trusted peer machines (post-pairing handshake). Long-lived rows.

### `mesh_invites`

Open invite tokens awaiting acceptance. TTL'd.

### `mesh_discovered`

mDNS discoveries — transient, auto-pruned after ~5 minutes of staleness.

---

## Domain: Chat Threads (legacy v1.5.0 surface)

### `chat_threads` + `chat_messages`

Persistent chat conversations independent of `sessions` (which is the newer v2.3.31+ surface). The two coexist; new work goes into `sessions`. Note: `chat_messages.thread_id` is a properly declared FK to `chat_threads.id`.

---

## Domain: Auth / Identity (local cache)

### `llm_api_keys`

User's stored API keys, AES-256-GCM encrypted under the macOS-keychain-backed master key. See `apps/cli/src/encryption.rs` + `docs/PERMISSIONS.md`.

| Column | Notes |
|---|---|
| `provider` | Case-insensitive — match with `LOWER(provider) = LOWER(?1)`. |
| `encrypted_key` | `v1:` prefix + base64(nonce + ciphertext + tag) for AES-GCM rows; legacy plain-base64 rows still readable. |
| `is_active` | Soft-delete flag. |
| `key_prefix` | First 8 chars of the plaintext key, kept for UI display ("sk-ant-abc…"). |
| `updated_at` | RFC3339. |

### `secrets`

User-named secrets (database connection strings, webhooks, etc.) for use in variable resolvers. Encrypted same as `llm_api_keys`.

### `env_vars`

Environment-variable snapshots for reproducible dispatches.

---

## Domain: Telemetry / Audit

### `audit_logs`

Auto-logged file writes (every `agent file write` records what changed + backup path).

### `events_log`

Internal event stream (used by `ato events watch`).

### `activity_posts`

Chronological human+agent posts in the activity feed (v2.x+).

### `watcher_state`

File-watcher state for project file watching (used by Insights → File Attribution).

---

## Domain: Settings

### `settings`

App-level key→value store (sync prefs, language, UI flags).

### `model_configs`

Per-runtime / per-project model overrides.

---

## Views (`packages/ato-db-views`, shipped commit `bd22893`)

These are SQL VIEWs auto-applied on every desktop startup AND every CLI `open_readwrite`. Use them in ad-hoc SQL queries — they centralize the joins that used to be re-implemented at every call site.

### `v_session_audit`

Per-turn joined with its matching `execution_logs` row (correlated subquery + LIMIT 1 → 1:1 cardinality, no Cartesian explosion). Use this to see "what was the cost / model / auth_mode for each turn in session X."

```sql
SELECT * FROM v_session_audit WHERE session_id = 'b1547c69-…' ORDER BY turn_index;
```

### `v_recent_dispatches`

`execution_logs` with prompt/response previews pre-truncated to 200 chars. No baked ORDER BY — caller sorts.

### `v_session_cost_summary`

Per-session aggregate. Dispatch count, success count, error count, distinct runtimes, distinct agents, tokens, duration, cost. Empty sessions still appear (LEFT JOIN).

```sql
SELECT * FROM v_session_cost_summary WHERE session_id = 'b1547c69-…';
-- → 18 dispatches, 17 success, 4 distinct runtimes, 4 distinct agents, $0.1468 total
```

### `v_cost_by_agent_runtime`

`(agent_slug, runtime, auth_mode)` rollup across ALL execution_logs. `is_generalist BOOLEAN` flags rows where `agent_slug IS NULL`.

### `v_orphaned_session_turns` + `v_orphaned_execution_logs`

Audit/debug. Surfaces rows that DON'T correlate across tables — usually means chat-thread writes or a mid-flight crash. Healthy installs have small numbers here.

---

## Foreign-key honesty note

Only **3 declared foreign keys** in the OSS schema today:
- `agent_group_members.agent_id → agents.id`
- `agent_group_members.group_id → agent_groups.id`
- `chat_messages.thread_id → chat_threads.id`

Every other cross-table reference (`session_turns.session_id`, `execution_logs.session_id`, `execution_logs.agent_slug`, etc.) is **stringly-typed** — no DB-level integrity. Deleting a session does NOT cascade to its turns or execution_logs; cleanup is the caller's job. Fine for a single-user local SQLite; will need real FKs if we ever lift this to Postgres.

The `v_orphaned_*` views exist specifically to surface this class of dangling reference.

---

## Migration path

Schema evolves via **idempotent `ALTER TABLE` + `CREATE INDEX IF NOT EXISTS` + `CREATE VIEW IF NOT EXISTS`** statements in:
- `apps/desktop/src-tauri/src/lib.rs` (the canonical run-on-startup path)
- `apps/cli/src/db.rs::open_readwrite` (defensive mirror for CLI-only users who never launch the desktop)

There are **no numbered migration files** like the cloud has. The trade-off:
- ✅ Idempotent on every open — no migration runner needed, no version table to maintain.
- ✅ Easy to add new columns without ceremony.
- ❌ Harder to read schema evolution from `git log` — the change history of any column is spread across `lib.rs` commits.
- ❌ No down-migrations. Removing a column requires shipping a fresh `ALTER TABLE … DROP COLUMN` and accepting that older rows lose data.

If schema complexity grows, a real migration runner (sqlite-migrations or similar) is the right answer. Today it's intentionally simple.

---

## See also

- [`SESSIONS.md`](SESSIONS.md) — full lifecycle + dispatch types + discipline rules for the Sessions domain
- [`PERMISSIONS.md`](PERMISSIONS.md) — what permissions the encryption layer requires + when
- [`RELEASE_TESTING_PROCEDURE.md`](RELEASE_TESTING_PROCEDURE.md) — must-pass before any schema change merges
- [`CROSS_REPO_DATA_CONTRACT.md`](CROSS_REPO_DATA_CONTRACT.md) — how this schema connects to ato-cloud's Postgres (TODO — pairs with this doc)
- `packages/ato-db-views/src/lib.rs` — source of truth for the SQL VIEW definitions
- `apps/desktop/src-tauri/src/lib.rs` — startup migration code (canonical)
