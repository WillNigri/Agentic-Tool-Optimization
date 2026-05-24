# ATO Pro Methodology Runner — architecture spec (v2.10 PR-1)

> **The product.** Industry-baseline automated methodology runs (n ≥ 30 per cell,
> cross-prompt, cross-model, confidence-interval bounded, regression-detectable),
> composed on top of the v2.9 grounded-mode receipts. What our customers pay for.
> What the v2.9 build log demonstrated by hand (and what the [Parts 2–4 blog post][1]
> honestly admits is below industry sample size — this fixes that).
>
> [1]: https://agentictool.ai/posts/we-used-ato-to-test-ato-parts-2-3-4.html

## Problem statement (what the v2.9 dogfood already proved)

The grounded-mode build log we just shipped fired ~30 dispatches across 4 PRs to
make architectural decisions. That's two orders of magnitude below what Braintrust
/ Patronus / Promptfoo customers actually run for a single methodology — those
customers run **50–500 cases × N variants** per eval, **n ≥ 30 per cell** for
statistical confidence, and re-run the same methodology weekly to detect
regressions when models change.

Our v2.9 ad-hoc "I'll fire 6 dispatches and decide" is below that bar. We said
so in the blog. Now we close it as the Pro product.

## Architecture — what the runner does

```
                    methodology template
                         (frozen config)
                              │
                              ▼
        ┌──────────────────────────────────────────────┐
        │  Methodology Runner (apps/cli + ato-cloud)   │
        │                                              │
        │  1. Expand variant matrix                    │
        │     prompts × models × conditions × N_reps   │
        │                                              │
        │  2. Fan out dispatches (parallel, batched)   │
        │     each lands in execution_logs with the    │
        │     v2.9 grounded-mode receipt shape         │
        │                                              │
        │  3. Score each dispatch against the rubric   │
        │     (regex | LLM judge | structural)         │
        │                                              │
        │  4. Compose: mean, sd, 95% CI per cell;      │
        │     significance test across cells           │
        │                                              │
        │  5. Cost decomposition (DUAL ACCOUNTING)     │
        │     - customer_cost_usd: what they spent     │
        │     - provider_cost_usd: what we spent       │
        │     - margin_usd: customer − provider        │
        │                                              │
        │  6. Persist methodology_run row with         │
        │     verdict + receipts + cost ledger         │
        └──────────────────────────────────────────────┘
                              │
                              ▼
                     Insights → Methodologies tab
                       (filter, compare, regress)
```

## Schema deltas

```sql
-- One methodology = a reusable test recipe (e.g., "which model for security review")
CREATE TABLE methodologies (
  id              TEXT PRIMARY KEY,
  slug            TEXT NOT NULL UNIQUE,
  description     TEXT,
  archetype       TEXT NOT NULL,
    -- 'which-model' | 'tools-vs-no-tools' | 'order-effects' |
    -- 'cost-quality-pareto' | 'regression-detection' | 'custom'
  variant_matrix  TEXT NOT NULL,
    -- JSON: {prompts: [...], models: [...], conditions: [...], reps: N}
  rubric          TEXT NOT NULL,
    -- JSON: {kind: 'regex'|'llm_judge'|'structural', config: {...}}
  created_at      TEXT NOT NULL,
  created_by      TEXT
);

-- One methodology RUN = one execution of the recipe against the customer's data
CREATE TABLE methodology_runs (
  id                          TEXT PRIMARY KEY,
  methodology_id              TEXT NOT NULL REFERENCES methodologies(id),
  customer_user_id            TEXT,
  started_at                  TEXT NOT NULL,
  ended_at                    TEXT,
  status                      TEXT NOT NULL,
    -- 'queued' | 'running' | 'complete' | 'failed' | 'cancelled'
  total_dispatches_planned    INTEGER NOT NULL,
  total_dispatches_completed  INTEGER NOT NULL DEFAULT 0,

  ---- DUAL COST ACCOUNTING (the part that makes Pro economics work) ----
  -- Customer-side: what they "spent" (in their accounting)
  customer_cost_usd           REAL NOT NULL DEFAULT 0,
  customer_tokens_in          INTEGER NOT NULL DEFAULT 0,
  customer_tokens_out         INTEGER NOT NULL DEFAULT 0,
  customer_dispatches         INTEGER NOT NULL DEFAULT 0,
  -- Which side of BYOK this customer is on
  customer_billing_mode       TEXT NOT NULL,
    -- 'byok' (their API key, $0 to us) |
    -- 'pool' (our shared Pro pool key — they pay flat $/mo, we pay actual API cost)

  -- Provider-side (us): what WE pay to run this
  provider_llm_cost_usd       REAL NOT NULL DEFAULT 0,
    -- Only non-zero when billing_mode='pool'; equals the API-provider invoice
    -- attributable to this methodology run. Customer doesn't see this column.
  provider_judge_cost_usd     REAL NOT NULL DEFAULT 0,
    -- Cost of running the LLM judge (if rubric kind='llm_judge')
  provider_compute_seconds    REAL NOT NULL DEFAULT 0,
    -- Railway / cloud compute attributed to this run (orchestrator + scoring)
  provider_storage_bytes      INTEGER NOT NULL DEFAULT 0,
    -- Receipts + transcripts persisted in cloud trace retention
  provider_bandwidth_bytes    INTEGER NOT NULL DEFAULT 0,

  -- Computed margin (Pro tier value = customer_cost saved or replaced
  -- minus our_cost; positive means the methodology is sustainable)
  provider_total_cost_usd     REAL NOT NULL DEFAULT 0,
    -- = llm + judge + compute_seconds*$_per_compute_sec + storage*$_per_byte
  margin_usd                  REAL NOT NULL DEFAULT 0,
    -- billing_mode='byok': margin = customer's $29/mo / runs_this_month - provider_total_cost
    -- billing_mode='pool': margin = customer's $99/mo (higher tier) / runs_this_month - provider_total_cost

  -- Result
  verdict_json                TEXT,
    -- Composed result: per-cell statistics, recommended variant, confidence
  receipt_url                 TEXT
);

-- Per-dispatch link from methodology_runs to execution_logs.
-- Composition table — every receipt the methodology composed is foreign-keyed here.
CREATE TABLE methodology_run_dispatches (
  methodology_run_id  TEXT NOT NULL REFERENCES methodology_runs(id),
  execution_log_id    TEXT NOT NULL REFERENCES execution_logs(id),
  variant_cell        TEXT NOT NULL,
    -- JSON: {prompt_id, model, condition, rep_idx} — the cell coordinates
  score               REAL,
    -- Result of running the rubric against this dispatch
  PRIMARY KEY (methodology_run_id, execution_log_id)
);

CREATE INDEX idx_methodology_run_dispatches_run ON methodology_run_dispatches(methodology_run_id);
CREATE INDEX idx_methodology_runs_status ON methodology_runs(status, started_at DESC);
CREATE INDEX idx_methodology_runs_customer ON methodology_runs(customer_user_id, started_at DESC);
```

## Transparency — what the customer sees, BEFORE and DURING the run

ATO is open source. The cost rate constants live in a public file in the repo
(`packages/ato-pricing/pricing.json`), not buried in cloud config. Customers can
audit our margin math the same way they audit our dispatch code.

### Pre-run cost estimate (REQUIRED before any fan-out)

Every methodology run must surface a cost estimate **before the customer
commits to the spend**. The runner refuses to fan out without explicit
acknowledgment of the estimate (the CLI `--yes` flag bypasses the prompt for
scripted use).

```
$ ato evaluations methodology run which-model-for-security-review

About to run methodology: which-model-for-security-review
  Variants:     4 models × 3 conditions × 30 reps = 360 dispatches
  Models:       claude-sonnet-4-6, codex/gpt-4.1, gemini-2.5-flash,
                openai/gpt-4o
  Prompts:      5 distinct prompts from prompts.json

Estimated TOKEN spend (your API keys, your invoice):
  claude:       120 dispatches × ~1,840 in + ~412 out × $3.00/MTok in,
                $15.00/MTok out  =  ~$0.66 + ~$0.74  =  ~$1.40
  codex:        90 dispatches × ~1,610 in + ~588 out × $5.00/MTok in,
                $15.00/MTok out  =  ~$0.72 + ~$0.79  =  ~$1.52
  gemini:       90 dispatches × ~2,100 in + ~344 out × $0.075/MTok in,
                $0.30/MTok out   =  ~$0.01 + ~$0.01  =  ~$0.03
  openai:       60 dispatches × ~1,750 in + ~480 out × $2.50/MTok in,
                $10.00/MTok out  =  ~$0.26 + ~$0.29  =  ~$0.55
  ─────────────────────────────────────────────────────────────
  Your total estimated LLM cost:                      ~$3.50

Estimated ATO compute cost (already covered by your $29/mo Pro tier):
  Orchestrator + scoring (Railway):  ~120 sec  =  $0.0006
  LLM-judge runs (claude-haiku-4-5):  360 × $0.001  =  $0.36
  Trace storage (28 days):          ~14 MB  =  $0.00001
  ─────────────────────────────────────────────────────────────
  Our cost to deliver this run:                       ~$0.36

  Your Pro tier budget this month:    $29.00
  Our cumulative cost for you this month: $4.18
  Remaining margin in your tier:      $24.82 ✓

Continue? [y/N]:
```

Two columns on every estimate, every time, no exceptions:
- **What YOU'RE about to spend** (their LLM provider invoice, exactly as if they
  ran it manually — we don't mark up tokens)
- **What WE'RE about to spend** (our cost to orchestrate + judge + store) and
  whether the customer's tier covers it

### Live cost meter during the run

```
$ ato evaluations methodology run which-model-for-security-review --yes

[ 1/360] claude  prompt=P1 cond=cold   ✓ 1840/412 tok  $0.012  cum=$0.012
[ 2/360] claude  prompt=P1 cond=cold   ✓ 1812/389 tok  $0.011  cum=$0.023
[ 3/360] claude  prompt=P1 cond=soft   ✓ 1923/487 tok  $0.014  cum=$0.037
...
[180/360] codex  prompt=P3 cond=strict ✓ 1602/588 tok  $0.017  cum=$1.84
─────────────────────────────────────────────────────────────────────────
PROGRESS  50.0%  estimated remaining: $1.71  current pace: $2.06/min
```

Customers see their spend accumulate in real time. They can `Ctrl-C` at any
moment and the cost is bounded to what's been billed already (no surprise
phantom charges from a runaway run).

### Post-run cost decomposition — both columns

```
$ ato evaluations methodology show <run-id> --human

Methodology run: which-model-for-security-review
Status: complete   Duration: 3m 14s   Dispatches: 360 / 360

Recommended variant:  codex with --mode-override strict
  Cost-quality Pareto winner: 0.94 quality score at $0.017/dispatch
  vs claude (best quality 0.97 at $0.024) and gemini (best cost
  $0.001 but quality 0.81)

YOUR token spend (your LLM invoice, by provider):
  Anthropic (claude):  $1.44   ████████████████████░░░░  40%
  OpenAI (codex+4o):   $2.07   ████████████████████████  59%
  Google (gemini):     $0.03   ░░░░░░░░░░░░░░░░░░░░░░░░   1%
  ─────────────────────────
  Total YOUR spend:    $3.54

OUR cost to deliver this run (already covered by your Pro tier):
  Orchestrator compute:     $0.0006
  LLM-judge (claude-haiku): $0.36
  Trace storage (28d):      $0.00001
  ─────────────────────────
  Total OUR cost:           $0.36

  Pro tier still ahead:     $24.82 of $29.00 (budget for this month)

Verdict ladder for the 360 dispatches:
  compliant:   178  (49%)    advisory:  102  (28%)
  violation:    47  (13%)    not-enforced:  33  (10%)

Re-run this methodology weekly to detect when a model swap regresses
quality: `ato evaluations methodology schedule <id> --weekly`
```

### `packages/ato-pricing/pricing.json` — the published rate card

This file is **public, in the OSS repo, and version-controlled**. Customers can
read it, audit it, pin a specific version, or propose PRs to correct rates.
What our margin math reads from at runtime.

```json
{
  "$schema": "https://agentictool.ai/schemas/pricing-v1.json",
  "updated_at": "2026-05-24",
  "rates": {
    "llm_judge_default_model": "claude-haiku-4-5",
    "llm_judge_cost_per_call_usd": 0.001,
    "compute_per_second_usd": 0.000005,
    "storage_per_byte_month_usd": 0.000000023,
    "bandwidth_per_byte_usd": 0.00000009
  },
  "tiers": {
    "free": {
      "monthly_usd": 0,
      "methodology_runs_per_month": 1,
      "max_dispatches_per_run": 50,
      "llm_judge_calls_included": 50
    },
    "pro": {
      "monthly_usd": 29,
      "methodology_runs_per_month": 10,
      "max_dispatches_per_run": 500,
      "llm_judge_calls_included": 5000
    },
    "team": {
      "monthly_usd": 99,
      "monthly_usd_per_seat": 99,
      "methodology_runs_per_month": "unlimited",
      "max_dispatches_per_run": 2000,
      "llm_judge_calls_included": 25000,
      "pool_key_credit_usd_per_seat": 50
    }
  },
  "source_invoices": {
    "compute": "Railway shared CPU baseline 2026-Q2",
    "storage": "Cloudflare R2 standard tier 2026-Q2",
    "bandwidth": "Cloudflare R2 egress 2026-Q2",
    "llm_judge": "Anthropic claude-haiku-4-5 average over Q2 2026 internal calls"
  }
}
```

If our actual Railway bill goes up 30% next quarter, this file gets bumped in a
commit anyone can read. No black-box pricing decisions. (We can run promotional
discounts via a separate `discounts.json` overlay without obscuring the base
rates.)

### Why transparency wins here

- **Trust**: customers can run `cat packages/ato-pricing/pricing.json` and see
  exactly what we charge ourselves for. No mystery markup.
- **Cool factor**: open-source pricing math is rare and memorable. The blog
  post for v2.10 PR-1 is half spec, half *"here's our actual margin per
  customer per month, calculated live."*
- **Honest growth signal**: if the rate card gets cheaper (compute prices
  drop), customers see the price cut as a PR. If it gets more expensive (Anthropic
  raises pricing), customers see it land in a commit message and can plan.
- **Auditable methodology**: when a customer asks *"why did this methodology
  cost $X.YZ?"* we point at the rate card + the receipt rows. No support back-
  and-forth needed.

## Cost accounting — the load-bearing dimension

**Why this matters.** Without dual accounting, we ship "unlimited methodology
runs at $29/mo" and discover three months in that a single customer's nightly
regression run costs us $400/mo in API + compute. Margin goes negative. The
pricing only works if we can answer **per active customer**: *did our $29/mo
cover what we delivered them?*

### Customer-side (what they see and "spend")

| Billing mode | What the customer experiences | What the customer accounting shows |
|---|---|---|
| `byok` (default Pro $29/mo) | Their own Anthropic/OpenAI/etc keys are used; ATO orchestrates but doesn't pay the API bill | Their API-provider invoice, exact same as if they ran the dispatches manually. We're a $29/mo orchestrator on top. |
| `pool` (Team tier $99/mo) | Our shared Pro pool keys are used for some/all dispatches | Their methodology dashboard shows "$X.YZ worth of LLM calls included in tier" — no separate provider invoice for them |

### Provider-side (what WE see and pay)

Every methodology run computes:

```
provider_total_cost_usd =
    provider_llm_cost_usd          (only on pool tier, = invoice from Anthropic/OpenAI)
  + provider_judge_cost_usd        (LLM-judge rubric runs — always our cost since they use OUR key)
  + provider_compute_seconds * COMPUTE_RATE_USD_PER_SEC
  + provider_storage_bytes  * STORAGE_RATE_USD_PER_BYTE
  + provider_bandwidth_bytes * BANDWIDTH_RATE_USD_PER_BYTE
```

`COMPUTE_RATE_USD_PER_SEC`, `STORAGE_RATE_USD_PER_BYTE`, `BANDWIDTH_RATE_USD_PER_BYTE`
are configured from our actual Railway / object-storage / egress bills, refreshed
monthly. Initial values for v2.10 PR-1 calibration:

| Cost | Rate (initial) | Source |
|---|---|---|
| Compute | $0.000005 per second (Railway shared CPU pricing baseline) | actual Railway invoice / total seconds |
| Storage | $0.000000023 per byte/month (S3-equivalent) | object-storage invoice |
| Bandwidth | $0.00000009 per byte (CloudFront egress baseline) | CDN invoice |
| LLM-judge | $0.001 per judge call (claude-haiku-4-5 average) | actual LLM provider invoice |

These get re-calibrated quarterly. The point isn't precision to the 6th decimal —
it's having a **defensible margin signal per customer per month** so we know if a
heavy user is profitable or not.

### Margin reports

Two reports the company uses, both backed by these columns:

```sql
-- Daily roll-up: total $ we owe API providers vs total $ Pro customers paid us
SELECT
  date(mr.started_at) AS day,
  COUNT(*) AS runs_today,
  SUM(provider_total_cost_usd) AS our_cost_today,
  SUM(margin_usd) AS margin_today,
  -- Customers in red: their monthly margin already exceeded their tier price
  COUNT(DISTINCT CASE WHEN margin_usd < 0 THEN customer_user_id END) AS unprofitable_customer_count
FROM methodology_runs mr
WHERE mr.status = 'complete'
GROUP BY day
ORDER BY day DESC;

-- Per-customer monthly profitability (the support / pricing-tier signal)
SELECT
  customer_user_id,
  COUNT(*) AS runs_this_month,
  SUM(customer_dispatches) AS dispatches_this_month,
  SUM(provider_total_cost_usd) AS our_cost_this_month,
  -- $29/mo Pro = $29 of "subscription revenue" allocated per customer per month
  29.00 - SUM(provider_total_cost_usd) AS margin_assuming_pro_tier,
  CASE
    WHEN 29.00 - SUM(provider_total_cost_usd) < 0 THEN 'auto_offer_team_tier'
    WHEN 29.00 - SUM(provider_total_cost_usd) < 5  THEN 'flag_for_attention'
    ELSE 'green'
  END AS status
FROM methodology_runs
WHERE strftime('%Y-%m', started_at) = strftime('%Y-%m', 'now')
GROUP BY customer_user_id
ORDER BY our_cost_this_month DESC;
```

The `auto_offer_team_tier` row is what gets surfaced in the customer's dashboard
as *"your usage pattern would fit better on Team — same features, fits your run
cadence better"* (read: we're losing money on you at Pro and need to upgrade you
before it bleeds further).

## CLI + MCP surface

```bash
# Define a methodology (writes to methodologies table)
ato evaluations methodology create which-model-for-security-review \
    --archetype which-model \
    --prompts-file ./prompts.json \
    --models claude,codex,gemini,openai \
    --conditions cold,soft,strict \
    --reps 30 \
    --rubric ./security-review-rubric.json

# Run it (writes a methodology_runs row + fans out 4 × 3 × 30 = 360 dispatches)
ato evaluations methodology run which-model-for-security-review

# Inspect (shows the cost decomposition + per-cell stats + recommended variant)
ato evaluations methodology show <run-id> --human

# Cost report (for the customer)
ato evaluations methodology cost --month current

# Cost report (for us, ops-only — gated behind admin role on ato-cloud)
ato-cloud admin methodologies margin --month current
```

MCP tools (so an AI agent can drive a methodology run via MCP):

```typescript
run_methodology({ methodology_id, override_reps?, override_models? })
  → { run_id, planned_dispatches, estimated_cost_usd, receipt_url }

get_methodology_run({ run_id })
  → { status, completed_dispatches, current_stats, verdict_so_far }

list_methodologies({ archetype?, customer_user_id? })
  → array of methodology records with last_run_at + last_verdict

methodology_cost_estimate({ methodology_id, override_reps? })
  → { estimated_customer_cost, estimated_provider_cost, estimated_margin }
```

The `methodology_cost_estimate` tool is the **load-bearing UX** — every call
that would fire 300+ dispatches must surface the estimate first so the customer
knows what they're committing to. AI agents calling this from MCP get the same
estimate, so they can decide whether to fan out at full N or downscale.

## Rollout — 5 PRs to v2.10 PR-1

**PR-1.** Schema: `methodologies` + `methodology_runs` + `methodology_run_dispatches`
tables. Migration + Rust types + unit tests on the dual-cost computation.
~150 LOC.

**PR-2.** Methodology template loader + CLI `ato evaluations methodology create`
+ `list` + `get`. JSON config schema for variant matrices and rubrics. ~250 LOC.

**PR-3.** Runner core: expand variant matrix → fan-out dispatcher → progress
tracking → composition (mean / sd / CI per cell + significance tests).
Reuses the v2.9 grounded-mode receipt as the atomic event. ~400 LOC + statistical
analysis unit tests.

**PR-4.** Rubrics: regex, structural assertion, LLM-judge. The LLM-judge variant
counts toward `provider_judge_cost_usd`. ~250 LOC.

**PR-5.** Dual cost accounting wiring + admin margin reports + customer-facing
dashboard panel. Calibrate the rate constants against an actual Railway month.
~300 LOC.

Total: ~1,350 LOC across 5 PRs. Each shipped with its own scaled-empirical
proof against the bench from this v2.9 series.

## First four methodology archetypes we ship pre-built

These are the templates a Pro customer gets out of the box, mapped to questions
they actually ask. **The first one — within-provider model A/B/C testing — is
the primary use case**: customers want to know whether to keep paying Opus 4.7
or downgrade Agent X to Sonnet 4.6, whether Gemini 2.5 is worth migrating from
2.0, etc. The whole product is built around answering that question with
defensible numbers.

### Archetype 1: `model-ladder` — within-provider model A/B/C

**Question**: *"Should Agent X stay on Opus 4.7, or can Sonnet 4.6 handle this
at 1/8th the cost?"*

The most common Pro use case. Tests the same agent across **every model the
customer has keys for**, within and across providers:

```yaml
methodology: model-ladder
agent: @security-reviewer
prompts: ./prompts/security/*.md         # customer's actual prompts
models:
  - claude-haiku-4-5         # cheap baseline
  - claude-sonnet-4-6        # mid-tier
  - claude-opus-4-7          # top-tier
  - gemini-2.0-flash
  - gemini-2.5-flash
  - gemini-2.5-pro
  - gpt-4o
  - gpt-4.1
  - codex/gpt-4.1
reps_per_cell: 30                         # industry baseline
rubric:
  kind: llm_judge
  judge_model: claude-haiku-4-5
  criteria: "did the reviewer catch the same N issues claude-opus-4-7 caught?"
```

Output: a Pareto frontier with the recommended model, plus a "**if you migrate
from Opus to Sonnet, you save $X.YZ/month at Q% quality regression**" callout.
Same shape as the v2.9 grounded-mode build's verdict math, but composed across
N models with confidence intervals on the quality delta.

This is **the methodology customers pay $29/mo to run weekly** — every time a
provider releases a new model (Sonnet 4.7, Gemini 3, etc.), they re-run the
ladder and either migrate or stay put on data. No vibes-based "Twitter says
Opus 4.7 is better."

### Archetype 2: `tools-vs-no-tools`

Same prompt × same model × {cold, soft, strict} × R reps. Output: does grounding
actually change behavior on YOUR work? Quantifies tool-use rate, hallucination
amplitude (response_chars when tools blocked vs used), and verdict diversity.
**This is the methodology that produced the v2.9 build log** — packaged so
customers can run it on their own agents.

### Archetype 3: `reviewer-order-effects`

Sticky session with reviewer order permuted N times. Output: how much does the
order of voices change the consensus? Quantifies bias from "round 1 reviewer
shapes round 2" effects.

### Archetype 4: `regression-watch`

Scheduled weekly re-run of any methodology with the result diffed against last
week. Output: an alert when the recommended-model rank changes, when quality
drops > X percentage points, or when cost rises > Y%. The "did our agent get
worse this week" signal that production teams actually need.

```bash
ato evaluations methodology schedule which-model-for-security-review \
    --weekly --on regression-watch --alert-on quality_drop_gt:5pp
```

After v2.10 PR-1 ships, customers can build their own methodologies on top.

## Sample-size guidance — what the runner recommends and why

The product surfaces a **sample-size advisor** every time a customer defines a
new methodology, with explicit references to what industry-standard evals use.
Customers can take the recommendation or override to anything ≥ 2 reps per cell.

| Mode | reps/cell | Total dispatches (3 models × 5 prompts × 3 conditions) | Cost (typical claude) | Time | Confidence |
|---|---|---|---|---|---|
| **`fast`** (founder mode) | **n=10** | **450** | ~$11 | ~7 min | wide CIs, directional only |
| **`industry-baseline`** (recommended for production decisions) | **n=30** | **1,350** | ~$33 | ~20 min | tight enough to publish |
| **`high-confidence`** (model-migration calls) | **n=100** | **4,500** | ~$110 | ~70 min | publishable as benchmark |
| **`research`** (one-off academic-grade) | **n=300** | **13,500** | ~$330 | ~3.5 hr | rare; for blog-post material |

What the prompt looks like at methodology-create time:

```
$ ato evaluations methodology create model-ladder-security-review --interactive

Methodology: model-ladder-security-review
Matrix: 9 models × 5 prompts × 1 condition = 45 cells

How many replications per cell?

  [1] fast              n=10   ~450 dispatches   ~$11    ~7 min
  [2] industry-baseline n=30   ~1,350 dispatches ~$33    ~20 min   (recommended)
  [3] high-confidence   n=100  ~4,500 dispatches ~$110   ~70 min
  [4] research          n=300  ~13,500 dispatches ~$330  ~3.5 hr
  [5] custom            (you pick — minimum 2)

  For reference:
    Promptfoo (open source):     typical eval = 10-100 cases × 3-5 models = 30-500 completions
    Braintrust customer dashboards: median eval run = 50-300 examples × N variants
    Patronus public RAG benchmark:  ~1,000 examples
    OpenAI evals library standard:  100-10,000 examples
    HumanEval (academic):           164 problems × N candidates

  Choice [2]:
```

The sample-size advisor numbers come from the same public `pricing.json` rate
card so customers can pre-compute the spend in their head.

## Continuous learning loop (opt-in data sharing)

When a customer opts in (off by default, surfaced as `ato auth share-evaluations`),
their methodology results contribute to a community benchmark surface:

```
Community signal (this week, across N=1,247 opted-in customers running
                   model-ladder methodologies):
  • claude-sonnet-4-6 is winning 67% of @security-reviewer cells (up 4pp wk-over-wk)
  • gemini-2.5-pro is now competitive with gpt-4.1 on @code-reviewer (within 2pp)
  • opus-4.7 still wins @migration-reviewer by 11pp (unchanged)
```

Three load-bearing properties:

1. **Opt-in only.** Default off. We don't aggregate anything a customer
   hasn't explicitly opted to share. The opt-in toggle is per-methodology so
   customers can share `model-ladder` results without exposing their proprietary
   `customer-support-bot` evals.

2. **Differential-privacy aware.** We share **cell-level statistics** (mean,
   sd, n) — never raw prompts, never raw responses. Customer prompts are PII /
   IP; only the aggregated quality scores leave the local DB.

3. **Re-shareable receipts.** A customer running `model-ladder` weekly produces
   a regression-watch artifact they can paste into their team Slack: *"Our
   security-review agent quality dropped 6pp this week — claude-sonnet-4-6
   suddenly underperforming on prompt-class P3. Either roll back to last week's
   model snapshot or investigate."*

This is the data flywheel: every Pro customer running methodologies makes the
community recommendations better, which makes the product more valuable to the
next customer. The receipts they generate are atomic enough to aggregate
without privacy violation; the methodology runner produces the same shape across
every customer so the aggregation is mechanical, not handwavy.

## Where this connects to the v2.9 work we just shipped

Today's grounded-mode build log shipped the **atomic event** the runner composes
— every dispatch is now a row in `execution_logs` with `grounding_verdict`,
`tool_calls_summary`, `grounding_overrides`, cost + duration + model + agent
attribution. The methodology runner is the layer that fans those out at scale
and aggregates. We can't ship the runner without the receipts. We just shipped
the receipts.

Empirical proof we hit the bar: the n=150 scaled eval we ran while writing
this spec (see Part 5 of the build log) produced *real* confidence intervals on
claude's behavior across 5 prompts × 3 conditions × 10 reps — **20× the original
n=1 eval, with publishable statistics**. That's the recipe the runner ships.

## Why this is the wedge

Braintrust / Patronus / Promptfoo grade SINGLE dispatches. Langfuse / Helicone
OBSERVE production. Neither composes methodologies WITH grounding receipts as
the atomic event. The cockpit framing — *"your agent followed your rules
(verifiable receipts), AND we have a Pareto chart showing it was the best
choice on YOUR data, AND we can run the same methodology next week to detect
when the model drifted"* — is the angle nobody else has.

The dual cost accounting is what makes this sellable as a tier rather than a
loss-leader. Per-customer profitability is a single SQL query away. That's the
difference between selling a service and being a service that bleeds you to
death.

## Open questions before PR-1

1. **What's the Pro tier price for unlimited BYOK methodology runs?** Plan
   recommends `$29/mo` (current Pro) covers up to N runs/month with rate-limited
   compute. Customers above the limit get auto-upgraded suggestion.
2. **Team tier price (with pool key)?** Recommend `$99/mo per seat` with
   capped LLM-pool spend, hard-stop with notification at the cap.
3. **Default reps per cell?** `n=30` matches industry baseline. Customers can
   override down to `n=10` for faster cycles or up to `n=100` for higher confidence.
4. **LLM-judge model default?** `claude-haiku-4-5` for cost; `claude-opus-4-7`
   available as upgrade. Judge model is recorded on every scored dispatch so
   regression detection across judge-model swaps is possible.
