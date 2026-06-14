// v2.16 Wave 1 — read-only Team Workspaces: shared resource detail + live tail.
// v2.17 Wave 3 — e2e branch: tether client replaces static "Open in desktop" gate.

import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ArrowLeft, AlertCircle, Lock, Wifi, WifiOff, Loader2, ShieldAlert } from 'lucide-react';
import {
  getSharedDetail,
  backfillTeamEvents,
  RESOURCE_KIND_META,
  type SharedResourceKind,
  type SharedDetail,
  type TeamEvent,
} from '../lib/api';
import { subscribeTeamEvents } from '../lib/teamEventStream';
import {
  startTether,
  stopTether,
  subscribeTetherState,
  listTetherSessions,
  type TetherState,
  type TetherInfo,
} from '../lib/tether/client';
import {
  subscribeDecryptedEvents,
  type TeamEventDecrypted,
} from '../lib/tether/decryptedEventStream';

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

/** Extract a short text blurb from a plaintext event payload. */
function eventText(ev: TeamEvent): string {
  const p = ev.payload_json as Record<string, unknown> | null;
  if (!p) return '';
  if (typeof p.text === 'string') return p.text.slice(0, 200);
  return JSON.stringify(p).slice(0, 200);
}

/** Extract a short text blurb from a decrypted (tether) event payload. */
function decryptedEventText(ev: TeamEventDecrypted): string {
  const p = ev.payload_json as Record<string, unknown> | null;
  if (!p) return '';
  if (typeof p.text === 'string') return p.text.slice(0, 200);
  return JSON.stringify(p).slice(0, 200);
}

// ──────────────────────────────────────────────────────────────────
// Snapshot mini-render (plaintext only)
// ──────────────────────────────────────────────────────────────────

function SnapshotBlock({ detail }: { detail: SharedDetail }) {
  const snap = detail.snapshot as Record<string, unknown> | null;
  if (!snap) return null;

  const turnsArr =
    (snap.turns as unknown[]) ??
    (snap.seats as unknown[]) ??
    (snap.messages as unknown[]) ??
    null;

  return (
    <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-4 space-y-3">
      <h3 className="text-xs font-semibold uppercase tracking-wider text-[#8888a0]">Snapshot</h3>

      <div className="grid grid-cols-2 gap-3 text-xs">
        {detail.title && (
          <div>
            <span className="text-[#8888a0]">Title</span>
            <p className="text-white mt-0.5 font-medium truncate">{detail.title}</p>
          </div>
        )}
        {typeof detail.turn_count === 'number' && (
          <div>
            <span className="text-[#8888a0]">Turns</span>
            <p className="text-white mt-0.5 font-medium">{detail.turn_count}</p>
          </div>
        )}
        {detail.runtime && (
          <div>
            <span className="text-[#8888a0]">Runtime</span>
            <p className="text-white mt-0.5 font-medium">{detail.runtime}</p>
          </div>
        )}
        {detail.agent_slug && (
          <div>
            <span className="text-[#8888a0]">Agent</span>
            <p className="text-white mt-0.5 font-medium">{detail.agent_slug}</p>
          </div>
        )}
      </div>

      {turnsArr && turnsArr.length > 0 ? (
        <div className="space-y-1.5">
          <p className="text-[10px] uppercase tracking-wider text-[#8888a0]">
            Latest turns (snapshot)
          </p>
          {(turnsArr as Array<Record<string, unknown>>).slice(0, 3).map((t, i) => {
            const role = String(t.role ?? t.seat ?? t.from ?? '?');
            const content =
              typeof t.content === 'string'
                ? t.content.slice(0, 160)
                : typeof t.text === 'string'
                  ? t.text.slice(0, 160)
                  : JSON.stringify(t).slice(0, 100);
            return (
              <div key={i} className="bg-[#0a0a0f] rounded-md px-3 py-2 text-xs">
                <span className="text-[#00FFB2] font-medium">{role}</span>
                <span className="text-[#2a2a3a] mx-1">·</span>
                <span className="text-[#aaaab8]">{content}</span>
              </div>
            );
          })}
          {turnsArr.length > 3 && (
            <p className="text-[10px] text-[#8888a0]">+{turnsArr.length - 3} more in snapshot</p>
          )}
        </div>
      ) : (
        <p className="text-xs text-[#8888a0]">(no turns yet)</p>
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
    let cancelled = false;
    const since = Math.max(0, lastSeq - BACKFILL_WINDOW);

    void backfillTeamEvents(teamId, kind, resourceId, since).then((historical) => {
      if (cancelled) return;
      setEvents(historical);

      const unsub = subscribeTeamEvents(
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

      setConnStatus('connected');

      return () => {
        cancelled = true;
        unsub();
      };
    }).catch(() => {
      if (!cancelled) setConnStatus('reconnecting');
    });

    return () => {
      cancelled = true;
    };
  }, [teamId, kind, resourceId, lastSeq]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [events.length]);

  return (
    <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-4 space-y-3">
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
            <div
              key={ev.seq_num}
              className="bg-[#0a0a0f] rounded-md px-3 py-2 text-xs space-y-0.5"
            >
              <div className="flex items-center gap-2 text-[10px] text-[#8888a0]">
                <span className="font-mono">#{ev.seq_num}</span>
                <span className="text-[#2a2a3a]">·</span>
                <span className="font-medium text-[#aaaab8]">{ev.event_kind}</span>
                {ev.surface && (
                  <>
                    <span className="text-[#2a2a3a]">·</span>
                    <span>{ev.surface}</span>
                  </>
                )}
                {ev.initiator_runtime && (
                  <>
                    <span className="text-[#2a2a3a]">·</span>
                    <span>{ev.initiator_runtime}</span>
                  </>
                )}
                <span className="text-[#2a2a3a] ml-auto">{formatIso(ev.created_at)}</span>
              </div>
              {eventText(ev) && (
                <p className="text-[#e8e8f0] leading-relaxed">{eventText(ev)}</p>
              )}
            </div>
          ))}
          <div ref={bottomRef} />
        </div>
      )}
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────
// Tethered events section (e2e — events routed through desktop)
// ──────────────────────────────────────────────────────────────────

interface TetheredEventsProps {
  teamId: string;
  kind: SharedResourceKind;
  resourceId: string;
  lastSeq: number;
}

function TetheredEventsSection({ teamId, kind, resourceId, lastSeq }: TetheredEventsProps) {
  const [events, setEvents] = useState<TeamEventDecrypted[]>([]);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unsub = subscribeDecryptedEvents(
      { teamId, resourceKind: kind, resourceId },
      lastSeq,
      (ev) => {
        setEvents((prev) => {
          if (prev.some((e) => e.seq_num === ev.seq_num)) return prev;
          return [...prev, ev];
        });
      },
    );
    return unsub;
  }, [teamId, kind, resourceId, lastSeq]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [events.length]);

  return (
    <div className="bg-[#16161e] border border-[#2a2a3a] rounded-lg p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-[#8888a0]">
          Live events <span className="text-[#00FFB2]">(tethered)</span>
        </h3>
        <span className="flex items-center gap-1.5 text-xs font-medium text-[#00FFB2]">
          <Lock className="w-3 h-3" />
          E2E decrypted via desktop
        </span>
      </div>

      {events.length === 0 ? (
        <p className="text-xs text-[#8888a0]">Waiting for events from desktop…</p>
      ) : (
        <div className="max-h-80 overflow-y-auto space-y-1.5 pr-1">
          {events.map((ev) => (
            <div
              key={ev.seq_num}
              className="bg-[#0a0a0f] rounded-md px-3 py-2 text-xs space-y-0.5"
            >
              <div className="flex items-center gap-2 text-[10px] text-[#8888a0]">
                <span className="font-mono">#{ev.seq_num}</span>
                <span className="text-[#2a2a3a]">·</span>
                <span className="font-medium text-[#aaaab8]">{ev.event_kind}</span>
                {ev.surface && (
                  <>
                    <span className="text-[#2a2a3a]">·</span>
                    <span>{ev.surface}</span>
                  </>
                )}
                {ev.initiator_runtime && (
                  <>
                    <span className="text-[#2a2a3a]">·</span>
                    <span>{ev.initiator_runtime}</span>
                  </>
                )}
                <span className="text-[#2a2a3a] ml-auto">{formatIso(ev.created_at)}</span>
              </div>

              {/* Sig-invalid events: show redaction banner instead of body. */}
              {!ev.sig_valid ? (
                <div className="flex items-center gap-2 mt-1 px-2 py-1.5 bg-red-500/10 border border-red-500/30 rounded text-red-400">
                  <ShieldAlert className="w-3.5 h-3.5 shrink-0" />
                  <span className="text-[11px] font-medium">Tampered — content hidden</span>
                </div>
              ) : (
                decryptedEventText(ev) && (
                  <p className="text-[#e8e8f0] leading-relaxed">{decryptedEventText(ev)}</p>
                )
              )}
            </div>
          ))}
          <div ref={bottomRef} />
        </div>
      )}
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────
// E2E + tether state cards
// ──────────────────────────────────────────────────────────────────

/** Fallback card shown when no desktop host is available. */
function HostOfflineCard() {
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

/** Card shown while tether is connecting or waiting on desktop approval. */
function TetherStatusCard({ state, machineName }: { state: TetherState; machineName: string | null }) {
  if (state === 'idle' || state === 'connecting') {
    return (
      <div className="flex flex-col items-center justify-center py-14 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-center px-8 space-y-3">
        <Loader2 className="w-10 h-10 text-[#8888a0] animate-spin" />
        <p className="text-white font-semibold">Connecting to desktop…</p>
      </div>
    );
  }

  if (state === 'pending_approval') {
    return (
      <div className="flex flex-col items-center justify-center py-14 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-center px-8 space-y-3">
        <Loader2 className="w-10 h-10 text-[#00FFB2] animate-spin" />
        <p className="text-white font-semibold">Waiting for approval on your desktop…</p>
        {machineName && (
          <p className="text-[#8888a0] text-sm max-w-xs leading-relaxed">
            Open ATO on <span className="text-white font-mono">{machineName}</span> and click{' '}
            <span className="text-[#00FFB2] font-medium">Allow</span>.
          </p>
        )}
        {/* TODO (v2.17.x): wire the desktop "keychain-prompt-in-progress" state
            here to show "Desktop is unlocking your keychain..." with a 30s
            timeout before falling through to HostOfflineCard. The tether
            ready frame will eventually carry a keychain_status field. */}
      </div>
    );
  }

  if (state === 'denied') {
    return (
      <div className="flex flex-col items-center justify-center py-14 bg-[#16161e] border border-red-500/20 rounded-lg text-center px-8 space-y-3">
        <AlertCircle className="w-10 h-10 text-red-400" />
        <p className="text-white font-semibold">Desktop denied this browser session</p>
        <p className="text-[#8888a0] text-sm max-w-xs leading-relaxed">
          Open ATO on your desktop to view this encrypted share.
        </p>
      </div>
    );
  }

  if (state === 'error') {
    return (
      <div className="flex flex-col items-center justify-center py-14 bg-[#16161e] border border-red-500/20 rounded-lg text-center px-8 space-y-3">
        <AlertCircle className="w-10 h-10 text-red-400" />
        <p className="text-white font-semibold">Tether error</p>
        <p className="text-[#8888a0] text-sm">Could not establish a secure connection. Try reloading.</p>
      </div>
    );
  }

  // host_offline — fall through to default
  return <HostOfflineCard />;
}

// ──────────────────────────────────────────────────────────────────
// Machine picker (multiple approved hosts)
// ──────────────────────────────────────────────────────────────────

interface MachinePickerProps {
  machines: string[];
  onPick: (name: string) => void;
}

function MachinePicker({ machines, onPick }: MachinePickerProps) {
  return (
    <div className="flex flex-col items-center justify-center py-10 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-center px-8 space-y-4">
      <Lock className="w-8 h-8 text-[#8888a0]" />
      <p className="text-white font-semibold">Choose a desktop to decrypt</p>
      <p className="text-[#8888a0] text-sm">Multiple ATO desktops are available. Pick one:</p>
      <div className="flex flex-col gap-2 w-full max-w-xs">
        {machines.map((name) => (
          <button
            key={name}
            onClick={() => onPick(name)}
            className="w-full px-4 py-2.5 bg-[#0a0a0f] border border-[#2a2a3a] hover:border-[#00FFB2]/50 rounded-lg text-sm text-white font-mono transition-colors"
          >
            {name}
          </button>
        ))}
      </div>
    </div>
  );
}

// ──────────────────────────────────────────────────────────────────
// E2E branch — tether orchestrator
// ──────────────────────────────────────────────────────────────────

interface E2EBranchProps {
  teamId: string;
  kind: SharedResourceKind;
  resourceId: string;
  lastSeq: number;
}

function E2EBranch({ teamId, kind, resourceId, lastSeq }: E2EBranchProps) {
  const [tetherInfo, setTetherInfo] = useState<TetherInfo>({
    state: 'idle',
    machineName: null,
    sessionId: null,
  });

  // Available host machines queried from the cloud tether/sessions endpoint.
  const [availableMachines, setAvailableMachines] = useState<string[] | null>(null);
  const [pickedMachine, setPickedMachine] = useState<string | null>(null);

  // Subscribe to global tether state changes.
  useEffect(() => {
    const unsub = subscribeTetherState((info) => setTetherInfo(info));
    return unsub;
  }, []);

  // On mount: discover available hosts, then auto-pair or show picker.
  useEffect(() => {
    let cancelled = false;

    void listTetherSessions().then((sessions) => {
      if (cancelled) return;

      const approved = sessions
        .filter((s) => s.approval_state === 'approved' || s.approval_state === 'pending')
        .map((s) => s.desktop_machine_name);

      // Deduplicate.
      const unique = [...new Set(approved)];
      setAvailableMachines(unique);

      if (unique.length === 1) {
        // Single host — auto-pair immediately.
        setPickedMachine(unique[0]);
        void startTether(unique[0]);
      } else if (unique.length === 0) {
        // No hosts online — stay in host_offline visual.
        setTetherInfo({ state: 'host_offline', machineName: null, sessionId: null });
      }
      // If >1, render MachinePicker and wait for user selection.
    });

    return () => {
      cancelled = true;
      stopTether();
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handlePickMachine = (name: string) => {
    setPickedMachine(name);
    void startTether(name);
  };

  // Multiple machines and none picked yet — show picker.
  if (availableMachines !== null && availableMachines.length > 1 && pickedMachine === null) {
    return <MachinePicker machines={availableMachines} onPick={handlePickMachine} />;
  }

  // Approved: render tethered event stream.
  if (tetherInfo.state === 'approved') {
    return (
      <TetheredEventsSection
        teamId={teamId}
        kind={kind}
        resourceId={resourceId}
        lastSeq={lastSeq}
      />
    );
  }

  // All other states: status card.
  return <TetherStatusCard state={tetherInfo.state} machineName={tetherInfo.machineName} />;
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

  const kindLabel = RESOURCE_KIND_META[kind].label.replace(/s$/, '');

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

      {/* E2E branch — tether client drives decryption via desktop */}
      {detail?.encryption_mode === 'e2e' && (
        <E2EBranch
          teamId={teamId}
          kind={kind}
          resourceId={resourceId}
          lastSeq={detail.last_seq}
        />
      )}

      {/* Plaintext detail */}
      {detail?.encryption_mode === 'plaintext' && (
        <>
          <SnapshotBlock detail={detail} />
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
