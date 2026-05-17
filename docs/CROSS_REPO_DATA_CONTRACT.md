# Cross-Repo Data Contract — OSS ↔ ato-cloud

> **What this is:** the column-by-column mapping between ATO's local SQLite (`Agentic-Tool-Optimization`, `~/.ato/local.db`) and the cloud-side Postgres (`ato-cloud`, Railway). Without this doc, a schema change on one side can silently break the bridge — exactly the failure mode that bit us with `gemini-2.5-flash` missing from the pricing table for weeks.
>
> **Mirrored to** `Agentic-Tool-Optimization/docs/CROSS_REPO_DATA_CONTRACT.md` (same content; keep in sync).

---

## Why this doc exists

ATO is local-first. Most user data stays on the user's machine. Some surfaces — Pro-tier trace retention, hosted evaluators, mesh relay, provider-usage polling — require cloud-side storage. Those features create **bridges**: data flows from OSS to cloud (and occasionally back) via well-defined endpoints.

When the OSS schema changes a column name, or the cloud schema renames a column, **the bridge silently breaks**. The trace upload fails with a 400, or worse, succeeds but stores garbage. This doc makes the contract explicit so schema changes on either side know what they're touching on the other.

---

## Architecture at a glance

```
                          ┌─────────────────────────────────────────────┐
                          │  ato-cloud (Postgres on Railway)            │
                          │  https://api.agentictool.ai                 │
                          │                                             │
                          │  users   agent_traces   agent_config_…      │
                          │   ▲          ▲              ▲               │
                          │   │ JWT      │ POST         │ POST          │
                          └───┼──────────┼──────────────┼───────────────┘
                              │          │              │
              ┌───────────────┴──────────┴──────────────┴───────────┐
              │  HTTPS — embed_key OR JWT, never plaintext keys     │
              └────────────┬────────────────────────────────────────┘
                           │
              ┌────────────┴───────────────────────────────────────┐
              │ OSS desktop / CLI (SQLite at ~/.ato/local.db)      │
              │                                                    │
              │ execution_logs   agents   agent_config_changes     │
              │  + cloud_trace_id     +  slug, runtime              │
              └────────────────────────────────────────────────────┘
```

The OSS app authenticates via JWT (after sign-in) OR via `embed_key` (for external-trace-upload deploy bundles). Every payload sent up carries the user's auth; the cloud's `agent_traces.user_id` is derived from the JWT/embed_key, not from the payload.

---

## Bridge 1 — Execution traces

**Direction:** OSS → cloud (one-way upload). The OSS keeps the canonical local copy; the cloud is for cross-device retention + Pro evaluator runs.

**Trigger:** every successful `ato dispatch` (and `ato replay` job) optionally uploads via `apps/desktop/src/lib/agentTraceUpload.ts`. Free tier: last 100 runs cached locally only. Pro tier: 30 days retention in cloud. Team: 90 days. Enterprise: unlimited.

**Endpoint:** `POST /api/agent-traces/embed` (embed-key auth) OR `POST /api/agent-traces` (JWT auth).

### Column mapping

| OSS `execution_logs` column | Cloud `agent_traces` column | Notes |
|---|---|---|
| `id` (TEXT, UUID) | `cloud_trace_id` returned in response; written BACK into `execution_logs.cloud_trace_id` | The cloud has its own UUID PK `agent_traces.id`; OSS records the cloud's id locally so the link is stable. |
| `runtime` | `runtime` | Direct copy. Values: claude / codex / gemini / openclaw / hermes / minimax / grok / deepseek / qwen / openrouter / anthropic / google. |
| `agent_slug` | `agent_slug` | Direct copy. NULL for generalist. Both schemas agree on the column name (commit `8170ed5` shipped this column on OSS side). |
| `model` | `model` | Direct copy. |
| `prompt` (truncated 64KB) | `prompt_summary` (further truncated by cloud) | **Schema-name divergence.** OSS calls it `prompt`; cloud calls it `prompt_summary` (because cloud retention reduces footprint further). Upload code in `agentTraceUpload.ts` renames the field. **If either side renames, both must update simultaneously.** |
| `response` (truncated 64KB) | `response_summary` (further truncated) | Same name-divergence pattern as prompt. |
| `tokens_in` | `tokens_input` | **Schema-name divergence.** OSS: `tokens_in`. Cloud: `tokens_input`. Upload code renames. |
| `tokens_out` | `tokens_output` | Same pattern. |
| `duration_ms` | `duration_ms` | Direct copy. |
| `cost_usd_estimated` | `cost_usd` | **Schema-name divergence.** OSS: `cost_usd_estimated`. Cloud: `cost_usd`. The cloud drops "estimated" because the cloud trusts the value once stored. |
| `status` ('success'/'error') | `ok` (boolean) | **Schema-shape divergence.** OSS uses a TEXT enum; cloud uses a boolean. Upload code maps `status='success' → ok=true`, `status='error' → ok=false`, `status='killed' → ok=false`. |
| `error_message` (truncated) | `error_message` (truncated further) | Direct copy. |
| `session_id` | (not propagated today) | Local-only concept. Cloud doesn't model session boundaries yet. |
| (computed at upload) | `started_at`, `finished_at` | OSS has `created_at` + `duration_ms`; cloud derives both from that. |
| (computed at upload) | `user_id` | From JWT or embed_key. Never trusted from payload. |
| `dispatch_kind` ('active'/'passive') | (not yet on cloud) | Tier 1 passive observer field. Cloud column TODO when PR-A.5 ships. |
| `auth_mode` ('subscription'/'api_key'/'env_var') | (not yet on cloud) | Cloud doesn't yet distinguish billing-mode per trace. TODO. |
| `tool_calls_count`, `tool_calls_summary` | (not yet on cloud) | Tier 2 audit fields. Cloud column TODO. |

### Failure modes the contract prevents

1. **Schema rename on one side without the other.** If OSS renames `tokens_in → input_tokens` without updating cloud's `tokens_input`, every upload silently writes `NULL` to the cloud column. Caught only when someone queries the data weeks later.
2. **New OSS column never propagated.** If OSS adds `provider_session_id` and the upload code doesn't include it, the column is local-only (intentional for some) or shadow-missing (unintentional for others). The doc forces an explicit decision.
3. **Cloud rename without OSS update.** If cloud renames `cost_usd → cost_usd_actual`, OSS uploads keep sending `cost_usd` and the cloud either rejects (good) or silently drops it (bad).

**Rule of thumb:** any rename on either side of a bridged column requires a PR that updates BOTH schemas AND the upload code AND this doc, in the same commit.

---

## Bridge 2 — Agents (cloud sync, Pro tier)

**Direction:** bi-directional. OSS is the primary editor; cloud is the sync store; other devices read-back.

**Trigger:** `ato agents sync` (Pro-tier feature). Currently the server route exists but the desktop client doesn't ship it yet — see roadmap.

**Endpoint:** `GET /api/agents` (list), `POST /api/agents` (upsert), `DELETE /api/agents/:slug`.

### Column mapping

| OSS `agents` column | Cloud `agents` column | Notes |
|---|---|---|
| `id` (UUID) | (cloud has its own UUID) | Both sides generate their own; the bridge keys on `slug`. |
| `slug` | `slug` | Direct. |
| `runtime` | `runtime` | Direct. |
| `display_name` | `display_name` | Direct. |
| `description` | `description` | Direct. |
| `system_prompt` | `system_prompt` | Direct. |
| `model` | `model` | Direct. |
| `permissions`, `skills`, `mcps` (TEXT JSON) | `permissions`, `skills`, `mcps` (JSONB) | Same content, different storage representation. |
| `project_id` | `project_id` | Direct. |
| `kind` ('internal'/'external') | `kind` | Direct. |
| (n/a — single-user local) | `user_id` (FK to `users.id`) | Cloud only — multi-tenant scoping. |
| (n/a) | `team_id` (FK to `teams.id`) | Cloud only — Team tier shared agents. |
| `created_at`, `last_used_at` | `created_at`, `updated_at` | **Name divergence:** OSS tracks `last_used_at` (touched on dispatch); cloud tracks `updated_at` (touched on sync). |

---

## Bridge 3 — Agent config changes (regression detection)

**Direction:** OSS → cloud (one-way).

**Trigger:** every `ato agents update` (which is rare today; the GUI Create Agent wizard writes locally, doesn't push).

**Endpoint:** `POST /api/agent-config-changes`.

Schemas almost match. Both have: `agent_slug`, `field_name` (which field changed), `old_value`, `new_value`, `changed_at`. Cloud adds `user_id`.

---

## Bridge 4 — Provider usage (v2.6 observatory)

**Direction:** cloud → OSS (read-back). Cloud's `usage-poller` cron fetches daily aggregates from provider dashboards; the desktop's Usage tab pulls them back.

**Endpoint:** `GET /api/analytics/provider-usage?days=30`.

This bridge is cloud-side-only on the storage end (OSS doesn't store these aggregates locally — they're cloud-managed). The OSS UI just renders the response.

---

## Bridge 5 — Mesh relay (cloud-tier, paid)

**Direction:** OSS daemons ↔ cloud relay ↔ OSS daemons.

**Trigger:** `ato mesh relay <peer>` for NAT-traversing dispatches.

**Bridge:** the daemons authenticate via `mesh_tokens.token_hash` and the cloud routes messages without storing them long-term. No persistent data flow; cloud is a relay, not a store.

---

## Schema-divergence detector (TODO)

This doc is human-maintained today. A future improvement: a CI check that diffs the OSS schema dump against the cloud schema dump (extracted from migration files), and fails if a bridged column has a name mismatch.

Sketch:

```bash
# 1. Dump OSS schema for bridged tables
ato schema export --tables execution_logs,agents,agent_config_changes > /tmp/oss.schema

# 2. Extract cloud schema for bridged tables from migrations
node ato-cloud/scripts/extract-bridged-schema.js > /tmp/cloud.schema

# 3. Diff against the contract spec
node check-bridge-contract.js docs/CROSS_REPO_DATA_CONTRACT.md /tmp/oss.schema /tmp/cloud.schema
# Exit non-zero on any unmapped or renamed column on a bridged table.
```

Not built yet. Tracked in `ROADMAP-INTERNAL.md` as a Maintenance backlog item.

---

## Maintenance rules

1. **Bridged-column renames touch both repos in the same logical change.** If you must rename a column on a bridged table on EITHER side, the PR includes:
   - The schema change (migration on cloud OR new column in `lib.rs` on OSS)
   - The upload-code update (`apps/desktop/src/lib/agentTraceUpload.ts` or the relevant Tauri command)
   - This doc's mapping table updated
   - A regression-row added to `RELEASE_TESTING_PROCEDURE.md` confirming the upload round-trips
2. **Non-bridged columns are free.** Adding a column that's local-only (or cloud-only) doesn't require this doc change. Adding a column that needs to cross the bridge does.
3. **When in doubt, ship the column local-only first.** It's cheaper to add cloud propagation later than to find out you propagated something the cloud can't accept.

---

## See also

- `Agentic-Tool-Optimization/docs/SCHEMA.md` — OSS schema (local SQLite)
- `ato-cloud/docs/SCHEMA.md` — cloud schema (Railway Postgres)
- `apps/desktop/src/lib/agentTraceUpload.ts` — the upload code that implements bridge 1
- `services/skills/src/routes/agentTraces.ts` — the cloud endpoint that receives bridge 1
- `RELEASE_TESTING_PROCEDURE.md` § 6 — regression suite (includes bridge round-trip when applicable)
