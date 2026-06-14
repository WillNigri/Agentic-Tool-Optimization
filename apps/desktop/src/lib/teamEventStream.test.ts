// Unit tests for the TeamEventStream singleton transport.
//
// Strategy: mock the WebSocket global so tests don't open real
// connections. We intercept `new WebSocket(url)` and return a
// controllable fake that exposes fire() helpers for open/message/close.
//
// Also mocks:
//  • getStoredTokens → returns a fake access token.
//  • fetch → returns a fake mesh-presence-token response.

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

// ── WebSocket mock ────────────────────────────────────────────────────

type WsEventName = "open" | "message" | "close" | "error";

class MockWebSocket {
  static instances: MockWebSocket[] = [];
  readyState: number = 0; // CONNECTING
  url: string;
  private handlers: Map<WsEventName, EventListenerOrEventListenerObject[]> = new Map();

  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
  }

  addEventListener(event: WsEventName, handler: EventListenerOrEventListenerObject) {
    if (!this.handlers.has(event)) this.handlers.set(event, []);
    this.handlers.get(event)!.push(handler);
  }

  close() {
    this.readyState = 3; // CLOSED
    this._fire("close", {});
  }

  send(_data: string) {
    // no-op in tests
  }

  /** Simulate the WS reaching OPEN state. */
  _fireOpen() {
    this.readyState = 1; // OPEN
    this._fire("open", {});
  }

  /** Simulate a JSON message arriving. */
  _fireMessage(data: unknown) {
    this._fire("message", { data: JSON.stringify(data) });
  }

  /** Simulate the WS closing. */
  _fireClose() {
    this.readyState = 3;
    this._fire("close", {});
  }

  private _fire(event: WsEventName, payload: unknown) {
    for (const h of this.handlers.get(event) ?? []) {
      if (typeof h === "function") {
        (h as (e: unknown) => void)(payload);
      } else {
        (h as EventListenerObject).handleEvent(payload as Event);
      }
    }
  }
}

// Patch global WebSocket and fetch before importing the module under test.
vi.stubGlobal("WebSocket", MockWebSocket);
vi.stubGlobal("WebSocket", Object.assign(MockWebSocket, {
  CONNECTING: 0,
  OPEN: 1,
  CLOSING: 2,
  CLOSED: 3,
}));

// Mock getStoredTokens → always returns a fake access token.
vi.mock("./cloud-api", () => ({
  getStoredTokens: () => ({ accessToken: "fake-access-token", refreshToken: "r" }),
}));

// Mock fetch → returns a valid presence token response.
const FAKE_TOKEN = "mst_fake_token_abc123";
const FAKE_EXPIRES = new Date(Date.now() + 15 * 60 * 1000).toISOString();

vi.stubGlobal(
  "fetch",
  vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({
      success: true,
      data: { token: FAKE_TOKEN, peer_id: "peer-1", expires_at: FAKE_EXPIRES },
    }),
  }),
);

// ── Module under test ──────────────────────────────────────────────────

// Import AFTER patching globals.
import { teamEventStream } from "./teamEventStream";
import type { TeamEvent } from "./cloud-api";

function fakeEvent(seq_num: number, overrides: Partial<TeamEvent> = {}): TeamEvent {
  return {
    seq_num,
    event_kind: "turn_appended",
    payload_json: { role: "user", text: `msg ${seq_num}` },
    ciphertext_b64: null,
    nonce_b64: null,
    signature_b64: null,
    signer_key_id: null,
    initiator_user_id: null,
    initiator_runtime: "human",
    initiator_agent_slug: null,
    surface: "desktop",
    created_at: new Date().toISOString(),
    ...overrides,
  };
}

// Helper: wait one microtask tick so async fetch → open logic settles.
function tick() {
  return new Promise<void>((r) => setTimeout(r, 0));
}

describe("TeamEventStream", () => {
  beforeEach(() => {
    MockWebSocket.instances.length = 0;
    vi.mocked(fetch).mockClear();
  });

  afterEach(() => {
    // Nothing to clean up — the singleton resets its connection map on
    // each closeIntentionally call (triggered by the unsubscribes).
  });

  it("delivers event to subscriber on listener invocation", async () => {
    const received: TeamEvent[] = [];
    const unsub = teamEventStream.subscribe(
      "team-1", "session", "res-1", 0,
      (e) => received.push(e),
    );
    await tick();

    const ws = MockWebSocket.instances.at(-1)!;
    ws._fireOpen();
    ws._fireMessage(fakeEvent(1));

    expect(received).toHaveLength(1);
    expect(received[0].seq_num).toBe(1);
    unsub();
  });

  it("subscribing twice to same tuple opens only one WS connection", async () => {
    const unsub1 = teamEventStream.subscribe("team-2", "session", "res-2", 0, () => {});
    await tick();
    const countAfterFirst = MockWebSocket.instances.length;

    const unsub2 = teamEventStream.subscribe("team-2", "session", "res-2", 0, () => {});
    await tick();

    expect(MockWebSocket.instances.length).toBe(countAfterFirst);
    unsub1();
    unsub2();
  });

  it("unsubscribing last subscriber closes the WS", async () => {
    const unsub = teamEventStream.subscribe("team-3", "loop", "res-3", 0, () => {});
    await tick();

    const ws = MockWebSocket.instances.at(-1)!;
    ws._fireOpen();
    expect(ws.readyState).toBe(1); // OPEN

    unsub();
    // After unsub the close() path sets readyState to CLOSED (3).
    expect(ws.readyState).toBe(3);
  });

  it("deduplicates events with the same seq_num", async () => {
    const received: TeamEvent[] = [];
    const unsub = teamEventStream.subscribe(
      "team-4", "chat", "res-4", 0,
      (e) => received.push(e),
    );
    await tick();

    const ws = MockWebSocket.instances.at(-1)!;
    ws._fireOpen();
    ws._fireMessage(fakeEvent(5));
    ws._fireMessage(fakeEvent(5)); // duplicate

    expect(received).toHaveLength(1);
    unsub();
  });

  it("reconnects with ?since=<last_seen_seq> after close", async () => {
    // Track the WS count before subscribing so we can wait for a new one.
    const startCount = MockWebSocket.instances.length;

    // Subscribe and let the first WS open.
    const unsub = teamEventStream.subscribe(
      "team-5", "mission", "res-5", 0,
      () => {},
    );
    // Wait for the fetch → new WebSocket chain to complete.
    await new Promise<void>((r) => setTimeout(r, 50));

    const ws1 = MockWebSocket.instances.at(-1)!;
    expect(MockWebSocket.instances.length).toBeGreaterThan(startCount);
    ws1._fireOpen();
    ws1._fireMessage(fakeEvent(7));

    // Close triggers scheduleReconnect (1s delay). Since we're using
    // real timers, we wait for the reconnect to fire and the new WS
    // to be constructed (max 2.5s budget well inside the 5s default).
    ws1._fireClose();

    // Poll until a second WS appears (reconnect fired + token fetched).
    const deadline = Date.now() + 2_500;
    while (MockWebSocket.instances.length <= startCount + 1 && Date.now() < deadline) {
      await new Promise<void>((r) => setTimeout(r, 50));
    }

    const ws2 = MockWebSocket.instances.at(-1)!;
    // The reconnect URL must carry since=7 (the last delivered seq_num).
    expect(ws2.url).toContain("since=7");
    expect(ws2).not.toBe(ws1);

    unsub();
  }, 8_000);
});
