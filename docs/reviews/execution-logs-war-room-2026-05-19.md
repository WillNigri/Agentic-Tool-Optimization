# execution_logs schema audit war-room

**Date:** 2026-05-19
**Session id:** 5b71cb18-3489-4d46-a901-f050c0aaa4d2
**War-room id:** 89493D0D-786D-40CC-9712-DB5E11B0BBAF
**Seats:** claude, codex (independent dispatches, same brief)

## TL;DR — productive disagreement

Both seats returned **[REWORK]** with **different splits**:

- **Claude verdict: Option 1 (no split).** Ship filter discipline + `active_dispatches` view in the same PR that enables passive-observation writes. JOIN/UNION cost on Sessions-feed isn't worth it.
- **Codex verdict: Option 2, but smaller** — `dispatches` + `passive_observations`, **skip `war_room_participants`**. Mixed-meaning rows are the real problem, not column count.

**Both seats agree on the urgent must-have:** PR-A cannot ship without filter discipline on legacy read paths. They flagged the same 5 places.

## What both seats independently corrected in the prompt

The schema migration **already shipped**. `dispatch_kind TEXT NOT NULL DEFAULT 'active'`, `billing_surface`, `provider_session_id` are already on the table (plus `auth_mode`, `cost_usd_estimated`, `sequence_within_session` that weren't in the prompt). The real table is ~24 columns today, ~25 after PR-A finishes wiring it up. `compute_billing_surface_summary` already exists in `analytics.rs:53`.

So the framing "should we split before more columns land" is moot. The actual decision is: **should we split before passive_observation rows actually start landing in volume?**

## The 5 read paths that break silently if PR-A ships without filters (consensus)

Both seats listed the same set in the same order:

1. **`sessions_view.rs:332-336`** — single-run synthesis. Predicate is only `session_id IS NULL AND war_room_id IS NULL`. Passive rows match, get rendered as single_run cards with no prompt/response. **Must add `AND dispatch_kind = 'active'`.**
2. **`analytics.rs:128-133` + `get_usage_metrics`** — hourly tokens_in/out aggregation sums both kinds. Step-function on the day passive observation starts. **Must filter, OR ship a UI toggle that defaults to active-only.**
3. **`replay.rs:59`** — `SELECT FROM execution_logs WHERE cloud_trace_id = ?1 OR id = ?1`. Passive observation has no replayable spec; replay emits a malformed dispatch.
4. **`regressions.rs:286-291`** — agent_slug scoped. Probably safe in practice but the safety is load-bearing on the coincidence that passive rows from Claude Code CLI won't carry an ATO `agent_slug`.
5. **`read_agent_traces` / `get_agent_metrics` / `load_agent_log_lines`** — runtime/status/since filters but no kind filter.

Plus shared views in `packages/ato-db-views`: `v_recent_dispatches`, `v_cost_by_agent_runtime`.

`compute_billing_surface_summary` is the **one** place the kind-mix is intentional — that's the whole point of the surface group-by. Leave that unfiltered.

## Where the seats diverged

### Claude's case for Option 1 (no split)

- 25 columns isn't where SQLite hurts. NULL columns are ~1 byte each.
- Sessions feed is the **hottest read path**. Already runs 3 separate queries against `execution_logs`. Splitting adds either a 4th query or UNION ALL — paid on every Sessions feed render.
- War_room_participants buys nothing: `GROUP BY war_room_id` is already correct and uses the partial index. Moving FK to its own table replaces one-table aggregate with JOIN-then-aggregate, same cardinality.
- Option 3 (EAV) is the wrong shape for 25 columns; that's a hundreds-of-nullables pattern.
- **The breakeven where Option 1 starts hurting** isn't column count, row count, or query latency. It's "the moment a developer writes a new read path and forgets to filter by `dispatch_kind`." That's a discipline problem.
- **Fix:** `CREATE VIEW active_dispatches AS SELECT * FROM execution_logs WHERE dispatch_kind = 'active'` and point legacy reads at the view. Option-2-shaped read isolation without the JOIN tax.

### Codex's case for Option 2 (split)

- Column count isn't the issue; **mixed meaning** is. `dispatch_kind` is effectively a type discriminator. Once a discriminator becomes required for most legacy queries to stay correct, the one-table model has already started working against you.
- Passive rows have **different invariants**: no `prompt`/`response` guarantee, no `cloud_trace_id`, not replayable, not ATO-initiated. `dispatches` keeps the active-row invariants tight.
- **Best read-path win:** `get_usage_metrics`, `get_execution_logs`, `read_agent_traces`, local regressions, cost recs stop needing "remember to exclude passive" everywhere.
- **Cloud sync gets cleaner.** `cloud_trace_id` is an active-dispatch concern; keeping passive rows in the same table creates more null-heavy rows and branchy upload logic.
- **Skip `war_room_participants`.** War-room reads are simple because `war_room_id` and `war_room_round` sit on the row itself. Moving to a join table now adds joins to every war-room read without fixing the passive/active problem.

### What both agreed Option 2 wouldn't solve

- Unified "all activity" view still needs `UNION ALL` across both tables.
- Billing/cost summaries that span both kinds still need cross-table logic.
- Runtime utilization dashboards if they want "everything that happened on this machine."
- Replay stays dispatch-only (split makes that explicit, doesn't solve it).
- Cloud-trace lookup (4 commands) becomes a UNION across both tables if passive observations ever get uploadable to ato-cloud.

## CTO synthesis

Both seats agree on the **urgent locked-in deliverable** (PR-A filter discipline). They disagree on whether to also do the split.

**Decision: Hybrid path. Ship Option 1 now, defer the split decision.**

1. **Now (with PR-A):** Add the 5 read-path filters. Create `CREATE VIEW active_dispatches AS SELECT * FROM execution_logs WHERE dispatch_kind = 'active'` and point legacy reads at the view by default. Document that `compute_billing_surface_summary` is the intentional cross-kind reader.
2. **New code default:** When adding a new "list dispatches" read, the default is `active_dispatches`. Opting into passive requires an explicit `FROM execution_logs WHERE dispatch_kind = 'passive_observation'`. This buys codex's type-level safety today without paying claude's JOIN cost.
3. **Re-evaluate split after passive-observation ships and we have data on:**
   - Sessions-feed render latency.
   - Actual passive/active row ratio (claude flagged ~10× as a tripwire).
   - Whether a third `dispatch_kind` value gets added (semantic divergence pressure).
   - Whether passive_observations grow columns that don't belong on active dispatches (e.g., source-file path, jsonl byte offset for resumable tail).

The view + filter discipline gives us the option to split later with low risk because the SQL is already written against the discriminator. If codex's "every reader has to filter" prediction comes true, we promote the view to a table.

## Locked-in deliverable for PR-A

Before PR-A enables passive-observation writes, this checklist must be green:

- [ ] `CREATE VIEW active_dispatches` added to schema.
- [ ] `sessions_view.rs:332-336` single-run predicate: add `AND dispatch_kind = 'active'`.
- [ ] `analytics.rs:128-133` tokens aggregation: filter or UI toggle.
- [ ] `get_usage_metrics`: filter to active-only by default; new param to opt into all.
- [ ] `replay.rs:59`: filter source lookup.
- [ ] `regressions.rs:286-291`: explicit `dispatch_kind = 'active'` (don't rely on agent_slug being null on passive rows).
- [ ] `read_agent_traces` / `get_agent_metrics` / `load_agent_log_lines`: filter.
- [ ] `packages/ato-db-views`: `v_recent_dispatches`, `v_cost_by_agent_runtime` filtered.
- [ ] `compute_billing_surface_summary`: documented as the intentional cross-kind reader.
- [ ] Test: synthetic passive-observation row in a fixture DB → run all the above readers → verify totals unchanged.
