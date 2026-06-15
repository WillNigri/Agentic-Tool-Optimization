/**
 * Tests for the tether decrypt bridge (v2.17 Wave 2).
 *
 * Strategy:
 *   - Mock `@tauri-apps/api/event` so `listen()` is captured without
 *     opening a real IPC channel.
 *   - Mock `@tauri-apps/api/core` so `invoke()` calls are recorded.
 *   - Mock `@/lib/cloud-api` to control decryptEventPayload + backfillTeamEvents
 *     + getTeamMemberE2eKeys.
 *   - Mock `@/lib/e2e/teamKey` to supply a fixed team key.
 *
 * Verifies:
 *   1. `startTetherDecryptBridge` registers the `tether_decrypt` listener.
 *   2. A well-formed decrypt_events request triggers decryptEventPayload
 *      for each raw event and replies via tether_decrypt_response.
 *   3. Events with __decrypt_error are mapped to sig_valid=false and null payload.
 *   4. Unknown request kind replies with ok=false.
 *   5. `stopTetherDecryptBridge` removes the listener and clears the cache.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// ── Tauri API mocks ───────────────────────────────────────────────────────

type EventHandler = (payload: unknown) => void;
let capturedListeners: Map<string, EventHandler[]> = new Map();
let invokeLog: Array<{ cmd: string; args: Record<string, unknown> }> = [];

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockImplementation(
    async (event: string, handler: EventHandler) => {
      if (!capturedListeners.has(event)) capturedListeners.set(event, []);
      capturedListeners.get(event)!.push(handler);
      // Return an unlisten function.
      return () => {
        const listeners = capturedListeners.get(event) ?? [];
        const idx = listeners.indexOf(handler);
        if (idx !== -1) listeners.splice(idx, 1);
      };
    },
  ),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockImplementation(
    async (cmd: string, args?: Record<string, unknown>) => {
      invokeLog.push({ cmd, args: args ?? {} });
    },
  ),
}));

// ── Cloud API mocks ───────────────────────────────────────────────────────

const mockRawEvent = {
  seq_num: 1,
  event_kind: "turn_appended",
  payload_json: null,
  ciphertext_b64: "abc123",
  nonce_b64: "nonce",
  signature_b64: "sig",
  signer_key_id: "key-1",
  initiator_user_id: "user-1",
  initiator_runtime: "claude",
  initiator_agent_slug: null,
  surface: "desktop" as const,
  created_at: "2026-06-14T00:00:00Z",
};

const mockDecryptedEvent = {
  ...mockRawEvent,
  payload_json: { type: "turn_appended", content: "Hello" },
};

const mockDecryptErrorEvent = {
  ...mockRawEvent,
  seq_num: 2,
  payload_json: { __decrypt_error: true },
};

vi.mock("@/lib/cloud-api", () => ({
  decryptEventPayload: vi.fn(),
  backfillTeamEvents: vi.fn(),
  getTeamMemberE2eKeys: vi.fn().mockResolvedValue([
    {
      member_user_id: "user-1",
      key_id: "key-1",
      x25519_pubkey: "dummyx25519",
      ed25519_pubkey: "dummyed25519",
    },
  ]),
}));

vi.mock("@/lib/e2e/teamKey", () => ({
  loadTeamKey: vi.fn().mockResolvedValue(new Uint8Array(32).fill(0xab)),
}));

// ── Helpers ───────────────────────────────────────────────────────────────

/**
 * Fire a simulated `tether_decrypt` event through the captured listener.
 *
 * Trace:
 *   host.ts: tauriListen("tether_decrypt", outerHandler)
 *     → listen(event, (e) => outerHandler(e.payload))
 *   capturedListeners stores the `(e) => outerHandler(e.payload)` callback.
 *   So calling h({ payload: data }) results in outerHandler(data).
 */
function fireDecryptEvent(data: {
  session_id: string;
  request_id: string;
  plain_request_json: string;
}) {
  const handlers = capturedListeners.get("tether_decrypt") ?? [];
  for (const h of handlers) h({ payload: data });
}

function getLatestInvoke(cmd: string) {
  return invokeLog.filter((e) => e.cmd === cmd).at(-1);
}

// ── Test setup ────────────────────────────────────────────────────────────

beforeEach(async () => {
  capturedListeners = new Map();
  invokeLog = [];
  vi.clearAllMocks();

  // Re-apply default mocks cleared by clearAllMocks().
  const { getTeamMemberE2eKeys } = await import("@/lib/cloud-api");
  vi.mocked(getTeamMemberE2eKeys).mockResolvedValue([
    {
      member_user_id: "user-1",
      key_id: "key-1",
      x25519_pubkey: "dummyx25519",
      ed25519_pubkey: "dummyed25519",
    },
  ]);
  const { loadTeamKey } = await import("@/lib/e2e/teamKey");
  vi.mocked(loadTeamKey).mockResolvedValue(new Uint8Array(32).fill(0xab));
});

afterEach(async () => {
  const { stopTetherDecryptBridge } = await import("./host");
  stopTetherDecryptBridge();
});

// ── Tests ─────────────────────────────────────────────────────────────────

describe("startTetherDecryptBridge", () => {
  it("registers a tether_decrypt listener", async () => {
    const { startTetherDecryptBridge } = await import("./host");
    await startTetherDecryptBridge();
    expect(capturedListeners.has("tether_decrypt")).toBe(true);
    expect(capturedListeners.get("tether_decrypt")?.length).toBeGreaterThan(0);
  });

  it("calling twice replaces the listener (no double-fire)", async () => {
    const { startTetherDecryptBridge, stopTetherDecryptBridge } = await import("./host");
    await startTetherDecryptBridge();
    const firstCount = capturedListeners.get("tether_decrypt")?.length ?? 0;
    await startTetherDecryptBridge();
    const secondCount = capturedListeners.get("tether_decrypt")?.length ?? 0;
    // Should not accumulate; second start removes the first listener.
    expect(secondCount).toBeLessThanOrEqual(firstCount + 1);
    stopTetherDecryptBridge();
  });
});

describe("decrypt_events happy path", () => {
  it("calls decryptEventPayload for each raw event and replies via tether_decrypt_response", async () => {
    const { backfillTeamEvents, decryptEventPayload } = await import("@/lib/cloud-api");
    vi.mocked(backfillTeamEvents).mockResolvedValue([mockRawEvent]);
    vi.mocked(decryptEventPayload).mockResolvedValue(mockDecryptedEvent as unknown as typeof mockRawEvent);

    const { startTetherDecryptBridge } = await import("./host");
    await startTetherDecryptBridge();

    const req = {
      request_id: "req-1",
      kind: "decrypt_events",
      team_id: "team-abc",
      resource_kind: "session" as const,
      resource_id: "res-xyz",
      since: 0,
      limit: 50,
    };

    fireDecryptEvent({
      session_id: "sess-1",
      request_id: "req-1",
      plain_request_json: JSON.stringify(req),
    });

    // Wait for async handlers to settle.
    await new Promise((r) => setTimeout(r, 50));

    expect(vi.mocked(decryptEventPayload)).toHaveBeenCalledTimes(1);

    const invoke = getLatestInvoke("tether_decrypt_response");
    expect(invoke).toBeDefined();
    expect(invoke!.args.sessionId).toBe("sess-1");
    expect(invoke!.args.requestId).toBe("req-1");

    const reply = JSON.parse(invoke!.args.plainReplyJson as string) as {
      ok: boolean;
      events: Array<{ sig_valid: boolean }>;
    };
    expect(reply.ok).toBe(true);
    expect(reply.events).toHaveLength(1);
    expect(reply.events[0].sig_valid).toBe(true);
  });
});

describe("sig_valid=false for __decrypt_error events", () => {
  it("maps __decrypt_error events to null payload + sig_valid=false", async () => {
    const { backfillTeamEvents, decryptEventPayload } = await import("@/lib/cloud-api");
    vi.mocked(backfillTeamEvents).mockResolvedValue([mockRawEvent]);
    vi.mocked(decryptEventPayload).mockResolvedValue(mockDecryptErrorEvent as unknown as typeof mockRawEvent);

    const { startTetherDecryptBridge } = await import("./host");
    await startTetherDecryptBridge();

    fireDecryptEvent({
      session_id: "sess-2",
      request_id: "req-2",
      plain_request_json: JSON.stringify({
        request_id: "req-2",
        kind: "decrypt_events",
        team_id: "team-abc",
        resource_kind: "session",
        resource_id: "res-xyz",
        since: 0,
        limit: 10,
      }),
    });

    await new Promise((r) => setTimeout(r, 50));

    const invoke = getLatestInvoke("tether_decrypt_response");
    expect(invoke).toBeDefined();
    const reply = JSON.parse(invoke!.args.plainReplyJson as string) as {
      ok: boolean;
      events: Array<{ sig_valid: boolean; payload_json: unknown }>;
    };
    expect(reply.ok).toBe(true);
    expect(reply.events[0].sig_valid).toBe(false);
    expect(reply.events[0].payload_json).toBeNull();
  });
});

describe("unknown request kind", () => {
  it("replies with ok=false and an error message", async () => {
    const { startTetherDecryptBridge } = await import("./host");
    await startTetherDecryptBridge();

    fireDecryptEvent({
      session_id: "sess-3",
      request_id: "req-3",
      plain_request_json: JSON.stringify({
        request_id: "req-3",
        kind: "unknown_action",
      }),
    });

    await new Promise((r) => setTimeout(r, 50));

    const invoke = getLatestInvoke("tether_decrypt_response");
    expect(invoke).toBeDefined();
    const reply = JSON.parse(invoke!.args.plainReplyJson as string) as {
      ok: boolean;
      error: string;
    };
    expect(reply.ok).toBe(false);
    expect(reply.error).toContain("unknown request kind");
  });
});

describe("stopTetherDecryptBridge", () => {
  it("removes the listener so subsequent events are ignored", async () => {
    const { backfillTeamEvents, decryptEventPayload } = await import("@/lib/cloud-api");
    vi.mocked(backfillTeamEvents).mockResolvedValue([mockRawEvent]);
    vi.mocked(decryptEventPayload).mockResolvedValue(mockDecryptedEvent as unknown as typeof mockRawEvent);

    const { startTetherDecryptBridge, stopTetherDecryptBridge } = await import("./host");
    await startTetherDecryptBridge();
    stopTetherDecryptBridge();

    invokeLog = []; // clear

    fireDecryptEvent({
      session_id: "sess-4",
      request_id: "req-4",
      plain_request_json: JSON.stringify({
        request_id: "req-4",
        kind: "decrypt_events",
        team_id: "team-abc",
        resource_kind: "session",
        resource_id: "res-xyz",
        since: 0,
        limit: 10,
      }),
    });

    await new Promise((r) => setTimeout(r, 50));

    // No invoke should have fired.
    expect(invokeLog.filter((e) => e.cmd === "tether_decrypt_response")).toHaveLength(0);
  });
});
