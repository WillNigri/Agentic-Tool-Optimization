# Path B Stage 2 war-room — chat_threads → sessions schema migration

**Date:** 2026-05-19
**Session id:** a0f3f460-1040-41a5-9a61-55d949338ad4
**War-room id:** 4997000D-186D-4029-96A0-674DDCE0F25D
**Seats:** claude, codex (independent — separate runtimes, separate prompts of the same brief)

## TL;DR

Both seats independently returned **[REWORK]**. Strong consensus to defer Path B Stage 2.

The Path A read-side UNION (`sessions_view.rs` merging chat_threads into the Sessions feed) is sufficient for now. The 6-step proposed migration shape is doing too much in one commit and overstates the "one inbox, one mental model" it delivers.

## Consensus on the decision

**Don't migrate for hygiene.** Migrate when a concrete forcing feature appears. The candidate both seats named:

> When coordinator-driven summarization + tags becomes a product requirement for chat threads (or any Slice-C-shaped lifecycle feature on chats), the cost of re-implementing it on `chat_threads` justifies the migration. Until then, the UNION is the cheaper answer.

## Consensus on the highest-risk step (B)

**Step 4 (backfill).** Three specific failure modes both seats flagged:

1. **`turn_index` synthesis is unstable under millisecond ties.** `chat_messages` is ordered by RFC3339 timestamp strings; same-ms writes have undefined SQLite sort order. If you materialize `(session_id, turn_index)` from a bad sort, the transcript reads scrambled **permanently** — no natural reordering key recovers it. Use `ROW_NUMBER() OVER (PARTITION BY thread_id ORDER BY created_at, id)` — the secondary `id` sort breaks ties deterministically.

2. **`chat_messages.id` is consumer-visible.** `deleteChatMessage` is exported on the TS surface and any agent-trace records on `ato-cloud` reference these ids. The surrogate `id` on `session_turns` must be **the same UUID**, not freshly minted, or external references break silently.

3. **`role='attachment'`** rows will pass DB-level constraints and then break downstream consumers (coordinator summarization, history-replay for stateless providers). `session_turns.role` is free-form TEXT but every consumer assumes `role ∈ {user, assistant, system}`.

## Safer staged rollout (both seats independently proposed similar shapes)

Not a single PR. Five PRs, each independently revertable:

- **Stage A — additive schema only.** ALTER `sessions` to add `kind`, `archived`, `last_message_at`, `message_count`. ALTER `session_turns` to add nullable surrogate `id` (UUID) + `metadata` (TEXT). No backfill, no PromptBar changes.
- **Stage B — dual-write behind a flag.** Teach PromptBar to *new-write* to both stores under a settings flag. Reads still go to chat_threads. Historical data untouched.
- **Stage C — idempotent + reversible backfill.** Add `legacy_thread_id` on sessions so synthesized rows are identifiable + deletable for rollback. Backfill in chunks. **Preserve `chat_messages.id` as `session_turns.id`.** Verify `COUNT(*)` matches per thread before any read flip. Pre-compute checksums of ordered `(role, content, runtime, metadata)`.
- **Stage D — flip reads.** PromptBar reads from sessions. Old tables stay for one release.
- **Stage E — drop.** Next release.

## Consensus on what this does NOT solve (C)

The "unification" framing is overstated. Things that survive Path B Stage 2 unchanged:

1. **Lifecycle divergence stays.** Chats are "rolling, archive after 30d." Sessions are "explicit close → summary → reopen." A `kind` discriminator means every filter and status badge still asks "is this a chat or a session?" — just in TypeScript instead of SQL.

2. **`execution_logs` is still a parallel conversation source.** `row_kind=single_run` and `row_kind=war_room` rows in the Sessions feed are synthesized from `execution_logs`. The 4-way feed merge in `sessions_view.rs:637-642` becomes 3-way — a 25% reduction in UNION complexity, not 50%.

3. **Message-level addressability is half-solved.** Adding a surrogate `id` makes deletion API-possible, but `(session_id, turn_index)` is assumed dense + monotonic across the codebase. **You have to pre-decide:** "gaps allowed after delete" OR "renumber on delete." Renumber breaks every external reference past the deleted turn. Pick now, document in `docs/SCHEMA.md`.

4. **The `metadata` JSON column will accrete schemas.** Once one JSON column exists on `session_turns`, the path of least resistance for every future column is to add a key to it. Pre-declare the schema or accept the debt.

5. **Search corpus stays split.** `search_chat_threads` and `search_session_turns` have different UX (palette vs filter) and different intent (all-tokens-must-match vs title-match-preferred). Schema migration doesn't merge them; a separate "unified search" PR has to.

6. **Continuation semantics differ structurally.** Sessions use native runtime resume via `runtime_session_id`. Quick chat uses stitched history with no runtime-side session. Same table doesn't unify that behavior — needs an explicit `continuation_mode` column.

7. **Attachments need product support, not just storage.** `role="attachment"` is a chat-specific content type, not just extra JSON.

## Codex verdict

> **[REWORK]** — The proposal is trying to cram chat semantics into the sessions schema by accretion, and that usually means you end up with one physical table but two logical systems anyway. If you unify later, do it as a generalized conversation/message model with a staged migration, not by stretching `sessions` and `session_turns` until they secretly become `chat_threads` and `chat_messages`.

## Claude verdict

> **[REWORK]** — The direction is right. The proposed shape is doing too much in one commit and overstates the unification it delivers. Don't migrate for hygiene — migrate when you commit to a Slice-C-shaped feature on chat threads.

## Decision

**DEFER Path B Stage 2.** Keep Path A's read-side UNION.

Pre-conditions to revisit:
- A product commitment to coordinator-driven summarization/tags/auto-title on chat threads (or another Slice-C-shaped feature).
- OR: an explicit decision to add `continuation_mode`, `lifecycle_mode`, `default_agent_id` as first-class conversation columns (which would justify the broader generalized-conversation table both seats sketched).

When we do migrate, follow the 5-stage rollout above, NOT the original 6-step monolithic proposal.

## Out-of-scope notes from the war-room

Both seats independently noted that **`execution_logs` is the bigger schema audit target** if "one inbox, one schema" is the goal. The Sessions feed is currently a 4-way UNION (sessions, chat_threads, single_runs from execution_logs, war_rooms from execution_logs). Path B Stage 2 makes it 3-way. The real "unified conversations" model would migrate single_runs + war_rooms out of execution_logs too — a much bigger commit because execution_logs is the audit-trail source-of-truth for every cost/latency rollup.

Recommendation: hold the execution_logs audit as its own separate war-room before either path moves.
