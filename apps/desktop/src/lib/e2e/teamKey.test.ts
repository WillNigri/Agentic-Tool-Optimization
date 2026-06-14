/**
 * Unit tests for the Team Key cache module.
 * The OS keychain and cloud API are mocked so these tests run in CI.
 */

import { describe, it, expect, vi, beforeEach, beforeAll } from "vitest";
import { ensureSodiumReady } from "./crypto";

// ── Set up mocks BEFORE importing the module under test ───────────────────

// Mock the keychain bridge so no real OS keychain is touched.
vi.mock("./keychain", () => ({
  loadE2eKeypair: vi.fn(),
}));

// Mock the cloud-api getTeamKeyEnvelope call.
vi.mock("@/lib/cloud-api", () => ({
  getTeamKeyEnvelope: vi.fn(),
}));

// ── Imports (after mocks are registered) ─────────────────────────────────

import { loadTeamKey, clearTeamKeyCache, TeamKeyUnsealError } from "./teamKey";
import { loadE2eKeypair } from "./keychain";
import { getTeamKeyEnvelope } from "@/lib/cloud-api";
import {
  generateE2eKeypair,
  generateTeamKey,
  sealTeamKey,
  toBase64,
} from "./crypto";

const mockLoadE2eKeypair = vi.mocked(loadE2eKeypair);
const mockGetTeamKeyEnvelope = vi.mocked(getTeamKeyEnvelope);

// ── Helper: build a valid sealed-envelope fixture ─────────────────────────

let aliceKeypair: Awaited<ReturnType<typeof generateE2eKeypair>>;
let aliceTeamKey: Uint8Array;
let validEnvelope: { team_key_id: string; sealed_key: string; sealed_by_user_id: string; created_at: string };

beforeAll(async () => {
  await ensureSodiumReady();

  aliceKeypair = await generateE2eKeypair();
  aliceTeamKey = await generateTeamKey();

  const sealedBytes = await sealTeamKey(aliceTeamKey, aliceKeypair.x25519PublicKey);

  validEnvelope = {
    team_key_id: "tk_test_001",
    sealed_key: toBase64(sealedBytes),
    sealed_by_user_id: "admin_user_1",
    created_at: new Date().toISOString(),
  };
});

beforeEach(() => {
  clearTeamKeyCache();
  vi.clearAllMocks();
});

// ── Tests ─────────────────────────────────────────────────────────────────

describe("loadTeamKey", () => {
  it("returns the correct Team Key on first call", async () => {
    mockGetTeamKeyEnvelope.mockResolvedValue(validEnvelope);
    mockLoadE2eKeypair.mockResolvedValue({
      x25519PrivateKey: aliceKeypair.x25519PrivateKey,
      ed25519PrivateKey: aliceKeypair.ed25519PrivateKey,
    });

    const teamKey = await loadTeamKey("tk_test_001");
    // Compare via base64 to avoid cross-realm Uint8Array toEqual issues in jsdom.
    expect(toBase64(teamKey)).toBe(toBase64(aliceTeamKey));
    expect(teamKey.byteLength).toBe(32);
  });

  it("returns the cached key on second call (no extra API round-trip)", async () => {
    mockGetTeamKeyEnvelope.mockResolvedValue(validEnvelope);
    mockLoadE2eKeypair.mockResolvedValue({
      x25519PrivateKey: aliceKeypair.x25519PrivateKey,
      ed25519PrivateKey: aliceKeypair.ed25519PrivateKey,
    });

    await loadTeamKey("tk_test_001"); // first call — populates cache
    await loadTeamKey("tk_test_001"); // second call — cache hit

    // Cloud API and keychain each called exactly once.
    expect(mockGetTeamKeyEnvelope).toHaveBeenCalledTimes(1);
    expect(mockLoadE2eKeypair).toHaveBeenCalledTimes(1);
  });

  it("throws TeamKeyUnsealError when the sealed envelope is corrupt / wrong key", async () => {
    // Generate a DIFFERENT keypair so unseal fails.
    const wrongKeypair = await generateE2eKeypair();

    // The envelope is sealed to aliceKeypair, but we return wrongKeypair from keychain.
    mockGetTeamKeyEnvelope.mockResolvedValue(validEnvelope);
    mockLoadE2eKeypair.mockResolvedValue({
      x25519PrivateKey: wrongKeypair.x25519PrivateKey,
      ed25519PrivateKey: wrongKeypair.ed25519PrivateKey,
    });

    await expect(loadTeamKey("tk_test_001")).rejects.toBeInstanceOf(TeamKeyUnsealError);
  });

  it("throws TeamKeyUnsealError for a malformed (non-base64) sealed_key", async () => {
    mockGetTeamKeyEnvelope.mockResolvedValue({
      ...validEnvelope,
      sealed_key: "THIS IS NOT BASE64 !!!",
    });
    mockLoadE2eKeypair.mockResolvedValue({
      x25519PrivateKey: aliceKeypair.x25519PrivateKey,
      ed25519PrivateKey: aliceKeypair.ed25519PrivateKey,
    });

    await expect(loadTeamKey("tk_test_001")).rejects.toBeInstanceOf(TeamKeyUnsealError);
  });

  it("clearTeamKeyCache causes the next call to re-fetch from cloud", async () => {
    mockGetTeamKeyEnvelope.mockResolvedValue(validEnvelope);
    mockLoadE2eKeypair.mockResolvedValue({
      x25519PrivateKey: aliceKeypair.x25519PrivateKey,
      ed25519PrivateKey: aliceKeypair.ed25519PrivateKey,
    });

    await loadTeamKey("tk_test_001"); // populates cache
    clearTeamKeyCache();
    await loadTeamKey("tk_test_001"); // should re-fetch

    expect(mockGetTeamKeyEnvelope).toHaveBeenCalledTimes(2);
  });
});
