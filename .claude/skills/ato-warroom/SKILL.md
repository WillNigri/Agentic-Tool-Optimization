---
name: ato-warroom
version: 1.1.0
description: |
  Before any material decision — code chunk, plan, strategy, design, scope
  cut, push to GitHub — convene a war-room. Claude (this session) is
  ALWAYS the CEO seat: rethink the problem, challenge the framing, pick
  the wedge. The war-room then dispatches specialist seats (designer,
  eng manager, security officer, debugger, DX lead) via gstack skills
  AND via `ato dispatch` to a cross-family runtime so priors actually
  disagree. Every dispatch and decision is filtered through Karpathy's
  four failure modes: wrong assumptions, overcomplexity, orthogonal
  edits, imperative-over-declarative. Complement to `ato-review` which
  is post-code diff review; this is the BEFORE half. Fires before:
  sending a code draft to the user as final, opening a PR, pushing to
  a branch with a remote, committing >50 LOC of behavior change, or
  delivering a plan or strategic recommendation as the final answer.
allowed-tools:
  - Bash
  - Read
  - Edit
  - Write
  - Skill
---

## Stance

This is an **internal operating system** for the people building ATO
(Will + Claude). It is NOT a feature of the ATO app — ATO already
supports war-rooms, dispatch, sessions natively. This skill is the
*workflow discipline* the team applies on top of ATO's primitives so
we make better decisions and miss fewer things.

Treat the procedure as load-bearing for now; iterate after each PR.

## Why this skill exists

To get more of the gstack methodology's catch-rate on ATO's own
decisions. Three components stacked together:

1. **gstack methodology** (Garry Tan, github.com/garrytan/gstack).
   Turns the agent into a virtual engineering team: CEO, designer,
   eng manager, debugger, security officer, QA lead, release engineer.
   Each specialist has its own slash skill, its own forcing questions,
   its own anti-AI-slop checks. The sprint runs **Think → Plan → Build
   → Review → Test → Ship → Reflect**.

2. **Karpathy's four failure modes** (No Priors, March 2026 /
   forrestchang/andrej-karpathy-skills). Every war-room turn is
   filtered through:
   - **Wrong assumptions** — surface them BEFORE code is written.
   - **Overcomplexity** — would a simpler shape ship 80% of the value?
   - **Orthogonal edits** — anything in this change unrelated to the
     stated goal and creeping in via drive-by?
   - **Imperative over declarative** — is the work expressed as
     verifiable outcomes + tests, or as a sequence of "do this, then
     this"?

3. **Use ATO's own primitives.** Cross-family dispatch (`ato dispatch
   minimax|gemini|deepseek`) runs alongside in-session gstack skills
   so priors actually disagree. Same-family pairs are echo chambers.
   ATO's dispatch + sessions are the substrate; this skill is just
   the way *we* use them.

## The CEO seat is yours; specialists are on standby

Claude (this session) is **always the CEO**. The war-room is a room
the CEO presides over, with a permanent roster of specialists waiting
to be summoned. You don't run every specialist for every decision —
you summon the ones whose domain is at stake. But the roster is
always available; specialists are pre-installed (gstack skills) or
one `ato dispatch` away.

CEO responsibilities:
- Frame the question. Decide what tradeoff matters.
- Pick which specialists to summon — by name, matched to the decision
  class (table in §2). Two or more, always, with cross-family
  potential, so disagreement is informative.
- Don't defer. "I'll just ask the user" instead of forming a view is
  abdication. Form a view, run the war-room, let it overturn you if
  it should.
- Read every summoned specialist's reply, force the disagreement to
  the surface, and **pick a position**. War-rooms produce decisions,
  not surveys.
- Record the decision (and the rejected option(s) with reasons) in
  the deliverable's audit trail — PR description, plan doc, commit
  body. No audit trail = the rule was skipped.

Two summons mechanisms, both available, often both used:
- **gstack specialist skills** for in-session perspective from a
  domain-trained role (designer, eng manager, security officer, etc.)
- **`ato dispatch <runtime>`** for a cross-family voice so priors
  actually disagree and we dogfood the product

Default pattern: at least one gstack specialist + at least one
cross-family ATO dispatch, per material decision.

## When this skill fires

Run a war-room before any of these. The trigger is "the decision is
real and reversing it is expensive."

### Code

- New SQL migration / schema column / index
- New service, daemon, module
- New public surface: CLI subcommand, Tauri command, MCP tool, REST endpoint
- Security boundary: encryption at rest, auth, key handling, IPC trust
- Cross-runtime contract: event shape, IPC protocol, dispatch envelope
- Anything that took >1 hour the last time you did something similar
- Anything you'd describe as "architectural"

### Non-code

- Plan or roadmap recommendation about to land in the user's hands
- Pricing / packaging / positioning choice
- Scope cut (deferring feature X to ship Y)
- UI / UX / IA / naming with multiple reasonable answers
- "Should we build this at all" / "is this the right wedge"
- Strategic question the user asked where you have a view

### Delivery moments — never skip

- About to send a code diff or implementation as final answer
- About to `git commit` with >50 LOC of behavior change
- About to `git push` to a branch with a remote
- About to `gh pr create`
- About to deliver a multi-paragraph plan or recommendation as final

If you hit a delivery moment and the war-room hasn't happened, **stop**
and run it retroactively. Apply or record the findings. Then proceed.

### Skip rules

- Trivial fixes (1–2 line bug, typo, comment-only)
- Edits the user dictated verbatim
- Pure formatting / lint / dependency bump
- Mechanical changes the trigger heuristics caught as a false positive

Skip silently is wrong. Skip with a one-line note in the deliverable
("war-room skipped: trivial typo fix") is right.

## Procedure

### 1. CEO frames the question

Bad: "How should I build the provider-key encryption?"
Good: "For encrypting provider keys at rest in Node, I'm choosing
between (A) `crypto.createCipheriv('aes-256-gcm')` built-in vs
(B) `@noble/ciphers`. A is zero-dep but easy to misuse (iv reuse, auth
tag handling); B adds a dep but the API is misuse-resistant. Pick one
and justify against the rejected option."

A specific tradeoff prompt forces commitment. "How should I X?" gets
generic essays.

Before you dispatch, draft your own CEO position in one paragraph. If
the war-room arrives at your position, you'll know it wasn't just
the loudest voice winning. If it overturns your position, that's the
catch you needed.

### 2. Summon specialists from YOUR roster

War-rooms summon agents YOU built — not a pre-vendored list. Different
installs have different skills available; your war-room voices should
reflect what you've actually adopted.

**Build the roster** with the companion `ato-make-agent` skill. It
takes any installed skill (gstack, custom, third-party) and produces
an agent file at `.claude/agents/<slug>.md` (project) or
`~/.claude/agents/<slug>.md` (global), with a model roster (primary +
cross-family alts) baked into the frontmatter.

Common starting roster if you're starting from gstack (build only on
first-need — never the whole table upfront):

| Source skill          | Suggested agent slug | Suggested cross-family alt |
|-----------------------|----------------------|---------------------------|
| `plan-ceo-review`     | `founder`            | MiniMax                   |
| `office-hours`        | `forcing-questions`  | MiniMax                   |
| `plan-eng-review`     | `eng-manager`        | DeepSeek or MiniMax       |
| `plan-design-review`  | `designer`           | Gemini                    |
| `plan-devex-review`   | `dx-lead`            | MiniMax                   |
| `cso`                 | `cso`                | DeepSeek or Grok          |
| `investigate`         | `debugger`           | Codex or Gemini           |
| `codex` (adversarial) | `adversary`          | (already off-family)      |

**Summon a built agent** two ways, both load the same persona file:

- `Task(<slug>)` — in-session via Claude Code's Task tool. Loads the
  agent file at `.claude/agents/<slug>.md` (or `~/.claude/agents/`).
- `ato dispatch <runtime> --agent <slug>` — cross-family voice via
  ATO. (Caveat: until the agent-loading fix lands in ATO, this flag
  is "label only" per the CLI help — see fallback below.)

Two or more summons per war-room. The cross-family leg must use a
different model FAMILY from Claude (not Sonnet-vs-Opus) — that's the
whole point.

CEO presides. Specialists advise. CEO decides.

**Fallback for label-only `--agent`.** Today (v2.6 PR-A era) ATO's CLI
treats `--agent <slug>` as a label only — the agent's system_prompt
isn't loaded into the dispatch. Until v2.6 PR-A.5 ships the fix,
prepend the persona text manually. Use Python so multiline YAML
frontmatter (e.g. gstack's `description: |` blocks containing `---`)
parses correctly — naive `awk '/^---$/'` splits at the wrong place:

```bash
AGENT_FILE=".claude/agents/<slug>.md"
[ -f "$AGENT_FILE" ] || AGENT_FILE="$HOME/.claude/agents/<slug>.md"
PERSONA=$(python3 - "$AGENT_FILE" <<'PY'
import sys, re
text = open(sys.argv[1]).read()
# Strip a leading "---\n...\n---\n" frontmatter block only when it's
# the first non-blank construct. Body is everything after.
m = re.match(r'^---\n(.*?)\n---\n', text, re.DOTALL)
print(text[m.end():] if m else text)
PY
)
ato dispatch minimax "$PERSONA

---

<your war-room prompt>" --agent <slug> --session "$SID" --human
```

When the CLI fix lands, drop the prefix and rely on `--agent` alone.

**Verification note on the Task-tool leg.** Claude Code's `Task` tool
reads agent files from `~/.claude/agents/<slug>.md` (verified: Will's
install has `code-reviewer`, `code-writer`, etc. there). Project-scoped
`.claude/agents/<slug>.md` follows the same precedence pattern as
project-scoped skills (override global). If on your first agent the
Task tool can't see a project-scoped agent, fall back to `--global`
scope in `ato-make-agent` for that persona.

**Default if you have no agents built yet.** Run `ato-make-agent` on
one source skill matching the decision class before opening the
war-room (~5 min per agent). Or fall back to in-session
`Skill(<gstack-skill-name>)` for the specialist leg + a generic
`ato dispatch minimax` (no agent) for the cross-family leg. The
generic path works but loses persona depth; build the agent the
first time a war-room voice recurs.

### 3. Filter every dispatch through Karpathy's four

Embed all four into the prompt you hand each seat. Suggested template:

```
You are the <seat> for ATO. The CEO (Claude) is convening a war-room.
Decision under debate: <one-line>.
Tradeoff: <specific A-vs-B from step 1>.

Karpathy filter — comment on each of the four explicitly:
1. WRONG ASSUMPTIONS — what is the CEO assuming that may not hold?
2. OVERCOMPLEXITY — what simpler shape would ship 80% of the value?
3. ORTHOGONAL EDITS — what's in the proposed scope that doesn't
   belong to the stated goal?
4. IMPERATIVE-OVER-DECLARATIVE — is the goal expressed as verifiable
   outcomes + tests, or as a sequence of steps? Push for outcomes.

Then: pick A or B. Justify against the rejected option. Brief — three
to six bullets, not a wall of text.
```

If a seat declines to commit, ask again with the wedge sharpened. A
seat that won't commit isn't a seat, it's a participant.

### 4. Dispatch — dogfood via `ato dispatch`

For each cross-family seat, route the prompt through ATO. This is the
dogfood beat — DON'T just open another tab.

```bash
# Stable per-decision session so context carries across follow-ups.
SLUG="pr-b-encryption"
SID=$(ato sessions list --limit 20 2>/dev/null | python3 -c '
import sys, json
slug = sys.argv[1]
sessions = json.load(sys.stdin)
for s in sessions:
    if s.get("title", "") == "warroom/" + slug:
        print(s["id"]); break
' "$SLUG" 2>/dev/null)
if [ -z "$SID" ]; then
    SID=$(ato sessions new --runtime claude --title "warroom/$SLUG" 2>/dev/null \
          | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
fi

QUESTION="<the Karpathy-filtered prompt from step 3>"

# Cross-family seat #1 — pick a runtime in a DIFFERENT family from Claude
ato dispatch minimax "$QUESTION" --session "$SID" --human | tee /tmp/wr-cf1-$$.txt
# (optional) second cross-family seat for adversarial-deadlock breaking
ato dispatch gemini  "$QUESTION" --session "$SID" --human | tee /tmp/wr-cf2-$$.txt 2>/dev/null
```

For the in-session specialist seats, invoke the gstack skill directly
via the Skill tool — for example `Skill(plan-ceo-review)`,
`Skill(plan-eng-review)`, `Skill(cso)`, `Skill(investigate)`,
`Skill(codex)` for the adversarial leg.

If `ato dispatch` fails (network, quota, key missing, runtime not
configured), STOP and report it as an ATO bug — friction IS feedback.
Fall back to a second gstack skill OR a manual cross-perspective prompt
in this session, and note the dispatch failure in the deliverable so
it gets fixed.

### 5. CEO synthesizes

Read every seat's response. Force the disagreement to the surface:

- If two seats agree completely, the question was too easy or too
  leading. Reframe or proceed with a flag noting the war-room added
  no signal.
- If seats split, that's the signal. As CEO, pick a position and
  justify it against each loser.
- Apply Karpathy's four filter to YOUR OWN draft answer one more time
  before committing.

You may overrule any single seat. You may not overrule unanimous
disagreement without recording why.

### 6. Record the war-room — audit trail

This is what closes the loop. No audit trail = the rule was skipped.

**PR description**:

```
## War-room (pre-decision)
- Question: <the specific tradeoff>
- Seats: <gstack skills used> + <runtimes dispatched via ato>
- Disagreement: <one-line summary>
- CEO decision: <what won, and the rejected option(s) with reasons>
- Karpathy filter pass:
  - Wrong assumptions: <what was surfaced>
  - Overcomplexity: <what was cut>
  - Orthogonal edits: <what was kept out of scope>
  - Imperative-vs-declarative: <how the goal is verifiable>
- Session id: <warroom session uuid>
```

**Plan / recommendation** sent to user, prepend:

```
## War-room
- Seats: <…>
- CEO decision: <…>
- Karpathy filter: <one-line per mode>
- (Transcript: <session id>)
```

**Commit body** for >50 LOC behavior change:

```
### War-room (pre-code)
<one paragraph: what was debated, what won, which Karpathy mode caught
something>
```

Missing section ⇒ rule skipped ⇒ skill failed ⇒ retroactive war-room
required before next delivery moment.

### 7. Cleanup

```bash
rm -f /tmp/wr-cf1-$$.txt /tmp/wr-cf2-$$.txt
```

The session stays open. Next decision in the same class reuses it via
the `warroom/<slug>` title lookup so context compounds.

## Anti-patterns

- **Skipping the CEO frame.** "Let me ask the agents what to do" — no.
  Form a position first, let the war-room overturn it if it should.
- **Single-seat dispatch.** Second opinion ≠ war-room. Always ≥2 seats
  with cross-family disagreement potential.
- **Generic "reviewer" for every question.** Match specialist to domain
  (`/cso` for crypto, `/plan-ceo-review` for strategy, etc.).
- **Asking "is this OK?"** A sign-off prompt is not a war-room. Ask
  "A vs B, pick one and justify against the loser."
- **Karpathy filter as boilerplate.** If the seat returns "no concerns
  on wrong assumptions" for every prompt, re-ask with a sharper wedge
  or the seat isn't earning its seat.
- **Hiding the war-room from the deliverable.** No audit trail = the
  rule was skipped, period.
- **Skipping because "I already know the answer."** That's exactly when
  the war-room catches what you're missing. The CEO has a position;
  the war-room tests it.

## Pairs with

- **`ato-review`** — post-code diff review (the AFTER half). Run both
  on the same chunk: war-room before drafting, review after drafting.
  Neither replaces the other.
- **`/autoplan`** — gstack's full review pipeline (CEO → design →
  eng → DX). The default war-room for multi-stage features.
- **`/codex`** — gstack adversarial mode. Use as the cross-family leg
  when your `ato dispatch` runtime would otherwise mirror Claude.
- **`/investigate`** — when the decision is "why doesn't this work,"
  not "what should I build." Iron Law: no fix without investigation.

## Origin

This skill exists because v2.6 PR-A shipped without a pre-code
war-room — only the post-code `ato-review` ran. Two of the five
review findings (start() idempotency, scoped Codex token latch) would
have been caught earlier in a Karpathy-filtered war-room. Recording
the rule so future sessions inherit it and the team building ATO is
actually using ATO.
