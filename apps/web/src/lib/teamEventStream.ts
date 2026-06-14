// v2.16 Wave 1 — browser-side team event stream.
//
// Mirrors apps/desktop/src/lib/teamEventStream.ts but stripped of the
// React-Tauri integrations. WS upgrade path lives in the cloud at
// `/api/teams/:teamId/events?presence_token=…&resource_kind=…&resource_id=…&since=…`
// (see ato-cloud/services/mesh-relay/src/teamEvents.ts).
//
// Read-only on web: we never call appendTeamEvent. The stream is a
// live tail on plaintext shares; E2E shares show "Open in desktop"
// in the UI before this is ever subscribed to.

import {
  WS_BASE,
  mintPresenceToken,
  type SharedResourceKind,
  type TeamEvent,
} from "./api";

type Listener = (event: TeamEvent) => void;

interface SubKey {
  teamId: string;
  resourceKind: SharedResourceKind;
  resourceId: string;
}

interface Subscription {
  key: string;
  url: string;
  ws: WebSocket | null;
  listeners: Set<Listener>;
  seenSeqs: Set<number>;
  lastSeq: number;
  reconnectMs: number;
  closeOnDone: boolean;
  reconnectTimer: ReturnType<typeof setTimeout> | null;
}

const subs = new Map<string, Subscription>();

function keyOf({ teamId, resourceKind, resourceId }: SubKey): string {
  return `${teamId}|${resourceKind}|${resourceId}`;
}

export function subscribeTeamEvents(
  k: SubKey,
  initialSeq: number,
  onEvent: Listener,
): () => void {
  const key = keyOf(k);
  let sub = subs.get(key);
  if (!sub) {
    sub = {
      key,
      url: "",
      ws: null,
      listeners: new Set(),
      seenSeqs: new Set(),
      lastSeq: initialSeq,
      reconnectMs: 1000,
      closeOnDone: false,
      reconnectTimer: null,
    };
    subs.set(key, sub);
    void open(sub, k);
  }
  sub.listeners.add(onEvent);
  return () => {
    sub!.listeners.delete(onEvent);
    if (sub!.listeners.size === 0) closeIntentionally(sub!);
  };
}

async function open(sub: Subscription, k: SubKey): Promise<void> {
  if (sub.ws && (sub.ws.readyState === WebSocket.OPEN || sub.ws.readyState === WebSocket.CONNECTING)) {
    return;
  }
  const creds = await mintPresenceToken();
  if (!creds) {
    // No auth → can't open. Schedule a retry; if the user lands here
    // before logging in, the next click will re-trigger.
    scheduleReconnect(sub, k);
    return;
  }
  const params = new URLSearchParams({
    presence_token: creds.token,
    resource_kind: k.resourceKind,
    resource_id: k.resourceId,
    since: String(sub.lastSeq),
  });
  const url = `${WS_BASE}/api/teams/${k.teamId}/events?${params.toString()}`;
  sub.url = url;
  let ws: WebSocket;
  try {
    ws = new WebSocket(url);
  } catch {
    scheduleReconnect(sub, k);
    return;
  }
  sub.ws = ws;
  ws.addEventListener("open", () => {
    sub.reconnectMs = 1000;
  });
  ws.addEventListener("message", (e) => {
    let frame: { type?: string } & TeamEvent;
    try {
      frame = JSON.parse(String(e.data));
    } catch {
      return;
    }
    if (frame.type !== "event") return;
    const ev = frame as TeamEvent;
    if (sub.seenSeqs.has(ev.seq_num)) return;
    sub.seenSeqs.add(ev.seq_num);
    if (ev.seq_num > sub.lastSeq) sub.lastSeq = ev.seq_num;
    for (const l of sub.listeners) {
      try {
        l(ev);
      } catch {
        // Listener errors must not break the stream.
      }
    }
  });
  ws.addEventListener("close", () => {
    sub.ws = null;
    if (!sub.closeOnDone && sub.listeners.size > 0) scheduleReconnect(sub, k);
  });
  ws.addEventListener("error", () => {
    try {
      ws.close();
    } catch {
      // ignore
    }
  });
}

function scheduleReconnect(sub: Subscription, k: SubKey): void {
  if (sub.reconnectTimer) clearTimeout(sub.reconnectTimer);
  const delay = Math.min(sub.reconnectMs, 30_000);
  sub.reconnectMs = Math.min(sub.reconnectMs * 2, 30_000);
  sub.reconnectTimer = setTimeout(() => {
    sub.reconnectTimer = null;
    if (sub.listeners.size > 0) void open(sub, k);
  }, delay);
}

function closeIntentionally(sub: Subscription): void {
  sub.closeOnDone = true;
  if (sub.reconnectTimer) {
    clearTimeout(sub.reconnectTimer);
    sub.reconnectTimer = null;
  }
  if (sub.ws) {
    try {
      sub.ws.close();
    } catch {
      // ignore
    }
    sub.ws = null;
  }
  subs.delete(sub.key);
}
