// PR 14c (2026-05-18) — war-room drill-in view. Renders the
// constituent single-runs sharing a war_room_id as a vertical
// stack of per-seat cards. Each card shows the seat's runtime +
// agent + prompt + response inline so the user can read what each
// LLM actually said without N separate clicks. A war-room is by
// definition R1-parallel (no seat sees another's reply), so the
// vertical stack is the right reading order: each card is an
// independent first-pass opinion.
//
// Compare with SessionTranscriptView which renders sequential
// turns in a single conversation (each turn sees the prior ones
// via history replay). The shape difference reflects the topology
// difference — see the war-room vs session table in the PR 14
// commit message.

import { Loader2 } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

import { cn } from "@/lib/utils";
import {
  runtimeBadge,
  personaBadge,
  personaDisplay,
  formatTime,
} from "./_helpers";
import type { SingleRunDetail } from "./SingleRunDetailView";

export default function WarRoomDetailView({
  warRoomId,
  onBack,
}: {
  warRoomId: string;
  onBack: () => void;
}) {
  const q = useQuery<SingleRunDetail[]>({
    queryKey: ["war-room-constituents", warRoomId],
    queryFn: () =>
      invoke<SingleRunDetail[]>("get_war_room_constituents", { warRoomId }),
    staleTime: 60_000,
  });

  if (q.isLoading) {
    return (
      <div className="flex items-center justify-center h-32">
        <Loader2 className="animate-spin text-cs-accent" size={28} />
      </div>
    );
  }
  if (q.isError || !q.data) {
    return (
      <div className="space-y-4">
        <button
          onClick={onBack}
          className="text-sm text-cs-muted hover:text-cs-text"
        >
          ← Back to Sessions
        </button>
        <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 p-4 text-sm text-cs-text">
          Could not load war-room
          {q.error instanceof Error ? `: ${q.error.message}` : ""}.
        </div>
      </div>
    );
  }

  const rows = q.data;
  const totalCost = rows.reduce(
    (sum, r) => sum + (r.costUsdEstimated ?? 0),
    0,
  );
  const distinctRuntimes = Array.from(new Set(rows.map((r) => r.runtime)));
  const distinctAgents = Array.from(
    new Set(
      rows
        .map((r) => r.agentSlug)
        .filter((s): s is string => typeof s === "string" && s.length > 0),
    ),
  );

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <button
          onClick={onBack}
          className="text-sm text-cs-muted hover:text-cs-text"
        >
          ← Back to Sessions
        </button>
        <div className="text-xs text-cs-muted font-mono">{warRoomId}</div>
      </div>

      {/* Header summary card — counts, badges, total cost. */}
      <div className="rounded-lg border border-cs-accent/30 bg-cs-card p-4 space-y-3">
        <div className="flex flex-wrap items-center gap-3">
          <span
            className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-accent/15 text-cs-accent"
            title={`War-room ${warRoomId.slice(0, 8)}`}
          >
            ⚔ war room
          </span>
          <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
            seats
          </span>
          {distinctRuntimes.map((rt) => (
            <span key={rt} className={runtimeBadge(rt)}>
              {rt}
            </span>
          ))}
          {distinctAgents.map((slug) => (
            <span key={slug} className={personaBadge()}>
              {personaDisplay(slug)}
            </span>
          ))}
          <span className="text-xs text-cs-muted ml-auto">
            {rows.length} dispatch{rows.length !== 1 ? "es" : ""}
            {totalCost > 0 && (
              <span className="ml-2 font-mono">${totalCost.toFixed(4)}</span>
            )}
          </span>
        </div>
        <p className="text-[11px] text-cs-muted">
          Each card below is one seat's independent reply — no seat saw
          another's response (R1-parallel methodology). For a back-and-forth
          conversation, see Sessions.
        </p>
      </div>

      {/* One card per constituent dispatch. */}
      <div className="space-y-3">
        {rows.map((d) => {
          const isErr = d.status !== "success";
          return (
            <div
              key={d.id}
              className={cn(
                "rounded-lg border p-4 space-y-3",
                isErr
                  ? "border-cs-danger/40 bg-cs-card/40"
                  : "border-cs-border/60 bg-cs-card/60",
              )}
            >
              <div className="flex flex-wrap items-center gap-3">
                <span className={runtimeBadge(d.runtime)}>{d.runtime}</span>
                {d.agentSlug && (
                  <span className={personaBadge()}>
                    {personaDisplay(d.agentSlug)}
                  </span>
                )}
                <span
                  className={cn(
                    "px-1.5 py-0.5 rounded text-[10px] font-medium uppercase",
                    isErr
                      ? "bg-cs-danger/15 text-cs-danger"
                      : "bg-cs-muted/15 text-cs-muted",
                  )}
                >
                  {d.status}
                </span>
                {d.model && (
                  <span className="text-xs text-cs-muted font-mono">
                    {d.model}
                  </span>
                )}
                <span className="text-xs text-cs-muted ml-auto">
                  {formatTime(d.createdAt)}
                </span>
              </div>
              <div className="flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-cs-muted">
                {d.durationMs !== null && (
                  <span>
                    duration:{" "}
                    <span className="text-cs-text font-mono">
                      {(d.durationMs / 1000).toFixed(2)}s
                    </span>
                  </span>
                )}
                {(d.tokensIn !== null || d.tokensOut !== null) && (
                  <span>
                    tokens:{" "}
                    <span className="text-cs-text font-mono">
                      {d.tokensIn ?? 0} / {d.tokensOut ?? 0}
                    </span>
                  </span>
                )}
                {d.costUsdEstimated !== null && d.costUsdEstimated > 0 && (
                  <span>
                    cost:{" "}
                    <span className="text-cs-text font-mono">
                      ${d.costUsdEstimated.toFixed(4)}
                    </span>
                  </span>
                )}
              </div>

              <div className="rounded-lg border border-cs-border/40 bg-cs-bg/40 p-3">
                <div className="text-[10px] uppercase tracking-wider text-cs-muted font-medium mb-2">
                  Prompt
                </div>
                <pre className="text-xs text-cs-text whitespace-pre-wrap break-words font-mono">
                  {d.prompt ?? "(no prompt recorded)"}
                </pre>
              </div>

              <div className="rounded-lg border border-cs-border/40 bg-cs-bg/40 p-3">
                <div className="text-[10px] uppercase tracking-wider text-cs-muted font-medium mb-2">
                  Response
                </div>
                <pre className="text-xs text-cs-text whitespace-pre-wrap break-words font-mono">
                  {d.response ?? "(no response recorded)"}
                </pre>
              </div>

              {d.errorMessage && (
                <div className="rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3">
                  <div className="text-[10px] uppercase tracking-wider text-cs-danger font-medium mb-2">
                    Error
                  </div>
                  <pre className="text-xs text-cs-text whitespace-pre-wrap break-words font-mono">
                    {d.errorMessage}
                  </pre>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
