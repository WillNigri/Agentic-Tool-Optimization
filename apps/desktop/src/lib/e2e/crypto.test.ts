/**
 * Unit tests for the E2E crypto primitives module.
 * All tests run in the Vitest jsdom environment with libsodium-wrappers.
 */

import { describe, it, expect, beforeAll } from "vitest";
import {
  ensureSodiumReady,
  generateE2eKeypair,
  generateTeamKey,
  sealTeamKey,
  unsealTeamKey,
  encryptPayload,
  decryptPayload,
  signMessage,
  verifyMessage,
  toBase64,
  fromBase64,
} from "./crypto";

beforeAll(async () => {
  await ensureSodiumReady();
});

// ── generateE2eKeypair ────────────────────────────────────────────────────

describe("generateE2eKeypair", () => {
  it("produces arrays of the correct byte lengths", async () => {
    const kp = await generateE2eKeypair();
    expect(kp.x25519PublicKey).toBeInstanceOf(Uint8Array);
    expect(kp.x25519PublicKey.byteLength).toBe(32);
    expect(kp.x25519PrivateKey).toBeInstanceOf(Uint8Array);
    expect(kp.x25519PrivateKey.byteLength).toBe(32);
    expect(kp.ed25519PublicKey).toBeInstanceOf(Uint8Array);
    expect(kp.ed25519PublicKey.byteLength).toBe(32);
    expect(kp.ed25519PrivateKey).toBeInstanceOf(Uint8Array);
    expect(kp.ed25519PrivateKey.byteLength).toBe(64);
  });

  it("generates distinct keypairs on each call", async () => {
    const a = await generateE2eKeypair();
    const b = await generateE2eKeypair();
    // Public keys should differ (astronomically unlikely to collide).
    expect(toBase64(a.x25519PublicKey)).not.toBe(toBase64(b.x25519PublicKey));
    expect(toBase64(a.ed25519PublicKey)).not.toBe(toBase64(b.ed25519PublicKey));
  });
});

// ── sealTeamKey / unsealTeamKey (crypto_box_seal roundtrip) ──────────────

describe("sealTeamKey / unsealTeamKey", () => {
  it("roundtrip preserves the Team Key plaintext", async () => {
    const kp = await generateE2eKeypair();
    const teamKey = await generateTeamKey();

    const sealed = await sealTeamKey(teamKey, kp.x25519PublicKey);
    const recovered = await unsealTeamKey(sealed, kp.x25519PublicKey, kp.x25519PrivateKey);

    // Compare via base64 to avoid jsdom cross-realm Uint8Array toEqual issues.
    expect(toBase64(recovered)).toBe(toBase64(teamKey));
  });

  it("unseal fails with wrong keypair", async () => {
    const kpAlice = await generateE2eKeypair();
    const kpBob = await generateE2eKeypair();
    const teamKey = await generateTeamKey();

    // Seal to Alice, but try to open with Bob's keys.
    const sealed = await sealTeamKey(teamKey, kpAlice.x25519PublicKey);
    await expect(
      unsealTeamKey(sealed, kpBob.x25519PublicKey, kpBob.x25519PrivateKey),
    ).rejects.toThrow();
  });
});

// ── encryptPayload / decryptPayload (XChaCha20-Poly1305) ─────────────────

describe("encryptPayload / decryptPayload", () => {
  const plaintext = new TextEncoder().encode("hello e2e event payload");
  const ad = new TextEncoder().encode("event-id:42,team_key_id:abc");

  it("roundtrip preserves plaintext with correct AD", async () => {
    const teamKey = await generateTeamKey();
    const { nonce, ciphertext } = await encryptPayload(plaintext, teamKey, ad);

    expect(nonce.byteLength).toBe(24);
    // ciphertext = plaintext.length + 16 (POLY1305 tag)
    expect(ciphertext.byteLength).toBe(plaintext.byteLength + 16);

    const recovered = await decryptPayload(ciphertext, nonce, teamKey, ad);
    // Compare as hex strings to avoid Uint8Array cross-realm issues in jsdom.
    expect(toBase64(recovered)).toBe(toBase64(plaintext));
  });

  it("decryptPayload with WRONG associated data throws", async () => {
    const teamKey = await generateTeamKey();
    const { nonce, ciphertext } = await encryptPayload(plaintext, teamKey, ad);

    const wrongAd = new TextEncoder().encode("tampered-ad");
    await expect(decryptPayload(ciphertext, nonce, teamKey, wrongAd)).rejects.toThrow();
  });

  it("decryptPayload with WRONG key throws", async () => {
    const teamKey = await generateTeamKey();
    const wrongKey = await generateTeamKey();
    const { nonce, ciphertext } = await encryptPayload(plaintext, teamKey, ad);

    await expect(decryptPayload(ciphertext, nonce, wrongKey, ad)).rejects.toThrow();
  });

  it("decryptPayload with tampered ciphertext throws", async () => {
    const teamKey = await generateTeamKey();
    const { nonce, ciphertext } = await encryptPayload(plaintext, teamKey, ad);

    const tampered = new Uint8Array(ciphertext);
    tampered[0] ^= 0xff; // flip first byte

    await expect(decryptPayload(tampered, nonce, teamKey, ad)).rejects.toThrow();
  });
});

// ── signMessage / verifyMessage (Ed25519) ─────────────────────────────────

describe("signMessage / verifyMessage", () => {
  const msg = new TextEncoder().encode("ATO event authenticity test");

  it("valid signature verifies true", async () => {
    const kp = await generateE2eKeypair();
    const sig = await signMessage(msg, kp.ed25519PrivateKey);

    expect(sig.byteLength).toBe(64);
    const valid = await verifyMessage(msg, sig, kp.ed25519PublicKey);
    expect(valid).toBe(true);
  });

  it("tampered message returns false", async () => {
    const kp = await generateE2eKeypair();
    const sig = await signMessage(msg, kp.ed25519PrivateKey);

    const tampered = new TextEncoder().encode("ATO event authenticity TAMPERED");
    const valid = await verifyMessage(tampered, sig, kp.ed25519PublicKey);
    expect(valid).toBe(false);
  });

  it("signature from different key returns false", async () => {
    const kpAlice = await generateE2eKeypair();
    const kpBob = await generateE2eKeypair();
    const sig = await signMessage(msg, kpAlice.ed25519PrivateKey);

    // Verify with Bob's public key — should fail.
    const valid = await verifyMessage(msg, sig, kpBob.ed25519PublicKey);
    expect(valid).toBe(false);
  });
});

// ── Base64 helpers ────────────────────────────────────────────────────────

describe("toBase64 / fromBase64", () => {
  it("roundtrips arbitrary bytes", () => {
    const bytes = new Uint8Array([0x00, 0xff, 0x80, 0x42, 0x01]);
    const b64 = toBase64(bytes);
    // Standard base64 characters only (no URL-safe chars).
    expect(b64).toMatch(/^[A-Za-z0-9+/]+=*$/);
    expect(fromBase64(b64)).toEqual(bytes);
  });

  it("encodes all-zero 32 bytes to the correct base64", () => {
    const zeros = new Uint8Array(32);
    // 32 zero bytes → "AAAA...AA==" in standard base64
    const b64 = toBase64(zeros);
    expect(fromBase64(b64)).toEqual(zeros);
  });
});
