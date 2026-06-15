// LiveCursors — render teammate cursor markers on a detail page.
//
// Drop-in component for the live-team-workspace surface. Expects to be
// placed inside a relative-positioned parent (e.g. a session-detail
// container) and absolutely-positions a small colored marker per
// non-self viewer, plus an onMouseMove handler that throttles outbound
// cursor reports via the usePresence hook.

import { useEffect, useMemo, useRef } from "react";

import { cn } from "@/lib/utils";
import { usePresence } from "./usePresence";
import type { PresenceResourceKind } from "@/lib/meshRelay";

interface LiveCursorsProps {
  resourceKind: PresenceResourceKind;
  resourceId: string;
  viewerLabel?: string;
  // Optional override for the cursor palette — by default a 6-slot
  // colorblind-safe ramp is used; callers can pass a custom list for
  // brand-consistency.
  palette?: string[];
  className?: string;
}

const DEFAULT_PALETTE = [
  "#00FFB2",
  "#F58231",
  "#911EB4",
  "#42D4F4",
  "#F032E6",
  "#9A6324",
];

function colorForPeer(peerId: string, palette: string[]): string {
  let hash = 0;
  for (let i = 0; i < peerId.length; i++) {
    hash = (hash * 31 + peerId.charCodeAt(i)) >>> 0;
  }
  return palette[hash % palette.length];
}

export default function LiveCursors({
  resourceKind,
  resourceId,
  viewerLabel,
  palette = DEFAULT_PALETTE,
  className,
}: LiveCursorsProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const { snapshot, reportCursor, enabled } = usePresence({
    resourceKind,
    resourceId,
    viewerLabel,
  });

  useEffect(() => {
    if (!enabled) return;
    const el = containerRef.current?.parentElement;
    if (!el) return;
    const onMove = (e: MouseEvent) => {
      const rect = el.getBoundingClientRect();
      const x = e.clientX - rect.left;
      const y = e.clientY - rect.top;
      // Clamp to the host element so an out-of-bounds drag doesn't
      // ship runaway coordinates over the wire.
      if (x < 0 || y < 0 || x > rect.width || y > rect.height) return;
      reportCursor(x / rect.width, y / rect.height);
    };
    el.addEventListener("mousemove", onMove);
    return () => el.removeEventListener("mousemove", onMove);
  }, [enabled, reportCursor]);

  const cursors = useMemo(() => snapshot.cursors, [snapshot.cursors]);

  if (!enabled || cursors.length === 0) {
    return (
      <div ref={containerRef} className={cn("pointer-events-none absolute inset-0", className)} />
    );
  }

  return (
    <div
      ref={containerRef}
      className={cn("pointer-events-none absolute inset-0 overflow-hidden", className)}
      aria-hidden
    >
      {cursors.map((c) => {
        const color = colorForPeer(c.peerId, palette);
        const viewer = snapshot.viewers.find((v) => v.peerId === c.peerId);
        const label = viewer?.viewerLabel ?? c.peerId.slice(0, 6);
        // Codex R1: clamp inbound coords defensively. The sender clamps
        // before emitting but a malicious or buggy peer could ship
        // out-of-bounds coords and we don't want a cursor floating
        // outside the container or overflowing the page.
        const cx = Math.min(1, Math.max(0, c.x));
        const cy = Math.min(1, Math.max(0, c.y));
        return (
          <div
            key={c.peerId}
            className="absolute transition-transform duration-75 ease-out"
            style={{
              left: `${(cx * 100).toFixed(3)}%`,
              top: `${(cy * 100).toFixed(3)}%`,
              transform: "translate(-2px, -2px)",
            }}
          >
            <svg width={20} height={20} viewBox="0 0 20 20">
              <path
                d="M2 2 L18 8 L9 10 L8 18 Z"
                fill={color}
                stroke="#0a0a0f"
                strokeWidth={1}
                strokeLinejoin="round"
              />
            </svg>
            <div
              className="rounded-sm px-1.5 py-0.5 text-[9px] font-medium text-cs-bg shadow-md whitespace-nowrap"
              style={{
                background: color,
                marginLeft: 10,
                marginTop: -2,
                display: "inline-block",
              }}
            >
              {label}
            </div>
          </div>
        );
      })}
    </div>
  );
}
