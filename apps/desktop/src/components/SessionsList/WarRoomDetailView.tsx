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

import { useState } from "react";
import { Loader2, Lock, Send, Sparkles, Tag as TagIcon, Unlock } from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

import { cn } from "@/lib/utils";
import {
  runtimeBadge,
  personaBadge,
  personaDisplay,
  formatTime,
} from "./_helpers";
import type { SingleRunDetail } from "./SingleRunDetailView";
import CloseConversationModal from "./CloseConversationModal";

interface WarRoomDispatchResult {
  warRoomId: string;
  round: number;
}

/// War-rooms row snapshot returned by `get_war_room`. Maps directly
/// to commands::war_rooms::WarRoom on the Rust side. v2.7.14: serde
/// rename_all = "camelCase" is set there now so the wire shape
/// matches every other Tauri command's response.
interface WarRoomSnapshot {
  id: string;
  status: "open" | "closed";
  closedAt: string | null;
  autoTitle: string | null;
  summary: string | null;
  coordinatorRuntime: string | null;
  humanComment: string | null;
  tags: string[];
  seatCount: number;
}

/// Mirrors the `ato war-rooms close` JSON payload. Used by the close
/// flow to surface the coordinator's response in the summary card
/// immediately, without re-querying.
///
/// v2.7.14 NOTE: snake_case here is *intentional asymmetry* with
/// `WarRoomSnapshot` above (which is camelCase to match the rest of
/// the Tauri command surface). The close-payload comes from CLI
/// `emit_json_close` which uses hand-rolled `serde_json::json!()`
/// with snake_case literal keys. The close handler doesn't actually
/// read these fields — it just awaits the result and refetches the
/// snapshot, which IS camelCase. If a future contributor wires these
/// to render directly, either align the close-payload keys to
/// camelCase in `commands::war_rooms::emit_json_close` OR rewrite
/// these field names to camelCase + add an adapter on the close
/// boundary. War-room 95C52D64 reviewers (claude + minimax) flagged
/// the asymmetry; consensus was "ok for now, comment + revisit".
interface WarRoomCloseResult {
  id: string;
  status: string;
  auto_title: string | null;
  summary: string | null;
  tags: string[];
  human_comment: string | null;
  coordinator_runtime: string;
}

export default function WarRoomDetailView({
  warRoomId,
  onBack,
}: {
  warRoomId: string;
  onBack: () => void;
}) {
  const qc = useQueryClient();
  const q = useQuery<SingleRunDetail[]>({
    queryKey: ["war-room-constituents", warRoomId],
    queryFn: () =>
      invoke<SingleRunDetail[]>("get_war_room_constituents", { warRoomId }),
    staleTime: 60_000,
  });
  // v2.7.13 — war_rooms row snapshot for the close lifecycle. Falls
  // through gracefully when the war_rooms row doesn't exist yet
  // (legacy war room that pre-dates the table) — get_war_room returns
  // a synthetic 'open' row in that case.
  const snapshotQ = useQuery<WarRoomSnapshot>({
    queryKey: ["war-room-snapshot", warRoomId],
    queryFn: () => invoke<WarRoomSnapshot>("get_war_room", { warRoomId }),
    staleTime: 30_000,
    // Don't surface errors here — the snapshot is a render hint, not
    // load-bearing. The constituents query above is the actual gate.
    retry: false,
  });
  const isClosed = snapshotQ.data?.status === "closed";

  // Close lifecycle state — mirrors SessionTranscriptView's shape so
  // the cancel/error/blocker UX is consistent across conversation
  // types.
  const [closing, setClosing] = useState(false);
  const [closeError, setCloseError] = useState<string | null>(null);
  const [reopening, setReopening] = useState(false);
  const [reopenError, setReopenError] = useState<string | null>(null);
  const [closeModalOpen, setCloseModalOpen] = useState(false);

  const handleClose = async (opts: {
    coordinator: string | null;
    humanComment: string | null;
  }) => {
    if (closing) return;
    setCloseModalOpen(false);
    setClosing(true);
    setCloseError(null);
    setReopenError(null);
    try {
      await invoke<WarRoomCloseResult>("close_war_room", {
        warRoomId,
        agentSlug: null,
        model: null,
        coordinator: opts.coordinator,
        humanComment: opts.humanComment,
      });
      await qc.invalidateQueries({ queryKey: ["war-room-snapshot", warRoomId] });
      await qc.invalidateQueries({ queryKey: ["sessions-full"] });
    } catch (e) {
      const msg = String(e);
      if (!msg.includes("__cancelled__")) {
        setCloseError(msg);
      }
    } finally {
      setClosing(false);
    }
  };

  const handleReopen = async () => {
    if (reopening) return;
    setReopening(true);
    setReopenError(null);
    setCloseError(null);
    try {
      await invoke("reopen_war_room", { warRoomId });
      await qc.invalidateQueries({ queryKey: ["war-room-snapshot", warRoomId] });
      await qc.invalidateQueries({ queryKey: ["sessions-full"] });
    } catch (e) {
      setReopenError(String(e));
    } finally {
      setReopening(false);
    }
  };
  // PR 16-PR-B — "Send next round" input state. Disabled while a
  // round is in flight (the parallel dispatches block this Tauri
  // call until all seats return, so the loading flag tracks the
  // user's intent reliably).
  const [nextRoundPrompt, setNextRoundPrompt] = useState("");
  const sendNextRound = useMutation({
    mutationFn: async ({
      runtimes,
      prompt,
      round,
    }: {
      runtimes: string[];
      prompt: string;
      round: number;
    }) => {
      return await invoke<WarRoomDispatchResult>("dispatch_war_room", {
        runtimes,
        prompt,
        warRoomId,
        round,
      });
    },
    onSuccess: async () => {
      // Re-fetch constituents so the new round's cards appear.
      // The list_sessions_full cache also gets invalidated so the
      // war-room card on Sessions shows the new participant count
      // and last-used timestamp.
      await qc.invalidateQueries({
        queryKey: ["war-room-constituents", warRoomId],
      });
      await qc.invalidateQueries({ queryKey: ["sessions-full"] });
      setNextRoundPrompt("");
    },
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
          Could not load war room
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

  // 2026-05-19 — receipts table mirrors SessionTranscriptView's. War-rooms
  // were missing it; Will caught it dogfooding. Aggregation is client-side
  // because q.data already has every per-seat dispatch with cost / tokens /
  // duration / authMode — no backend Tauri call needed.
  type ReceiptRow = {
    runtime: string;
    agentSlug: string | null;
    billingMode: string;
    successfulTurns: number;
    totalTurns: number;
    tokensIn: number;
    tokensOut: number;
    totalDurationMs: number;
    totalCostUsd: number;
    costNullTurns: number;
  };
  const receiptMap = new Map<string, ReceiptRow>();
  for (const r of rows) {
    const billing = r.authMode ?? "unknown";
    const key = `${r.runtime}|${r.agentSlug ?? ""}|${billing}`;
    const acc = receiptMap.get(key) ?? {
      runtime: r.runtime,
      agentSlug: r.agentSlug,
      billingMode: billing,
      successfulTurns: 0,
      totalTurns: 0,
      tokensIn: 0,
      tokensOut: 0,
      totalDurationMs: 0,
      totalCostUsd: 0,
      costNullTurns: 0,
    };
    acc.totalTurns += 1;
    if (r.status === "success") acc.successfulTurns += 1;
    acc.tokensIn += r.tokensIn ?? 0;
    acc.tokensOut += r.tokensOut ?? 0;
    acc.totalDurationMs += r.durationMs ?? 0;
    if (r.costUsdEstimated === null) acc.costNullTurns += 1;
    else acc.totalCostUsd += r.costUsdEstimated;
    receiptMap.set(key, acc);
  }
  const receiptRows = Array.from(receiptMap.values()).sort(
    (a, b) => b.totalCostUsd - a.totalCostUsd,
  );
  const totalDurationMs = rows.reduce((s, r) => s + (r.durationMs ?? 0), 0);
  const totalTokens = rows.reduce(
    (s, r) => s + (r.tokensIn ?? 0) + (r.tokensOut ?? 0),
    0,
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
        <div className="flex items-center gap-2">
          <div className="text-xs text-cs-muted font-mono">{warRoomId}</div>
          {/* v2.7.13 — close + reopen buttons (mirrors the session
              detail view's lifecycle controls). Disabled while a
              dispatch is in flight or no seats have replied yet. */}
          {isClosed ? (
            <button
              onClick={handleReopen}
              disabled={reopening}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border bg-cs-card hover:bg-cs-border/30 text-sm font-medium disabled:opacity-40 disabled:cursor-not-allowed"
              title="Reopen this war room. The next close will refresh the summary with any newly-added seats or rounds."
            >
              {reopening ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <Unlock size={14} />
              )}
              {reopening ? "Reopening…" : "Reopen"}
            </button>
          ) : (
            <button
              onClick={() => setCloseModalOpen(true)}
              disabled={closing || q.data.length === 0}
              className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border bg-cs-card hover:bg-cs-border/30 text-sm font-medium disabled:opacity-40 disabled:cursor-not-allowed"
              title="Close this war room. You'll pick the coordinator LLM and can attach a note before the seats are summarized."
            >
              <Lock size={14} /> Close war room
            </button>
          )}
        </div>
      </div>

      {/* v2.7.13 — coordinator summary card at the TOP (Will dogfood
          2026-05-21: war-room used to render this below the seat list
          which buried it; session view always rendered above). Same
          shape as SessionTranscriptView's summary card. */}
      {isClosed && snapshotQ.data?.summary && (
        <div className="border border-cs-accent/30 rounded-md bg-cs-accent/5 p-3 space-y-2">
          <div className="text-xs font-medium uppercase text-cs-accent flex items-center gap-2">
            <Sparkles size={12} /> Coordinator summary
            {snapshotQ.data.closedAt && (
              <span className="text-[10px] text-cs-muted normal-case font-normal">
                · closed {formatTime(snapshotQ.data.closedAt)}
              </span>
            )}
            {snapshotQ.data.coordinatorRuntime && (
              <span className={cn(runtimeBadge(snapshotQ.data.coordinatorRuntime), "normal-case")}>
                {snapshotQ.data.coordinatorRuntime}
              </span>
            )}
          </div>
          {snapshotQ.data.autoTitle && (
            <div className="text-sm font-medium text-cs-text">
              {snapshotQ.data.autoTitle}
            </div>
          )}
          <div className="text-sm text-cs-text whitespace-pre-wrap">
            {snapshotQ.data.summary}
          </div>
          {/* v2.7.13 fix — human's free-form note. Rendered as a
              distinct sub-block so a glance separates LLM output from
              human framing. Skipped when null/empty. */}
          {snapshotQ.data.humanComment && snapshotQ.data.humanComment.trim() && (
            <div className="border-t border-cs-accent/20 pt-2 mt-2">
              <div className="text-[10px] uppercase tracking-wider font-medium text-cs-muted mb-1">
                Note from human
              </div>
              <div className="text-sm text-cs-text whitespace-pre-wrap">
                {snapshotQ.data.humanComment}
              </div>
            </div>
          )}
          {snapshotQ.data.tags.length > 0 && (
            <div className="flex items-center gap-1 flex-wrap pt-1">
              <TagIcon size={10} className="text-cs-muted" />
              {snapshotQ.data.tags.map((tag) => (
                <span
                  key={tag}
                  className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-cs-accent/10 text-cs-accent"
                >
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Header summary card — counts, badges, total cost. */}
      <div className="rounded-lg border border-cs-accent/30 bg-cs-card p-4 space-y-3">
        <div className="flex flex-wrap items-center gap-3">
          <span
            className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-accent/15 text-cs-accent"
            title={`War room ${warRoomId.slice(0, 8)}`}
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
          Within a round, every seat fires in parallel and doesn't see the
          others' replies. Between rounds, every seat sees the FULL transcript
          of prior rounds before the next dispatch. For a sequential back-and-
          forth (each turn anchored on the prior), see Sessions instead.
        </p>
      </div>

      {/* PR 16-PR-B — group by round. Rows are already sorted by
          (war_room_round ASC, created_at ASC) on the Tauri side. */}
      {(() => {
        // Build round buckets. NULL warRoomRound collapses to 1
        // (matches the backfill migration).
        const rounds: Map<number, SingleRunDetail[]> = new Map();
        for (const r of rows) {
          const idx = r.warRoomRound ?? 1;
          if (!rounds.has(idx)) rounds.set(idx, []);
          rounds.get(idx)!.push(r);
        }
        const sortedRoundKeys = Array.from(rounds.keys()).sort((a, b) => a - b);
        const latestRound = sortedRoundKeys.at(-1) ?? 1;
        const nextRound = latestRound + 1;
        // Distinct (runtime, agent_slug) pairs from THE LATEST
        // round drive the next-round dispatch. Per Will's spec
        // war-room seats don't change mid-conversation — same seats
        // re-fire each round.
        const latestSeats = rounds.get(latestRound) ?? [];
        const nextRoundRuntimes = latestSeats.map((r) => r.runtime);
        return (
          <>
            {sortedRoundKeys.map((roundIdx) => {
              const seats = rounds.get(roundIdx)!;
              return (
                <section key={roundIdx} className="space-y-3">
                  <h3 className="text-[10px] uppercase tracking-wider text-cs-muted font-bold flex items-center gap-2">
                    <span className="px-1.5 py-0.5 rounded bg-cs-accent/10 text-cs-accent">
                      Round {roundIdx}
                    </span>
                    <span className="opacity-60">
                      {seats.length} seat{seats.length !== 1 ? "s" : ""} —
                      fired in parallel
                      {roundIdx > 1
                        ? "; each seat saw every prior round's replies"
                        : ""}
                    </span>
                  </h3>
                  <div className="space-y-3">
                    {seats.map((d) => {
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
                </section>
              );
            })}

            {/* PR 16-PR-B — "Send round N+1" input. Same seats
                re-fire in parallel; each will see the full
                transcript of rounds 1..N when forming its R_(N+1)
                reply. Disabled while a round is in flight. */}
            {nextRoundRuntimes.length > 0 && (
              <section className="rounded-lg border border-cs-accent/40 bg-cs-card p-4 space-y-3">
                <div className="flex items-center gap-2">
                  <span className="text-[10px] uppercase tracking-wider text-cs-accent font-bold">
                    Send round {nextRound}
                  </span>
                  <span className="text-[10px] text-cs-muted">
                    same {nextRoundRuntimes.length} seat
                    {nextRoundRuntimes.length !== 1 ? "s" : ""} re-fire in
                    parallel; each will see rounds 1–{latestRound} replies
                  </span>
                </div>
                <textarea
                  value={nextRoundPrompt}
                  onChange={(e) => setNextRoundPrompt(e.target.value)}
                  placeholder={`Round ${nextRound} prompt — what do you want each seat to react to given the prior rounds?`}
                  rows={3}
                  className="w-full bg-cs-bg-raised border border-cs-border rounded-md p-2 text-xs font-mono focus:outline-none focus:border-cs-accent resize-none"
                  disabled={sendNextRound.isPending}
                />
                {sendNextRound.isError && (
                  <div className="text-xs text-cs-danger">
                    {sendNextRound.error instanceof Error
                      ? sendNextRound.error.message
                      : String(sendNextRound.error)}
                  </div>
                )}
                <div className="flex items-center justify-end gap-2">
                  <button
                    type="button"
                    onClick={() =>
                      sendNextRound.mutate({
                        runtimes: nextRoundRuntimes,
                        prompt: nextRoundPrompt.trim(),
                        round: nextRound,
                      })
                    }
                    disabled={
                      sendNextRound.isPending || nextRoundPrompt.trim() === ""
                    }
                    className="inline-flex items-center gap-2 rounded-md bg-cs-accent text-cs-bg px-3 py-1.5 text-xs font-medium hover:opacity-90 disabled:opacity-40"
                  >
                    {sendNextRound.isPending ? (
                      <>
                        <Loader2 size={12} className="animate-spin" />
                        Firing round {nextRound}…
                      </>
                    ) : (
                      <>
                        <Send size={12} />
                        Send round {nextRound}
                      </>
                    )}
                  </button>
                </div>
              </section>
            )}
          </>
        );
      })()}

      {/* 2026-05-19 — Receipts table. Mirrors SessionTranscriptView's;
          war-rooms shipped without this affordance. Aggregated client-
          side from the per-seat rows q.data already loads. */}
      {receiptRows.length > 0 && (
        <div className="border border-cs-border rounded-lg overflow-hidden">
          <div className="px-3 py-2 bg-cs-card border-b border-cs-border flex items-center justify-between">
            <span className="text-xs font-medium text-cs-text uppercase tracking-wide">
              Receipts
            </span>
            <span className="text-xs text-cs-muted font-mono">
              total{" "}
              <span className="text-cs-accent">
                {totalCost === 0
                  ? "free (subscription)"
                  : `$${totalCost.toFixed(4)}`}
              </span>
              {" · "}
              {rows.length} dispatch{rows.length !== 1 ? "es" : ""}
              {" · "}
              {(totalDurationMs / 1000).toFixed(1)}s
              {" · "}
              {totalTokens.toLocaleString()} tok
            </span>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead className="text-cs-muted border-b border-cs-border bg-cs-card/40">
                <tr>
                  <th className="text-left px-3 py-1.5 font-medium">Runtime</th>
                  <th className="text-left px-3 py-1.5 font-medium">Seat</th>
                  <th className="text-right px-3 py-1.5 font-medium">Turns</th>
                  <th className="text-right px-3 py-1.5 font-medium">Tokens in</th>
                  <th className="text-right px-3 py-1.5 font-medium">Tokens out</th>
                  <th className="text-right px-3 py-1.5 font-medium">Duration</th>
                  <th className="text-right px-3 py-1.5 font-medium">Cost</th>
                </tr>
              </thead>
              <tbody className="font-mono">
                {receiptRows.map((row, i) => (
                  <tr
                    key={`${row.runtime}-${row.agentSlug ?? "_"}-${i}`}
                    className="border-b border-cs-border/40 last:border-0"
                  >
                    <td className="px-3 py-1.5">
                      <span className={runtimeBadge(row.runtime)}>
                        {row.runtime}
                      </span>
                    </td>
                    <td className="px-3 py-1.5">
                      {row.agentSlug ? (
                        <span className={personaBadge()}>
                          {personaDisplay(row.agentSlug)}
                        </span>
                      ) : (
                        <span className="text-cs-muted italic">generalist</span>
                      )}
                    </td>
                    <td className="text-right px-3 py-1.5">
                      {row.successfulTurns}
                      {row.totalTurns !== row.successfulTurns && (
                        <span
                          className="text-cs-muted ml-1"
                          title={`${row.totalTurns - row.successfulTurns} error turn(s)`}
                        >
                          (+{row.totalTurns - row.successfulTurns}e)
                        </span>
                      )}
                    </td>
                    <td className="text-right px-3 py-1.5 text-cs-muted">
                      {row.tokensIn.toLocaleString()}
                    </td>
                    <td className="text-right px-3 py-1.5 text-cs-muted">
                      {row.tokensOut.toLocaleString()}
                    </td>
                    <td className="text-right px-3 py-1.5 text-cs-muted">
                      {(row.totalDurationMs / 1000).toFixed(1)}s
                    </td>
                    <td
                      className={cn(
                        "text-right px-3 py-1.5",
                        row.totalCostUsd === 0 ? "text-cs-muted" : "text-cs-text",
                      )}
                      title={
                        row.billingMode === "subscription"
                          ? "Subscription auth (Claude Code / Codex CLI / Gemini CLI). No per-token billing — cost is the equivalent if you were paying per-token directly."
                          : row.billingMode === "local"
                            ? "Local runtime (Ollama / OpenClaw / Hermes). No network, no cost."
                            : row.costNullTurns > 0
                              ? `${row.costNullTurns} turn(s) had no cost computed — model missing from pricing table.`
                              : "Estimated from published per-token rates."
                      }
                    >
                      {row.costNullTurns > 0 ? (
                        <span className="text-amber-400">
                          $?{" "}
                          <span className="text-[10px]">(pricing missing)</span>
                        </span>
                      ) : row.billingMode === "local" ? (
                        <span className="text-cs-muted">local</span>
                      ) : row.totalCostUsd === 0 ? (
                        row.billingMode === "subscription" ? (
                          <span className="text-cs-muted">subscription</span>
                        ) : (
                          <span className="text-cs-muted">$0.0000</span>
                        )
                      ) : row.billingMode === "subscription" ? (
                        <span>
                          <span className="text-cs-muted">≈ </span>
                          ${row.totalCostUsd.toFixed(4)}
                          <span className="text-[10px] text-cs-muted ml-1">
                            (sub est.)
                          </span>
                        </span>
                      ) : (
                        <>${row.totalCostUsd.toFixed(4)}</>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="px-3 py-1.5 text-[10px] text-cs-muted border-t border-cs-border/40">
            Costs estimated from published per-runtime rates × tokens used.
            For metered providers (api_key) this should match your bill. For
            subscription runtimes this is the equivalent if you were paying
            per-token. "$?" means the model is missing from the pricing
            table — see{" "}
            <code className="text-cs-text">
              apps/cli/src/runtime.rs:pricing_for_model
            </code>
            .
          </div>
        </div>
      )}

      {/* Close + reopen error banners. Mirrors the session view's
          inline error surface; the modal/blocker absorbs the success
          path so these only render when something went wrong. */}
      {closeError && (
        <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
          <strong className="text-cs-danger">Close failed:</strong> {closeError}
        </div>
      )}
      {reopenError && (
        <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
          <strong className="text-cs-danger">Reopen failed:</strong>{" "}
          {reopenError}
        </div>
      )}

      {/* Blocking close modal — coordinator is summarizing. Identical
          shape to SessionTranscriptView's blocker. Cancel here aborts
          the underlying `ato war-rooms close` subprocess. */}
      {closing && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-cs-bg/80 backdrop-blur-sm">
          <div className="border border-cs-border bg-cs-card rounded-lg p-6 max-w-md w-full mx-4 space-y-4">
            <div className="flex items-center gap-3">
              <Loader2
                size={20}
                className="animate-spin text-cs-accent shrink-0"
              />
              <div className="flex-1 min-w-0">
                <div className="text-sm font-medium text-cs-text">
                  Coordinator is summarizing the war room…
                </div>
                <div className="text-xs text-cs-muted mt-1">
                  Reading every seat's reply across all rounds. Typically
                  finishes in 5–20 seconds.
                </div>
              </div>
            </div>
            <button
              type="button"
              onClick={() =>
                void invoke("cancel_close_session", {
                  sessionId: warRoomId,
                }).catch(() => undefined)
              }
              className="w-full px-3 py-1.5 rounded-md border border-cs-border bg-cs-card text-sm text-cs-muted hover:text-cs-text"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      <CloseConversationModal
        open={closeModalOpen}
        busy={closing}
        conversationType="war_room"
        onCancel={() => setCloseModalOpen(false)}
        onSubmit={handleClose}
      />
    </div>
  );
}
