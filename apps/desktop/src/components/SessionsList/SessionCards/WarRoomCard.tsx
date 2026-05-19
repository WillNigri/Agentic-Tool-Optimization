// SessionsList/SessionCards/WarRoomCard.tsx — war-room synthetic card.
//
// Extracted from SessionsList.tsx (2026-05-19 frontend elegance push).
// Used for rows with `rowKind === "war_room"` — groups N parallel
// single-runs sharing a war_room_id into one card with seat badges
// (all co-equal, no Coord/+ split since R1-parallel war-rooms are
// peers by design). Click opens WarRoomDetailView via `onOpen`.

import { cn } from "@/lib/utils";
import {
  runtimeBadge,
  personaBadge,
  personaDisplay,
  formatTime,
} from "../_helpers";
import type { SessionListRow } from "../_helpers";

interface Props {
  session: SessionListRow;
  onOpen: () => void;
}

export function WarRoomCard({ session: s, onOpen }: Props) {
  const participantCount = s.runtimesUsed.length;
  return (
    <button
      onClick={onOpen}
      title={`War room ${s.id}`}
      className="w-full text-left border rounded-lg p-4 transition-colors border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40"
    >
      <div className="flex items-center gap-3 flex-wrap">
        <span
          aria-label="war room"
          className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-accent/15 text-cs-accent"
          title={`War room ${s.id.slice(0, 8)} — ${participantCount} parallel seats`}
        >
          ⚔ war room
        </span>
        {/* Participant runtime badges. All co-equal — no Coord/+ split
            since R1-parallel war-rooms are peers by design. */}
        <div className="flex items-center gap-1">
          <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
            seats
          </span>
          {s.runtimesUsed.map((r) => (
            <span
              key={r}
              className={cn(runtimeBadge(r))}
              title={`Participant runtime: ${r}`}
            >
              {r}
            </span>
          ))}
        </div>
        {s.agentsUsed.length > 0 && (
          <div className="flex items-center gap-1">
            {s.agentsUsed.map((slug) => (
              <span
                key={slug}
                className={personaBadge()}
                title={`Persona: ${personaDisplay(slug)}`}
              >
                {personaDisplay(slug)}
              </span>
            ))}
          </div>
        )}
        <div className="ml-auto inline-flex items-center gap-3 text-xs text-cs-muted">
          <span>
            {participantCount} seat{participantCount !== 1 ? "s" : ""}
          </span>
          {s.totalCostUsd !== null && s.totalCostUsd > 0 && (
            <span className="font-mono">${s.totalCostUsd.toFixed(4)}</span>
          )}
          <span>{formatTime(s.lastUsedAt)}</span>
        </div>
      </div>
      <div className="mt-2 text-sm font-medium text-cs-text truncate">
        {s.title || (
          <span className="text-cs-muted italic font-normal">
            untitled war room
          </span>
        )}
      </div>
      {/* PR 17 — meta line on the war-room card so it has parity with
          the session card's coordinator/team line. Shows the seat-count
          summary; the war-room title (first round's prompt prefix)
          already appears in the chip row above. */}
      <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
        <span>
          seats: <span className="text-cs-text">{participantCount}</span>
        </span>
        <span>
          kind:{" "}
          <span className="text-cs-text">parallel</span> · each seat fires
          independently within a round; across rounds every seat sees the
          full peer transcript
        </span>
      </div>
      {/* PR 17 — body preview: first-round prompt. Matches single-run +
          session cards which both surface a body line, so the Sessions
          feed feels uniform. */}
      {s.title && (
        <div className="mt-2 text-xs text-cs-muted line-clamp-2">{s.title}</div>
      )}
    </button>
  );
}
