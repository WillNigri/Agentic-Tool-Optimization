// PR-5 UI (v2.7.10) — Run-detail surfaces denied + advisory tool events.
//
// v2.7.8 plumbed agent permissions through dispatch (see
// `~/.claude/projects/-Users-beatriznigri/memory/project_v2_7_8_agent_perms_shipped.md`)
// and the CLI now records every tool call in `execution_logs.tool_calls_summary`
// as a JSON array of {name, args_brief, is_error}. SingleRunDetailView already
// shows the prompt/response/error, but if an agent attempted a tool that policy
// blocked, the user saw nothing — the promise of "this agent can't do X" was
// real but invisible. This panel surfaces those events.
//
// Categories (war-room 2026-05-20, claude+google):
//   - allowed   — default; the tool ran and returned a result
//   - denied    — args_brief mentions "blocked by agent policy". The marker
//                 alone is sufficient — both reviewers flagged the obvious
//                 fragility of `isError && marker`: a backend that emits the
//                 marker but forgets isError silently demotes the denial to
//                 "allowed". The marker is the contract; isError is advisory.
//                 Backend wiring is on the v2.7.10 docket — empty until then.
//   - advisory  — codex-style soft-deny ("advisory_only" marker). Codex's
//                 --sandbox is a binary 3-mode, so per-tool denies are advisory
//                 on that runtime (see project_v2_7_8 memo). Empty in v2.7.10.
//
// Substring match on args_brief is intentionally permissive — when the backend
// adds a structured `category` field (or similar) we'll switch to that. A
// path literally named "advisory_only.log" or a grep for "blocked by agent
// policy" would miscategorize today; documented limitation, not load-bearing.
//
// Parsing reuses `parseToolCallsSummary` (lib/tauri-api.ts:1671) — keeping a
// single source of truth so a future backend schema bump (e.g. a real
// `content`/`reason` field) widens one parser, not two.

import { parseToolCallsSummary, type ToolCallAuditEntry } from "@/lib/tauri-api";
import { cn } from "@/lib/utils";

const DENIED_MARKER = "blocked by agent policy";
const ADVISORY_MARKER = "advisory_only";

export interface CategorizedPermissionEvents {
  allowed: ToolCallAuditEntry[];
  denied: ToolCallAuditEntry[];
  advisory: ToolCallAuditEntry[];
}

/** Bucket parsed tool-call entries into allowed / denied / advisory.
 *  Pure function — no JSX. Exported for unit tests. */
export function categorizeToolCalls(
  entries: ToolCallAuditEntry[],
): CategorizedPermissionEvents {
  const out: CategorizedPermissionEvents = {
    allowed: [],
    denied: [],
    advisory: [],
  };
  for (const e of entries) {
    const brief = e.argsBrief ?? "";
    if (brief.includes(ADVISORY_MARKER)) {
      out.advisory.push(e);
    } else if (brief.includes(DENIED_MARKER)) {
      out.denied.push(e);
    } else {
      out.allowed.push(e);
    }
  }
  return out;
}

/** Combined parse + categorize. Returns empty buckets for null / undefined /
 *  malformed JSON (parseToolCallsSummary swallows the parse error). */
export function parsePermissionEvents(
  raw: string | null | undefined,
): CategorizedPermissionEvents {
  return categorizeToolCalls(parseToolCallsSummary(raw));
}

interface BucketStyle {
  label: string;
  dot: string;
  text: string;
}

const BUCKETS: Record<keyof CategorizedPermissionEvents, BucketStyle> = {
  allowed: {
    label: "Allowed",
    dot: "bg-cs-success",
    text: "text-cs-success",
  },
  denied: {
    label: "Denied",
    dot: "bg-cs-danger",
    text: "text-cs-danger",
  },
  advisory: {
    label: "Advisory",
    dot: "bg-cs-warning",
    text: "text-cs-warning",
  },
};

const BUCKET_ORDER: (keyof CategorizedPermissionEvents)[] = [
  "denied",
  "advisory",
  "allowed",
];

function EventRow({ entry }: { entry: ToolCallAuditEntry }) {
  return (
    <div className="flex items-baseline gap-2 text-xs font-mono">
      <span className="text-cs-text">{entry.name || "(unnamed)"}</span>
      {entry.argsBrief && (
        <span className="text-cs-muted truncate">{entry.argsBrief}</span>
      )}
    </div>
  );
}

export interface PermissionEventsPanelProps {
  /** Raw JSON from execution_logs.tool_calls_summary. Pass null/empty for
   *  legacy rows — the panel hides itself rather than render three empty
   *  buckets on every pre-v2.7.8 dispatch. */
  toolCallsSummary: string | null | undefined;
}

export default function PermissionEventsPanel({
  toolCallsSummary,
}: PermissionEventsPanelProps) {
  const events = parsePermissionEvents(toolCallsSummary);
  const total =
    events.allowed.length + events.denied.length + events.advisory.length;

  // Legacy / no-tool dispatches: don't bother the user with an empty panel.
  if (!toolCallsSummary || total === 0) return null;

  return (
    <div className="rounded-lg border border-cs-border bg-cs-card p-4 space-y-3">
      <div className="flex items-center justify-between">
        <div className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
          Tool calls
        </div>
        <div className="text-[10px] text-cs-muted font-mono">
          {total} event{total === 1 ? "" : "s"}
        </div>
      </div>
      <div className="space-y-3">
        {BUCKET_ORDER.map((key) => {
          const items = events[key];
          if (items.length === 0) return null;
          const style = BUCKETS[key];
          return (
            <div key={key} className="space-y-1.5">
              <div className="flex items-center gap-2">
                <span
                  className={cn(
                    "inline-block w-1.5 h-1.5 rounded-full",
                    style.dot,
                  )}
                  aria-hidden
                />
                <span
                  className={cn(
                    "text-[10px] uppercase tracking-wider font-medium",
                    style.text,
                  )}
                >
                  {style.label}
                </span>
                <span className="text-[10px] text-cs-muted font-mono">
                  {items.length}
                </span>
              </div>
              <ul className="space-y-0.5 pl-3.5">
                {items.map((entry, i) => (
                  <li key={i}>
                    <EventRow entry={entry} />
                  </li>
                ))}
              </ul>
            </div>
          );
        })}
      </div>
    </div>
  );
}
