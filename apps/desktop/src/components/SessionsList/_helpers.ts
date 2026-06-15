// PR 5c follow-up (pr-reviewer Round-2 #1) — helpers extracted from
// SessionsList.tsx into this internal module so SingleRunDetailView
// can reuse them without creating an import cycle. The cycle was:
//
//   SessionsList.tsx  imports  SingleRunDetailView
//   SingleRunDetailView.tsx  imports  runtimeBadge/etc. from SessionsList
//
// ESM live-bindings make it work today because every helper use is at
// render time (no module-init dependency), but it bites HMR ordering
// and surprises the next reader. Moving the four helpers here means
// both consumers depend on `_helpers.ts` instead, breaking the cycle.
//
// The `_` prefix marks this as an internal-to-the-SessionsList-dir
// module; if anything in here grows into broader reusable utility it
// should move up to `apps/desktop/src/lib/`.
//
// 2026-05-18 — RUNTIME_COLORS used to live here as one of TEN copies
// of the same map across the codebase. Now sources from the single
// runtime registry (`lib/runtimes.ts`). Re-exported as RUNTIME_COLORS
// (a Record<string, string> facade) so existing call sites in this
// directory don't need to flip imports en masse.

import { cn } from "@/lib/utils";
import { RUNTIME_REGISTRY, runtimeTw } from "@/lib/runtimes";

/** Facade matching the legacy shape. Read-only — callers should
 *  prefer `runtimeTw(rt)` for safer fallback handling. */
export const RUNTIME_COLORS: Record<string, string> = Object.fromEntries(
  Object.entries(RUNTIME_REGISTRY).map(([id, meta]) => [id, meta.tw]),
);

export function runtimeBadge(rt: string) {
  return cn(
    "px-1.5 py-0.5 rounded text-xs font-medium capitalize",
    runtimeTw(rt),
  );
}

export function formatTime(iso: string) {
  return new Date(iso).toLocaleString();
}

// 2026-05-16 — persona slug → human label. "positioning" → "Positioning",
// "office-hours" → "Office Hours". Falls back to capitalized slug for
// custom personas users define (security-specialist → "Security Specialist").
export function personaDisplay(slug: string): string {
  return slug
    .split("-")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

// Persona-badge styling for the SessionsList card cluster + chat-bubble
// role labels. Uses a single cyan-tinted treatment so the cluster reads
// as "these are the named seats that spoke" without competing with the
// per-turn runtime badges.
export function personaBadge(): string {
  return "px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-cs-accent/10 text-cs-accent border border-cs-accent/20";
}

// 2026-05-18 — elegance push #2. The following types + helpers used to
// live inline in SessionsList.tsx; moving them here so SessionTranscriptView
// and NewSessionModal can also import them without re-declaring or
// reaching back into the parent.

// 2026-05-19 — elegance push: SessionListRow moved out of SessionsList.tsx
// so the SessionCards/ split can import it without reaching back. The row
// is the shape `list_sessions_full` returns from the Tauri backend; one row
// for every kind of conversation in the unified feed.
export interface SessionListRow {
  id: string;
  runtime: string;
  agentSlug: string | null;
  title: string | null;
  createdAt: string;
  lastUsedAt: string;
  turnCount: number;
  runtimesUsed: string[];
  agentsUsed: string[];
  totalCostUsd: number | null;
  lastAssistantPreview: string | null;
  status: string;
  closedAt: string | null;
  autoTitle: string | null;
  summary: string | null;
  tags: string[];
  projectId: string | null;
  projectName: string | null;
  category: string | null;
  team: string | null;
  /** v2.7.13 — coordinator runtime that summarized this conversation
   *  on its most recent close. Drives the COORD badge on the list
   *  card so a glance shows who summarized. NULL when never closed. */
  coordinatorRuntime: string | null;
  /** v2.7.13 — free-form human note attached at close time. Surfaced
   *  on the list card so users see their own framing without having
   *  to drill in. NULL when no comment was attached. */
  humanComment: string | null;
  /** v2.7.14 — stable anchor runtime for chats (the WhatsApp-row
   *  "this chat is with claude" identity). Distinct from `runtime`
   *  which can flip per-message. NULL for sessions / war-rooms /
   *  single-runs (anchor concept only applies to chat threads). */
  anchorRuntime: string | null;
  rowKind:
    | "session"
    | "single_run"
    | "war_room"
    | "chat"
    | "eval_cluster"
    // teamfilter (#1) — cloud-shared rows merged into the feed when the
    // Team filter chip is picked. Carried through the same SessionListRow
    // pipeline as local rows; the team-only fields below are populated and
    // the click handler routes them to a placeholder read-only view (#6
    // replaces the placeholder). Synthetic id: `shared:<teamId>:<originalId>`.
    | "team_shared_session"
    | "team_shared_war_room"
    | "team_shared_chat";
  /** teamfilter (#1) — ISO timestamp the row was shared with the team.
   *  Drives the shared_at-DESC sort for the Team feed and the relative
   *  time on the card. NULL/undefined for local rows. */
  sharedAt?: string | null;
  /** teamfilter (#1) — display label for the 'Shared by X' pill. Today
   *  the sharer's user id (no name on the shared-* payloads yet); #6 can
   *  upgrade this to a resolved display name. */
  sharedByLabel?: string | null;
  /** teamfilter (#1) — the cloud team this row was shared into. Shown on
   *  the Team badge and used to rebuild the original id from the synthetic
   *  one. */
  sharedTeamId?: string | null;
  sharedTeamName?: string | null;
  /**
   * v2.10.0 PR-1 (UI) — eval-cluster only. When `rowKind === "eval_cluster"`,
   * this synthesized row stands in for N consecutive single_run rows that
   * share the same prompt (typical signal of a methodology eval, like the
   * v2.9 Part 5 n=150 sweep). The user can expand the cluster to see the
   * individual receipts. NULL for non-cluster rows.
   */
  clusterCount?: number;
  /** v2.10.0 PR-1 (UI) — for `rowKind === "eval_cluster"`: the original
   *  single_run rows the cluster groups. Used by the expand/collapse UI
   *  to render the individual receipts without re-fetching from the DB. */
  clusterMembers?: SessionListRow[];
  /** v2.10.0 PR-1 (UI) — aggregated total cost across cluster members.
   *  Pre-summed at cluster-time so the card can render without iterating
   *  members every paint. */
  clusterTotalCostUsd?: number | null;
}

/**
 * v2.10.0 PR-1 (UI) — cluster consecutive single_run rows with the same
 * prompt into a single synthetic `eval_cluster` row.
 *
 * The Runs tab was getting drowned (Will reported 2026-05-24 after the
 * n=150 Part 5 eval landed 150 identical-looking SINGLE RUN cards in
 * the feed). This helper folds the noise into one expandable card per
 * eval, surfacing the aggregate: count, total cost, runtime mix.
 *
 * Algorithm (intentionally simple — same-prompt + same-runtime,
 * order-preserving):
 *
 *   1. Walk rows in display order (most-recent-first).
 *   2. For each run of ≥ `minClusterSize` consecutive single_run rows
 *      with the same title (prompt preview) AND the same runtime,
 *      emit a single synthesized eval_cluster row in their place.
 *   3. Sub-cluster-size runs pass through unchanged.
 *
 * Why same-prompt + same-runtime (not same-prompt only): a customer
 * running a multi-model methodology will have interleaved claude /
 * gemini / openai dispatches for the same prompt. Clustering across
 * runtimes would hide the per-model breakdown that's the WHOLE POINT
 * of a methodology eval. The methodology runner's own composer surface
 * (v2.10 PR-3) shows the cross-runtime aggregate; the Runs tab cluster
 * is for the within-runtime noise reduction.
 *
 * @param rows  source rows (any rowKind mix), display order
 * @param minClusterSize  minimum consecutive same-prompt runs to fold
 *                        into a cluster. Default 3 — below that, the
 *                        individual cards are still readable. Above 3
 *                        is where the feed starts to drown.
 */
export function clusterEvalRuns(
  rows: SessionListRow[],
  minClusterSize: number = 3,
): SessionListRow[] {
  const out: SessionListRow[] = [];
  let i = 0;
  while (i < rows.length) {
    const row = rows[i];
    if (row.rowKind !== "single_run") {
      out.push(row);
      i += 1;
      continue;
    }
    // Greedy: find the longest run of consecutive same-prompt + same-
    // runtime single_runs starting at i.
    let j = i + 1;
    while (
      j < rows.length &&
      rows[j].rowKind === "single_run" &&
      rows[j].title === row.title &&
      rows[j].runtime === row.runtime
    ) {
      j += 1;
    }
    const runLen = j - i;
    if (runLen >= minClusterSize) {
      const members = rows.slice(i, j);
      const totalCost = members.reduce(
        (acc, m) => acc + (m.totalCostUsd ?? 0),
        0,
      );
      // Synthesize one cluster row using the FIRST member's metadata
      // as the anchor (so the timestamp / runtime / agent badges read
      // sensibly). The cluster's `id` borrows the first member's id
      // so React keys stay stable across re-renders.
      out.push({
        ...row,
        id: `cluster:${row.id}`,
        rowKind: "eval_cluster",
        clusterCount: runLen,
        clusterMembers: members,
        clusterTotalCostUsd: totalCost,
        // lastAssistantPreview for the cluster card shows aggregate
        // metadata, not the first member's response (which would be
        // misleading — every member has a different response).
        lastAssistantPreview: `${runLen} dispatches · same prompt · total $${totalCost.toFixed(4)}`,
      });
      i = j;
    } else {
      // Below threshold — pass through the run unchanged.
      for (let k = i; k < j; k += 1) {
        out.push(rows[k]);
      }
      i = j;
    }
  }
  return out;
}

export interface SessionTurn {
  turnIndex: number;
  role: string;
  text: string;
  runtime: string;
  createdAt: string;
  // 2026-05-16 — null for generalist dispatches, slug otherwise.
  agentSlug: string | null;
  // v2.17 — per-message attribution. Each turn stamps its own
  // initiator at send time so live-attached agents that take over
  // mid-conversation surface correctly per-turn rather than
  // inheriting the session-open initiator.
  initiatorKind?: string | null;
  clientSurface?: string | null;
  initiatorId?: string | null;
}

// 2026-05-16 — cost-receipts panel data shape, mirrors the backend
// SessionCostBreakdown / SessionCostRow.
export interface SessionCostRow {
  runtime: string;
  agentSlug: string | null;
  totalTurns: number;
  successfulTurns: number;
  tokensIn: number | null;
  tokensOut: number | null;
  totalDurationMs: number | null;
  costNullTurns: number;
  totalCostUsd: number;
  // 2026-05-16 — from execution_logs.auth_mode (authoritative per-row)
  // with a runtime-string fallback for pre-auth-mode rows.
  billingMode: string; // "subscription" | "api_key" | "local"
}

export interface SessionCostBreakdown {
  sessionId: string;
  totalCostUsd: number;
  totalTurns: number;
  totalTokensIn: number;
  totalTokensOut: number;
  totalDurationMs: number;
  rows: SessionCostRow[];
}

export interface SessionTranscript {
  id: string;
  runtime: string;
  agentSlug: string | null;
  title: string | null;
  turns: SessionTurn[];
  status: "open" | "closed";
  closedAt: string | null;
  autoTitle: string | null;
  summary: string | null;
  tags: string[];
  projectId: string | null;
  /** v2.7.12 — free-form human note attached at close time. Surfaced
   *  in the closed-session summary card alongside the coordinator's
   *  auto-generated summary. Null when the user closed without typing
   *  one, or when the session pre-dates the column. */
  humanComment: string | null;
}

export interface CloseSessionResult {
  id: string;
  status: string;
  autoTitle: string | null;
  summary: string | null;
  tags: string[];
  projectId: string | null;
  coordinatorRuntime: string;
  coordinatorModel: string | null;
  durationMs: number;
  /** v2.7.12 — echo of the human_comment that was just persisted
   *  (trimmed, empty → null). */
  humanComment: string | null;
}

// Pretty-name lookup for runtimes. Used in chat-bubble sender labels
// where "google" or "minimax" alone is opaque. Pairs with the model
// when known (e.g. "Google AI · Gemini 2.5 Flash"). Falls back to the
// capitalized runtime slug for unknown values.
//
// Different from `runtimeLabel(rt)` in lib/runtimes.ts — the registry
// label is the SHORT form for badges ("Claude", "Codex"); this is the
// LONG form for chat-bubble sender lines ("Claude", "OpenAI Codex").
// Keeping both is intentional — badges and sender labels have
// different density constraints.
const RUNTIME_DISPLAY: Record<string, string> = {
  claude: "Claude",
  codex: "OpenAI Codex",
  gemini: "Google Gemini",
  google: "Google Gemini",
  hermes: "Hermes",
  openclaw: "OpenClaw",
  minimax: "MiniMax",
  grok: "xAI Grok",
  deepseek: "DeepSeek",
  qwen: "Qwen",
  openrouter: "OpenRouter",
  anthropic: "Anthropic",
};

export function runtimeDisplay(rt: string): string {
  return RUNTIME_DISPLAY[rt] ?? rt.replace(/^[a-z]/, (c) => c.toUpperCase());
}

// Heuristic to detect when a `user`-role turn was authored by the
// `ato review` orchestrator (or another scripted dispatch) versus a
// human-typed prompt. The orchestrator's prompts have a predictable
// opener — "# Code review request for `<runtime>`" or "<runtime> —
// consensus round." — that we lean on to flip the rendered sender from
// "You" to "ATO Coordinator → @<addressee>". Best-effort: if neither
// pattern matches, treat as human input. (No false positives observed
// for human prose in 2026-05-15 dogfooding, but the regex is narrow
// enough to fix if one shows up.)
export function inferCoordinatorTarget(text: string): string | null {
  const m1 = text.match(
    /^\s*#\s*Code review request for\s+`([a-z][a-z0-9_-]*)`/i
  );
  if (m1) return m1[1];
  const m2 = text.match(
    /^\s*([a-z][a-z0-9_-]*)\s+—\s+consensus round/i
  );
  if (m2) return m2[1];
  return null;
}

// Two-letter avatar from the speaker label. "MiniMax" → "Mi",
// "Google Gemini" → "GG", "ATO Coordinator" → "AC". Easier to scan
// in a chat list than a generic robot icon.
export function avatarInitials(label: string): string {
  const words = label.split(/\s+/).filter(Boolean);
  if (words.length >= 2) {
    return (words[0][0] + words[1][0]).toUpperCase();
  }
  return label.slice(0, 2).toUpperCase();
}

// Runtimes offered in the New Session / Continue dropdowns. The full
// registry from lib/runtimes.ts includes CLI runtimes whose session
// story isn't wired yet — `ato sessions` only resumes runtimes that
// either (a) maintain conversation state themselves and ATO can hand
// them a resume token (claude today) OR (b) are stateless API
// providers whose history ATO replays per turn (the api_providers
// crate — minimax/grok/deepseek/qwen/openrouter/anthropic/google).
//
// Codex + Gemini still need their resume-flag wiring + signing-cert
// dance; Hermes + OpenClaw have no session story yet. The backend
// `supported_runtimes()` in apps/cli/src/commands/sessions.rs is the
// source of truth — filter mirrors that list. Will caught this
// during dogfood when the modal offered codex and `ato sessions new`
// errored with "Runtime 'codex' is not yet supported".
import { RUNTIME_IDS } from "@/lib/runtimes";

/** Runtimes the `ato sessions` backend currently resumes. After
 *  `da3b01f` (2026-05-19), every CLI runtime + every api_provider
 *  is supported via history-replay (or native --resume for claude).
 *  Mirrors `supported_runtimes()` in apps/cli/src/commands/sessions.rs.
 *
 *  The set stays here (empty) so future runtimes that the backend
 *  hasn't wired yet have a single place to land — add the slug, the
 *  modal disables it with the SESSION_UNSUPPORTED_REASON tooltip.
 *  Codex unlock verified end-to-end via dispatch on 2026-05-19. */
export const SESSION_UNSUPPORTED_RUNTIMES = new Set<string>([]);

/** Human-readable note per unsupported runtime — surfaced as the
 *  disabled-option tooltip in NewSessionModal. Currently empty
 *  because every runtime is supported. Future entries follow the
 *  format `<slug>: "Reason — when it lands"`. */
export const SESSION_UNSUPPORTED_REASON: Record<string, string> = {};

/** All runtimes from the registry, in display order. NewSessionModal
 *  uses this and styles the unsupported ones as disabled with
 *  SESSION_UNSUPPORTED_REASON as the tooltip. */
export const NEW_SESSION_RUNTIMES: string[] =
  RUNTIME_IDS as unknown as string[];
