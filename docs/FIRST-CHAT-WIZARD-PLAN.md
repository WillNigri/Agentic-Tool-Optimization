# First Chat Wizard — onboarding plan

> **Decision (Will, 2026-05-18):** the wizard's purpose flips. v1.3.0
> shipped "Create Agent" as the primary onboarding verb; in practice
> ATO's biggest value is the multi-LLM conversation (war-rooms +
> sessions). The new onboarding wizard puts that experience FIRST.
> Agent creation becomes downstream of "I'm in a war-room and want
> a persistent persona for it."
>
> **War-room verdict (2026-05-18, 4 seats × 3 LLM families,
> war_room_id `258F1FDA…`):** unanimous `[REFINE]`. ceo + positioning
> + designer + office-hours independently converged on cutting
> phases B/C and trimming A. The 5-phase plan was 50-65s of setup
> ceremony BEFORE the demo proof point lands; the trimmed flow is
> ~25s from launch to first multi-LLM reply. The Loom IS the wedge;
> every phase that doesn't appear in the 60-second screen recording
> is dead weight.
>
> **PR 1 — refined shape (post war-room):**
> 1. Home primary CTA: "Start a war-room". Single verb, no chooser.
> 2. Click → screen with prompt input + small counter "Firing to
>    N LLMs · add another". Silent auth detection only.
> 3. `Send` → mint war_room_id, parallel dispatch to ALL enabled
>    runtimes (no chip-picker, no persona-picker, no title).
> 4. Land on WarRoomDetailView (PR 14c) showing N replies as cards.
>
> **Cut from PR 1, queued for PR 2+:**
> - Session-vs-war-room chooser (session becomes a "Want to
>   refine with one LLM?" link revealed AFTER first reply).
> - Per-runtime persona + title pickers.
> - Phase A upsell copy / "encourage 2+".
> - Phase D agent scaffolding (own PR).
>
> **Falsifier metrics:** 60-second Loom must clear 500 X views in
> 48h (wedge test) AND the same viewers must install ATO within
> 7 days (demo conversion test). Views-high + installs-low =
> wedge resonates but demo doesn't convert (different fix).

## What the user sees

### Phase A — Auth gate

A clear, low-friction check: *do you have at least one LLM available?*

- Detect via:
  - CLI subscriptions present and authenticated (`claude --print`, `codex --print`, `gemini -p`, etc.)
  - API keys configured in `llm_api_keys` (existing path)
  - Local Ollama on `localhost:11434` (existing path)
- **Surface counts**: "You have 2 LLMs available: Claude (subscription), MiniMax (API key)."
- **Encourage 2+**: "ATO is at its best with 3+ LLMs to compare across. Add another?" with `Add API key` + `Connect CLI` shortcuts.
- **Block proceed when 0**: route to Settings → Runtimes / API Keys, return when one connects.
- **Allow proceed with 1**: warn but don't block. Session works fine with 1; war-room needs 2+ but we'll explain that in Phase B.

### Phase B — Pick the conversation shape

Two cards, side-by-side, with the language straight out of the README + SKILL.md docs we shipped this morning:

| | **Session** (sequential) | **War room** (parallel) |
|---|---|---|
| **Card headline** | One AI at a time. Multi-turn. | Many AIs at once. Independent replies. |
| **Subhead** | "Talk to one LLM. Add others later as turns. Each new turn sees prior turns." | "Send the same prompt to 2+ LLMs. Each replies independently — no anchoring bias." |
| **Best for** | Refinement, iteration, debugging | Comparison, falsification, scoping |
| **Min LLMs** | 1 | 2 |

Picking war-room when only 1 LLM is connected surfaces an inline upsell to add another (same hooks as Phase A).

### Phase C — Pick participants

Different shape per choice.

**Session:**
- One **anchor runtime** picker (chips with runtime icons, only enabled runtimes selectable).
- Optional **persona** picker (their existing agent roster, scoped to the anchor runtime). Default to generalist.
- Optional **title** field (auto-suggested from a 1-line tag like "First chat 2026-05-18").

**War room:**
- Pick **2+ runtimes** from a chip cluster. Each chip shows runtime + LLM model + auth source. Required: at least 2 distinct runtimes.
- Optional **per-runtime persona** picker. Default: each seat is a generalist of its runtime.
- Optional **title** field.

### Phase D — (Conditional) Agent scaffolding for empty rosters

If the user has 0 agents OR fewer than 3 (war-room mode), offer one-click creation of a starter set:

| Starter | What it does |
|---|---|
| `pr-reviewer` (claude) | Reviews code diffs with the pr-reviewer system prompt we already use internally |
| `positioning` (minimax or claude) | Strategic / wedge / pitch lens (existing gstack agent shape) |
| `security-specialist` (claude or codex) | OWASP / threat-model lens |
| `devex` (google or claude) | Developer experience lens |
| `ceo` (claude) | Synthesis seat for war-room R2 rounds |

Each starter writes the runtime file (per existing `ato agents create` path) + creates the SQLite agent record. The user can skip this step and just dispatch generalist seats.

### Phase E — First prompt

A simple text input with placeholder copy that anchors the verb:

- Session: "Ask your first question — Claude will answer, you'll see the receipt"
- War room: "Ask one question — all 3 LLMs will answer independently. See whose answer fits the question best."

`Send` → either creates the session and dispatches the first turn, OR mints a war_room_id and fires N parallel dispatches.

Both paths land the user in:
- Session: `SessionTranscriptView` (existing) showing the first turn live.
- War room: `WarRoomDetailView` (PR 14c shipped this morning) showing the N replies as cards.

### Phase F — Always closeable

- Big visible "Close" button in the chat surface (already there for sessions — closes with coordinator summary; war-rooms have no close, that's by design).
- "Back to Home" link in the top-left.
- The session/war-room persists in the Sessions tab regardless — closing is a deliverable thing (auto-title + summary + tags), not a discard.

## What this is NOT

- NOT a replacement for the existing `CreateAgentWizard`. That stays. It just stops being the primary Home CTA. The first-chat flow becomes the primary CTA; "Create agent" demotes to a secondary action in the Agents tab.
- NOT a new SQL surface. Reuses `sessions` + `execution_logs.war_room_id` + `agents` tables already shipped.
- NOT a new CLI surface. The wizard is a desktop-only flow. CLI users already have `ato sessions new` + `ato dispatch --war-room-id $WR`.

## What ships in PR 1

Minimum viable wizard that demonstrates the pitch end-to-end:

1. New `FirstChatWizard` component at `apps/desktop/src/components/FirstChatWizard/index.tsx` with the 5 phases above.
2. Replace the Home page's primary CTA from "Create Agent" → "Start your first chat" (Create Agent demotes to secondary). Verify the existing CreateAgentWizard still works as a secondary path.
3. Auth-detection logic — reuse existing runtime-health + llm_api_keys queries.
4. Two card-based pickers (shape, participants).
5. Wire the session-create path: reuse `create_session` Tauri command. Wire the war-room path: mint UUID client-side, dispatch N parallel via existing `--war-room-id` infrastructure.
6. Land the user in the existing transcript / war-room detail view.

## What's deferred to PR 2+

- Agent scaffolding (Phase D). PR 1 just shows "you have 0 agents — that's fine, you can dispatch as generalist." Scaffolding is a follow-up.
- Polish/animations/copy iteration. PR 1 is functional; PR 2 is designer-polish.
- A11y audit (focus management across phases, keyboard nav, aria-live for dispatch progress).
- Telemetry to measure drop-off per phase (Pro feature).

## Open questions

1. **What happens if user already has prior sessions?** Show the Home page normally? Or still show First-Chat as the CTA because "starting another chat" is the primary action? My instinct: First-Chat stays primary, but the Home page also shows a "Recent" row underneath so users with prior sessions get a quick re-entry.
2. **Should war-room mode require ≥2 *distinct* runtimes, or could it run 2 Claude seats with different personas?** Per the README/SKILL.md, war-rooms are about *independent priors*. Same runtime ≠ independent priors. Recommendation: require 2+ distinct runtime *families* (Claude family, Codex family, MiniMax family, Google family, etc.). But this might be too strict for the first-chat case where users may only have 1 family configured.
3. **Default model per runtime in war-rooms.** Each runtime has a default; should the wizard expose model selection in Phase C, or just use defaults and let the user customize later in the dispatch?
4. **Empty-state copy on the war-room card when only 1 LLM is available.** Disable the card with a soft prompt to add another LLM? Or always show enabled and let the click trigger the upsell?

## Karpathy filter

- **Wrong assumptions**: PR 1's scope assumes the war-room R1-parallel methodology is what users want for their first compare-LLMs experience. Sessions might be the better first impression (less infrastructure, single LLM, conversational). Consider making session the default in Phase B.
- **Overcomplexity**: 5 phases is a lot for a first-run. Phase D (agent scaffolding) and Phase A (auth encouragement beyond the minimum) could push to PR 2 if the core 3-phase flow ships sooner.
- **Orthogonal edits**: do NOT change the existing CreateAgentWizard internals; it stays. Home page CTA swap is the only Home edit.
- **Imperative over declarative**: The wizard should drive actions, not collect parameters. Each phase has ONE action ("connect another LLM", "pick a shape", "pick LLMs", "ask a question") — not a form-fill.

## The CEO question

Is THIS the demo? *"Open ATO, see Claude + MiniMax + Codex connected, click Start, type 'critique my pitch', see 3 replies side-by-side, share the screenshot"* — is that the 60-second flow that wins the office-hours falsifier (500 X views in 48h on a Loom)?

If yes, PR 1's success criterion isn't "feature works" — it's "the flow takes ≤60 seconds from app launch to the first multi-LLM reply rendering."
