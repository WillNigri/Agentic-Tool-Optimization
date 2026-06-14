// usePresence — Live Team Workspace hook.
//
// Subscribes to mesh-relay presence frames for a given resource and
// returns the live snapshot of co-viewers + their cursors. Emits
// presence_join on mount, presence_leave on unmount, and throttled
// presence_cursor on mouse move.
//
// PRO-tier gated by the caller — this hook is a no-op when the user's
// effective tier is "free". The check happens here too as a defense
// in depth (no frames sent on free tier; no listeners attached).

import { useCallback, useEffect, useRef, useState } from "react";

import { useTier } from "@/lib/tier";
import {
  meshRelay,
  type PresenceFrame,
  type PresenceResourceKind,
} from "@/lib/meshRelay";

export interface PresenceViewer {
  peerId: string;
  viewerLabel?: string;
}

export interface PresenceCursor {
  peerId: string;
  x: number;
  y: number;
  updatedAt: number;
}

export interface PresenceSnapshot {
  viewers: PresenceViewer[];
  cursors: PresenceCursor[];
}

const EMPTY_SNAPSHOT: PresenceSnapshot = { viewers: [], cursors: [] };

const CURSOR_THROTTLE_MS = 50;
const CURSOR_STALE_MS = 5_000;

export interface UsePresenceOptions {
  resourceKind: PresenceResourceKind;
  resourceId: string;
  viewerLabel?: string;
}

export interface UsePresenceResult {
  snapshot: PresenceSnapshot;
  reportCursor: (x: number, y: number) => void;
  enabled: boolean;
}

export function usePresence({
  resourceKind,
  resourceId,
  viewerLabel,
}: UsePresenceOptions): UsePresenceResult {
  const tier = useTier();
  const enabled = tier !== "free";
  const [snapshot, setSnapshot] = useState<PresenceSnapshot>(EMPTY_SNAPSHOT);
  const lastCursorSendRef = useRef(0);

  useEffect(() => {
    if (!enabled) {
      setSnapshot(EMPTY_SNAPSHOT);
      return;
    }
    let unsubscribed = false;

    const listener = (frame: PresenceFrame) => {
      if (frame.resource_kind !== resourceKind || frame.resource_id !== resourceId) return;
      setSnapshot((prev) => {
        switch (frame.type) {
          case "presence_snapshot": {
            return {
              viewers: frame.viewers.map((v) => ({
                peerId: v.peer_id,
                viewerLabel: v.viewer_label,
              })),
              // Keep the prior cursor positions — snapshot fires on join
              // and shouldn't blank out cursors that already moved.
              cursors: prev.cursors,
            };
          }
          case "presence_join": {
            if (prev.viewers.some((v) => v.peerId === frame.peer_id)) return prev;
            return {
              viewers: [...prev.viewers, { peerId: frame.peer_id, viewerLabel: frame.viewer_label }],
              cursors: prev.cursors,
            };
          }
          case "presence_leave": {
            return {
              viewers: prev.viewers.filter((v) => v.peerId !== frame.peer_id),
              cursors: prev.cursors.filter((c) => c.peerId !== frame.peer_id),
            };
          }
          case "presence_cursor": {
            const next = prev.cursors.filter((c) => c.peerId !== frame.peer_id);
            next.push({ peerId: frame.peer_id, x: frame.x, y: frame.y, updatedAt: Date.now() });
            return { viewers: prev.viewers, cursors: next };
          }
          default:
            return prev;
        }
      });
    };

    const unsubscribe = meshRelay.subscribe(listener);

    // Emit join + snapshot query.
    meshRelay.send({
      type: "presence_join",
      resource_kind: resourceKind,
      resource_id: resourceId,
      viewer_label: viewerLabel,
    });
    meshRelay.send({
      type: "presence_query",
      resource_kind: resourceKind,
      resource_id: resourceId,
    });

    // Cull stale cursors every second so a teammate that stopped moving
    // doesn't leave a stuck marker on screen forever.
    const cullInterval = window.setInterval(() => {
      const cutoff = Date.now() - CURSOR_STALE_MS;
      setSnapshot((prev) => {
        const fresh = prev.cursors.filter((c) => c.updatedAt > cutoff);
        if (fresh.length === prev.cursors.length) return prev;
        return { viewers: prev.viewers, cursors: fresh };
      });
    }, 1_000);

    return () => {
      unsubscribed = true;
      window.clearInterval(cullInterval);
      try {
        meshRelay.send({
          type: "presence_leave",
          resource_kind: resourceKind,
          resource_id: resourceId,
        });
      } catch {
        // Best-effort — if the connection is already gone, the server
        // will time out the claim after 30s anyway.
      }
      unsubscribe();
      void unsubscribed;
    };
  }, [enabled, resourceKind, resourceId, viewerLabel]);

  const reportCursor = useCallback(
    (x: number, y: number) => {
      if (!enabled) return;
      const now = Date.now();
      if (now - lastCursorSendRef.current < CURSOR_THROTTLE_MS) return;
      lastCursorSendRef.current = now;
      meshRelay.send({
        type: "presence_cursor",
        resource_kind: resourceKind,
        resource_id: resourceId,
        x,
        y,
      });
    },
    [enabled, resourceKind, resourceId],
  );

  return { snapshot, reportCursor, enabled };
}
