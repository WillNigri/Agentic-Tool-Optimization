// SessionsList/SessionCards/ChatCard.tsx — bottom-pane chat thread card.
//
// Extracted from SessionsList.tsx (2026-05-19 frontend elegance push).
// Used for rows with `rowKind === "chat"` — Path A consolidation
// (2026-05-18) UNIONs chat threads into the Sessions feed; this is the
// inbox card for them. Clicking opens ChatThreadDetailView via the
// `onOpen` callback.

import { cn } from "@/lib/utils";
import { runtimeBadge, runtimeDisplay, formatTime } from "../_helpers";
import type { SessionListRow } from "../_helpers";

interface Props {
  session: SessionListRow;
  onOpen: () => void;
}

export function ChatCard({ session: s, onOpen }: Props) {
  return (
    <button
      onClick={onOpen}
      title={`Chat thread ${s.id}`}
      className="w-full text-left border rounded-lg p-4 transition-colors border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40"
    >
      <div className="flex items-center gap-3 flex-wrap">
        <span
          aria-label="chat"
          className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-muted/15 text-cs-muted"
          title="Bottom-pane chat thread. One-on-one conversation, can hop runtimes per message."
        >
          🗨 chat
        </span>
        <span
          className={cn(runtimeBadge(s.runtime))}
          title={`Most recent runtime: ${runtimeDisplay(s.runtime)}`}
        >
          {s.runtime}
        </span>
        <div className="ml-auto inline-flex items-center gap-3 text-xs text-cs-muted">
          <span>
            {s.turnCount} msg{s.turnCount !== 1 ? "s" : ""}
          </span>
          <span>{formatTime(s.lastUsedAt)}</span>
        </div>
      </div>
      <div className="mt-2 text-sm font-medium text-cs-text truncate">
        {s.title || (
          <span className="text-cs-muted italic font-normal">untitled chat</span>
        )}
      </div>
      <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
        <span>
          runtime:{" "}
          <span className="text-cs-text">{runtimeDisplay(s.runtime)}</span>
        </span>
        <span>
          kind: <span className="text-cs-text">bottom-pane chat</span>
        </span>
      </div>
      {s.lastAssistantPreview && (
        <div className="mt-2 text-xs text-cs-muted line-clamp-2">
          {s.lastAssistantPreview}
        </div>
      )}
    </button>
  );
}
