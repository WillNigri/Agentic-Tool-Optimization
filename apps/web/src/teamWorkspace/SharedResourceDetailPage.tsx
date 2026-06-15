// v2.16 Wave 2 — per-kind snapshot renderers wired into the detail page.
// Wave 1 had a minimal SnapshotBlock (JSON.stringify preview).
// Wave 2 replaces it with full-fidelity renderers for session/war-room/chat.

import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ArrowLeft, AlertCircle, Lock, Monitor, Wifi, WifiOff } from 'lucide-react';
import {
  getSharedDetail,
  backfillTeamEvents,
  RESOURCE_KIND_META,
  type SharedResourceKind,
  type SharedDetail,
  type TeamEvent,
} from '../lib/api';
import { subscribeTeamEvents } from '../lib/teamEventStream';
import SessionTurnsRenderer, { type SnapshotTurn } from './renderers/SessionTurnsRenderer';
import WarRoomSeatsRenderer, { type SnapshotSeat } from './renderers/WarRoomSeatsRenderer';
import ChatMessagesRenderer, { type SnapshotMessage } from './renderers/ChatMessagesRenderer';

// ──────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────

function kindBadgeClasses(kind: SharedResourceKind): string {
  const map: Record<SharedResourceKind, string> = {
    session: 'bg-blue-500/15 text-blue-400 border border-blue-500/30',
    'war-room': 'bg-purple-500/15 text-purple-400 border border-purple-500/30',
    chat: 'bg-[#00FFB2]/15 text-[#00FFB2] border border-[#00FFB2]/30',
    loop: 'bg-orange-500/15 text-orange-400 border border-orange-500/30',
    mission: 'bg-yellow-500/15 text-yellow-400 border border-yellow-500/30',
  };
  return map[kind];
}

function formatIso(isoStr: string): string {
  try {
    return new Date(isoStr).toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  } catch {
    return isoStr;
  }
}

// ──────────────────────────────────────────────────────────────────
// Per-kind snapshot renderers (Wave 2)
// ──────────────────────────────────────────────────────────────────

/** "Open in Desktop" placeholder for loop / mission kinds. */
function OpenInDesktopPlaceholder({ kind }: { kind: SharedResourceKind }) {
  const label = RESOURCE_KIND_META[kind].label.replace(/s$/, '');
  return (
    <div className="flex flex-col items-center justify-center py-12 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-center px-8 space-y-3">
      <Monitor className="w-9 h-9 text-[#8888a0]" />
      <p className="text-white font-semibold">Read-only view shipping later</p>
      <p className="text-[#8888a0] text-sm max-w-xs leading-relaxed">
        Full-fidelity web rendering for {label}s is planned for a future
        cluster. To view this, open ATO on your desktop.
      </p>
    </div>
  );
}

/**
 * Snapshot section: routes to per-kind renderer or placeholder.
 * Only rendered when encryption_mode === 'plaintext'.
 */
function SnapshotSection({
  detail,
  kind,
}: {
  detail: SharedDetail;
  kind: SharedResourceKind;
}) {
  const snap = detail.snapshot as Record<string, unknown> | null;

  return (
    <div className="space-y-3">
      {/* Top-level meta (title, turn count, runtime) */}
      <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-4">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-[#8888a0] mb-3">
          Snapshot
        </h3>
        <div className="grid grid-cols-2 gap-3 text-xs">
          {detail.title && (
            <div>
              <span className="text-[#8888a0]">Title</span>
              <p className="text-white mt-0.5 font-medium truncate">{detail.title}</p>
            </div>
          )}
          {/* Codex R1 follow-up — surface kind-specific snapshot
              metadata the api.ts normalizer doesn't lift to top-level.
              Sessions have turn_count (normalized); war-rooms put
              seat_count, chats put message_count + coordinator_runtime
              inside `snapshot`. Pre-fix shape rendered nothing for
              those kinds. */}
          {kind === 'session' && typeof detail.turn_count === 'number' && (
            <div>
              <span className="text-[#8888a0]">Turns</span>
              <p className="text-white mt-0.5 font-medium">{detail.turn_count}</p>
            </div>
          )}
          {kind === 'war-room' && typeof (snap?.seat_count ?? snap?.seatCount) === 'number' && (
            <div>
              <span className="text-[#8888a0]">Seats</span>
              <p className="text-white mt-0.5 font-medium">{snap?.seat_count ?? snap?.seatCount}</p>
            </div>
          )}
          {kind === 'chat' && typeof (snap?.message_count ?? snap?.messageCount) === 'number' && (
            <div>
              <span className="text-[#8888a0]">Messages</span>
              <p className="text-white mt-0.5 font-medium">{snap?.message_count ?? snap?.messageCount}</p>
            </div>
          )}
          {detail.runtime && (
            <div>
              <span className="text-[#8888a0]">Runtime</span>
              <p className="text-white mt-0.5 font-medium">{detail.runtime}</p>
            </div>
          )}
          {kind === 'war-room' && (snap?.coordinator_runtime || snap?.coordinatorRuntime) && (
            <div>
              <span className="text-[#8888a0]">Coordinator</span>
              <p className="text-white mt-0.5 font-medium">
                {snap?.coordinator_runtime ?? snap?.coordinatorRuntime}
              </p>
            </div>
          )}
          {detail.agent_slug && (
            <div>
              <span className="text-[#8888a0]">Agent</span>
              <p className="text-white mt-0.5 font-medium">{detail.agent_slug}</p>
            </div>
          )}
        </div>
      </div>

      {/* Per-kind body */}
      {kind === 'session' ? (
        <SessionTurnsRenderer
          turns={((snap?.turns ?? []) as SnapshotTurn[])}
        />
      ) : kind === 'war-room' ? (
        <WarRoomSeatsRenderer
          seats={((snap?.seats ?? []) as SnapshotSeat[])}
        />
      ) : kind === 'chat' ? (
        <ChatMessagesRenderer
          messages={((snap?.messages ?? []) as SnapshotMessage[])}
        />
      ) : (
        <OpenInDesktopPlaceholder kind={kind} />
      )}
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────
// Live event card — knows how to render turn_appended events richly;
// falls back to a generic card for everything else.
// ──────────────────────────────────────────────────────────────────

function LiveEventCard({ ev }: { ev: TeamEvent }) {
  const p = ev.payload_json as Record<string, unknown> | null;

  // Render turn_appended events with role + runtime when available.
  const isTurnAppended = ev.event_kind === 'turn_appended';
  const role = isTurnAppended && p ? String(p.role ?? '') : null;
  const runtime = isTurnAppended && p ? String(p.runtime ?? '') : null;
  const text =
    isTurnAppended && p
      ? typeof p.text === 'string'
        ? p.text.slice(0, 300)
        : null
      : p
        ? typeof p.text === 'string'
          ? p.text.slice(0, 200)
          : JSON.stringify(p).slice(0, 200)
        : null;

  return (
    <div className="bg-[#0a0a0f] rounded-md px-3 py-2 text-xs space-y-0.5">
      {/* Meta row */}
      <div className="flex items-center gap-2 text-[10px] text-[#8888a0]">
        <span className="font-mono">#{ev.seq_num}</span>
        <span className="text-[#2a2a3a]">·</span>
        <span className="font-medium text-[#aaaab8]">{ev.event_kind}</span>
        {role && (
          <>
            <span className="text-[#2a2a3a]">·</span>
            <span className="text-[#00FFB2]">{role}</span>
          </>
        )}
        {runtime && runtime !== 'undefined' && (
          <>
            <span className="text-[#2a2a3a]">·</span>
            <span>{runtime}</span>
          </>
        )}
        {!role && ev.surface && (
          <>
            <span className="text-[#2a2a3a]">·</span>
            <span>{ev.surface}</span>
          </>
        )}
        {!role && ev.initiator_runtime && (
          <>
            <span className="text-[#2a2a3a]">·</span>
            <span>{ev.initiator_runtime}</span>
          </>
        )}
        <span className="text-[#2a2a3a] ml-auto">{formatIso(ev.created_at)}</span>
      </div>
      {/* Payload text */}
      {text && (
        <p className="text-[#e8e8f0] leading-relaxed">{text}</p>
      )}
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────
// Live events tail (plaintext only)
// ──────────────────────────────────────────────────────────────────

const BACKFILL_WINDOW = 50;

type ConnStatus = 'connecting' | 'connected' | 'reconnecting';

interface LiveEventsProps {
  teamId: string;
  kind: SharedResourceKind;
  resourceId: string;
  lastSeq: number;
}

function LiveEventsSection({ teamId, kind, resourceId, lastSeq }: LiveEventsProps) {
  const [events, setEvents] = useState<TeamEvent[]>([]);
  const [connStatus, setConnStatus] = useState<ConnStatus>('connecting');
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // Codex R1 #1 fix — hoist the unsubscribe to effect scope so
    // React's actual cleanup runs it. Pre-fix shape returned the
    // cleanup from inside `.then()`, which React never sees because
    // it's a Promise resolution, not the effect's return value. WS
    // listeners and the WebSocket itself leaked on unmount / route
    // change.
    let cancelled = false;
    let unsub: (() => void) | null = null;
    const since = Math.max(0, lastSeq - BACKFILL_WINDOW);

    void backfillTeamEvents(teamId, kind, resourceId, since)
      .then((historical) => {
        if (cancelled) return;
        setEvents(historical);

        unsub = subscribeTeamEvents(
          { teamId, resourceKind: kind, resourceId },
          lastSeq,
          (ev) => {
            if (cancelled) return;
            setConnStatus('connected');
            setEvents((prev) => {
              if (prev.some((e) => e.seq_num === ev.seq_num)) return prev;
              return [...prev, ev];
            });
          },
        );
        // If the effect was cancelled between the .then start and the
        // subscribeTeamEvents call landing, immediately tear down so
        // we don't leak a zombie WS.
        if (cancelled) {
          unsub();
          unsub = null;
          return;
        }
        setConnStatus('connected');
      })
      .catch(() => {
        if (!cancelled) setConnStatus('reconnecting');
      });

    return () => {
      cancelled = true;
      if (unsub) {
        unsub();
        unsub = null;
      }
    };
  }, [teamId, kind, resourceId, lastSeq]);

  // Auto-scroll to bottom as new events arrive.
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [events.length]);

  return (
    <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-4 space-y-3">
      {/* Section header + connection indicator */}
      <div className="flex items-center justify-between">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-[#8888a0]">Live events</h3>
        <span className={`flex items-center gap-1.5 text-xs font-medium ${
          connStatus === 'connected'
            ? 'text-[#00FFB2]'
            : connStatus === 'reconnecting'
              ? 'text-yellow-400'
              : 'text-[#8888a0]'
        }`}>
          {connStatus === 'connected' ? (
            <Wifi className="w-3 h-3" />
          ) : (
            <WifiOff className="w-3 h-3" />
          )}
          {connStatus === 'connected'
            ? 'Connected'
            : connStatus === 'reconnecting'
              ? 'Reconnecting…'
              : 'Connecting…'}
        </span>
      </div>

      {events.length === 0 ? (
        <p className="text-xs text-[#8888a0]">Waiting for events…</p>
      ) : (
        <div className="max-h-80 overflow-y-auto space-y-1.5 pr-1">
          {events.map((ev) => (
            <LiveEventCard key={ev.seq_num} ev={ev} />
          ))}
          <div ref={bottomRef} />
        </div>
      )}
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────
// E2E gate card
// ──────────────────────────────────────────────────────────────────

function E2EGate() {
  return (
    <div className="flex flex-col items-center justify-center py-14 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-center px-8 space-y-3">
      <Lock className="w-10 h-10 text-[#8888a0]" />
      <p className="text-white font-semibold">End-to-End Encrypted</p>
      <p className="text-[#8888a0] text-sm max-w-xs leading-relaxed">
        Your team chose to encrypt this share. The decryption key lives only on the desktop app.
        To view this, open ATO on your machine.
      </p>
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────
// Main detail page
// ──────────────────────────────────────────────────────────────────

interface Props {
  teamId: string;
  kind: SharedResourceKind;
  resourceId: string;
  onBack(): void;
}

export default function SharedResourceDetailPage({ teamId, kind, resourceId, onBack }: Props) {
  const { data: detail, isLoading, error } = useQuery<SharedDetail>({
    queryKey: ['detail', teamId, kind, resourceId],
    queryFn: () => getSharedDetail(teamId, kind, resourceId),
  });

  const kindLabel = RESOURCE_KIND_META[kind].label.replace(/s$/, ''); // singular

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-start gap-3">
        <button
          onClick={onBack}
          className="p-1.5 rounded-md hover:bg-[#2a2a3a]/60 text-[#8888a0] hover:text-white transition-colors mt-0.5"
          aria-label="Back to workspace"
        >
          <ArrowLeft className="w-4 h-4" />
        </button>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <h2 className="text-xl font-semibold text-white truncate">
              {detail?.title ?? resourceId.slice(0, 16) + '…'}
            </h2>
            <span className={`text-[11px] font-medium px-2 py-0.5 rounded-full shrink-0 ${kindBadgeClasses(kind)}`}>
              {kindLabel}
            </span>
          </div>
          <p className="text-xs text-[#8888a0] font-mono mt-0.5">{resourceId}</p>
        </div>
      </div>

      {/* Loading */}
      {isLoading && (
        <div className="space-y-3 animate-pulse">
          <div className="h-32 bg-[#16161e] rounded-lg border border-[#2a2a3a]" />
          <div className="h-48 bg-[#16161e] rounded-lg border border-[#2a2a3a]" />
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="flex items-start gap-3 bg-red-500/10 border border-red-500/30 rounded-lg p-4">
          <AlertCircle className="w-5 h-5 text-red-400 shrink-0 mt-0.5" />
          <div>
            <p className="text-sm text-red-400 font-medium">Failed to load resource</p>
            <p className="text-xs text-[#8888a0] mt-1">
              {error instanceof Error ? error.message : 'Unknown error'}
            </p>
          </div>
        </div>
      )}

      {/* E2E gate — no further calls */}
      {detail?.encryption_mode === 'e2e' && <E2EGate />}

      {/* Plaintext detail */}
      {detail?.encryption_mode === 'plaintext' && (
        <>
          <SnapshotSection detail={detail} kind={kind} />
          {/* Divider between snapshot and live tail */}
          <div className="border-t border-[#2a2a3a]" />
          <LiveEventsSection
            teamId={teamId}
            kind={kind}
            resourceId={resourceId}
            lastSeq={detail.last_seq}
          />
        </>
      )}
    </div>
  );
}
