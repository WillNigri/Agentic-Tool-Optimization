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

// v2.16 Wave 4 — connection state surface. Wave 1's "connected" was
// optimistic (flipped after backfill resolved). Callers can now
// subscribe to actual WS lifecycle events.
export type ConnectionState =
  | "idle"          // no listeners + no open WS
  | "connecting"    // WS opening / reconnect pending
  | "open"          // WS is open and receiving events
  | "reconnecting"  // WS dropped; backoff timer running
  | "error";        // unrecoverable (e.g. mint token returned null)
export type ConnectionListener = (state: ConnectionState) => void;

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
  // Wave 4 — connection-state listeners are tracked separately so a
  // caller can subscribe to one or both surfaces independently.
  connListeners: Set<ConnectionListener>;
  state: ConnectionState;
  lastSuccessAt: number | null; // unix ms of last successful open
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

function setState(sub: Subscription, next: ConnectionState): void {
  if (sub.state === next) return;
  sub.state = next;
  if (next === "open") sub.lastSuccessAt = Date.now();
  for (const l of sub.connListeners) {
    try { l(next); } catch { /* ignore */ }
  }
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
      connListeners: new Set(),
      state: "idle",
      lastSuccessAt: null,
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
    if (sub!.listeners.size === 0 && sub!.connListeners.size === 0) {
      closeIntentionally(sub!);
    }
  };
}

// v2.16 Wave 4 — public subscriber for connection state. Callers get
// the current snapshot immediately, then notifications on every
// transition. Returns an unsubscribe; if both event and connection
// listeners are gone, the WS closes intentionally.
//
// Codex R1 #3 fix — pre-fix shape fired "idle" once when called
// before subscribeTeamEvents and never opened a WS. Comment claimed
// callers could subscribe to connection state independently of
// events; behavior contradicted it. Now: create / reuse the
// Subscription the same way subscribeTeamEvents does. initialSeq
// defaults to 0 because the connection-only caller doesn't know
// what to ask for; the next subscribeTeamEvents call will refresh
// lastSeq before the WS reconnect window.
export function subscribeConnectionState(
  k: SubKey,
  cb: ConnectionListener,
): () => void {
  const key = keyOf(k);
  let sub = subs.get(key);
  if (!sub) {
    sub = {
      key,
      url: "",
      ws: null,
      listeners: new Set(),
      connListeners: new Set(),
      state: "idle",
      lastSuccessAt: null,
      seenSeqs: new Set(),
      lastSeq: 0,
      reconnectMs: 1000,
      closeOnDone: false,
      reconnectTimer: null,
    };
    subs.set(key, sub);
    void open(sub, k);
  }
  sub.connListeners.add(cb);
  cb(sub.state);
  return () => {
    sub!.connListeners.delete(cb);
    if (sub!.listeners.size === 0 && sub!.connListeners.size === 0) {
      closeIntentionally(sub!);
    }
  };
}

// Convenience snapshot accessor — useful for one-off reads in
// React's render path. Re-render via subscribeConnectionState.
export function getConnectionState(k: SubKey): {
  state: ConnectionState;
  lastSuccessAt: number | null;
} {
  const sub = subs.get(keyOf(k));
  if (!sub) return { state: "idle", lastSuccessAt: null };
  return { state: sub.state, lastSuccessAt: sub.lastSuccessAt };
}

async function open(sub: Subscription, k: SubKey): Promise<void> {
  if (sub.ws && (sub.ws.readyState === WebSocket.OPEN || sub.ws.readyState === WebSocket.CONNECTING)) {
    return;
  }
  // Wave 4 — transition idle/reconnecting → connecting before the
  // async mint call so the UI can show a spinner during auth.
  setState(sub, "connecting");
  const creds = await mintPresenceToken();
  if (!creds) {
    // No auth → can't open. Schedule a retry; if the user lands here
    // before logging in, the next click will re-trigger.
    setState(sub, "reconnecting");
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
    setState(sub, "reconnecting");
    scheduleReconnect(sub, k);
    return;
  }
  sub.ws = ws;
  ws.addEventListener("open", () => {
    sub.reconnectMs = 1000;
    setState(sub, "open");
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
    if (sub.closeOnDone) {
      // Intentional close; the closeIntentionally helper already
      // emits the idle state.
      return;
    }
    if (sub.listeners.size > 0 || sub.connListeners.size > 0) {
      setState(sub, "reconnecting");
      scheduleReconnect(sub, k);
    }
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
    if (sub.listeners.size > 0 || sub.connListeners.size > 0) void open(sub, k);
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
  setState(sub, "idle");
  subs.delete(sub.key);
}
