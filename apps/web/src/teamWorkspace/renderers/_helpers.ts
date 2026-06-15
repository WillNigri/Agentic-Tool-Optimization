// Wave 2 — shared helpers for per-kind snapshot renderers.
// No cs-* Tailwind tokens in the web app; we translate the desktop
// palette to standard Tailwind + hex classes that the web's config knows.

// ──────────────────────────────────────────────────────────────────
// Time
// ──────────────────────────────────────────────────────────────────

/**
 * Short human-readable relative time for inline badges.
 * Falls back gracefully for future/unparseable dates.
 */
export function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    const now = Date.now();
    const diffMs = now - d.getTime();
    const diffSec = Math.floor(diffMs / 1000);
    const diffMin = Math.floor(diffSec / 60);
    const diffHr = Math.floor(diffMin / 60);
    const diffDay = Math.floor(diffHr / 24);

    if (diffSec < 60) return 'just now';
    if (diffMin < 60) return `${diffMin}m ago`;
    if (diffHr < 24) return `${diffHr}h ago`;
    if (diffDay === 1) return 'yesterday';
    if (diffDay < 7) return `${diffDay}d ago`;

    // Older than a week — show "Jun 14, 4:32 PM" style.
    return d.toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  } catch {
    return iso;
  }
}

// ──────────────────────────────────────────────────────────────────
// Runtime chip — mirrors desktop RUNTIME_REGISTRY tw values but using
// standard Tailwind only (no cs-* tokens in the web build).
// ──────────────────────────────────────────────────────────────────

const RUNTIME_TW: Record<string, string> = {
  claude:     'text-orange-400 bg-orange-400/10',
  codex:      'text-green-400 bg-green-400/10',
  gemini:     'text-blue-400 bg-blue-400/10',
  openclaw:   'text-cyan-400 bg-cyan-400/10',
  hermes:     'text-purple-400 bg-purple-400/10',
  minimax:    'text-pink-400 bg-pink-400/10',
  grok:       'text-slate-400 bg-slate-400/10',
  deepseek:   'text-indigo-400 bg-indigo-400/10',
  qwen:       'text-amber-400 bg-amber-400/10',
  openrouter: 'text-violet-400 bg-violet-400/10',
};

/**
 * Tailwind class string for a runtime chip.
 * Falls back to a muted treatment for unknown runtimes.
 */
export function runtimeChipClass(runtime: string | null): string {
  if (!runtime) return 'text-[#8888a0] bg-[#2a2a3a]';
  const tw = RUNTIME_TW[runtime.toLowerCase()];
  return (
    'px-1.5 py-0.5 rounded text-xs font-medium capitalize ' +
    (tw ?? 'text-[#8888a0] bg-[#2a2a3a]')
  );
}

// ──────────────────────────────────────────────────────────────────
// Agent slug chip
// ──────────────────────────────────────────────────────────────────

/**
 * Tailwind class for the agent_slug (persona) chip.
 * Cyan/mint tinted — same accent as the web palette.
 */
export function personaChipClass(): string {
  return 'px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-[#00FFB2]/10 text-[#00FFB2] border border-[#00FFB2]/20';
}

/**
 * Turn "office-hours" → "Office Hours". Matches desktop personaDisplay().
 */
export function personaDisplay(slug: string): string {
  return slug
    .split('-')
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(' ');
}

// ──────────────────────────────────────────────────────────────────
// Role-based bubble styling (user / assistant / error)
// ──────────────────────────────────────────────────────────────────

/**
 * Border + background Tailwind classes for a message/turn bubble,
 * keyed by the `role` field (or a status of "error").
 */
export function roleStyleClass(role: string): string {
  switch (role) {
    case 'user':
      return 'border border-[#2a2a3a] bg-[#16161e]';
    case 'assistant':
      return 'border border-[#2a2a3a]/60 bg-[#16161e]/60';
    case 'error':
      return 'border border-red-500/40 bg-red-500/10';
    case 'system':
    case 'attachment':
      return 'border border-[#2a2a3a]/40 bg-[#0a0a0f]/60';
    default:
      return 'border border-[#2a2a3a]/60 bg-[#16161e]/60';
  }
}
