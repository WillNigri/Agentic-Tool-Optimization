// v2.17 Wave 3 — browser-side tether crypto primitives.
//
// Invariants (from V2-17-WEB-TETHER-SYNTHESIS.md):
//   - X25519 ephemeral keypairs; privkey MUST be discarded after
//     HKDF derive — callers must zero/drop the reference immediately.
//   - session_key lives in memory ONLY. Never written to IndexedDB or
//     localStorage.
//   - AEAD = XChaCha20-Poly1305 (24-byte nonce, 16-byte tag).
//   - KDF = HKDF-SHA256 with info = "ato-tether-v1" || session_id.
//
// All crypto is @stablelib which ships proper ESM and has no
// dynamic-require of WASM, making it safe in Vite's ESM resolver.

import { generateKeyPair, sharedKey } from "@stablelib/x25519";
import { XChaCha20Poly1305 } from "@stablelib/xchacha20poly1305";
import { HKDF } from "@stablelib/hkdf";
import { SHA256 } from "@stablelib/sha256";

// ──────────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────────

export interface TetherKeypair {
  pubkey: Uint8Array;  // 32 bytes
  privkey: Uint8Array; // 32 bytes — MUST be discarded after deriveSessionKey()
}

// ──────────────────────────────────────────────────────────────────
// DH + KDF
// ──────────────────────────────────────────────────────────────────

/** Generate an ephemeral X25519 keypair for browser-side pairing. */
export function generateTetherKeypair(): TetherKeypair {
  const kp = generateKeyPair();
  return { pubkey: kp.publicKey, privkey: kp.secretKey };
}

/**
 * Derive the tether session_key via X25519 DH + HKDF-SHA256.
 *
 * info = UTF8("ato-tether-v1") || UTF8(sessionId)
 * salt = 32 zero bytes (per synthesis doc)
 *
 * IMPORTANT: caller MUST zero out ephemeralPrivkey after this call.
 * The returned key is 32 bytes.
 */
export function deriveSessionKey(
  ephemeralPrivkey: Uint8Array,
  peerPubkey: Uint8Array,
  sessionId: string,
): Uint8Array {
  const dhOutput = sharedKey(ephemeralPrivkey, peerPubkey);

  const prefix = new TextEncoder().encode("ato-tether-v1");
  const sid = new TextEncoder().encode(sessionId);
  const info = new Uint8Array(prefix.length + sid.length);
  info.set(prefix);
  info.set(sid, prefix.length);

  const salt = new Uint8Array(32); // zero32

  const hkdf = new HKDF(SHA256, dhOutput, salt, info);
  return hkdf.expand(32);
}

// ──────────────────────────────────────────────────────────────────
// AEAD — XChaCha20-Poly1305
// ──────────────────────────────────────────────────────────────────

/**
 * Encrypt plaintext with XChaCha20-Poly1305.
 *
 * @param plaintext   Payload bytes.
 * @param sessionKey  32-byte derived session key.
 * @param nonce       24-byte caller-managed nonce (see client.ts for
 *                    the frame nonce scheme).
 * @returns ciphertext with appended 16-byte Poly1305 tag.
 */
export function aeadEncrypt(
  plaintext: Uint8Array,
  sessionKey: Uint8Array,
  nonce: Uint8Array,
): Uint8Array {
  if (nonce.length !== 24) throw new Error("nonce must be 24 bytes");
  const aead = new XChaCha20Poly1305(sessionKey);
  return aead.seal(nonce, plaintext);
}

/**
 * Decrypt and authenticate an XChaCha20-Poly1305 ciphertext.
 *
 * Throws if the Poly1305 tag does not match (tampered or wrong key).
 */
export function aeadDecrypt(
  ciphertext: Uint8Array,
  sessionKey: Uint8Array,
  nonce: Uint8Array,
): Uint8Array {
  if (nonce.length !== 24) throw new Error("nonce must be 24 bytes");
  const aead = new XChaCha20Poly1305(sessionKey);
  const plain = aead.open(nonce, ciphertext);
  if (plain === null) {
    throw new Error("AEAD tag mismatch — ciphertext is tampered or key is wrong");
  }
  return plain;
}

// ──────────────────────────────────────────────────────────────────
// Base64 helpers (standard base64; cloud WS frames use base64 strings)
// ──────────────────────────────────────────────────────────────────

export function toBase64(b: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < b.length; i++) {
    binary += String.fromCharCode(b[i]);
  }
  return btoa(binary);
}

export function fromBase64(s: string): Uint8Array {
  const binary = atob(s);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}
