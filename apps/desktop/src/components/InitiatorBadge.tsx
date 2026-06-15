import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

interface InitiatorBadgeProps {
  /** e.g. "human", "agent:claude", "agent:codex", "scheduler", "coordinator" */
  initiatorKind?: string | null;
  /** e.g. "cli", "desktop", "mcp_stdio", "cloud", "tick" */
  clientSurface?: string | null;
  /** stable id within the kind; shown on hover only */
  initiatorId?: string | null;
  className?: string;
}

// kind → emoji + i18n key for the label. The wire form is either a bare
// kind ("human", "scheduler") or "agent:<name>" (see attribution.rs).
const KIND_META: Record<string, { emoji: string; labelKey: string }> = {
  human: { emoji: "👤", labelKey: "initiatorBadge.kind.human" },
  agent: { emoji: "🤖", labelKey: "initiatorBadge.kind.agent" },
  "agent:claude": { emoji: "🤖", labelKey: "initiatorBadge.kind.claude" },
  "agent:codex": { emoji: "🤖", labelKey: "initiatorBadge.kind.codex" },
  "agent:gemini": { emoji: "🤖", labelKey: "initiatorBadge.kind.gemini" },
  "agent:openclaw": { emoji: "🤖", labelKey: "initiatorBadge.kind.openclaw" },
  "agent:hermes": { emoji: "🤖", labelKey: "initiatorBadge.kind.hermes" },
  coordinator: { emoji: "🕸️", labelKey: "initiatorBadge.kind.coordinator" },
  scheduler: { emoji: "⏰", labelKey: "initiatorBadge.kind.scheduler" },
  hook: { emoji: "🪝", labelKey: "initiatorBadge.kind.hook" },
};

// client_surface → i18n key for the compact suffix label.
const SURFACE_KEY: Record<string, string> = {
  cli: "initiatorBadge.surface.cli",
  desktop: "initiatorBadge.surface.desktop",
  mcp: "initiatorBadge.surface.mcp",
  mcp_stdio: "initiatorBadge.surface.mcp",
  cloud: "initiatorBadge.surface.cloud",
  tick: "initiatorBadge.surface.tick",
  web: "initiatorBadge.surface.web",
};

function resolveKindMeta(kind?: string | null): { emoji: string; labelKey: string | null; fallback: string } {
  if (!kind) return { emoji: "•", labelKey: "initiatorBadge.kind.unknown", fallback: "Unknown" };
  const meta = KIND_META[kind];
  if (meta) return { emoji: meta.emoji, labelKey: meta.labelKey, fallback: meta.labelKey.split(".").pop() ?? "" };
  if (kind.startsWith("agent:")) {
    const name = kind.slice("agent:".length);
    const titled = name.charAt(0).toUpperCase() + name.slice(1);
    return { emoji: "🤖", labelKey: null, fallback: titled };
  }
  return { emoji: "•", labelKey: null, fallback: kind.charAt(0).toUpperCase() + kind.slice(1) };
}

/**
 * Compact provenance pill — who started a run and through which surface.
 * Renders like `🤖 Codex · CLI`. Surface suffix is dropped when unknown.
 *
 * Meant for Sessions / Mission / Loop detail rows; wiring into specific
 * pages is a follow-up.
 */
export default function InitiatorBadge({
  initiatorKind,
  clientSurface,
  initiatorId,
  className,
}: InitiatorBadgeProps) {
  const { t } = useTranslation();
  const { emoji, labelKey, fallback } = resolveKindMeta(initiatorKind);
  const label = labelKey ? t(labelKey, { defaultValue: fallback }) : fallback;

  let surface: string | undefined;
  if (clientSurface) {
    const surfaceKey = SURFACE_KEY[clientSurface];
    surface = surfaceKey ? t(surfaceKey, { defaultValue: clientSurface }) : clientSurface;
  }

  const title = [
    initiatorKind ?? t("initiatorBadge.kind.unknown", { defaultValue: "unknown" }),
    clientSurface ? t("initiatorBadge.via", { defaultValue: "via {{surface}}", surface: clientSurface }) : null,
    initiatorId ? `(${initiatorId})` : null,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <span
      title={title}
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-2 py-0.5",
        "bg-cs-bg-raised border border-cs-border text-[10px] font-medium text-cs-text",
        className,
      )}
    >
      <span aria-hidden>{emoji}</span>
      <span>{label}</span>
      {surface && (
        <>
          <span className="text-cs-muted" aria-hidden>·</span>
          <span className="text-cs-muted">{surface}</span>
        </>
      )}
    </span>
  );
}
