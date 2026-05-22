# ATO GTM Strategy — 2026-05-22

**Status:** PRIVATE / INTERNAL. Do NOT commit to the public OSS repo.
**Author:** Will + claude (Anthropic Opus 4.7) synthesizing the
2026-05-22 six-seat G-stack war-room (`87E6CADF`).
**Companion docs:** [Sales pitch](./SALES-PITCH-2026-05-22.md) ·
[Slide deck](./SLIDE-DECK-2026-05-22.md) · Memory
[[project-security-warroom-2026-05-22]]

---

## 1. Why this document exists

On 2026-05-22 Daniel Lestinge asked Eduardo Tolmasquim on LinkedIn:
*"tem alguma trava de restrição de usuário? ou tipo, todo mundo
pode ver todas as conversas?"* That single question — asked under a
post about a custom MCP wired into a meeting-transcript data lake —
exposed the canonical 2026 pattern:

> CTOs and product teams have already shipped their own data lakes
> + custom MCPs. The agents work. The team is using them heavily.
> Now they're hitting the wall — *who can see what*, *who's
> accountable*, *can the agent be tricked into leaking what it
> shouldn't*?

ATO answers all three. But our positioning today (GUI for creating
agents) doesn't say so. This document fixes that — by mapping
exactly **where ATO enters** a buyer's workflow, **what they do
today instead**, and **how we sell that we're the better path**.

The strategic axis here is *control plane*, not features. Everyone
ships features. The win is being the place a CTO drops their MCP
and says: *"now my team can use this without me losing sleep."*

---

## 2. The buyer archetype (who pays us first)

From the office-hours seat (war-room 87E6CADF, 2026-05-22):

> 15-50 person LATAM startup, custom MCP shipped in last 90 days,
> hooked to a transcript / financial / customer corpus they don't
> want every employee browsing, with ≥1 regulator-facing role on
> the team, on Claude Code or Codex (not just ChatGPT). ~50-200
> hand-DMable companies in pt-BR / es-LA tech Twitter.

The three named first-paying-logo candidates (ranked by sales-cycle
compression, NOT by stated interest):

| Rank | Name | Company | Why they buy first | Their pain |
|------|------|---------|--------------------|-------------|
| **#1** | Bruno Cury | Cumbuca (YC S21, fintech) | Compliance compresses cycle ~6 months — regulator is asking now | Every dev with claude.ai is one prompt-injection away from a regulatory event |
| **#2** | Eduardo Tolmasquim | Purple Metrics | Built lake + MCP already; team in active heavy use → surface area is real | "We trust the team" only scales to ~15 people; he's there |
| **#3** | Daniel Lestinge | BlueForecast | Asked the question publicly → demand is real but he's pre-build | He's about to write 1,000 LOC of permission code we already have |

**Disqualified personas** (do NOT chase first):
- Enterprise security buyers (12-month cycles)
- Regulated banks (need SOC 2 / SSO we don't ship)
- Non-MCP teams (they don't yet feel the pain)
- "AI tinkerers" with no team (don't need RBAC; they're our OSS users)

---

## 3. How they do it today (the 3 states a buyer is in)

### State 1: DIY everything (likely Bruno)

```
1. Pick a vendor (Anthropic, OpenAI, Google) → write code calling SDK
2. Build MCP server in Python/TS using Anthropic's MCP SDK
3. Configure env vars on each team member's laptop
4. Deploy somewhere (Vercel function, internal server, Slack bot)
5. Hope auth works (usually doesn't — everyone shares one API key)
6. Get bill at end of month with NO per-user breakdown
7. Bolt on Langfuse/Helicone AFTER something breaks
```
**Time to first agent**: days-weeks. **Time to prod**: weeks-months.
**Pain index**: ▓▓▓▓▓ (5/5)

### State 2: Claude Code / Claude Desktop with MCP (likely Eduardo)

```
1. Each dev installs Claude Code on their Mac
2. Writes ~/.claude/config.json pointing at the MCP server
3. Asks questions; Claude calls MCP tools; results stream back
4. NO room scoping — everyone with the config sees everything
5. NO audit — claude.ai's history is per-user, not per-team
6. SINGLE vendor — can't compare what gpt/gemini would say
7. NO deployment story — only works in Claude Code itself
```
**Time to first agent**: hours. **Time to prod**: never (dev-only).
**Pain index**: ▓▓▓░░ (3/5 — works for the dev, fails for the team)

### State 3: Cobble SaaS observability (mid-market)

```
1. Use OpenAI/Anthropic API directly
2. Add Langfuse OR Helicone as gateway
3. Get traces, get cost-per-user maybe
4. Still write all agent orchestration yourself
5. Bolt on frontend (Streamlit, custom React)
6. Still no RBAC, no anti-injection, no comparison
```
**Time to first agent**: days. **Time to RBAC+audit**: months of custom.
**Pain index**: ▓▓▓▓░ (4/5)

---

## 4. How they enter with ATO (the demoable arc)

### Solo dev → first dispatched agent — **3 minutes**
```
brew install ato                                                  # 30s
ato login                                                          # 1m
ato agent create my-agent --runtime claude                         # 30s
ato mcp add purple-lake --url https://purplemcp.acme.com           # 30s
ato dispatch my-agent "ask the question"                           # works
```

### Team-shared + audited — **5 minutes**
```
# (continuing from above)
# Open ATO desktop → drop agent in a room → invite team
# Each team member runs `ato login` on their Mac → joins workspace
# Every dispatch from any team member is in the audit log
```

### Production deploy — **6 minutes**
```
ato deploy my-agent --target vercel
# bundle includes UNTRUSTED_INPUT wrapping + identity headers
# customer's Vercel project + customer's BYOK key = agent serves end-users
```

**This is the demo arc.** Three minutes from `brew install` to
working multi-LLM agent. Five to team-shared. Six to production.
No competitor lands the same three milestones in one tool.

---

## 5. Where ATO enters the workflow (the three entry points)

```
┌─────────────────────────────────────────────────────────────────┐
│ ENTRY 1 — Terminal user (developer)                            │
│   They type `ato dispatch` instead of `claude` or `codex`       │
│   Get multi-LLM + audit + war-rooms immediately                 │
│   Time: 1 command                                               │
│   Status: SHIPPED                                               │
├─────────────────────────────────────────────────────────────────┤
│ ENTRY 2 — Desktop user (team lead)                             │
│   Install ATO.app → create agent → connect MCP → share         │
│   Get rooms + RBAC + workspace sync (Phase 2 cloud)            │
│   Time: 5 minutes                                               │
│   Status: OSS desktop SHIPPED; team workspace = Phase 2         │
├─────────────────────────────────────────────────────────────────┤
│ ENTRY 3 — Deployed bundle author (consumer-facing)              │
│   Design agent in ATO desktop → `ato deploy --target X`        │
│   Bundle runs on customer's infra; ATO is the build pipeline    │
│   Time: 6 minutes                                               │
│   Status: Bundle generator SHIPPED; security floor = Phase 1   │
└─────────────────────────────────────────────────────────────────┘
```

The constant across all three: **ATO never holds the LLM bill**.
Customer pays Anthropic / OpenAI / Google directly. We're a
control + sync + guardrails layer. This is the structural
difference vs every observability-as-gateway competitor.

---

## 6. Competitive landscape — the stack, not a vs-matrix

> **Locked scope per [[project-ato-scope-boundary]]: ATO is
> COMPLEMENTARY to Langfuse / Helicone, NOT competitive.** We chose
> to stay out of production observability + production gateway. The
> stack map below is how it lives in production.
>
> Detailed per-competitor breakdown:
> [COMPETITIVE-RESEARCH-2026-05-22.md](./COMPETITIVE-RESEARCH-2026-05-22.md)

```
   ┌──────────────────────────────────────────────────────┐
   │  END USER (or developer asking a question)           │
   └────────────────────────┬─────────────────────────────┘
                            ▼
   ┌──────────────────────────────────────────────────────┐
   │  ATO — Build / dispatch / war-room / RBAC / deploy   │
   │  ★ our lane: dev workflow + team control plane       │
   └────────────────────────┬─────────────────────────────┘
                            ▼
   ┌──────────────────────────────────────────────────────┐
   │  HELICONE / PORTKEY — Gateway, caching, fallback     │
   │  their lane: production routing layer                │
   └────────────────────────┬─────────────────────────────┘
                            ▼
   ┌──────────────────────────────────────────────────────┐
   │  LLM PROVIDER (Anthropic / OpenAI / Google / etc)    │
   │  + customer's fine-tuned models live here (BYOM)     │
   └────────────────────────┬─────────────────────────────┘
                            │
                            ▼
   ┌──────────────────────────────────────────────────────┐
   │  LANGFUSE / LANGSMITH / BRAINTRUST — Observability   │
   │  their lane: post-hoc tracing + eval workbench       │
   └──────────────────────────────────────────────────────┘
```

**ATO sits ABOVE the gateway, the LLM, and the observability
layer.** Customers can use ATO **AND** Langfuse **AND** Helicone —
and most mature teams will. We are not trying to take Langfuse's
revenue. The integration story is the pitch.

### Where each tool genuinely lives

| Layer | Who owns it | ATO's stance |
|-------|------------|--------------|
| Dev workflow / team control plane | **ATO** | Our lane |
| Build / multi-LLM war-rooms / RBAC | **ATO** | Our lane |
| Gateway / routing / caching | Helicone, PortKey | We integrate (point ATO at their endpoint) |
| LLM inference | Anthropic, OpenAI, Google, fine-tuned customers | BYOK + BYOM — we never hold the bill |
| Production observability | Langfuse, LangSmith | We integrate (output traces to them) |
| Eval workbench | Braintrust, Promptfoo | We integrate (export to them at design time) |
| Enterprise MS-stack | Copilot Studio | We don't compete (different ICP) |
| Enterprise GCP-stack | Vertex Agents | We don't compete (different ICP) |
| DIY framework | LangChain | We absorb LangChain users (they graduate from DIY to ATO) |

### The category gap ATO genuinely fills

```
                       ┌─────────────────────────────┐
                       │  DEV WORKFLOW + CONTROL     │
                       │  PLANE for multi-LLM agents │
                       │                              │
                       │  (build → test → war-room → │
                       │   RBAC → deploy)            │
                       │                              │
                       │           = ATO              │
                       │                              │
                       │  Nobody else in this box.    │
                       │  Everyone else is one        │
                       │  layer above or below.       │
                       └─────────────────────────────┘
```

Nobody is shipping a control plane at the dev-workflow level.
Everyone is observability (post-hoc), eval (test-time), gateway
(cost layer), or framework (write-it-yourself). ATO sits at the
intervention point — between the team and the model, at the moment
of decision.

---

## 7. Where ATO genuinely wins (the 5 we sell)

1. **Multi-LLM by design** — war-rooms ask 3+ LLMs the same
   question in parallel for decisions. NO competitor does this.
   For non-trivial calls, this is *irreplaceable*.
2. **Local-first** — all data on the Mac. Legal/medical/financial
   buyers literally can't use cloud-only tooling for compliance.
   ATO is one of the only options that doesn't require shipping
   their data to a SaaS.
3. **MCP-native** — not a connector library; the *primary* data path.
   Anthropic's MCP is the spec everyone's converging on; ATO bets the
   company on it.
4. **Dev workflow control plane** — we own the moment of decision
   (permissions, room ACLs, content policy, war-room dispatch).
   Different *category* from observability — Langfuse/LangSmith/
   Braintrust observe what happened; ATO controls what's about to
   happen.
5. **Build → test → compare → deploy in one tool** — every other
   workflow tool is one slice. ATO is the lifecycle.
6. **BYOM (Bring Your Own Model)** — customer's fine-tuned models
   from OpenAI/Anthropic/Together/Fireworks plug into ATO as
   first-class citizens. War-rooms, audit, deploy all work with
   custom model IDs identically to base models. We never hold the
   training contract or the inference bill — both stay with the
   provider, where they belong.

---

## 8. Where ATO honestly loses (don't lie)

1. **No SaaS hosting today.** Customer must install on a Mac. Linux
   barely tested. Windows roadmap. Deployed-bundle is the workaround.
2. **Younger / smaller ecosystem.** Langfuse has thousands of
   installs and 50+ SDK integrations; ATO is in months not years.
3. **No enterprise compliance certifications yet** (SOC 2, ISO).
   Anyone who needs that today goes to Copilot Studio or Vertex.
4. **No native vector DB.** Customer brings their own via MCP
   (which is actually the right call — see *advantages*, not a loss).

**NOT a loss (intentional architecture)**:

- **Fine-tuning / training** — happens AT the provider (OpenAI,
  Anthropic, Google, Together, Fireworks). Customer trains their
  custom model with the provider, gets a model ID, points ATO at
  that ID. ATO works with the custom model exactly like any base
  model: war-rooms, audit, deploy, RBAC — all of it. This is
  **BYOM (Bring Your Own Model)** — the natural extension of BYOK.
- **Production observability** — Langfuse / LangSmith own this.
  We integrate; we don't compete. See §6 stack map.
- **Gateway / caching** — Helicone / PortKey own this. We
  integrate; we don't compete.

**How to handle each in a sales conversation:**

| Objection | Response |
|-----------|----------|
| "We need SaaS" | "Deploy-bundle ships your agent to Vercel / Lambda / Anthropic's hosted infra in 1 command. The build pipeline is local; the runtime is yours." |
| "You're new" | "We ship in days, not quarters. Production-ready security floor (UNTRUSTED_INPUT + identity passthrough) shipping Phase 1. You'll see velocity." |
| "We need fine-tuning" | "Fine-tune at the provider. Then point ATO at your custom model ID — war-rooms, audit, deploy all work the same as with a base model. We're BYOM by design." |
| "We need RAG" | "Wire your vector DB via MCP — that's MCP's purpose. We don't lock you to one." |
| "We need integrations" | "MCP is THE integration layer. Anyone shipping an MCP today is shipping it for everyone — including us, automatically." |
| "We use Langfuse for observability" | "Keep using Langfuse — we sit ABOVE it. ATO builds + dispatches; Langfuse traces. They're complementary, not competitive." |
| "We use Helicone as our gateway" | "Same answer — keep Helicone for the gateway layer. ATO points at your Helicone endpoint as the LLM URL. Both happy." |
| "We need SOC 2" | "Phase 3 enterprise tier ships SSO + audit retention; SOC 2 in flight. For Q1 contracts that's not blocking; for Q4 contracts it will be. Pilot now, certify together." |

---

## 9. The three pitch angles (one per archetype)

### To Bruno (CISO concerned, fintech)
> **"The control plane your CISO would let you ship.**
> Audit, RBAC, anti-injection — built-in, not bolted-on.
> Every agent call carries a user ID; every output goes through a
> guardrail; every denial is logged. Your data never leaves the
> Mac unless you wire the sync. Your API key never leaves the
> keychain."

**Demo flow** (live, 6 min):
1. `ato dispatch` against his MCP — show audit log row with user_id
2. Show a deployed agent's UNTRUSTED_INPUT wrapper rejecting an
   injection attempt
3. Show denial events in the room UI
4. Show the workspace audit retention controls

### To Eduardo (CTO with built lake + heavy team usage)
> **"You built the lake. ATO is where your team uses it —
> with rooms, audit, and the freedom to A/B different LLMs
> without rewiring. Same MCP. Same data. Now scoped, audited,
> deployable."**

**Demo flow** (live, 6 min):
1. Import his MCP into ATO (1 command)
2. Drop in a room with 3 PMs
3. Run a war-room — same product question to claude + gemini + gpt
4. Show the audit panel: who asked what, when, which model
5. Show the deploy button → "this same agent, on Vercel, in 30 seconds"

### To Daniel (greenfield, asking the right questions)
> **"Stop building the same chrome around every internal agent.
> ATO is the team chrome — RBAC, audit, multi-LLM,
> deploy-anywhere. You build the data lake; we handle the
> rest. Your 1,000 LOC of permission code is our default."**

**Demo flow** (live, 6 min):
1. `brew install ato` live
2. `ato login`, `ato agent create`, `ato mcp add`
3. Drop in a room, invite a teammate, show audit
4. `ato deploy --target vercel` → paste URL → working agent
5. Quote: "Compare this to what you'd have to write yourself"

---

## 10. The 90-second Loom (the one asset everything else depends on)

**Hero artifact.** Single piece of content that lands all three
archetypes via different ~20s slices. Without this, nothing else
converts.

### Shotlist

| Time | Visual | Voiceover |
|------|--------|-----------|
| 0:00–0:20 | ATO desktop UI; user drops an MCP into a new agent | *"I built a Postgres MCP for my team's customer data. I want to query it with LLMs — but my team needs scoping, audit, and the freedom to compare answers across models. Here's how ATO does it in five clicks."* |
| 0:20–0:40 | Terminal: `ato dispatch` against the MCP; response streams | *"One command dispatches against the agent. Same response you'd get from Claude Code — but logged with my user ID and the exact tokens used."* |
| 0:40–0:55 | War-room: same prompt fans out to claude+gemini+gpt; side-by-side outputs | *"Now the same question, in parallel, to three models. Pick the right answer. No competitor does this."* |
| 0:55–1:15 | ATO desktop: drop agent in a room; invite teammate; show audit feed scrolling | *"Drop the agent in a room. Invite my teammate. Every query they run shows up in the audit feed — who, what, when, which model."* |
| 1:15–1:30 | Terminal: `ato deploy --target vercel`; URL paste in browser; working agent | *"And when I'm ready to ship — one command exports the agent as a Vercel function. The deployed bundle includes the same safety floor."* |

### Voiceover guidelines
- Will (Portuguese-accented English) — sounds authentic, beats AI voice
- ~120 wpm, no filler
- Show real data (Eduardo's actual product context if he'll share)
- End with: *"Local-first, multi-LLM, MCP-native. Sleep next to your agents."*

### Distribution
1. **Eduardo's LinkedIn thread reply** — public, social proof for Bruno + Daniel
2. **Bruno DM** — 24h later, "saw your Granola vs lake question; built something adjacent; would love your read"
3. **Daniel DM** — 48h later, link to the Loom + Team-trial code
4. **ATO landing page hero** — replace any existing video

---

## 11. The killer ask in each first conversation

**Bruno (Cumbuca)**: *"What's your current process for a dev who
wants to ask your customer DB a question via LLM — and what would
your CISO require to approve that pattern at scale?"*
→ Maps directly to our P2 + room ACL story.

**Eduardo (Purple Metrics)**: *"When the next 5 PMs join, how do you
plan to scope which customer cohorts each one sees through the
lake? And how do you A/B different models for different question
types?"*
→ Maps directly to our rooms + war-rooms story.

**Daniel (BlueForecast)**: *"What were the 3 features you were
about to build yourself when you posted that question? Mine likely
already does them — but if I'm missing one, that's where I want
your feedback."*
→ Co-creates the spec with him. Daniel as design partner.

---

## 12. Pricing (private — do NOT commit to public roadmap)

### Tier table

```
FREE / OSS
  Local dev mode, BYOK, local audit
  P0 sanitization + P2 identity headers
  Unlimited agents, unlimited dispatches
  
TEAM — $25 USD / seat / month
  Everything in Free PLUS:
  - Workspace + room ACLs (P3)
  - Cloud audit sync + denial UI (P4)
  - 10k classifier calls / seat / month INCLUDED (P1.b)
  - $0.50 / 1k classifier-call overage
  - Up to 5 deployed bundles (Mode C)
  - 5 named seats; rooms unlimited
  
DEPLOYED BUNDLE OVERAGE (Team tier add-on)
  Beyond 5 bundles or 100k requests/bundle/mo:
  - $50 / bundle / month
  - Classifier calls metered same as Team tier ($0.50/1k)
  
ENTERPRISE — custom
  SSO, audit retention >1yr, on-prem audit option, SLA
```

### CAC payback math (pricing seat, war-room 87E6CADF)

- Team @ $25 × 5 seats = **$125/mo recurring**
- Target CAC payback: < 12 months → max CAC ≤ $1,500
- On a $500 CAC: payback in **4 months**, profitable from month 5
- 20% annual churn ceiling

### The pricing decision tree for an inbound

```
Are they a single dev?         → Free / OSS
Are they ≤ 5 people, local?    → Free / OSS
Are they 5-50 people, internal? → Team ($25/seat)
Are they shipping a public agent? → Team + bundle overage
Are they regulated / SOC 2 needed? → Enterprise (custom)
```

### Pricing-seat warning

> "Flat-rate including unlimited classifier calls is a margin-death
> trap because per-call cost scales linearly with usage and Google
> can adjust flash-lite pricing without notice." — pricing seat,
> 2026-05-22

This is why the SKU is **seat + bundled classifier + overage**, NOT
flat. Repeat this mantra when ANY sales conversation tries to
negotiate "include unlimited classifier calls in Team."

---

## 13. The 14-day falsifier (from office-hours seat)

**Hand-pick 3 named buyers**: Bruno, Eduardo, Daniel.
**DM each** a 90-sec Loom + Team-trial code (sequencing per §10).
**Threshold**: ≥1 of 3 activates the trial within 14 days.

**If 0/3 activate** → the problem isn't acute enough for paid
procurement; reconsider whether MCP-builders are the buyer at all.
Drop the security narrative + refocus on the GUI-for-agents wedge
that v1.3.0 just shipped.

**Demo-falsifier** (separate, runs in parallel):
- Post the Loom in Eduardo's LinkedIn thread
- **Threshold**: ≥500 views in 48h AND ≥3 inbound DMs in 7d
- **If miss**: messaging failed, not the wedge. Rewrite pitch.

---

## 14. What ships when (the actual plan)

### Phase 1 — OSS (this 2-week sprint)

| Artifact | Owner | Status |
|----------|-------|--------|
| P0 — UNTRUSTED_INPUT wrappers (api_dispatch_tools.rs + deployBundleGenerators) | claude + minimax | branch open |
| P2 — Identity passthrough headers + MCP-author guide | claude + minimax | next |
| ROADMAP entry (public, OSS-only) | claude | `b3b4b8a` SHIPPED |
| Loom (90 sec) | Will | not started |
| Comparison docs (public, SEO surface) | claude | drafting now |

### Phase 2 — Cloud (after falsifier passes)

| Artifact | Owner | Status |
|----------|-------|--------|
| workspace + members + role schema | ato-cloud repo | not started |
| war_rooms gain allowed_agents + members | ato-cloud repo | not started |
| P4 denial-event writer + room banner | ato-cloud repo | not started |
| P1.b classifier path + metering | ato-cloud repo | not started |
| Team-tier billing page | ato-cloud landing | not started |

### Phase 3 — Enterprise (when first paying logo asks)

| Artifact | Status |
|----------|--------|
| SSO via WorkOS / Okta | not started |
| Audit retention > 1yr | not started |
| On-prem audit option | not started |
| Contractual SLA | not started |

---

## 15. Risks + mitigations

| Risk | Mitigation |
|------|------------|
| Anthropic launches a competing "Claude Teams" with hosted agents + RBAC | Lean harder on multi-LLM. Their lock-in is our differentiator. |
| Langfuse adds an "agent builder" surface | Already would; they don't because their customer is the SRE, ours is the dev. Different ICP. |
| MCP fizzles as a standard | Unlikely (Anthropic + community converging) — but if so, ATO's MCP-native bet weakens. Fallback: support tool-calling natively (we do already). |
| LATAM startups can't pay USD | Offer BRL pricing on Team for pt-BR. Same dollar value, different currency presentation. |
| Bruno/Eduardo/Daniel ghost us | Office-hours' falsifier kicks in. 14 days, drop the narrative, refocus on GUI wedge. |
| Will runs out of energy before the 14-day falsifier completes | This is the real risk. Block 6 hours total over 2 weeks for the 3 DMs + 1 Loom + 1 LinkedIn reply. No new strategic threads until done. |

---

## 16. What this document does NOT cover

(Filed as follow-ups; out of scope for the 14-day window.)

- Channel partnerships (Anthropic ecosystem, GitHub Marketplace,
  Vercel integrations partner program)
- Open-source community building (Discord, conference talks)
- Pricing experiments beyond $25/seat (annual discount, NGO tier,
  startup credit programs)
- International expansion outside LATAM/US English
- Self-serve onboarding analytics + cohort funnels
- The actual ato-cloud billing implementation (Stripe vs LemonSqueezy
  vs Paddle — punt to whoever ships P1.b)

---

## 17. Sign-off + next actions

**Locked decisions** (from war-room 87E6CADF, six seats unanimous
ADJUST-SCOPE):
1. ✅ Ship P0 + P2 in OSS within 14 days
2. ✅ KILL P1.a (system-prompt-only policy) as theater
3. ✅ Defer P1.b / P3 / P4 to closed-source ato-cloud, Phase 2
4. ✅ Position as **control**, not security
5. ✅ Hero: *"The war room for building agents. The guardrails for
   running them."*
6. ✅ $25/seat Team SKU with bundled classifier + overage

**Will's next 5 actions** (this week):
1. Record the 90-sec Loom (script in §10)
2. Reply to Eduardo's LinkedIn thread with the Loom + soft positioning
3. DM Bruno 24h later
4. DM Daniel 48h later
5. Track activation in ato-cloud trial signups

**Claude's next 5 actions** (concurrent):
1. Ship P0 implementation (this branch, with minimax as sub-engineer)
2. Ship P2 implementation (next branch)
3. Land `docs/comparison.md` in OSS (SEO surface, public)
4. Land `docs/mcp-author-guide.md` in OSS
5. Update ROADMAP with Phase 2 closed-source pointers

---

*End of GTM document. The companion sales pitch + slide deck pull
specific sections of this into customer-facing artifacts.*
