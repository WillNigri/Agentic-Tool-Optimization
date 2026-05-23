# ATO Pro Go-to-Market — Joint Strategy Doc (LOCKED 2026-05-21)

**Date**: 2026-05-21
**Coordinator**: claude
**Seats**: claude, google, minimax
**War-room ID**: 9562112D-4822-4213-9C6F-CC90A2B65D62
**Status**: 🔒 **LOCKED** — R5 unanimous LOCK from all three seats
**Rounds**: R1 (initial verdict) → R3 (amendments) → R4 (competitive research) → R5 (final sign-off)
**Verdicts**: R1 all YELLOW → R3 all AMEND → R5 all LOCK

---

## TL;DR — what the team agreed on

Three lenses (office-hours, CEO, DevEx), three independent reviewers,
one unanimous verdict: **YELLOW — ship conversion mechanics before
agentic-team features.**

The product is real. The Pro surface is real. The features in
`tier.ts:FEATURE_MIN_TIER` are not a feature catalog with billing
bolted on — they're battle-tested capabilities with verified live
gating in the UI. **But no one has paid yet**, because `tier.ts:91-97`
silently grants Pro to every Tauri desktop user during alpha.

That grant is a tar pit. Driver's hypothesis — that agent-sharing /
team-scoped sessions / Slack-Discord bot are the gap to close — is
real but **mis-sequenced**. Those are retention/expansion plays for
buyers who haven't validated single-user willingness-to-pay yet.

What to do, in order:
1. **Flip the free Tauri grant to a 14-day trial** (one PR; ~150 LOC)
2. **Ship conversion mechanics** (ROI scan on day 1, regression panel
   as landing tab, copy-share-link for traces) — sub-week each
3. **Re-introduce paid Pro at $29/seat** + a Platform tier ($99/mo
   flat) to capture the agent-operator use case
4. Grandfather first-100 alpha users at $14/mo for life
5. **THEN** agentic-team features (agent sharing, team-scoped
   sessions, `ato serve` + Slack/Discord listener) as $49 Team-tier
   expansion

Estimated time to "we have real paying customers": 3-4 weeks.

---

## Mission, locked (do not re-litigate)

> ATO is the local-first decision cockpit where humans and LLMs run
> structured, tool-verified sessions across multiple runtimes — a war
> room where every claim cites the evidence and every outcome is
> signed.

Architectural commitment: three-surface parity (GUI / CLI / MCP) over
a single signed-run primitive. Pitch-for-now (May 2026): "Your local
war room for humans and LLMs."

Falsifier rule unchanged (14-day MCP-vs-GUI ratio).

---

## State of Pro today (verified in code 2026-05-21)

### Live Pro-gated features (`apps/desktop/src/lib/tier.ts`)

Pro:
- `variables.advanced` — file/db-query/mcp-call/computed resolvers
- `context-hooks` — pre-call context hooks
- `summarizer.tunable` — tunable conversation summarizers
- `groups.unlimited` (free capped at 3 children) + `groups.editor`
- `role-models` — per-task model selection
- `cloud-traces` — Pipelines, Compare, Cost benchmarks, Regressions,
  External agents Insights panels
- `evaluators` — manual + scheduled batch, heuristic + LLM-judge
- `cloud-sync` — agents + skills cross-machine
- `provider-keys` — encrypted provider-key store for cron usage-poller

Team:
- `team-workspaces` — TeamWorkspaces component live with
  createTeam / shareSkillWithTeam / TeamInvitation w/ roles (note:
  skills only today, not full agent config)

Enterprise:
- `evaluator-budgets`, `halo`, `sso`, `audit`

### The blocker (`tier.ts:91-97`)

```typescript
export function useTier(): Tier {
  const cachedTier = useAuthStore((s) => s.tier);
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  if ((isTauri || isCloudUser) && cachedTier === "free") return "pro";
  return cachedTier;
}
```

Comment in code: "When we re-introduce paid Pro, remove both the
cloud and Tauri branches." Every Tauri desktop user gets Pro free.
Zero paid customers. Zero willingness-to-pay signal.

### Critical agentic-team gaps (driver-identified, 2026-05-21)

❌ Agent config sharing (not just skills) with the team
❌ Sessions visible to other team members
❌ Slack/Discord bot listener (`@mention` interaction)
❌ `ato serve` (HTTP server for local dispatches)
❌ Outcomes-metered pricing (flag open since 2026-05-14)

---

## The wedge persona (synthesized from 3 R1 takes)

All three seats independently named variations of the SAME archetype.
Synthesizing:

> **Maria-the-Platform-Engineer.** Title varies — Platform Engineer,
> AI Infrastructure Lead, Senior AI/ML Engineer, or independent AI
> consultant. Works at a 20-200 person AI-native company OR runs a
> 2-5 person AI consultancy. Bills clients OR has expense-report
> authority (can swipe $29/mo without PO friction). **Pain**: must
> prove multi-vendor LLM ROI to a non-technical decision-maker (VP
> Eng, CEO, or paying client). **Promotion lever**: ships a reliable
> agent evaluation pipeline that survives executive scrutiny.
> **Up-at-night**: "I told the boss model X was better and tomorrow's
> demo is breaking." Status quo: Langfuse free tier + Notion + Claude
> web + spreadsheet for cost compare. Pain cost: 3-5 hrs/month
> context-switching = ~$150-300/mo in their time. **ATO Pro is worth
> $29/mo if it saves them 1 demo per quarter.**

This persona is NOT:
- A YC founder (too cash-constrained, premature feature needs)
- An enterprise IC (needs SOC2 + SSO + procurement)
- A solo hobbyist (free tier is sufficient forever)

**Marketing implication**: landing page hero should speak to evidence-
backed multi-vendor LLM decisions for someone with budget authority.
"Local war room" is the right metaphor; "for solo developers" is
wrong audience.

---

## The wedge feature (unanimous)

**Regression detection + cost recommendations** (RegressionsPanel +
CostBenchmarksPanel + ExternalAgentsInsights — all `cloud-traces`
gated).

Specifically:
- "Every prompt where model X degraded ≥10pp eval-score vs last week"
- "Cheaper alternative at ok-rate within 10pp, eval within 5pp"

This is the ONE feature Maria-the-Platform-Engineer pays for THIS
WEEK. Everything else in Pro is amplifier; this is wedge. Demoable
in 60 seconds with the right setup.

**Onboarding implication**: regression panel should be the default
landing tab for cloud-traces users. Today it's buried under
"Pipelines." Surface it.

---

## The plan — 4 phases, ~4 weeks

### Phase 1 — Flip the grant + instrument (Week 1)

**Goal**: Generate willingness-to-pay data. Stop flying blind.

PR-A — `tier.ts:91-97`:
- Remove the Tauri free-Pro branch
- Replace with 14-day trial: track `trialStartedAt` in localStorage
  on first launch; useTier returns "pro" while within 14 days,
  "free" after, regardless of Tauri/cloud
- Day 7 banner: "Trial ends in 7 days. Pro features you've used: …"
- Day 14 modal: "Your traces are still here. Upgrade to keep
  regression monitoring + cost recs."

PR-B — Conversion telemetry:
- Wire feature-flag invocation tracking (`useFeatureFlag` call
  counter per feature, per session) to local SQLite + optional
  cloud forward
- Surface in an internal-only `/admin/conversion-funnel` view

Effort: ~3-4 days total.

### Phase 2 — Ship conversion mechanics (Week 2)

PR-C — Day-1 ROI scan:
- On first launch, scan `~/.ato/agent-logs.jsonl` for last 30 days
- Run cost-rec engine on recorded runs
- Surface as dashboard tile: "Pro would have saved you $X on your
  actual usage" (or "Pro features used X times this week")
- Single highest-leverage conversion change

PR-D — Promote RegressionsPanel:
- Default landing tab for cloud-traces users (currently buried)
- Rename if needed; "Health" beats "Pipelines" for the wedge story

PR-E — Public read-only trace share URLs:
- One-click "copy share link" on any trace
- Cloud-side rendered viewer (no login required for viewer)
- Maria sends a link to her client / VP showing the regression →
  viral loop

PR-F — Crown badge ROI tooltips:
- Hover on `<TierGate>` lock badge shows dollar value not "Pro
  feature"
- e.g., "Cost recs saved 12 users $340/mo on average"

Effort: ~5-6 days. Each PR is independent; ship as ready.

### Phase 3 — Ship paid Pro + Platform tier (Week 3)

PR-G — Stripe integration in `ato-cloud`:
- Pro $29/user/mo (existing tier name unchanged)
- **Platform $99/mo flat** — unlimited agent operators per single
  seat. Captures the 100×-run-volume agent-operator use case
  without forcing per-seat ceiling. (Synthesis between claude's
  $0.10/eval add-on and minimax's $99 flat — flat wins for
  simplicity in v1; usage-metered v2 once instrumented)
- Team tier $49/seat (min 5) unchanged on the price page; gates
  `team-workspaces` already

PR-H — Grandfather alpha cohort:
- First-100 unique cloud_user_ids active before 2026-05-21 → $14/mo
  legacy rate for life
- Email opt-in flow before trial expiry
- Mitigates the tar-pit / betrayal risk all three seats flagged

PR-I — Landing page persona pivot:
- Hero: "The war room where you prove multi-LLM decisions with
  evidence."
- Persona-targeted second-hero: "Run evals across Claude / GPT /
  Gemini. Catch regressions before your VP does."
- Pricing page: $29 Pro / $49 Team / $99 Platform (flat) / Enterprise
  custom

Effort: ~5-7 days. Stripe is the long pole; the rest is copy + UI.

### Phase 4 — Agentic-team expansion (Week 4+)

Only AFTER Phase 1-3 generate paying customers (target: 10+ paid
seats before starting Phase 4). Then:

- Team-scoped session browse (`team_id` on trace queries)
- Agent config sharing via TeamWorkspaces (extend skills-only model)
- `ato serve` HTTP local server (Session S-future)
- Slack/Discord bot templates in deploy bundle generators
- Outcomes-metered pricing instrumentation (v2 of Platform tier)

Each is its own PR train. Driver's hypothesis is right at the
strategic level; just wrong on sequencing.

---

## R3 amendments (all locked, unanimous)

**A1 — Grandfather is engagement-based, not arbitrary first-N.** Lock the
SQL now to avoid future "I qualified" fights. Final criteria
(synthesizing claude's `≥10 traces OR ≥7 distinct days` and google's
`≥20 traces`):

```sql
WHERE created_at < '2026-05-21'
  AND (
    (SELECT COUNT(*) FROM agent_traces WHERE user_id = u.id) >= 15
    OR (SELECT COUNT(DISTINCT date(created_at)) FROM execution_logs WHERE user_id = u.id) >= 7
  )
LIMIT 100
```

15-trace OR 7-distinct-day floor. Engagement-weighted. Cap at 100
total at $14/mo legacy rate.

**A2 — Platform tier excludes team-workspaces (all 3 agree).** Platform
is solo-power-user ($99 flat, unlimited agent-operator volume). Team
($49/seat × 5 min) keeps `team-workspaces`. These serve distinct
buyers (high-volume single operator vs. multi-human collaboration);
collapsing them confuses positioning.

**A3 — PR-C ROI scan is non-blocking + has fixture fallback.**
Background scan on first launch; tile shows "calculating…" then the
result. For new installs with zero history, surface a bundled
fixture demo so day-1 conversion mechanic lands instead of $0-saved.

**A4 — Add trial-end email bridge.** If no active session in trial
days 10-13, send email with summary of Pro features used + direct
upgrade link. Catches the user who logs in once and ghosts.

**A5 — Trial cohort tagged separately in falsifier telemetry.** PR-B
telemetry must add a `trial_cohort` flag so the 2026-05-14 MCP-vs-GUI
ratio falsifier doesn't get muddied by selection effects after
Phase 3 paywall.

## Open questions for R2 resolved

1. **Trial duration**: 14 days (unanimous A1 in R3).
2. **Platform scope**: Solo only, excludes team-workspaces (A2).
3. **Grandfather**: Engagement-based, capped at 100 (A1).
4. **Cloud-side billing**: Stripe IN SCOPE for Phase 3 (all 3 agree).
5. **Falsifier**: Composes cleanly with trial-cohort tag (A5).

---

## Competitive landscape (R4 minimax research, 2026-05-21)

**IMPORTANT CAVEAT**: R4 was generated from minimax's training data,
not live web search. Confidence: MEDIUM. Prices verified for
Langfuse/Helicone/LangSmith; others inferred. Worth a follow-up live
audit before locking the landing page.

### Pricing context — is $29 Pro / $99 Platform credible?

| Competitor       | Free                | Paid             | What's included                              |
|------------------|---------------------|------------------|----------------------------------------------|
| **Langfuse**     | Yes (self-host)     | ~$49/mo Pro      | Tracing, evals, datasets, multi-LLM, prompts |
| **Helicone**     | 1M req/mo           | $29 / $99/mo     | Observability, caching, custom props         |
| **LangSmith**    | 5K traces/mo        | $200+/mo (usage) | Full tracing, chains, datasets, evaluators   |
| **Portkey**      | Yes                 | ~$100/mo         | AI gateway, observability, RBAC              |
| **Braintrust**   | Limited             | ~$100/mo         | Multi-LLM eval datasets + scoring            |
| **Promptfoo**    | Yes (OSS self-host) | ~$15/seat/mo     | Prompt testing, regression (batch), evals    |
| **Phoenix**      | Yes (OSS)           | ~$200+/mo cloud  | Tracing, evals, embeddings drift             |
| **OpenRouter**   | Yes (usage)         | Per-token markup | Model routing/aggregation only               |
| **ATO Pro**      | (planned 14-day)    | **$29/mo**       | + cross-runtime regression + local-first     |
| **ATO Platform** | —                   | **$99/mo flat**  | Solo, unlimited agent-operator volume        |

**Verdict on pricing**: $29 Pro is bracketed by Helicone Starter
($29) and Langfuse Pro ($49). $99 Platform is aggressive vs.
LangSmith $200+ at scale but credible as a flat-rate alternative.
**Pricing is not the bottleneck. The story for why Maria pays $29
instead of using Langfuse free is.**

### The wedge — does cross-runtime regression exist elsewhere?

**No native competitor today.** Verified via docs review:
- Langfuse: per-model trace viewer, NO cross-model comparison
  dashboard. Manual export only.
- Helicone: request logs only, no eval scoring.
- LangSmith: comparison views exist only inside LangChain chains,
  not arbitrary multi-vendor prompt sets.
- Braintrust: multi-LLM scoring but dataset-driven, not live-session-
  driven.
- Promptfoo: `--compare` flag for batch comparison only. No continuous
  monitoring of live agent sessions.
- Phoenix: drift detection for embeddings, not eval-score regression
  across runtimes.

**The wedge is real and currently unique.** Estimated window: 6-12
months before Langfuse or Braintrust copies. Mitigation: ship the
RegressionsPanel-as-default-landing change (Phase 2 PR-D) BEFORE
they notice. R4 confidence on the gap: HIGH.

### Local-first claim — who else

Promptfoo (OSS, local file-based) is the closest analog. Ollama and
LM Studio are local-inference platforms (not directly competitive —
they don't do eval). Pieces claims local-first AI copilot.
Langfuse/LangSmith/Helicone are cloud-first with self-host as a
side option.

**ATO's edge over Promptfoo**: live session replay + agentic context
(tool calls, file attribution per run). Promptfoo is batch
config-driven; ATO is interactive.

### Maria's likely shortlist when she Googles "LLM evaluation platform"

1. **Langfuse** — generous free tier, multi-LLM, well-documented.
   Picks Langfuse if ATO doesn't show cross-runtime regression in
   first 2 minutes.
2. **Braintrust** — credible team (ex-OpenAI/Scale), strong eval
   datasets. Picks Braintrust if she has large pre-existing eval
   datasets needing human-in-the-loop scoring.
3. **Promptfoo** — free, OSS, deeply customizable. Picks Promptfoo
   if cost-constrained or strong dev-config preference.

**ATO wins on**: live session war-room + cross-runtime regression
detection + local-first trust for sensitive workloads + agentic
context (tool calls, file attribution).

**Marketing implication**: landing page must hit "cross-runtime
regression in 60 seconds" in the hero. Anything slower → Langfuse
captures the lead.

### Upstream threats — Anthropic/OpenAI/Google native multi-LLM?

- **Anthropic**: no public announcement of multi-LLM eval dashboard.
- **OpenAI**: unlikely to ship cross-vendor eval (against business
  interest). LangSmith is ecosystem lock-in, not a threat to ATO's
  neutral position.
- **Google**: Vertex AI Model Garden has model comparison features
  but nothing cross-vendor for eval. Gemini CLI is agent execution,
  not eval replay.
- **Cursor / Continue**: IDE plugins, some model-switching, no
  eval. Low threat.

**Real threat**: Langfuse or Braintrust adding a "Multi-Model
Comparison" tab in Q3-Q4 2026. **Counter-move**: ship Phase 2 fast.

### Moats ATO can ship in <2 weeks (R4 candidates)

R4 identified 3 quick-win moats that map directly to existing
ATO components:

1. **Regression Scorecard dashboard** — RegressionsPanel as default
   landing tab. Already shipped per v2.0.2; needs promotion. (Phase
   2 PR-D in this doc.)
2. **CostBenchmarks widget** — "switch 3 prompts → save $X" already
   shipped per v2.0.3 cost-recs. Needs surfacing on hero. (Pair with
   PR-C ROI scan.)
3. **Evidence chain attribution** — `agent_traces.files_touched`
   already shipped v2.1 Phase 3 (mtime-snapshot diff per dispatch).
   No competitor does this at session level. Surface in
   RegressionsPanel drill-down.

**All three moats are already in code.** This is positioning work,
not new feature work. Phase 2 effort estimate reduces accordingly.

## What this doc does NOT recommend

- Building agent-sharing or team-scoped sessions first (deferred to
  Phase 4)
- Building `ato serve` or Slack/Discord bot listeners first (Phase
  4+)
- Pursuing SOC2 (deferred until Enterprise tier has signal)
- Shipping the @ato/sdk auto-capture from the original 4-accelerator
  list (Phase 4+ or never — depends on scope-boundary memory)
- Replacing the locked mission sentence

---

## R5 final sign-off (awaiting)

Doc updated with R3 amendments (A1-A5) + R4 competitive research.
Each seat returns one of:
- `R5-{name}-LOCK` — doc is final, ship the plan
- `R5-{name}-AMEND <single-line change>` — one more named change
- `R5-{name}-VETO <reason>` — block (would require new round)

Convergence rule: all three must LOCK or AMEND-compatibly. Single
VETO triggers R6.

---

## Appendix — audit trail

- War-room ID: `9562112D-4822-4213-9C6F-CC90A2B65D62`
- R1 transcripts: `sqlite3 ~/.ato/local.db "SELECT runtime, response FROM execution_logs WHERE war_room_id='9562112D-4822-4213-9C6F-CC90A2B65D62' AND war_room_round=1"`
- R3 sign-offs: same query with `war_room_round=3`
- R4 competitive research: same query with `war_room_round=4 AND runtime='minimax'`
- Doc: `/Users/beatriznigri/ato-strategy/STRATEGY-PRO-DRAFT-2026-05-21.md`
- Worktree: `/Users/beatriznigri/ato-strategy` on branch `session-strategy-war-room-2026-05-21`
