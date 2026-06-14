// v2.15 Wave 2 — Live-wired detail view for team-shared resources.
//
// Mount sequence:
//   1. REST snapshot fetch → extract last_seq (default 0).
//   2. useTeamEventStream subscribes to WS with ?since=last_seq.
//   3. Render: snapshot events FIRST, then live-arrived events after a
//      "Live since you opened" divider.
//   4. AppendTurnComposer at the bottom (disabled for E2E shares until
//      Wave 3).
//
// Supported kinds in this file:
//   session / war_room / chat (v2.14 originals)
//   loop / mission (v2.14 #12 additions, stub bodies)
//
// Wave 3 TODO: wire encryption_mode from snapshot → isE2e prop +
//   decrypt payload_json from live events before rendering.

import { useCallback, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Eye, Lock, Radio } from "lucide-react";

import { cn } from "@/lib/utils";
import {
  getSharedChatDetail,
  getSharedSessionDetail,
  getSharedWarRoomDetail,
  getSharedLoopDetail,
  getSharedMissionDetail,
  backfillTeamEvents,
  decryptEventPayload,
  getTeamMemberE2eKeys,
  type SharedChatDetail,
  type SharedSessionDetail,
  type SharedWarRoomDetail,
  type SharedLoopDetail,
  type SharedMissionDetail,
  type TeamEvent,
} from "@/lib/cloud-api";
import { loadTeamKey, TeamKeyUnsealError } from "@/lib/e2e/teamKey";
import { fromBase64 } from "@/lib/e2e/crypto";
import { formatTime } from "@/components/SessionsList/_helpers";
import { useTeamEventStream } from "./useTeamEventStream";
import type { DecryptorFn } from "@/lib/teamEventStream";
import AppendTurnComposer from "./AppendTurnComposer";
import type { SharedResourceKind } from "@/lib/cloud-api";

// Re-export so callers that previously imported from here keep working.
export type { SharedResourceKind };

interface SharedDetailViewProps {
  resourceKind: SharedResourceKind;
  teamId: string;
  resourceId: string;
  onBack: () => void;
}

type SharedDetail =
  | SharedSessionDetail
  | SharedWarRoomDetail
  | SharedChatDetail
  | SharedLoopDetail
  | SharedMissionDetail;

async function fetcher(
  resourceKind: SharedResourceKind,
  teamId: string,
  resourceId: string,
): Promise<SharedDetail> {
  if (resourceKind === "session") return getSharedSessionDetail(teamId, resourceId);
  if (resourceKind === "war-room") return getSharedWarRoomDetail(teamId, resourceId);
  if (resourceKind === "chat") return getSharedChatDetail(teamId, resourceId);
  if (resourceKind === "loop") return getSharedLoopDetail(teamId, resourceId);
  return getSharedMissionDetail(teamId, resourceId);
}

export default function SharedDetailView({
  resourceKind,
  teamId,
  resourceId,
  onBack,
}: SharedDetailViewProps) {
  const { t } = useTranslation();
  const q = useQuery<SharedDetail>({
    queryKey: ["shared-detail", resourceKind, teamId, resourceId],
    queryFn: () => fetcher(resourceKind, teamId, resourceId),
    staleTime: 60_000,
  });

  // Extract last_seq from the snapshot response (Wave 2+).
  const lastSeq = (q.data as (SharedDetail & { last_seq?: number }) | undefined)
    ?.last_seq ?? 0;

  // Determine whether the share is E2E (Wave 3).
  const isE2e =
    (q.data as (SharedDetail & { encryption_mode?: string }) | undefined)
      ?.encryption_mode === "e2e";

  // E2E state: backfilled+decrypted historical events.
  const [e2eHistoryEvents, setE2eHistoryEvents] = useState<TeamEvent[]>([]);
  const [e2eHistorySince, setE2eHistorySince] = useState(0);
  const [e2eHistoryLoading, setE2eHistoryLoading] = useState(false);
  const [e2eHistoryExhausted, setE2eHistoryExhausted] = useState(false);
  // team_key_id is in the snapshot response when encryption_mode=e2e.
  const teamKeyId = (q.data as (SharedDetail & { team_key_id?: string }) | undefined)
    ?.team_key_id ?? null;

  // Build the AD-hint for a given raw event. The hint must match what
  // appendTeamEventEncrypted wrote: teamId|resourceId|seq_num|event_kind.
  const buildAdHint = useCallback(
    (event: TeamEvent) =>
      `${teamId}|${resourceId}|${event.seq_num}|${event.event_kind}`,
    [teamId, resourceId],
  );

  // Build a stable decryptor by lazily loading the Team Key and member pubkeys.
  // The closure captures teamKeyId + buildAdHint; re-created when those change.
  const decryptor = useCallback<DecryptorFn>(
    async (raw: TeamEvent) => {
      if (!teamKeyId) return { ...raw, payload_json: { __decrypt_error: true } };
      try {
        const teamKey = await loadTeamKey(teamKeyId);
        // Preload member pubkeys for signature verification.
        const memberKeyList = await getTeamMemberE2eKeys(teamId);
        const memberPubkeys: Record<string, { ed25519_pubkey: string; key_id: string }> = {};
        for (const m of memberKeyList) {
          memberPubkeys[m.member_user_id] = {
            ed25519_pubkey: m.ed25519_pubkey,
            key_id: m.key_id,
          };
        }
        // Attach AD hint for signature verification inside decryptEventPayload.
        const withHint = { ...raw, __ad_hint: buildAdHint(raw) } as TeamEvent & { __ad_hint: string };
        return decryptEventPayload(withHint, teamKey, memberPubkeys);
      } catch {
        return { ...raw, payload_json: { __decrypt_error: true } };
      }
    },
    [teamKeyId, teamId, buildAdHint],
  );

  // For E2E shares, fetch the initial 200-event backfill (replaces snapshot).
  const loadE2eHistory = useCallback(
    async (since: number) => {
      if (!isE2e || !teamKeyId) return;
      setE2eHistoryLoading(true);
      try {
        const rawBatch = await backfillTeamEvents(teamId, resourceKind, resourceId, since, 200);
        if (rawBatch.length < 200) setE2eHistoryExhausted(true);
        const decryptedBatch = await Promise.all(rawBatch.map(decryptor));
        setE2eHistoryEvents((prev) => {
          const all = [...prev, ...decryptedBatch];
          // Sort by seq_num, dedup.
          const seen = new Set<number>();
          return all
            .filter((e) => { if (seen.has(e.seq_num)) return false; seen.add(e.seq_num); return true; })
            .sort((a, b) => a.seq_num - b.seq_num);
        });
        if (rawBatch.length > 0) {
          setE2eHistorySince(rawBatch[rawBatch.length - 1].seq_num);
        }
      } finally {
        setE2eHistoryLoading(false);
      }
    },
    [isE2e, teamKeyId, teamId, resourceKind, resourceId, decryptor],
  );

  // Trigger initial E2E backfill when the snapshot loads and confirms e2e.
  // Use a ref to guard against firing twice in React Strict Mode.
  const e2eBackfillFiredRef = { current: false };
  if (isE2e && teamKeyId && !e2eBackfillFiredRef.current && e2eHistoryEvents.length === 0 && !e2eHistoryLoading) {
    e2eBackfillFiredRef.current = true;
    void loadE2eHistory(0);
  }

  // Derive the initial seq for the live stream.
  // Plaintext: use snapshot's last_seq. E2E: start after the last history event.
  const liveStreamSince = isE2e
    ? (e2eHistoryEvents.length > 0 ? e2eHistoryEvents[e2eHistoryEvents.length - 1].seq_num : 0)
    : lastSeq;

  const { events: liveEvents, isConnected } = useTeamEventStream(
    q.data ? teamId : null,
    q.data ? resourceKind : null,
    q.data ? resourceId : null,
    liveStreamSince,
    // Pass decryptor only for E2E shares.
    isE2e ? { decryptor } : undefined,
  );

  if (q.isLoading) {
    return (
      <div className="space-y-4">
        <button
          onClick={onBack}
          className="inline-flex items-center gap-1 text-sm text-cs-muted hover:text-cs-text"
        >
          <ArrowLeft size={14} />{" "}
          {t("teamShare.detail.back", { defaultValue: "Back to sessions" })}
        </button>
        <div className="rounded-md border border-cs-border/40 bg-cs-card/40 p-4 text-sm text-cs-muted">
          {t("teamShare.detail.loading", { defaultValue: "Loading shared snapshot…" })}
        </div>
      </div>
    );
  }
  if (q.isError || !q.data) {
    return (
      <div className="space-y-4">
        <button
          onClick={onBack}
          className="inline-flex items-center gap-1 text-sm text-cs-muted hover:text-cs-text"
        >
          <ArrowLeft size={14} />{" "}
          {t("teamShare.detail.back", { defaultValue: "Back to sessions" })}
        </button>
        <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 p-4 text-sm text-cs-text">
          {q.error instanceof Error
            ? q.error.message
            : t("teamShare.detail.failed", {
                defaultValue: "Could not load the shared snapshot.",
              })}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {/* Header row: back + badges */}
      <div className="flex items-center justify-between gap-3">
        <button
          onClick={onBack}
          className="inline-flex items-center gap-1 text-sm text-cs-muted hover:text-cs-text"
        >
          <ArrowLeft size={14} />{" "}
          {t("teamShare.detail.back", { defaultValue: "Back to sessions" })}
        </button>
        <div className="flex items-center gap-2 text-[10px]">
          {/* isConnected indicator */}
          <span
            title={
              isConnected
                ? t("teamShare.detail.liveConnected", { defaultValue: "Live" })
                : t("teamShare.detail.liveDisconnected", {
                    defaultValue: "Connecting…",
                  })
            }
            className={cn(
              "inline-flex items-center gap-1 rounded-full px-2 py-0.5 font-medium",
              isConnected
                ? "border border-emerald-500/40 bg-emerald-500/10 text-emerald-400"
                : "border border-cs-border bg-cs-bg-raised text-cs-muted",
            )}
          >
            <Radio size={9} />
            {isConnected
              ? t("teamShare.detail.live", { defaultValue: "Live" })
              : t("teamShare.detail.connecting", { defaultValue: "Connecting…" })}
          </span>
          <span className="inline-flex items-center gap-1 rounded-full border border-cs-accent/40 bg-cs-accent/10 px-2 py-0.5 text-cs-accent font-medium">
            <Eye size={10} />{" "}
            {t("teamShare.detail.readOnly", { defaultValue: "Read-only" })}
          </span>
          <span className="inline-flex items-center gap-1 rounded-full border border-cs-border bg-cs-bg-raised px-2 py-0.5 text-cs-muted">
            <Lock size={10} />{" "}
            {t("teamShare.detail.sharedSnapshot", {
              defaultValue: "Shared snapshot",
            })}
          </span>
        </div>
      </div>

      {/* Snapshot banner */}
      <div
        className={cn(
          "rounded-md border border-cs-accent/30 bg-cs-accent/5 px-3 py-2",
          "text-[11px] text-cs-text",
        )}
      >
        {t("teamShare.detail.banner", {
          defaultValue:
            "You're viewing a snapshot a teammate shared into this workspace. Edits don't sync back to the original.",
        })}
        {q.data.expires_at && (
          <span className="ml-1 text-cs-muted">
            {t("teamShare.detail.expires", { defaultValue: "Expires" })}{" "}
            {formatTime(q.data.expires_at)}.
          </span>
        )}
      </div>

      {/* Snapshot body (plaintext) or E2E materialized state */}
      {isE2e ? (
        <E2eSnapshotSection
          events={e2eHistoryEvents}
          loading={e2eHistoryLoading}
          exhausted={e2eHistoryExhausted}
          teamKeyId={teamKeyId}
          onLoadMore={() => void loadE2eHistory(e2eHistorySince)}
          t={t}
        />
      ) : (
        <>
          {resourceKind === "session" && (
            <SharedSessionBody data={q.data as SharedSessionDetail} />
          )}
          {resourceKind === "war-room" && (
            <SharedWarRoomBody data={q.data as SharedWarRoomDetail} />
          )}
          {resourceKind === "chat" && (
            <SharedChatBody data={q.data as SharedChatDetail} />
          )}
          {resourceKind === "loop" && (
            <SharedLoopBody data={q.data as SharedLoopDetail} />
          )}
          {resourceKind === "mission" && (
            <SharedMissionBody data={q.data as SharedMissionDetail} />
          )}
        </>
      )}

      {/* Live events divider + events */}
      {liveEvents.length > 0 && (
        <div className="space-y-2">
          <div className="flex items-center gap-2">
            <div className="h-px flex-1 bg-cs-border/40" />
            <span className="text-[10px] text-cs-muted">
              {t("teamShare.detail.liveDivider", {
                defaultValue: "Live since you opened",
              })}
            </span>
            <div className="h-px flex-1 bg-cs-border/40" />
          </div>
          <div className="space-y-2">
            {liveEvents.map((event) => (
              <LiveEventRow key={event.seq_num} event={event} />
            ))}
          </div>
        </div>
      )}

      {/* Append composer — disabled for E2E shares */}
      <AppendTurnComposer
        teamId={teamId}
        kind={resourceKind}
        resourceId={resourceId}
        isE2e={isE2e}
      />
    </div>
  );
}

// ── E2E materialized snapshot section ─────────────────────────

interface E2eSnapshotSectionProps {
  events: TeamEvent[];
  loading: boolean;
  exhausted: boolean;
  teamKeyId: string | null;
  onLoadMore: () => void;
  t: (key: string, opts?: { defaultValue: string }) => string;
}

function E2eSnapshotSection({
  events,
  loading,
  exhausted,
  teamKeyId,
  onLoadMore,
  t,
}: E2eSnapshotSectionProps) {
  // If the user doesn't have the Team Key envelope yet, show a waiting state.
  if (!teamKeyId) {
    return (
      <div className="rounded-md border border-cs-border/40 bg-cs-bg-raised/40 px-4 py-3 text-sm text-cs-muted space-y-1">
        <div className="font-medium text-cs-text">
          {t("teamShare.e2e.waitingForKey", { defaultValue: "Waiting for a teammate to share the key" })}
        </div>
        <div className="text-xs">
          {t("teamShare.e2e.waitingExplain", {
            defaultValue:
              "This share is end-to-end encrypted. A team member who is online will " +
              "automatically seal the Team Key to your public key. This usually takes " +
              "a few seconds.",
          })}
        </div>
      </div>
    );
  }

  if (loading && events.length === 0) {
    return (
      <div className="rounded-md border border-cs-border/40 bg-cs-bg-raised/40 px-4 py-3 text-xs text-cs-muted">
        {t("teamShare.e2e.decrypting", { defaultValue: "Decrypting event history…" })}
      </div>
    );
  }

  if (events.length === 0) {
    return (
      <div className="rounded-md border border-cs-border/40 bg-cs-bg-raised/40 px-4 py-3 text-xs text-cs-muted">
        {t("teamShare.e2e.noEvents", { defaultValue: "No events in this encrypted share yet." })}
      </div>
    );
  }

  const hasDecryptError = events.some(
    (e) =>
      typeof e.payload_json === "object" &&
      e.payload_json !== null &&
      (e.payload_json as Record<string, unknown>).__decrypt_error,
  );

  return (
    <div className="space-y-2">
      {hasDecryptError && (
        <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-300">
          {t("teamShare.e2e.decryptErrorBanner", {
            defaultValue:
              "Some events could not be decrypted. They may have been encrypted with " +
              "a different Team Key version or the signature was invalid.",
          })}
        </div>
      )}
      <div className="space-y-2">
        {events.map((event) => (
          <LiveEventRow key={event.seq_num} event={event} />
        ))}
      </div>
      {!exhausted && (
        <button
          type="button"
          onClick={onLoadMore}
          disabled={loading}
          className="w-full rounded-md border border-cs-border px-3 py-1.5 text-xs text-cs-muted hover:bg-cs-border/30 transition-colors disabled:opacity-50"
        >
          {loading
            ? t("teamShare.e2e.loadingMore", { defaultValue: "Loading earlier events…" })
            : t("teamShare.e2e.loadEarlier", { defaultValue: "Load earlier" })}
        </button>
      )}
    </div>
  );
}

// ── Live event renderer ────────────────────────────────────────

function LiveEventRow({ event }: { event: TeamEvent }) {
  if (event.event_kind === "turn_appended") {
    const payload = event.payload_json as
      | { role?: string; text?: string }
      | null
      | undefined;
    return (
      <div className="rounded-md border border-emerald-500/30 bg-emerald-500/5 p-2 text-xs">
        <div className="flex items-center gap-2 mb-1 text-[10px] text-cs-muted">
          <span className="uppercase">{payload?.role ?? "?"}</span>
          {event.initiator_runtime && (
            <span>{event.initiator_runtime}</span>
          )}
          <span>{formatTime(event.created_at)}</span>
          <span className="ml-auto rounded bg-emerald-500/10 px-1 py-0.5 text-[9px] text-emerald-400">
            live #{event.seq_num}
          </span>
        </div>
        <pre className="whitespace-pre-wrap font-sans text-cs-text">
          {payload?.text ?? ""}
        </pre>
      </div>
    );
  }

  if (
    event.event_kind === "mission_task_added" ||
    event.event_kind === "mission_task_completed"
  ) {
    // TODO Wave 4: wire proper MissionTask renderers.
    return (
      <div className="rounded-md border border-cs-border/40 bg-cs-card/40 p-2 text-xs text-cs-muted">
        <span className="uppercase text-[9px]">{event.event_kind}</span>{" "}
        — renderer wired in Wave 4
        <span className="ml-2 text-[9px] opacity-60">#{event.seq_num}</span>
      </div>
    );
  }

  // Generic fallback for unknown event kinds.
  return (
    <div className="rounded-md border border-cs-border/40 bg-cs-card/40 p-2 text-xs text-cs-muted">
      <span className="uppercase text-[9px]">{event.event_kind}</span>
      <span className="ml-2 text-[9px] opacity-60">#{event.seq_num}</span>
    </div>
  );
}

// ── kind-specific snapshot renderers (unchanged from v2.14) ────

interface SnapshotTurn {
  role?: string;
  text?: string;
  runtime?: string;
  agent_slug?: string | null;
  created_at?: string;
  initiator_kind?: string | null;
  client_surface?: string | null;
}

function SharedSessionBody({ data }: { data: SharedSessionDetail }) {
  const snap = data.snapshot ?? {};
  const turns = Array.isArray((snap as { turns?: unknown }).turns)
    ? (snap as { turns: SnapshotTurn[] }).turns
    : [];
  return (
    <div className="space-y-3">
      <div className="rounded-lg border border-cs-border bg-cs-card p-3 text-xs">
        <div className="font-medium text-cs-text">
          {data.title ?? "Untitled session"}
        </div>
        {(data as SharedSessionDetail & { runtime?: string }).runtime && (
          <div className="text-cs-muted">
            runtime:{" "}
            {(data as SharedSessionDetail & { runtime?: string }).runtime}
          </div>
        )}
        {(data as SharedSessionDetail & { agent_slug?: string | null }).agent_slug && (
          <div className="text-cs-muted">
            agent:{" "}
            {(data as SharedSessionDetail & { agent_slug?: string | null }).agent_slug}
          </div>
        )}
        {typeof (data as SharedSessionDetail & { turn_count?: number }).turn_count === "number" && (
          <div className="text-cs-muted">
            turns:{" "}
            {(data as SharedSessionDetail & { turn_count?: number }).turn_count}
          </div>
        )}
      </div>
      {turns.length === 0 ? (
        <div className="rounded-md border border-cs-border/40 bg-cs-bg-raised/40 px-3 py-2 text-xs text-cs-muted">
          No turns in the snapshot.
        </div>
      ) : (
        <div className="space-y-2">
          {turns.map((turn, i) => (
            <div
              key={`${turn.created_at ?? ""}-${i}`}
              className="rounded-md border border-cs-border/60 bg-cs-card/40 p-2 text-xs"
            >
              <div className="flex items-center gap-2 mb-1 text-[10px] text-cs-muted">
                <span className="uppercase">{turn.role ?? "?"}</span>
                {turn.runtime && <span>{turn.runtime}</span>}
                {turn.created_at && <span>{formatTime(turn.created_at)}</span>}
              </div>
              <pre className="whitespace-pre-wrap font-sans text-cs-text">
                {turn.text ?? ""}
              </pre>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

interface SnapshotSeat {
  runtime?: string;
  agent_slug?: string | null;
  prompt?: string;
  response?: string;
  status?: string;
}

function SharedWarRoomBody({ data }: { data: SharedWarRoomDetail }) {
  const snap = data.snapshot ?? {};
  const seats = Array.isArray((snap as { seats?: unknown }).seats)
    ? (snap as { seats: SnapshotSeat[] }).seats
    : [];
  const framing = (snap as { framing?: string }).framing;
  return (
    <div className="space-y-3">
      <div className="rounded-lg border border-cs-border bg-cs-card p-3 text-xs">
        <div className="font-medium text-cs-text">
          {data.title ?? "Untitled war room"}
        </div>
        {framing && (
          <div className="mt-1 text-cs-muted whitespace-pre-wrap">{framing}</div>
        )}
      </div>
      {seats.length === 0 ? (
        <div className="rounded-md border border-cs-border/40 bg-cs-bg-raised/40 px-3 py-2 text-xs text-cs-muted">
          No seats in the snapshot.
        </div>
      ) : (
        <div className="space-y-2">
          {seats.map((seat, i) => (
            <div
              key={i}
              className="rounded-md border border-cs-border/60 bg-cs-card/40 p-2 text-xs"
            >
              <div className="flex items-center gap-2 mb-1 text-[10px] text-cs-muted">
                {seat.runtime && <span>{seat.runtime}</span>}
                {seat.agent_slug && <span>{seat.agent_slug}</span>}
                {seat.status && (
                  <span className="uppercase">{seat.status}</span>
                )}
              </div>
              {seat.prompt && (
                <div className="mb-1 rounded bg-cs-bg-raised/40 p-1 text-cs-muted">
                  <span className="text-[9px] uppercase">prompt</span>
                  <pre className="whitespace-pre-wrap font-sans">
                    {seat.prompt}
                  </pre>
                </div>
              )}
              {seat.response && (
                <div className="rounded bg-cs-bg-raised/40 p-1 text-cs-text">
                  <span className="text-[9px] uppercase text-cs-muted">
                    response
                  </span>
                  <pre className="whitespace-pre-wrap font-sans">
                    {seat.response}
                  </pre>
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

interface SnapshotMessage {
  role?: string;
  text?: string;
  runtime?: string;
  created_at?: string;
}

function SharedChatBody({ data }: { data: SharedChatDetail }) {
  const snap = data.snapshot ?? {};
  const messages = Array.isArray((snap as { messages?: unknown }).messages)
    ? (snap as { messages: SnapshotMessage[] }).messages
    : [];
  return (
    <div className="space-y-3">
      <div className="rounded-lg border border-cs-border bg-cs-card p-3 text-xs">
        <div className="font-medium text-cs-text">
          {data.title ?? "Untitled chat"}
        </div>
        {(data as SharedChatDetail & { runtime?: string }).runtime && (
          <div className="text-cs-muted">
            runtime:{" "}
            {(data as SharedChatDetail & { runtime?: string }).runtime}
          </div>
        )}
      </div>
      {messages.length === 0 ? (
        <div className="rounded-md border border-cs-border/40 bg-cs-bg-raised/40 px-3 py-2 text-xs text-cs-muted">
          No messages in the snapshot.
        </div>
      ) : (
        <div className="space-y-2">
          {messages.map((m, i) => (
            <div
              key={`${m.created_at ?? ""}-${i}`}
              className="rounded-md border border-cs-border/60 bg-cs-card/40 p-2 text-xs"
            >
              <div className="flex items-center gap-2 mb-1 text-[10px] text-cs-muted">
                <span className="uppercase">{m.role ?? "?"}</span>
                {m.runtime && <span>{m.runtime}</span>}
                {m.created_at && <span>{formatTime(m.created_at)}</span>}
              </div>
              <pre className="whitespace-pre-wrap font-sans text-cs-text">
                {m.text ?? ""}
              </pre>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function SharedLoopBody({ data }: { data: SharedLoopDetail }) {
  return (
    <div className="rounded-lg border border-cs-border bg-cs-card p-3 text-xs">
      <div className="font-medium text-cs-text">{data.title ?? "Untitled loop"}</div>
      <div className="mt-1 text-cs-muted">
        {/* TODO Wave 3/4: render loop iterations from snapshot. */}
        Loop snapshot render not yet implemented.
      </div>
    </div>
  );
}

function SharedMissionBody({ data }: { data: SharedMissionDetail }) {
  return (
    <div className="rounded-lg border border-cs-border bg-cs-card p-3 text-xs">
      <div className="font-medium text-cs-text">
        {data.title ?? "Untitled mission"}
      </div>
      <div className="mt-1 text-cs-muted">
        {/* TODO Wave 4: render mission tasks + tick log from snapshot. */}
        Mission snapshot render not yet implemented.
      </div>
    </div>
  );
}
