# ATO Roadmap

## Mission

**ATO is your local war room for humans and LLMs: decide together, call tools, and verify every outcome.** Drive it from a GUI, a CLI, or your coding agent over MCP — same data, same operations, same audit trail.

See [`README.md`](./README.md) for the full pitch, [`AGENTS.md`](./AGENTS.md) for the surface a coding agent reads, and [`docs/tiers.md`](./docs/tiers.md) for the open-core tiering principle (what's Free, what's Pro, and why).

This roadmap focuses on **direction**, not sprint mechanics. Internal release planning, per-PR implementation breakdowns, and historical war-room transcripts live in our internal archive and are not part of this OSS distribution.

---

## ⚠️ Open-core boundary — read this before adding a row to the roadmap

> **Pro / Team / Enterprise features must live in the CLOSED-SOURCE repo `ato-cloud`. Never in this OSS repo (Agentic-Tool-Optimization, MIT).**
>
> Runtime tier-gates (`tier::require_feature(...)`) are NOT enough — they're trivially removed by anyone who forks the MIT repo. The IMPLEMENTATION of any Pro automation belongs in the private `ato-cloud` repo (Rust, distributed as the `ato-pro` paid binary, or TypeScript hosted services).

The boundary, restated in plain language:

- **Free (OSS, this repo)** = the **building blocks** a customer can wire together by hand. Primitives, schemas, math, viewing surfaces, config formats. *"Capability without the harness."*
- **Pro (ato-cloud, paid)** = the **codified one-button automations** on top of those building blocks. Orchestrators, safety harnesses, auto-loops, cloud-hosted services. *"Same capability, but we run it for you."*
- **Team (ato-cloud, paid)** = multi-user state on top of Pro.
- **Enterprise (ato-cloud, paid + contract)** = org-wide governance, SSO, audit, on-prem.

### The three tests to run before any new commit to OSS

1. **The fork test.** If a competitor forked this repo and removed every `tier::require_feature()` call, would they get the feature? If yes → wrong repo. Move it to ato-cloud.
2. **The by-hand test.** Can a customer reproduce this feature with a bash script around `ato dispatch` + their own jq/awk? If yes, that bash recipe IS the free path; the codified one-button version goes to Pro.
3. **The "split" test.** If a feature is part-primitive part-automation, **break it up**. The primitive stays free in OSS; the codified automation goes to ato-cloud. Document both paths in [`docs/tiers.md`](./docs/tiers.md).

When in doubt: **default to ato-cloud**. It's cheap to graduate something from Pro → Free if we change our mind; it's expensive (and sometimes impossible) to claw a feature back from MIT history.

| Tier | License | Distribution |
|---|---|---|
| Free (this repo) | MIT | GitHub |
| Pro / Team / Enterprise (`ato-cloud`) | UNLICENSED | Paid binary via `ato pro install` + hosted services at `api.agentictool.ai` |

---

## Where we are (v2.16)

The current product line is the **proactive coordinator class**: humans state a goal, ATO coordinates LLMs as a writing-and-reviewing team across worktrees, humans inspect the audit trail of who did what. This is the multi-agent system shape.

| Capability | What it does | OSS / Pro |
|---|---|---|
| **Missions** | A persisted goal with verifiable success criteria, a worker config (which runtime / model / tools), a workspace strategy (single CWD or per-agent worktree), and a merge strategy. Runs over days/weeks, not in one shot. | **OSS** |
| **Coordinator tick** | The proactive wake — reads mission state + recent events, decides next action (dispatch / escalate / mark complete / nothing). Scheduled as a launchd / cron job. One action per mission per wake. | **OSS** (local scheduler) — hosted scheduler that fires even when the laptop is asleep is **Pro** |
| **Per-agent git worktrees** | Each agent in a Mission gets its own worktree on `ato/mission/<slug>/<agent>` branched from a recorded base SHA. Cleanup runs per `cleanup_policy` on terminal state. | **OSS** |
| **Merge strategies** | Coordinator integrates accepted agent work onto a dedicated integration branch via squash-merge with full provenance, runs `success_criteria` check_commands after each accepted merge, rolls back regressors. `human_approves_each` and `coordinator_merges_all` shipped. | **OSS** |
| **Decision briefs on escalation** | Every escalated event carries a canonical brief: reason, summary, options. No URLs or status labels — exact next-step choices the human can pick. | **OSS** |
| **Narrative auto-population** | Every mission_event appends a deterministic markdown bullet to `~/.ato/missions/<slug>.md` so the play-by-play is `cat`-readable without booting the GUI. | **OSS** |
| **Local Mission-control board** | Single-machine board in the desktop app under Runs → Missions. Four-column kanban by state + detail drawer with events, narrative, escalations, mutations. | **OSS** |
| **API-provider tool surface** | All Mission workers — including `anthropic` / `openai` / `google` / `minimax` API providers — can call `edit_file`, `write_file`, `bash`, `git_*`, etc. (PR-1.5). Sandboxed to the workspace root with a bash allowlist. Any runtime can be a coding-agent worker. | **OSS** |
| **Resilience layer** | Subscription-exhaustion detection per runtime, pause-and-wake with abandon decision briefs, retry-with-backoff on 503/502/504/429 for every provider, post-retry fallback chain across the user-ordered runtime list. | **OSS** primitives — cross-machine sync + hosted scheduler webhook for resume are **Pro** |
| **Cost-accounting receipts** | TokenClasses cost engine that bills every billed class (Google `thoughtsTokenCount`, Anthropic cache-creation/cache-read tiers, OpenAI reasoning tokens), live pricing registry with `unpriced_dispatches` badge, idempotent NULL-cost healing migration. | **OSS** |
| **Cross-machine aggregation, hosted scheduler, hosted judge LLM, team-shared Missions, push notifications, analytics dashboards** | — | **Pro** (`ato-cloud`) |

---

## Where we're heading

### Short horizon — sharpen the proactive coordinator

- **Coordinator tick UX**: ergonomic install of the launchd/cron schedule, per-mission interval override, owner-pause toggle.
- **More merge strategies**: `coordinator_picks_winner` and `ranked_by_score` (the picks_winner/rank shapes ship locally with a heuristic ranker; the LLM-judge version is **Pro**).
- **Mission-control board enhancements**: filters by category, agent worktree status indicators, embedded brief renderer with one-click action buttons.
- **Cost dashboard honesty**: surface unpriced-model count prominently, show per-billed-class breakdowns (input vs cache write vs cache read vs reasoning), reconcile against billing portals on a schedule.

### Medium horizon — the developer cockpit positioning

Post-subsidy AI bills 5×. **"Loops that pick the right model per step, with receipts, and merge parallel agent work as a team"** is the wedge. The product becomes the *dashboard* in this world, not a curiosity. Coming work:

- **Cheapest-model-per-step**: the methodology runner already does this for test recipes; pull the same engine into the Mission coordinator so every step picks the cheapest model that passes the rubric.
- **Production-signal ingestion**: Langfuse / Helicone / custom log ingestion into `production_signals` table; diagnose loop reads them alongside dev rubric scores. Consumer (write) side stays OSS; the hosted ingester is **Pro**.
- **Online improvements loop**: when production signals or holdout cells regress, the variant-lineage tracker proposes a change, runs A/B against a baseline methodology, ships if it wins all three win-condition predicates. Diagnose loop is **Pro**; the underlying methodology runner + holdout matrix are OSS primitives.

### Long horizon — the agentic team primitive

- **Inputs panel**: stored markdown context bundles any agent / loop / mission can reference.
- **Live team workspace**: real-time collab on sessions and war-rooms.
- **Output bundles**: packaged inference results, signed URL, shareable externally.
- **Git linkage on every dispatch**: stamp current commit SHA so "what run produced this commit?" has an answer.

Blue-sky (research bets that don't currently fit the mission) are tracked privately.

---

## Recently shipped (headline list)

> One-liner per release. The OSS history is in `git log`; the public app's changelog is generated per tag.

| Version | Headline |
|---|---|
| **v2.16** (alpha series, in progress) | Missions: proactive coordinator class — schema + dispatch + per-agent worktrees + coordinator tick + merge strategies + decision briefs + narrative auto-population + local Mission-control board. API-provider write/edit/exec tool surface so any runtime can be a coding worker. Cost-accounting cluster (token classes + cache + reasoning receipts + NULL-cost healing). Post-retry fallback chain. |
| **v2.15.x** | Resilient dispatch: subscription-exhaustion detection per runtime, pause-and-wake with abandon decision briefs, retry-with-backoff for every provider, fallback-chain UI. Live model picker per provider. |
| **v2.14** | Loop Composer — persisted SQLite-backed graphs of LLM operations (dispatch / methodology run / diagnose / review / war-room) compose into recurring inference workflows. |
| **v2.13** | Team workspaces (cloud-side) — shared agents and methodologies for teammates (Team tier in `ato-cloud`). |
| **v2.11** | Learning loop + open-core tier gate. Methodology diagnose proposes structured changes from failing-cell evidence; A/B against a baseline; holdout cells defend against overfitting; variant-lineage tracker warns on rapid iteration. Tier-gate primitive (`tier::require_feature`). |
| **v2.10** | Methodology runner — reusable test recipes (N prompts × M models × R reps, scored with a rubric). Per-cell mean / SD / 95% CI / pairwise Welch t. Dual-cost ledger (your LLM bill vs our delivery cost). |
| **v2.7..v2.8** | Agent safety floor — agent permission model with allow/deny/requireApproval gates; close lifecycle for sessions / war-rooms / chats; Linux WebKitGTK fix; Cost Optimization Engine. |
| **v2.5..v2.6** | LAN mesh daemon scaffolding (Phase 7.0 + 7.1 cloud-relay for NAT). Universal multi-LLM passive observer that tails `~/.claude` / `~/.codex` / `~/.gemini` session JSONLs into a single ledger. |
| **v2.1..v2.3** | Multi-runtime observability with file attribution, cross-runtime conversation bridges (`@runtime` mentions), CLI dispatches visible in Live Runs, agent-driveable platform surface. |
| **v2.0** | External Agents / Hosted Deployment — companies build customer-facing chatbots, deploy to their own infra, observe behavior. |
| **v1.x** | Foundations — multi-runtime support (Claude Code, Codex, OpenClaw, Hermes, Gemini, Ollama), Skills + Skills Marketplace, MCP server with 17 tools, Automation Builder, Cron Monitor, Setup Wizard, i18n (EN/PT/ES), cloud sync, team workspaces foundation. |

---

## Contributing direction

If you want to contribute, look at:

1. **[`docs/v2.16-missions.md`](./docs/v2.16-missions.md)** — the live design slice for the proactive coordinator. Open questions list at the bottom.
2. **[`docs/v2.16-pr-1.5-tools.md`](./docs/v2.16-pr-1.5-tools.md)** — the API-provider tool surface spec.
3. **[`docs/v2.11-learning-loop.md`](./docs/v2.11-learning-loop.md)** — the methodology + diagnose + A/B design.
4. **[`docs/tiers.md`](./docs/tiers.md)** — what's OSS vs Pro and why; **read this before opening a PR**.
5. **[`CONTRIBUTING.md`](./CONTRIBUTING.md)** — local-dev setup, test conventions, review process.
6. **[`AGENTS.md`](./AGENTS.md)** — the surface a coding agent (Claude Code / Codex / Gemini CLI / Cursor) reads to drive ATO.

Open issues with the `good-first-issue` label are scoped for a single afternoon. War-room any architectural change with at least one cross-family second opinion before opening the PR — we use ATO itself for this (see `AGENTS.md` § "war-rooms").
