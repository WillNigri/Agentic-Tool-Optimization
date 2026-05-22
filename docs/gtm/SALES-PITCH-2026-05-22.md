# ATO Sales Pitch — 2026-05-22

**Status:** PRIVATE. Use as the script for Bruno / Eduardo / Daniel
DMs, the LinkedIn thread reply, and the 90-second Loom voiceover.
Pulls from
[GTM-STRATEGY-2026-05-22](./GTM-STRATEGY-2026-05-22.md) §10-11.

---

## The one-sentence pitch

> **"ATO is the war room for building agents and the guardrails for
> running them — multi-LLM, MCP-native, local-first, deploy-anywhere."**

If you say one sentence, say that one. If you have 30 seconds, expand:

> **"Your team built an MCP. Now five people need to use it — with
> rooms, audit, anti-injection, and the freedom to A/B different LLMs
> without rewiring. ATO is the layer that gives you all of that on
> top of what you already built."**

---

## The 90-second Loom script (record this; everything depends on it)

**Setting**: Will at desk. Screen capture. Mac native voice.

### Shot 1 — Hook (0:00 – 0:08)
*"I built a Postgres MCP for my team's customer data. It works, but
five people now need access — and I want every query audited, every
output safe, every model on the table. Here's how ATO does it."*

**Visual**: ATO desktop UI open. New agent dialog. MCP picker shows
"acme-customer-db" highlighted.

### Shot 2 — Dispatch (0:08 – 0:25)
*"One command dispatches against the MCP. Same as Claude Code — but
logged with my user ID, the exact tokens, and the cost split per
runtime."*

**Visual**: Terminal. `ato dispatch my-agent "what are our top 3
churn signals in the last 30 days?"` — output streams. Cut to the
audit log row appearing in the desktop UI in real time.

### Shot 3 — War-room (0:25 – 0:42)
*"Now the same question, in parallel, to three models — Claude,
Gemini, GPT. Side by side. Pick the right answer. No competitor
does this."*

**Visual**: Terminal: `ato dispatch claude codex google "<same
prompt>" --war-room-id $(uuidgen)`. Cut to desktop UI — war-room
card with 3 responses streaming in parallel, divergent answers
highlighted.

### Shot 4 — Team scope (0:42 – 1:05)
*"Drop the agent in a room. Invite my teammate. Every query they
run shows up in the audit feed — who, what, when, which model.
The room can also restrict which agents are visible — your sales
PM doesn't see your finance agents."*

**Visual**: Desktop UI. Drag agent into "Customer Research" room.
Invite dialog. Cut to a different Mac (or simulated split-screen)
where teammate dispatches the same agent. Audit feed scrolls with
both users' queries.

### Shot 5 — Deploy (1:05 – 1:25)
*"And when I'm ready to ship to end users — one command exports the
agent as a Vercel function. The deployed bundle includes the same
safety floor: tool outputs treated as data, user identity passed
through to the MCP, audit phoned home to my workspace."*

**Visual**: Terminal: `ato deploy my-agent --target vercel`.
Output: deployment URL. Browser opens the URL — working chat
interface. Send a prompt-injection attempt — bundle refuses it.

### Shot 6 — Close (1:25 – 1:30)
*"Local-first, multi-LLM, MCP-native. The war room for building.
The guardrails for running. ATO."*

**Visual**: ATO logo. URL: ato.dev. CTA: "Free OSS install. Team
trial for shared workspaces."

---

## The LinkedIn thread reply (Eduardo's post)

**Channel**: Public comment under Eduardo's data-lake-MCP post.
**Goal**: Social proof for Bruno + Daniel + the other 37
commenters. NOT a sales pitch.

> Eduardo, post excelente — o "AI desbloqueou" tá realmente
> acontecendo nesse padrão. Curto trabalhando com a camada que vem
> DEPOIS de ter o MCP: quando o time quer perguntar pro claude vs
> gemini vs gpt em paralelo e cada PM precisa do rastro do que
> consultou. Construí o ATO (open-source, MIT, local-first)
> exatamente pra isso — fica EM CIMA do MCP de vocês, sem
> substituir.
>
> Loom de 90 segundos: [link]
>
> Se topar dar uma olhada de como conectaria no seu lake, me chama
> no DM.

**Length**: under 80 words. Portuguese. Loom link is the call-to-action.
Says "EM CIMA do MCP" explicitly so Eduardo knows we're complementary,
not competing.

---

## The Bruno DM (24h after the Eduardo reply)

**Channel**: LinkedIn DM. **Goal**: open a conversation that closes
the loop on his earlier polite decline + positions ATO as relevant
to Cumbuca's compliance pressure.

> Fala Bruno! Vi sua pergunta pro Eduardo sobre Granola vs lake + MCP
> próprio — exatamente o tipo de decisão que motivou o que to
> construindo nos últimos meses.
>
> Sigo no advisory e na holding com o Daniel Peres Chor, mas em
> paralelo desenvolvi o ATO: uma camada local-first que fica EM
> CIMA de MCPs como o que o Eduardo descreveu. Multi-LLM
> (claude/codex/gemini/grok rodam a mesma pergunta em paralelo),
> war-rooms pra decisão, audit por usuário, RBAC por sala —
> exatamente o que um time fintech precisa quando compliance
> começa a perguntar quem viu o quê.
>
> Não substitui nada que vocês já tenham (Helicone, Langfuse,
> seu lake) — fica em cima. Loom de 90 segundos:
> [link]
>
> Se quiser dar uma olhada eu monto setup em 15min em cima do que
> vocês já têm. Mesmo que não faça sentido pra Cumbuca agora, vc
> tá no perfil exato de quem ia me dar feedback útil. Abs

**Length**: 4 paragraphs. Specifically calls out "compliance
começa a perguntar quem viu o quê" — his exact pain. Explicit
"não substitui nada" — reinforces complementary stance.

---

## The Daniel DM (48h after Eduardo reply)

**Channel**: LinkedIn DM. **Goal**: open a design-partner
conversation. Daniel is greenfield; we can co-create the spec
with him.

> Daniel, vi sua pergunta sobre travas de restrição de usuário no
> thread do Eduardo. To construindo exatamente essa camada — ATO,
> open-source, MIT.
>
> Como funciona: você define salas, convida pessoas, atribui
> agentes específicos por sala. Cada query carrega user-id +
> workspace-id + room-id como header pro seu MCP, então o seu
> lake aplica o ACL no lado dele. Audit por usuário, denial UI,
> anti-injection no input.
>
> Loom de 90 segundos: [link]
>
> Pergunta honesta: das features que você estava prestes a
> construir você mesmo, quais são as 3 mais críticas? Se ATO
> resolve, te mando setup hoje. Se não, é exatamente esse tipo
> de feedback que me ajuda a priorizar.

**Length**: 3 paragraphs. Explicit ask for his 3 most-critical
features = makes him a design partner. Loom link + setup offer.

---

## Conversation flow when one of them responds

### If Bruno says yes
1. 15-min Zoom. Screen share. Walk through his Cumbuca use case.
2. Set up Team-trial account on the call.
3. Send him a private Postgres MCP recipe specific to his stack.
4. Follow up in 7 days to check usage.
5. If usage ≥ 50 queries → schedule pricing conversation.

### If Eduardo says yes
1. Send him the Loom + a Github repo with MCP examples wired to ATO.
2. Offer to drop in for 30 min to wire Purple's lake-MCP into ATO live.
3. The pitch: "your team adopts in 1 week, you get rooms + audit
   without writing code."
4. Convert via Purple Metrics being a public case study + logo.

### If Daniel says yes
1. Treat as design partner. Weekly call for 30 days.
2. Build the features he names FIRST (within Phase 1 scope).
3. He becomes the named case study for "saved us 3 months of
   permission code."
4. Convert via BlueForecast paying Team tier + writing a blog post.

### If 0/3 respond in 14 days
Per office-hours falsifier: drop the security narrative; refocus on
the GUI-for-agents wedge that v1.3.0 just shipped. The MCP-builder
buyer hypothesis was wrong. Re-grade in 30 days.

---

## Universal "don't"s

| Don't | Why |
|-------|-----|
| Say "ATO replaces Langfuse" | Wrong. We're complementary. Use stack diagram from competitive doc §3. |
| Say "ATO is a Langfuse alternative" | Same reason. |
| Lead with security/SOC 2/compliance | Reach the door first via the workflow story. Compliance is a CLOSE objection, not an OPEN. |
| Drop a github URL in the first message | LinkedIn de-prioritizes posts with links. Lead with the Loom, send the repo on response. |
| Pitch the Team tier in DMs | Free OSS first. Team tier surfaces once they're using it. |
| Bury the multi-LLM win | This is the single durable differentiator. Lead with it visually in the Loom. |
| Use "AI agent" without context | Specific > generic. "Multi-LLM customer-research agent" beats "AI agent." |
| Mention "fine-tuning" without "BYOM" | Customers worry about lock-in. The BYOM framing reassures. |

---

## Universal "do"s

| Do | Why |
|-----|-----|
| Reference their specific MCP / data lake | They built it; acknowledge the work. |
| Frame as "above" their stack, not "instead of" | Complementary positioning is locked. |
| Quote their language back ("trava de restrição") | Shows you read the thread. |
| Make the Loom Portuguese-narrated for LATAM | Native language closes faster. |
| Offer a 15-min wire-up call | Lowers the bar from "buy" to "see if it works for me." |
| Keep messages under 200 words | LinkedIn cuts at ~250; readers bail at ~200. |
| Drop "Daniel Chor / W3Block / advisory" once in Bruno DM | Establishes credibility without listing résumé. |

---

## Pricing conversation when it comes up

(It will come up earlier than you expect. Be ready.)

**If asked "how much does it cost":**
> "OSS is free forever — local agent dispatch, multi-LLM, audit on
> your Mac. Team tier is $25/seat/month for shared workspaces,
> room ACLs, and the cloud audit dashboard. That includes 10,000
> classifier-policy calls per seat per month for content
> guardrails; overage runs $0.50 per thousand calls beyond that.
> Annual prepay knocks 15% off."

**If they push back on classifier overage:**
> "We bundle the overage to keep the math honest. Flat-rate
> 'unlimited classifier' sounds great until the team's traffic 10x's
> and we're losing money on every call. The bundle covers 95% of
> teams without ever touching the overage line. If you're in the
> 5% that does, you're already paying us $5K+/year and we'll talk."

**If they ask for an annual contract:**
> "Annual prepay: 15% off the monthly price. Multi-year locks at
> 20%, contract terms negotiable. Send your procurement docs and
> we'll turn it around in 48 hours."

**If they want to negotiate seat pricing:**
> "Seat price is firm — it's the unit we built the unit economics
> around. What's flexible: number of bundled classifier calls,
> volume discount on deployed-bundle SKU above 5 bundles, custom
> SSO + audit retention in Enterprise. Tell me which lever
> matters and we'll work the deal there."

---

## After the demo: the 4 follow-up emails (sequenced)

### Email 1 — Day 0 (within 2h of demo)
**Subject**: ATO follow-up: setup script + your private MCP recipe

> Hey [name], thanks for the time today. As promised:
>
> 1. Setup script: [private gist link]
> 2. Your MCP recipe: [link, generated from their stack]
> 3. Team-trial account: [trial code, 14 days]
>
> Two questions for you when you've had a chance to poke around:
> - Did the Postgres ACL flow read cleanly?
> - Which of your team members would be the second user we set up?
>
> I'll check in Friday.
> Will

### Email 2 — Day 3
**Subject**: Quick check — are the room ACLs feeling right?

> [name], short one — how's the room setup feel? Specifically:
> [insert one specific thing relevant to their stack].
>
> Two things you might not have tried yet:
> - `ato war-room` — runs claude+gemini+gpt in parallel on a single
>   prompt. Try it on a real product question.
> - `ato deploy --target vercel` — exports a working agent in 30
>   seconds.
>
> Will

### Email 3 — Day 7
**Subject**: Quick pricing question

> [name], one week in — usage looks like [X] dispatches across [Y]
> users. At that rate the Team tier is the right shape. Want to
> get on a 15-min call to walk through the pricing and lock in
> annual? Got a slot open Thursday 3pm.
>
> Will

### Email 4 — Day 14
**Subject**: Trial wraps Friday — make it official?

> [name], trial ends Friday. Two paths:
> - Convert to Team @ $25/seat/mo, 15% annual discount = $20.40/seat.
>   I'll send the Stripe link.
> - Stick with free OSS — your local dispatches keep working, just
>   no cloud workspace.
>
> Either is fine. Which?
> Will

---

## The "no" responses you should expect

| "No" reason | Counter |
|-------------|---------|
| "Not ready for shared agents yet" | "Free OSS forever. Come back when you are. Mind if I check in Q3?" |
| "Need SOC 2" | "Phase 3 enterprise — flag this and I'll loop you in when we start the cert process. We can be a design partner there too." |
| "Too expensive at $25/seat" | "How many seats? If ≤3 you can stay on Free OSS — multi-user just means cloud sync, which you might not need. Let's check the actual seat math." |
| "Already using Langfuse" | "Keep using Langfuse. ATO sits ABOVE Langfuse — output your traces to Langfuse, intervene at decision time with ATO. Use them together." |
| "Already using LangChain" | "Same answer — LangChain is your runtime; ATO is your build + team layer. They compose." |
| "We'll just build it" | "Most teams that say that take 3 months to ship what we already have. If you're committed to the build, can I check in after you've scoped it? Sometimes the 'won't fit our needs' realization lands two weeks in." |

---

## The one falsifier you check every Monday morning

```
Question: did anyone convert to Team this week?

  YES → keep going, the wedge is real
  NO and we've been at it < 14 days → keep going, too early
  NO and we've been at it ≥ 14 days → office-hours falsifier kicks in
                                        drop the security narrative
                                        refocus on the GUI wedge
                                        post-mortem in memory
```

Don't argue with the falsifier. The whole G-stack signed off on it.
