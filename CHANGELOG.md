# Changelog

All notable changes to ATO are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/).

## [2.18.9] — 2026-06-19

### Fixed
- **Team filter crash.** Filtering Sessions by "Team" (introduced in 2.18.8)
  threw `undefined is not an object (evaluating 'label.split')` and blanked the
  feed: `getTeamMembers` returned the raw backend rows where name/email are
  nested under `.user`, so the member avatars called `avatarInitials(undefined)`.
  Now maps members to a flat shape and `avatarInitials` is null-safe.

## [2.18.8] — 2026-06-19

### Added
- **Google-Drive-style unified team-shared cards.** A shared war-room / session /
  quick-chat now renders through the **same rich card** as a local one (full
  title, summary, tags, coordinator, distinct seat count + runtime badges, and a
  `👥 TEAM` badge + "shared by" + member avatars) instead of a sparse
  "untitled · read-only" card. The owner sees **one** card (their local card,
  team-badged) rather than a duplicate; recipients see the same rich card built
  from the cloud snapshot. Dedupe is status-aware so a shared (closed) snapshot
  still shows even when the local copy is still open. The sharer name + seat
  data are resolved server-side (closed repo) to keep the OSS client thin.

### Fixed
- **Real-time team participation now works end-to-end** (cloud). Recovered the
  append-event + live-WS-push loop and fixed three latent cross-service bugs
  (migration FK to a dropped table, gateway presence-token route shadowing, and
  the auth `x-user-id` header mapping) that had kept it from ever functioning.
  Append a turn on one machine → it appears live on every teammate's machine,
  both directions.

## [2.18.3] — 2026-06-17

### Added
- **Model A attribution (#85).** Every dispatch now records `member_id`
  (signed-in cloud user, decoded from the JWT) + `machine_id` (stable
  per-install id) on `execution_logs`, so a shared war-room/session can
  attribute each turn to {member, machine, runtime}. Stamped across all five
  production dispatch paths (CLI, API-provider, remote, replay, subagent).

### Fixed
- **CLI `teams delete/members/invite/remove` now accept a team slug (#86)** —
  they resolve slug→UUID like `share` already did, instead of 404/400ing when
  given a slug. Pairs with the new cloud `GET /teams/:id/members` route.

## [2.18.2] — 2026-06-17

### Added
- **Z.AI (Zhipu GLM) as a first-class API provider (#83).** OpenAI-compatible
  coding endpoint (`https://api.z.ai/api/coding/paas/v4`), env `ZAI_API_KEY`,
  default `glm-5.2`. Because both CLI and desktop dispatch share the
  `ato_api_providers` registry, z.ai has full parity: `ato dispatch zai`,
  war-room seats, sessions, desktop runtime/model pickers, desktop chat, BYOK
  key entry, and the OpenAI tool loop (read/write/grep/bash). Verified GLM
  pricing wired into cost tracking (glm-5.2 $1.4/$4.4, glm-4.6/4.5 $0.6/$2.2,
  glm-4.5-air $0.2/$1.1, flash models free).

## [2.18.1] — 2026-06-17

Security patch.

### Security
- **Mission `check_command` RCE (HIGH).** Success-criteria `check_command`s
  ran via `sh -c` under the assumption they were always operator-authored —
  but v2.15 Wave 4 team-shared missions can carry attacker-controlled commands
  that would execute on a victim's `ato missions tick`. Untrusted (non-`manual`
  origin) missions are now confined to a no-shell, allowlisted argv execution
  path; locally-authored missions keep full `sh -c` ergonomics. Fail-closed
  provenance gate (#82).

### Fixed
- CLI team-share errors are now self-diagnosing — 402 surfaces the required
  tier + upgrade URL; a route-missing 404 says so explicitly instead of an
  opaque `HTTP 404` (#81).
- Removed stray committed `node_modules` symlinks that broke `npm install` on
  CI / release builds; hardened `.gitignore` to ignore symlinked node_modules.

## [2.18.0] — 2026-06-15

The "every AI in one war room" release. Three clusters land together:
the read-only web Team Workspace, the browser ⇄ desktop tether, and a
team-management surface on the web. Plus subagent observability, a
war-room sweep verb, and a heap of QA fixes.

### Added

**v2.16 — Read-only web Team Workspace (#51, 6 waves)**
- New web client at `apps/web/` — sign in to your cloud account and
  browse every team-shared session, war-room, chat, loop, and mission
  in the browser.
- Snapshot renderers per kind with desktop parity: session turns,
  war-room seats with verdict badges, chat messages with role +
  initiator chips, loop step receipts, mission card metadata.
- Pagination on shared resources + Load-more on the events stream.
- Connection-state indicator pill (connected / reconnecting / offline)
  with sub-2s feedback on WS state changes.
- Mobile-responsive layout with accessible hamburger nav (focus
  trap, Escape close, aria-modal).
- Clickable execution-log receipts that open a detail drawer.
- Initiator + surface badges per-message (knowing *who* and *where*
  matters in multi-turn replays).

**v2.17 — Browser ⇄ Desktop tether (#57)**
- New Tokio task `tether_host.rs` runs alongside the desktop app:
  X25519 DH handshake + HKDF-SHA256 + XChaCha20-Poly1305 AEAD over a
  cloud-relayed WebSocket. The cloud sees only ciphertext.
- `TetherApprovalModal` shows the 12-character `browser_pubkey_fp`
  during pairing for shoulder-surfer defense — the browser displays
  the same fingerprint so users can visually verify before approving.
- "Allow always" mode persists to a local `tether_approvals` table
  (defense-in-depth: cloud-side `auto_approved=true` only proceeds if
  the desktop's local DB also has a matching row).
- Browser-side primitives in `apps/web/src/lib/tether/`: crypto
  (`@stablelib/*`), client state machine, decrypted event stream.

**v2.18.1 — Team management from the web (#59)**
- "+ New team" modal on the Team Workspaces page (was previously
  desktop-only).
- Per-team settings: rename, members list, invite by email + role,
  per-member role change, remove (with self-action gate so admins
  can't strand themselves), danger-zone delete with name-typing
  confirmation.
- New "Account" sidebar panel: profile (name, email, plan via
  `subscription_tier`, joined date), sign-out (with local clear
  fallback), honest "lives on desktop" note for LLM API keys /
  runtimes / skills / E2E team keys.

**v2.18 — Web UX redesign (#58)**
- Minimal centered sign-in card replaces the old hero+features+form
  stack. Marketing copy stays on agentictool.ai; this page is the
  dashboard entry point, not a duplicate landing.
- Onboarding rewritten as a 3-step setup: install SDK / save key as
  `.env` / wrap the client. Each step explains *why* (e.g. what
  `.env` is, why not paste in code, why to `.gitignore` it). Code
  block reads `process.env.ATO_API_KEY` with inline comments.
- Cost Dashboard date picker `<select>` redesigned with custom SVG
  chevron + dark theme; no more OS-default white chrome popping out.
- `docs/v2.18-active-workstation.md` — architecture doc for the next
  wave (browser drives the desktop's LLM dispatch over the v2.17
  tether channel).

**Observability — `ato subagent log` (#62, fka #71/#77/#78)**
- `ato subagent log create <prompt>` so Claude Code's Agent (Task)
  tool dispatches show up in execution_logs alongside outer-session
  work. Bracket pattern: `ato subagent log create` before, ATO logs
  the row, sub-agent runs, finish after.
- Canonical vocab gates: `--auth-mode <subscription|api_key|local>`,
  `--billing-surface <claude_code_subscription|anthropic_api|...>`,
  validated against canonical sets so PRO analytics can group across
  CLI / desktop / browser uniformly.
- `git_commit_sha` capture via bounded `dispatch::capture_git_head`
  (2-second timeout) so every subagent receipt is pinned to its
  source state.
- UTF-8-safe truncation for prompt + response logging (the same bug
  also lived in `dispatch.rs`; #77 ported the fix).

**Operations — `ato war-rooms sweep` (#61, fka #70)**
- New CLI verb that scans `execution_logs` for war-rooms idle longer
  than `--idle-minutes`, excludes already-closed ones, and runs the
  same coordinator-summary close orchestrator on up to
  `--max-per-run` of them per invocation.
- Default `--coordinator google` (free quota, cheap summarization).
- `--dry-run` prints candidates without closing.
- All flags validated at the clap layer; single JSON envelope per
  invocation for downstream tooling.
- Designed to wire into launchd / cron so one-shot R1 multi-LLM
  reviews self-close once seats land — removes the manual `ato
  war-rooms close <id>` step that was the #1 UX trap.

**Models — Google deprecated-id filter (#60)**
- `packages/ato-list-models/src/providers.rs` now drops
  known-deprecated Google model ids before the chat picker sees them
  (initial list covers `gemini-2.0-flash-001`, `-lite-001`, plus the
  retired gemini-1.x family).
- Explicit-id deny-list only — glob/prefix matching would shadow
  live successors (e.g. `gemini-2.0-flash` is live).

### Changed

- README "Why ATO" section refreshed: the "no hands" runtimes are
  API providers without a first-party coding agent (Grok / MiniMax /
  DeepSeek / Qwen / GLM / Yi / Kimi), not Hermes / OpenClaw which
  already ship their own tool layers.
- All versions aligned to 2.18.0 across `Cargo.toml`,
  `package.json`, and `tauri.conf.json`.

### Fixed

- Desktop's Cost Dashboard date picker no longer renders OS-default
  white chrome on dark theme.
- Browser sign-in screen no longer dumps marketing copy + hero + 3
  feature cards on top of the sign-in card.
- Dispatch + subagent log truncation now walks UTF-8 boundaries
  instead of panicking on multi-byte chars.
- War-rooms appear in the Sessions feed automatically when the new
  `sweep` verb runs from cron — used to require a manual close call.

### Known follow-ups (deferred)

- Loop Composer node-kind coverage: `diagnose` / `apply` / `review` /
  `war_room` / `score` / `input` / `output` still write
  `status="skipped"`. Tracked as v2.14.1; only the `dispatch` and
  `methodology_run` kinds actually execute today.
- v2.18 Wave 1 — browser-driven dispatch over the tether channel.
  The v2.17 tether currently carries the `decrypt_events` frame
  only; `dispatch_request` / `dispatch_chunk` / `dispatch_complete`
  are scoped in the architecture doc and queued for the next wave.
- CLI runtimes (claude / codex / gemini-CLI) don't have an in-chat
  model picker yet — `--model` pass-through to the binary is queued.
- PRO-tier buttons (Create Team, Invite) still render for free-tier
  users — server rejects with 403; client-side gate on
  `subscription_tier !== 'free'` is queued as a polish.

---

[2.18.0]: https://github.com/WillNigri/Agentic-Tool-Optimization/releases/tag/v2.18.0
