// PresencePills — small row of pills shown at the top of a detail view
// to tell the user who else is currently looking at this resource.
//
// Pure read-only — does NOT report cursor, does NOT emit join (the
// caller that wants the pill should also mount LiveCursors or use
// usePresence directly for cursor tracking). When the user is the
// only viewer the component renders nothing so the header stays clean.

import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";
import { usePresence } from "./usePresence";
import type { PresenceResourceKind } from "@/lib/meshRelay";

interface PresencePillsProps {
  resourceKind: PresenceResourceKind;
  resourceId: string;
  viewerLabel?: string;
  className?: string;
}

export default function PresencePills({
  resourceKind,
  resourceId,
  viewerLabel,
  className,
}: PresencePillsProps) {
  const { t } = useTranslation();
  const { snapshot, enabled } = usePresence({
    resourceKind,
    resourceId,
    viewerLabel,
  });

  // Always render nothing for free-tier or zero co-viewers. The lone
  // viewer (you) is implicit in the URL — no need to label it.
  if (!enabled) return null;
  // Filter out the local peer if the relay echoed it back; we don't
  // have a stable self-peer id on the client yet so this dedupes by
  // label as a best effort.
  const others = snapshot.viewers.filter((v) => v.viewerLabel !== viewerLabel);
  if (others.length === 0) return null;

  const labels = others
    .map((v) => v.viewerLabel || v.peerId.slice(0, 6))
    .slice(0, 3);
  const overflow = others.length - labels.length;

  return (
    <div
      className={cn(
        "inline-flex items-center gap-1 rounded-full border border-cs-border bg-cs-bg-raised px-2 py-0.5",
        className,
      )}
      title={t("livePresence.tooltip", {
        defaultValue: "{{count}} teammate(s) viewing",
        count: others.length,
      })}
    >
      <span className="text-[10px] text-cs-muted">●</span>
      <span className="text-[10px] text-cs-text font-medium">
        {labels.join(", ")}
        {overflow > 0
          ? t("livePresence.andOthers", {
              defaultValue: " +{{count}} more",
              count: overflow,
            })
          : ""}
      </span>
      <span className="text-[10px] text-cs-muted">
        {t("livePresence.viewingNow", { defaultValue: "viewing" })}
      </span>
    </div>
  );
}
