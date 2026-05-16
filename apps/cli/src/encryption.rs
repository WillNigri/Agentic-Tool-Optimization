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

/// 2026-05-16 — wrap the keychain read in a hard timeout. macOS shows a
/// Keychain Access permission dialog the first time a new binary
/// signature reads the master_key entry. In headless/background
/// invocations (subprocess-of-self in demo-compare, CI, watchers) the
/// dialog can't be approved, so the keyring call hangs forever. Bug
/// #48 dogfooded this directly: every cargo build between dispatches
/// invalidated the prior approval and the next dispatch sat at zero
/// CPU waiting for input that never came.
///
/// The fix is defensive: cap the call at KEYCHAIN_TIMEOUT_SECS. On
/// timeout, surface a clear error naming the env-var workaround so
/// the caller can unblock themselves without code changes.
const KEYCHAIN_TIMEOUT_SECS: u64 = 8;

fn master_key() -> Result<[u8; 32]> {
    use std::sync::mpsc;
    use std::time::Duration;

    // Send a String error rather than anyhow::Error since the latter
    // isn't Send. Caller re-wraps with anyhow!() after recv.
    let (tx, rx) = mpsc::channel::<std::result::Result<[u8; 32], String>>();

    std::thread::spawn(move || {
        let result = master_key_inner().map_err(|e| format!("{:#}", e));
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_secs(KEYCHAIN_TIMEOUT_SECS)) {
        Ok(Ok(key)) => Ok(key),
        Ok(Err(s)) => Err(anyhow!("{}", s)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(anyhow!(
            "keychain access timed out after {}s — macOS is likely showing a Keychain Access permission dialog \
             (the first read after a new binary build needs explicit approval). \
             Approve the dialog if visible (use 'Always Allow' so future dispatches don't re-prompt), \
             or bypass the keychain for this dispatch by setting the provider's API key as an env var \
             (e.g. GEMINI_API_KEY=..., MINIMAX_API_KEY=..., ANTHROPIC_API_KEY=...).",
            KEYCHAIN_TIMEOUT_SECS
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!(
            "keychain reader thread disconnected without sending a result — unexpected; report as a bug"
        )),
    }
}

fn master_key_inner() -> Result<[u8; 32]> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, MASTER_KEY_ACCOUNT)
        .with_context(|| {
            format!(
                "open keyring entry {}/{}",
                KEYCHAIN_SERVICE, MASTER_KEY_ACCOUNT
            )
        })?;
    // 2026-05-15 — CRITICAL FIX. Previous shape was:
    //
    //     if let Ok(b64) = entry.get_password() { ... }
    //     else { generate_and_store_new_random_key() }
    //
    // That treated EVERY error from get_password() as "no key exists,
    // generate a new one." But the keyring crate's error enum has
    // multiple variants — NoEntry, NoStorageAccess, PlatformFailure,
    // BadEncoding, TooLong, Invalid, Ambiguous — and treating them
    // all as "missing" silently rotates the master key, orphaning
    // every previously-encrypted llm_api_keys row. The user hit this
    // when the keychain entry was overwritten on 2026-05-14, breaking
    // decrypt of all keys stored earlier.
    //
    // Correct shape: ONLY NoEntry triggers a new-key generation.
    // Anything else fails loud — better to surface "your keychain is
    // locked / permission denied" than to silently destroy existing
    // ciphertexts.
    match entry.get_password() {
        Ok(b64) => decode_key_b64(&b64),
        Err(keyring::Error::NoEntry) => {
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
        Err(e) => Err(anyhow!(
            "keyring get_password failed (NOT a missing-entry case — refusing to silently rotate the master key): {}. \
             If your stored ciphertexts can't decrypt and you've confirmed the OS keychain entry was reset, \
             re-enter your API keys via the desktop's API Keys panel.",
            e
        )),
    }
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
