# ATO tiers — what's free, what's paid, and why

This is the source of truth for ATO's tiering. README.md, agentictool.ai, and the desktop app's `<TierGate>` components all read from the principle + table below.

## The open-core principle

**Customers can run primitives free. We charge for the codified automation we package on top.**

Same model as GitLab, Sentry, Supabase, Mattermost: you can do everything yourself with our open-source primitives — write your own bash loop, set up your own launchd plist, hand-prompt your own diagnose LLM. We don't lock that path. What we charge for is the **one-click button that codifies our methodology**: the workflow we encoded, the automation we run, the safety net we wrap around it.

Will's articulation (2026-05-25): *"if you want to do everything by yourself you can but you need to set up everything, create your flows using our app. If you want faster access, cloud, automations, access to our systems to facilitate, that's where we charge."*

The doctrine *isn't* "BYOK vs. cloud" — that was an older framing. The doctrine is **DIY-with-primitives vs. ATO-automates-it-for-you**, regardless of where the LLM call lands.

## What this means for the methodology runner (v2.10)

The methodology runner is the headline Pro feature *positioning-wise*, but most of its surface is FREE primitives that customers can compose. The automation buttons on top are Pro:

| Surface | Tier | Why |
|---|---|---|
| `ato evaluations methodology create / list / get` | Free | You're defining + viewing your own methodology config. No automation. |
| `ato evaluations methodology run` | **Pro** | The codified fan-out orchestrator. Customer can replicate by hand with a 100-line bash loop around `ato dispatch` + their own JSON parser — that's the free path. |
| `ato evaluations methodology adopt` | **Pro** | The codified "ingest your existing dispatches into a structured run" automation. Customer can replicate by writing the same INSERT statements by hand. |
| `ato evaluations methodology score` | **Pro** | The codified rubric-application loop. Customer can compose the same rubric against any dispatch by hand using the (free) rubric library schema. |
| `ato evaluations methodology runs list / show` | Free | Read access to your own data. |
| `ato evaluations methodology cost-estimate` | Free | Math against the published rate card. |
| `ato evaluations methodology margin` | **Pro** | The codified cost ledger view. Customer can read the same columns from SQLite by hand. |
| `ato evaluations methodology calibrate show / set / reset` | Free | You're providing the calibration data; we just persist it. |
| `ato evaluations methodology archetypes` | Free | Reading the built-in archetype registry. |
| `ato evaluations methodology schedule create` | **Pro** | Codified scheduling. Customer can write their own launchd plist or crontab line by hand. |
| `ato evaluations methodology schedule list / delete / trigger` | Free | Managing what you (or your past Pro self) already set up. |
| `ato evaluations methodology diagnose` (v2.11) | **Pro** | Codified learning-loop automation. You could write your own diagnose-prompt bash loop. |
| `ato evaluations methodology diagnose --cross-runtime` (v2.12) | **Pro** | Codified cross-model tournament: N models propose, picker policy picks the winner, audit log persists losing proposals. Free DIY: run `methodology diagnose --diagnose-model X` twice with two model names and compare the JSON output by hand. |
| `ato dispatch ... --depth-cap N --budget X` (v2.12 PR-14) | **Pro** | Bounded recursive dispatch with depth cap + budget envelope (shared across siblings) + cycle detection (agent_slug or hash(runtime, prompt)). The capability of an agent calling `ato dispatch` from inside its own run is already free; the codified safety harness is what you pay for. Free DIY: customer increments `ATO_DISPATCH_DEPTH` env var before each call and bails on overflow in a 5-line bash check. |
| Cloud sync of methodology runs across devices | **Pro** | Our infra runs it. |
| Hosted scheduled diagnose with email alerts | **Pro** | Our infra runs it. |
| Auto-revert watch from Langfuse traces (v2.11.5) | **Pro** | Our automation watches your prod + reverts on regression. |
| Auto-PR after A/B wins | **Pro** | Our automation opens the PR. |
| Methodology runs shared across team workspace | **Team** | Multi-user state. |

## What this means for the rest of the product

| Capability | Tier | Notes |
|---|---|---|
| ATO desktop app | Free | Local-first, MIT, runs forever. |
| Multi-runtime dispatch (`ato dispatch`) | Free | Claude / Codex / Gemini / OpenClaw / Hermes / 15+ API providers. BYOK. |
| Agent creation wizard + Quick form | Free | |
| Skills + MCPs (local) | Free | |
| Sessions, war-rooms, replay, file attribution | Free | The core primitives. |
| `ato review` (multi-LLM code review) | Free | Ad-hoc — fire when you want. |
| Workspaces (local) | Free | Auto-seeded "Personal" workspace; create more locally. |
| Variables (advanced resolvers — MCP/DB/file/computed) | Free | War-room 87E6CADF round 3 locked this 2026-05-22. |
| Context hooks (local pre-call) | Free | |
| Tunable summarizer | Free | |
| Multi-agent groups (unlimited) | Free | 3-child cap killed 2026-05-22. |
| Group editor | Free | |
| Role-models (per-task model selection) | Free | |
| Ad-hoc evaluators (single-shot LLM-as-judge) | Free | |
| MCP server (17 tools + methodology) | Free | Drives ATO from any MCP client. |
| Embedded terminal (xterm) | Free | |
| Schedules (UI to view existing schedules) | Free | The CREATE button is Pro from v2.11. |
| Cron monitor in desktop app | Free | Reading what's scheduled. |
| **Scheduled evaluators** | Pro | Cloud cron worker. |
| **Cloud trace upload + retention** | Pro | 30-day cross-device retention; regression detection across devices. |
| **Cloud sync (agents + skills + methodologies)** | Pro | Cross-device automation. |
| **Embed key** (API key for trace upload) | Pro | Mint-on-first-read. |
| **`ato evaluations methodology run`** | Pro | Codified fan-out orchestrator (re-tiered 2026-05-25). |
| **`ato evaluations methodology adopt`** | Pro | Codified ingest-existing-dispatches automation. |
| **`ato evaluations methodology score`** | Pro | Codified rubric-application loop. |
| **`ato evaluations methodology margin`** | Pro | Codified cost-ledger view. |
| **`ato evaluations methodology schedule create`** | Pro | Codified scheduling. |
| **`ato evaluations methodology diagnose`** (v2.11) | Pro | Codified learning loop. |
| **Provider keys (encrypted key store for cron usage-poller)** | Team | ATO holds user credentials → highest trust. |
| **Team workspaces (multi-user)** | Team | Shared agents + skills across teammates. |
| **Enterprise SSO** | Enterprise | SAML / OIDC. |
| **Audit trail (unlimited retention, SOC2)** | Enterprise | |
| **Evaluator budgets** | Enterprise | Per-team spend caps. |
| **HALO (org-wide safety guardrails)** | Enterprise | |

## Pricing (as of 2026-05-25, not yet public)

| Tier | Price | What you get |
|---|---|---|
| **Free** | $0 forever | Every primitive above marked Free. MIT-licensed desktop app. BYOK API keys. Unlimited use. |
| **Pro** | $29/seat/mo | Everything Free + the seven Pro automations above. Local execution still uses your API keys; cloud features run on our infra. |
| **Team** | $49/seat/mo | Everything Pro + multi-user workspaces + encrypted provider-key store. Multi-user state syncs through ato-cloud. |
| **Enterprise** | Custom | Everything Team + SSO + SOC2 audit retention + eval budgets + HALO. Contact sales. |

## How the gate is enforced

Three checkpoints in the stack — all read from the same FEATURES catalog so the surfaces stay consistent:

1. **Desktop app**: `<TierGate feature="methodology.diagnose">` wraps the button. Free users see a crown badge + click → UpgradePrompt modal. Already shipping for `evaluators.scheduled`, `cloud-traces`, `cloud-sync`. v2.11 adds `methodology.diagnose` + `methodology.schedule`.
2. **CLI**: `crate::tier::require_feature("methodology.schedule")` at the top of each gated handler. Bails with a structured upgrade prompt that mentions the DIY escape hatch. Resolution chain: `ATO_TIER` env override → cached `~/.ato/auth.json` tier → `/api/auth/me` probe (5s timeout) → Free fallback. 24h cache TTL.
3. **MCP server**: tier-gated tools are simply absent from `tools/list` for Free users (a future MCP-side change tracked separately). For v2.11, gating happens in the CLI the MCP tool shells out to.

## The DIY escape hatch (always available, by design)

Every Pro feature has a free-primitive equivalent the customer can compose by hand. This is the principle: we charge for the button, not for the capability. Documented examples:

- **Replace `methodology.run`** with a bash loop around `ato dispatch` that fires the variant matrix yourself + parses JSON receipts + writes summary stats with `jq` + your own math. Loses the runner's structured state machine (resume on partial failure, atomic per-cell commits), the LLM-judge fan-out, the cost ledger integration, the per-dispatch cell tagging that lets `methodology runs show` reconstruct your matrix later.
- **Replace `methodology.schedule`** with `crontab -e` + a shell script calling your own version of the above. Loses the per-job log file, status tracking, integration with the desktop app's Schedules tab.
- **Replace `methodology.diagnose`** with `ato dispatch` calls that construct the diagnose prompt yourself + apply the JSON output to a copy of the agent definition + re-run the methodology + diff scores by hand. Loses the locked input shape, the structured operations enum, the Welch-t win condition, the lineage tracker, the auto-revert watch.
- **Replace `cloud-sync`** with a personal git repo + a cron job to push `~/.claude/agents/` and `~/.ato/local.db`. Loses cross-device live sync; gains git history (some prefer this).

Marketing this escape hatch is the point. Customers who DIY become customers who know exactly what value the Pro button adds, then buy when they're tired of maintaining their own.

## Doctrine guardrails (what we don't do)

- **No artificial scarcity on local execution.** If it runs on your Mac with your API keys + adds no incremental infra cost to us, it stays Free. Period.
- **No retroactive Free→Pro re-tiers without grandfathering.** v2.10's schedule was Free; v2.11 gates only NEW creates Pro. Existing schedules keep firing.
- **No locking the underlying primitive when we gate the button.** `ato dispatch`, `ato review`, the SQLite schema, the MCP server — all Free. Pro gates the *automation we built on top*, never the primitive.
- **No selling features that aren't shipped.** Pricing page lists what's available today; the roadmap lists what's coming. We don't pre-sell.

## Cross-references

- Implementation: `apps/desktop/src/lib/tier.ts` (catalog), `apps/cli/src/tier.rs` (CLI gate), `apps/desktop/src/components/Tier/TierGate.tsx` (UI gate).
- ROI copy: `apps/desktop/src/lib/featureRoiCopy.ts`.
- Feature lookup for the CLI: `apps/cli/src/commands/pro.rs` (`FEATURES` const).
- Learning-loop design: `docs/v2.11-learning-loop.md`.
- Methodology runner spec: `docs/methodology-runner.md`.
- Pricing transparency rate card: `packages/ato-pricing/pricing.json`.
