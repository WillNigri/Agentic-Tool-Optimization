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

// 2026-05-17 — process-memory cache. macOS keychain ACL re-prompts on
// every binary-signature change, AND historically the dispatch path
// fetched the master key per call. Within a single process there's no
// reason to hit the keychain more than once: cache the [u8; 32] in a
// OnceLock, the first call pays the keychain round-trip, every
// subsequent call returns instantly. Cuts the "dialog spam during
// rebuilds" problem AND makes scripted/cron flows cheaper.
//
// Cache is per-process — a new `ato dispatch` invocation starts cold,
// which is correct (macOS code-signature ACL is the right enforcement
// boundary across processes; in-process caching shouldn't bypass it).
static MASTER_KEY_CACHE: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();

// PR-6 (master_key_v2) — exposed to `commands::master_key::export`
// so the CLI's `ato master-key export` subcommand can read the
// current keychain key without duplicating the keychain + cache
// + ATO_MASTER_KEY_B64 env-bypass logic. Keep `master_key` itself
// private (callers should go through encrypt/decrypt); this wrapper
// is the only PR-6-blessed access path. SYNC WITH: any future
// rename of master_key must also rename here.
pub(crate) fn export_master_key_b64() -> Result<String> {
    use base64::{engine::general_purpose, Engine as _};
    let bytes = master_key()?;
    Ok(general_purpose::STANDARD.encode(bytes))
}

fn master_key() -> Result<[u8; 32]> {
    if let Some(cached) = MASTER_KEY_CACHE.get() {
        return Ok(*cached);
    }
    let key = master_key_fetch()?;
    // Use `get_or_init`-style write — if two threads raced through the
    // check above, the loser's write is dropped; both end up with the
    // same value (keychain is the source of truth).
    let _ = MASTER_KEY_CACHE.set(key);
    Ok(key)
}

fn master_key_fetch() -> Result<[u8; 32]> {
    // 2026-05-17 — dev-mode bypass. Unsigned local builds (cargo build
    // --release on a dev machine) produce a fresh code signature on
    // every rebuild, which macOS keychain treats as "a new app" — so
    // even after clicking "Always Allow" the dialog comes back on the
    // next rebuild. The env var bypass lets dev builds skip the
    // keychain entirely. Production users on signed Apple Developer
    // releases NEVER set this var and go through the normal keychain
    // path. Same security model: an attacker with user-level env access
    // already has the user's keychain too.
    if let Ok(b64) = std::env::var("ATO_MASTER_KEY_B64") {
        let trimmed = b64.trim();
        if !trimmed.is_empty() {
            return decode_key_b64(trimmed);
        }
    }

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
             Approve the dialog if visible (use 'Always Allow' so future dispatches don't re-prompt). \
             To bypass the keychain for this run, copy the master key from the OS keychain into the env var ATO_MASTER_KEY_B64: \
             `export ATO_MASTER_KEY_B64=\"$(security find-generic-password -s ato-desktop -a master_key_v1 -w)\"`. \
             Provider API-key env vars (GEMINI_API_KEY, MINIMAX_API_KEY, ANTHROPIC_API_KEY, ...) only help if you also have those keys handy — the ATO_MASTER_KEY_B64 path is the real bypass.",
            KEYCHAIN_TIMEOUT_SECS
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!(
            "keychain reader thread disconnected without sending a result — unexpected; report as a bug"
        )),
    }
}

/// PR 13 (2026-05-17) — first-run sentinel file. The 2026-05-15
/// hotfix limited regeneration to the `keyring::Error::NoEntry` arm
/// of the match. That's necessary but NOT sufficient on macOS,
/// because `keyring-2.3.3/src/macos.rs:234` maps the underlying
/// `errSecItemNotFound` to `NoEntry` — and macOS returns that same
/// errSecItemNotFound when a binary's code-signing Designated
/// Requirement does not appear in the keychain entry's ACL.
///
/// In other words: an adhoc-signed dev build of `ato` reading the
/// production-signed desktop's `master_key_v1` entry sees `NoEntry`
/// from the keyring crate — semantically identical to "no row at
/// all," even though the entry exists for the user. The 2026-05-15
/// guard then rotates the master key, orphaning every existing
/// llm_api_keys ciphertext (today's bug Will hit during PR 3
/// dogfood).
///
/// The sentinel resolves the ambiguity. We touch
/// `~/.ato/.master_key_initialized` the first time we successfully
/// generate a master key. On subsequent runs, if `NoEntry` comes
/// back AND the sentinel exists, this binary is being ACL-masked
/// out of an entry the user has previously initialized — never
/// regenerate. If the sentinel is absent, it's a true fresh
/// install and regeneration is correct.
const FIRST_RUN_SENTINEL_NAME: &str = ".master_key_initialized";

fn first_run_sentinel_path() -> Option<std::path::PathBuf> {
    // Avoid pulling in the `dirs` crate just for this — std::env
    // gives us $HOME on Unix and %USERPROFILE% on Windows is
    // shimmed by the system. Returns None on environments that
    // expose neither (sandboxed CI, etc.), in which case the
    // sentinel-existence check returns false and the regenerate
    // path proceeds. That degrades to pre-PR-13 behavior for
    // unknown environments rather than hard-failing every dispatch.
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| std::path::PathBuf::from(h).join(".ato").join(FIRST_RUN_SENTINEL_NAME))
}

fn first_run_sentinel_exists() -> bool {
    first_run_sentinel_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

fn write_first_run_sentinel() {
    if let Some(p) = first_run_sentinel_path() {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Best-effort; a failed sentinel write does NOT block the
        // master-key generate path. Worst case is the protection
        // doesn't trip on a subsequent ACL-mask, which is the same
        // pre-PR-13 behavior.
        let _ = std::fs::write(&p, b"1\n");
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
    // 2026-05-15 — CRITICAL FIX (limited): previous shape treated
    // every error from get_password() as "no key exists, generate."
    // That collapsed NoEntry, PlatformFailure, BadEncoding, etc. all
    // into the regenerate path and silently orphaned every existing
    // llm_api_keys ciphertext. Fixed: only NoEntry triggers regen,
    // everything else fails loud.
    //
    // 2026-05-17 PR 13 — sufficient fix: macOS returns
    // errSecItemNotFound (→ NoEntry) on ACL-mask too, so NoEntry
    // alone is still ambiguous. The first-run sentinel file
    // (`~/.ato/.master_key_initialized`) disambiguates: present
    // means the user has previously initialized → ACL mask, NEVER
    // regenerate; absent means true first-run.
    match entry.get_password() {
        Ok(b64) => {
            // PR 13 migration step — a successful read on an install
            // that has no sentinel yet means the user is already
            // initialized (entry exists, ACL grants access). Write
            // the sentinel so future ACL-mask events (e.g., a dev-
            // build CLI on the same machine) trip the refuse-to-
            // regen branch below. Idempotent: if the file already
            // exists, write_first_run_sentinel() just rewrites it.
            if !first_run_sentinel_exists() {
                write_first_run_sentinel();
            }
            decode_key_b64(&b64)
        }
        Err(keyring::Error::NoEntry) => {
            if first_run_sentinel_exists() {
                Err(anyhow!(
                    "keychain returned NoEntry for {}/{} BUT the first-run sentinel at \
                     `~/.ato/{}` exists. This almost always means the keychain entry was \
                     created by a different code-signed binary (e.g., the production Apple-\
                     Developer-signed desktop) and the current binary (adhoc-signed dev build, \
                     or a freshly re-signed install with a different Designated Requirement) \
                     does not have ACL access. Generating a new master key here would orphan \
                     every existing llm_api_keys ciphertext under the previous one.\n\
                     \n\
                     Fix: copy the master key out of the OS keychain into the env var \
                     ATO_MASTER_KEY_B64 for this process. On macOS:\n\
                     `export ATO_MASTER_KEY_B64=\"$(security find-generic-password -s {} -a {} -w)\"`\n\
                     \n\
                     If you genuinely want to start fresh and re-enter all API keys, delete the \
                     sentinel file (`rm ~/.ato/{}`) AND the keychain entry, then re-run.",
                    KEYCHAIN_SERVICE,
                    MASTER_KEY_ACCOUNT,
                    FIRST_RUN_SENTINEL_NAME,
                    KEYCHAIN_SERVICE,
                    MASTER_KEY_ACCOUNT,
                    FIRST_RUN_SENTINEL_NAME,
                ))
            } else {
                let mut new_key = [0u8; 32];
                rand::rngs::OsRng.fill_bytes(&mut new_key);
                let b64 = general_purpose::STANDARD.encode(new_key);
                entry
                    .set_password(&b64)
                    .context("keyring set_password (master key)")?;
                let final_b64 = entry
                    .get_password()
                    .context("keyring get_password (master key, post-set)")?;
                let key = decode_key_b64(&final_b64)?;
                // Sentinel write is the last step — only marks the
                // run as "initialized" once the key is verifiably
                // round-trippable from the keychain.
                write_first_run_sentinel();
                Ok(key)
            }
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
