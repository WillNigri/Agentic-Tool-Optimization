/**
 * Tests for the v2.15 Wave 3 E2E-encrypted event append functions.
 *
 * Covers deliverable H: appendTeamEventEncrypted does reserve → encrypt → commit
 * in order; AAD includes seq_num; getShareEncryptionMode reads from the detail response.
 */

import { describe, it, expect, beforeAll, vi, beforeEach } from "vitest";

// ── Sodium setup ─────────────────────────────────────────────────────────────

import {
  ensureSodiumReady,
  generateTeamKey,
  generateE2eKeypair,
  decryptPayload,
  verifyMessage,
  fromBase64,
} from "@/lib/e2e/crypto";

beforeAll(async () => {
  await ensureSodiumReady();
});

// ── Fetch mock ────────────────────────────────────────────────────────────────
// We need fine-grained control per test to verify call order.

let fetchCalls: Array<{ url: string; options: RequestInit }> = [];
let fetchResponses: Array<{ ok: boolean; body: unknown }> = [];

vi.stubGlobal(
  "fetch",
  vi.fn().mockImplementation(async (url: string, options: RequestInit) => {
    fetchCalls.push({ url, options });
    const response = fetchResponses.shift();
    if (!response) {
      return { ok: false, json: async () => ({ success: false, error: { code: "NO_MOCK", message: "No mock response" } }) };
    }
    return {
      ok: response.ok,
      json: async () => response.body,
    };
  }),
);

// Mock localStorage so getStoredTokens() returns a fake token.
vi.stubGlobal("localStorage", {
  getItem: (key: string) => {
    if (key === "ato_cloud_tokens") {
      return JSON.stringify({ accessToken: "test-at", refreshToken: "test-rt" });
    }
    return null;
  },
  setItem: () => {},
  removeItem: () => {},
});

// We import AFTER stubbing globals.
import {
  appendTeamEventEncrypted,
  getShareEncryptionMode,
} from "@/lib/cloud-api";

// ── Helpers ───────────────────────────────────────────────────────────────────

function apiOk(data: unknown) {
  return { ok: true, body: { success: true, data } };
}

beforeEach(() => {
  fetchCalls = [];
  fetchResponses = [];
});

// ── appendTeamEventEncrypted ──────────────────────────────────────────────────

describe("appendTeamEventEncrypted", () => {
  it("calls reserve then commit (two POST requests in order)", async () => {
    const teamKey = await generateTeamKey();
    const kp = await generateE2eKeypair();

    // Mock: reserve returns seq_num=7.
    fetchResponses.push(apiOk({ seq_num: 7 }));
    // Mock: commit succeeds.
    fetchResponses.push(apiOk({ seq_num: 7, created_at: "2026-06-14T00:00:00Z" }));

    await appendTeamEventEncrypted(
      "team-1",
      "session",
      "res-1",
      "turn_appended",
      { role: "user", text: "hello" },
      "desktop",
      teamKey,
      kp.ed25519PrivateKey,
      "key-id-1",
    );

    expect(fetchCalls).toHaveLength(2);
    // First call: reserve
    expect(fetchCalls[0].url).toContain("/events/reserve");
    expect(fetchCalls[0].options.method).toBe("POST");
    // Second call: commit
    expect(fetchCalls[1].url).toContain("/events");
    expect(fetchCalls[1].url).not.toContain("/events/reserve");
    expect(fetchCalls[1].options.method).toBe("POST");
  });

  it("includes seq_num in the commit body", async () => {
    const teamKey = await generateTeamKey();
    const kp = await generateE2eKeypair();

    fetchResponses.push(apiOk({ seq_num: 42 }));
    fetchResponses.push(apiOk({ seq_num: 42, created_at: "2026-06-14T00:00:00Z" }));

    await appendTeamEventEncrypted(
      "team-2", "session", "res-2", "turn_appended",
      { text: "payload" }, "desktop",
      teamKey, kp.ed25519PrivateKey, "key-2",
    );

    const commitBody = JSON.parse(fetchCalls[1].options.body as string) as {
      seq_num: number;
      ciphertext_b64: string;
      nonce_b64: string;
      signature_b64: string;
      signer_key_id: string;
    };
    expect(commitBody.seq_num).toBe(42);
    expect(typeof commitBody.ciphertext_b64).toBe("string");
    expect(typeof commitBody.nonce_b64).toBe("string");
    expect(typeof commitBody.signature_b64).toBe("string");
    expect(commitBody.signer_key_id).toBe("key-2");
  });

  it("AAD includes the seq_num (AEAD decrypts only with correct AD)", async () => {
    const teamKey = await generateTeamKey();
    const kp = await generateE2eKeypair();
    const TEAM_ID = "team-aad";
    const RES_ID = "res-aad";
    const SEQ = 17;
    const EVENT_KIND = "turn_appended";
    const PAYLOAD = { role: "user", text: "aad test" };

    fetchResponses.push(apiOk({ seq_num: SEQ }));
    fetchResponses.push(apiOk({ seq_num: SEQ, created_at: "2026-06-14T00:00:00Z" }));

    await appendTeamEventEncrypted(
      TEAM_ID, "session", RES_ID, EVENT_KIND,
      PAYLOAD, "desktop",
      teamKey, kp.ed25519PrivateKey, "key-aad",
    );

    const commitBody = JSON.parse(fetchCalls[1].options.body as string) as {
      ciphertext_b64: string;
      nonce_b64: string;
    };

    const ciphertext = fromBase64(commitBody.ciphertext_b64);
    const nonce = fromBase64(commitBody.nonce_b64);

    // Reconstruct the correct AD.
    const correctAd = new TextEncoder().encode(
      `${TEAM_ID}|${RES_ID}|${SEQ}|${EVENT_KIND}`,
    );

    // Decrypt with correct AD should succeed.
    const plaintext = await decryptPayload(ciphertext, nonce, teamKey, correctAd);
    const recovered = JSON.parse(new TextDecoder().decode(plaintext)) as typeof PAYLOAD;
    expect(recovered).toEqual(PAYLOAD);

    // Decrypt with WRONG seq_num in AD should fail.
    const wrongAd = new TextEncoder().encode(
      `${TEAM_ID}|${RES_ID}|${SEQ + 1}|${EVENT_KIND}`,
    );
    await expect(decryptPayload(ciphertext, nonce, teamKey, wrongAd)).rejects.toThrow();
  });

  it("signature covers ciphertext || nonce || AD", async () => {
    const teamKey = await generateTeamKey();
    const kp = await generateE2eKeypair();
    const TEAM_ID = "team-sig";
    const RES_ID = "res-sig";
    const SEQ = 3;
    const EVENT_KIND = "turn_appended";

    fetchResponses.push(apiOk({ seq_num: SEQ }));
    fetchResponses.push(apiOk({ seq_num: SEQ, created_at: "2026-06-14T00:00:00Z" }));

    await appendTeamEventEncrypted(
      TEAM_ID, "session", RES_ID, EVENT_KIND,
      { text: "sig test" }, "desktop",
      teamKey, kp.ed25519PrivateKey, "key-sig",
    );

    const commitBody = JSON.parse(fetchCalls[1].options.body as string) as {
      ciphertext_b64: string;
      nonce_b64: string;
      signature_b64: string;
    };

    const ciphertext = fromBase64(commitBody.ciphertext_b64);
    const nonce = fromBase64(commitBody.nonce_b64);
    const signature = fromBase64(commitBody.signature_b64);
    const adBytes = new TextEncoder().encode(`${TEAM_ID}|${RES_ID}|${SEQ}|${EVENT_KIND}`);

    // Reconstruct the signed message = ciphertext || nonce || AD.
    const signedMsg = new Uint8Array(ciphertext.length + nonce.length + adBytes.length);
    signedMsg.set(ciphertext, 0);
    signedMsg.set(nonce, ciphertext.length);
    signedMsg.set(adBytes, ciphertext.length + nonce.length);

    const valid = await verifyMessage(signedMsg, signature, kp.ed25519PublicKey);
    expect(valid).toBe(true);
  });
});

// ── getShareEncryptionMode ────────────────────────────────────────────────────

describe("getShareEncryptionMode", () => {
  it("returns 'e2e' when the detail response has encryption_mode=e2e", async () => {
    fetchResponses.push(
      apiOk({ encryption_mode: "e2e", team_key_id: "tk-1", last_seq: 0 }),
    );
    const mode = await getShareEncryptionMode("team-1", "session", "res-1");
    expect(mode).toBe("e2e");
  });

  it("returns 'plaintext' when encryption_mode is absent", async () => {
    fetchResponses.push(apiOk({ last_seq: 0 }));
    const mode = await getShareEncryptionMode("team-1", "session", "res-2");
    expect(mode).toBe("plaintext");
  });

  it("returns 'plaintext' when encryption_mode='plaintext'", async () => {
    fetchResponses.push(apiOk({ encryption_mode: "plaintext", last_seq: 0 }));
    const mode = await getShareEncryptionMode("team-1", "session", "res-3");
    expect(mode).toBe("plaintext");
  });
});
