// Live event transport for team-shared resource event logs.
//
// This module is SEPARATE from meshRelay.ts, which handles presence
// frames only (join/leave/cursor). Do not conflate the two transports.
//
// Architecture (matches Wave 2 synthesis §Q4):
//   • One WS per (teamId, kind, resourceId) tuple — lazy-opened on
//     first subscribe, closed on last unsubscribe.
//   • Auth: POST /api/auth/mesh-presence-token mints a short-lived
//     mst_ JWT (same token the presence relay accepts). Cached and
//     refreshed ~1 min before expiry to avoid reconnect storms at TTL.
//   • URL: wss://…/api/teams/:tid/events?presence_token=<t>
//           &resource_kind=<k>&resource_id=<id>&since=<seq>
//   • On connect the server backfills (seq_num > since) then streams
//     live appends. On reconnect, ?since=<last_seen_seq> catches up.
//   • Dedupe: each subscriber wrapper tracks seen seq_nums and skips
//     duplicates. Guards against server retransmission on reconnect.
//   • Reconnect: exponential backoff 1s → 30s; re-issues ?since=<last>
//     so no gaps after a reconnect.
//
// Kill-switch: VITE_TEAM_EVENT_STREAM_DISABLED=1 makes subscribe() a
// no-op (returns an empty unsubscribe). Mirrors the presence pattern.

import { getStoredTokens } from "./cloud-api";
import type { TeamEvent, SharedResourceKind } from "./cloud-api";

export type { TeamEvent, SharedResourceKind };

/**
 * Optional decryptor callback injected into the event stream for E2E shares.
 *
 * When set on a subscription, every incoming event passes through this function
 * before being delivered to UI listeners. The decryptor should:
 *   1. Parse ciphertext + nonce from the raw event.
 *   2. Look up the signer's pubkey via a local member key cache.
 *   3. Verify the Ed25519 signature.
 *   4. Decrypt with the Team Key from the in-memory cache.
 *   5. Return a new TeamEvent with payload_json filled in.
 *
 * On any failure the decryptor should return
 * `{ ...raw, payload_json: { __decrypt_error: true } }` rather than throwing,
 * so listener errors don't break the relay loop. The UI surfaces a banner.
 */
export type DecryptorFn = (raw: TeamEvent) => Promise<TeamEvent>;

const TRANSPORT_ENABLED =
  (import.meta.env.VITE_TEAM_EVENT_STREAM_DISABLED as string | undefined) !== "1";

const CLOUD_API_URL =
  (import.meta.env.VITE_CLOUD_API_URL as string | undefined) ||
  "https://api.agentictool.ai";

const CLOUD_WS_URL =
  (import.meta.env.VITE_CLOUD_WS_URL as string | undefined) ||
  "wss://api.agentictool.ai";

// Map SharedResourceKind → URL plural segment (mirrors cloud-api.ts)
function kindToSegment(kind: SharedResourceKind): string {
  const map: Record<SharedResourceKind, string> = {
    session: "sessions",
    "war-room": "war-rooms",
    chat: "chats",
    loop: "loops",
    mission: "missions",
  };
  return map[kind];
}

type EventListener = (event: TeamEvent) => void;

/** Per-subscription options (Wave 3: decryptor for E2E shares). */
interface SubscribeOptions {
  decryptor?: DecryptorFn;
}

interface CachedToken {
  token: string;
  expiresAt: number; // unix ms
}

/** Canonical key for a (teamId, kind, resourceId) tuple. */
function tupleKey(teamId: string, kind: SharedResourceKind, resourceId: string): string {
  return `${teamId}:${kind}:${resourceId}`;
}

/** Internal state for one open (or reconnecting) WS connection. */
interface ConnectionState {
  ws: WebSocket | null;
  /** Map from raw EventListener → wrapped listener (may include decryptor). */
  listeners: Map<EventListener, EventListener>;
  /** Connected/disconnected observers for the isConnected flag. */
  connectionListeners: Set<(connected: boolean) => void>;
  lastSeenSeq: number;
  seenSeqs: Set<number>;
  reconnectDelayMs: number;
  intentionalClose: boolean;
  reconnectTimer: ReturnType<typeof setTimeout> | null;
  /** Fires ~10s after a WS opens; only then is the backoff reset to 1s, so an
   *  open-then-instantly-close socket (e.g. server has no events handler) can't
   *  reset the backoff and hammer mesh-presence-token every second. Cleared on
   *  close so a quick close never resets. */
  stabilityTimer: ReturnType<typeof setTimeout> | null;
  // Store last open parameters for reconnect.
  teamId: string;
  kind: SharedResourceKind;
  resourceId: string;
}

class TeamEventStreamManager {
  /** One entry per active (teamId, kind, resourceId) tuple. */
  private connections = new Map<string, ConnectionState>();
  /** Single shared token cache — same endpoint, same TTL for all tuples. */
  private cachedToken: CachedToken | null = null;
  private tokenFetchPromise: Promise<CachedToken | null> | null = null;

  /**
   * Subscribe to live events for a resource.
   *
   * @param teamId     - Team UUID
   * @param kind       - Resource kind ('session' | 'war-room' | ...)
   * @param resourceId - Resource UUID
   * @param since      - Initial seq_num to replay from (exclusive). Use
   *                     `snapshot.last_seq ?? 0` from the REST response.
   * @param onEvent    - Called for each new (or replayed) event.
   * @param options    - Optional: `decryptor` for E2E shares (Wave 3).
   * @returns  Unsubscribe function — call it when the component unmounts.
   */
  subscribe(
    teamId: string,
    kind: SharedResourceKind,
    resourceId: string,
    since: number,
    onEvent: EventListener,
    options?: SubscribeOptions,
  ): () => void {
    if (!TRANSPORT_ENABLED) return () => {};

    const key = tupleKey(teamId, kind, resourceId);
    let state = this.connections.get(key);
    if (!state) {
      state = {
        ws: null,
        listeners: new Map(),
        connectionListeners: new Set(),
        lastSeenSeq: since,
        seenSeqs: new Set(),
        reconnectDelayMs: 1_000,
        intentionalClose: false,
        reconnectTimer: null,
        stabilityTimer: null,
        teamId,
        kind,
        resourceId,
      };
      this.connections.set(key, state);
    }

    // Wrap the listener with the decryptor if one is provided.
    // The decryptor runs async, but we call listeners synchronously via the
    // wrapped fn which schedules a .then() and re-delivers. Events stay in order
    // because they're pushed through the same setEvents reducer in the React hook.
    const wrapped: EventListener = options?.decryptor
      ? (event: TeamEvent) => {
          void options.decryptor!(event).then((decrypted) => {
            try { onEvent(decrypted); } catch { /* ignore listener errors */ }
          });
        }
      : onEvent;

    state.listeners.set(onEvent, wrapped);
    this.ensureOpen(state);

    return () => {
      state!.listeners.delete(onEvent);
      if (state!.listeners.size === 0 && state!.connectionListeners.size === 0) {
        this.closeIntentionally(state!);
        this.connections.delete(key);
      }
    };
  }

  /**
   * Subscribe to the raw connected/disconnected state for a tuple.
   * Used by useTeamEventStream to expose `isConnected`.
   * Returns unsubscribe.
   */
  subscribeConnectionState(
    teamId: string,
    kind: SharedResourceKind,
    resourceId: string,
    since: number,
    onConnectionChange: (connected: boolean) => void,
  ): () => void {
    if (!TRANSPORT_ENABLED) return () => {};

    const key = tupleKey(teamId, kind, resourceId);
    let state = this.connections.get(key);
    if (!state) {
      state = {
        ws: null,
        listeners: new Map(),
        connectionListeners: new Set(),
        lastSeenSeq: since,
        seenSeqs: new Set(),
        reconnectDelayMs: 1_000,
        intentionalClose: false,
        reconnectTimer: null,
        stabilityTimer: null,
        teamId,
        kind,
        resourceId,
      };
      this.connections.set(key, state);
    }

    state.connectionListeners.add(onConnectionChange);
    this.ensureOpen(state);

    return () => {
      state!.connectionListeners.delete(onConnectionChange);
      if (state!.listeners.size === 0 && state!.connectionListeners.size === 0) {
        this.closeIntentionally(state!);
        this.connections.delete(key);
      }
    };
  }

  private ensureOpen(state: ConnectionState): void {
    if (state.intentionalClose) state.intentionalClose = false;
    if (
      state.ws &&
      (state.ws.readyState === WebSocket.OPEN ||
        state.ws.readyState === WebSocket.CONNECTING)
    ) {
      return;
    }
    void this.openWithToken(state);
  }

  private async fetchToken(): Promise<CachedToken | null> {
    // Reuse if still fresh (≥60s remaining).
    if (this.cachedToken && this.cachedToken.expiresAt - Date.now() > 60_000) {
      return this.cachedToken;
    }
    // Deduplicate concurrent fetches.
    if (this.tokenFetchPromise) return this.tokenFetchPromise;

    const tokens = getStoredTokens();
    if (!tokens?.accessToken) return null;

    this.tokenFetchPromise = (async () => {
      try {
        const res = await fetch(`${CLOUD_API_URL}/api/auth/mesh-presence-token`, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${tokens.accessToken}`,
          },
        });
        if (!res.ok) return null;
        const body = (await res.json()) as {
          success?: boolean;
          data?: { token?: string; expires_at?: string };
        };
        if (!body.success || !body.data?.token || !body.data?.expires_at) return null;
        const cached: CachedToken = {
          token: body.data.token,
          expiresAt: new Date(body.data.expires_at).getTime(),
        };
        this.cachedToken = cached;
        return cached;
      } catch {
        return null;
      } finally {
        this.tokenFetchPromise = null;
      }
    })();
    return this.tokenFetchPromise;
  }

  private async openWithToken(state: ConnectionState): Promise<void> {
    const cached = await this.fetchToken();
    if (!cached || state.intentionalClose) return;
    if (
      state.ws &&
      (state.ws.readyState === WebSocket.OPEN ||
        state.ws.readyState === WebSocket.CONNECTING)
    ) {
      return;
    }

    const segment = kindToSegment(state.kind);
    // Unused segment variable kept for semantic clarity — the server
    // uses resource_kind + resource_id query params on the single /events
    // endpoint, not a per-kind path.
    void segment;

    const url =
      `${CLOUD_WS_URL}/api/teams/${state.teamId}/events` +
      `?presence_token=${encodeURIComponent(cached.token)}` +
      `&resource_kind=${encodeURIComponent(state.kind)}` +
      `&resource_id=${encodeURIComponent(state.resourceId)}` +
      `&since=${state.lastSeenSeq}`;

    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch {
      this.scheduleReconnect(state);
      return;
    }
    state.ws = ws;

    ws.addEventListener("open", () => {
      // Only reset the backoff once the socket proves STABLE (still open after
      // ~10s). Resetting immediately on open let an open-then-instantly-close
      // socket (server with no events handler) loop every 1s and hammer
      // mesh-presence-token. Cleared in the close handler.
      if (state.stabilityTimer) clearTimeout(state.stabilityTimer);
      state.stabilityTimer = setTimeout(() => {
        state.stabilityTimer = null;
        if (state.ws && state.ws.readyState === WebSocket.OPEN) {
          state.reconnectDelayMs = 1_000;
        }
      }, 10_000);
      // Drop the cached token on open so the next reconnect re-mints.
      // (The token TTL is 15 min; we cache it for reuse across short
      //  reconnects but drop it here in case the WS lived long enough
      //  to approach the boundary.)
      for (const l of state.connectionListeners) {
        try { l(true); } catch { /* ignore listener errors */ }
      }
    });

    ws.addEventListener("message", (e) => {
      let event: TeamEvent | undefined;
      try {
        event = JSON.parse(String(e.data)) as TeamEvent;
      } catch {
        return;
      }
      if (!event || typeof event.seq_num !== "number") return;

      // Dedupe: skip if we've seen this seq_num already.
      if (state.seenSeqs.has(event.seq_num)) return;
      state.seenSeqs.add(event.seq_num);
      if (event.seq_num > state.lastSeenSeq) {
        state.lastSeenSeq = event.seq_num;
      }

      // Call wrapped listeners (which may include async decryptors).
      for (const wrapped of state.listeners.values()) {
        try { wrapped(event); } catch { /* listener errors must not break the relay */ }
      }
    });

    ws.addEventListener("close", () => {
      state.ws = null;
      // Cancel the stability timer — a socket that closed before proving
      // stable must NOT reset the backoff, so repeated quick closes keep
      // backing off (1s→2→4…→30s) instead of looping every second.
      if (state.stabilityTimer) {
        clearTimeout(state.stabilityTimer);
        state.stabilityTimer = null;
      }
      // Evict token so next reconnect re-mints (guards against stale
      // token at ~15-min TTL boundary).
      this.cachedToken = null;
      for (const l of state.connectionListeners) {
        try { l(false); } catch { /* ignore */ }
      }
      if (
        !state.intentionalClose &&
        (state.listeners.size > 0 || state.connectionListeners.size > 0)
      ) {
        this.scheduleReconnect(state);
      }
    });

    ws.addEventListener("error", () => {
      try { ws.close(); } catch { /* ignore */ }
    });
  }

  private scheduleReconnect(state: ConnectionState): void {
    if (state.reconnectTimer) clearTimeout(state.reconnectTimer);
    const delay = Math.min(state.reconnectDelayMs, 30_000);
    state.reconnectDelayMs = Math.min(state.reconnectDelayMs * 2, 30_000);
    state.reconnectTimer = setTimeout(() => {
      state.reconnectTimer = null;
      if (
        !state.intentionalClose &&
        (state.listeners.size > 0 || state.connectionListeners.size > 0)
      ) {
        this.ensureOpen(state);
      }
    }, delay);
  }

  private closeIntentionally(state: ConnectionState): void {
    state.intentionalClose = true;
    if (state.reconnectTimer) {
      clearTimeout(state.reconnectTimer);
      state.reconnectTimer = null;
    }
    // Clear the stability timer directly on teardown rather than relying on the
    // async "close" event firing — a torn-down connection (deleted from the map)
    // must not leave a 10s timer holding the state alive (codex R1).
    if (state.stabilityTimer) {
      clearTimeout(state.stabilityTimer);
      state.stabilityTimer = null;
    }
    if (state.ws) {
      try { state.ws.close(); } catch { /* ignore */ }
      state.ws = null;
    }
  }

  // Exposed for tests only — returns the internal connection map size.
  _connectionCount(): number {
    return this.connections.size;
  }

  // Exposed for tests — returns the current WS readyState for a tuple
  // or -1 if no connection exists.
  _wsReadyState(
    teamId: string,
    kind: SharedResourceKind,
    resourceId: string,
  ): number {
    const state = this.connections.get(tupleKey(teamId, kind, resourceId));
    if (!state?.ws) return -1;
    return state.ws.readyState;
  }

  // Exposed for tests — returns the lastSeenSeq for a tuple.
  _lastSeenSeq(
    teamId: string,
    kind: SharedResourceKind,
    resourceId: string,
  ): number {
    return this.connections.get(tupleKey(teamId, kind, resourceId))?.lastSeenSeq ?? 0;
  }
}

/** Singleton — all components share one manager instance. */
export const teamEventStream = new TeamEventStreamManager();
