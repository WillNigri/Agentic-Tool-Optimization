# Where ATO fits in your AI agent stack

Most "AI agent" tools fall into one of these buckets: agent
framework (LangChain), observability (Langfuse, LangSmith,
Braintrust), gateway (Helicone, PortKey), or hosted-agent
platform (Anthropic Agents, Microsoft Copilot Studio, Google
Vertex Agent Builder).

**ATO is none of those.** ATO is the *dev workflow + team
control plane* that sits above all of them.

This page exists to show you where ATO fits in your existing
stack — and which tools you should keep using *together with*
ATO, not instead of.

---

## The stack

```
┌──────────────────────────────────────────────────────┐
│  END USER (or developer asking a question)           │
└────────────────────────┬─────────────────────────────┘
                         ▼
┌──────────────────────────────────────────────────────┐
│  ★ ATO — Build / dispatch / war-room / RBAC / deploy │
│  (this repo)                                         │
└────────────────────────┬─────────────────────────────┘
                         ▼
┌──────────────────────────────────────────────────────┐
│  Helicone / PortKey — Gateway, caching, fallback     │
│  (your existing tools — keep them)                   │
└────────────────────────┬─────────────────────────────┘
                         ▼
┌──────────────────────────────────────────────────────┐
│  Anthropic / OpenAI / Google / Mistral / Together    │
│  (LLM inference — you pay them directly, BYOK)       │
└────────────────────────┬─────────────────────────────┘
                         │
                         ▼
┌──────────────────────────────────────────────────────┐
│  Langfuse / LangSmith / Braintrust — Observability   │
│  (your existing tools — keep them)                   │
└──────────────────────────────────────────────────────┘
```

**ATO sits above the gateway, above the LLM, above the
observability layer.** Most mature teams will use ATO **and**
their gateway **and** their observability tool. We're a control
plane, not a replacement.

---

## What each layer does

| Layer | Who owns it | What you'd use it for | ATO's relationship |
|-------|------------|----------------------|---------------------|
| **Dev workflow / team control plane** | **ATO** | Build agents, war-room multi-LLM decisions, set room ACLs, audit dispatches, deploy to anywhere | This is what we ship |
| **Gateway / routing / caching** | Helicone, PortKey | Cache repeat LLM calls, route fallback chains, single auth endpoint | Point ATO at their endpoint as the LLM URL |
| **LLM inference** | Anthropic, OpenAI, Google, Together, Fireworks, Mistral | The actual model call | BYOK — you pay them directly; ATO never holds the bill |
| **Fine-tuning / training** | Anthropic, OpenAI, Together, Fireworks | Custom models trained on your data | BYOM — point ATO at your custom model ID; war-rooms, audit, deploy all work identically to base models |
| **Production observability** | Langfuse, LangSmith, Braintrust | Trace replay, cost-per-trace, dataset eval | Output traces to them; we never compete on observability |
| **Eval workbench** | Promptfoo, Braintrust | Dataset-driven prompt eval at design time | Export at design time; complementary |
| **DIY framework** | LangChain, LlamaIndex, Vercel AI SDK | Write your own agent orchestration in code | ATO absorbs this — you graduate from DIY framework to ATO when you need team coordination |
| **Enterprise MS-stack agents** | Microsoft Copilot Studio | M365-locked low-code agents for Office365 shops | Different ICP — ATO targets 15-50p startups, not enterprises |
| **Enterprise GCP-stack agents** | Google Vertex Agent Builder | GCP-locked agents on Vertex AI | Same as above |
| **Anthropic-native hosted agents** | Anthropic Console Agents | Native Claude agents hosted by Anthropic | Use Anthropic's surface for Claude-only; use ATO for multi-LLM |

---

## What ATO genuinely does that no one else does

1. **Multi-LLM at decision time, in parallel.** ATO's war-rooms
   ask Claude + GPT + Gemini + Grok the same question
   simultaneously. You pick the right answer per question. No
   competitor offers this — Braintrust does eval-time
   comparison; PortKey does runtime routing; nobody runs N
   models in parallel for the same decision.
2. **Local-first by architecture.** All data on the Mac. BYOK
   API keys. Audit log in local SQLite. The compliance buyer's
   only option in this list — everyone else is cloud-only.
3. **MCP-native from day one.** Not a connector library; the
   *primary* data path. Anthropic's MCP is the spec everyone's
   converging on; ATO bets the company on it.
4. **Dev workflow control plane.** We intervene *before* the
   model runs (permissions, room ACLs, content policy, identity
   passthrough). Observability tools record after the fact;
   we're a different category that runs above theirs.
5. **Build → test → compare → deploy in one tool.** Every
   competitor is one slice (build OR observe OR deploy). ATO is
   the full lifecycle.
6. **BYOM (Bring Your Own Model).** Customer fine-tunes at the
   provider (OpenAI, Anthropic, Together, Fireworks), gets a
   custom model ID, points ATO at it. War-rooms, audit, deploy
   all work with custom models identically to base models. We
   don't compete with the providers on training.

---

## What ATO honestly doesn't do (and doesn't try to)

- ❌ **Foundation model training** — Anthropic, OpenAI, Google do this
- ❌ **Fine-tuning service** — same providers + Together, Fireworks
- ❌ **LLM inference hosting** — providers run the models
- ❌ **Production trace observability** — Langfuse / LangSmith own this
- ❌ **Gateway / caching** — Helicone / PortKey own this
- ❌ **Vector DB / RAG storage** — bring your own via MCP

If you need any of those, use the right tool. ATO orchestrates
them; it doesn't replace them.

---

## How to use ATO together with your existing stack

### With Langfuse (production observability)

```
ATO dispatches your agent → LLM provider responds → ATO logs
locally → Langfuse SDK ALSO captures the trace → both happy
```

You keep using Langfuse for production trace replay, alerting,
and eval. ATO covers the part Langfuse doesn't: building,
multi-LLM comparison, deploying.

### With Helicone (gateway / caching)

```
ato config set llm.endpoint https://oai.helicone.ai/v1
ato dispatch ...  # routes through Helicone for caching + cost
```

Point ATO at your Helicone endpoint as the LLM URL. Helicone
caches, ATO orchestrates, both happy.

### With LangChain (DIY framework)

Use LangChain if your agent needs custom orchestration ATO doesn't
support. Use ATO if you want the team layer (RBAC, audit, multi-LLM
war-room) without writing it yourself.

Most teams graduate from LangChain to ATO when they hit the team
threshold — when more than one person needs to use the same agent
and the "everyone shares one config" pattern stops scaling.

### With Anthropic Agents (Claude-only hosted)

Use Anthropic Agents if you're shipping a Claude-only product and
you want native Anthropic hosting. Use ATO if you want to A/B
Claude vs Gemini vs GPT on the same question, OR if you need
local-first / BYOK / RBAC.

You can use both — deploy ATO bundles to Anthropic's hosted infra
as a deployment target.

### With OpenAI / Anthropic fine-tuning

Fine-tune at the provider. Then in ATO:

```bash
ato model add my-custom-claude \
  --provider anthropic \
  --model-id ft:claude-3-5-sonnet-20241022:acme:research:abc123

ato dispatch claude --model my-custom-claude "..."
```

Your custom model joins war-rooms, gets audited, deploys to
bundles — all identical to base models.

---

## The OSS / Cloud / Team split

| | OSS (this repo, MIT) | Team tier (`ato-cloud`) | Enterprise |
|---|----------------------|-------------------------|------------|
| Local dispatch + multi-LLM | ✅ | ✅ | ✅ |
| War-rooms | ✅ | ✅ | ✅ |
| MCP integration | ✅ | ✅ | ✅ |
| Anti-injection floor (P0) | ✅ | ✅ | ✅ |
| Identity passthrough (P2) | ✅ | ✅ | ✅ |
| Deployed bundles | ✅ | ✅ (more) | ✅ (unlimited) |
| Cloud workspace + room ACLs | — | ✅ | ✅ |
| Audit + denial UI | — | ✅ | ✅ |
| Classifier-policy enforcement | — | ✅ | ✅ |
| SSO + audit retention | — | — | ✅ |

Free OSS covers the single-user / small-team local-first case.
Team tier adds the cloud workspace layer when you need to
coordinate identity + audit across multiple people. Enterprise
adds SSO + compliance certifications when you need them.

---

## FAQ

### "Should I replace Langfuse with ATO?"

No. Use both. ATO is the layer that intervenes before the model
runs (RBAC, war-rooms, anti-injection). Langfuse is the layer
that observes what happened. They're complementary.

### "Should I replace Helicone with ATO?"

No. Helicone is your gateway; ATO sits above it. Point ATO at
your Helicone endpoint as the LLM URL and use both.

### "I'm already on LangChain — why ATO?"

Use LangChain for the agent runtime. Use ATO for the team layer
(RBAC, audit, multi-LLM comparison, deploy pipeline). They
compose.

### "I'm Claude-only — why not just use Anthropic Agents?"

You can. ATO's multi-LLM differentiator only matters if you want
to A/B different vendors. If you're committed to Claude
permanently, Anthropic's hosted surface is a fine choice. ATO is
the better choice when you want optionality or local-first.

### "We use OpenAI fine-tunes — does ATO support them?"

Yes. Fine-tune at OpenAI, get the custom model ID, point ATO at
it. Same dispatch + audit + deploy flow as base models.

### "What does ATO replace?"

The custom permission / audit / war-room / deploy code your team
was about to write itself. That code is what ATO is.

---

## Get started

```bash
brew install ato
ato login
ato agent create my-first-agent
ato dispatch my-first-agent "ask your question"
```

3 minutes from install to first dispatch.

If you want to wire an MCP server you've already built, see
[`mcp-author-guide.md`](./mcp-author-guide.md).
