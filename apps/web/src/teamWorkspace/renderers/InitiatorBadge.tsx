// Wave 2 — web port of apps/desktop/src/components/InitiatorBadge.tsx.
// Drop-in re-implementation without react-i18next (web app has i18next in
// package.json but it's not configured — we use literal fallback labels
// directly so there's no translation call-site to trip over).

interface InitiatorBadgeProps {
  /** e.g. "human", "agent:claude", "agent:codex", "scheduler", "coordinator" */
  initiatorKind?: string | null;
  /** e.g. "cli", "desktop", "mcp_stdio", "cloud", "tick", "web" */
  clientSurface?: string | null;
  /** Stable id within the kind; shown on hover only. */
  initiatorId?: string | null;
  className?: string;
}

// Mirrors the desktop KIND_META table — emoji + display label.
const KIND_META: Record<string, { emoji: string; label: string }> = {
  human:            { emoji: '👤', label: 'Human' },
  agent:            { emoji: '🤖', label: 'Agent' },
  'agent:claude':   { emoji: '🤖', label: 'Claude' },
  'agent:codex':    { emoji: '🤖', label: 'Codex' },
  'agent:gemini':   { emoji: '🤖', label: 'Gemini' },
  'agent:openclaw': { emoji: '🤖', label: 'OpenClaw' },
  'agent:hermes':   { emoji: '🤖', label: 'Hermes' },
  coordinator:      { emoji: '🕸️', label: 'Coordinator' },
  scheduler:        { emoji: '⏰', label: 'Scheduler' },
  hook:             { emoji: '🪝', label: 'Hook' },
};

// Mirrors the desktop SURFACE_KEY table — compact suffix labels.
const SURFACE_LABEL: Record<string, string> = {
  cli:       'CLI',
  desktop:   'Desktop',
  mcp:       'MCP',
  mcp_stdio: 'MCP',
  cloud:     'Cloud',
  tick:      'Tick',
  web:       'Web',
};

function resolveKind(kind?: string | null): { emoji: string; label: string } {
  if (!kind) return { emoji: '•', label: 'Unknown' };
  const meta = KIND_META[kind];
  if (meta) return meta;
  // "agent:<custom>" — extract the name and title-case it.
  if (kind.startsWith('agent:')) {
    const name = kind.slice('agent:'.length);
    return { emoji: '🤖', label: name.charAt(0).toUpperCase() + name.slice(1) };
  }
  // Fallback: capitalize whatever we got.
  return { emoji: '•', label: kind.charAt(0).toUpperCase() + kind.slice(1) };
}

/**
 * Compact provenance pill: "🤖 Codex · CLI".
 * Surface suffix is omitted when unknown or absent.
 */
export default function InitiatorBadge({
  initiatorKind,
  clientSurface,
  initiatorId,
  className = '',
}: InitiatorBadgeProps) {
  const { emoji, label } = resolveKind(initiatorKind);
  const surface = clientSurface
    ? (SURFACE_LABEL[clientSurface] ?? clientSurface)
    : undefined;

  const titleParts = [
    initiatorKind ?? 'unknown',
    clientSurface ? `via ${clientSurface}` : null,
    initiatorId ? `(${initiatorId})` : null,
  ].filter(Boolean);

  return (
    <span
      title={titleParts.join(' ')}
      className={[
        'inline-flex items-center gap-1 rounded-full px-2 py-0.5',
        'bg-[#16161e] border border-[#2a2a3a] text-[10px] font-medium text-[#e8e8f0]',
        className,
      ].join(' ')}
    >
      <span aria-hidden>{emoji}</span>
      <span>{label}</span>
      {surface && (
        <>
          <span className="text-[#8888a0]" aria-hidden>·</span>
          <span className="text-[#8888a0]">{surface}</span>
        </>
      )}
    </span>
  );
}
