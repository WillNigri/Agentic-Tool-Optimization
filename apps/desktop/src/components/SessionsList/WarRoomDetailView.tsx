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
//
// 2026-06-18 — the header summary card, round-grouped seat cards, and
// receipts table moved to the shared WarRoomRoundsView so the team-shared
// snapshot view renders with identical UI/UX. The interactive bits
// (next-round input, close/reopen lifecycle, presence, share) stay here.

import { useState } from "react";
import { Loader2, Lock, Send, Sparkles, Tag as TagIcon, Unlock } from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

import { cn } from "@/lib/utils";
import { buildWarRoomSnapshot } from "@/lib/teamShareSnapshot";
import { runtimeBadge, formatTime } from "./_helpers";
import type { SingleRunDetail } from "./SingleRunDetailView";
import WarRoomRoundsView from "./WarRoomRoundsView";
import LiveCursors from "@/components/livePresence/LiveCursors";
import PresencePills from "@/components/livePresence/PresencePills";
import CloseConversationModal from "./CloseConversationModal";
import ShareWithTeamButton from "@/components/TeamWorkspaces/ShareWithTeamButton";

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
/// v2.7.14 — camelCase to match the getter (`get_war_room`) + every
/// other Tauri command surface. The earlier intentional snake_case
/// asymmetry (flagged by war-room 95C52D64) was resolved by flipping
/// the CLI `emit_json_close` payload to camelCase too. No future
/// contributor lands on a "why is this one snake_case?" foot-gun.
interface WarRoomCloseResult {
  id: string;
  status: string;
  autoTitle: string | null;
  summary: string | null;
  tags: string[];
  humanComment: string | null;
  coordinatorRuntime: string;
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

  // Next-round dispatch targets: the same seats as the LATEST round re-fire
  // (war-room seats don't change mid-conversation). Rows are pre-sorted by
  // (war_room_round ASC, created_at ASC) on the Tauri side. The round-grouped
  // seat cards + header + receipts now render in the shared WarRoomRoundsView;
  // here we only need the latest-round seat set to drive the next-round input.
  const roundBuckets = new Map<number, SingleRunDetail[]>();
  for (const r of rows) {
    const idx = r.warRoomRound ?? 1;
    if (!roundBuckets.has(idx)) roundBuckets.set(idx, []);
    roundBuckets.get(idx)!.push(r);
  }
  const sortedRoundKeys = Array.from(roundBuckets.keys()).sort((a, b) => a - b);
  const latestRound = sortedRoundKeys.at(-1) ?? 1;
  const nextRound = latestRound + 1;
  const nextRoundRuntimes = (roundBuckets.get(latestRound) ?? []).map(
    (r) => r.runtime,
  );

  return (
    <div className="relative space-y-4">
      <LiveCursors resourceKind="war_room" resourceId={warRoomId} />
      <div className="flex items-center justify-between gap-3">
        <button
          onClick={onBack}
          className="text-sm text-cs-muted hover:text-cs-text"
        >
          ← Back to Sessions
        </button>
        <div className="flex items-center gap-2">
          <PresencePills resourceKind="war_room" resourceId={warRoomId} />
          <ShareWithTeamButton
            resourceKind="war_room"
            resourceId={warRoomId}
            getSnapshot={() => buildWarRoomSnapshot(warRoomId)}
          />
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

      {/* Header summary card + round-grouped seat cards + receipts table —
          shared with the team-shared snapshot view (WarRoomRoundsView). The
          interactive "Send round N+1" input is injected as the roundsFooter
          so it keeps its position between the rounds and the receipts. */}
      <WarRoomRoundsView
        seats={rows}
        warRoomId={warRoomId}
        showReceiptLinks
        roundsFooter={
          nextRoundRuntimes.length > 0 ? (
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
          ) : null
        }
      />

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
