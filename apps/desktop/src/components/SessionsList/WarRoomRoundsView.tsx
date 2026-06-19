// WarRoomRoundsView — the shared, read-only presentation of a war-room's
// rounds. Extracted from WarRoomDetailView (2026-06-18) so the team-shared
// snapshot view (TeamWorkspaces/SharedDetailView) renders with the SAME
// UI/UX as a normal local war room — round headers, per-seat cards with
// runtime/model/status/metrics, prompt + response blocks, and the receipts
// table — instead of a flattened seat list. Will dogfood: "when i opened
// [a shared war room] i can see but its not the same ui/ux as normal war
// rooms why?"
//
// This component is purely presentational: it takes an already-fetched seat
// array and renders it. The interactive bits (next-round input, close/reopen
// lifecycle, live presence, share button) stay in WarRoomDetailView because
// they don't apply to a read-only remote snapshot.

import type { ReactNode } from "react";

import { cn } from "@/lib/utils";
import { runtimeBadge, personaBadge, personaDisplay, formatTime } from "./_helpers";
import InitiatorBadge from "@/components/InitiatorBadge";
import ExecutionLogReceipt from "@/components/receipts/ExecutionLogReceipt";

// Normalized seat shape (camelCase). Both the local view (SingleRunDetail,
// already camelCase) and the shared snapshot (snake_case, normalized by the
// caller) feed this. Only the fields the presentation reads are required.
export interface WarRoomSeat {
  id?: string | null;
  runtime: string;
  agentSlug?: string | null;
  model?: string | null;
  status?: string | null;
  prompt?: string | null;
  response?: string | null;
  errorMessage?: string | null;
  createdAt?: string | null;
  durationMs?: number | null;
  tokensIn?: number | null;
  tokensOut?: number | null;
  costUsdEstimated?: number | null;
  warRoomRound?: number | null;
  authMode?: string | null;
  initiatorKind?: string | null;
  clientSurface?: string | null;
  initiatorId?: string | null;
}

interface Props {
  seats: WarRoomSeat[];
  warRoomId?: string;
  /** Render each seat's "open run" receipt link. Only valid when the seats
   *  are local execution_logs rows (false for a remote shared snapshot). */
  showReceiptLinks?: boolean;
  /** Rendered between the round sections and the receipts table — the local
   *  view uses this for its interactive "Send round N+1" input. */
  roundsFooter?: ReactNode;
}

export default function WarRoomRoundsView({
  seats: rows,
  warRoomId,
  showReceiptLinks = true,
  roundsFooter,
}: Props) {
  const totalCost = rows.reduce((sum, r) => sum + (r.costUsdEstimated ?? 0), 0);
  const distinctRuntimes = Array.from(new Set(rows.map((r) => r.runtime)));
  const distinctAgents = Array.from(
    new Set(
      rows
        .map((r) => r.agentSlug)
        .filter((s): s is string => typeof s === "string" && s.length > 0),
    ),
  );

  // Receipts aggregation — mirrors SessionTranscriptView's table. Client-side
  // because the seat rows already carry cost / tokens / duration / authMode.
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
      agentSlug: r.agentSlug ?? null,
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
    if (r.costUsdEstimated === null || r.costUsdEstimated === undefined)
      acc.costNullTurns += 1;
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

  // Group by round. NULL warRoomRound collapses to 1 (matches the backfill
  // migration). Rows are expected pre-sorted by (round ASC, created_at ASC).
  const rounds = new Map<number, WarRoomSeat[]>();
  for (const r of rows) {
    const idx = r.warRoomRound ?? 1;
    if (!rounds.has(idx)) rounds.set(idx, []);
    rounds.get(idx)!.push(r);
  }
  const sortedRoundKeys = Array.from(rounds.keys()).sort((a, b) => a - b);

  return (
    <div className="space-y-4">
      {/* Header summary card — counts, badges, total cost. */}
      <div className="rounded-lg border border-cs-accent/30 bg-cs-card p-4 space-y-3">
        <div className="flex flex-wrap items-center gap-3">
          <span
            className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-accent/15 text-cs-accent"
            title={warRoomId ? `War room ${warRoomId.slice(0, 8)}` : "War room"}
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

      {/* Round-grouped seat cards. */}
      {sortedRoundKeys.map((roundIdx) => {
        const seats = rounds.get(roundIdx)!;
        return (
          <section key={roundIdx} className="space-y-3">
            <h3 className="text-[10px] uppercase tracking-wider text-cs-muted font-bold flex items-center gap-2">
              <span className="px-1.5 py-0.5 rounded bg-cs-accent/10 text-cs-accent">
                Round {roundIdx}
              </span>
              <span className="opacity-60">
                {seats.length} seat{seats.length !== 1 ? "s" : ""} — fired in
                parallel
                {roundIdx > 1
                  ? "; each seat saw every prior round's replies"
                  : ""}
              </span>
            </h3>
            <div className="space-y-3">
              {seats.map((d, seatIdx) => {
                const isErr = d.status !== "success";
                return (
                  <div
                    key={d.id ?? `${roundIdx}-${seatIdx}`}
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
                        {d.status ?? "unknown"}
                      </span>
                      {d.model && (
                        <span className="text-xs text-cs-muted font-mono">
                          {d.model}
                        </span>
                      )}
                      {(d.initiatorKind || d.clientSurface) && (
                        <InitiatorBadge
                          initiatorKind={d.initiatorKind ?? null}
                          clientSurface={d.clientSurface ?? null}
                          initiatorId={d.initiatorId ?? null}
                        />
                      )}
                      {showReceiptLinks && d.id && (
                        <ExecutionLogReceipt logId={d.id} label="open run" />
                      )}
                      {d.createdAt && (
                        <span className="text-xs text-cs-muted ml-auto">
                          {formatTime(d.createdAt)}
                        </span>
                      )}
                    </div>
                    <div className="flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-cs-muted">
                      {d.durationMs !== null && d.durationMs !== undefined && (
                        <span>
                          duration:{" "}
                          <span className="text-cs-text font-mono">
                            {(d.durationMs / 1000).toFixed(2)}s
                          </span>
                        </span>
                      )}
                      {(d.tokensIn != null || d.tokensOut != null) && (
                        <span>
                          tokens:{" "}
                          <span className="text-cs-text font-mono">
                            {d.tokensIn ?? 0} / {d.tokensOut ?? 0}
                          </span>
                        </span>
                      )}
                      {d.costUsdEstimated != null && d.costUsdEstimated > 0 && (
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

      {roundsFooter}

      {/* Receipts table. Aggregated client-side from the per-seat rows. */}
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
            Costs estimated from published per-runtime rates × tokens used. For
            metered providers (api_key) this should match your bill. For
            subscription runtimes this is the equivalent if you were paying
            per-token. "$?" means the model is missing from the pricing table.
          </div>
        </div>
      )}
    </div>
  );
}
