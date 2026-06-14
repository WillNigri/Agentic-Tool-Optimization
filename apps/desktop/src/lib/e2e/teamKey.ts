/**
 * Team Key cache — loads, unseals, and caches symmetric Team Keys.
 *
 * A Team Key is a 32-byte XChaCha20-Poly1305 key shared among all team
 * members.  Each member receives it as a crypto_box_seal envelope
 * (sealed to their X25519 public key); they unseal it using their private
 * key stored in the OS keychain.
 *
 * The cache is an in-memory Map keyed by team_key_id.  It lives for the
 * duration of the page/process; call clearTeamKeyCache() on logout or
 * member-removal events so stale keys don't accumulate.
 */

import _sodium from "libsodium-wrappers";
import { ensureSodiumReady, fromBase64, unsealTeamKey } from "./crypto";
import { loadE2eKeypair } from "./keychain";
import { getTeamKeyEnvelope } from "@/lib/cloud-api";

// ── In-memory cache ───────────────────────────────────────────────────────

/** team_key_id → 32-byte Team Key (plaintext, after unseal). */
const teamKeyCache = new Map<string, Uint8Array>();

// ── Error types ───────────────────────────────────────────────────────────

/**
 * Thrown when the sealed Team Key envelope cannot be opened.
 * Callers should prompt the user to re-push their public keys so the
 * team admin can re-seal the Team Key for the updated public key.
 */
export class TeamKeyUnsealError extends Error {
  constructor(teamKeyId: string, cause?: unknown) {
    super(
      `Failed to unseal Team Key ${teamKeyId}. ` +
        "Your E2E public key may be out of date — ask a team admin to rotate the Team Key. " +
        (cause instanceof Error ? cause.message : String(cause ?? "")),
    );
    this.name = "TeamKeyUnsealError";
  }
}

// ── Public API ────────────────────────────────────────────────────────────

/**
 * Load and unseal a Team Key by its ID.
 * - Returns the cached key on subsequent calls (no keychain round-trip).
 * - On first call: fetches the sealed envelope from the cloud, loads own
 *   X25519 private key from the keychain, unseals the envelope.
 * - Throws TeamKeyUnsealError if unseal fails (wrong key, tampered envelope).
 */
export async function loadTeamKey(teamKeyId: string): Promise<Uint8Array> {
  const cached = teamKeyCache.get(teamKeyId);
  if (cached) return cached;

  await ensureSodiumReady();

  // Fetch the sealed envelope from the cloud.
  const envelope = await getTeamKeyEnvelope(teamKeyId);

  // Wrap all crypto operations (base64 decode, unseal) in a single try/catch
  // so any failure — malformed envelope, wrong key, truncated bytes — surfaces
  // as a TeamKeyUnsealError rather than a raw libsodium or base64 error.
  let teamKey: Uint8Array;
  try {
    const sealedBytes = fromBase64(envelope.sealed_key);

    // Load own X25519 keypair from the OS keychain.
    const { x25519PrivateKey } = await loadE2eKeypair();

    // Derive our X25519 public key from the private key so we can pass the
    // full keypair to crypto_box_seal_open (libsodium requires both).
    const ownX25519PublicKey = _sodium.crypto_scalarmult_base(x25519PrivateKey);

    teamKey = await unsealTeamKey(sealedBytes, ownX25519PublicKey, x25519PrivateKey);
  } catch (err) {
    throw new TeamKeyUnsealError(teamKeyId, err);
  }

  if (teamKey.length !== 32) {
    throw new TeamKeyUnsealError(
      teamKeyId,
      `unexpected Team Key length ${teamKey.length} (expected 32)`,
    );
  }

  teamKeyCache.set(teamKeyId, teamKey);
  return teamKey;
}

/**
 * Evict all cached Team Keys.  Call on logout or when a member is removed
 * from a team so the stale plaintext keys don't linger in memory.
 */
export function clearTeamKeyCache(): void {
  teamKeyCache.clear();
}
