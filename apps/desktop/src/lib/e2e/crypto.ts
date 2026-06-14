/**
 * E2E crypto primitives — Wave 1, v2.15 Live Collab.
 *
 * All cryptography delegates to libsodium-wrappers so the primitives are
 * auditable OSS with no hand-rolled crypto.  This file is intentionally
 * kept free of any PRO logic; it is the OSS-safe half of the E2E stack.
 *
 * Algorithms:
 *   - X25519 / crypto_box_seal   — Team Key envelope encryption
 *   - XChaCha20-Poly1305 AEAD   — Event payload encryption
 *   - Ed25519 detached sign/verify — Message authenticity
 */

import _sodium from "libsodium-wrappers";

let _ready = false;

/** Call once before any crypto operation (idempotent). */
export async function ensureSodiumReady(): Promise<void> {
  if (_ready) return;
  await _sodium.ready;
  _ready = true;
}

// ── Internal helpers ──────────────────────────────────────────────────────

/**
 * Coerce a Uint8Array to a plain `new Uint8Array(...)` from the current
 * global realm. This is required when running under jsdom (Vitest): jsdom's
 * TextEncoder and other APIs produce Uint8Arrays from a separate V8 context,
 * so `buf instanceof Uint8Array` returns false in Node's realm — libsodium's
 * internal type-guard throws "unsupported input type". A copy forces the right
 * realm. In production (Vite/browser) all Uint8Arrays are already from the
 * same realm, so the copy is a cheap no-op identity.
 */
function u8(buf: Uint8Array): Uint8Array {
  // Fast path: already our realm — just return.
  // eslint-disable-next-line no-instanceof/no-instanceof
  if (buf instanceof Uint8Array && buf.constructor === Uint8Array) return buf;
  return new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
}

// ── Types ─────────────────────────────────────────────────────────────────

export interface E2eKeypair {
  /** 32-byte X25519 public key. */
  x25519PublicKey: Uint8Array;
  /** 32-byte X25519 private key. */
  x25519PrivateKey: Uint8Array;
  /** 32-byte Ed25519 public key. */
  ed25519PublicKey: Uint8Array;
  /** 64-byte Ed25519 expanded private key (libsodium form). */
  ed25519PrivateKey: Uint8Array;
}

// ── Base64 helpers ────────────────────────────────────────────────────────

/**
 * Encode bytes as standard base64 (no URL-safe, padding present).
 * Matches the cloud-side regex: /^[A-Za-z0-9+/]+={0,2}$/
 */
export function toBase64(bytes: Uint8Array): string {
  // Use libsodium's own encoder to stay consistent with how it expects bytes.
  // Equivalent to btoa(String.fromCharCode(...bytes)) but correct for Node.
  return _sodium.to_base64(u8(bytes), _sodium.base64_variants.ORIGINAL);
}

/**
 * Decode standard base64 to bytes.
 */
export function fromBase64(b64: string): Uint8Array {
  return _sodium.from_base64(b64, _sodium.base64_variants.ORIGINAL);
}

// ── Key generation ────────────────────────────────────────────────────────

/**
 * Generate a fresh E2E keypair consisting of an X25519 keypair (for
 * crypto_box_seal envelope encryption) and an Ed25519 keypair (for
 * detached signatures).  Returns raw bytes; the caller is responsible for
 * base64-encoding before storage or transport.
 */
export async function generateE2eKeypair(): Promise<E2eKeypair> {
  await ensureSodiumReady();
  const x25519 = _sodium.crypto_box_keypair();
  const ed25519 = _sodium.crypto_sign_keypair();
  return {
    x25519PublicKey: x25519.publicKey,   // 32 bytes
    x25519PrivateKey: x25519.privateKey, // 32 bytes
    ed25519PublicKey: ed25519.publicKey,  // 32 bytes
    ed25519PrivateKey: ed25519.privateKey, // 64 bytes (expanded)
  };
}

/**
 * Generate a fresh 32-byte symmetric Team Key suitable for use with
 * encryptPayload / decryptPayload.
 */
export async function generateTeamKey(): Promise<Uint8Array> {
  await ensureSodiumReady();
  return _sodium.randombytes_buf(_sodium.crypto_aead_xchacha20poly1305_ietf_KEYBYTES);
}

// ── Envelope encryption (crypto_box_seal) ────────────────────────────────

/**
 * Seal a Team Key (32 bytes) to a recipient's X25519 public key.
 * Uses libsodium's crypto_box_seal (anonymous sender, ECDH ephemeral + XSalsa20-Poly1305).
 * Output length = plaintext.length + crypto_box_SEALBYTES (48 bytes overhead for 32-byte input).
 */
export async function sealTeamKey(
  teamKey: Uint8Array,
  recipientX25519Pubkey: Uint8Array,
): Promise<Uint8Array> {
  await ensureSodiumReady();
  return _sodium.crypto_box_seal(u8(teamKey), u8(recipientX25519Pubkey));
}

/**
 * Unseal an envelope using the recipient's own X25519 keypair.
 * Throws if the envelope is malformed or the key pair is wrong.
 */
export async function unsealTeamKey(
  sealed: Uint8Array,
  ownX25519PublicKey: Uint8Array,
  ownX25519PrivateKey: Uint8Array,
): Promise<Uint8Array> {
  await ensureSodiumReady();
  const result = _sodium.crypto_box_seal_open(
    u8(sealed),
    u8(ownX25519PublicKey),
    u8(ownX25519PrivateKey),
  );
  if (!result) {
    throw new Error("crypto_box_seal_open failed: invalid ciphertext or wrong keypair");
  }
  return result;
}

// ── AEAD payload encryption (XChaCha20-Poly1305) ─────────────────────────

/**
 * Encrypt an event payload with XChaCha20-Poly1305-IETF AEAD.
 * Returns a fresh 24-byte nonce and the ciphertext (plaintext.length + 16 auth tag).
 * The nonce must be stored alongside the ciphertext and passed to decryptPayload.
 * associatedData is authenticated but not encrypted (e.g. event ID + team key ID).
 */
export async function encryptPayload(
  plaintext: Uint8Array,
  teamKey: Uint8Array,
  associatedData: Uint8Array,
): Promise<{ nonce: Uint8Array; ciphertext: Uint8Array }> {
  await ensureSodiumReady();
  const nonce = _sodium.randombytes_buf(
    _sodium.crypto_aead_xchacha20poly1305_ietf_NPUBBYTES, // 24 bytes
  );
  const ciphertext = _sodium.crypto_aead_xchacha20poly1305_ietf_encrypt(
    u8(plaintext),
    u8(associatedData),
    null, // no secret nonce
    nonce,
    u8(teamKey),
  );
  return { nonce, ciphertext };
}

/**
 * Decrypt an event payload encrypted by encryptPayload.
 * Throws if the tag is invalid (wrong key, tampered ciphertext, or wrong AD).
 */
export async function decryptPayload(
  ciphertext: Uint8Array,
  nonce: Uint8Array,
  teamKey: Uint8Array,
  associatedData: Uint8Array,
): Promise<Uint8Array> {
  await ensureSodiumReady();
  const plaintext = _sodium.crypto_aead_xchacha20poly1305_ietf_decrypt(
    null, // no secret nonce
    u8(ciphertext),
    u8(associatedData),
    u8(nonce),
    u8(teamKey),
  );
  if (!plaintext) {
    throw new Error(
      "crypto_aead_xchacha20poly1305_ietf_decrypt failed: authentication tag invalid",
    );
  }
  return plaintext;
}

// ── Ed25519 detached sign / verify ────────────────────────────────────────

/**
 * Produce a detached Ed25519 signature over message.
 * Returns 64 bytes. Store alongside the ciphertext in the event envelope.
 */
export async function signMessage(
  message: Uint8Array,
  ed25519PrivateKey: Uint8Array,
): Promise<Uint8Array> {
  await ensureSodiumReady();
  return _sodium.crypto_sign_detached(u8(message), u8(ed25519PrivateKey));
}

/**
 * Verify a detached Ed25519 signature.
 * Returns true iff the signature is valid for message under ed25519PublicKey.
 */
export async function verifyMessage(
  message: Uint8Array,
  signature: Uint8Array,
  ed25519PublicKey: Uint8Array,
): Promise<boolean> {
  await ensureSodiumReady();
  return _sodium.crypto_sign_verify_detached(u8(signature), u8(message), u8(ed25519PublicKey));
}
