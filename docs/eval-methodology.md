# How we test AI decisions at ATO — the reusable methodology

> **One-line rule:** before claiming an AI behavior generalizes, run it at
> n ≥ 10 per cell across ≥ 3 prompts. Anything below that is an anecdote,
> not a finding.

This doc codifies the loop the v2.9 grounded-mode build log used. It exists
because the loop is expensive (the n=150 sweep that produced
[Part 5](https://agentictool.ai/posts/we-used-ato-to-test-ato-part-5.html)
cost ~$6 and 12 minutes) — so we run it only when an AI behavior claim is
about to ship to customers, marketing, or our own product decisions. **When
we run it, we run it right.**

## When to run this loop

Run a scaled empirical eval when ALL of these are true:

- **A claim is about to ship.** The blog post, README, sales deck, or
  agent-design decision depends on a behavioral statement about what a
  model does / doesn't do.
- **The claim generalizes.** "Model X is better at task Y" — not "this
  specific dispatch returned this specific text."
- **It's hard to undo if wrong.** Pricing tier copy, agent defaults shipped
  to customers, model-recommendation tables.
- **The cost is justifiable.** ~$3–$30 of API spend depending on sample
  size + variants. Marginal cost matters; default to `industry_baseline`
  (n=30/cell), step down to `fast` (n=10) when iterating, step up to
  `high_confidence` (n=100) for migration calls.

## When NOT to run this loop

Don't burn the tokens when:

- **You're iterating on a one-off product decision.** A 6-dispatch war-room
  is the right tool for "should the README say cockpit or command center."
  Save the n=30 sweep for when the decision is durable.
- **The claim doesn't generalize.** "We dispatched this prompt and got this
  result" is a receipt, not a finding. Receipts are free; n=30 is not.
- **The hypothesis is already established.** "Claude reads files when given
  read_file" doesn't need 30 confirmations. We ran that exact test at n=67
  in Part 5; it's settled. Don't re-burn tokens to re-prove it.

## The four-step loop

### 1. Cold — capture the bench

Fire the prompt with no grounding, no flags, defaults only. Record what each
runtime actually does in its default state. **This is the baseline we're
going to score the implementation against.**

```bash
ato dispatch claude "your prompt"
ato dispatch gemini "your prompt"
# n ≥ 10 per runtime if comparing — single shot only for hypothesis-formation
```

### 2. Control — identify the gap

Look at the cold receipts. Find the row where one runtime does the right
thing and one doesn't. That's the empirical evidence motivating the
implementation. (For the v2.9 series this was the claude-refused-honestly
vs gemini-hallucinated-5KB row.)

### 3. Impl — build the smallest slice that should change the outcome

Whatever your code change is. The smaller the slice, the cleaner the score
in step 4.

### 4. Score — replay the bench at scale

Now the expensive part. Run the cold-control prompt with the impl on, at
real sample size. Compare row-by-row.

**Sample-size ladder** (from `packages/ato-pricing/pricing.json` —
authoritative, published, customer-auditable):

| Mode | reps/cell | Use for |
|---|---|---|
| `fast` (founder) | n=10 | Iterating on product calls; directional only |
| `industry-baseline` (default for shipped claims) | **n=30** | Decisions that go to customers / marketing |
| `high-confidence` | n=100 | Model migrations, infrastructure swaps |
| `research` | n=300 | Rare; benchmark-grade publications |

Industry references the runner cites at methodology-create time:

- Promptfoo: 10–100 cases × 3–5 models per eval (30–500 completions)
- Braintrust median dashboard: 50–300 examples × N variants
- HumanEval academic: 164 problems
- OpenAI evals library standard: 100–10,000 examples

## Cross-prompt minimum (the Part 5 lesson)

**Never report a finding from a single prompt.** The Part 5 scaled eval
falsified one of our own Part 4 claims (`"+109% hallucination under soft
hints"`) because the +109% was the auth.ts cell — at n=10 across 5 prompts
the effect ranged from −55% to +22%. **Prompt-condition interaction
dominates over the average effect.**

Rule: **≥ 3 distinct prompts per cell**, ideally 5+. If the effect doesn't
hold across most of them, the finding isn't real.

## What "real" looks like

A real finding has:

- **n ≥ 10 per cell** (n=30 if going to customers)
- **≥ 3 prompts** in the variant matrix
- **Confidence intervals** (95% CI is the floor)
- **Cross-prompt heterogeneity check** — if the per-prompt variance dwarfs
  the average effect, the finding is prompt-specific, not universal
- **Honest cost decomposition** — "$X spent on Y dispatches across Z
  conditions"
- **Reproducibility recipe** — exact CLI commands so anyone can re-run

What it does NOT look like:

- n=1 vs n=1 on a single prompt with no replications (this is the trap
  Parts 1–4 fell into; Part 5 retracted)
- "We tried it and it looked better" without specifying what "better"
  means in a column the DB can query
- Findings reported as a percent change without a sample size
- Cost numbers that mix cold and grounded dispatches without separating
  them (grounded is ~25× more expensive per dispatch per Part 5 data —
  averaging across them produces nonsense)

## How to publish results

Three deliverables, in this order:

1. **The receipts.** Trace IDs, the SQL query that reproduces the table,
   the exact prompts used. Persisted under `~/.ato/local.db` so anyone
   with the trace IDs can pull the same data.
2. **The score table.** Cell-by-cell means with 95% CI. If you're claiming
   a directional finding, the CI must not cross zero.
3. **The retractions, if any.** If a previous finding fails to replicate
   at scale, retract it explicitly in the same medium that published it
   (or a successor post linked from it). Hiding falsified claims
   disqualifies us from selling the Methodology Runner to customers who
   demand the same rigor of themselves.

## Cost discipline

The token spend is real. The n=150 eval that produced Part 5 cost $6.22
across 12 minutes of wall-clock. The same methodology at n=30/cell across
5 prompts + 9 models = 1,350 dispatches would run ~$33 on claude rates,
~70 minutes wall-clock.

**This is the ceiling** for a routine in-house eval. Don't go above
`industry_baseline` (n=30) without a specific reason — the marginal
confidence gain from n=30 → n=100 is real but slow, and the $33 → $110
cost gap is steep.

Use the `--dry-run` flag liberally before fanning out:

```bash
ato dispatch claude "..." --mode-override strict --require-tools Bash --dry-run
```

…and the methodology runner's pre-run cost estimate (v2.10 PR-1+) will
refuse to fan out a > $5 run without explicit confirmation.

## When in doubt, default to:

1. **n=10/cell** (fast mode) for first-pass exploration → cheap, fast,
   surfaces direction.
2. **5 prompts minimum**, drawn from realistic work, not synthetic.
3. **3 conditions** if testing grounding (cold / soft / strict); otherwise
   2–4 conditions matched to the hypothesis under test.
4. **Receipts in `~/.ato/local.db`** — never in screenshots, never in chat
   transcripts. Receipts are the audit trail.
5. **Publish the retraction policy** alongside the eval — anyone running
   the methodology must commit, in writing, to retracting findings that
   don't replicate. Without that, the loop is theater.

## Templates

Each methodology runner archetype that ships with v2.10 PR-1 is a
pre-configured version of this loop:

- **`model-ladder`** — same agent × N models the customer has keys for ×
  M prompts × R reps. Output: cost-quality Pareto with recommended pick.
  See `docs/methodology-runner.md` for the full spec.
- **`tools-vs-no-tools`** — the methodology that produced the v2.9 build
  log, packaged for customers to run on their own agents.
- **`reviewer-order-effects`** — sticky session with reviewer order
  permuted N times. Quantifies "round 1 shapes round 2" bias.
- **`regression-watch`** — scheduled weekly re-run with diff alerts when
  the recommended model changes, quality drops, or cost spikes.

When a built-in template fits, use it. When it doesn't, fall back to the
hand-rolled loop above.

---

This doc is a living artifact. When we run a new eval, the loop above gets
sharpened. When we retract a finding, the retraction-and-why-it-mattered
gets noted in `history` so future-us doesn't re-make the same mistake.

Last sharpened: 2026-05-24 (v2.9 grounded mode build log → Part 5
retraction).
