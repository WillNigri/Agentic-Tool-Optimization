// E2E keychain commands — stores the user's X25519 and Ed25519 private keys
// in the OS keychain (macOS Keychain / Linux Secret Service / Windows
// Credential Manager) for use by the live-collab E2E encryption layer.
//
// Keys are stored as standard base64 strings (no URL-safe encoding) so they
// can round-trip through the cloud-side base64 regex: /^[A-Za-z0-9+/]+={0,2}$/
//
// Account names:
//   e2e_x25519_privkey_v1  — 32-byte X25519 private key (DH for crypto_box_seal)
//   e2e_ed25519_privkey_v1 — 64-byte Ed25519 expanded private key (detached signatures)
//
// Both entries live under the same service name ("ato-desktop") as the master
// key, matching the unified keychain access-control group for this app.
//
// Timeout shape mirrors encryption.rs: a spawned thread does the keychain I/O
// with an 8-second hard timeout. This prevents the Tauri process from hanging
// if macOS shows a permission dialog in a headless context.
//
// TODO: integration test on real keychain (gated on ATO_ENCRYPTION_TESTS=1,
// same pattern as encryption.rs — skipped on CI / headless runners).

use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::sync::mpsc;
use std::time::Duration;

const KEYCHAIN_SERVICE: &str = "ato-desktop";
const X25519_ACCOUNT: &str = "e2e_x25519_privkey_v1";
const ED25519_ACCOUNT: &str = "e2e_ed25519_privkey_v1";
const KEYCHAIN_TIMEOUT_SECS: u64 = 8;

/// Returned by `e2e_load_keypair`.
/// Field names use snake_case so Tauri's serde_json serialises them to
/// `x25519_privkey_b64` / `ed25519_privkey_b64` on the JS side.
#[derive(Debug, Serialize, Deserialize)]
pub struct E2eKeypairBytes {
    pub x25519_privkey_b64: String,
    pub ed25519_privkey_b64: String,
}

// ── internal helpers ──────────────────────────────────────────────────────

fn keychain_get(account: &str) -> Result<Option<String>, String> {
    let account = account.to_string();
    let account_for_err = account.clone(); // retained for timeout error message
    let (tx, rx) = mpsc::channel::<Result<Option<String>, String>>();
    std::thread::spawn(move || {
        let result = (|| {
            let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &account)
                .map_err(|e| format!("keyring entry {}/{}: {}", KEYCHAIN_SERVICE, account, e))?;
            match entry.get_password() {
                Ok(v) => Ok(Some(v)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(format!(
                    "keyring get_password {}/{}: {}",
                    KEYCHAIN_SERVICE, account, e
                )),
            }
        })();
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(KEYCHAIN_TIMEOUT_SECS)) {
        Ok(r) => r,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "keychain access timed out after {}s for {}/{} — macOS may be showing a \
             Keychain Access permission dialog. Approve it or set ATO_MASTER_KEY_B64 \
             to bypass the keychain for dev runs.",
            KEYCHAIN_TIMEOUT_SECS, KEYCHAIN_SERVICE, account_for_err
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!(
            "keychain reader thread disconnected for {}/{}",
            KEYCHAIN_SERVICE, account_for_err
        )),
    }
}

fn keychain_set(account: &str, value: &str) -> Result<(), String> {
    let account = account.to_string();
    let account_for_err = account.clone(); // retained for timeout error message
    let value = value.to_string();
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        let result = (|| {
            let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &account)
                .map_err(|e| format!("keyring entry {}/{}: {}", KEYCHAIN_SERVICE, account, e))?;
            entry
                .set_password(&value)
                .map_err(|e| format!("keyring set_password {}/{}: {}", KEYCHAIN_SERVICE, account, e))
        })();
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(KEYCHAIN_TIMEOUT_SECS)) {
        Ok(r) => r,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "keychain write timed out after {}s for {}/{}",
            KEYCHAIN_TIMEOUT_SECS, KEYCHAIN_SERVICE, account_for_err
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!(
            "keychain writer thread disconnected for {}/{}",
            KEYCHAIN_SERVICE, account_for_err
        )),
    }
}

/// Validate that a base64 string decodes to exactly `expected_len` bytes.
fn validate_b64_len(b64: &str, label: &str, expected_len: usize) -> Result<(), String> {
    let bytes = general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("{} is not valid base64: {}", label, e))?;
    if bytes.len() != expected_len {
        return Err(format!(
            "{} decoded to {} bytes (expected {})",
            label,
            bytes.len(),
            expected_len
        ));
    }
    Ok(())
}

// ── Tauri commands ────────────────────────────────────────────────────────

/// Return true iff both E2E private key entries are present in the keychain.
#[tauri::command]
pub fn e2e_keypair_exists() -> Result<bool, String> {
    let x25519 = keychain_get(X25519_ACCOUNT)?;
    let ed25519 = keychain_get(ED25519_ACCOUNT)?;
    Ok(x25519.is_some() && ed25519.is_some())
}

/// Write (or overwrite) both E2E private keys in the keychain.
/// Both arguments must be standard base64.
/// x25519_privkey_b64 must decode to exactly 32 bytes.
/// ed25519_privkey_b64 must decode to exactly 64 bytes.
#[tauri::command]
pub fn e2e_store_keypair(
    x25519_privkey_b64: String,
    ed25519_privkey_b64: String,
) -> Result<(), String> {
    validate_b64_len(&x25519_privkey_b64, "x25519_privkey_b64", 32)?;
    validate_b64_len(&ed25519_privkey_b64, "ed25519_privkey_b64", 64)?;
    keychain_set(X25519_ACCOUNT, x25519_privkey_b64.trim())?;
    keychain_set(ED25519_ACCOUNT, ed25519_privkey_b64.trim())?;
    Ok(())
}

/// Load both E2E private keys from the keychain.
/// Returns an error if either is missing (caller should call `e2e_keypair_exists`
/// first and generate + store if absent).
#[tauri::command]
pub fn e2e_load_keypair() -> Result<E2eKeypairBytes, String> {
    let x25519 = keychain_get(X25519_ACCOUNT)?.ok_or_else(|| {
        format!(
            "E2E X25519 private key not found in keychain ({}/{}). \
             Call e2e_store_keypair first.",
            KEYCHAIN_SERVICE, X25519_ACCOUNT
        )
    })?;
    let ed25519 = keychain_get(ED25519_ACCOUNT)?.ok_or_else(|| {
        format!(
            "E2E Ed25519 private key not found in keychain ({}/{}). \
             Call e2e_store_keypair first.",
            KEYCHAIN_SERVICE, ED25519_ACCOUNT
        )
    })?;
    Ok(E2eKeypairBytes {
        x25519_privkey_b64: x25519,
        ed25519_privkey_b64: ed25519,
    })
}
