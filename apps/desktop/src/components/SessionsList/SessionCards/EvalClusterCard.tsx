// SessionsList/SessionCards/EvalClusterCard.tsx — eval-cluster card.
//
// v2.10.0 PR-1 (UI). Shipped to close the "Runs tab is drowning" bug
// Will reported 2026-05-24 after the n=150 Part 5 eval landed 150
// identical-looking SINGLE RUN cards in the feed.
//
// Renders a single collapsed card representing N consecutive single_run
// rows that share the same prompt + same runtime. Click expands to
// reveal the individual SingleRunCard children (each clickable to drill
// into its own receipt). Aggregated stats (count, total cost) read
// from the synthesized row's `clusterCount` / `clusterTotalCostUsd`
// fields populated by `clusterEvalRuns()` in `_helpers.ts`.

import { useState } from "react";
import { cn } from "@/lib/utils";
import { runtimeBadge, runtimeDisplay, formatTime } from "../_helpers";
import type { SessionListRow } from "../_helpers";
import { SingleRunCard } from "./SingleRunCard";

interface Props {
  session: SessionListRow;
  onOpenMember: (memberId: string) => void;
}

export function EvalClusterCard({ session: s, onOpenMember }: Props) {
  const [expanded, setExpanded] = useState(false);
  const members = s.clusterMembers ?? [];
  const count = s.clusterCount ?? members.length;
  const totalCost = s.clusterTotalCostUsd ?? 0;
  const promptPreview = s.title ?? "(no prompt recorded)";

  // Verdict mix when grounded — show a tiny strip of how many
  // members landed at each verdict. Helps a glance answer
  // "did most of the eval pass?" without expanding.
  // PR-1 ships the simple count; verdict-strip is a follow-on
  // once grounding_verdict is on SessionListRow (currently
  // not — would need a backend query change).

  return (
    <div
      className={cn(
        "w-full border rounded-lg transition-colors",
        "border-cs-accent/30 bg-cs-card/40 hover:border-cs-accent/50",
      )}
    >
      <button
        onClick={() => setExpanded((v) => !v)}
        className="w-full text-left p-4"
        title="Click to expand the individual dispatches in this eval cluster"
      >
        <div className="flex items-center gap-3 flex-wrap">
          <span
            aria-label="eval cluster"
            className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-accent/15 text-cs-accent"
            title={`Eval cluster · ${count} same-prompt dispatches grouped`}
          >
            ▦ EVAL · {count}
          </span>
          <span
            className={cn(
              "px-1.5 py-0.5 rounded text-[10px] font-semibold uppercase tracking-wide",
              runtimeBadge(s.runtime),
            )}
            title={`Runtime: ${runtimeDisplay(s.runtime)}`}
          >
            {runtimeDisplay(s.runtime)}
          </span>
          <span className="text-cs-muted text-xs ml-auto">
            ${totalCost.toFixed(4)}
          </span>
          <span className="text-cs-muted text-xs">
            {formatTime(s.lastUsedAt ?? s.createdAt)}
          </span>
        </div>
        <div className="mt-2 text-cs-fg font-medium">{promptPreview}</div>
        <div className="mt-1 text-cs-muted text-xs flex items-center gap-2 flex-wrap">
          <span>
            {count} dispatches · same prompt · total ${totalCost.toFixed(4)}
          </span>
          <span aria-hidden="true">·</span>
          <span className="text-cs-accent/80">
            {expanded ? "▾ click to collapse" : "▸ click to expand"}
          </span>
        </div>
      </button>
      {expanded && (
        <div className="border-t border-cs-border/40 px-4 py-3 space-y-2 bg-cs-card/20">
          <div className="text-[11px] text-cs-muted uppercase tracking-wide">
            Individual dispatches ({members.length})
          </div>
          {members.map((m) => (
            <SingleRunCard
              key={m.id}
              session={m}
              onOpen={() => onOpenMember(m.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
