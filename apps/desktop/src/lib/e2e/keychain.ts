/**
 * Keychain bridge — thin wrappers around the Tauri `e2e_keychain` commands.
 * These are the only path through which E2E private key bytes cross the
 * Rust ↔ JS boundary.  Bytes never touch localStorage or any other
 * persistent JS storage.
 */

import { invoke } from "@tauri-apps/api/core";
import { fromBase64, toBase64 } from "./crypto";

/** True iff both E2E private keys are present in the OS keychain. */
export async function hasE2eKeypair(): Promise<boolean> {
  return await invoke<boolean>("e2e_keypair_exists");
}

/**
 * Write (or overwrite) both E2E private keys in the OS keychain.
 * Accepts raw bytes; base64-encodes before the IPC call.
 */
export async function storeE2eKeypair(
  x25519PrivateKey: Uint8Array,
  ed25519PrivateKey: Uint8Array,
): Promise<void> {
  await invoke("e2e_store_keypair", {
    x25519PrivkeyB64: toBase64(x25519PrivateKey),
    ed25519PrivkeyB64: toBase64(ed25519PrivateKey),
  });
}

/**
 * Load both E2E private keys from the OS keychain.
 * Returns raw bytes decoded from the stored base64.
 * Throws if either key is missing — call hasE2eKeypair() first.
 */
export async function loadE2eKeypair(): Promise<{
  x25519PrivateKey: Uint8Array;
  ed25519PrivateKey: Uint8Array;
}> {
  const out = await invoke<{ x25519_privkey_b64: string; ed25519_privkey_b64: string }>(
    "e2e_load_keypair",
  );
  return {
    x25519PrivateKey: fromBase64(out.x25519_privkey_b64),
    ed25519PrivateKey: fromBase64(out.ed25519_privkey_b64),
  };
}
