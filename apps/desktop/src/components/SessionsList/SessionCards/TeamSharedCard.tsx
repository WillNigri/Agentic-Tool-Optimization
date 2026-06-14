// SessionsList/SessionCards/TeamSharedCard.tsx — cloud team-shared card.
//
// teamfilter (#1) — rendered for rows with rowKind ===
// "team_shared_session" | "team_shared_war_room" | "team_shared_chat".
// These rows are fetched from the cloud (getSharedSessions /
// getSharedWarRooms / getSharedChats) when the Team filter chip is picked
// and merged into the unified Sessions feed, sorted shared_at DESC.
//
// The card carries a distinct Team accent badge + a 'Shared by X' pill so
// a shared row reads differently from a local one at a glance. Clicking
// opens a placeholder read-only view via `onOpen` (#6 replaces the
// placeholder with the real cross-machine transcript).

import { Users } from "lucide-react";

import { cn } from "@/lib/utils";
import { formatTime } from "../_helpers";
import type { SessionListRow } from "../_helpers";

interface Props {
  session: SessionListRow;
  onOpen: () => void;
}

// Per-kind label for the leading pill so the feed still reads "what kind
// of conversation is this" even for shared rows.
const KIND_LABEL: Record<string, string> = {
  team_shared_session: "🔗 shared session",
  team_shared_war_room: "🔗 shared war room",
  team_shared_chat: "🔗 shared chat",
};

// teamfilter (#1) — distinct Team accent. `cs-accent-purple` is not
// defined in tailwind.config.js today, so per the spec we fall back to
// the cs-accent token. Centralized here so a future purple token only
// needs to land in one place.
const TEAM_ACCENT_BADGE =
  "px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-accent/15 text-cs-accent ring-1 ring-cs-accent/40";

export function TeamSharedCard({ session: s, onOpen }: Props) {
  const kindLabel = KIND_LABEL[s.rowKind] ?? "🔗 shared";

  return (
    <button
      onClick={onOpen}
      title={`Team-shared ${s.rowKind} ${s.id}`}
      className={cn(
        "w-full text-left border rounded-lg p-4 transition-colors",
        "border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40 hover:bg-cs-border/20",
      )}
    >
      <div className="flex items-center gap-3 flex-wrap">
        <span
          aria-label="team shared"
          className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-muted/15 text-cs-muted"
          title="Shared with you by a teammate via ATO Cloud."
        >
          {kindLabel}
        </span>
        {/* teamfilter (#1) — the distinct Team accent badge. */}
        <span className={TEAM_ACCENT_BADGE} title="Shared into a team workspace">
          <span className="inline-flex items-center gap-1">
            <Users size={10} /> Team
          </span>
        </span>
        {/* 'Shared by X' pill. */}
        {s.sharedByLabel && (
          <span
            className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-cs-accent/10 text-cs-accent"
            title={`Shared by ${s.sharedByLabel}`}
          >
            Shared by {s.sharedByLabel}
          </span>
        )}
        <div className="ml-auto inline-flex items-center gap-3 text-xs text-cs-muted">
          {s.sharedAt && (
            <span title={`Shared ${formatTime(s.sharedAt)}`}>
              {formatTime(s.sharedAt)}
            </span>
          )}
        </div>
      </div>
      <div className="mt-2 text-sm font-medium text-cs-text truncate">
        {s.title || (
          <span className="text-cs-muted italic font-normal">
            untitled shared conversation
          </span>
        )}
      </div>
      <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
        {s.sharedTeamName && (
          <span>
            team:{" "}
            <span className="text-cs-text font-mono">{s.sharedTeamName}</span>
          </span>
        )}
        <span>read-only · opens a placeholder until #6 lands the full view</span>
      </div>
      {s.summary && (
        <div className="mt-2 text-xs text-cs-muted line-clamp-2">
          {s.summary}
        </div>
      )}
    </button>
  );
}
