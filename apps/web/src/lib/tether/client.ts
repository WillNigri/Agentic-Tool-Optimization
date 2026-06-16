// v2.17 Wave 3 — browser tether client singleton.
//
// One open WS per browser-session-id (minted once on module load; stable
// for the tab lifetime). Cloud relay is a DUMB PIPE of AEAD blobs; this
// client handles the X25519 handshake end-to-end.
//
// Frame nonce scheme (24 bytes):
//   direction_byte(1) || frame_seq_be(8) || random(15)
//   direction_byte = 0x01 browser→host, 0x02 host→browser.
//   random(15) ensures uniqueness even if frame_seq resets.

import { SHA256 } from "@stablelib/sha256";
import {
  generateTetherKeypair,
  deriveSessionKey,
  deriveNonce,
  aeadEncrypt,
  aeadDecrypt,
  toBase64,
  fromBase64,
} from "./crypto";
import { WS_BASE, mintPresenceToken, apiRequest } from "../api";

// ──────────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────────

export type TetherState =
  | "idle"
  | "connecting"
  | "pending_approval"  // sent tether_hello, awaiting desktop user
  | "approved"
  | "denied"
  | "host_offline"
  | "error";

export interface TetherInfo {
  state: TetherState;
  machineName: string | null;
  sessionId: string | null;
}

// ──────────────────────────────────────────────────────────────────
// Internal singleton state
// ──────────────────────────────────────────────────────────────────

/** Stable browser-session-id for this tab. Never persisted. */
const BROWSER_SESSION_ID: string = crypto.randomUUID();

interface PendingRpc {
  resolve: (value: unknown) => void;
  reject: (err: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

interface TetherSingleton {
  state: TetherState;
  machineName: string | null;
  sessionId: string | null;
  ws: WebSocket | null;
  sessionKey: Uint8Array | null;
  txSeq: bigint;
  rxSeq: bigint;
  pendingRpcs: Map<string, PendingRpc>;
  listeners: Set<(info: TetherInfo) => void>;
  hostFrameListeners: Set<(frame: Record<string, unknown>) => void>;
}

const singleton: TetherSingleton = {
  state: "idle",
  machineName: null,
  sessionId: null,
  ws: null,
  sessionKey: null,
  txSeq: 0n,
  rxSeq: 0n,
  pendingRpcs: new Map(),
  listeners: new Set(),
  hostFrameListeners: new Set(),
};

// ──────────────────────────────────────────────────────────────────
// Public read/subscribe API
// ──────────────────────────────────────────────────────────────────

export function getTether(): TetherInfo {
  return {
    state: singleton.state,
    machineName: singleton.machineName,
    sessionId: singleton.sessionId,
  };
}

export function subscribeTetherState(
  cb: (info: TetherInfo) => void,
): () => void {
  singleton.listeners.add(cb);
  // Fire immediately with current state.
  cb(getTether());
  return () => {
    singleton.listeners.delete(cb);
  };
}

// ──────────────────────────────────────────────────────────────────
// API: available tether sessions
// ──────────────────────────────────────────────────────────────────

interface TetherSession {
  id: string;
  desktop_machine_name: string;
  approval_state: string;
}

export async function listTetherSessions(): Promise<TetherSession[]> {
  try {
    return await apiRequest<TetherSession[]>("/tether/sessions");
  } catch {
    return [];
  }
}

// Codex R6 fix — GET /api/tether/hosts returns LIVE hosts from the
// mesh-relay's hostsByUser registry (mirrored into the
// tether_hosts_online table on connect/disconnect). Use this instead
// of listTetherSessions() for host discovery: the sessions list
// returns historical approval rows, which fresh users have zero of
// (causing the "no hosts" empty state) and stale entries that look
// live when the desktop has disconnected.
export interface OnlineHost {
  machine_name: string;
  host_xd_pubkey_b64: string;
  connected_at: string;
  last_heartbeat_at: string;
}

export async function listOnlineHosts(): Promise<OnlineHost[]> {
  try {
    return await apiRequest<OnlineHost[]>("/tether/hosts");
  } catch {
    return [];
  }
}

// ──────────────────────────────────────────────────────────────────
// startTether / stopTether
// ──────────────────────────────────────────────────────────────────

export async function startTether(targetMachineName: string): Promise<void> {
  if (singleton.state === "approved" || singleton.state === "pending_approval" || singleton.state === "connecting") {
    // Already in flight or paired — noop.
    return;
  }

  // Reset state for a fresh pairing attempt.
  teardownWs();
  setState("connecting", targetMachineName, null);

  const creds = await mintPresenceToken();
  if (!creds) {
    setState("error", targetMachineName, null);
    return;
  }

  // Generate ephemeral keypair. We'll discard privkey after DH below.
  const ephemeralKp = generateTetherKeypair();

  // Compute UA hash for the tether_hello frame.
  const uaHash = await computeUaHash();

  const params = new URLSearchParams({
    presence_token: creds.token,
    browser_session_id: BROWSER_SESSION_ID,
  });
  const url = `${WS_BASE}/api/tether/client?${params.toString()}`;

  let ws: WebSocket;
  try {
    ws = new WebSocket(url);
  } catch {
    setState("error", targetMachineName, null);
    return;
  }
  singleton.ws = ws;

  ws.addEventListener("open", () => {
    // Send tether_hello immediately on open.
    const hello = {
      type: "tether_hello",
      browser_xb_pub_b64: toBase64(ephemeralKp.pubkey),
      browser_ua_hash: uaHash,
      browser_ip_class: null, // server-side derive; cloud already accepts null
      target_machine_name: targetMachineName,
      browser_session_id: BROWSER_SESSION_ID,
    };
    try {
      ws.send(JSON.stringify(hello));
    } catch {
      setState("error", targetMachineName, null);
    }
  });

  ws.addEventListener("message", (e) => {
    let frame: Record<string, unknown>;
    try {
      frame = JSON.parse(String(e.data)) as Record<string, unknown>;
    } catch {
      return;
    }
    handleFrame(frame, ephemeralKp.privkey, targetMachineName);
  });

  ws.addEventListener("close", () => {
    singleton.ws = null;
    if (singleton.state !== "approved" && singleton.state !== "denied") {
      setState("host_offline", targetMachineName, null);
    }
  });

  ws.addEventListener("error", () => {
    try { ws.close(); } catch { /* ignore */ }
  });
}

export function stopTether(): void {
  teardownWs();
  singleton.pendingRpcs.forEach(({ reject, timer }) => {
    clearTimeout(timer);
    reject(new Error("Tether stopped"));
  });
  singleton.pendingRpcs.clear();
  setState("idle", null, null);
}

export function subscribeHostFrames(
  cb: (frame: Record<string, unknown>) => void,
): () => void {
  singleton.hostFrameListeners.add(cb);
  return () => {
    singleton.hostFrameListeners.delete(cb);
  };
}

// ──────────────────────────────────────────────────────────────────
// RPC
// ──────────────────────────────────────────────────────────────────

const RPC_TIMEOUT_MS = 30_000;

export async function tetherRpc<TReq, TResp>(
  kind: string,
  req: TReq,
): Promise<TResp> {
  if (singleton.state !== "approved" || !singleton.sessionKey || !singleton.ws) {
    throw new Error(`Tether not approved (state: ${singleton.state})`);
  }

  const requestId = crypto.randomUUID();

  return new Promise<TResp>((resolve, reject) => {
    const timer = setTimeout(() => {
      singleton.pendingRpcs.delete(requestId);
      reject(new Error(`tetherRpc '${kind}' timed out after ${RPC_TIMEOUT_MS}ms`));
    }, RPC_TIMEOUT_MS);

    singleton.pendingRpcs.set(requestId, {
      resolve: resolve as (v: unknown) => void,
      reject,
      timer,
    });

    try {
      sendTetherFrame({ request_id: requestId, kind, ...(req as object) });
    } catch (err) {
      clearTimeout(timer);
      singleton.pendingRpcs.delete(requestId);
      reject(err instanceof Error ? err : new Error(String(err)));
    }
  });
}

export function sendTetherFrame(payload: Record<string, unknown>): void {
  if (singleton.state !== "approved" || !singleton.sessionKey || !singleton.ws) {
    throw new Error(`Tether not approved (state: ${singleton.state})`);
  }

  const plaintext = new TextEncoder().encode(JSON.stringify(payload));
  const nonce = deriveNonce(singleton.sessionKey, 0x01, singleton.txSeq);
  singleton.txSeq++;

  let ciphertext: Uint8Array;
  try {
    ciphertext = aeadEncrypt(plaintext, singleton.sessionKey, nonce);
  } catch (err) {
    throw new Error(`AEAD encrypt failed: ${String(err)}`);
  }

  const packed = new Uint8Array(nonce.length + ciphertext.length);
  packed.set(nonce, 0);
  packed.set(ciphertext, nonce.length);

  const frame = {
    type: "forward",
    session_id: singleton.sessionId,
    payload_b64: toBase64(packed),
  };

  try {
    singleton.ws.send(JSON.stringify(frame));
  } catch (err) {
    throw err instanceof Error ? err : new Error(String(err));
  }
}

// ──────────────────────────────────────────────────────────────────
// Internal — frame handler
// ──────────────────────────────────────────────────────────────────

function handleFrame(
  frame: Record<string, unknown>,
  ephemeralPrivkey: Uint8Array,
  machineName: string,
): void {
  const type = frame.type as string | undefined;

  switch (type) {
    case "pair_pending": {
      // Cloud relayed the host_hello from the desktop: complete DH.
      const hostPubB64 = frame.host_xd_pub_b64 as string | undefined;
      // Codex R2 fix — cloud emits `pair_pending.session_id`, not
      // `tether_session_id`. Pre-fix shape never derived the
      // session_key because this read returned undefined.
      const sessionId = frame.session_id as string | undefined;
      if (!hostPubB64 || !sessionId) break;

      const hostPub = fromBase64(hostPubB64);
      const sessionKey = deriveSessionKey(ephemeralPrivkey, hostPub, sessionId);

      // CRITICAL: zero out ephemeral privkey after DH — synthesis invariant.
      ephemeralPrivkey.fill(0);

      singleton.sessionKey = sessionKey;
      singleton.sessionId = sessionId;
      setState("pending_approval", machineName, sessionId);
      break;
    }

    case "tether_ready": {
      // Desktop approved (may be AEAD-wrapped or plaintext ACK; cloud spec
      // allows both; we accept the frame type as the approval signal and
      // the WS being established as authentication).
      setState("approved", machineName, singleton.sessionId);
      break;
    }

    case "tether_denied": {
      const reason = frame.reason as string | undefined;
      const nextState: TetherState =
        reason === "host_offline" ? "host_offline" : "denied";
      setState(nextState, machineName, singleton.sessionId);
      teardownWs();
      break;
    }

    case "forwarded_from_host": {
      // Decrypt and dispatch to pending RPC promise.
      if (!singleton.sessionKey) break;

      // CSO #1 + H7 fix — packed wire format `base64(nonce || ct)`. The
      // expected nonce is re-derived from (session, direction=0, rxSeq)
      // and asserted to match the packed nonce. Any mismatch (replay,
      // reorder, tamper) is rejected before AEAD; rxSeq increments
      // strictly monotonically so the cloud can't replay an earlier
      // frame to land on a stale request_id.
      const payloadB64 = frame.payload_b64 as string | undefined;
      if (!payloadB64) break;

      let plaintext: Uint8Array;
      try {
        const packed = fromBase64(payloadB64);
        if (packed.length < 24) {
          console.warn("[tether] forwarded_from_host: packed too short");
          break;
        }
        const nonce = packed.subarray(0, 24);
        const ct = packed.subarray(24);

        const expected = deriveNonce(singleton.sessionKey, 0x00, singleton.rxSeq);
        let matches = nonce.length === expected.length;
        if (matches) {
          for (let i = 0; i < expected.length; i++) {
            if (nonce[i] !== expected[i]) { matches = false; break; }
          }
        }
        if (!matches) {
          console.warn("[tether] nonce mismatch — replay or reorder; tearing down");
          teardownWs();
          setState("error", machineName, singleton.sessionId);
          break;
        }
        singleton.rxSeq++;

        plaintext = aeadDecrypt(ct, singleton.sessionKey, nonce);
      } catch (err) {
        console.error("[tether] AEAD decrypt failed on host frame:", err);
        break;
      }

      let parsed: Record<string, unknown>;
      try {
        parsed = JSON.parse(new TextDecoder().decode(plaintext)) as Record<string, unknown>;
      } catch {
        break;
      }

      const requestId = parsed.request_id as string | undefined;
      if (requestId) {
        const pending = singleton.pendingRpcs.get(requestId);
        if (pending) {
          clearTimeout(pending.timer);
          singleton.pendingRpcs.delete(requestId);

          if (parsed.error) {
            pending.reject(new Error(String(parsed.error)));
          } else {
            pending.resolve(parsed);
          }
          break;
        }
      }

      for (const listener of singleton.hostFrameListeners) {
        try {
          listener(parsed);
        } catch {
          // Listener errors must not break the shared transport.
        }
      }
      break;
    }

    default:
      // Unknown frame types are silently ignored.
      break;
  }
}

// ──────────────────────────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────────────────────────

function setState(
  state: TetherState,
  machineName: string | null,
  sessionId: string | null,
): void {
  singleton.state = state;
  singleton.machineName = machineName;
  singleton.sessionId = sessionId;
  const info = getTether();
  for (const l of singleton.listeners) {
    try { l(info); } catch { /* listener errors must not break the singleton */ }
  }
}

function teardownWs(): void {
  if (singleton.ws) {
    try { singleton.ws.close(); } catch { /* ignore */ }
    singleton.ws = null;
  }
  // Zero session key on teardown.
  if (singleton.sessionKey) {
    singleton.sessionKey.fill(0);
    singleton.sessionKey = null;
  }
  singleton.txSeq = 0n;
  singleton.rxSeq = 0n;
}

// (CSO #1 fix — old buildNonce removed; nonces are now HKDF-derived in
// crypto.ts to match the Rust host's wire format.)

/**
 * SHA-256 of the raw userAgent string, returned as lowercase hex.
 * Stored in the tether_sessions cloud table as browser_user_agent_hash.
 */
async function computeUaHash(): Promise<string> {
  const ua = navigator.userAgent;
  const encoded = new TextEncoder().encode(ua);
  // @stablelib SHA256 is synchronous; use it to avoid the async WebCrypto
  // path and keep the function testable without mocking SubtleCrypto.
  const hash = new SHA256();
  hash.update(encoded);
  const digest = hash.digest();
  return Array.from(digest)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}
