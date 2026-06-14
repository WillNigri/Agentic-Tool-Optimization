// v2.14 — Read-only detail view for team-shared resources.
//
// When a teammate clicks a row in the Team filter (#5 in the
// shared-workspaces cluster), they may not have the local SQLite row
// — the resource was authored by someone else. This view fetches the
// snapshot from the cloud share row and renders a read-only version
// of the appropriate detail surface (session transcript, war-room
// seats, chat thread).
//
// Renders nothing of its own beyond a header banner — the body is
// dispatched to a kind-specific renderer that consumes the snapshot.

import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Eye, Lock } from "lucide-react";

import { cn } from "@/lib/utils";
import {
  getSharedChatDetail,
  getSharedSessionDetail,
  getSharedWarRoomDetail,
  type SharedChatDetail,
  type SharedSessionDetail,
  type SharedWarRoomDetail,
} from "@/lib/cloud-api";
import { formatTime } from "@/components/SessionsList/_helpers";

export type SharedResourceKind = "session" | "war_room" | "chat";

interface SharedDetailViewProps {
  resourceKind: SharedResourceKind;
  teamId: string;
  resourceId: string;
  onBack: () => void;
}

type SharedDetail = SharedSessionDetail | SharedWarRoomDetail | SharedChatDetail;

async function fetcher(
  resourceKind: SharedResourceKind,
  teamId: string,
  resourceId: string,
): Promise<SharedDetail> {
  if (resourceKind === "session") return getSharedSessionDetail(teamId, resourceId);
  if (resourceKind === "war_room") return getSharedWarRoomDetail(teamId, resourceId);
  return getSharedChatDetail(teamId, resourceId);
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
    // 60s cache — the share row's snapshot is point-in-time so it
    // doesn't move except when the sharer hits "Refresh" (#7).
    staleTime: 60_000,
  });

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
          <ArrowLeft size={14} /> {t("teamShare.detail.back", { defaultValue: "Back to sessions" })}
        </button>
        <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 p-4 text-sm text-cs-text">
          {q.error instanceof Error
            ? q.error.message
            : t("teamShare.detail.failed", { defaultValue: "Could not load the shared snapshot." })}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <button
          onClick={onBack}
          className="inline-flex items-center gap-1 text-sm text-cs-muted hover:text-cs-text"
        >
          <ArrowLeft size={14} /> {t("teamShare.detail.back", { defaultValue: "Back to sessions" })}
        </button>
        <div className="flex items-center gap-2 text-[10px]">
          <span className="inline-flex items-center gap-1 rounded-full border border-cs-accent/40 bg-cs-accent/10 px-2 py-0.5 text-cs-accent font-medium">
            <Eye size={10} /> {t("teamShare.detail.readOnly", { defaultValue: "Read-only" })}
          </span>
          <span className="inline-flex items-center gap-1 rounded-full border border-cs-border bg-cs-bg-raised px-2 py-0.5 text-cs-muted">
            <Lock size={10} /> {t("teamShare.detail.sharedSnapshot", { defaultValue: "Shared snapshot" })}
          </span>
        </div>
      </div>

      <div
        className={cn(
          "rounded-md border border-cs-accent/30 bg-cs-accent/5 px-3 py-2",
          "text-[11px] text-cs-text",
        )}
      >
        {t("teamShare.detail.banner", {
          defaultValue: "You're viewing a snapshot a teammate shared into this workspace. Edits don't sync back to the original.",
        })}
        {q.data.expires_at && (
          <span className="ml-1 text-cs-muted">
            {t("teamShare.detail.expires", { defaultValue: "Expires" })}{" "}
            {formatTime(q.data.expires_at)}.
          </span>
        )}
      </div>

      {resourceKind === "session" && <SharedSessionBody data={q.data as SharedSessionDetail} />}
      {resourceKind === "war_room" && <SharedWarRoomBody data={q.data as SharedWarRoomDetail} />}
      {resourceKind === "chat" && <SharedChatBody data={q.data as SharedChatDetail} />}
    </div>
  );
}

// ── kind-specific renderers ────────────────────────────────────────

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
    ? ((snap as { turns: SnapshotTurn[] }).turns)
    : [];
  return (
    <div className="space-y-3">
      <div className="rounded-lg border border-cs-border bg-cs-card p-3 text-xs">
        <div className="font-medium text-cs-text">
          {data.title ?? "Untitled session"}
        </div>
        {data.runtime && (
          <div className="text-cs-muted">runtime: {data.runtime}</div>
        )}
        {data.agent_slug && (
          <div className="text-cs-muted">agent: {data.agent_slug}</div>
        )}
        {typeof data.turn_count === "number" && (
          <div className="text-cs-muted">turns: {data.turn_count}</div>
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
    ? ((snap as { seats: SnapshotSeat[] }).seats)
    : [];
  const framing = (snap as { framing?: string }).framing;
  return (
    <div className="space-y-3">
      <div className="rounded-lg border border-cs-border bg-cs-card p-3 text-xs">
        <div className="font-medium text-cs-text">{data.title ?? "Untitled war room"}</div>
        {framing && <div className="mt-1 text-cs-muted whitespace-pre-wrap">{framing}</div>}
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
                {seat.status && <span className="uppercase">{seat.status}</span>}
              </div>
              {seat.prompt && (
                <div className="mb-1 rounded bg-cs-bg-raised/40 p-1 text-cs-muted">
                  <span className="text-[9px] uppercase">prompt</span>
                  <pre className="whitespace-pre-wrap font-sans">{seat.prompt}</pre>
                </div>
              )}
              {seat.response && (
                <div className="rounded bg-cs-bg-raised/40 p-1 text-cs-text">
                  <span className="text-[9px] uppercase text-cs-muted">response</span>
                  <pre className="whitespace-pre-wrap font-sans">{seat.response}</pre>
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
    ? ((snap as { messages: SnapshotMessage[] }).messages)
    : [];
  return (
    <div className="space-y-3">
      <div className="rounded-lg border border-cs-border bg-cs-card p-3 text-xs">
        <div className="font-medium text-cs-text">{data.title ?? "Untitled chat"}</div>
        {data.runtime && <div className="text-cs-muted">runtime: {data.runtime}</div>}
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
              <pre className="whitespace-pre-wrap font-sans text-cs-text">{m.text ?? ""}</pre>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
