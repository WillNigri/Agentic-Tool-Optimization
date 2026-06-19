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

import { Lock, Tag, Users } from "lucide-react";

import { cn } from "@/lib/utils";
import { runtimeBadge, runtimeDisplay, formatTime, avatarInitials } from "../_helpers";
import type { SessionListRow } from "../_helpers";

interface TeamShareAnnotation {
  teamName: string | null;
  sharedByLabel: string | null;
  isOwner: boolean;
  members?: { userId: string; name: string | null; email: string }[];
  onManageAccess?: () => void;
}

interface Props {
  session: SessionListRow;
  onOpen: () => void;
  teamShare?: TeamShareAnnotation;
}

export function ChatCard({ session: s, onOpen, teamShare }: Props) {
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
      {/* Part D — team-share annotation banner */}
      {teamShare && (
        <div className="mb-3 flex items-center gap-2 flex-wrap pb-3 border-b border-cs-border/40">
          <span
            className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-accent/15 text-cs-accent ring-1 ring-cs-accent/40"
            title={`Shared into team: ${teamShare.teamName ?? "unknown"}`}
          >
            <Users size={10} />
            {teamShare.teamName ?? "Team"}
          </span>
          {!teamShare.isOwner && teamShare.sharedByLabel && (
            <span className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-cs-accent/10 text-cs-accent">
              shared by {teamShare.sharedByLabel}
            </span>
          )}
          {teamShare.members && teamShare.members.length > 0 && (
            <div className="flex items-center gap-1">
              {teamShare.members.slice(0, 4).map((m) => (
                <span
                  key={m.userId}
                  title={m.name ?? m.email}
                  className="inline-flex items-center justify-center w-5 h-5 rounded-full bg-cs-accent/20 text-cs-accent text-[9px] font-bold uppercase"
                >
                  {avatarInitials(m.name ?? m.email)}
                </span>
              ))}
              {teamShare.members.length > 4 && (
                <span className="text-[10px] text-cs-muted">+{teamShare.members.length - 4}</span>
              )}
            </div>
          )}
          {teamShare.onManageAccess && (
            <button
              type="button"
              onClick={(e) => { e.stopPropagation(); teamShare.onManageAccess?.(); }}
              className="ml-auto text-[10px] text-cs-accent hover:underline"
            >
              Manage access
            </button>
          )}
        </div>
      )}
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
        {/* v2.7.14 — When anchorRuntime is set, it's the WhatsApp-row
            "this chat is with X" identity (stable across runtime hops).
            Render with a small "with" label + ring to distinguish from
            the per-message latest-runtime fallback. When anchor isn't
            set (legacy chat with no backfill, OR a chat with no
            assistant turn yet) we still show s.runtime as a plain
            badge so something always renders. */}
        {s.anchorRuntime ? (
          <div className="flex items-center gap-1">
            <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
              With
            </span>
            <span
              className={cn(runtimeBadge(s.anchorRuntime), "ring-1 ring-cs-muted/40")}
              title={`Anchor runtime: ${runtimeDisplay(s.anchorRuntime)} — the LLM this chat thread is primarily with. Individual messages can be routed to other runtimes; this stays stable.`}
            >
              {s.anchorRuntime}
            </span>
          </div>
        ) : (
          <span
            className={cn(runtimeBadge(s.runtime))}
            title={`Most recent runtime: ${runtimeDisplay(s.runtime)}`}
          >
            {s.runtime}
          </span>
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
