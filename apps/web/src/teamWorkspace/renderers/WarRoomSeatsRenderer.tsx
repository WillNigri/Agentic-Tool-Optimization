// Wave 2 — war-room seat stack renderer.
// Mirrors desktop WarRoomDetailView's per-seat cards but read-only:
// no send-next-round input, no close/reopen lifecycle, no receipt table
// (cost data already available inline on each seat).
//
// Snapshot shape: snapshot.seats[] from teamShareSnapshot.buildWarRoomSnapshot().
// Each seat is a SingleRunDetail row (camelCase on wire):
//   id, runtime, agent_slug, prompt, response, status, model,
//   duration_ms, tokens_in, tokens_out, cost_usd_estimated,
//   error_message, war_room_round, initiator_*

import InitiatorBadge from './InitiatorBadge';
import AttributionBadge from './AttributionBadge';
import {
  formatTime,
  runtimeChipClass,
  personaChipClass,
  personaDisplay,
} from './_helpers';

export interface SnapshotSeat {
  id?: string | null;
  runtime: string;
  agentSlug?: string | null;
  agent_slug?: string | null;
  prompt?: string | null;
  response?: string | null;
  status?: string | null;
  model?: string | null;
  durationMs?: number | null;
  duration_ms?: number | null;
  tokensIn?: number | null;
  tokens_in?: number | null;
  tokensOut?: number | null;
  tokens_out?: number | null;
  costUsdEstimated?: number | null;
  cost_usd_estimated?: number | null;
  errorMessage?: string | null;
  error_message?: string | null;
  warRoomRound?: number | null;
  war_room_round?: number | null;
  createdAt?: string | null;
  created_at?: string | null;
  initiatorKind?: string | null;
  initiator_kind?: string | null;
  clientSurface?: string | null;
  client_surface?: string | null;
  initiatorId?: string | null;
  initiator_id?: string | null;
  // Model A — who ran this seat (cloud member id) + on which machine.
  memberId?: string | null;
  member_id?: string | null;
  machineId?: string | null;
  machine_id?: string | null;
}

interface Props {
  seats: SnapshotSeat[];
}

/**
 * Vertical seat stack — one expandable card per seat.
 * Prompt and response are in <details> so long outputs don't overwhelm
 * the page. Error banner in red when status !== 'success'.
 */
export default function WarRoomSeatsRenderer({ seats }: Props) {
  if (seats.length === 0) {
    return (
      <div className="rounded-lg border border-[#2a2a3a] bg-[#16161e] p-6 text-center text-[#8888a0] text-sm">
        No seats in this war-room snapshot.
      </div>
    );
  }

  // Group by war_room_round — nulls collapse to round 1.
  const rounds = new Map<number, SnapshotSeat[]>();
  for (const seat of seats) {
    const r = seat.warRoomRound ?? seat.war_room_round ?? 1;
    if (!rounds.has(r)) rounds.set(r, []);
    rounds.get(r)!.push(seat);
  }
  const sortedRoundKeys = Array.from(rounds.keys()).sort((a, b) => a - b);

  return (
    <div className="space-y-6">
      {sortedRoundKeys.map((roundIdx) => {
        const roundSeats = rounds.get(roundIdx)!;
        return (
          <section key={roundIdx} className="space-y-3">
            {/* Round label */}
            <div className="flex items-center gap-2">
              <span className="px-1.5 py-0.5 rounded bg-[#00FFB2]/10 text-[#00FFB2] text-[10px] font-bold uppercase">
                Round {roundIdx}
              </span>
              <span className="text-[10px] text-[#8888a0]">
                {roundSeats.length} seat{roundSeats.length !== 1 ? 's' : ''} — fired in parallel
                {roundIdx > 1 ? '; each seat saw prior rounds’ replies' : ''}
              </span>
            </div>

            <div className="space-y-3">
              {roundSeats.map((seat, si) => {
                const agentSlug = seat.agentSlug ?? seat.agent_slug ?? null;
                const durationMs = seat.durationMs ?? seat.duration_ms ?? null;
                const tokensIn = seat.tokensIn ?? seat.tokens_in ?? null;
                const tokensOut = seat.tokensOut ?? seat.tokens_out ?? null;
                const cost = seat.costUsdEstimated ?? seat.cost_usd_estimated ?? null;
                const errorMessage = seat.errorMessage ?? seat.error_message ?? null;
                const createdAt = seat.createdAt ?? seat.created_at ?? null;
                const initiatorKind = seat.initiatorKind ?? seat.initiator_kind ?? null;
                const clientSurface = seat.clientSurface ?? seat.client_surface ?? null;
                const initiatorId = seat.initiatorId ?? seat.initiator_id ?? null;
                const memberId = seat.memberId ?? seat.member_id ?? null;
                const machineId = seat.machineId ?? seat.machine_id ?? null;
                const isErr = seat.status && seat.status !== 'success';

                return (
                  <div
                    key={seat.id ?? `r${roundIdx}-s${si}`}
                    className={`rounded-lg border p-4 space-y-3 ${
                      isErr
                        ? 'border-red-500/40 bg-[#16161e]/40'
                        : 'border-[#2a2a3a]/60 bg-[#16161e]/60'
                    }`}
                  >
                    {/* Card header: runtime + agent + status + model + time */}
                    <div className="flex flex-wrap items-center gap-2">
                      <span className={runtimeChipClass(seat.runtime)}>
                        {seat.runtime}
                      </span>
                      {agentSlug && (
                        <span className={personaChipClass()}>
                          {personaDisplay(agentSlug)}
                        </span>
                      )}
                      {seat.status && (
                        <span
                          className={`px-1.5 py-0.5 rounded text-[10px] font-medium uppercase ${
                            isErr
                              ? 'bg-red-500/15 text-red-400'
                              : 'bg-[#2a2a3a] text-[#8888a0]'
                          }`}
                        >
                          {seat.status}
                        </span>
                      )}
                      {seat.model && (
                        <span className="text-xs text-[#8888a0] font-mono">
                          {seat.model}
                        </span>
                      )}
                      {(initiatorKind || clientSurface) && (
                        <InitiatorBadge
                          initiatorKind={initiatorKind}
                          clientSurface={clientSurface}
                          initiatorId={initiatorId}
                        />
                      )}
                      <AttributionBadge memberId={memberId} machineId={machineId} />
                      {createdAt && (
                        <span className="ml-auto text-xs text-[#8888a0]">
                          {formatTime(createdAt)}
                        </span>
                      )}
                    </div>

                    {/* Stats row: duration / tokens / cost */}
                    {(durationMs !== null || tokensIn !== null || cost !== null) && (
                      <div className="flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-[#8888a0]">
                        {durationMs !== null && (
                          <span>
                            duration:{' '}
                            <span className="text-[#e8e8f0] font-mono">
                              {(durationMs / 1000).toFixed(2)}s
                            </span>
                          </span>
                        )}
                        {(tokensIn !== null || tokensOut !== null) && (
                          <span>
                            tokens:{' '}
                            <span className="text-[#e8e8f0] font-mono">
                              {tokensIn ?? 0} / {tokensOut ?? 0}
                            </span>
                          </span>
                        )}
                        {cost !== null && cost > 0 && (
                          <span>
                            cost:{' '}
                            <span className="text-[#e8e8f0] font-mono">
                              ${cost.toFixed(4)}
                            </span>
                          </span>
                        )}
                      </div>
                    )}

                    {/* Error banner */}
                    {errorMessage && (
                      <div className="rounded-lg border border-red-500/40 bg-red-500/10 p-3">
                        <div className="text-[10px] uppercase tracking-wider text-red-400 font-medium mb-2">
                          Error
                        </div>
                        <pre className="text-xs text-[#e8e8f0] whitespace-pre-wrap break-words font-mono">
                          {errorMessage}
                        </pre>
                      </div>
                    )}

                    {/* Prompt — collapsible */}
                    <details className="group">
                      <summary className="cursor-pointer select-none text-[10px] uppercase tracking-wider text-[#8888a0] font-medium list-none flex items-center gap-1">
                        <span className="group-open:rotate-90 transition-transform inline-block">▶</span>
                        Prompt
                      </summary>
                      <div className="mt-2 rounded-lg border border-[#2a2a3a]/40 bg-[#0a0a0f]/40 p-3">
                        <pre className="text-xs text-[#e8e8f0] whitespace-pre-wrap break-words font-mono">
                          {seat.prompt ?? '(no prompt recorded)'}
                        </pre>
                      </div>
                    </details>

                    {/* Response — collapsible, open by default.
                        Codex R1 follow-up — open:rotate-90 isn't a
                        Tailwind-native variant; use the same
                        group/group-open pattern the Prompt block
                        uses so the chevron actually rotates with
                        the <details> toggle. */}
                    <details open className="group">
                      <summary className="cursor-pointer select-none text-[10px] uppercase tracking-wider text-[#8888a0] font-medium list-none flex items-center gap-1">
                        <span className="group-open:rotate-90 transition-transform inline-block">▶</span>
                        Response
                      </summary>
                      <div className="mt-2 rounded-lg border border-[#2a2a3a]/40 bg-[#0a0a0f]/40 p-3">
                        <pre className="text-xs text-[#e8e8f0] whitespace-pre-wrap break-words font-mono">
                          {seat.response ?? '(no response recorded)'}
                        </pre>
                      </div>
                    </details>
                  </div>
                );
              })}
            </div>
          </section>
        );
      })}
    </div>
  );
}
