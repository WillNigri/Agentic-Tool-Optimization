// SessionsList/SessionCards/ChatCard.tsx — bottom-pane chat thread card.
//
// Extracted from SessionsList.tsx (2026-05-19 frontend elegance push).
// Used for rows with `rowKind === "chat"` — Path A consolidation
// (2026-05-18) UNIONs chat threads into the Sessions feed; this is the
// inbox card for them. Clicking opens ChatThreadDetailView via the
// `onOpen` callback.
//
// v2.7.13 — closed chats render the same family of metadata SessionCard
// + WarRoomCard do for closed conversations: COORD badge, CLOSED state,
// category, coordinator auto_title, summary, tags, human note. Open
// chats render the original runtime + msg-count layout unchanged.

import { Lock, Tag } from "lucide-react";

import { cn } from "@/lib/utils";
import { runtimeBadge, runtimeDisplay, formatTime } from "../_helpers";
import type { SessionListRow } from "../_helpers";

interface Props {
  session: SessionListRow;
  onOpen: () => void;
}

export function ChatCard({ session: s, onOpen }: Props) {
  const isClosed = s.status === "closed";
  const displayTitle = isClosed ? s.autoTitle || s.title : s.title;
  // Closed → coordinator summary is the meaningful preview. Open →
  // the most recent assistant message (lastAssistantPreview) carries
  // the "what's this about" hook.
  const previewText =
    isClosed && s.summary ? s.summary : s.lastAssistantPreview;

  return (
    <button
      onClick={onOpen}
      title={`Chat thread ${s.id}`}
      className={cn(
        "w-full text-left border rounded-lg p-4 transition-colors",
        isClosed
          ? "border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40"
          : "border-cs-border bg-cs-card hover:border-cs-accent/40 hover:bg-cs-border/20",
      )}
    >
      <div className="flex items-center gap-3 flex-wrap">
        <span
          aria-label="chat"
          className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-muted/15 text-cs-muted"
          title="Bottom-pane chat thread. One-on-one conversation, can hop runtimes per message."
        >
          🗨 chat
        </span>
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
              )} — summarized this chat at close`}
            >
              {s.coordinatorRuntime}
            </span>
          </div>
        )}
        <span
          className={cn(runtimeBadge(s.runtime))}
          title={`Most recent runtime: ${runtimeDisplay(s.runtime)}`}
        >
          {s.runtime}
        </span>
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
            {s.turnCount} msg{s.turnCount !== 1 ? "s" : ""}
          </span>
          <span>{formatTime(s.lastUsedAt)}</span>
        </div>
      </div>
      <div className="mt-2 text-sm font-medium text-cs-text truncate">
        {displayTitle || (
          <span className="text-cs-muted italic font-normal">untitled chat</span>
        )}
      </div>
      <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
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
            {s.team && (
              <span>
                team: <span className="text-cs-text font-mono">{s.team}</span>
              </span>
            )}
          </>
        ) : (
          <>
            <span>
              runtime:{" "}
              <span className="text-cs-text">{runtimeDisplay(s.runtime)}</span>
            </span>
            <span>
              kind: <span className="text-cs-text">bottom-pane chat</span>
            </span>
          </>
        )}
      </div>
      {previewText && (
        <div className="mt-2 text-xs text-cs-muted line-clamp-2">
          {previewText}
        </div>
      )}
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
