// SessionsList/SessionCards/WarRoomCard.tsx — war-room synthetic card.
//
// Extracted from SessionsList.tsx (2026-05-19 frontend elegance push).
// Used for rows with `rowKind === "war_room"` — groups N parallel
// single-runs sharing a war_room_id into one card with seat badges.
// Click opens WarRoomDetailView via `onOpen`.
//
// v2.7.13 — closed war rooms now render the same family of metadata
// the SessionCard does for closed sessions: COORD badge with the
// summarizer runtime, CLOSED status, category, coordinator-generated
// auto_title (preferred over the raw first-round prompt), summary,
// tags, and the human's "Note from human." Open war rooms render the
// original peer-seats + parallel-kind layout unchanged. Will dogfood
// 2026-05-21: the card used to show only the prompt + "kind: parallel"
// intro text even after close, burying the work the coordinator did.

import { Lock, Tag } from "lucide-react";

import { cn } from "@/lib/utils";
import {
  runtimeBadge,
  runtimeDisplay,
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
  const isClosed = s.status === "closed";
  // Prefer the coordinator-generated auto_title once closed; for open
  // war rooms fall back to the first-round prompt fragment from
  // list_sessions_inner.
  const displayTitle = isClosed ? s.autoTitle || s.title : s.title;
  // For closed war rooms the summary is the right preview (it captures
  // where seats agreed/diverged + what got decided). For open war
  // rooms there is no summary yet — show the kicking-off prompt as the
  // body line, same as before.
  const previewText = isClosed && s.summary ? s.summary : s.title;

  return (
    <button
      onClick={onOpen}
      title={`War room ${s.id}`}
      className={cn(
        "w-full text-left border rounded-lg p-4 transition-colors",
        isClosed
          ? "border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40"
          : "border-cs-border bg-cs-card hover:border-cs-accent/40 hover:bg-cs-border/20",
      )}
    >
      <div className="flex items-center gap-3 flex-wrap">
        <span
          aria-label="war room"
          className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-accent/15 text-cs-accent"
          title={`War room ${s.id.slice(0, 8)} — ${participantCount} parallel seats`}
        >
          ⚔ war room
        </span>
        {/* v2.7.13 — coordinator badge mirrors the SessionCard's
            Coord/+ split. Only renders for closed war rooms; open
            ones have no single coordinator (the coordinator is the
            human reading the rounds). */}
        {isClosed && s.coordinatorRuntime && (
          <div className="flex items-center gap-1">
            <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
              Coord
            </span>
            <span
              className={cn(
                runtimeBadge(s.coordinatorRuntime),
                "ring-1 ring-cs-accent/70",
              )}
              title={`Coordinator runtime: ${runtimeDisplay(
                s.coordinatorRuntime,
              )} — summarized this war room at close`}
            >
              {s.coordinatorRuntime}
            </span>
          </div>
        )}
        {/* Participant seats. Co-equal — no Coord/+ split among seats
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
        {isClosed && (
          <span
            className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-cs-muted/20 text-cs-muted"
            title={s.closedAt ? `Closed ${formatTime(s.closedAt)}` : "Closed"}
          >
            <Lock size={10} /> closed
          </span>
        )}
        {s.category && (
          <span
            className="px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-cs-accent/15 text-cs-accent"
            title={`Category: ${s.category} — populated by the coordinator at close`}
          >
            {s.category}
          </span>
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
        {displayTitle || (
          <span className="text-cs-muted italic font-normal">
            untitled war room
          </span>
        )}
      </div>
      <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
        {/* v2.7.13 — for closed rooms the meta line shows the
            coordinator name + team + project, matching SessionCard.
            Open rooms keep the kind/parallel explainer that helps
            new users understand the war-room semantics. */}
        {isClosed ? (
          <>
            {s.coordinatorRuntime && (
              <span>
                coordinator:{" "}
                <span className="text-cs-text">
                  {runtimeDisplay(s.coordinatorRuntime)}
                </span>
              </span>
            )}
            {s.projectId && (
              <span>
                project:{" "}
                <span
                  className="text-cs-text font-mono"
                  title={`project_id: ${s.projectId}`}
                >
                  {s.projectName ?? s.projectId.slice(0, 8)}
                </span>
              </span>
            )}
            {s.team && (
              <span>
                team: <span className="text-cs-text font-mono">{s.team}</span>
              </span>
            )}
          </>
        ) : (
          <>
            <span>
              seats: <span className="text-cs-text">{participantCount}</span>
            </span>
            <span>
              kind: <span className="text-cs-text">parallel</span> · each seat
              fires independently within a round; across rounds every seat sees
              the full peer transcript
            </span>
          </>
        )}
      </div>
      {previewText && (
        <div className="mt-2 text-xs text-cs-muted line-clamp-2">
          {previewText}
        </div>
      )}
      {/* v2.7.13 — human's free-form note. Only renders on closed war
          rooms where the user attached one. Distinct visual treatment
          so a glance separates LLM output from human framing. */}
      {isClosed && s.humanComment && s.humanComment.trim() && (
        <div className="mt-2 border-l-2 border-cs-accent/40 pl-2 text-xs text-cs-muted italic">
          <span className="text-[10px] uppercase tracking-wider not-italic font-medium text-cs-muted mr-1">
            note:
          </span>
          {s.humanComment}
        </div>
      )}
      {s.tags.length > 0 && (
        <div className="mt-2 flex items-center gap-1 flex-wrap">
          <Tag size={10} className="text-cs-muted" />
          {s.tags.map((tag) => (
            <span
              key={tag}
              className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-cs-accent/10 text-cs-accent"
              title={`Tag: ${tag}`}
            >
              {tag}
            </span>
          ))}
        </div>
      )}
    </button>
  );
}
