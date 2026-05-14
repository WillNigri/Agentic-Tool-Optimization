---
name: ato-make-agent
version: 1.0.0
description: |
  Turn any installed skill (gstack, custom, third-party) into an ATO agent
  the user can summon into war-rooms. Reads a SKILL.md, extracts the
  persona, strips runtime boilerplate, and writes an agent file at
  `.claude/agents/<slug>.md` (project-scoped) or `~/.claude/agents/<slug>.md`
  (global). Prompts for a model roster (primary + 1-2 alts) so cross-family
  dispatch in war-rooms produces real disagreement. Companion to
  `ato-warroom` — that skill summons agents this skill creates. Use when
  asked "turn this skill into an agent", "register X as a war-room agent",
  or when scoping a new persona before a war-room runs.
allowed-tools:
  - Bash
  - Read
  - Edit
  - Write
  - Glob
  - AskUserQuestion
---

## What this skill does

You install skills. You want some of them to act as full agents that can
be summoned into war-rooms (via `ato-warroom`). This skill is the
converter.

Inputs:
- A source skill (path to SKILL.md, or skill name to look up in standard
  locations).
- An agent slug (short, lowercase, hyphenated — e.g., `cso`, `eng-mgr`).
- A model roster: primary + 1–2 alts. Cross-family is the point.
- Scope: project-vendored or user-global.

Outputs:
- `.claude/agents/<slug>.md` (or `~/.claude/agents/<slug>.md`) with the
  persona system prompt + the model roster recorded in frontmatter so
  `ato-warroom` knows which runtimes to dispatch to.
- A one-line addition to the project's agent inventory (if a `WARROOM-ROSTER.md`
  is present in the repo root).

This skill does NOT:
- Create the ATO SQLite `agents` row. That's the desktop's
  Create Agent wizard's job — and `ato-make-agent`'s output file is
  importable by it on next refresh.
- Pick the persona for you. You decide which skill becomes which agent.

## When to fire

- "Turn `<skill>` into an agent." / "Register `<skill>` as a war-room voice."
- About to use `ato-warroom` and realize the role you want isn't in your
  current agent roster yet.
- Re-tuning an existing agent's model roster.

Skip:
- Trivial single-prompt scratchwork.
- Skills that are pure tooling (e.g., `gstack-upgrade`, `setup-deploy`).
  Those aren't personas. Convert only skills that codify a *role* with
  forcing questions or methodology (e.g., `cso`, `plan-ceo-review`,
  `office-hours`, `investigate`).

## Procedure

### 1. Locate the source skill

If the user gave a path, use it. If they gave a name, search standard
locations:

```bash
SLUG="$1"  # the source skill name from user
for candidate in \
  "$PWD/.claude/skills/$SLUG/SKILL.md" \
  "$HOME/.claude/skills/$SLUG/SKILL.md" \
  "$HOME/.claude/skills/gstack/$SLUG/SKILL.md" ; do
  if [ -f "$candidate" ]; then
    SOURCE="$candidate"
    break
  fi
done
echo "SOURCE=$SOURCE"
```

If nothing found, list available skills and ask.

### 2. Decide agent slug, name, scope, roster — ask once

Use AskUserQuestion (one call, multiple questions) to gather:

- **Agent slug** — short, lowercase, hyphenated. Default: same as source skill slug.
- **Display name** — human-readable. Default: title-case of slug.
- **Scope** — `project` (commits to `.claude/agents/`) or `global` (`~/.claude/agents/`). Default: project for repo-local work, global for cross-project roles.
- **Primary model** — Claude Opus / Claude Sonnet / Codex / MiniMax / Gemini / DeepSeek / Grok / Ollama-local. Default: Claude Opus for heavyweight (CEO, CSO, eng-manager) personas, Claude Sonnet otherwise.
- **Alt model #1** — required. Must be a DIFFERENT family from primary so cross-family dispatch produces real disagreement. Default: MiniMax if primary is Claude.
- **Alt model #2** — optional. Another different family. Default: skip if alt #1 covers the cross-family need.

Don't ask in five separate turns. One AskUserQuestion call, multi-question.

### 3. Extract persona from the source SKILL.md

Read the source. The persona-relevant content is usually:

- **Frontmatter `description`** — keep as one-line summary
- **Sections about voice, methodology, forcing questions, anti-patterns,
  "instructions", "phases"** — keep
- **Sections about preamble, gstack telemetry, plan-mode safe ops, skill
  routing, AskUserQuestion format, completion status, operational
  self-improvement** — strip. These are runtime boilerplate, not persona.

Heuristic rules for stripping:
- Drop any code block that reads `~/.gstack/`, calls `gstack-config`,
  emits `_TEL` / `_SESSION_ID` / `_LEARN_FILE` lines, or runs
  `gstack-update-check`.
- Drop sections titled `## Preamble`, `## Plan Mode Safe Operations`,
  `## Skill Invocation During Plan Mode`, `## Skill routing`,
  `## AskUserQuestion Format`, `## Artifacts Sync`,
  `## Model-Specific Behavioral Patch`, `## Voice` (gstack's voice
  triggers, not the persona's voice — different thing),
  `## Context Recovery`, `## Writing Style`, `## Question Tuning`,
  `## Completion Status Protocol`, `## Operational Self-Improvement`,
  `## Telemetry`, `## Plan Status Footer`.
- Keep everything under sections titled `## Instructions`,
  `## Phase N:`, `## Confidence Calibration`, `## Important Rules`,
  `## Disclaimer`, anything explaining the role's methodology /
  forcing questions / anti-patterns / output format.
- Keep the `# <Title>` heading (becomes the agent display name) and the
  one-line intro paragraph (becomes the agent description).

If unsure whether a section is persona or runtime, default to KEEP and
flag for manual trim — overzealous stripping is worse than a little
bloat.

### 4. Compose the agent file

Frontmatter template:

```yaml
---
name: <slug>
display_name: <display name>
description: <one-line summary>
model: <primary model>
roster:
  primary: <primary model>
  alt1: <alt #1>
  alt2: <alt #2 or omitted>
source_skill: <path or slug of source>
karpathy_filter: true
---
```

`karpathy_filter: true` signals to `ato-warroom` that every dispatch
prompt to this agent should be wrapped with the four-failure-mode
checklist (wrong assumptions / overcomplexity / orthogonal edits /
imperative-over-declarative).

Body template:

```markdown
# <display name>

## Role

<extracted persona intro from source skill>

## Methodology

<extracted phases / forcing questions / anti-patterns>

## Karpathy filter (run on every turn)

For each response, comment explicitly on:
1. **Wrong assumptions** — what is the user assuming that may not hold?
2. **Overcomplexity** — what simpler shape would ship 80% of the value?
3. **Orthogonal edits** — what's in scope that doesn't belong to the
   stated goal?
4. **Imperative-over-declarative** — is the goal expressed as
   verifiable outcomes + tests, or as a sequence of steps?

Then commit to a position. Brief — bullets, not essays.
```

### 5. Write the file

```bash
TARGET_DIR=""
if [ "$SCOPE" = "global" ]; then
  TARGET_DIR="$HOME/.claude/agents"
else
  TARGET_DIR="$PWD/.claude/agents"
fi
mkdir -p "$TARGET_DIR"
TARGET="$TARGET_DIR/$AGENT_SLUG.md"
if [ -f "$TARGET" ]; then
  # Don't clobber. Ask whether to overwrite or pick a new slug.
  echo "EXISTS: $TARGET"
  exit 7  # caller handles
fi
# write file from composed template
```

### 6. Report

Print to user:

```
✓ Wrote <slug> to <path>
  Primary model: <primary>
  Alt roster: <alt1>[, <alt2>]
  Source skill: <source path>

Summon in war-rooms via:
  Task(<slug>)                           # in-session, primary model
  ato dispatch <runtime> --agent <slug>  # cross-family, alt model

(If `ato dispatch --agent` doesn't load system_prompt yet on your
ATO build, prepend the agent's persona text manually until v2.6 PR-A.5
lands. See ato-warroom skill's "fallback for label-only --agent" section.)
```

## Anti-patterns

- **Pre-building a full roster the user didn't ask for.** This skill is
  on-demand. Convert one skill at a time when the user needs that voice.
- **Same-family alt model.** Claude Opus primary + Claude Sonnet alt is
  not cross-family. Defeats the war-room's purpose. Force the alt to a
  different vendor (MiniMax, Gemini, DeepSeek, Codex, Grok).
- **Including gstack runtime preambles in the persona.** The dispatch
  target doesn't run gstack telemetry; it runs as a one-shot turn.
  Strip aggressively.
- **Skipping the Karpathy filter section.** Without it the agent is just
  "a model with a system prompt" — not a war-room voice.
- **Vendoring an agent globally that's repo-specific.** A persona built
  around your ATO codebase belongs at `<repo>/.claude/agents/` so
  teammates inherit it; a generic CSO belongs at `~/.claude/agents/`.

## Pairs with

- **`ato-warroom`** — the consumer. Summons agents created by this
  skill into pre-decision multi-perspective war-rooms.
- **`ato-review`** — post-code diff review. Different lane; runs after
  drafting, not before. Doesn't use the agent roster (yet).
- **Any gstack persona skill** (`cso`, `plan-ceo-review`, `plan-eng-review`,
  `plan-design-review`, `plan-devex-review`, `investigate`, `office-hours`,
  `codex`) — these are the canonical inputs. Run `ato-make-agent` on
  each one you want as a permanent war-room voice.

## Origin

The user (Will) explicitly rejected pre-vending a fixed war-room roster:
different installs have different skill sets, and the right primitive is
*"let users build their own roster from skills they actually have."*
This skill is that primitive. The companion `ato-warroom` summons from
whatever roster the user has built.
