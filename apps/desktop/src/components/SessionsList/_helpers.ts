// PR 5c follow-up (pr-reviewer Round-2 #1) — helpers extracted from
// SessionsList.tsx into this internal module so EphemeralDetailView
// can reuse them without creating an import cycle. The cycle was:
//
//   SessionsList.tsx  imports  EphemeralDetailView
//   EphemeralDetailView.tsx  imports  runtimeBadge/etc. from SessionsList
//
// ESM live-bindings make it work today because every helper use is at
// render time (no module-init dependency), but it bites HMR ordering
// and surprises the next reader. Moving the four helpers here means
// both consumers depend on `_helpers.ts` instead, breaking the cycle.
//
// The `_` prefix marks this as an internal-to-the-SessionsList-dir
// module; if anything in here grows into broader reusable utility it
// should move up to `apps/desktop/src/lib/`.

import { cn } from "@/lib/utils";

export const RUNTIME_COLORS: Record<string, string> = {
  claude: "text-orange-400 bg-orange-400/10",
  codex: "text-green-400 bg-green-400/10",
  gemini: "text-blue-400 bg-blue-400/10",
  google: "text-blue-400 bg-blue-400/10",
  hermes: "text-purple-400 bg-purple-400/10",
  openclaw: "text-cyan-400 bg-cyan-400/10",
  minimax: "text-pink-400 bg-pink-400/10",
  grok: "text-slate-400 bg-slate-400/10",
  deepseek: "text-indigo-400 bg-indigo-400/10",
  qwen: "text-amber-400 bg-amber-400/10",
  openrouter: "text-violet-400 bg-violet-400/10",
  anthropic: "text-orange-400 bg-orange-400/10",
};

export function runtimeBadge(rt: string) {
  return cn(
    "px-1.5 py-0.5 rounded text-xs font-medium capitalize",
    RUNTIME_COLORS[rt] || "text-cs-muted bg-cs-border"
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
