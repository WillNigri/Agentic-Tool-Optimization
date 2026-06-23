// SessionsList/SessionCards/SingleRunCard.tsx — single-dispatch card.
//
// Extracted from SessionsList.tsx (2026-05-19 frontend elegance push).
// Used for rows with `rowKind === "single_run"` — execution_logs rows
// with no session_id and no war_room_id. Click opens SingleRunDetailView
// via `onOpen`. Error rows get a danger-tinted border.

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

export function SingleRunCard({ session: s, onOpen }: Props) {
  const promptPreview = s.title ?? "(no prompt recorded)";
  const responsePreview = s.lastAssistantPreview;
  const isErr = s.status !== "success";
  return (
    <button
      onClick={onOpen}
      title="Open the full prompt + response for this single-run dispatch."
      className={cn(
        "w-full text-left border rounded-lg p-4 transition-colors",
        isErr
          ? "border-cs-danger/40 bg-cs-card/40 hover:border-cs-danger/60"
          : "border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40",
      )}
    >
      <div className="flex items-center gap-3 flex-wrap">
        {/* PR 17 — leading kind marker pill for parity with war-room
            (⚔ WAR ROOM) and session (💬 SESSION). Status (error vs
            success) encoded via the pill bg so the same pill does
            double duty. */}
        <span
          aria-label="single run"
          className={cn(
            "px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide",
            isErr ? "bg-cs-danger/15 text-cs-danger" : "bg-cs-muted/15 text-cs-muted",
          )}
          title={`Single run · ${s.status}`}
        >
          ⚡ single run
        </span>
        <span
          className={cn(runtimeBadge(s.runtime))}
          title={`Runtime: ${runtimeDisplay(s.runtime)}`}
        >
          {s.runtime}
        </span>
        {s.agentSlug && (
          <span
            className={personaBadge()}
            title={`Persona seat: ${personaDisplay(s.agentSlug)}`}
          >
            {personaDisplay(s.agentSlug)}
          </span>
        )}
        <div className="ml-auto inline-flex items-center gap-3 text-xs text-cs-muted">
          {s.totalCostUsd !== null && s.totalCostUsd > 0 && (
            <span
              className="font-mono"
              title="Estimated cost from execution_logs.cost_usd_estimated."
            >
              ${s.totalCostUsd.toFixed(4)}
            </span>
          )}
          {s.unpricedCount > 0 && (
            <span
              className="font-mono text-amber-400"
              title={`${s.unpricedCount} dispatch(es) have no cost estimate (model missing from the pricing table). The cost shown counts only priced dispatches.`}
            >
              {s.unpricedCount} unpriced
            </span>
          )}
          <span>{formatTime(s.lastUsedAt)}</span>
        </div>
      </div>
      <div className="mt-2 text-sm font-medium text-cs-text truncate">
        {promptPreview}
      </div>
      <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
        <span>
          runtime:{" "}
          <span className="text-cs-text">{runtimeDisplay(s.runtime)}</span>
        </span>
        {s.agentSlug && (
          <span>
            persona:{" "}
            <span className="text-cs-accent">
              {personaDisplay(s.agentSlug)}
            </span>
          </span>
        )}
        <span>
          kind: <span className="text-cs-text">single dispatch</span>
        </span>
      </div>
      {responsePreview && (
        <div className="mt-2 text-xs text-cs-muted line-clamp-2 font-mono">
          {responsePreview}
        </div>
      )}
    </button>
  );
}
