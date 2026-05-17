# ATO Continuation Plan — Maintenance Sprint + Strategic Roadmap

> **Purpose:** a single doc capturing (a) everything shipped during the maintenance sprint with test status, (b) what's queued with priority, and (c) **a copy-paste session-kickoff prompt for each remaining item** so work can resume cleanly across multiple sessions/days.
>
> **Last updated:** 2026-05-17 (PR 3 + Sessions UX polish wave 5 — Runs IA collapse — landed). Maintained in OSS repo. Mirror not required (this is the master log).

---

## Section 1 — Audit log of what shipped (with test status)

All commits listed below ran through their relevant slice of `docs/RELEASE_TESTING_PROCEDURE.md` (the procedure itself shipped mid-sprint, so items before `5544803` were retroactively ratified).

### Marketing / pitch work (compare-runtimes wedge)

| Commit | What | Procedure status |
|---|---|---|
| `7858562` | Round-5 README hero rewrite + UpgradePrompt → sign-in capture (EN/PT/ES) | §1+§4 smoke + §5C war-room (5-seat unanimous AMEND) |
| `6dd6afc` | Hero kicker line "compare any AI · keep the receipts" + version bump v2.4→v2.6 in marketing site (EN/PT/ES) | §1+§4 |
| `c23c2bd` | og:image + twitter:image meta tags + 1200×630 PNG generated from headless Chrome | §1+§4 + curl verification |
| `7550a55` | Product screenshot section "Real sessions, real receipts" in EN/PT/ES marketing site | §1+§4 + visual inspection |
| `1fa2d2c` | README Sessions section — lifecycle, dispatch types, discipline summary | §1+§4 |
| `1797d57` | `docs/SESSIONS.md` (canonical sessions reference, 270 lines) + skill link-out | §1+§4 |

### Bug fixes (high impact)

| Commit | What | Test |
|---|---|---|
| `8170ed5` | `execution_logs.agent_slug` actually persisted (was always NULL despite the column existing); also macOS keychain reads capped at 8s (was hanging indefinitely) | Live verified — row `4a5fc321` shows slug=positioning on error; row `2dd30c20` shows slug=ceo on success; keychain timeout fires in ~8s |
| `f3091db` | `ato demo-compare` CLI command — zero-config first-run wedge demo | Stub path verified; configured-runtime path verified with timeout fallback |
| `7e63075` | Persona badges + coordinator marker + cost pill on SessionsList card; chat-bubble role labels render `positioning` instead of `ASSISTANT` | Live verified against PMF session (b1547c69) |
| `f9c9539` | Cost receipts panel at bottom of chat detail; per-(runtime, persona) table; cheapest-metered callout | Live verified — $0.0666 sum for claude row |
| `10ff6e7` | Pricing table expanded to 2025/2026 models (gemini-2.5-flash bug fixed); new billing_mode enum; UI distinguishes subscription vs api_key with caveat | Live verified — $0.1468 PMF total split correctly across api_key/subscription |
| `0ccdc12` | Keychain in-memory cache via OnceLock + `$ATO_MASTER_KEY_B64` dev-mode bypass + `docs/PERMISSIONS.md` ladder | Verified — dialog stops re-prompting within a process |
| `3d58f98` | Insights dashboard reads from `execution_logs` (SQLite) instead of stale `agent-logs.jsonl`; "unknown" → italic "generalist" for NULL slugs | Live verified — 1,484 runs, 98% success, real slugs on today's gstack work |

### Agent / war-room flow

| Commit | What | Test |
|---|---|---|
| `637109f` | 5 gstack agent records (positioning, devex, ceo, designer, office-hours) + `.claude/skills/ato-warroom/SKILL.md` section 4a (three seat types: generalist / agent / skill / hook) + section 4b (parallel vs sequential) | Live A/B test on minimax: specialist vs generalist on same prompt |
| `908e1c6` | Skill section 4c: session discipline (one subject per session, never re-open off-topic, smoke tests get throwaway session) | Applied retroactively to PMF session — moved smoke turns 32-33 to a separate "agent-flow smoke test" session |
| `259c99e` | `commands/COMMANDS_SPLIT_PLAN.md` — 29-PR plan for splitting commands.rs (war-room reviewed) | §5C war-room: codex `[REFINE]` with 5 issues all applied; pr-reviewer `[APPROVE-PR-1]` |

### Sessions UX polish wave (2026-05-17)

| Commit | What | Test |
|---|---|---|
| `20c63f9` | PR 1 — explicit "Coord" + "+" badge groups replace the ★-prefix cluster on SessionsList card | §5B visual verification |
| `dee47a7` | PR 2 — schema: `sessions.category` with CHECK on controlled vocab + `sessions.team` free-form + two indexes | §3 + §4 + manual ALTER dogfood |
| `348fd9e` | PR 4 — category badge + team display rendered on SessionsList card | §5B visual verification (TS types + Tauri SELECT already wired) |
| `fff889e` | **PR 3 — closure-time coordinator enforcement of `category` + `team`.** Coordinator prompt asks for both; parse-time validator hard-fails on out-of-vocab category; soft-warns on NULL; `--force-close-without-context` suppresses warning. `validate_category` owns trim + empty-coalesce. UPDATE uses COALESCE on category+team so a weaker re-close can't erase taxonomy. Drift-killer unit test parses `apps/desktop/src-tauri/src/lib.rs` CHECK at compile time and asserts set-equality with `ALLOWED_CATEGORIES`. `ato review` auto-close passes `force_close_without_context=true` to silence dual stderr. | §3 ✓ §4 ✓ §5B mechanical smoke + SQL exercise ✓; §5B live LLM round-trip (after Will rotated minimax key with fresh value + I pinned `ATO_MASTER_KEY_B64`): minimax M2.7-highspeed closed session `ae268adf` and multi-runtime session `fc3c71b8` (Claude+Minimax+Google+Codex, 8 turns) with `category=Dev / team=backend`. §5C codex-reviewer + pr-reviewer both `[REFINE]` with 7 issues total, all applied. |
| `203654f` | **PR 5a — UNION ephemeral dispatches into the Sessions feed.** `list_sessions_full` reads standalone `execution_logs` (session_id IS NULL) and emits them as ephemeral rows alongside real sessions. `SessionListRow.rowKind: "session" \| "ephemeral"` discriminator (codex Round-1 #2: bool too weak for routing/caching). Frontend TS picks up the field; `status` literal broadened to string. | §3 ✓ §4 ✓ §5C codex Round 1 [REFINE: UNION not implicit-session migration, typed discriminator, unified row contract, explicit click contract before tab removal] — all 4 addressed before the diff landed. |
| `3926af0` | **PR 5b — kind filter chips + ephemeral card variant.** "All / Sessions / Single runs" chip group above the existing Open/Closed chips with per-kind counts. Ephemeral card variant: lighter render, one runtime badge + optional persona + "single run" marker, prompt prefix headline, response preview, cost + timestamp. Ephemeral cards visible but not yet clickable (pending 5c). | §4 ✓ Visual verification deferred to 5c bundle. |
| `b01edc7` | **PR 5c — drop History tab + ephemeral detail view + IA collapse.** New `get_ephemeral_detail(log_id)` Tauri command with `AND session_id IS NULL` contract enforcement. New `EphemeralDetailView.tsx` component. `openSelection: {kind,id}` routing replaces `openId`. New `_helpers.ts` module breaks the previous SessionsList ↔ EphemeralDetailView circular import. Lifecycle chips relabeled "Lifecycle (sessions only)" with per-chip tooltips. RunsSection drops the History tab + LogViewer import; `components/LogViewer/` directory deleted entirely. | §3 ✓ §4 ✓ §5C codex Round 1 [REFINE: 5 items] + pr-reviewer Round 2 [REFINE: confirmed 5 + 2 more] — all 7 applied (ephemeral-only WHERE clause, setOpenId alias removed, component extracted, dead code deleted, lifecycle chips clarified, helpers.ts cycle break, stale comment fixed). |

### Strategy / docs

| Commit | What |
|---|---|
| `5544803` | `docs/RELEASE_TESTING_PROCEDURE.md` — the must-pass-before-merge contract |
| `19df4ed` / `8f23ad6` | `docs/SCHEMA.md` for both repos + `docs/CROSS_REPO_DATA_CONTRACT.md` (mirrored) |
| `4d37c70` | Roadmap: Sessions UX polish (closure metadata + coordinator visual + project/team taxonomy) |
| (in flight) | Roadmap: ATO Auto-Optimization (Pro-tier automated LLM-fit testing) — captured below |

### Shared crates extracted (drift killers)

| Commit | Crate | Purpose |
|---|---|---|
| `6d47133` | `packages/ato-pricing` | Single source of truth for model pricing + billing_mode classification. 4 unit tests including `pricing_parity_contract`. War-room reviewed (codex REFINE + pr-reviewer APPROVE). |
| `bd22893` | `packages/ato-db-views` | 6 SQL views applied idempotently on every desktop startup + CLI open_readwrite. 5 unit tests including 3 convention guards. War-room reviewed (codex REFINE + pr-reviewer APPROVE). |

### Release

| Tag | Date | Commit | Notes |
|---|---|---|---|
| `v2.7.3` | 2026-05-17 | `b01a6a4` | Sessions UX polish wave 5. PR 3 (closure-time category+team) + PR 5a/5b/5c (Runs IA collapse, WhatsApp-feed Sessions inbox) + PR 6+7 (taxonomy filters + count reconciliation footer). Two release-day fixes: RUNTIME_COLORS re-export (`44fb571`), keychain timeout help text → ATO_MASTER_KEY_B64 (`305e222`). War-room: 7 seats × 4 LLM families, 3 APPROVE / 4 REFINE / 0 DISSENT; Round 2 ceo synthesis → Option B (ship after the one help-text fix). Tag pushed; awaiting CI artifacts + Homebrew tap update. |

| Older tag | Status | Notes |
|---|---|---|
| `v2.7.0` | Built + released | First tag with `ato demo-compare`, agent_slug persistence, persona UI, keychain timeout |
| `v2.7.1` | Built + released, **Homebrew tap update pending** | Adds cost receipts panel + pricing fixes |

---

## Section 2 — What's queued (priority order with rationale)

| # | Item | Effort | Why this priority |
|---|---|---|---|
| 1 | **v2.7.1 Homebrew tap update** | 30 min | Mechanical, unblocks fresh `brew install` users from getting the new binary |
| 2 | **PR 1 of commands.rs split (`shared.rs`)** | 2-3 hours | Foundation for the next 28 PRs; pr-reviewer already approved the plan |
| 3 | **Sessions UX polish** (your screenshot feedback) | 3-4 days | High user-visible payoff; converts the war-room infrastructure into actually-navigable artifacts |
| 4 | **Auto-Optimization Pro feature** (your strategic ask) | 1-2 weeks (v1) | The Pro-tier value prop. Build only when wedge ratifies OR ≥3 founder calls request it. |
| 5 | **PR 2-29 of commands.rs split** (one domain each) | 30-50 hours across days | Maintenance debt; pays back every future feature PR |
| 6 | **Knowledge Source Adapters** (roadmap) | 1.5-2 weeks | Second-surface feature; ships in wave with Agent⇄Skill linkage |
| 7 | **Agent ⇄ Skill linkage** (roadmap, paired with #6) | bundled with #6 | Same wave; same pitch |

---

## Section 3 — Per-item session-kickoff prompts

> **How to use:** paste the prompt below as the FIRST message of a fresh Claude Code session. Each prompt is self-contained — references commits + docs in the repo so the new session can pick up cold.

### Item 1 — v2.7.1 Homebrew tap update

**When:** any time, mechanical work.

**Prompt:**

```
v2.7.1 release artifacts should be built by now. Update the Homebrew tap
at https://github.com/WillNigri/homebrew-ato so `brew install willnigri/ato/ato`
pulls v2.7.1 instead of v2.7.0. Steps:

1. Fetch the SHA256 of each artifact from the v2.7.1 GitHub release
   (https://github.com/WillNigri/Agentic-Tool-Optimization/releases/tag/v2.7.1).
2. Update the Cask formula in homebrew-ato/Casks/ato.rb with the new
   version + sha256s for each platform.
3. Commit + push to the tap.
4. Verify by running `brew install --cask willnigri/ato/ato` on a fresh
   shell session and confirming `ato --version` reports 2.7.1.
5. Smoke test: `ato demo-compare --human` runs without crashing,
   `ato sessions list --limit 3` returns valid JSON.

This is a doc-only / config change per the release testing procedure
(docs/RELEASE_TESTING_PROCEDURE.md section 8) — section 1 + section 4
smoke is sufficient. No full dogfood needed.
```

---

### Item 2 — PR 1 of commands.rs split (`shared.rs` foundation)

**When:** after item 1, in its own session.

**Prompt:**

```
Execute PR 1 of the commands.rs split per
apps/desktop/src-tauri/src/commands/COMMANDS_SPLIT_PLAN.md. PR 1 is the
shared.rs foundation — extract cross-cutting types and helpers that
multiple domains will need, BEFORE any commands move. War-room already
approved this approach (session 6cf41892, codex REFINE applied, pr-
reviewer APPROVE-PR-1).

Scope of PR 1:
1. Identify types used across multiple domains: AgentMessage,
   DispatchResult, ReplayJob, hook loaders, active-runs bookkeeping,
   log attribution helpers, prompt-history summarization.
2. Move them to apps/desktop/src-tauri/src/commands/shared.rs.
3. Each type gets a brief comment naming its eventual home domain
   (per pr-reviewer's request — helps reviewers of PRs 2-28 verify
   provenance).
4. Update imports in apps/desktop/src-tauri/src/commands.rs (the old
   monolith file stays; just shrinks) AND in lib.rs / any other
   referencing file.

Full release testing procedure compliance required (docs/RELEASE_
TESTING_PROCEDURE.md):
- §3 Shared-crate tests pass
- §4 Build matrix: cargo check + cargo build --release for both
  apps/cli and apps/desktop/src-tauri; tsc --noEmit on frontend
- §5A Dogfood session created: dogfood/PR-1-shared-rs-extraction-DATE
- §5B Mechanical smoke: every CLI command in section 2A of the
  procedure still works (ato sessions, ato dispatch, ato review, ato
  demo-compare, ato agents, ato skills, ato runtimes)
- §5C War-room: dispatch codex --agent codex-reviewer with the diff,
  then claude --agent pr-reviewer with codex's amendments visible.
  [APPROVE] required.
- §6 Regression suite green
- §7 Will signs off

Commit message names every type moved + which domain it's destined
for. Push when green.
```

---

### Item 3 — Sessions UX polish (your screenshot feedback)

**When:** after item 2, multi-session work.

**Prompt:**

```
Implement the Sessions UX polish per the roadmap entry in
ato-cloud/ROADMAP-INTERNAL.md (under "Second-surface candidates",
section "Sessions UX polish — closure metadata + coordinator visual
+ project/team taxonomy").

Three gaps Will flagged on 2026-05-17:
1. Closure tags are inconsistent — only present on closed sessions
2. Coordinator vs participants visually weak — the ★ marker exists
   but is easy to miss; needs an explicit "Coordinator: X · Participants: …"
   label
3. No category / team / project taxonomy on the card; sessions can't
   be filtered by Business / Marketing / Dev / Backend / Design / etc.

Build plan (from the roadmap entry):

A. SCHEMA — add to sessions table:
   - category TEXT (controlled vocab: Business / Marketing / Dev /
     Frontend / Backend / Design / Security / Compliance / Ops / Other)
   - team TEXT (free-form)
   - project_id already exists; just surface it in UI + require at close

B. CLOSURE CONTRACT — ato sessions close prompts the coordinator to
   pick category + project + team. Warning (not hard fail) if missing;
   --force-close-without-context flag for explicit override. Updates
   the ato-warroom skill section 4c with this rule.

C. UI (SessionsList card) — explicit "Coordinator: ★ Claude · Participants:
   Minimax, Google" row replacing the badge-cluster + ★ prefix shape.
   Category badge (cyan accent) next to title. Project + team line below
   title. Tags row de-emphasized at the bottom.

D. SEARCH + FILTER — Category filter dropdown in the toolbar (next to
   All/Open/Closed). Project filter dropdown. Click-tag-to-filter.

E. BACKFILL the existing PMF session (b1547c69) and other 2026-05 work:
   category=Marketing, project=ato-2026-05, team=founder.

Full release procedure required for each meaningful PR. Probably
breaks into 3-4 PRs:
- PR A: schema additions + closure contract + skill update
- PR B: SessionsList card render changes
- PR C: Search/filter toolbar
- PR D: Backfill historical data

Each PR runs §3 §4 §5A-C §6 §7. Use the war-room for design review
of the card layout BEFORE writing the JSX.
```

---

### Item 4 — Auto-Optimization (the Pro pitch)

**When:** trigger fires (compare-runtimes wedge ratifies OR 3+ founder calls request it). Likely 2-3 weeks out, not next session.

**Prompt:**

```
Build v1 of ATO Auto-Optimization per the roadmap entry in
ato-cloud/ROADMAP-INTERNAL.md (under "Second-surface candidates",
section "ATO Auto-Optimization — automated LLM-fit testing as the Pro
pitch").

The pitch: continuously test the user's stack of agents/recipes/
workflows across alternative runtime/model combinations, score each
output, rank by quality × cost, propose concrete config changes the
user can apply with one click. Pro-tier feature.

Build on existing infrastructure:
- ato dispatch <runtime> --agent <slug> — already runs any agent
  against any runtime
- ato review --consensus — already does multi-LLM scoring
- execution_logs.cost_usd_estimated — cost data per dispatch
- agent_evaluators table — heuristic + LLM-as-judge already implemented
- agents.role_models_json — per-task model split already columned

What to add:
1. Recommendation engine (Rust): reads test results, ranks alternatives
   by composite (quality_score × cost_inverse), surfaces a per-seat
   proposal.
2. Approval workflow (UI): Settings → Optimization tab. Shows
   "Proposed changes" list with quality/cost deltas; one-click apply
   updates agent records.
3. Cron scheduler: weekly re-tests OR after a new model ships
   (poll provider model list).
4. Cloud-side: agent_traces.optimization_run_id column to tag runs
   that are optimization tests vs user dispatches. Per-user
   optimization quota.

v1 scope (~5-7 days):
- Manual "Run optimization now" button (no scheduler yet)
- Test 3 alternatives per seat (codex / claude / gemini for code
  agents; google / minimax / anthropic for API agents)
- Score via existing agent_evaluators
- Show recommendation list
- One-click apply

Defer to v2: scheduled re-tests, automatic new-model detection,
team-tier shared optimization, change history + revert.

Full release procedure compliance throughout. War-room review on the
recommendation algorithm (the LLM-as-judge scoring + the cost-vs-
quality weighting) BEFORE coding it.

Pricing exploration — what's the right price/test ratio? Run a quick
war-room (positioning + ceo + office-hours seats) on:
- $19/mo unlimited
- $0.05/test pay-as-you-go
- First 100 tests/mo free, then $0.05
- Free for first 30 days, then a per-user MRR target
```

---

### Item 5 — PRs 2-29 of commands.rs split

**When:** chip away across many sessions. No urgency.

**Prompt template (replace `<DOMAIN>` per PR):**

```
Execute PR N of the commands.rs split per
apps/desktop/src-tauri/src/commands/COMMANDS_SPLIT_PLAN.md. This PR
moves the <DOMAIN> domain (~M commands) from commands.rs to
commands/<DOMAIN>.rs.

Procedure:
1. List the M commands in this domain (the plan doc has them).
2. Create apps/desktop/src-tauri/src/commands/<DOMAIN>.rs.
3. Move each command function + its supporting structs + private
   helpers from commands.rs to <DOMAIN>.rs. Imports update; functions
   stay `pub fn` to keep lib.rs::invoke_handler! visibility.
4. Add `pub mod <DOMAIN>;` + `pub use <DOMAIN>::*;` in commands/mod.rs
   so the existing call sites resolve unchanged.
5. Verify the moved commands are still in lib.rs::invoke_handler!
   (no rename, just import path change).
6. Cargo check + cargo build --release passes.
7. Frontend tsc --noEmit clean.
8. §5B dogfood: exercise every UI surface in section 2 of the
   procedure that touches this domain. (For agents.rs, that's every
   surface; for models.rs, just Settings → Models.)
9. §5C war-room review on the diff. [APPROVE] required.
10. §6 regression suite green.
11. Commit + push.

If you discover a function used by multiple domains, move it to
shared.rs (extracted in PR 1) instead of duplicating.
```

---

### Item 6+7 — Knowledge Source Adapters + Agent ⇄ Skill linkage (bundled wave)

**When:** trigger fires (per roadmap entry).

**Prompt:**

```
Both Knowledge Source Adapters and Agent ⇄ Skill linkage live in
ato-cloud/ROADMAP-INTERNAL.md under "Second-surface candidates". The
sibling-sequencing rule there says: ship them together so the
marketing story is one continuous pitch — "your agents and your
project context follow you across LLMs."

Sequencing:
1. PR A: packages/ato-knowledge-adapters (new shared crate) — the
   KnowledgeAdapter trait + the markdown_dir + gstack_brain +
   plain_url adapters
2. PR B: schema additions to local SQLite — knowledge_sources +
   knowledge_pointers tables. Apply via lib.rs + db.rs idempotent
   migrations.
3. PR C: CLI surface — ato knowledge add/list/search
4. PR D: MCP tool — mcp__ato__find_knowledge_pointers
5. PR E: Desktop UI — new "Knowledge" tab under Skills & MCPs
6. PR F: AgentDetail Skills tab + Skills section in OverviewTab
7. PR G: Dispatch path reads agents.skills and inlines (capped 30KB)
8. PR H: Stack export — `ato stack export --out team-stack.yaml`
   bundles knowledge_sources + agents + recipes

Each PR runs full release procedure. War-room review on the adapter
trait shape BEFORE writing the markdown_dir adapter.

Marketing co-deliverable: a /memory landing page on agentictool.ai
for the second wedge.
```

---

## Section 4 — How the release testing procedure applies across these items

The procedure (`docs/RELEASE_TESTING_PROCEDURE.md`) is now load-bearing. Every item above runs sections 3-7 unless explicitly noted as doc-only.

The critical guards that catch the bug-class each section exists for:

- **§3 Shared-crate contract** — Rust changes to `ato-pricing` / `ato-db-views` / `ato-api-providers` MUST run their crate tests. Caught the gemini-2.5-flash drift retroactively; will prevent the next one.
- **§4 Build matrix** — cargo check + tsc must be green before review. Catches Rust type errors + TypeScript prop-type drift.
- **§5B Mechanical smoke** — exercises EVERY CLI verb + EVERY desktop tab + EVERY MCP tool listed in section 2 of the procedure. Caught the v_session_audit `no such column` bug that compile-green missed.
- **§5C War-room** — codex-reviewer Round 1 + pr-reviewer Round 2 on the diff. Caught 5 substantive issues in the ato-db-views extraction; caught 5 more in the commands.rs split plan.
- **§6 Regression suite** — 9 specific tests covering known prior bugs (keychain hang, agent_slug persistence, pricing parity, dashboard not-unknown, sessions discipline, dialog frequency, migration idempotency, og:image, views queryable). Add a row for every new bug fixed.
- **§7 Human sign-off** — Will reviews diff end-to-end, verifies dogfood session, spot-checks screenshots, confirms no DISSENT outstanding.

---

## Section 5 — Sequence + dependencies

```
Item 1 (Tap update)      [stands alone; 30 min]
  ↓
Item 2 (shared.rs PR 1)  [foundation for items 5]
  ↓
Item 3 (Sessions UX)     [user-visible, ~4 days, 3-4 PRs]
  ↓
Decision point:
  ├─ Trigger Auto-Optimization (item 4) — IF wedge ratifies or 3+ user calls request
  └─ OR continue commands.rs split (item 5 PRs 2-29) — IF wedge hasn't ratified yet
  ↓
Items 6+7 (Knowledge + Agent⇄Skill bundled wave) — when their trigger fires
```

**Loom recording** — Will is handling tomorrow. Doesn't gate anything.

**Distribution-first 14-day window** — STILL ACTIVE. Items 4, 6, 7 are gated on it. Items 1, 2, 3, 5 are maintenance (procedure says maintenance is fine during the window).

---

## Section 6 — Open questions for next session

1. **v2.7.1 release artifacts** — workflow finished at ~22:40 UTC 2026-05-16; need to verify all platforms green before the tap update. Quick `gh run view <id>` check.
2. **Auto-Optimization pricing** — $19/mo? $0.05/test? Free-trial-then-X? Worth a positioning war-room before building.
3. **Sessions UX category vocabulary** — Will's list (Business / Marketing / Dev / Frontend / Backend / Design) is good. Should we add Security / Compliance / Ops / Research? A 4-seat war-room can settle this.
4. **commands.rs split scheduling** — 29 PRs across days. Is one-per-day reasonable, or batch on weekends?

---

## Master checklist (track here as items ship)

- [x] Maintenance sprint item 1 — health_checks retention (existed already)
- [x] Maintenance sprint item 2 — `ato-pricing` extraction (`6d47133`)
- [x] Maintenance sprint item 3 — `ato-db-views` extraction (`bd22893`)
- [x] Maintenance sprint item 4 — SCHEMA.md docs + cross-repo contract (`19df4ed` / `8f23ad6`)
- [x] Maintenance sprint item 5 PLAN — `COMMANDS_SPLIT_PLAN.md` (`259c99e`)
- [x] Release testing procedure (`5544803`)
- [ ] v2.7.1 Homebrew tap update — next session
- [x] commands.rs PR 1 (shared.rs) — `2e0069c` + `73d7583`
- [x] Sessions UX polish PR 1 — coordinator/participants badge split (`20c63f9`)
- [x] Sessions UX polish PR 2 — schema for `category` + `team` (`dee47a7`)
- [x] Sessions UX polish PR 3 — closure-time coordinator enforcement (`fff889e`)
- [x] Sessions UX polish PR 4 — SessionsList card surfaces category + team (`348fd9e`)
- [x] Sessions UX polish PR 5a — UNION ephemeral dispatches into Sessions feed (`203654f`)
- [x] Sessions UX polish PR 5b — kind filter chips + ephemeral card variant (`3926af0`)
- [x] Sessions UX polish PR 5c — drop History tab + ephemeral detail view (`b01edc7`)
- [x] Sessions UX polish PR 6 — taxonomy filters + click-tag-to-filter (`cff59b4`)
- [x] Sessions UX polish PR 7 — lifecycle chip count footer (`cff59b4` bundled)
- [x] v2.7.3 release — tag pushed `b01a6a4`; CI building; Homebrew tap update pending artifacts
- [ ] **PR 8 — rename "ephemeral" → "single-shot" everywhere** (Tauri command name, struct, TS interface, chip label, footer). Positioning Round 1 verdict: "ephemeral" contradicts "keep the receipts" pitch. Touches Rust struct + TS + every comment in the wave.
- [ ] **PR 9 — designer polish wave**: ⚡ leading glyph on single-run cards (`var(--accent)` at 0.7 opacity, 12px); collapse category+team into a `Filters ▾` disclosure to save vertical space; tag chip pressed-state with bg+font-weight+letter-spacing not border-only (color-blind a11y). All flagged by designer seat as PR-8 sub-10 polish items.
- [ ] **PR 10 — drop "Taxonomy" toolbar label + add comparison-verb copy** that telegraphs intent ("Filter receipts by…" or empty-state copy bridging to "Compare these runs"). Positioning Round 1.
- [ ] **PR 11 — Slice C PR-2 (resurrected): `project_id` snapshot at session create** + `--project` flag on `ato sessions new`. Will surfaced 2026-05-17: sessions are born project-less because the active-project sidebar isn't snapshotted at create. Column exists, write path doesn't. Originally queued in v2.6 Slice C PR-2 plan; bypassed in favor of category/team work; now overdue.
- [ ] **PR 12 — codex lifecycle-counts global-vs-contextual semantics decision**: Open/Closed chips count globally while category/team count contextually in the same toolbar. Pick one model and apply everywhere. Codex Round-1 on v2.7.3 release review.
- [ ] **PR 13 — keychain ACL-as-NoEntry root-cause fix**: `master_key_inner()` in `apps/cli/src/encryption.rs` + `apps/desktop/src-tauri/src/encryption.rs` should inspect the underlying `keyring` crate error variant and refuse to regenerate on `errSecAuthFailed` / permission-denied subcases — only true `NoEntry` (no row at all) triggers generation. Today's `ATO_MASTER_KEY_B64` workaround is documented in `memory/feedback_dev_build_keychain.md`.
- [ ] **PR 14 — war-room cohesion**: today's Round-1-parallel methodology fires N standalone dispatches per round, showing as N separate single-run cards instead of one logical war-room. Two designs on the table: (a) add `war_room_id` UUID to `execution_logs`, group cards sharing the same id, (b) shift methodology so Round-1-parallel goes into a single session with parallel turns. Both candidate PR shapes captured in chat 2026-05-17.
- [ ] **60s Loom post-ship recording** — office-hours falsifier: if the Loom doesn't clear 500 X views in 48h, the IA collapse was not user-pulled and the team pivots to the Create Agent wizard next, not more sessions polish.
- [ ] commands.rs PR 2-29
- [ ] Auto-Optimization Pro feature
- [ ] Knowledge Source Adapters + Agent ⇄ Skill linkage wave
- [x] Loom recording — Will handling

---

## See also

- `docs/RELEASE_TESTING_PROCEDURE.md` — the must-pass contract
- `docs/SCHEMA.md` — OSS schema reference
- `docs/SESSIONS.md` — sessions lifecycle + discipline
- `docs/PERMISSIONS.md` — permission ladder + dev-mode bypass
- `docs/CROSS_REPO_DATA_CONTRACT.md` — OSS ↔ cloud bridge mapping
- `ato-cloud/ROADMAP-INTERNAL.md` — full strategic roadmap including Knowledge Adapters, Agent⇄Skill linkage, Sessions UX polish, ATO Auto-Optimization
- `apps/desktop/src-tauri/src/commands/COMMANDS_SPLIT_PLAN.md` — 29-PR sequence for the commands.rs elephant
- `.claude/skills/ato-warroom/SKILL.md` — war-room methodology used in all §5C reviews
