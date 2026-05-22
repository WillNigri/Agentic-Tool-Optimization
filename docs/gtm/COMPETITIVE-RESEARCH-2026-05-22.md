# Competitive Research — 2026-05-22

**Status:** PRIVATE / INTERNAL. Companion to
[GTM-STRATEGY-2026-05-22](./GTM-STRATEGY-2026-05-22.md).
**Sources:** Public docs, pricing pages, GitHub READMEs, hands-on
familiarity. Knowledge cutoff Jan 2026 — pricing pages may have
moved by the time you read this, verify before quoting in a deal.

---

## 1. Why this exists separately from the GTM doc

The GTM doc summarises competition in a one-screen table. This
document is the source-of-truth detail behind that table — the
per-competitor breakdown a salesperson needs when a prospect
says *"why not just use Langfuse?"*.

---

## 2. The 9 competitors that matter

### 2.1 Langfuse — `langfuse.com`

**One-sentence**: Open-source LLM observability platform with
tracing, prompt management, evals, and a usage dashboard.

**Pricing (as of Q4 2025)**:
- **Hobby**: free, self-host or 50k events/mo cloud, single user
- **Pro**: $59/user/mo cloud, unlimited events
- **Team**: $499/mo flat, SSO, projects, advanced eval
- **Enterprise**: custom (SOC 2 Type II, on-prem)

**Hosting**: Self-host (OSS) OR Langfuse Cloud (EU/US regions).

**Core noun**: "Observability." They call themselves "the observability
platform for LLM applications."

**They nail**:
- The SDK ecosystem (Python, JS, Go, Ruby, official wrappers for
  every major framework — LangChain, LlamaIndex, Vercel AI SDK,
  Haystack, OpenAI/Anthropic SDKs)
- The trace viewer (best-in-class span tree, replay, cost-per-trace)
- Self-host story (their Docker Compose works first time)

**They miss for our buyer**:
- **No agent builder.** You write the agent yourself. Langfuse only
  observes it.
- **No multi-LLM orchestration.** It TRACES whatever you call, but
  doesn't help you compare or route.
- **No RBAC at the room/conversation level.** Project-level only.
  You can't say "Alice sees customer-A traces, Bob sees customer-B."
- **No anti-injection enforcement.** It can RECORD that an injection
  attempt happened (if you tag it); it can't BLOCK it.
- **No MCP-native flow.** Bring your own MCP client; Langfuse just
  traces the calls.

**How a 15-50p team adopts them**:
1. Read the SDK docs, install the wrapper for their stack
2. Wrap their existing OpenAI/Anthropic calls
3. View traces in Cloud dashboard
4. Maybe set up alerts on cost / error rate
5. Add eval scoring via UI or SDK

**Multi-LLM stance**: Vendor-agnostic observability. They TRACE all
providers but don't help you orchestrate across them.

**Local-first stance**: Self-host option exists. Cloud is the
default path; team workspaces require Cloud.

**MCP stance**: They observe MCP calls if you trace them, but no
native MCP support.

**Public signals (Q4 2025)**:
- Series A from Lightspeed
- ~$3M ARR estimate
- ~10k GitHub stars
- Active community on Discord (~2k members)

**Where ATO wins vs Langfuse**: control plane (we intervene; they
record). Multi-LLM orchestration (war-rooms). MCP-native. Mac-native
UI. Anti-injection enforcement (P0 sanitization).

**Where Langfuse wins vs ATO**: SDK breadth (we have CLI + Tauri;
they have 8+ language SDKs). Maturity (they're 2+ years older).
Trace UI polish (their flame graph beats ours).

---

### 2.2 Helicone — `helicone.ai`

**One-sentence**: LLM gateway with caching, rate limiting, cost
tracking, and a trace viewer.

**Pricing (Q4 2025)**:
- **Free**: 10k requests/mo, no SSO
- **Pro**: $20/user/mo, 100k req/mo, support tier
- **Team**: $200/mo flat for 5 seats, SSO
- **Enterprise**: custom

**Hosting**: SaaS first, self-host edition available.

**Core noun**: "Gateway." They sell themselves as the proxy you put
in front of your OpenAI/Anthropic calls.

**They nail**:
- The drop-in gateway model — change your API URL, done
- Caching (semantic + exact) — real cost savings on repeat queries
- Rate limiting per API key
- Cost-per-user / cost-per-feature attribution

**They miss for our buyer**:
- **No agent builder.** Same Langfuse critique.
- **No team RBAC at agent/room level.** Per-key, not per-user.
- **Not MCP-aware.** They proxy LLM calls; MCP runs separately.
- **No multi-LLM orchestration.** They proxy whatever vendor you
  point them at, but don't run them in parallel.

**How a 15-50p team adopts them**:
1. Get Helicone API key
2. Change `OPENAI_BASE_URL` to `https://oai.helicone.ai/v1`
3. Add `Helicone-Auth` header to every request
4. Start seeing logs + cost in dashboard
5. Configure caching + rate limits per use case

**Multi-LLM stance**: They proxy multiple providers but don't
orchestrate; you pick one per call.

**Local-first stance**: Self-host edition exists but most users
are on cloud.

**MCP stance**: Not MCP-aware.

**Public signals (Q4 2025)**:
- Y Combinator W23 batch
- ~$1.5M ARR estimate
- Strong Discord community
- Aggressive open-source posture (most code MIT)

**Where ATO wins vs Helicone**: agent surface (we build agents;
they proxy them). RBAC at conversation level. War-rooms.

**Where Helicone wins vs ATO**: SaaS-first onboarding (zero install).
Caching (real $$ savings). SDK-free integration (just change URL).

---

### 2.3 LangSmith — `smith.langchain.com`

**One-sentence**: LangChain's official observability + eval product
for production LLM apps.

**Pricing (Q4 2025)**:
- **Free**: 5k traces/mo, 1 seat
- **Plus**: $39/seat/mo, 10k traces/mo
- **Enterprise**: custom (SSO, SLA, on-prem)

**Hosting**: SaaS first (US + EU regions); on-prem in Enterprise.

**Core noun**: "Observability + eval." Tightly bundled with the
LangChain framework.

**They nail**:
- LangChain integration (zero config if you're already LangChain)
- The Hub (prompt sharing + versioning)
- Eval workflows tied to dataset versioning
- The trace UI — best LangChain debugging surface

**They miss for our buyer**:
- **LangChain lock-in.** Best-in-class only if you're LangChain;
  weaker if you're using raw SDKs or other frameworks.
- **No agent builder UI.** You write LangChain code; LangSmith
  observes it.
- **No multi-LLM war-rooms.**
- **No local-first** — Cloud only for most features.
- **MCP support arriving but immature.**

**How a 15-50p team adopts them**:
1. Already using LangChain → add `LANGCHAIN_API_KEY` env var
2. Traces appear automatically
3. Set up eval datasets in UI
4. Run regression evals against new model versions

**Multi-LLM stance**: Whatever LangChain supports (most providers).
Eval surface lets you A/B them.

**Local-first stance**: Cloud-first. Enterprise on-prem option exists.

**MCP stance**: Early support; not their primary integration path.

**Public signals (Q4 2025)**:
- Owned by LangChain Inc; reportedly Sequoia/Benchmark backed
- ~$8M ARR estimate
- Massive distribution via LangChain (100k+ GitHub stars on LangChain itself)

**Where ATO wins vs LangSmith**: framework-agnostic (we're not
LangChain-shaped). MCP-native. War-rooms. Local-first.

**Where LangSmith wins vs ATO**: LangChain ecosystem (if customer is
already there). Eval workbench depth. Enterprise sales motion.

---

### 2.4 Braintrust — `braintrust.dev`

**One-sentence**: Eval-first LLM developer platform — datasets,
evals, playground.

**Pricing (Q4 2025)**:
- **Free**: 1M spans/mo
- **Pro**: $249/mo flat, 10M spans
- **Enterprise**: custom

**Hosting**: Cloud only, EU/US regions.

**Core noun**: "Evals." Their playground is the entry point.

**They nail**:
- The eval playground — best UI for iterating on prompts
- Dataset management (versioning, branching)
- Side-by-side prompt comparison
- Integration with their own SDK

**They miss for our buyer**:
- **Eval-first means runtime-second.** They're a workbench, not a
  control plane.
- **No agent builder.** You build the agent elsewhere.
- **No RBAC at conversation level.**
- **Not local-first.**
- **No MCP.**

**How a 15-50p team adopts them**:
1. Sign up, get API key
2. Upload eval dataset (CSV / JSONL)
3. Run prompts in playground; iterate
4. Pin best prompt → deploy to production via their SDK
5. Observe production calls via SDK traces

**Multi-LLM stance**: Side-by-side prompt comparison across providers
(closest thing to war-rooms in this list, but at eval time, not
decision time).

**Local-first stance**: Cloud-only.

**MCP stance**: Not native.

**Public signals**:
- Founded by ex-Figma / Stripe leadership
- a16z seed + Series A
- ~$5M ARR estimate
- Strong developer-tools brand

**Where ATO wins vs Braintrust**: runtime intervention vs eval-time
only. Multi-LLM at decision time. Local-first.

**Where Braintrust wins vs ATO**: eval workbench depth. Dataset
versioning. Polished playground.

---

### 2.5 Promptfoo — `promptfoo.dev`

**One-sentence**: CLI-first prompt eval tool with red-teaming.

**Pricing (Q4 2025)**:
- **OSS**: free (MIT), self-host
- **Pro**: $99/seat/mo cloud, team eval dashboard
- **Enterprise**: custom (red-team service tier)

**Hosting**: OSS CLI runs anywhere; Cloud dashboard is optional.

**Core noun**: "Eval CLI." `promptfoo eval` is the entry point.

**They nail**:
- The CLI — fits dev workflows (CI / GitHub Actions)
- Red-teaming / adversarial test generation (best in class)
- YAML-defined test suites
- Multi-provider eval matrices

**They miss for our buyer**:
- **Test-time only.** No production runtime, no observability.
- **No agent builder.**
- **No RBAC.**
- **No deployment story.**

**How a 15-50p team adopts them**:
1. `npm install -g promptfoo`
2. Write `promptfooconfig.yaml` with test cases
3. Run `promptfoo eval`
4. View results in CLI or web UI
5. Wire into CI for regression eval

**Multi-LLM stance**: Eval-time multi-provider matrices.

**Local-first stance**: CLI is local; Cloud is optional add-on.

**MCP stance**: Not native.

**Public signals**:
- ~5k GitHub stars
- Strong reputation in AI red-teaming
- a16z backing

**Where ATO wins vs Promptfoo**: runtime + RBAC + agent builder.

**Where Promptfoo wins vs ATO**: red-teaming surface, CI/CD-native
eval flows.

---

### 2.6 PortKey — `portkey.ai`

**One-sentence**: AI gateway with routing, caching, fallbacks, and
observability.

**Pricing (Q4 2025)**:
- **Free**: 10k requests/mo
- **Pro**: $49/seat/mo, 100k requests, smart routing
- **Enterprise**: custom

**Hosting**: SaaS first, self-host option.

**Core noun**: "Gateway." Routing-first competitor to Helicone.

**They nail**:
- Smart routing (fallback chains, A/B routing)
- 100+ provider integrations
- Caching tiers
- Cost analytics

**They miss for our buyer**:
- **Pure gateway — no agent surface.**
- **No RBAC at conversation level.**
- **Not MCP-aware.**

**How a 15-50p team adopts them**:
1. Get PortKey API key
2. Change base URL to PortKey's
3. Configure routing rules
4. Get analytics dashboard

**Multi-LLM stance**: Multi-provider routing (their core feature).

**Local-first stance**: Self-host option exists.

**MCP stance**: Not native.

**Where ATO wins vs PortKey**: agent surface, RBAC, MCP.

**Where PortKey wins vs ATO**: routing sophistication, provider
breadth, gateway maturity.

---

### 2.7 Anthropic's hosted agentic platform (Claude Skills + Agents)

**One-sentence**: Anthropic-native hosted agents using Claude with
Skills and Tool Use; runs in Anthropic's cloud.

**Pricing**:
- Folded into Anthropic API pricing (per-token)
- Skills + Agents are platform features, not separately billed
- Enterprise pricing for Anthropic Console / Workbench seats

**Hosting**: Anthropic Cloud. Customer doesn't operate infra.

**Core noun**: "Claude agents."

**They nail**:
- Native Claude integration (best Claude UX)
- Skills as the primary abstraction
- Anthropic's safety + alignment baked in
- Tool Use API is mature

**They miss for our buyer**:
- **Single-vendor lock.** Anthropic-only.
- **No multi-LLM comparison** (their whole product is "use Claude").
- **No team RBAC primitives** (Anthropic Console has org-level only).
- **No local-first** (Anthropic Cloud only).
- **Customer can't bring their own MCP and have Anthropic host it.**

**How a 15-50p team adopts them**:
1. Sign up for Anthropic Console
2. Build agent in Workbench
3. Deploy via Claude API
4. Use Anthropic's Skills system for tools
5. Manage org seats in Console

**Multi-LLM stance**: Single-vendor (Claude only).

**Local-first stance**: Cloud only.

**MCP stance**: They INVENTED MCP. Best support.

**Where ATO wins vs Anthropic Agents**: multi-LLM (we A/B against
GPT, Gemini, Grok — they can't). Local-first. BYOK (customer keeps
keys; with Anthropic, the data flows through Anthropic Cloud).

**Where Anthropic wins vs ATO**: best Claude UX. Native MCP. Brand
trust. Hosted infra.

---

### 2.8 Microsoft Copilot Studio

**One-sentence**: Enterprise low-code agent builder tied to
Microsoft 365 + Azure.

**Pricing**:
- Per-message tier: $200/mo for 25k messages
- M365 Copilot license: $30/user/mo (bundled with Office)
- Enterprise: custom

**Hosting**: Azure / M365 cloud.

**Core noun**: "Copilot." MS-stack agents.

**They nail**:
- M365 integration (Outlook, Teams, SharePoint, Dynamics)
- Enterprise SSO + compliance (SOC 2, ISO 27001, HIPAA available)
- Low-code visual builder
- Azure ecosystem play

**They miss for our buyer**:
- **Hard MS lock-in.** Outside the MS-shop, this is friction.
- **No multi-LLM** (uses Azure OpenAI by default).
- **No local-first.**
- **Heavy / slow** — designed for enterprise, not 15-50p startups.

**Where ATO wins vs Copilot Studio**: lightweight, multi-LLM,
local-first, MCP-native, for startups not enterprises.

**Where Copilot Studio wins vs ATO**: M365 integration. Enterprise
compliance. SSO. Existing budget line.

---

### 2.9 Google Vertex Agent Builder

**One-sentence**: GCP's enterprise agent builder for Vertex AI +
Gemini.

**Pricing**:
- Per-call (Gemini API pricing)
- Vertex AI Agent Builder seat: $200/mo
- Enterprise: custom

**Hosting**: GCP only.

**Core noun**: "Agent." GCP-stack.

**They nail**:
- Gemini integration
- GCP data warehousing integration (BigQuery, Cloud SQL)
- Vertex AI Search RAG built-in
- Enterprise compliance

**They miss for our buyer**:
- **Hard GCP lock-in.**
- **No multi-LLM** (Gemini-first).
- **No local-first.**
- **Heavyweight onboarding** (must learn Vertex AI primitives).

**Where ATO wins vs Vertex**: multi-LLM, local-first, MCP-native,
3-minute time-to-first-agent vs hours of Vertex onboarding.

**Where Vertex wins vs ATO**: BigQuery / GCP data warehouse depth.
Enterprise sales motion. Gemini lock-in for Google-shop teams.

---

### 2.10 (Bonus) LangChain Agents — the DIY framework

**One-sentence**: Python/JS framework for building agents from primitives.

**Pricing**: OSS / free; commercial via LangSmith.

**Hosting**: Bring your own.

**Core noun**: "Framework."

**They nail**:
- Most-popular framework
- Massive ecosystem (every model + tool + DB)
- Cookbook recipes
- LCEL composition language

**They miss for our buyer**:
- **Customer writes everything themselves.** ATO is the *product*;
  LangChain is the *library*.
- **No team UI.**
- **No deployment story** (you wire your own).
- **Steep learning curve.**

**Where ATO wins vs LangChain**: time-to-first-agent (3 min vs days).
Team layer (rooms, audit). No-code surface for non-engineers.

**Where LangChain wins vs ATO**: extreme flexibility. Ecosystem
breadth. If customer needs custom orchestration we don't support.

---

## 3. Master stack map — NOT a vs. matrix

**Correction from earlier draft (Will caught it 2026-05-22):** our
locked positioning per [[project-ato-scope-boundary]] is
**complementary** to Langfuse / Helicone, NOT competitive. We chose
to stay out of production observability. The earlier vs-matrix
broke that boundary and made us look like we're trying to be a
worse Langfuse. We're not. We're in a different category that
runs ABOVE theirs.

### How the stack actually layers in production

```
   ┌──────────────────────────────────────────────────────┐
   │  END USER                                            │
   │  (or developer asking a question)                    │
   └────────────────────────┬─────────────────────────────┘
                            ▼
   ┌──────────────────────────────────────────────────────┐
   │  ATO — Build / dispatch / war-room / RBAC / deploy   │
   │  (our lane: dev workflow + team control plane)       │
   └────────────────────────┬─────────────────────────────┘
                            ▼
   ┌──────────────────────────────────────────────────────┐
   │  HELICONE / PORTKEY — Gateway, caching, fallback     │
   │  (their lane: production routing layer)              │
   └────────────────────────┬─────────────────────────────┘
                            ▼
   ┌──────────────────────────────────────────────────────┐
   │  LLM PROVIDER (Anthropic / OpenAI / Google / etc)    │
   └────────────────────────┬─────────────────────────────┘
                            │
                            ▼
   ┌──────────────────────────────────────────────────────┐
   │  LANGFUSE / LANGSMITH / BRAINTRUST — Observability   │
   │  (their lane: post-hoc tracing + eval workbench)     │
   └──────────────────────────────────────────────────────┘
```

ATO sits ABOVE the gateway, the LLM, and the observability layer.
Customers can use ATO **AND** Langfuse **AND** Helicone — and most
mature teams will. We are not trying to take Langfuse's revenue.

### Where each tool genuinely lives

| Layer | Tool | What they own |
|-------|------|---------------|
| Dev workflow / team control plane | **ATO** | Build agents, war-room, RBAC, deploy |
| Gateway / routing | **Helicone / PortKey** | Cache, route, fallback |
| Provider | **Anthropic / OpenAI / Google** | Model inference |
| Observability | **Langfuse / LangSmith** | Trace, replay, cost-per-trace |
| Eval workbench | **Braintrust / Promptfoo** | Dataset versioning + eval matrices |
| LangChain agents | **LangChain** | DIY framework primitives |
| Enterprise MS-stack | **Copilot Studio** | M365-locked low-code agents |
| Enterprise GCP-stack | **Vertex Agents** | GCP-locked agents |
| Anthropic-native hosted | **Anthropic Agents** | Single-vendor hosted Claude agents |

### Multi-dimensional scoring (only on ATO's actual dimensions)

We grade ourselves on dimensions WE chose to compete on. We do
NOT grade Langfuse / Helicone on dimensions they explicitly own —
that would be unfair and irrelevant (a customer who needs
production trace replay buys Langfuse and we recommend them).

| Tool | Dev workflow (build → test → war-room) | Team RBAC / rooms | Multi-LLM at decision time | MCP-native | Anti-injection floor | Local-first |
|------|----------------------------------------|--------------------|------------------------------|------------|----------------------|-------------|
| **ATO** | **5** | **5** (P3 ships Phase 2) | **5** | **5** | **4** (P0 ships Phase 1) | **5** |
| Claude Code + MCP | 2 | 1 | 1 | 5 | 1 | 4 |
| Anthropic Agents | 3 | 2 | 1 | 5 | 4 | 1 |
| Promptfoo | 2 (eval-time) | 1 | 4 (eval-time) | 1 | 4 (red-team) | 5 |
| LangChain (DIY) | 3 (write it yourself) | 0 | 5 | 3 | 0 | 5 |
| Copilot Studio | 3 | 5 | 1 | 1 | 3 | 1 |
| Vertex Agents | 3 | 5 | 1 | 1 | 3 | 1 |

**Excluded from this matrix on purpose**:
- Langfuse / LangSmith — different category (observability)
- Helicone / PortKey — different category (gateway)
- Braintrust — different category (eval workbench)

When a prospect mentions any of those, the answer is **"use them
together with ATO, not instead of"**. See §8 objection handling.

---

## 4. Where ATO genuinely wins — the 6 we sell

1. **Multi-LLM at decision time, in parallel.** War-rooms are unique.
   Braintrust does eval-time comparison; PortKey does runtime routing.
   *Nobody* runs N models in parallel for the same decision.
2. **Local-first by architecture.** Data on the Mac, BYOK, audit in
   SQLite. The compliance buyer's only viable workflow tool.
3. **MCP-native from day one.** Anthropic Agents support MCP too,
   but they lock you to Claude. ATO is the *MCP-native + multi-vendor*
   intersection — nobody else.
4. **Dev workflow control plane** — we intervene before the model
   runs (permissions, room ACLs, content policy). Observability tools
   (Langfuse, LangSmith) record after the fact; we're a different
   category that runs ABOVE theirs, not against them.
5. **Build → test → compare → deploy in one tool.** Every other tool
   is one slice.
6. **BYOM (Bring Your Own Model).** Customer fine-tunes at the
   provider (OpenAI / Anthropic / Together / Fireworks), gets a
   custom model ID, points ATO at it. War-rooms, audit, deploy all
   work with custom models identically to base models. We don't
   compete with the providers on training; we layer on top of
   whatever the customer trained.

---

## 5. Where ATO honestly loses

1. **No SaaS-first onboarding.** Helicone gets you live in 5 minutes
   without installing anything. ATO needs `brew install`.
2. **Younger / smaller ecosystem.** Langfuse has 50+ SDK integrations
   and thousands of customers. ATO doesn't.
3. **No enterprise compliance certifications yet** (SOC 2, ISO).
   Anyone who needs that today goes to Copilot Studio or Vertex.

**NOT a loss (intentional architecture)**:

- **Production observability** — Langfuse / LangSmith own this. We
  output traces to them; we don't compete.
- **Production gateway / caching** — Helicone / PortKey own this.
  ATO points at their endpoint as the LLM URL; we don't compete.
- **Fine-tuning** — providers own this. Customer trains there,
  points ATO at the custom model ID. BYOM works.
- **Vector DB / RAG** — customer brings via MCP. MCP is the right
  abstraction; we don't lock anyone to a specific vector store.

---

## 6. The competitor we're most worried about

**Anthropic itself, if they launch "Claude Teams."**

Anthropic owns the MCP spec. If they ship a hosted agent platform
with team RBAC + content policy + workspace audit, they'd compete
directly on 3 of our 5 wins (MCP-native, anti-injection,
RBAC) — but they CAN'T compete on multi-LLM (their whole business
is Claude). That's our durable wedge.

Strategic response if they ship it: lean harder on multi-LLM in
positioning. Their lock-in is our differentiator.

---

## 7. The competitor that's closest in spirit

**Langfuse + a homemade agent layer.**

A team that runs Langfuse for observability + writes a custom RBAC +
custom agent dispatcher is effectively rolling their own ATO. The
opportunity: catch them BEFORE they write the homemade layer.
Office-hours seat's insight: that's exactly the moment Daniel is
in. He's about to write 1,000 LOC of permissions code. Get to him
first.

---

## 8. Sales objection handling

| Objection | Truthful response | Aggressive response (if needed) |
|-----------|-------------------|----------------------------------|
| "Langfuse already does observability" | "Yes, and we'd recommend Langfuse for trace UI if that's all you need. ATO is the *layer in front of* observability — we intervene, they record. Different category." | "If you're comparing observability tools, Langfuse wins. If you're comparing control planes, we're alone." |
| "Helicone is cheaper at $20/seat" | "Helicone is a gateway. ATO is a control plane + agent builder + deploy pipeline. You'd buy both, not one or the other." | "Per-dollar-of-value, ATO ships 5 categories of product Helicone doesn't." |
| "We're on LangChain — LangSmith is native" | "LangSmith only wins if you're going deeper into LangChain. ATO works with raw SDKs, LangChain, LlamaIndex, anything — and adds the team layer LangSmith doesn't have." | "LangChain lock-in is a 12-month re-platform if you grow out of it. ATO has zero lock-in." |
| "We use Anthropic exclusively, why multi-LLM?" | "Today maybe. Six months from now you'll want to A/B Gemini on a 10x cheaper rate, or compare GPT-5 for a specific question type. ATO makes that a 30-second test, not a re-platforming." | "Single-vendor strategy ages poorly. Buy optionality." |
| "We need SaaS, not local-first" | "The deployed bundle ships your agent to Vercel / Lambda / Anthropic's hosted infra in 1 command. Build pipeline is local; runtime is yours." | "Local-first is a feature for your compliance team. SaaS-only is a bug for theirs." |
| "We need SOC 2" | "Phase 3 enterprise tier ships SSO + audit retention; SOC 2 in flight. For Q1 contracts that's not blocking; for Q4 contracts it will be. Let's pilot now and certify together." | (Don't be aggressive; this is a real gap.) |

---

## 9. One-slide summary for the deck

**ATO sits where everyone else doesn't:**

```
                  Build agents
                  ┌──────────┐
                  │ LangChain │
                  │  Vercel   │ ATO
                  │ Anthropic │ ←—— here
                  └──────────┘
                       │
                  ┌──────────┐
                  │  Run     │
                  │   ATO    │ ←—— and here
                  │ ────────  │
                  │  Helicone │
                  │  PortKey  │
                  └──────────┘
                       │
                  ┌──────────┐
                  │ Observe  │
                  │  Langfuse│
                  │  LangSmith│
                  │  Braintrust│
                  └──────────┘
```

ATO is the ONLY tool in the **build + run** intersection with
**multi-LLM + RBAC + MCP-native + local-first** all true.

---

## 10. Update schedule

- **Re-verify all pricing** before quoting in any deal (pricing pages
  move quarterly)
- **Re-rank the matrix** when a major competitor ships a meaningful
  release (e.g., if Langfuse ships an agent builder, P0 of this doc
  changes)
- **Add new competitors** as the category matures (Replit Agents,
  Vercel AI SDK upgrades, etc.)
