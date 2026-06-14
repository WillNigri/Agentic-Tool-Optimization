// usePresence-style hook that wraps the TeamEventStream singleton.
//
// Usage:
//   const { events, isConnected } = useTeamEventStream(
//     teamId, 'session', sessionId, snapshot.last_seq ?? 0,
//   );
//
// • If any arg is null, the hook is a no-op (returns empty array +
//   isConnected=false). This matches the usePresence pattern.
// • On mount: subscribes via teamEventStream.subscribe; new events are
//   appended to local state in arrival order.
// • On unmount: unsubscribes (releases WS when last subscriber gone).
// • isConnected tracks WS open/close via a second subscription to the
//   manager's connectionListeners channel.

import { useState, useEffect, useRef } from "react";
import { teamEventStream } from "@/lib/teamEventStream";
import type { TeamEvent, SharedResourceKind } from "@/lib/cloud-api";

export type { TeamEvent, SharedResourceKind };

export function useTeamEventStream(
  teamId: string | null,
  kind: SharedResourceKind | null,
  resourceId: string | null,
  initialSeq: number,
): { events: TeamEvent[]; isConnected: boolean } {
  const [events, setEvents] = useState<TeamEvent[]>([]);
  const [isConnected, setIsConnected] = useState(false);

  // Stable ref for initialSeq so the effect only fires when the tuple
  // actually changes (not on every render if the caller passes a literal).
  const initialSeqRef = useRef(initialSeq);
  useEffect(() => { initialSeqRef.current = initialSeq; }, [initialSeq]);

  useEffect(() => {
    if (!teamId || !kind || !resourceId) return;

    const seq = initialSeqRef.current;

    const unsubscribeEvents = teamEventStream.subscribe(
      teamId,
      kind,
      resourceId,
      seq,
      (event: TeamEvent) => {
        setEvents((prev) => {
          // Dedupe by seq_num in case the same event arrives twice
          // (should not happen — the manager dedupes too, but belt+suspenders).
          if (prev.some((e) => e.seq_num === event.seq_num)) return prev;
          // Keep events sorted by seq_num.
          const next = [...prev, event].sort((a, b) => a.seq_num - b.seq_num);
          return next;
        });
      },
    );

    const unsubscribeConn = teamEventStream.subscribeConnectionState(
      teamId,
      kind,
      resourceId,
      seq,
      setIsConnected,
    );

    return () => {
      unsubscribeEvents();
      unsubscribeConn();
    };
  }, [teamId, kind, resourceId]);

  return { events, isConnected };
}
