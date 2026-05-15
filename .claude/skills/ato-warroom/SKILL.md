---
name: ato-warroom
version: 1.2.0
description: |
  Before any material decision — code chunk, plan, strategy, design,
  scope cut, push to GitHub — convene a war-room. The session driver
  takes the CEO seat: frame the tradeoff, summon specialist seats from
  whatever agent roster the user has built, dispatch a cross-family
  voice via `ato dispatch` so priors actually disagree, decide. A
  failure-mode filter (wrong assumptions / overcomplexity / orthogonal
  edits / imperative-over-declarative — Karpathy's four are one good
  default, swap in your own) runs on every dispatch. Complement to
  `ato-review` (post-code diff review); this is the pre-decision half.
  Fires before: sending a code draft to the user as final, opening a
  PR, pushing to a remote-tracking branch, committing >50 LOC of
  behavior change, or delivering a plan or strategic recommendation as
  the final answer.
allowed-tools:
  - Bash
  - Read
  - Edit
  - Write
  - Skill
---

## What this skill is

A **tool**, not a stack-specific procedure. The war-room mechanism
works with whatever agent roster you've built (via `ato-make-agent` or
hand-authored) and whatever methodology you want layered on top. It
doesn't assume gstack, Claude Code, or any specific specialist set —
those are useful starting points referenced as examples below, not
requirements.

The mechanism in one paragraph: when a material decision is in front
of you, frame it as a tradeoff (A vs B with named costs), summon two
or more agents whose domain the decision touches, force at least one
to run on a different model family so priors diverge, apply a
failure-mode filter to the prompt template, then pick a position and
record the audit trail in your deliverable.

## Why have a war-room at all

Three principles, stack-agnostic:

1. **Specialist personas catch what generalist prompts miss.** A
   security review prompt that explicitly says "you are a security
   reviewer, look for OWASP-class issues" surfaces different findings
   than a generic "review this." Same idea as gstack's virtual team
   pattern; same idea as YC's office hours; same idea as code review
   itself.

2. **Cross-family disagreement raises the floor.** Two turns from the
   same model family confirm each other's blind spots. Cross-family
   pairs (e.g., Claude + MiniMax, Claude + Gemini, Claude + DeepSeek)
   raise the chance that an embedded assumption fails one of them.

3. **Filter every turn through known failure modes.** Karpathy's four
   (wrong assumptions / overcomplexity / orthogonal edits /
   imperative-over-declarative) is one well-tested default. You can
   swap in any framework — SPADE, RICE, OWASP, STRIDE, your own — as
   long as it forces commitment to specific categories of risk per
   turn.

## CEO seat + specialists on standby

The **CEO seat is user-configurable** — it's whichever LLM the user has
chosen to coordinate the war-room. If the user is in a Claude Code
session, Claude is the CEO. If they're driving from Codex, Codex is the
CEO. Same applies to Gemini, OpenClaw, Hermes, or any future runtime.

The CEO seat is set by **whoever is running this skill**. There is no
hardcoded coordinator — that would lock users into a specific stack and
defeat the point of a tool-shaped war-room.

CEO responsibilities (whichever LLM holds the seat):
- Frame the question as a specific tradeoff. "How should I X?" gets
  generic essays; "A vs B, costs C and D, pick one" gets commitment.
- Pick which specialists to summon, matched to the decision class.
  Two or more per war-room. At least one cross-family from the CEO.
- Don't defer. Form a position; let the war-room overturn it if it
  should. Picking "I'll just ask the user" is abdication.
- Read every summoned specialist's reply, surface disagreements, and
  pick a position. Record the decision and the rejected options in
  the deliverable's audit trail. No audit = the rule was skipped.

Specialists are summoned by name from the agent roster the user has
built — whatever that contains. There is no required set; build only
what your decisions actually need.

Two summons mechanisms, both available, often both used:
- **In-session subagent** via the host runtime's subagent mechanism
  (Claude Code's `Task` tool, Codex's agent invocation, Gemini's
  sub-agent call, etc.). Reads the persona from your agent file.
- **`ato dispatch <runtime>`** to a cross-family runtime so priors
  actually disagree. "Cross-family" = a different model family from
  the one holding the CEO seat (e.g., if CEO is Claude, cross-family
  is MiniMax / Gemini / DeepSeek / Codex / etc.).

Default pattern per material decision: at least one in-session
specialist + at least one cross-family dispatch.

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

War-rooms summon agents YOU built — not a fixed list. Different
installs have different skill stacks and different agent rosters;
your war-room voices should reflect what you've actually adopted.

**Build the roster** with the companion `ato-make-agent` skill, or
hand-author agent files directly. Any source works — a gstack skill,
a custom SKILL.md you wrote, a third-party persona file, an OpenAI
Agents SDK definition, anything you can express as a system prompt.
The agent file lands at `.claude/agents/<slug>.md` (project) or
`~/.claude/agents/<slug>.md` (global), with a model roster (primary
+ cross-family alts) declared in frontmatter.

Below is one example roster shape — names borrowed from gstack's
specialist taxonomy because it's a well-known reference, NOT because
it's required. Substitute your own personas freely.

| Domain                | Example agent slug   | Cross-family alt notes    |
|-----------------------|----------------------|---------------------------|
| Strategy / scope      | `founder`            | Any non-primary family    |
| Product framing       | `forcing-questions`  | Any non-primary family    |
| Architecture / tests  | `eng-manager`        | Any non-primary family    |
| Visual / UX / IA      | `designer`           | Any non-primary family    |
| Developer surface     | `dx-lead`            | Any non-primary family    |
| Security / threat     | `cso`                | Any non-primary family    |
| Debug / root-cause    | `debugger`           | Any non-primary family    |
| Adversarial critic    | `adversary`          | Already off-family        |

You can build none of these and instead create entirely different
specialists (`@compliance`, `@perf`, `@user-empathy`, `@ml-eval`, etc.).
The war-room mechanism doesn't care what the personas are — only that
at least two distinct specialists are summoned and at least one runs
cross-family.

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
one skill matching the decision class before opening the war-room
(~5 min per agent). Or fall back to invoking the source skill
in-session (e.g. `Skill(<skill-name>)` for Claude Code; equivalent
for other runtimes) for the specialist leg + a generic
`ato dispatch <cross-family-runtime>` (no agent) for the cross-family
leg. The generic path works but loses persona depth; build the agent
the first time a war-room voice recurs.

### 3. Apply a failure-mode filter to each prompt

Embed the filter framework you've chosen into the prompt template
handed to each summoned seat. Karpathy's four (wrong assumptions /
overcomplexity / orthogonal edits / imperative-over-declarative) is a
good default; you can substitute SPADE, RICE, OWASP, STRIDE, or any
domain-appropriate framework — as long as the framework forces
specific risk categories to be addressed per turn.

Default prompt template (Karpathy's four):

```
You are the <seat-role> for this project. The coordinator (CEO) is
convening a war-room. Decision under debate: <one-line>.
Tradeoff: <specific A-vs-B from step 1>.

Filter — comment on each of the four explicitly:
1. WRONG ASSUMPTIONS — what is the CEO assuming that may not hold?
2. OVERCOMPLEXITY — what simpler shape would ship 80% of the value?
3. ORTHOGONAL EDITS — what's in the proposed scope that doesn't
   belong to the stated goal?
4. IMPERATIVE-OVER-DECLARATIVE — is the goal expressed as verifiable
   outcomes + tests, or as a sequence of steps? Push for outcomes.

Then: pick A or B. Justify against the rejected option. Brief —
three to six bullets, not a wall of text.
```

If a seat declines to commit, ask again with the wedge sharpened. A
seat that won't commit isn't a seat, it's a participant.

### 4. Dispatch the cross-family seat(s) via `ato dispatch`

Route each cross-family seat through ATO so the work flows through
your dispatch + session primitives.

**Pick a tool-capable runtime when the seat needs to walk the code.**
This is the most-bitten failure mode in 2026-05-15 sessions. ATO's
`dispatch` targets split into two classes:

- **Tool-capable runtimes** (CLI binaries with their own Read / Grep /
  Bash loop): `claude`, `codex`, `gemini`, `openclaw`, `hermes`.
  Check live status with `ato runtimes health`. These walk files
  themselves; you pass them a brief, they go fetch the artifacts.
- **API-only providers** (one-shot HTTP request → text response): all
  of `minimax`, `grok`, `deepseek`, `qwen`, `openrouter`. No tool loop;
  they can only reason from what's in the prompt.

Match the seat to the question class:

| Question class | Tool access needed? | Pick |
|---|---|---|
| Code review, security audit, PR diff scrutiny | YES | tool-capable (`codex` is the canonical cross-family choice when CEO is Claude) |
| Scope / strategy / positioning / pricing | NO | either works; API-only is fine + cheaper + faster |
| Plan / roadmap review | Sometimes — only if the plan cites specific files the seat should verify | tool-capable if "yes," otherwise API-only |
| Adversarial challenge / 10-star reframe | NO | API-only is fine; the value is the model's priors, not file access |

**Pitfall to avoid.** If a code-touching war-room dispatches to an
API-only runtime with a summary instead of the raw code, the seat
can only validate against your paraphrase — it cannot catch what
you missed. The 2026-05-15 v2.6 security sweep hit this directly:
a Claude in-session seat reviewed the provider-keys path, an
API-only minimax dispatch was attempted (failed on prompt size
anyway), and the sweep shipped 5 fixes. Switching the cross-family
seat to `codex` (tool-capable) immediately surfaced a 6th — a TOCTOU
race on `MAX_ACTIVE_PROVIDER_KEYS` the Claude seat had read past.
The cross-family value comes from a different model FAMILY *and*
tool access; same-family-with-tools beats different-family-without-
tools for code review.

If the obvious tool-capable runtime is unavailable (not installed,
broken auth, etc.) and you must use an API-only fallback for a
code-touching question, COMPENSATE by sending the actual source
in the prompt (cap ~30 KB to fit `ato dispatch`'s command-line
arg ceiling — chunk by PR if larger, or use `--prompt-file` once
ATO supports it). Note the methodology gap in the audit trail so
it's visible.

```bash
# Stable per-decision session so context carries across follow-ups.
SLUG="pr-b-encryption"
# Pick whichever runtime you're driving as CEO for the session anchor.
CEO_RUNTIME="${ATO_CEO_RUNTIME:-claude}"
SID=$(ato sessions list --limit 20 2>/dev/null | python3 -c '
import sys, json
slug = sys.argv[1]
sessions = json.load(sys.stdin)
for s in sessions:
    if s.get("title", "") == "warroom/" + slug:
        print(s["id"]); break
' "$SLUG" 2>/dev/null)
if [ -z "$SID" ]; then
    SID=$(ato sessions new --runtime "$CEO_RUNTIME" --title "warroom/$SLUG" 2>/dev/null \
          | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
fi

QUESTION="<the filter-wrapped prompt from step 3>"

# Cross-family seat #1 — DIFFERENT family from the CEO runtime.
# Pick per the table above. Default for code-touching war-rooms when
# CEO is Claude: `codex` (tool-capable, can walk the source itself).
# Default for strategy / scope / positioning: any API-only provider
# is fine (`minimax`, `grok`, `deepseek`, `qwen`, `openrouter`).
ato dispatch codex "$QUESTION" --session "$SID" --human | tee /tmp/wr-cf1-$$.txt
# (optional) second cross-family seat for breaking ties / adversarial pass.
# Use a third family — e.g. minimax — so you have Claude + GPT + MiniMax priors.
ato dispatch minimax "$QUESTION" --session "$SID" --human | tee /tmp/wr-cf2-$$.txt 2>/dev/null
```

For the in-session specialist seats, invoke the source skill directly
via your runtime's subagent / skill mechanism (e.g. Claude Code's
`Skill(<name>)` or `Task(<agent-slug>)`; Codex's agent invocation).

If `ato dispatch` fails (network, quota, key missing, runtime not
configured), stop and note it — friction IS feedback. Fall back to a
second in-session specialist or a manual cross-perspective prompt and
record the dispatch failure in the deliverable so it gets fixed.

### 5. CEO synthesizes

Read every seat's response. Force the disagreement to the surface:

- If two seats agree completely, the question was too easy or too
  leading. Reframe or proceed with a flag noting the war-room added
  no signal.
- If seats split, that's the signal. As CEO, pick a position and
  justify it against each loser.
- Apply your filter framework to your OWN draft answer one more time
  before committing.

You may overrule any single seat. You may not overrule unanimous
disagreement without recording why.

### 6. Record the war-room — audit trail

This is what closes the loop. No audit trail = the rule was skipped.

**PR description**:

```
## War-room (pre-decision)
- Question: <the specific tradeoff>
- Seats: <in-session specialists> + <runtimes dispatched via ato>
- Disagreement: <one-line summary>
- CEO decision: <what won, and the rejected option(s) with reasons>
- Filter pass (<framework name, e.g. Karpathy>):
  - <category 1>: <what was surfaced>
  - <category 2>: <what was cut>
  - <category 3>: <what was kept out of scope>
  - <category 4>: <how the goal is verifiable>
- Session id: <warroom session uuid>
```

**Plan / recommendation** sent to user, prepend:

```
## War-room
- Seats: <…>
- CEO decision: <…>
- Filter pass: <one-line per category>
- (Transcript: <session id>)
```

**Commit body** for >50 LOC behavior change:

```
### War-room (pre-code)
<one paragraph: what was debated, what won, which filter category caught
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
- **Generic "reviewer" for every question.** Match specialist to
  domain (security agent for crypto, founder-mode agent for strategy,
  whichever specialists your roster contains).
- **Asking "is this OK?"** A sign-off prompt is not a war-room. Ask
  "A vs B, pick one and justify against the loser."
- **Filter as boilerplate.** If a seat returns "no concerns on
  <category>" for every prompt, re-ask with a sharper wedge or the
  seat isn't earning its seat.
- **Hiding the war-room from the deliverable.** No audit trail = the
  rule was skipped, period.
- **Skipping because "I already know the answer."** That's exactly when
  the war-room catches what you're missing. The CEO has a position;
  the war-room tests it.

## Pairs with

- **`ato-make-agent`** — the companion tool. Converts source skills
  into ATO agent files with model rosters so war-rooms can summon
  them by name.
- **`ato-review`** — post-code diff review (the AFTER half). Run both
  on the same chunk: war-room before drafting, review after drafting.
  Neither replaces the other.
- **Whatever specialist skills your stack provides** — gstack, custom,
  third-party, or hand-authored. The war-room mechanism doesn't care
  which stack populates your roster.

## Origin

This skill records a discipline gap: pre-code design decisions were
landing without multi-specialist review while post-code diffs were
getting full multi-LLM consensus passes. The pre-decision filter
catches a different class of error (assumptions, scope, framing) than
the post-code diff review catches (bugs, missed edges). Both need to
exist; this skill is the pre-decision half.
