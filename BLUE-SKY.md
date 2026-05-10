# ATO Blue-Sky Backlog

> **Not on the active roadmap.** Items here are research bets that **do not currently fit the north star** in `ato-cloud/docs/STRATEGY.md`:
>
> *ATO is the developer-workflow operations layer for multi-runtime AI agents.*
>
> Reconsider any item below only when one of the north-star invalidation triggers fires (see `STRATEGY.md` §"What changes when the north star is wrong"):
> 1. The market for multi-runtime ops is smaller than we think (signal: low engagement on Compare/Replay surfaces after the first 500 active users)
> 2. The market is bigger than we think but adjacent — paying customers repeatedly ask for "ATO but for our production traffic" (signal: 10+ paying customers, unprompted)
>
> Until then, these items live here so engineering decisions don't drift into them by accident. This file is the *graveyard*, not the roadmap.

---

## v3.0.0+ — Multi-Tenant + Compliance

Originally framed as "planned, exploratory" but each item is a different product that drifts from the dev-workflow ops north star:

- **Team workspaces with shared agents / knowledge / trace history + per-member ACLs.** Adjacent — could fit if scoped to "team developers operating multi-runtime agents together." Currently overshoots into permissioned-collaboration platform territory.
- **PII / safety scanning** of agent conversations with redact-on-export. This is production-observability territory (Langfuse / Helicone own this). In scope only if narrowed to *ATO's own data* (the developer's working files), not user conversations.
- **SOC2 compliance bundles** — audit log, retention controls, export-on-request, BYOK encryption. Enterprise sales territory; reconsider when paying-team count justifies the lift.
- **Marketplace for agent templates** with community submissions + revenue share. Marketplace operations ≠ developer-workflow operations. Curated read-only catalog is acceptable; community marketplace is its own product.
- **Agent versioning + rollback with A/B routing + canary deploys.** Partial fit — versioning + rollback aligns (developer-workflow). A/B routing + canary deploys overlaps with deployment platform territory.

**Verdict:** Keep agent versioning + rollback (north-star aligned) on the active roadmap when it's the priority. Move everything else here.

---

## v4.0.0+ — Federated Agent Network

Speculative; a different product entirely.

- **Agent-to-agent discovery protocol** — agents on different ATO installations call each other via a registered handle (`acme/triage` → `acme/legal-review`). MCP-based, optional.
- **Cross-tenant audit / abuse defense** — when external agents call each other, who pays / who's responsible / how is provenance preserved.
- **Agent reputation system** — an agent's track record (success rate, eval scores, conversations served) becomes a portable signal across deployments.

**Verdict:** Interesting research direction. Not on the path to first 500 users. Reconsider when there's a real federated-agents market and we're a credible reference implementation.

---

## v5.0.0+ — Open Standards / Spin-out Layer

Speculative.

- **ATO becomes the reference implementation for an open agent-deployment standard**, similar to how `kubectl` is the reference for Kubernetes' API. Anyone can build a competing GUI / hosting provider that speaks the same agent spec.
- **Plugin SDK** — third parties (Cursor, Windsurf, Aider, etc.) implement the protocol so the same agent runs unchanged across runtimes.

**Verdict:** Beautiful long-tail vision. Would require multiple deployed ATO competitors to actually be a standard — premature by years. Not the bet for the next 2-4 release cycles.

---

## Specific drifts from previous "v1.6 planned" list

These were tagged "planned" but don't survive the audit:

- **Real-time collaborative workspace (WebSocket via ato-cloud)** — fits if scoped to "developers operating shared agents together." Today's plan reads more like Figma-style collab UX which doesn't map.
- **Team cursors (Figma-style)** — drop. Hosted PTY for team collaboration is the right primitive; cursors are not.
- **Cross-runtime policy enforcement templates (Enterprise)** — enterprise policy ≠ dev workflow ops. Defer.
- **Proactive suggestions ("Your project is missing X")** — fits *only* if grounded in dev-workflow data (file attribution, cost recs, regression patterns). Currently reads as advisory chatbot; that drifts.

---

## How items get promoted back to the roadmap

For any item here to move back to `ROADMAP.md`, all three must be true:

1. **It fits the north star.** Re-read STRATEGY.md's six surfaces. If it doesn't land in one, it stays here.
2. **There's user demand from the active ICP.** Not a hypothetical persona; not a Twitter request; actual users who would use it.
3. **It earns its priority over the existing roadmap.** What gets bumped to make room? If the answer is "nothing, we'll just add it," priority is being decided emotionally.

If any of the three is false, the item stays in BLUE-SKY.md.
