// Real encryption for llm_api_keys.encrypted_key. (Audit H1)
//
// Pre-2.4.8 the column was plain base64 with the name `simple_encrypt`
// implying cryptography that wasn't there. Any local user with read
// access to ~/.ato/local.db could `base64 -d` every API key. This
// module replaces that with AES-256-GCM under a master key kept in the
// OS keychain (macOS Keychain / Linux Secret Service / Windows
// Credential Manager).
//
// Format:
//   - New (v1):    "v1:" + base64(nonce_12_bytes || ciphertext_with_tag)
//   - Legacy (v0): plain base64(plaintext)   — read-only, migrated on next write
//
// Detection happens at decrypt time via the "v1:" prefix. A row without
// the prefix decodes as legacy; the migration in init_database re-
// encrypts those rows once the master key is available.
//
// Master-key lifecycle:
//   - Service "ato-desktop" / account "master_key_v1" in the keychain
//   - Created on first encrypt call (32 random bytes from OsRng,
//     base64-encoded so the keychain stores a printable string)
//   - Subsequent callers (desktop or CLI) read the same entry
//   - TOCTOU race: two processes creating simultaneously both
//     `set_password`; the second wins. We `get_password` after our
//     set to confirm — if the readback differs from what we wrote,
//     the other process beat us, and we use their key
//
// CLI mirror: apps/cli/src/encryption.rs has an identical
// implementation. Both reach the same keychain entry, so a key the
// desktop wrote is decryptable by the CLI and vice-versa.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::{engine::general_purpose, Engine as _};
use rand::RngCore;

const KEYCHAIN_SERVICE: &str = "ato-desktop";
const MASTER_KEY_ACCOUNT: &str = "master_key_v1";
const VERSION_PREFIX: &str = "v1:";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// Read (or atomically create) the AES-256 master key from the OS
/// keychain. Result is the 32-byte raw key.
///
/// 2026-05-15 — CRITICAL FIX. Previous shape (`if let Ok(b64) =
/// entry.get_password() { ... } else { generate }`) treated EVERY
/// keyring error as "no key exists, generate one." The keyring crate's
/// error enum has multiple variants (NoEntry, NoStorageAccess,
/// PlatformFailure, BadEncoding, TooLong, Invalid, Ambiguous); treating
/// them all as "missing" silently rotates the master key, orphaning
/// every previously-encrypted `llm_api_keys` row. User hit this on
/// 2026-05-14 when the keychain entry was overwritten (most likely an
/// app re-sign / re-permission prompt that didn't resolve cleanly).
///
/// Correct shape: ONLY NoEntry → generate. Anything else fails loud.
fn master_key() -> Result<[u8; 32], String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, MASTER_KEY_ACCOUNT)
        .map_err(|e| format!("keyring entry {}/{}: {}", KEYCHAIN_SERVICE, MASTER_KEY_ACCOUNT, e))?;
    match entry.get_password() {
        Ok(b64) => decode_key_b64(&b64),
        Err(keyring::Error::NoEntry) => {
            // Genuine first-run. Generate + store. Race-window note
            // preserved: if two processes hit this simultaneously we
            // re-read after the write so we use whoever's value ended
            // up in the keychain.
            let mut new_key = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut new_key);
            let b64 = general_purpose::STANDARD.encode(new_key);
            entry
                .set_password(&b64)
                .map_err(|e| format!("keyring set_password: {}", e))?;
            let final_b64 = entry
                .get_password()
                .map_err(|e| format!("keyring get_password after set: {}", e))?;
            decode_key_b64(&final_b64)
        }
        Err(e) => Err(format!(
            "keyring get_password failed (NOT a missing-entry case — refusing to silently rotate the master key): {}. \
             If you've confirmed the OS keychain entry was reset, your stored API keys are orphaned and must be re-entered via Settings → API Keys.",
            e
        )),
    }
}

fn decode_key_b64(b64: &str) -> Result<[u8; 32], String> {
    let bytes = general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("decode master key b64: {}", e))?;
    if bytes.len() != 32 {
        return Err(format!("master key is {} bytes (expected 32)", bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Encrypt a plaintext API key into the storage format. Always
/// produces a `v1:`-prefixed string. Errors only if the OS keychain
/// is broken — bubble that up rather than silently fall back to plain
/// base64 (callers depend on the returned string being a real cipher).
pub fn encrypt(plaintext: &str) -> Result<String, String> {
    let key_bytes = master_key()?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("encrypt: {}", e))?;
    let mut payload = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(format!(
        "{}{}",
        VERSION_PREFIX,
        general_purpose::STANDARD.encode(&payload)
    ))
}

/// Decrypt either format. New "v1:" rows decrypt with the keychain
/// master key; legacy rows (no prefix) base64-decode to plaintext, the
/// pre-2.4.8 behavior. The caller is expected to re-encrypt legacy
/// values on the next UPDATE so the DB drains itself.
pub fn decrypt(stored: &str) -> Result<String, String> {
    if let Some(b64) = stored.strip_prefix(VERSION_PREFIX) {
        let bytes = general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| format!("decode v1 payload: {}", e))?;
        if bytes.len() < NONCE_LEN + TAG_LEN {
            return Err(format!(
                "v1 payload too short ({} bytes, need ≥{})",
                bytes.len(),
                NONCE_LEN + TAG_LEN
            ));
        }
        let (nonce_bytes, ciphertext) = bytes.split_at(NONCE_LEN);
        let key_bytes = master_key()?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| {
                // Don't echo the underlying error — AES-GCM's "auth tag
                // mismatch" message is unhelpful and could leak across
                // bug reports. The cause is almost always: master key
                // was regenerated (keychain wipe), or the row was
                // copied from a different machine.
                "v1 decrypt failed — master key mismatch (was the OS keychain reset?)".to_string()
            })?;
        return String::from_utf8(plaintext).map_err(|e| format!("v1 plaintext not utf-8: {}", e));
    }
    // Legacy plain-base64 row. The migration UPDATE that runs after
    // each successful decrypt rewrites these as v1.
    let bytes = general_purpose::STANDARD
        .decode(stored)
        .map_err(|e| format!("legacy decode: {}", e))?;
    String::from_utf8(bytes).map_err(|e| format!("legacy plaintext not utf-8: {}", e))
}

/// True iff the stored value is already in the encrypted v1 format.
/// Used by the migration to skip rows that don't need re-writing.
pub fn is_v1(stored: &str) -> bool {
    stored.starts_with(VERSION_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    // We can't unit-test the keychain on CI (no DBus on Linux runners,
    // no Keychain on headless macOS), so these tests run only when the
    // ATO_ENCRYPTION_TESTS=1 env var is set on a developer machine
    // with a working keychain.
    fn keychain_available() -> bool {
        std::env::var("ATO_ENCRYPTION_TESTS").ok().as_deref() == Some("1")
    }

    #[test]
    fn roundtrip() {
        if !keychain_available() {
            eprintln!("skipping (set ATO_ENCRYPTION_TESTS=1 to run)");
            return;
        }
        let plain = "sk-ant-test-key-do-not-leak-12345";
        let stored = encrypt(plain).expect("encrypt");
        assert!(is_v1(&stored), "encrypted value should be v1");
        let back = decrypt(&stored).expect("decrypt");
        assert_eq!(back, plain);
    }

    #[test]
    fn legacy_decode_path() {
        // Doesn't need the keychain — pure base64 path.
        let plain = "sk-legacy-test-key";
        let legacy = general_purpose::STANDARD.encode(plain.as_bytes());
        assert!(!is_v1(&legacy));
        let back = decrypt(&legacy).expect("decrypt legacy");
        assert_eq!(back, plain);
    }

    #[test]
    fn v1_detection() {
        assert!(is_v1("v1:abcdef=="));
        assert!(!is_v1("abcdef=="));
        assert!(!is_v1(""));
        assert!(!is_v1("v0:legacy"));
    }
}
