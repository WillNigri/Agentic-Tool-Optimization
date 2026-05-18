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
