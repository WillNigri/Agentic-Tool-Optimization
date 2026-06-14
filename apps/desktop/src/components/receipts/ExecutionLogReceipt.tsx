// FOLLOWUPS #1 — clickable execution_log receipts.
//
// Anywhere the UI surfaces an execution_log_id as a "where did this
// come from" reference (loop steps, mission events, methodology runs,
// war-room seats, etc.), wrap it with this component to make it
// click-through to the SingleRunDetailView for that log.
//
// Navigation flow: set sidebar section to "runs", then use the
// pending-open primitive from useUiStore. SessionsList consumes
// pendingOpenSession on mount and routes to SingleRunDetailView.
//
// Sizing matches existing in-flow monospace id chips (text-[10px]
// font-mono in muted color, hover to accent). When the logId is
// empty / null / "—", we render the placeholder as static text
// (no click target).

import { useUiStore } from "@/stores/useUiStore";
import { cn } from "@/lib/utils";

interface Props {
  logId?: string | null;
  /** Optional override label; default is the trailing 8 chars of the id. */
  label?: string;
  /** Pass through extra class tokens (e.g. text size). */
  className?: string;
}

function shortId(id: string): string {
  // execution_log ids are 36-char UUIDs; show the trailing chunk
  // because the prefix collides at scale.
  return id.length > 8 ? id.slice(-8) : id;
}

export default function ExecutionLogReceipt({ logId, label, className }: Props) {
  const setSection = useUiStore((s) => s.setSection);
  const openSessionDetail = useUiStore((s) => s.openSessionDetail);

  if (!logId) {
    return (
      <span className={cn("text-cs-muted", className)} title="No execution log">
        —
      </span>
    );
  }

  const display = label ?? shortId(logId);

  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        // The pending-open primitive expects "single_run" + the log id.
        // SessionsList lives under the "runs" sidebar section, so flip
        // there too. Both are idempotent.
        setSection("runs");
        openSessionDetail("single_run", logId);
      }}
      title={`Open run ${logId}`}
      className={cn(
        "inline-flex items-baseline rounded px-1.5 py-0.5",
        "font-mono text-[10px] text-cs-muted",
        "bg-cs-bg-raised/40 border border-cs-border/40",
        "hover:text-cs-accent hover:border-cs-accent/40 hover:bg-cs-accent/5",
        "transition-colors cursor-pointer",
        className,
      )}
    >
      {display}
    </button>
  );
}
