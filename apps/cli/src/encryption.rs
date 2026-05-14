// CLI mirror of apps/desktop/src-tauri/src/encryption.rs. Same
// keychain service+account, same v1 format, same legacy fallback.
// Both binaries decrypt rows written by either — the master key is
// process-agnostic, scoped to the OS user.
//
// Why duplicated rather than crate-extracted: the encryption module
// only depends on aes-gcm + keyring + base64 + rand, and both crates
// already pull those in transitively. A shared `ato-encryption` crate
// is reasonable but the surface is small enough that two copies
// drifting is easy to catch in code review — and the byok / api-
// providers modules followed the same pattern.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use rand::RngCore;

const KEYCHAIN_SERVICE: &str = "ato-desktop";
const MASTER_KEY_ACCOUNT: &str = "master_key_v1";
const VERSION_PREFIX: &str = "v1:";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

fn master_key() -> Result<[u8; 32]> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, MASTER_KEY_ACCOUNT)
        .with_context(|| {
            format!(
                "open keyring entry {}/{}",
                KEYCHAIN_SERVICE, MASTER_KEY_ACCOUNT
            )
        })?;
    if let Ok(b64) = entry.get_password() {
        return decode_key_b64(&b64);
    }
    let mut new_key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut new_key);
    let b64 = general_purpose::STANDARD.encode(new_key);
    entry
        .set_password(&b64)
        .context("keyring set_password (master key)")?;
    let final_b64 = entry
        .get_password()
        .context("keyring get_password (master key, post-set)")?;
    decode_key_b64(&final_b64)
}

fn decode_key_b64(b64: &str) -> Result<[u8; 32]> {
    let bytes = general_purpose::STANDARD
        .decode(b64.trim())
        .context("decode master key b64")?;
    if bytes.len() != 32 {
        return Err(anyhow!(
            "master key is {} bytes (expected 32)",
            bytes.len()
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[allow(dead_code)] // exposed for future write paths in the CLI
pub fn encrypt(plaintext: &str) -> Result<String> {
    let key_bytes = master_key()?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("encrypt: {}", e))?;
    let mut payload = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(format!(
        "{}{}",
        VERSION_PREFIX,
        general_purpose::STANDARD.encode(&payload)
    ))
}

/// Decrypt v1 or legacy. CLI callers (e.g., api_dispatch resolve_api_key)
/// route through this; the desktop's encryption.rs is the mirror.
pub fn decrypt(stored: &str) -> Result<String> {
    if let Some(b64) = stored.strip_prefix(VERSION_PREFIX) {
        let bytes = general_purpose::STANDARD
            .decode(b64.trim())
            .context("decode v1 payload")?;
        if bytes.len() < NONCE_LEN + TAG_LEN {
            return Err(anyhow!(
                "v1 payload too short ({} bytes)",
                bytes.len()
            ));
        }
        let (nonce_bytes, ciphertext) = bytes.split_at(NONCE_LEN);
        let key_bytes = master_key()?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| anyhow!("v1 decrypt failed (keychain master key mismatch?)"))?;
        return String::from_utf8(plaintext).context("v1 plaintext not utf-8");
    }
    let bytes = general_purpose::STANDARD
        .decode(stored)
        .context("legacy base64 decode")?;
    String::from_utf8(bytes).context("legacy plaintext not utf-8")
}

#[allow(dead_code)] // used by callers that want to skip the migration UPDATE
pub fn is_v1(stored: &str) -> bool {
    stored.starts_with(VERSION_PREFIX)
}
