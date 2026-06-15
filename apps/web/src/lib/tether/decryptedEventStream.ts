// v2.17 Wave 3 — decrypted event stream via the desktop tether.
//
// The browser equivalent of the v2.16 teamEventStream.ts but instead of
// subscribing to the cloud WS directly (which carries only ciphertext for
// e2e shares), it RPCs through the tether to the desktop. The desktop
// holds the Team Key, decrypts with full v2.15 sig-verify, and returns
// plaintext events + a sig_valid bit.
//
// TODO (Wave 3.x): replace polling with desktop-push. The desktop already
// listens on the cloud event WS; route new events into a tether "push"
// frame type so the browser gets them in ~realtime without polling.

import { tetherRpc } from "./client";
import type { SharedResourceKind } from "../api";

// ──────────────────────────────────────────────────────────────────
// Types
// ──────────────────────────────────────────────────────────────────

/**
 * A decrypted team event as returned by the desktop tether.
 *
 * The payload_json here is already in plaintext (decrypted + sig-verified
 * on the desktop). sig_valid = false means the desktop detected a
 * signature mismatch; the browser MUST NOT render the body as content.
 */
export interface TeamEventDecrypted {
  seq_num: number;
  event_kind: string;
  payload_json: unknown | null;
  sig_valid: boolean;
  initiator_user_id: string | null;
  initiator_runtime: string | null;
  initiator_agent_slug: string | null;
  surface: "desktop" | "cli" | "web" | "mcp" | "cron";
  created_at: string;
}

type Listener = (event: TeamEventDecrypted) => void;

interface SubKey {
  teamId: string;
  resourceKind: SharedResourceKind;
  resourceId: string;
}

interface Subscription {
  key: string;
  listeners: Set<Listener>;
  seenSeqs: Set<number>;
  lastSeq: number;
  pollTimer: ReturnType<typeof setTimeout> | null;
  stopped: boolean;
}

const POLL_INTERVAL_MS = 5_000;
const POLL_LIMIT = 200;

// ──────────────────────────────────────────────────────────────────
// Singleton subscription map
// ──────────────────────────────────────────────────────────────────

const subs = new Map<string, Subscription>();

function keyOf({ teamId, resourceKind, resourceId }: SubKey): string {
  return `${teamId}|${resourceKind}|${resourceId}`;
}

// ──────────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────────

/**
 * Subscribe to decrypted events for a shared resource via the tether.
 *
 * @param k          Team + resource identifiers.
 * @param since      Start from this seq_num (inclusive). Use detail.last_seq
 *                   to backfill the snapshot window on mount.
 * @param listener   Called for each new event. May be called with
 *                   sig_valid=false — caller must redact the body.
 * @returns          Unsubscribe function. Call on component unmount.
 */
export function subscribeDecryptedEvents(
  k: SubKey,
  since: number,
  listener: Listener,
): () => void {
  const key = keyOf(k);
  let sub = subs.get(key);
  if (!sub) {
    sub = {
      key,
      listeners: new Set(),
      seenSeqs: new Set(),
      lastSeq: since,
      pollTimer: null,
      stopped: false,
    };
    subs.set(key, sub);
    schedulePoll(sub, k, /* immediate */ true);
  }
  sub.listeners.add(listener);

  return () => {
    sub!.listeners.delete(listener);
    if (sub!.listeners.size === 0) stopSub(sub!);
  };
}

// ──────────────────────────────────────────────────────────────────
// Polling machinery
// ──────────────────────────────────────────────────────────────────

function schedulePoll(sub: Subscription, k: SubKey, immediate: boolean): void {
  if (sub.stopped) return;
  if (sub.pollTimer) clearTimeout(sub.pollTimer);
  sub.pollTimer = setTimeout(
    () => { void poll(sub, k); },
    immediate ? 0 : POLL_INTERVAL_MS,
  );
}

async function poll(sub: Subscription, k: SubKey): Promise<void> {
  if (sub.stopped || sub.listeners.size === 0) return;

  try {
    const resp = await tetherRpc<
      {
        team_id: string;
        resource_kind: SharedResourceKind;
        resource_id: string;
        since: number;
        limit: number;
      },
      { events: TeamEventDecrypted[] }
    >("decrypt_events", {
      team_id: k.teamId,
      resource_kind: k.resourceKind,
      resource_id: k.resourceId,
      since: sub.lastSeq,
      limit: POLL_LIMIT,
    });

    if (sub.stopped) return;

    const newEvents = resp.events ?? [];
    for (const ev of newEvents) {
      if (sub.seenSeqs.has(ev.seq_num)) continue;
      sub.seenSeqs.add(ev.seq_num);
      if (ev.seq_num > sub.lastSeq) sub.lastSeq = ev.seq_num;

      for (const l of sub.listeners) {
        try { l(ev); } catch { /* listener errors must not break the stream */ }
      }
    }
  } catch {
    // Tether not approved or RPC error — retry on next interval. Caller
    // observes tether state separately and can show an appropriate UI.
  }

  schedulePoll(sub, k, /* immediate */ false);
}

function stopSub(sub: Subscription): void {
  sub.stopped = true;
  if (sub.pollTimer) {
    clearTimeout(sub.pollTimer);
    sub.pollTimer = null;
  }
  subs.delete(sub.key);
}
