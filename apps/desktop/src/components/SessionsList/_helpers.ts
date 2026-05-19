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
  rowKind: "session" | "single_run" | "war_room" | "chat";
}

export interface SessionTurn {
  turnIndex: number;
  role: string;
  text: string;
  runtime: string;
  createdAt: string;
  // 2026-05-16 — null for generalist dispatches, slug otherwise.
  agentSlug: string | null;
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

// Runtimes offered in the New Session / Continue dropdowns. Mirrors
// the registry the CLI's dispatch path resolves through (CLI runtimes
// + the api_providers crate). Derived from the canonical runtime
// registry rather than hand-maintained — adding a runtime to
// lib/runtimes.ts populates this for free.
import { RUNTIME_IDS } from "@/lib/runtimes";
export const NEW_SESSION_RUNTIMES: string[] = RUNTIME_IDS as unknown as string[];
