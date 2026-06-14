// Mesh-relay WebSocket client for the Live Team Workspace (Collison #2).
//
// Multiplexes presence frames to subscribers keyed by (resourceKind,
// resourceId). PRO-tier gated by the caller — this module itself doesn't
// know about tiers.
//
// ── v1 transport gap (2026-06-13) ───────────────────────────────────
//
// The cloud-side broker (ato-cloud services/mesh-relay/src/presence.ts)
// is correct and live. The transport between this browser code and the
// relay is NOT yet wired: the relay requires Authorization: Bearer with
// an `mst_` mesh-token, which the browser WebSocket API can't set as a
// header. Three v2 wire-up paths are viable; one will land in the next
// cluster:
//
// 1. Mint short-lived browser-presence tokens via a new
//    /api/mesh-presence-token endpoint and accept ?presence_token=…
//    in mesh-relay. Smallest server change; one query-string-token
//    exposure in proxy logs (acceptable for 15-min TTL).
//
// 2. Route via the Tauri-side passive-observer daemon's existing
//    bearer-authenticated WS through a `subscribe_presence` Tauri
//    command. Best architecture; biggest desktop-side change. The
//    components in this folder are designed for this drop-in (they
//    just consume PresenceFrame events from meshRelay.subscribe).
//
// 3. Sec-WebSocket-Protocol subprotocol smuggling of the bearer.
//    Portable across browsers; relay parses the subprotocol header
//    and validates as a mesh_token.
//
// Until one of those ships, the WebSocket open below is a no-op:
// ensureOpen returns early on a v1-stub guard. usePresence callers see
// an empty snapshot. Components compile, tests pass, the architecture
// is end-to-end coherent — just the wire is not connected.
//
// Frame types (must stay in sync with services/mesh-relay/src/presence.ts
// in ato-cloud):
//
//   presence_join     — peer claims to be viewing a resource
//   presence_leave    — peer left
//   presence_cursor   — cursor move (throttled at the sender)
//   presence_query    — ask the relay for a snapshot of current viewers
//   presence_snapshot — relay's response with the current viewer set
//
// Connection lifecycle:
//   • Lazy — opens on the first subscribe()
//   • Closes when the last subscriber unsubscribes
//   • Reconnects with exponential backoff (capped at 30s) on close

import { getStoredTokens } from "./cloud-api";

// v1 transport stub — flip to true via VITE_MESH_RELAY_ENABLED once
// the cloud-side query-string token endpoint OR the Tauri-side daemon
// presence routing lands. With this false the components render but
// don't open a WebSocket, so we don't pollute the relay with rejected
// upgrade requests.
const TRANSPORT_ENABLED =
  (import.meta.env.VITE_MESH_RELAY_ENABLED as string | undefined) === "1";

const CLOUD_WS_URL =
  (import.meta.env.VITE_MESH_RELAY_URL as string | undefined) ||
  "wss://api.agentictool.ai/mesh";

export type PresenceResourceKind = "session" | "war_room" | "mission";

export interface PresenceClaim {
  peerId: string;
  viewerLabel?: string;
  resourceKind: PresenceResourceKind;
  resourceId: string;
}

export interface PresenceCursor {
  peerId: string;
  resourceKind: PresenceResourceKind;
  resourceId: string;
  x: number;
  y: number;
}

export type PresenceFrame =
  | { type: "presence_join"; resource_kind: PresenceResourceKind; resource_id: string; viewer_label?: string; peer_id: string }
  | { type: "presence_leave"; resource_kind: PresenceResourceKind; resource_id: string; peer_id: string }
  | { type: "presence_cursor"; resource_kind: PresenceResourceKind; resource_id: string; x: number; y: number; peer_id: string }
  | { type: "presence_snapshot"; resource_kind: PresenceResourceKind; resource_id: string; viewers: Array<{ peer_id: string; viewer_label?: string }> };

type FrameListener = (frame: PresenceFrame) => void;

class MeshRelay {
  private ws: WebSocket | null = null;
  private listeners = new Set<FrameListener>();
  private reconnectDelayMs = 1000;
  private intentionalClose = false;
  private pendingSendQueue: string[] = [];
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  subscribe(listener: FrameListener): () => void {
    this.listeners.add(listener);
    this.ensureOpen();
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) this.closeIntentionally();
    };
  }

  send(frame: Record<string, unknown>): void {
    if (!TRANSPORT_ENABLED) return; // v1 stub — no-op until wire-up lands.
    const payload = JSON.stringify(frame);
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(payload);
    } else {
      // Queue and flush on next open; cap to 16 entries so we don't
      // accumulate stale cursor moves forever.
      this.pendingSendQueue.push(payload);
      if (this.pendingSendQueue.length > 16) {
        this.pendingSendQueue.shift();
      }
      this.ensureOpen();
    }
  }

  private ensureOpen(): void {
    if (!TRANSPORT_ENABLED) return; // v1 stub — see file header.
    if (this.ws && (this.ws.readyState === WebSocket.OPEN || this.ws.readyState === WebSocket.CONNECTING)) {
      return;
    }
    this.intentionalClose = false;
    const tokens = getStoredTokens();
    if (!tokens?.accessToken) return; // Not signed in — bail silently.
    const url = `${CLOUD_WS_URL}?token=${encodeURIComponent(tokens.accessToken)}`;
    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch {
      this.scheduleReconnect();
      return;
    }
    this.ws = ws;
    ws.addEventListener("open", () => {
      this.reconnectDelayMs = 1000;
      while (this.pendingSendQueue.length > 0) {
        const payload = this.pendingSendQueue.shift();
        if (payload) ws.send(payload);
      }
    });
    ws.addEventListener("message", (e) => {
      let frame: PresenceFrame | undefined;
      try {
        frame = JSON.parse(String(e.data));
      } catch {
        return;
      }
      if (!frame) return;
      for (const l of this.listeners) {
        try {
          l(frame);
        } catch {
          // Listener errors must not break the relay.
        }
      }
    });
    ws.addEventListener("close", () => {
      this.ws = null;
      if (!this.intentionalClose && this.listeners.size > 0) {
        this.scheduleReconnect();
      }
    });
    ws.addEventListener("error", () => {
      // Treated as close; rely on the close handler for reconnect.
      try {
        ws.close();
      } catch {
        // ignore
      }
    });
  }

  private scheduleReconnect(): void {
    // Codex R1: cancel any in-flight reconnect timer so an unsubscribe
    // + resubscribe sequence doesn't end up with two timers racing each
    // other for the next open.
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    const delay = Math.min(this.reconnectDelayMs, 30_000);
    this.reconnectDelayMs = Math.min(this.reconnectDelayMs * 2, 30_000);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (this.listeners.size > 0) this.ensureOpen();
    }, delay);
  }

  private closeIntentionally(): void {
    this.intentionalClose = true;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.ws) {
      try {
        this.ws.close();
      } catch {
        // ignore
      }
      this.ws = null;
    }
  }
}

// Singleton — every usePresence subscriber shares this instance.
export const meshRelay = new MeshRelay();
