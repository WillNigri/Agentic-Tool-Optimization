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
  type SharedChatDetail,
  type SharedSessionDetail,
  type SharedWarRoomDetail,
  type SharedLoopDetail,
  type SharedMissionDetail,
  type TeamEvent,
} from "@/lib/cloud-api";
import { formatTime } from "@/components/SessionsList/_helpers";
import { useTeamEventStream } from "./useTeamEventStream";
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

  // Extract last_seq from the snapshot response. The cloud adds this
  // field in Wave 2; pre-Wave-2 responses omit it → default to 0.
  const lastSeq = (q.data as (SharedDetail & { last_seq?: number }) | undefined)
    ?.last_seq ?? 0;

  // Determine whether the share is E2E (Wave 3).
  // For now no share is E2E — the column exists but is always 'plaintext'.
  const isE2e =
    (q.data as (SharedDetail & { encryption_mode?: string }) | undefined)
      ?.encryption_mode === "e2e";

  const { events: liveEvents, isConnected } = useTeamEventStream(
    q.data ? teamId : null,
    q.data ? resourceKind : null,
    q.data ? resourceId : null,
    lastSeq,
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

      {/* Snapshot body */}
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
