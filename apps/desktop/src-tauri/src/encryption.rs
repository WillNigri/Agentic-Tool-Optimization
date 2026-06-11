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
/// v2.14.3 — fallback account name used ONLY when master_key_ledger is empty
/// (truly fresh install before the schema migration writes the v1 backfill).
/// In every other case the active account is read from the ledger row whose
/// `retired_at IS NULL`. See `read_active_master_key_account` below.
const MASTER_KEY_ACCOUNT_FALLBACK: &str = "master_key_v1";
const VERSION_PREFIX: &str = "v1:";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
/// v2.14.3 — fixed plaintext stored encrypted on the active ledger row.
/// Used by `rekey.rs` to prove a candidate "old key" actually corresponds
/// to the master key that wrote the existing ciphertexts. Distinctive +
/// short so a chance match against random bytes is astronomically unlikely.
pub(crate) const CANARY_PLAINTEXT: &str = "ATO_MASTER_KEY_CANARY_v1";

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
///
/// 2026-05-16 — wrap in a hard timeout. macOS shows a Keychain Access
/// permission dialog the first time a new binary signature reads this
/// entry. In headless contexts (background workers, mesh-relay daemons,
/// CLI-subprocess-of-self in demo-compare) the dialog can't be approved
/// and the read hangs forever. Bug #48 dogfooded this directly. The
/// 8-second timeout lets an interactive user approve the dialog while
/// also failing fast in headless contexts.
const KEYCHAIN_TIMEOUT_SECS: u64 = 8;

// 2026-05-17 — process-memory cache + dev-mode env-var bypass.
// Reasons for both: (a) macOS keychain re-prompts on every binary
// signature change, including unsigned dev rebuilds — even after
// "Always Allow"; (b) within a single process there's no reason to
// pay the keychain round-trip on every call.
//
// v2.14.3 cache shape change: `OnceLock<[u8; 32]>` → `OnceLock<RwLock<Option<(String, [u8; 32])>>>`.
// The old shape couldn't be invalidated after rekey, so an in-process
// post-rekey encrypt() would return the OLD key (codex war-room verdict
// 2026-06-10 518FBBA2). The new shape lets `invalidate_master_key_cache`
// clear the slot when rekey commits, and the (account, key) tuple lets
// us detect cache/ledger drift on every read.
static MASTER_KEY_CACHE: std::sync::OnceLock<std::sync::RwLock<Option<(String, [u8; 32])>>> =
    std::sync::OnceLock::new();

fn cache() -> &'static std::sync::RwLock<Option<(String, [u8; 32])>> {
    MASTER_KEY_CACHE.get_or_init(|| std::sync::RwLock::new(None))
}

/// v2.14.3 — invalidate the in-process master-key cache. Called by
/// `rekey.rs` after a successful rekey commits, so the next `encrypt`
/// / `decrypt` re-reads the active ledger row + keychain entry instead
/// of returning the stale pre-rekey key.
pub fn invalidate_master_key_cache() {
    if let Some(lock) = MASTER_KEY_CACHE.get() {
        if let Ok(mut w) = lock.write() {
            *w = None;
        }
    }
}

fn master_key() -> Result<[u8; 32], String> {
    // v2.15.0 REWORK from war_room 2EAAE58B (codex finding #1):
    // Cache hit MUST re-validate the cached account against the current
    // active ledger row. Pre-rework, a thread holding a cached key from
    // before a rekey commit would return that stale key indefinitely
    // (cache invalidation only protects FUTURE reads; in-flight calls
    // that already passed the cache-hit check kept using the old key).
    // Now: cache hit only short-circuits when the cached account name
    // STILL matches what the ledger says is active. If the ledger has
    // moved on (post-rekey), we drop to the cache-miss path which
    // re-reads keychain under the new account.
    let cached_check = cache().read().map_err(|e| format!("cache read lock: {}", e))?;
    let active_account = read_active_master_key_account().ok();
    if let (Some((cached_account, key)), Some(current_account)) =
        (cached_check.as_ref(), active_account.as_ref())
    {
        if cached_account == current_account {
            return Ok(*key);
        }
    }
    // Drop the read lock before any I/O — the env-bypass path and the
    // ATO_MASTER_KEY_B64 short-circuit below would self-deadlock if a
    // write tried to take the lock with us still holding read.
    drop(cached_check);

    // Cache miss (or stale cache from a rekey) — full resolution.
    let (account, key) = master_key_resolve()?;
    // Best-effort canary init for this ledger row. Runs once per process
    // per rekey; idempotent on the SQL side.
    let _ = ensure_canary_initialized(&account, &key);
    // Populate cache.
    if let Ok(mut w) = cache().write() {
        *w = Some((account, key));
    }
    Ok(key)
}

/// Resolve the current master key by reading the active ledger row and
/// fetching from the keychain under the account name it declares. Returns
/// `(account_name, key_bytes)` so the cache can be keyed by account.
fn master_key_resolve() -> Result<(String, [u8; 32]), String> {
    // Dev-mode bypass — unsigned local builds skip the keychain
    // entirely. Production releases (signed Apple Developer cert) never
    // set this env var and go through the normal keychain path.
    if let Ok(b64) = std::env::var("ATO_MASTER_KEY_B64") {
        let trimmed = b64.trim();
        if !trimmed.is_empty() {
            let key = decode_key_b64(trimmed)?;
            // Env bypass = no ledger lookup; use the fallback account
            // string just as the cache key. The ledger isn't consulted.
            return Ok((MASTER_KEY_ACCOUNT_FALLBACK.to_string(), key));
        }
    }

    let account = read_active_master_key_account()
        .unwrap_or_else(|_| MASTER_KEY_ACCOUNT_FALLBACK.to_string());

    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel::<Result<[u8; 32], String>>();
    let account_for_thread = account.clone();

    std::thread::spawn(move || {
        let _ = tx.send(master_key_inner(&account_for_thread));
    });

    match rx.recv_timeout(Duration::from_secs(KEYCHAIN_TIMEOUT_SECS)) {
        Ok(result) => result.map(|k| (account, k)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "keychain access timed out after {}s — macOS is likely showing a Keychain Access permission dialog \
             (the first read after a new binary build needs explicit approval). \
             Approve the dialog if visible ('Always Allow' so future opens don't re-prompt). \
             To bypass the keychain for this run, set ATO_MASTER_KEY_B64 in the env to the value of the keychain entry \
             (`security find-generic-password -s {} -a {} -w` on macOS). \
             Setting per-provider API-key env vars (GEMINI_API_KEY, MINIMAX_API_KEY, ANTHROPIC_API_KEY, ...) only sidesteps the keychain if you have those keys available.",
            KEYCHAIN_TIMEOUT_SECS,
            KEYCHAIN_SERVICE,
            account
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(
            "keychain reader thread disconnected without sending a result".to_string(),
        ),
    }
}

/// v2.14.3 — read the active master-key account from `master_key_ledger`.
/// "Active" = the row with `retired_at IS NULL`. If multiple match (should
/// never happen but is non-fatal), take the most recent. Failure means the
/// ledger table is missing or empty; callers fall back to the legacy
/// account name (`master_key_v1`) so brand-new installs still work.
fn read_active_master_key_account() -> Result<String, String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open db for ledger: {}", e))?;
    conn.query_row(
        "SELECT keychain_account FROM master_key_ledger
            WHERE retired_at IS NULL
         ORDER BY created_at DESC
            LIMIT 1",
        [],
        |r| r.get::<_, String>(0),
    )
    .map_err(|e| format!("read master_key_ledger active row: {}", e))
}

/// v2.14.3 — write the encrypted canary on the active ledger row if
/// missing. Best-effort: write failures (DB locked, schema drift) are
/// logged and ignored so the next master_key() call retries. The canary
/// lets rekey verify it has the right "old key" before destructive ops.
fn ensure_canary_initialized(account: &str, key: &[u8; 32]) -> Result<(), String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open db for canary init: {}", e))?;

    let current: Option<String> = conn
        .query_row(
            "SELECT canary_ciphertext FROM master_key_ledger
               WHERE keychain_account = ?1 AND retired_at IS NULL",
            [account],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap_or(None);

    if current.is_some() {
        return Ok(());
    }

    let ct = encrypt_with_key(CANARY_PLAINTEXT, key)?;
    conn.execute(
        "UPDATE master_key_ledger
             SET canary_ciphertext = ?1
           WHERE keychain_account = ?2 AND retired_at IS NULL",
        rusqlite::params![ct, account],
    )
    .map_err(|e| format!("write canary ciphertext: {}", e))?;
    Ok(())
}

/// v2.14.3 — read the encrypted canary for the active ledger row. Used by
/// `rekey.rs` precondition: the candidate "old key" must decrypt this to
/// `CANARY_PLAINTEXT` exactly, or the rekey aborts without touching keychain
/// or ciphertext columns.
pub fn read_active_canary_ciphertext() -> Result<Option<String>, String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open db for canary read: {}", e))?;
    let v: Option<String> = conn
        .query_row(
            "SELECT canary_ciphertext FROM master_key_ledger
               WHERE retired_at IS NULL
            ORDER BY created_at DESC
               LIMIT 1",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .map_err(|e| format!("read canary: {}", e))?;
    Ok(v)
}

/// v2.14.3 — write the encrypted canary onto a specific ledger row.
/// Called by `rekey.rs` after a successful rekey commits, so the new
/// active row holds the canary under the NEW master key. Same
/// best-effort posture as the init path.
pub fn write_canary_for_account(account: &str, key: &[u8; 32]) -> Result<(), String> {
    let db_path = crate::get_db_path();
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open db for canary write: {}", e))?;
    let ct = encrypt_with_key(CANARY_PLAINTEXT, key)?;
    conn.execute(
        "UPDATE master_key_ledger
             SET canary_ciphertext = ?1
           WHERE keychain_account = ?2",
        rusqlite::params![ct, account],
    )
    .map_err(|e| format!("write canary: {}", e))?;
    Ok(())
}

/// PR 13 (2026-05-17) — first-run sentinel. See the CLI mirror at
/// `apps/cli/src/encryption.rs` for the full design rationale. Short
/// version: macOS keychain returns errSecItemNotFound (→ NoEntry)
/// for both "no row at all" AND "ACL-masked from this binary." The
/// 2026-05-15 fix limited regeneration to NoEntry, which is
/// necessary but not sufficient — an ACL-masked NoEntry still hit
/// the regenerate path. The sentinel file at
/// `~/.ato/.master_key_initialized` distinguishes the two: present
/// = previously initialized → never regen; absent = true first run.
const FIRST_RUN_SENTINEL_NAME: &str = ".master_key_initialized";

fn first_run_sentinel_path() -> Option<std::path::PathBuf> {
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
        let _ = std::fs::write(&p, b"1\n");
    }
}

fn master_key_inner(account: &str) -> Result<[u8; 32], String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)
        .map_err(|e| format!("keyring entry {}/{}: {}", KEYCHAIN_SERVICE, account, e))?;
    match entry.get_password() {
        Ok(b64) => {
            // PR 13 migration — write the sentinel on the success
            // path so existing installs (entry present + no
            // sentinel) gain protection on the NEXT ACL-mask event.
            // Idempotent; rewrite is fine.
            if !first_run_sentinel_exists() {
                write_first_run_sentinel();
            }
            decode_key_b64(&b64)
        }
        Err(keyring::Error::NoEntry) => {
            // PR 13 — distinguish "truly fresh" from "ACL-masked
            // NoEntry." Sentinel present = previously initialized →
            // refuse to regen and orphan ciphertexts. Sentinel
            // absent = genuine first-run → generate + write sentinel.
            if first_run_sentinel_exists() {
                Err(format!(
                    "keychain returned NoEntry for {}/{} BUT the first-run sentinel at \
                     `~/.ato/{}` exists. The keychain entry was almost certainly created by a \
                     different code-signed binary (a different Designated Requirement on the \
                     entry's ACL list). Generating a new master key here would orphan every \
                     existing llm_api_keys ciphertext.\n\
                     \n\
                     Fix: launch ATO from the Apple-Developer-signed bundle (/Applications/ATO.app) \
                     so the entry's ACL grants access. If you intend a true reset, delete \
                     `~/.ato/{}` AND the keychain entry, then re-launch and re-enter all API keys.",
                    KEYCHAIN_SERVICE,
                    account,
                    FIRST_RUN_SENTINEL_NAME,
                    FIRST_RUN_SENTINEL_NAME,
                ))
            } else {
                // Genuine first-run. Generate + store. Race-window
                // note preserved: if two processes hit this
                // simultaneously we re-read after the write so we
                // use whoever's value ended up in the keychain.
                let mut new_key = [0u8; 32];
                rand::rngs::OsRng.fill_bytes(&mut new_key);
                let b64 = general_purpose::STANDARD.encode(new_key);
                entry
                    .set_password(&b64)
                    .map_err(|e| format!("keyring set_password: {}", e))?;
                let final_b64 = entry
                    .get_password()
                    .map_err(|e| format!("keyring get_password after set: {}", e))?;
                let key = decode_key_b64(&final_b64)?;
                write_first_run_sentinel();
                Ok(key)
            }
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
    encrypt_with_key(plaintext, &key_bytes)
}

/// PR-4 (master_key_v2) — pure crypto, no keychain access. Lets the
/// rekey transaction re-encrypt rows with an EXPLICIT new key
/// without touching the OS keychain inside the transaction. Same
/// AES-256-GCM + nonce format as `encrypt`; only the key source
/// differs.
pub(crate) fn encrypt_with_key(plaintext: &str, key_bytes: &[u8; 32]) -> Result<String, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_bytes));
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
        let key_bytes = master_key()?;
        return decrypt_v1_with_key(b64, &key_bytes);
    }
    // Legacy plain-base64 row. The migration UPDATE that runs after
    // each successful decrypt rewrites these as v1.
    let bytes = general_purpose::STANDARD
        .decode(stored)
        .map_err(|e| format!("legacy decode: {}", e))?;
    String::from_utf8(bytes).map_err(|e| format!("legacy plaintext not utf-8: {}", e))
}

/// PR-4 (master_key_v2) — pure crypto for re-keying. Accepts an
/// explicit master key (the OLD key for decrypt, then the NEW key
/// for re-encrypt) so the rekey transaction can swap keys per row
/// without touching the OS keychain inside the SQLite transaction.
/// Takes only the v1: payload bytes (caller strips the prefix).
pub(crate) fn decrypt_v1_with_key(
    b64_payload: &str,
    key_bytes: &[u8; 32],
) -> Result<String, String> {
    let bytes = general_purpose::STANDARD
        .decode(b64_payload.trim())
        .map_err(|e| format!("decode v1 payload: {}", e))?;
    if bytes.len() < NONCE_LEN + TAG_LEN {
        return Err(format!(
            "v1 payload too short ({} bytes, need ≥{})",
            bytes.len(),
            NONCE_LEN + TAG_LEN
        ));
    }
    let (nonce_bytes, ciphertext) = bytes.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_bytes));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| {
            "v1 decrypt failed — wrong master key (re-key paste mismatch?)".to_string()
        })?;
    String::from_utf8(plaintext).map_err(|e| format!("v1 plaintext not utf-8: {}", e))
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
