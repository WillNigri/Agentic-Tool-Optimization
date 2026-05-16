# ATO Sessions — how multi-turn conversations work

> Sessions are how ATO structures decision history on the local SQLite database. Each session is one work unit — one decision, one war-room, one debug, one ratification round. The Sessions list in the desktop is meant to be readable months later by someone (you, your teammate, your future self) asking *"what was decided about X, and when?"*. That only works when each row describes a coherent unit of work.

This doc is the canonical reference. The README's [Sessions section](../README.md#sessions-how-multi-turn-conversations-work) is the short version; `.claude/skills/ato-warroom/SKILL.md` section 4 covers the war-room methodology that sits on top of sessions.

---

## The data model

Every session has two SQLite tables behind it:

### `sessions` — one row per session

| Column | Type | Meaning |
|---|---|---|
| `id` | TEXT (UUID) | The session id. Stable forever; pass to `--session <id>`. |
| `runtime` | TEXT | Anchor runtime — where the session was created. Marked with `★` in the UI. Any runtime can dispatch into the session; this is just the default. |
| `agent_slug` | TEXT | Optional anchor agent (set via `ato sessions new --as <slug>`). The session-level persona, distinct from per-turn personas. |
| `title` | TEXT | Title set at creation time. Falls back here when `auto_title` is null. |
| `auto_title` | TEXT | Coordinator-generated title set by `ato sessions close`. Preferred over `title` in the UI. |
| `summary` | TEXT | Coordinator-generated summary of the full transcript. Set on close, refreshed on subsequent closes after reopen. |
| `tags_json` | TEXT (JSON array) | Coordinator-generated topic tags. Queryable across sessions. |
| `project_id` | TEXT | Inferred project the session belongs to. Used for grouping in the UI. |
| `status` | TEXT | `open` (taking new turns) or `closed` (frozen). |
| `created_at`, `last_used_at`, `closed_at` | TEXT (RFC3339) | Lifecycle timestamps. |
| `turn_count` | INTEGER | Cached count of turns in `session_turns` for this session. |

### `session_turns` — one row per turn

| Column | Type | Meaning |
|---|---|---|
| `session_id` | TEXT | Foreign key to `sessions.id`. |
| `turn_index` | INTEGER | Monotonic per-session, starts at 0. |
| `role` | TEXT | `user` (the dispatch prompt) or `assistant` (the response). |
| `text` | TEXT | The full turn body. |
| `runtime` | TEXT | The runtime that produced this turn. Differs from `sessions.runtime` for cross-runtime sessions. |
| `agent_slug` | TEXT | Persona slug if dispatched with `--agent <slug>`; NULL for generalist turns. |
| `created_at` | TEXT (RFC3339) | When the turn landed. |
| `sender_peer_id` | TEXT | Mesh peer id when the turn came from another machine; NULL for local. |

---

## The three dispatch types

A session is a container; what you put in it determines the value. There are three legitimate ways to dispatch a turn:

### 1. Generalist — pure model priors

```bash
ato dispatch minimax --session b1547c69-... "What do you think of the wedge?"
```

No `--agent`, no skill, no persona overlay. The model answers from its priors only. Useful when you want an untainted voice — for falsifying a specialist's view, for a sanity-check, for "what would a smart outsider say?". One generalist among specialists keeps the room honest; four specialists who agree on framing AND substance is groupthink.

**Trade-off:** no agent slug in the audit trail; can't be filtered as "the Positioning seat" later.

### 2. Agent-backed specialist — deterministic persona

```bash
ato dispatch minimax --agent positioning --session b1547c69-... "Round 1: name the wedge."
```

The agent record's `system_prompt` (a tight 150-300 word persona definition) is prepended to the dispatch automatically. The slug is captured in `execution_logs.agent_slug` and `session_turns.agent_slug`. The desktop UI renders the persona name (`POSITIONING`) instead of the generic `ASSISTANT` role, with the underlying runtime visible in a small pill.

**Trade-off:** one-time setup to create the agent record. After that, deterministic and cross-runtime portable.

**Creating an agent record:**

```bash
# From a Claude-style agent .md file (frontmatter + body = system prompt)
ato agents create --from-file ./positioning.md --runtime minimax

# Or inline
ato agents create --runtime minimax --slug positioning \
  --display-name "Positioning seat" \
  --system-prompt "You are the Positioning seat in a multi-LLM war-room, working in the April Dunford / Andy Raskin tradition..."
```

The 5 gstack agents (positioning, devex, ceo, designer, office-hours) ship as a template in `.claude/skills/ato-warroom/gstack-agents.sql`.

### 3. Skill-loaded (Claude in-session only)

Inside a Claude Code session, invoke the skill directly:

```
Skill(office-hours)
Task(positioning-clarifier)
```

This loads the full `~/.claude/skills/<name>/SKILL.md` with all its procedural depth (steps, decision trees, examples) — much richer than the distilled persona in an agent record. **Caveat:** doesn't transfer to cross-runtime dispatches. For a multi-LLM war-room, mirror the skill's persona-identity into an agent record (option 2) so non-Claude runtimes can play the same role.

### Mix freely

A war-room with 3 agent-backed specialists + 1 generalist usually produces sharper outputs than 4 of the same kind. The variety is the value.

---

## The lifecycle

```bash
# 1. Create the session at the topic boundary. Give it a clear title.
ato sessions new --runtime claude --title "PMF war-room — wedge + pitch 2026-05-16"
# → returns: { "id": "b1547c69-...", ... }

# 2. Dispatch turns. Each new turn sees prior turns via history replay.
ato dispatch minimax --agent positioning --session b1547c69-... "Round 1: name the wedge."
ato dispatch google  --agent devex       --session b1547c69-... "Round 2: TTHW audit."
ato dispatch claude  --agent ceo         --session b1547c69-... "Round 3: 10-star reframe."

# 3. Close. The coordinator runtime reads the full transcript and generates
#    auto_title, summary, tag list, and inferred project_id — all persisted
#    on the sessions row.
ato sessions close b1547c69-...

# 4. (Optional) Reopen ONLY if a follow-up belongs to the same topic.
#    The next close refreshes summary + tags with the new turns.
ato sessions reopen b1547c69-...
ato dispatch claude --agent designer --session b1547c69-... "One amendment on the hero."
ato sessions close b1547c69-...

# Inspect via CLI any time.
ato sessions list --limit 20
ato sessions get b1547c69-...
```

### What `close` does

The coordinator runtime (anchored to `sessions.runtime`) is given the full transcript and asked to produce:

- **`auto_title`** — a concise human-readable title that supersedes the user-supplied title in the UI.
- **`summary`** — a paragraph distilling the decision, key disagreements, and verdict.
- **`tags`** — topic tags (`["wedge", "positioning", "compare-runtimes"]`) for cross-session search.
- **`project_id`** — inferred project the session belongs to (e.g., `agentictool-marketing`).

These four fields become the row's identity in the Sessions list. They're how the row stays legible six months from now.

### What `reopen` does

Sets `status='open'`, clears `closed_at`, leaves all other coordinator-generated fields intact (so a partially-stale summary is visible until the next close refreshes it). The next dispatch into the session adds a new turn at the end. The next close regenerates auto_title/summary/tags from the FULL transcript (including pre-reopen turns) plus the new ones.

**Reopen is for genuine continuation:**

✅ Customer reply on a strategy debate that adds new evidence.
✅ Round 4 ratification of a Round 3 synthesis.
✅ A late security finding amends an earlier audit conclusion.

❌ A new question that happens to be vaguely related.
❌ A smoke test to verify something works.
❌ Anything that would change the summary's interpretation of what the session was about.

When in doubt, **create a new session** and link by tags or a meta-doc.

---

## How sessions render in the desktop app

The Sessions tab card for each session shows (top to bottom):

```
[★ minimax] [google] [claude]  [Positioning] [Devex] [CEO] [Designer] [Office Hours]   <closed badge>  Title…   32 turns   2 hr ago
coordinator: MiniMax / Positioning · project: agentictool-marketing
The last assistant turn's preview, or the coordinator summary once the session is closed.
[#wedge] [#positioning] [#compare-runtimes]
b1547c69-de23-445e-9a2e-32d4f20bef91
```

Where:

- **Runtime badges with `★`** mark the coordinator (anchor) runtime. Other badges are participants from cross-runtime dispatches.
- **Persona badges** are the distinct agent slugs across assistant turns, in first-spoken order. Hidden when no `--agent` was used.
- **Title** is `auto_title` when set, otherwise the title from creation. `untitled session` if both null.
- **Coordinator + project line** surfaces who anchors the session and which project it belongs to. The persona name appears when the session was created with `--as <slug>`.
- **Summary preview** is the last assistant turn while open, the coordinator's `summary` once closed.
- **Tags** are coordinator-generated, click to filter the Sessions list.
- **Session id (truncated)** appears on hover or full-row expand for direct CLI reference.

### The chat detail view (per-turn bubbles)

Each turn renders as a WhatsApp-style bubble:

- **Speaker label** — the persona name when set (`POSITIONING`), or the runtime display name for generalist turns (`MiniMax`).
- **Runtime pill** — small badge alongside the speaker showing which runtime answered underneath the persona (`minimax`, `google`, `claude`).
- **`@<target>` mention** — when the prompt was generated by the ATO orchestrator (e.g. `ato review --consensus`), the user-side bubble shows the addressee as `ATO Coordinator → @<runtime>`.
- **Bubble color** — runtime-tinted border + background so back-to-back turns from different LLMs visually contrast.
- **Timestamp + tool-call badge** — appears in the bubble header.

---

## Session discipline

The Sessions list is meant to be readable months later. Overload a session with off-topic dispatches and the row title, summary, and preview stop describing what's in it — the trail goes dark permanently.

Seven rules:

1. **One session per subject / decision / work block.** Sequential rounds of the same war-room belong in the same session (history replay is the value). Unrelated topics get separate sessions.

2. **Never re-open a closed session for a different topic.** `ato sessions reopen` is for genuinely continuing the same conversation — not for "scratch buffer."

3. **Smoke tests, schema verification, ack pings — always a separate throwaway session.** `ato sessions new --title "smoke test YYYY-MM-DD"` is one command; type it. Anything you'd regret seeing as the preview of a strategic session is the wrong dispatch to send there.

4. **Title and summary are part of the deliverable, not metadata.** A coordinator-generated summary of "Ack." because the last turn was a smoke test permanently degrades the row as a navigation artifact. Either keep the smoke-test dispatches out, or re-close the session with explicit context.

5. **Name sessions at creation time.** Convention: `<topic> war-room — <scope> <YYYY-MM-DD>`. Examples:
   - `PMF war-room — wedge / pitch / hero ratification 2026-05-16`
   - `Pricing war-room — tier collapse vs sign-in capture 2026-05-12`
   - `Security audit war-room — provider-keys path 2026-05-15`

6. **Multi-day decisions stay in one session; use tags for cadence.** Continuity of history beats date-bucketing. Tag rounds explicitly: `tags: ["round-1", "round-5", "ratified"]`.

7. **When in doubt, create a new session.** Sessions are free. Cluttering one is irreversible.

### Pre-dispatch checklist

Before sending a turn into an open session, ask:

> *Does this question belong in the session I'm about to target, or does it deserve a fresh one?*

If the answer is anywhere short of "yes, this is the same subject," create new.

### Pre-close checklist

Before closing a session, scroll the last 3-5 turns and confirm they would make a coherent summary. If not, dispatch one final "summarize this round in 80 words" turn so the coordinator has clean material to work with.

### Post-close sanity check

After closing, glance at the row in the Sessions list. The title + preview should describe the session in a way that's legible to someone who didn't run it. If it's not, the session needs work — either re-close with more context, or rename/restructure.

---

## Cross-runtime sessions

A session is anchored to one runtime (the `--runtime` you passed to `sessions new`), but **any runtime can dispatch a turn into it**. The history payload sent to each runtime is translated to that runtime's expected format:

- **OpenAI / Anthropic / OpenRouter** → `messages[]` with `role: "user" | "assistant"`.
- **Gemini** → `contents[]` with `role: "user" | "model"`, body as `parts: [{text}]`.
- **MiniMax** → `messages[]` with the same `user/assistant` shape, posted to `/v1/text/chatcompletion_v2` with a different success-check (`base_resp.status_code == 0`).
- **Claude / Codex / Gemini CLI runtimes** → translated to the CLI's native session-resume mechanism (e.g., Claude's `--resume`) when the anchor runtime matches; otherwise to history replay.

For a war-room with 5 seats across 4 model families, this is the primitive that makes the multi-LLM conversation coherent across providers. Without it, each runtime would start from zero on every turn.

---

## Programmatic + automation paths

- **CLI:** `ato sessions {new, list, get, close, reopen, delete}` covers the full lifecycle. All commands emit JSON by default; pass `--human` for terminal-friendly output. See `ato sessions --help`.
- **MCP server:** the standalone MCP server in `services/mcp-server/` exposes session operations as MCP tools. AI coding agents (Claude Code, Cursor agent mode) can drive sessions without leaving their harness.
- **Tauri commands (desktop):** `list_sessions_full`, `get_session_transcript`, `close_session_with_coordinator`, `reopen_session` — the same SQLite tables, exposed to the React frontend.

The data is the source of truth. CLI, MCP, and the desktop UI are three actor surfaces over the same `~/.ato/local.db`. Whichever you use, the audit trail lands in the same place.

---

## See also

- `README.md` — short version of this doc in the [Sessions section](../README.md#sessions-how-multi-turn-conversations-work).
- `.claude/skills/ato-warroom/SKILL.md` — war-room methodology layered on sessions (section 4a: seat types; section 4b: parallel vs sequential; section 4c: session discipline).
- `.claude/skills/ato-warroom/gstack-agents.sql` — template SQL for the 5 canonical gstack agent records.
- `AGENTS.md` — what coding agents need to know to drive ATO over CLI / MCP.
