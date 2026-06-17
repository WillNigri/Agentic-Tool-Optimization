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
/// v2.14.3 — fallback used only when master_key_ledger is empty
/// (truly fresh install before the schema migration writes the v1 backfill).
/// In every other case the active account is read from the ledger row whose
/// `retired_at IS NULL`. Mirrors apps/desktop/src-tauri/src/encryption.rs.
const MASTER_KEY_ACCOUNT_FALLBACK: &str = "master_key_v1";
const VERSION_PREFIX: &str = "v1:";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
/// v2.14.3 — same fixed plaintext as desktop. Mirrored so a key written
/// by desktop can be validated by CLI rekey paths (and vice-versa).
pub(crate) const CANARY_PLAINTEXT: &str = "ATO_MASTER_KEY_CANARY_v1";

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
// reason to hit the keychain more than once.
//
// v2.14.3 cache shape change: `OnceLock<[u8; 32]>` → `OnceLock<RwLock<Option<(String, [u8; 32])>>>`.
// Same rationale as the desktop mirror — the old shape couldn't be
// invalidated, so a rekey + same-process re-use returned a stale key.
// The (account, key) tuple lets us detect cache/ledger drift.
//
// Cache is per-process — a new `ato dispatch` invocation starts cold,
// which is correct (macOS code-signature ACL is the right enforcement
// boundary across processes; in-process caching shouldn't bypass it).
static MASTER_KEY_CACHE: std::sync::OnceLock<std::sync::RwLock<Option<(String, [u8; 32])>>> =
    std::sync::OnceLock::new();

fn cache() -> &'static std::sync::RwLock<Option<(String, [u8; 32])>> {
    MASTER_KEY_CACHE.get_or_init(|| std::sync::RwLock::new(None))
}

/// v2.14.3 — same shape as desktop. CLI doesn't run rekey itself but
/// this is exposed for the master-key export + tests that exercise the
/// cache lifecycle.
#[allow(dead_code)]
pub fn invalidate_master_key_cache() {
    if let Some(lock) = MASTER_KEY_CACHE.get() {
        if let Ok(mut w) = lock.write() {
            *w = None;
        }
    }
}

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
    // v2.15.0 — same cache-hit-revalidates-ledger pattern as the
    // desktop mirror, ported per war_room C75F743A codex follow-up:
    // the CLI shares the same race invariant. A cached key MUST only
    // short-circuit when its account name still matches the current
    // active ledger row. Otherwise drop to full resolution so the new
    // post-rekey account is honored.
    let cached_check = cache()
        .read()
        .map_err(|e| anyhow!("cache read lock: {}", e))?;
    let active_account = read_active_master_key_account().ok();
    if let (Some((cached_account, key)), Some(current_account)) =
        (cached_check.as_ref(), active_account.as_ref())
    {
        if cached_account == current_account {
            return Ok(*key);
        }
    }
    drop(cached_check);

    let (account, key) = master_key_resolve()?;
    let _ = ensure_canary_initialized(&account, &key);
    if let Ok(mut w) = cache().write() {
        *w = Some((account, key));
    }
    Ok(key)
}

fn master_key_resolve() -> Result<(String, [u8; 32])> {
    // 2026-05-17 — dev-mode bypass. Unsigned local builds (cargo build
    // --release on a dev machine) produce a fresh code signature on
    // every rebuild, which macOS keychain treats as "a new app".
    if let Ok(b64) = std::env::var("ATO_MASTER_KEY_B64") {
        let trimmed = b64.trim();
        if !trimmed.is_empty() {
            let key = decode_key_b64(trimmed)?;
            return Ok((MASTER_KEY_ACCOUNT_FALLBACK.to_string(), key));
        }
    }

    let account = read_active_master_key_account()
        .unwrap_or_else(|_| MASTER_KEY_ACCOUNT_FALLBACK.to_string());

    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel::<std::result::Result<[u8; 32], String>>();
    let account_for_thread = account.clone();

    std::thread::spawn(move || {
        let result = master_key_inner(&account_for_thread).map_err(|e| format!("{:#}", e));
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_secs(KEYCHAIN_TIMEOUT_SECS)) {
        Ok(Ok(key)) => Ok((account, key)),
        Ok(Err(s)) => Err(anyhow!("{}", s)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(anyhow!(
            "keychain access timed out after {}s — macOS is likely showing a Keychain Access permission dialog \
             (the first read after a new binary build needs explicit approval). \
             Approve the dialog if visible (use 'Always Allow' so future dispatches don't re-prompt). \
             To bypass the keychain for this run, copy the master key from the OS keychain into the env var ATO_MASTER_KEY_B64: \
             `export ATO_MASTER_KEY_B64=\"$(security find-generic-password -s {} -a {} -w)\"`. \
             Provider API-key env vars (GEMINI_API_KEY, MINIMAX_API_KEY, ANTHROPIC_API_KEY, ...) only help if you also have those keys handy — the ATO_MASTER_KEY_B64 path is the real bypass.",
            KEYCHAIN_TIMEOUT_SECS,
            KEYCHAIN_SERVICE,
            account
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!(
            "keychain reader thread disconnected without sending a result — unexpected; report as a bug"
        )),
    }
}

/// v2.14.3 — read the active master-key account from `master_key_ledger`.
/// Returns the keychain account name from the row where `retired_at IS NULL`.
/// Errors when the ledger is empty or schema-missing; callers fall back to
/// the legacy account name for fresh installs.
fn read_active_master_key_account() -> Result<String> {
    let db_path = crate::db::default_db_path();
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .with_context(|| format!("open db {} for ledger", db_path.display()))?;
    let s: String = conn
        .query_row(
            "SELECT keychain_account FROM master_key_ledger
                WHERE retired_at IS NULL
             ORDER BY created_at DESC
                LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .with_context(|| "read master_key_ledger active row")?;
    Ok(s)
}

/// v2.14.3 — write the encrypted canary on the active ledger row if missing.
/// Best-effort; failures are ignored so the next master_key() call retries.
fn ensure_canary_initialized(account: &str, key: &[u8; 32]) -> Result<()> {
    let db_path = crate::db::default_db_path();
    let conn = rusqlite::Connection::open(&db_path)
        .with_context(|| format!("open db {} for canary init", db_path.display()))?;

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

    let ct = encrypt_with_key_internal(CANARY_PLAINTEXT, key)?;
    conn.execute(
        "UPDATE master_key_ledger
             SET canary_ciphertext = ?1
           WHERE keychain_account = ?2 AND retired_at IS NULL",
        rusqlite::params![ct, account],
    )
    .with_context(|| "write canary ciphertext")?;
    Ok(())
}

/// CLI-side helper mirroring desktop's `encrypt_with_key`. Pure crypto,
/// no keychain access. Used by the canary init path.
fn encrypt_with_key_internal(plaintext: &str, key_bytes: &[u8; 32]) -> Result<String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Key, Nonce};
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_bytes));
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

fn master_key_inner(account: &str) -> Result<[u8; 32]> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)
        .with_context(|| {
            format!(
                "open keyring entry {}/{}",
                KEYCHAIN_SERVICE, account
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
                    account,
                    FIRST_RUN_SENTINEL_NAME,
                    KEYCHAIN_SERVICE,
                    account,
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

// ── v2.15.x heal-orphans helpers ──────────────────────────────────────────
//
// These exist for `ato master-key heal-orphans` (commands::master_key) —
// the cleanup tool that walks llm_api_keys for rows whose key_version
// disagrees with the active ledger row, decrypts each under its
// original keychain account, and re-encrypts under the active one.
// The bug they recover from (cross-process stale-cache during a dev-
// build save that pre-dated f740381's revalidation rework, 2026-06-11)
// is fixed in the read path; these helpers are the one-shot data-side
// migration so users don't have to re-enter every key.
//
// They are intentionally NOT public to the rest of the CLI: only the
// heal-orphans subcommand should reach for a specific-account key,
// because the rest of the dispatch path is supposed to flow through
// `master_key()` so the ledger stays the source of truth.

/// R1 codex #1 fix — strict read-only keychain fetch for heal-orphans.
///
/// `master_key_inner()` has TWO side effects that are wrong for a
/// repair tool:
///   (a) on a successful read it writes the first-run sentinel if
///       missing — so a heal-orphans dry-run could create the
///       sentinel on a fresh machine and silently change behavior of
///       future regenerate paths;
///   (b) on `NoEntry` without a sentinel it GENERATES a new key and
///       stores it — heal-orphans would then "decrypt" with a
///       fabricated key (it wouldn't match, so it'd report failure;
///       but the keychain side-effect of storing a brand new bogus
///       account entry under the retired name is worse than a clear
///       skip).
///
/// This helper does neither. It reads the keychain entry if present,
/// returns `Ok(None)` on `NoEntry` (so the caller can skip with a
/// clear reason), and never touches the sentinel.
///
/// Uses the same thread-channel timeout shape as master_key_resolve()
/// so a hung Keychain Access dialog cannot deadlock the caller.
pub(crate) fn read_keychain_key_for_account_readonly(
    account: &str,
) -> Result<Option<[u8; 32]>> {
    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel::<std::result::Result<Option<[u8; 32]>, String>>();
    let account_for_thread = account.to_string();
    std::thread::spawn(move || {
        let result = (|| -> Result<Option<[u8; 32]>> {
            let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &account_for_thread)
                .with_context(|| {
                    format!(
                        "open keyring entry {}/{}",
                        KEYCHAIN_SERVICE, account_for_thread
                    )
                })?;
            match entry.get_password() {
                Ok(b64) => Ok(Some(decode_key_b64(&b64)?)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(anyhow!("keyring get_password failed: {}", e)),
            }
        })()
        .map_err(|e| format!("{:#}", e));
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(KEYCHAIN_TIMEOUT_SECS)) {
        Ok(Ok(opt)) => Ok(opt),
        Ok(Err(s)) => Err(anyhow!("{}", s)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(anyhow!(
            "keychain read for account {}/{} timed out after {}s",
            KEYCHAIN_SERVICE,
            account,
            KEYCHAIN_TIMEOUT_SECS
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(anyhow!("keychain reader thread disconnected unexpectedly"))
        }
    }
}

/// Decrypt a v1-prefixed ciphertext with an arbitrary key. Used by
/// heal-orphans to decrypt rows under the RETIRED master-key account
/// before re-encrypting under the active one. Returns InvalidTag (as
/// "v1 decrypt failed") on key mismatch — the caller treats that as
/// "this orphan doesn't decrypt under this candidate, try the next."
pub(crate) fn decrypt_v1_with_key(stored: &str, key: &[u8; 32]) -> Result<String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Key, Nonce};
    let b64 = stored
        .strip_prefix(VERSION_PREFIX)
        .ok_or_else(|| anyhow!("not a v1-prefixed ciphertext"))?;
    let bytes = general_purpose::STANDARD
        .decode(b64.trim())
        .context("decode v1 payload")?;
    if bytes.len() < NONCE_LEN + TAG_LEN {
        return Err(anyhow!("v1 payload too short ({} bytes)", bytes.len()));
    }
    let (nonce_bytes, ciphertext) = bytes.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| anyhow!("v1 decrypt failed (key mismatch)"))?;
    String::from_utf8(plaintext).context("v1 plaintext not utf-8")
}

/// Return the active ledger version (`v1`, `v2`, …) from the caller's
/// `Connection`. Used by heal-orphans to decide which rows are
/// orphans. R1 codex #3 fix — was opening default_db_path
/// unconditionally, which broke `ato --db /other.db master-key
/// heal-orphans` (candidates from one file, ledger metadata from
/// another).
pub(crate) fn read_active_master_key_version_from(
    conn: &rusqlite::Connection,
) -> Result<String> {
    let v: String = conn
        .query_row(
            "SELECT version FROM master_key_ledger
                WHERE retired_at IS NULL
             ORDER BY created_at DESC
                LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .with_context(|| "read active master_key_ledger version")?;
    Ok(v)
}

/// Look up the ACTIVE (retired_at IS NULL) keychain account name
/// using the caller's `Connection`. R2 codex finding — the WRITE
/// path of heal-orphans called encrypt() which used the cached
/// master_key() that internally reads default_db_path(). Heal now
/// resolves the active account from the caller's conn instead.
pub(crate) fn read_active_master_key_account_from(
    conn: &rusqlite::Connection,
) -> Result<String> {
    let s: String = conn
        .query_row(
            "SELECT keychain_account FROM master_key_ledger
                WHERE retired_at IS NULL
             ORDER BY created_at DESC
                LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .with_context(|| "read active master_key_ledger account")?;
    Ok(s)
}

/// Encrypt plaintext under an explicit key. Bypasses the cached
/// master_key() resolution entirely. R2 codex finding — heal-orphans
/// re-encryption uses this so the WRITE path stays under the same
/// DB+keychain the READ path uses, instead of falling back to
/// default_db_path() through encrypt().
pub(crate) fn encrypt_v1_with_key(
    plaintext: &str,
    key_bytes: &[u8; 32],
) -> Result<String> {
    encrypt_with_key_internal(plaintext, key_bytes)
}

/// Look up the keychain account name for a specific ledger version
/// using the caller's `Connection`. Same R1 codex #3 rationale —
/// callsite-controlled DB.
pub(crate) fn keychain_account_for_version_from(
    conn: &rusqlite::Connection,
    version: &str,
) -> Result<String> {
    let s: String = conn
        .query_row(
            "SELECT keychain_account FROM master_key_ledger WHERE version = ?1",
            [version],
            |r| r.get::<_, String>(0),
        )
        .with_context(|| format!("read ledger row for version {}", version))?;
    Ok(s)
}
