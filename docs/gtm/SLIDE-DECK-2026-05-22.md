# ATO Slide Deck — 2026-05-22

**Status:** PRIVATE. Render via Marp (`npx @marp-team/marp-cli`)
or copy slides into Google Slides / Pitch.com. 12 slides + 2
appendix.

**Format**: Each `---` is a slide break. Speaker notes in
`> speaker:` blocks. Render with the dark theme to match ATO's
brand (#0a0a0f bg, #00FFB2 accent).

---
marp: true
theme: default
class: invert
paginate: true
backgroundColor: '#0a0a0f'
color: '#e5e7eb'
---

<!-- _class: lead -->

# ATO

## The war room for building agents.
## The guardrails for running them.

<br>

Multi-LLM · MCP-native · Local-first · Deploy-anywhere

<br><br>

Will Nigri · 2026-05-22

> speaker: Open with the one-sentence pitch. Don't elaborate. The
> tagline does the work. Take 3 seconds of silence after reading it.

---

## The pattern we keep seeing

> *"Tem alguma trava de restrição de usuário? ou tipo, todo mundo
> pode ver todas as conversas?"*
>
> — Daniel Lestinge, BlueForecast
> (in reply to a CTO's MCP-data-lake post, 2026-05-22)

CTOs ship a custom MCP wired to their data lake.
The team uses it heavily.
Then they hit the wall — **who can see what?**

> speaker: Land this quote in the room. It's the canonical "we
> built it, now we're scared" moment. Every CTO building with MCPs
> in 2026 will recognize this question.

---

## What teams do today

```
1. Build their own MCP server (Python/TS)
2. Configure env vars on every team Mac
3. Hope auth works (everyone shares one API key)
4. Bill arrives — no per-user breakdown
5. Bolt on Langfuse AFTER something breaks
6. RBAC? Write it from scratch.
```

**Time to first agent**: days–weeks
**Time to RBAC + audit**: months of custom

> speaker: This is the pain. Don't undersell it. Most teams in this
> spot have already written 200-1000 LOC of permission code that
> we ship by default.

---

## What teams do with ATO

```
brew install ato                                # 30 sec
ato login                                        # 1 min
ato agent create my-agent --runtime claude       # 30 sec
ato mcp add purple-lake --url https://...        # 30 sec
ato dispatch my-agent "ask the question"         # works
```

**3 minutes** from `brew install` to working agent.
**5 minutes** to team-shared + audited.
**6 minutes** to production deploy.

> speaker: The arc itself is the demo. If you only have 30 seconds
> with someone, walk them through these 6 lines on a whiteboard.

---

## Where ATO lives in the stack

```
┌──────────────────────────────────────────────────┐
│  END USER (or developer asking a question)       │
└─────────────────────┬────────────────────────────┘
                      ▼
┌──────────────────────────────────────────────────┐
│ ★ ATO — Build / war-room / RBAC / deploy         │
│   (our lane: dev workflow + team control plane)  │
└─────────────────────┬────────────────────────────┘
                      ▼
┌──────────────────────────────────────────────────┐
│  Helicone / PortKey — Gateway, caching           │
└─────────────────────┬────────────────────────────┘
                      ▼
┌──────────────────────────────────────────────────┐
│  Anthropic / OpenAI / Google + customer fine-tunes│
└─────────────────────┬────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────┐
│  Langfuse / LangSmith — Observability            │
└──────────────────────────────────────────────────┘
```

**Use them all. We're complementary, not competitive.**

> speaker: This is the critical reframe. We are NOT replacing
> Langfuse. We are NOT replacing Helicone. We sit ABOVE both. Most
> mature teams will use all three. The pitch line: "use ATO to
> build and ship; use Langfuse to observe in production."

---

## The 5 things we genuinely do that no one else does

1. **Multi-LLM at decision time, in parallel.**
   War-rooms ask 3+ models the same question simultaneously.

2. **Local-first by architecture.**
   Data on the Mac. BYOK. The compliance-buyer's only option.

3. **MCP-native from day one.**
   Not a connector — the *primary* data path.

4. **Dev-workflow control plane.**
   Intervene before the model runs (permissions, room ACLs, content policy).

5. **Build → test → compare → deploy in one tool.**
   Every competitor is one slice.

> speaker: Don't list these mechanically. Pick the ONE that
> resonates with this prospect (Bruno → 4, Eduardo → 1, Daniel → 5)
> and unpack it. Mention the others as bonus.

---

## The war-room (the unique demo)

![war-room screenshot placeholder]

```
$ ato dispatch claude codex google "should we ship feature X this sprint?" \
    --war-room-id $(uuidgen)

  ⏱ claude  responds in 18s  — recommend YES, with caveats
  ⏱ codex   responds in 22s  — recommend NO, scope creep risk
  ⏱ google  responds in 14s  — recommend YES, focus on smallest win
```

**Three LLMs. Same question. Side-by-side answer.**
**You pick the right one.**

> speaker: This is the most demoable single feature in the product.
> Open a real terminal. Type a real question. Let them watch three
> answers stream. They will not have seen this anywhere else.

---

## The 90-second product walkthrough

> [Insert embedded Loom here]

If they only watch one thing, this is it.

> speaker: If you're presenting live, pause the slide and play the
> Loom. Don't narrate it; let it do the work. If you're sending
> the deck, this is the slide that needs the most polish.

---

## Pricing — built for the 5-50p team

| | OSS Free | Team | Enterprise |
|---|----------|------|------------|
| Local dispatch + multi-LLM | ✅ | ✅ | ✅ |
| MCP integration | ✅ | ✅ | ✅ |
| Anti-injection floor (P0) | ✅ | ✅ | ✅ |
| Identity passthrough (P2) | ✅ | ✅ | ✅ |
| **Cloud workspace + room ACLs** | — | ✅ | ✅ |
| **Audit + denial UI** | — | ✅ | ✅ |
| **Classifier-policy enforcement** | — | 10k/seat/mo bundled | unlimited |
| Deployed bundles | — | up to 5 | unlimited |
| SSO + audit retention | — | — | ✅ |
| Annual SLA | — | — | ✅ |
| **Price** | **$0** | **$25/seat/mo** | **custom** |

**BYOK + BYOM** — your API keys, your fine-tuned models, your bill.
We never hold either.

> speaker: This slide is THE objection-handling slide. Lead with
> "OSS Free forever" so the line "$25/seat" doesn't sound expensive
> by comparison. The classifier overage is $0.50/1k calls — only
> mention if asked.

---

## How we sell to each archetype

### Bruno (Cumbuca, fintech, compliance pressure)
> *"The control plane your CISO would let you ship.*
> *Audit, RBAC, anti-injection — built-in, not bolted-on."*

### Eduardo (Purple Metrics, CTO with built lake + heavy use)
> *"You built the lake. ATO is where your team uses it — with*
> *rooms, audit, and the freedom to A/B different LLMs without*
> *rewiring."*

### Daniel (BlueForecast, asking the right questions)
> *"Stop building the same chrome around every internal agent.*
> *ATO is the team chrome. Your 1,000 LOC of permission code is*
> *our default."*

> speaker: Don't read these. Use whichever applies to the room. If
> presenting to a mixed audience, frame each as "for the X persona
> in your team."

---

## The 14-day proof point we'll commit to

If we put 3 named teams (Cumbuca, Purple Metrics, BlueForecast)
through the 90-second Loom + setup script:

```
≥1 of 3 activates the Team trial in 14 days
  → product-market signal is real
  → ship Phase 2 (workspace, classifier, denial UI)

0 of 3 activate in 14 days
  → re-grade. The MCP-builder isn't the buyer.
  → drop the security narrative, refocus on GUI wedge
```

> speaker: This is the office-hours falsifier from the war-room.
> Naming a real falsifier on a sales slide builds trust — they
> see you're not in delusion mode. Skip this slide for cold
> prospects; show it to design partners and investors.

---

## Why now

- **MCP went from spec to production in 9 months.** Anthropic
  shipped it; the community adopted it.
- **Every team with a custom MCP is hitting the same wall.**
  Daniel's question is canonical.
- **No competitor is shipping a control plane.** Langfuse observes;
  Helicone gateways; LangChain frameworks. The intervention layer
  is empty.
- **Multi-LLM is now table stakes.** GPT-5, Claude 4.7, Gemini 2.5,
  Grok all live in 2026. Single-vendor strategies age poorly.

> speaker: The "why now" deserves emphasis. Without it, this looks
> like another agent platform. With it, the timing argument makes
> the wedge feel inevitable.

---

## Get started in 3 minutes

```bash
brew install ato
ato login
ato agent create my-first-agent
```

OSS: github.com/WillNigri/Agentic-Tool-Optimization
Docs: ato.dev
Team trial: ato.dev/trial

**will@nigri.io · LinkedIn @ Guilherme Nigri**

> speaker: End on action, not summary. Make the install command
> the largest text on the slide. The CTA is "try it," not "buy it."

---

## Appendix A — The stack we're complementary with

| Layer | Who owns it | Our stance |
|-------|-------------|------------|
| Dev workflow / control plane | **ATO** | Our lane |
| Build / war-rooms / RBAC | **ATO** | Our lane |
| Gateway / routing | Helicone, PortKey | Integrate (point at their endpoint) |
| LLM inference | Anthropic, OpenAI, Google, fine-tuned | BYOK + BYOM, never hold the bill |
| Observability | Langfuse, LangSmith | Integrate (output traces to them) |
| Eval workbench | Braintrust, Promptfoo | Integrate (export at design time) |
| Enterprise MS-stack | Copilot Studio | Different ICP |
| Enterprise GCP-stack | Vertex Agents | Different ICP |
| Anthropic-native hosted | Anthropic Agents | We absorb (multi-LLM beats lock-in) |
| DIY framework | LangChain | We absorb (graduation from DIY) |

> speaker: This slide exists for the prospect who asks "but what
> about <tool>?" Walk them through the row that matters. Most of
> the time the answer is "use both."

---

## Appendix B — Roadmap shape

**Phase 1 (shipping next 2 weeks, OSS)**
- P0 — Tool-result sanitization (UNTRUSTED_INPUT wrappers)
- P2 — Identity passthrough headers + MCP-author guide
- Comparison docs + 90-sec Loom

**Phase 2 (ato-cloud, after 14-day falsifier passes)**
- Workspace + room ACLs (P3)
- Classifier-enforced content policy (P1.b)
- Denial-event UI (P4)
- Team billing + trial flow

**Phase 3 (Enterprise, when first paying logo asks)**
- SSO via WorkOS / Okta
- Audit retention > 1 year
- On-prem audit option
- Contractual SLA

> speaker: Don't promise Phase 2/3 dates. Phase 1 is committed;
> Phase 2 ships when the falsifier validates demand. Phase 3 is
> "we'll co-design it with you" for enterprise prospects.

---

*End of deck. 12 slides + 2 appendix.*

## Render instructions

```bash
# Install Marp
npm install -g @marp-team/marp-cli

# Render to HTML
marp SLIDE-DECK-2026-05-22.md -o SLIDE-DECK-2026-05-22.html

# Render to PDF
marp SLIDE-DECK-2026-05-22.md --pdf -o SLIDE-DECK-2026-05-22.pdf

# Render to PowerPoint
marp SLIDE-DECK-2026-05-22.md --pptx -o SLIDE-DECK-2026-05-22.pptx
```
