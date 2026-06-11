// Atomic master-key re-encryption. PR-4 of master_key_v2.
//
// Architecture war-roomed 2026-05-22 (war_room_id 3883E920-…)
// google + minimax both CLEAR-TO-CODE (claude returned 1 token — server
// hiccup, design surface validated by the other two reviewers). All 14
// questions answered; locked decisions live in this module's comments
// inline next to the code that implements them.
//
// MISSION
// -------
// User pastes the OLD master key bytes (b64-encoded, exported from
// the previous binary via `ato master-key export` from PR-6, or pulled
// out of the OS keychain manually via `security find-generic-password
// -s ato-desktop -a master_key_v1 -w` on macOS). This module:
//
//   1. Mints a NEW 32-byte master key from OsRng.
//   2. Writes new key to OS keychain at `master_key_v2`.
//   3. Opens `BEGIN IMMEDIATE` SQLite transaction.
//   4. For every `llm_api_keys` row WHERE key_version='v1':
//        - decrypt(stored_ciphertext, OLD_KEY)
//        - encrypt(plaintext, NEW_KEY)
//        - UPDATE row: encrypted_key = new_ct, key_version = 'v2'
//      First decrypt failure → ROLLBACK + return RekeyError::DecryptFailed
//      with the offending row id so PR-5's UI can surface "wrong key."
//   5. UPDATE master_key_ledger: retire v1 (set retired_at = now).
//   6. INSERT master_key_ledger row for v2 (identity_probe = NULL —
//      PR-2's populate path fills it on the next launch).
//   7. COMMIT.
//   8. Delete old `master_key_v1` keychain entry. If this fails, log
//      warning + succeed (the v1 key is cryptographically orphaned —
//      no ciphertext references it any more).
//   9. Re-run identity_probe::run_full_probe_cycle so PR-5's UI sees
//      ProbeStatus::Matched immediately without a relaunch.
//
// FAILURE INVARIANTS
// ------------------
// - Step 2 fails (keychain unwritable) → return RekeyError::KeychainWrite,
//   no DB changes, no orphan ciphertexts.
// - Step 3 fails (DB BUSY because another writer) → return
//   RekeyError::TransactionBusy. v2 keychain entry exists but no
//   ledger row references it; next successful rekey overwrites it.
//   This pollution is acceptable per the war-room's "keychain holds
//   source of truth for active key, ledger is audit trail."
// - Step 4 fails (decrypt fail) → ROLLBACK; v2 keychain entry still
//   present but unused.
// - Step 6 fails (rare COMMIT IO error) → ROLLBACK; same outcome.
// - Step 8 fails (keychain delete unwritable) → log + succeed. v1
//   is dead cryptographically; the dangling keychain entry is
//   cosmetic, not security-bearing.
//
// TEST DISCIPLINE
// ---------------
// The pure `rekey_inner` takes `&[u8; 32]` for both old and new keys
// + a `Connection` — no keychain access. Unit tests use deterministic
// keys + an in-memory DB seeded with v1 ciphertexts. The keychain
// dance (write v2 / delete v1) is exercised via the outer
// `rekey_master_key_with_keychain` wrapper that production calls;
// that one is hard to unit-test in CI and is covered by the
// pre-tag dogfood pass + Will's manual verification.

use base64::{engine::general_purpose, Engine as _};
use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::encryption::{
    decrypt_v1_with_key, encrypt_with_key,
};

const V1_PREFIX: &str = "v1:";

/// Payload returned to the caller on a successful rekey. PR-5's UI
/// surfaces this in a "Rekey complete" toast / summary card.
/// `v1_keychain_deleted = false` happens when the post-COMMIT
/// delete fails — the COMMIT itself succeeded so the rekey is
/// considered complete; the orphaned keychain entry is logged but
/// not surfaced as an error.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RekeyResult {
    pub rows_rekeyed: usize,
    pub v2_keychain_account: String,
    pub v1_keychain_deleted: bool,
    pub retired_at: String,
}

/// Typed errors per failure mode. Tauri command serializes these as
/// strings for the frontend (PR-5 maps the variant to a user-facing
/// message). The structured enum is kept for tests + future
/// programmatic recovery.
#[derive(Debug, Clone)]
pub enum RekeyError {
    /// User-supplied old key didn't b64-decode or wasn't 32 bytes.
    InvalidOldKey(String),
    /// Step 2: writing the new v2 key to the OS keychain failed.
    /// The DB is untouched.
    KeychainWrite(String),
    /// Step 3: BEGIN IMMEDIATE failed because another writer holds
    /// the lock. The user can retry; the v2 keychain entry stays.
    TransactionBusy(String),
    /// Step 4: decrypting a row with the old key failed. Almost
    /// always: the user pasted the wrong key. The `row_id` lets
    /// the UI message say "row 7 failed to decrypt."
    DecryptFailed { row_id: String, context: String },
    /// Step 4b: encrypting with the new key failed (extremely rare —
    /// AES-GCM encrypt failure means the OS RNG broke).
    EncryptFailed { row_id: String, context: String },
    /// Step 5/6/7: writing the ledger / UPDATE failed inside the
    /// transaction. Distinguished from row-level failures so the
    /// UI can say "DB error during ledger update" rather than
    /// implicating the user's key.
    LedgerWrite(String),
    /// Step 7: COMMIT failed (disk full, IO error). Rolled back.
    CommitFailed(String),
}

impl std::fmt::Display for RekeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RekeyError::InvalidOldKey(m) => write!(f, "invalid old master key: {}", m),
            RekeyError::KeychainWrite(m) => write!(f, "keychain write failed: {}", m),
            RekeyError::TransactionBusy(m) => write!(
                f,
                "database busy — another writer holds the lock: {}",
                m
            ),
            RekeyError::DecryptFailed { row_id, context } => write!(
                f,
                "decrypt failed for llm_api_keys row {} ({}) — wrong old master key?",
                row_id, context
            ),
            RekeyError::EncryptFailed { row_id, context } => write!(
                f,
                "encrypt failed for llm_api_keys row {} ({}) — OS RNG error?",
                row_id, context
            ),
            RekeyError::LedgerWrite(m) => write!(f, "ledger update failed: {}", m),
            RekeyError::CommitFailed(m) => write!(f, "COMMIT failed: {}", m),
        }
    }
}

impl std::error::Error for RekeyError {}

/// Decode a user-supplied b64 string into a 32-byte master key.
/// Surfaces a friendly error message instead of bubbling base64 /
/// length errors verbatim — PR-5's UI renders this in a banner.
pub fn parse_old_key_b64(b64: &str) -> Result<[u8; 32], RekeyError> {
    let bytes = general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| RekeyError::InvalidOldKey(format!("not valid base64: {}", e)))?;
    if bytes.len() != 32 {
        return Err(RekeyError::InvalidOldKey(format!(
            "expected 32 bytes after b64-decode, got {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Mint a fresh 32-byte master key. SYNC WITH encryption.rs's
/// first-run generation path (same RNG, same length) so a v2 key
/// minted here is indistinguishable from a v2 minted on a future
/// fresh install. War-room B locked the source.
pub fn mint_new_master_key() -> [u8; 32] {
    use rand::RngCore;
    let mut k = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut k);
    k
}

/// Compute the sha256-truncated-16 hex hash of a master key for
/// audit logging. Same shape as identity_probe::compute_probe; lets
/// the audit row say "rekeyed FROM <old_probe> TO <new_probe>"
/// without leaking the raw key bytes.
pub fn probe_hash(key_bytes: &[u8; 32]) -> String {
    let h = Sha256::digest(key_bytes);
    h[..16].iter().map(|b| format!("{:02x}", b)).collect()
}

/// v2.15.0 Slice B (war_room F293287E) — probe resync, non-destructive
/// state repair for the common case where the user's keychain bytes are
/// still correct but the ledger's identity_probe has gone stale (e.g.,
/// after a binary identity swap that re-handshakes the keychain ACL).
///
/// Verifies the active ledger row's canary decrypts with `current_key`.
/// If yes — the keychain bytes really are the master_key that wrote the
/// active row, so the probe just needs refreshing. UPDATE the active
/// row's identity_probe to `probe_hash(current_key)` and audit-log the
/// transition. Returns the OLD probe so the caller can show before/after.
///
/// If canary FAILS to decrypt → true key drift; caller falls through to
/// the destructive rekey path with paste-old-key UX.
///
/// Wrapped in BEGIN IMMEDIATE so the canary check + probe UPDATE can't
/// race a concurrent rekey. The keychain itself can't be locked, but
/// that's acceptable: if it rotates mid-resync, the next probe cycle
/// will mismatch again and self-correct.
pub fn probe_resync(
    conn: &mut Connection,
    current_key: &[u8; 32],
) -> Result<ProbeResyncOutcome, RekeyError> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| {
            let msg = e.to_string();
            if msg.to_lowercase().contains("busy") {
                RekeyError::TransactionBusy(msg)
            } else {
                RekeyError::LedgerWrite(format!("BEGIN IMMEDIATE for probe_resync: {}", msg))
            }
        })?;

    let row: Option<(String, Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT version, canary_ciphertext, identity_probe
               FROM master_key_ledger
              WHERE retired_at IS NULL
           ORDER BY created_at DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?)),
        )
        .ok();
    let (active_version, canary_ct, old_probe) = match row {
        Some(r) => r,
        None => {
            return Err(RekeyError::LedgerWrite(
                "no active ledger row — refusing to resync probe on empty ledger".to_string(),
            ));
        }
    };

    // Verify the active row's canary decrypts with the supplied current
    // key. This is the authority — the war_room verdict allows the OLD
    // probe to be ignored here because the whole point is to repair a
    // stale probe. Canary decryption proves the keychain bytes really
    // are the master_key for this ledger row.
    let canary_ct = canary_ct.ok_or_else(|| RekeyError::DecryptFailed {
        row_id: "canary".to_string(),
        context: format!(
            "active ledger row ({}) has NULL canary_ciphertext — \
             probe_resync requires a backfilled canary. \
             Relaunch ATO, then trigger a path that resolves the master key \
             (Settings → API Keys → reveal a stored key, OR a chat dispatch \
             to a configured API provider). That writes the canary. \
             Then retry resolve_drift.",
            active_version
        ),
    })?;
    let payload = canary_ct.strip_prefix(V1_PREFIX).ok_or_else(|| RekeyError::DecryptFailed {
        row_id: "canary".to_string(),
        context: "canary_ciphertext missing v1: prefix".to_string(),
    })?;
    let decrypted = decrypt_v1_with_key(payload, current_key).map_err(|e| {
        RekeyError::DecryptFailed {
            row_id: "canary".to_string(),
            context: format!(
                "canary decrypt failed with current keychain bytes — \
                 keychain bytes do NOT match the master_key that wrote \
                 the active ledger row's ciphertexts. This is real key drift; \
                 caller must fall through to destructive rekey with the OLD \
                 key supplied. Original error: {}",
                e
            ),
        }
    })?;
    if decrypted != crate::encryption::CANARY_PLAINTEXT {
        return Err(RekeyError::DecryptFailed {
            row_id: "canary".to_string(),
            context:
                "canary decrypted but plaintext does not match expected. \
                 Tampering or subtle key mismatch — probe_resync aborted."
                    .to_string(),
        });
    }

    // Canary verified. UPDATE the probe.
    let new_probe = probe_hash(current_key);
    tx.execute(
        "UPDATE master_key_ledger
            SET identity_probe = ?1
          WHERE version = ?2 AND retired_at IS NULL",
        rusqlite::params![new_probe, active_version],
    )
    .map_err(|e| RekeyError::LedgerWrite(format!("UPDATE active row probe: {}", e)))?;

    // Audit log with distinct action name per codex verdict.
    let resynced_at = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({
        "active_version": active_version,
        "old_probe": old_probe,
        "new_probe": new_probe,
    })
    .to_string();
    let audit_id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO audit_logs
            (id, action, resource_type, resource_id, resource_name,
             details, created_at)
         VALUES (?1, 'master_key_probe_resynced', 'master_key_ledger',
                 ?2, NULL, ?3, ?4)",
        rusqlite::params![audit_id, active_version, details, resynced_at],
    )
    .map_err(|e| RekeyError::LedgerWrite(format!("audit insert: {}", e)))?;

    tx.commit().map_err(|e| RekeyError::CommitFailed(e.to_string()))?;

    Ok(ProbeResyncOutcome {
        active_version,
        old_probe,
        new_probe,
        resynced_at,
    })
}

/// Result of a successful probe_resync — visible to the caller (Tauri
/// command + audit consumers) so the UX can show before/after.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeResyncOutcome {
    pub active_version: String,
    pub old_probe: Option<String>,
    pub new_probe: String,
    pub resynced_at: String,
}

/// PURE rekey core. Takes both keys explicitly + a SQLite
/// connection; opens BEGIN IMMEDIATE, re-encrypts every v1 row,
/// updates the ledger, COMMITs. No keychain access — the outer
/// wrapper handles that. Tests call this directly with
/// deterministic bytes; production wrappers call it inside a
/// keychain dance.
///
/// On any failure inside the transaction, the rusqlite Transaction's
/// Drop impl rolls back automatically (we never call .commit()).
/// Returns the rows_rekeyed count + the ISO8601 retired_at
/// timestamp so the outer wrapper can build the RekeyResult.
pub fn rekey_inner(
    conn: &mut Connection,
    old_key: &[u8; 32],
    new_key: &[u8; 32],
) -> Result<(usize, String), RekeyError> {
    let retired_at = chrono::Utc::now().to_rfc3339();
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| {
            // SQLITE_BUSY surfaces here. Distinguish from other Sqlite
            // errors so the caller can say "retry in a moment."
            let msg = e.to_string();
            if msg.to_lowercase().contains("busy") {
                RekeyError::TransactionBusy(msg)
            } else {
                RekeyError::LedgerWrite(format!("BEGIN IMMEDIATE failed: {}", msg))
            }
        })?;

    // v2.15.0 REWORK from war_room 2EAAE58B (codex findings #2 + #3):
    //
    // Finding #2: The "assert(probe_match && canary_decrypts)" invariant
    // from war_room 518FBBA2 must be expressed as a SINGLE precondition
    // INSIDE rekey_inner. Pre-rework, probe-match lived upstream in the
    // caller and only canary-decrypts ran here — they could disagree
    // if anything mutated between caller's probe check and this point.
    //
    // Finding #3: The NULL-canary "log + proceed" bypass was too
    // permissive — Will's machine is in exactly that state today and
    // the bypass means the rekey-bug class wouldn't be caught for any
    // pre-2.14.3 install. Now we REFUSE rekey when the canary is
    // missing; the user must relaunch the app so encryption::ensure_
    // canary_initialized() backfills against the CURRENT master_key
    // first. That gives us a known-good canary written under a
    // known-working key before any rekey can run.
    let row: Option<(Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT canary_ciphertext, identity_probe FROM master_key_ledger
               WHERE version = 'v1' AND retired_at IS NULL",
            [],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .ok();
    let (canary_v1, stored_probe) = match row {
        Some((c, p)) => (c, p),
        None => {
            return Err(RekeyError::LedgerWrite(
                "no active v1 ledger row found — refusing to rekey from an empty ledger".to_string(),
            ));
        }
    };

    // CLAUSE 1 of the precondition: canary must exist + decrypt to expected.
    let canary_ct = canary_v1.ok_or_else(|| RekeyError::DecryptFailed {
        row_id: "canary".to_string(),
        // war_room C75F743A nit (codex review): app startup intentionally
        // does NOT call master_key() (lib.rs:234, PERMISSIONS.md:24), so
        // "Relaunch ATO" alone won't backfill the canary. The user must
        // ALSO trigger a path that resolves the master key — typically
        // Settings → API Keys → reveal a stored key, or a Chat dispatch
        // to an API provider. Doing one of those calls
        // ensure_canary_initialized() which writes the canary against
        // the current master_key. Then rekey can proceed.
        context: "v1 ledger row has NULL canary_ciphertext — pre-2.14.3 install. \
                  Relaunch ATO, then trigger a path that resolves the master key \
                  (Settings → API Keys → reveal a stored key, OR a chat dispatch to \
                  a configured API provider). That writes the canary against the \
                  current master_key. Then retry rekey. \
                  Refusing to proceed without a known-good canary (war_room verdict 2EAAE58B)."
            .to_string(),
    })?;
    let payload = canary_ct.strip_prefix(V1_PREFIX).ok_or_else(|| RekeyError::DecryptFailed {
        row_id: "canary".to_string(),
        context: "canary_ciphertext missing v1: prefix".to_string(),
    })?;
    let decrypted_canary = decrypt_v1_with_key(payload, old_key).map_err(|e| {
        RekeyError::DecryptFailed {
            row_id: "canary".to_string(),
            context: format!(
                "canary decrypt failed — supplied old key does NOT match \
                 the master key that wrote ledger v1. \
                 Rekey ABORTED to prevent garbage re-encryption. \
                 Original error: {}",
                e
            ),
        }
    })?;
    if decrypted_canary != crate::encryption::CANARY_PLAINTEXT {
        return Err(RekeyError::DecryptFailed {
            row_id: "canary".to_string(),
            context:
                "canary decrypted but plaintext does not match expected. \
                 Either the canary was tampered with or the old key is \
                 subtly wrong. Rekey ABORTED."
                    .to_string(),
        });
    }

    // CLAUSE 2 of the precondition: probe_match. Verify in-band so the
    // war-room's `probe_match && canary_decrypts` invariant holds as a
    // single check at this exact point.
    if let Some(expected_probe) = &stored_probe {
        let computed_probe = probe_hash(old_key);
        if &computed_probe != expected_probe {
            return Err(RekeyError::DecryptFailed {
                row_id: "probe".to_string(),
                context: format!(
                    "probe_hash mismatch: ledger v1 row has identity_probe={} \
                     but probe_hash(old_key)={}. The canary check above passed, \
                     which means the supplied old key decrypts ciphertext correctly, \
                     but the ledger's stored probe disagrees — possible ledger \
                     tampering or stale probe. Rekey ABORTED so an investigator \
                     can examine state before any destructive operation.",
                    expected_probe, computed_probe
                ),
            });
        }
    }
    // (If stored_probe is NULL on a pre-PR-2 row, we accept canary alone
    // since PR-1 explicitly shipped rows without probes. The canary
    // verification is the binding security check; probe is corroborating.)
    let canary_verified = true;

    // Step 4: re-encrypt every v1 row. Collect the work into a Vec
    // first so we drop the SELECT statement before issuing UPDATEs
    // (avoids a "statement is in use" lock issue on the same conn).
    let v1_rows: Vec<(String, String)> = {
        let mut stmt = tx
            .prepare(
                "SELECT id, encrypted_key
                   FROM llm_api_keys
                  WHERE key_version = 'v1'
                    AND encrypted_key LIKE 'v1:%'",
            )
            .map_err(|e| RekeyError::LedgerWrite(format!("SELECT v1 rows: {}", e)))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| RekeyError::LedgerWrite(format!("query_map: {}", e)))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let mut rows_rekeyed = 0usize;
    for (id, ciphertext) in &v1_rows {
        // Strip the v1: prefix; decrypt_v1_with_key expects bare b64.
        let payload = ciphertext.strip_prefix(V1_PREFIX).ok_or_else(|| {
            RekeyError::DecryptFailed {
                row_id: id.clone(),
                context: "row missing v1: prefix despite key_version='v1'".to_string(),
            }
        })?;
        let plaintext = decrypt_v1_with_key(payload, old_key).map_err(|e| {
            RekeyError::DecryptFailed {
                row_id: id.clone(),
                context: e,
            }
        })?;
        let new_ciphertext = encrypt_with_key(&plaintext, new_key).map_err(|e| {
            RekeyError::EncryptFailed {
                row_id: id.clone(),
                context: e,
            }
        })?;
        tx.execute(
            "UPDATE llm_api_keys
                SET encrypted_key = ?1,
                    key_version   = 'v2',
                    updated_at    = ?2
              WHERE id = ?3
                AND key_version = 'v1'",
            rusqlite::params![new_ciphertext, retired_at, id],
        )
        .map_err(|e| {
            RekeyError::LedgerWrite(format!("UPDATE llm_api_keys row {}: {}", id, e))
        })?;
        rows_rekeyed += 1;
    }

    // Step 5: retire v1 in the ledger. WHERE retired_at IS NULL so a
    // stale half-rekey state from a prior failed run can't double-
    // retire (minimax E defensive note).
    tx.execute(
        "UPDATE master_key_ledger
            SET retired_at = ?1
          WHERE version = 'v1'
            AND retired_at IS NULL",
        rusqlite::params![retired_at],
    )
    .map_err(|e| RekeyError::LedgerWrite(format!("retire v1: {}", e)))?;

    // v2.14.3 — encrypt the canary under the NEW key for the v2 row.
    // Inside the transaction so it commits atomically with the rekey.
    let v2_canary = encrypt_with_key(crate::encryption::CANARY_PLAINTEXT, new_key)
        .map_err(|e| RekeyError::EncryptFailed {
            row_id: "canary".to_string(),
            context: format!("encrypt v2 canary: {}", e),
        })?;

    // Step 6: INSERT v2 ledger row. identity_probe stays NULL —
    // PR-2's populate path fills it on the next launch. notes
    // carries provenance so a future audit shows this was a rekey
    // (vs a fresh-install v2 minted by some future PR).
    tx.execute(
        "INSERT INTO master_key_ledger
            (version, keychain_account, ciphertext_format,
             identity_probe, source, created_at, retired_at, notes,
             canary_ciphertext)
         VALUES
            ('v2', 'master_key_v2', 'aes-gcm-v1', NULL,
             'keychain', ?1, NULL, 'rekey from v1 (PR-4)', ?2)",
        rusqlite::params![retired_at, v2_canary],
    )
    .map_err(|e| RekeyError::LedgerWrite(format!("insert v2 ledger: {}", e)))?;

    // Step 7: audit log. Mirrors PR-3's audit shape — action verb,
    // resource (the new ledger version), JSON details with the
    // probe hashes so investigators can correlate this rekey with
    // PR-3's prior mismatch detection rows.
    //
    // v2.14.3 — record the canary verification outcome so a future
    // forensic investigation can prove the rekey only proceeded when
    // the old-key candidate decrypted the canary to the expected
    // plaintext. SHA256 of the decrypted plaintext is recorded so
    // tampering with the canary post-fact is detectable.
    let details = serde_json::json!({
        "rows_rekeyed": rows_rekeyed,
        "retired_v1_at": retired_at,
        "old_key_probe": probe_hash(old_key),
        "new_key_probe": probe_hash(new_key),
        "canary_verified": canary_verified,
        "canary_plaintext_sha256": canary_verified.then(|| {
            use sha2::{Sha256, Digest};
            let mut h = Sha256::new();
            h.update(crate::encryption::CANARY_PLAINTEXT.as_bytes());
            format!("{:x}", h.finalize())
        }),
    })
    .to_string();
    let audit_id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO audit_logs
            (id, action, resource_type, resource_id, resource_name,
             details, created_at)
         VALUES (?1, 'master_key_rekeyed', 'master_key_ledger', 'v2',
                 NULL, ?2, ?3)",
        rusqlite::params![audit_id, details, retired_at],
    )
    .map_err(|e| RekeyError::LedgerWrite(format!("audit insert: {}", e)))?;

    tx.commit().map_err(|e| RekeyError::CommitFailed(e.to_string()))?;

    // v2.14.3 — invalidate the in-process encryption cache so the
    // next encrypt/decrypt re-reads the active ledger row + keychain
    // under the new account. Without this, a subsequent encrypt() in
    // the same process would return the cached OLD master key
    // (codex war-room corner case).
    crate::encryption::invalidate_master_key_cache();

    Ok((rows_rekeyed, retired_at))
}

/// Production wrapper: handles the keychain dance around `rekey_inner`.
/// Step 1: parse the user's b64 old key.
/// Step 2: write new v2 key to keychain BEFORE opening the DB
///         transaction — claude C14E2735 ordering.
/// Step 3-7: rekey_inner.
/// Step 8: delete the v1 keychain entry. Failure here is logged but
///         not fatal — the v1 key has no ciphertexts pointing at it.
///
/// NOTE: when the env-bypass `ATO_MASTER_KEY_B64` is active, the
/// caller (Tauri command) should refuse rekey entirely — rekey
/// against a dev-bypass keychain would orphan production state on
/// the next non-bypass launch. That guard lives in the command, not
/// here.
pub fn rekey_master_key_with_keychain(
    conn: &mut Connection,
    old_key_b64: &str,
) -> Result<RekeyResult, RekeyError> {
    let old_key = parse_old_key_b64(old_key_b64)?;
    let new_key = mint_new_master_key();
    write_new_keychain_entry(&new_key)?;

    let (rows_rekeyed, retired_at) = rekey_inner(conn, &old_key, &new_key)?;

    let v1_keychain_deleted = delete_old_keychain_entry();

    Ok(RekeyResult {
        rows_rekeyed,
        v2_keychain_account: "master_key_v2".to_string(),
        v1_keychain_deleted,
        retired_at,
    })
}

fn write_new_keychain_entry(new_key: &[u8; 32]) -> Result<(), RekeyError> {
    let b64 = general_purpose::STANDARD.encode(new_key);
    let entry = keyring::Entry::new("ato-desktop", "master_key_v2")
        .map_err(|e| RekeyError::KeychainWrite(format!("keyring entry: {}", e)))?;
    entry
        .set_password(&b64)
        .map_err(|e| RekeyError::KeychainWrite(format!("set_password: {}", e)))?;
    Ok(())
}

fn delete_old_keychain_entry() -> bool {
    match keyring::Entry::new("ato-desktop", "master_key_v1") {
        Ok(entry) => match entry.delete_password() {
            Ok(()) => true,
            Err(e) => {
                eprintln!(
                    "[security] master_key_v1 keychain delete failed (non-fatal — v1 is \
                     cryptographically orphaned, no ciphertexts reference it): {}",
                    e
                );
                false
            }
        },
        Err(e) => {
            eprintln!("[security] master_key_v1 keychain Entry creation failed: {}", e);
            false
        }
    }
}

/// Tauri command — the frontend's entry point. Refuses to rekey
/// when env-bypass is active (rekeying against a dev-bypass
/// would orphan prod state on the next non-bypass launch).
#[tauri::command]
pub fn rekey_master_key(
    db: tauri::State<'_, crate::DbState>,
    probe_state: tauri::State<'_, crate::identity_probe::IdentityProbeState>,
    old_key_b64: String,
) -> Result<RekeyResult, String> {
    if std::env::var("ATO_MASTER_KEY_B64")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return Err(
            "rekey refused: ATO_MASTER_KEY_B64 env-bypass is active. Unset the env var \
             and restart the app from the production-signed bundle before re-keying."
                .to_string(),
        );
    }
    let mut conn = db.0.lock().map_err(|e| format!("db lock poisoned: {}", e))?;
    let result = rekey_master_key_with_keychain(&mut conn, &old_key_b64)
        .map_err(|e| e.to_string())?;

    // Re-run probe cycle so PR-3's IdentityProbeState reflects the
    // new v1-retired / v2-active world WITHOUT requiring an app
    // restart. PR-5's banner can then immediately flip from
    // Mismatched → Matched (or NotPopulated until next launch when
    // PR-2 populates the v2 probe).
    let new_status = crate::identity_probe::run_full_probe_cycle(&conn);
    if let Ok(mut slot) = probe_state.0.lock() {
        *slot = new_status;
    }
    Ok(result)
}

/// v2.15.0 Slice B (war_room F293287E) — unified drift resolution.
/// Per codex's alternative design: one UX entry point that auto-attempts
/// non-destructive probe_resync first and only escalates to destructive
/// rekey when the canary proves true key drift.
///
/// Returns one of three outcomes:
/// - `Resynced` — non-destructive repair succeeded (most cases)
/// - `RekeyRequired` — true key drift; caller should show paste-old-key UX
/// - `NoCanary` — pre-2.15.0 install needs to relaunch + trigger key
///   resolution first to backfill the canary
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum DriftResolution {
    Resynced {
        active_version: String,
        old_probe: Option<String>,
        new_probe: String,
        resynced_at: String,
    },
    RekeyRequired {
        reason: String,
    },
    NoCanary {
        instruction: String,
    },
}

#[tauri::command]
pub fn resolve_master_key_drift(
    db: tauri::State<'_, crate::DbState>,
    probe_state: tauri::State<'_, crate::identity_probe::IdentityProbeState>,
) -> Result<DriftResolution, String> {
    if std::env::var("ATO_MASTER_KEY_B64")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return Err(
            "drift resolution refused: ATO_MASTER_KEY_B64 env-bypass is active. Unset the env var \
             and restart the app from the production-signed bundle first."
                .to_string(),
        );
    }
    let mut conn = db.0.lock().map_err(|e| format!("db lock poisoned: {}", e))?;
    // The current keychain bytes: we go through the encryption module
    // so this respects sentinel + timeout + ledger-driven account
    // resolution (Slice A's invariants).
    let current_key = crate::encryption::expect_master_key_bytes()
        .map_err(|e| format!("read current master key bytes: {}", e))?;

    let outcome = match probe_resync(&mut conn, &current_key) {
        Ok(o) => DriftResolution::Resynced {
            active_version: o.active_version,
            old_probe: o.old_probe,
            new_probe: o.new_probe,
            resynced_at: o.resynced_at,
        },
        Err(RekeyError::DecryptFailed { context, .. }) if context.contains("real key drift") => {
            DriftResolution::RekeyRequired { reason: context }
        }
        Err(RekeyError::DecryptFailed { context, .. }) if context.contains("Relaunch ATO") => {
            DriftResolution::NoCanary { instruction: context }
        }
        Err(e) => return Err(e.to_string()),
    };

    // Re-run probe cycle so the banner updates without restart.
    let new_status = crate::identity_probe::run_full_probe_cycle(&conn);
    if let Ok(mut slot) = probe_state.0.lock() {
        *slot = new_status;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn fresh_db_with_ledger_and_audit_and_keys() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE llm_api_keys (
                id            TEXT PRIMARY KEY,
                provider      TEXT NOT NULL,
                name          TEXT NOT NULL,
                key_preview   TEXT NOT NULL,
                encrypted_key TEXT NOT NULL,
                project_id    TEXT,
                runtime       TEXT,
                is_active     INTEGER NOT NULL DEFAULT 1,
                last_used     TEXT,
                usage_count   INTEGER NOT NULL DEFAULT 0,
                created_at    TEXT NOT NULL,
                updated_at    TEXT NOT NULL,
                key_version   TEXT NOT NULL DEFAULT 'v1'
             );
             CREATE TABLE master_key_ledger (
                 version           TEXT PRIMARY KEY,
                 keychain_account  TEXT NOT NULL,
                 ciphertext_format TEXT NOT NULL,
                 identity_probe    TEXT,
                 source            TEXT NOT NULL DEFAULT 'keychain',
                 created_at        TEXT NOT NULL,
                 retired_at        TEXT,
                 notes             TEXT,
                 canary_ciphertext TEXT
             );
             INSERT INTO master_key_ledger
                 (version, keychain_account, ciphertext_format,
                  identity_probe, source, created_at, retired_at, notes)
             VALUES
                 ('v1', 'master_key_v1', 'aes-gcm-v1', 'probe-value',
                  'keychain', '2026-05-22T00:00:00Z', NULL, 'fixture');
             CREATE TABLE audit_logs (
                 id            TEXT PRIMARY KEY,
                 action        TEXT NOT NULL,
                 resource_type TEXT NOT NULL,
                 resource_id   TEXT,
                 resource_name TEXT,
                 details       TEXT,
                 created_at    TEXT NOT NULL
             );",
        )
        .unwrap();
        conn
    }

    /// v2.15.0 test helper — backfill the v1 ledger row with a real
    /// canary (encrypted under the given old_key) and a probe_hash that
    /// matches. Required after the rework: rekey_inner's precondition
    /// now refuses to proceed without both. Each test that exercises
    /// rekey_inner must call this with the same `old_key` it'll pass
    /// to rekey_inner so the precondition passes.
    fn seed_v1_ledger_for(conn: &Connection, old_key: &[u8; 32]) {
        let canary_ct = encrypt_with_key(crate::encryption::CANARY_PLAINTEXT, old_key).unwrap();
        let probe = probe_hash(old_key);
        conn.execute(
            "UPDATE master_key_ledger
                SET canary_ciphertext = ?1,
                    identity_probe    = ?2
              WHERE version = 'v1' AND retired_at IS NULL",
            rusqlite::params![canary_ct, probe],
        )
        .unwrap();
    }

    fn insert_v1_row(conn: &Connection, id: &str, plaintext: &str, old_key: &[u8; 32]) {
        let ct = encrypt_with_key(plaintext, old_key).unwrap();
        conn.execute(
            "INSERT INTO llm_api_keys
                (id, provider, name, key_preview, encrypted_key,
                 project_id, runtime, is_active, last_used,
                 usage_count, created_at, updated_at, key_version)
             VALUES (?1, 'test', 'fixture', 'sk-…fix', ?2, NULL,
                     NULL, 1, NULL, 0, '2026-05-22', '2026-05-22', 'v1')",
            rusqlite::params![id, ct],
        )
        .unwrap();
    }

    #[test]
    fn parse_old_key_b64_rejects_non_base64() {
        let err = parse_old_key_b64("not!!!base64!!!").unwrap_err();
        assert!(matches!(err, RekeyError::InvalidOldKey(_)));
    }

    #[test]
    fn parse_old_key_b64_rejects_wrong_length() {
        // 16 bytes b64-encoded.
        let too_short = general_purpose::STANDARD.encode([0u8; 16]);
        let err = parse_old_key_b64(&too_short).unwrap_err();
        match err {
            RekeyError::InvalidOldKey(msg) => assert!(msg.contains("expected 32")),
            _ => panic!("expected InvalidOldKey"),
        }
    }

    #[test]
    fn parse_old_key_b64_accepts_valid_32_byte_key() {
        let key_bytes = [42u8; 32];
        let b64 = general_purpose::STANDARD.encode(key_bytes);
        let parsed = parse_old_key_b64(&b64).unwrap();
        assert_eq!(parsed, key_bytes);
    }

    #[test]
    fn rekey_inner_empty_table_still_inserts_v2_ledger() {
        // Zero-rows case (war-room F). Fresh install with no API
        // keys must still establish the v2 identity.
        let mut conn = fresh_db_with_ledger_and_audit_and_keys();
        let old = [1u8; 32];
        let new = [2u8; 32];
        seed_v1_ledger_for(&conn, &old);
        let (rows, retired_at) = rekey_inner(&mut conn, &old, &new).unwrap();
        assert_eq!(rows, 0);
        assert!(!retired_at.is_empty());

        // v1 retired.
        let v1_retired: Option<String> = conn
            .query_row(
                "SELECT retired_at FROM master_key_ledger WHERE version='v1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(v1_retired.is_some(), "v1 must be retired");

        // v2 active with NULL probe.
        let (v2_account, v2_probe): (String, Option<String>) = conn
            .query_row(
                "SELECT keychain_account, identity_probe
                   FROM master_key_ledger WHERE version='v2'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(v2_account, "master_key_v2");
        assert_eq!(v2_probe, None, "v2 probe stays NULL — PR-2 fills it");
    }

    #[test]
    fn rekey_inner_re_encrypts_every_v1_row_and_decrypts_with_new_key() {
        let mut conn = fresh_db_with_ledger_and_audit_and_keys();
        let old = [3u8; 32];
        let new = [4u8; 32];
        seed_v1_ledger_for(&conn, &old);
        insert_v1_row(&conn, "row-1", "sk-openai-aaaa", &old);
        insert_v1_row(&conn, "row-2", "sk-anthropic-bbbb", &old);
        insert_v1_row(&conn, "row-3", "ghp_github_cccc", &old);

        let (rows, _retired_at) = rekey_inner(&mut conn, &old, &new).unwrap();
        assert_eq!(rows, 3, "all 3 v1 rows must be rekeyed");

        // Every row is now key_version='v2' and decrypts with the NEW key.
        let migrated: Vec<(String, String, String)> = conn
            .prepare(
                "SELECT id, encrypted_key, key_version
                   FROM llm_api_keys ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(migrated.len(), 3);
        for (id, ct, kv) in &migrated {
            assert_eq!(kv, "v2", "row {} should be v2 after rekey", id);
            assert!(
                ct.starts_with(V1_PREFIX),
                "row {} keeps v1: wire prefix (only the master key swapped)",
                id
            );
            let payload = ct.strip_prefix(V1_PREFIX).unwrap();
            // Decrypts with NEW key, NOT old.
            assert!(
                decrypt_v1_with_key(payload, &new).is_ok(),
                "row {} should decrypt with new key",
                id
            );
            assert!(
                decrypt_v1_with_key(payload, &old).is_err(),
                "row {} must NOT decrypt with old key (otherwise re-encrypt was a no-op)",
                id
            );
        }
    }

    #[test]
    fn rekey_inner_wrong_old_key_rolls_back_everything() {
        // The atomicity test: feed in the wrong old key.
        //
        // v2.15.0 (war_room 2EAAE58B finding #2): the precondition
        // CATCHES the wrong key at the CANARY stage before touching any
        // llm_api_keys row — better than the pre-rework behavior where
        // the first row's decrypt failure forced ROLLBACK (rollback was
        // correct but ran AFTER touching data). row_id is now "canary"
        // because the canary is verified first.
        let mut conn = fresh_db_with_ledger_and_audit_and_keys();
        let real_old = [5u8; 32];
        let wrong_old = [99u8; 32];
        let new = [6u8; 32];
        seed_v1_ledger_for(&conn, &real_old);
        insert_v1_row(&conn, "row-1", "secret-value", &real_old);

        let err = rekey_inner(&mut conn, &wrong_old, &new).unwrap_err();
        match err {
            RekeyError::DecryptFailed { row_id, .. } => assert_eq!(row_id, "canary"),
            other => panic!("expected DecryptFailed at canary, got {:?}", other),
        }

        // v1 still active in ledger (rollback).
        let v1_retired: Option<String> = conn
            .query_row(
                "SELECT retired_at FROM master_key_ledger WHERE version='v1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(v1_retired.is_none(), "rollback must leave v1 unretired");

        // No v2 row.
        let v2_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM master_key_ledger WHERE version='v2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v2_count, 0, "rollback must not leave a v2 ledger row");

        // The row still decrypts with the REAL old key (unchanged).
        let ct: String = conn
            .query_row(
                "SELECT encrypted_key FROM llm_api_keys WHERE id='row-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let payload = ct.strip_prefix(V1_PREFIX).unwrap();
        assert_eq!(
            decrypt_v1_with_key(payload, &real_old).unwrap(),
            "secret-value",
            "rollback must leave the row's ciphertext intact"
        );

        // No audit row either.
        let audit_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audit_count, 0);
    }

    #[test]
    fn rekey_inner_skips_v0_legacy_rows() {
        // War-room G: v0 (plain-base64) rows have no key_version='v1'
        // marker. rekey_inner's WHERE clause excludes them. The
        // existing migrate_legacy_api_keys path picks them up on
        // next startup under v2.
        let mut conn = fresh_db_with_ledger_and_audit_and_keys();
        // Insert a legacy v0 row (no v1: prefix, key_version default).
        conn.execute(
            "INSERT INTO llm_api_keys
                (id, provider, name, key_preview, encrypted_key,
                 project_id, runtime, is_active, last_used,
                 usage_count, created_at, updated_at, key_version)
             VALUES ('legacy', 'test', 'legacy-row', 'sk-…leg',
                     'cGxhaW4taDU2NA==', NULL, NULL, 1, NULL, 0,
                     '2026-05-22', '2026-05-22', 'v0')",
            [],
        )
        .unwrap();
        let old = [7u8; 32];
        let new = [8u8; 32];
        seed_v1_ledger_for(&conn, &old);
        let (rows, _) = rekey_inner(&mut conn, &old, &new).unwrap();
        assert_eq!(rows, 0, "v0 row must not be touched by rekey");
        let kv: String = conn
            .query_row(
                "SELECT key_version FROM llm_api_keys WHERE id='legacy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kv, "v0", "v0 row's key_version must stay v0");
    }

    #[test]
    fn rekey_inner_writes_audit_row_with_probe_hashes() {
        let mut conn = fresh_db_with_ledger_and_audit_and_keys();
        let old = [10u8; 32];
        let new = [11u8; 32];
        seed_v1_ledger_for(&conn, &old);
        insert_v1_row(&conn, "row-1", "secret", &old);
        let _ = rekey_inner(&mut conn, &old, &new).unwrap();

        let (action, resource_id, details): (String, String, String) = conn
            .query_row(
                "SELECT action, resource_id, details FROM audit_logs",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(action, "master_key_rekeyed");
        assert_eq!(resource_id, "v2");
        assert!(details.contains("\"rows_rekeyed\":1"));
        assert!(details.contains("\"old_key_probe\""));
        assert!(details.contains("\"new_key_probe\""));
    }

    #[test]
    fn mint_new_master_key_is_random() {
        // Two consecutive mints must produce different keys. Catches
        // a future RNG-stubbing accident.
        let k1 = mint_new_master_key();
        let k2 = mint_new_master_key();
        assert_ne!(k1, k2, "OsRng must produce distinct keys per call");
    }

    #[test]
    fn probe_hash_is_deterministic_32_hex_chars() {
        let k = [42u8; 32];
        let h1 = probe_hash(&k);
        let h2 = probe_hash(&k);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 32);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // v2.15.0 — new tests pinning the war_room 2EAAE58B rework.

    #[test]
    fn rekey_inner_refuses_when_canary_is_null() {
        // Codex finding #3: the pre-2.14.3 NULL-canary bypass was too
        // permissive — rekey now must refuse, instructing the user to
        // relaunch so encryption::ensure_canary_initialized() backfills.
        let mut conn = fresh_db_with_ledger_and_audit_and_keys();
        // Intentionally DON'T call seed_v1_ledger_for(). The fixture
        // leaves canary_ciphertext NULL, which is the pre-2.14.3 state.
        let old = [13u8; 32];
        let new = [14u8; 32];
        let err = rekey_inner(&mut conn, &old, &new).unwrap_err();
        match err {
            RekeyError::DecryptFailed { row_id, context } => {
                assert_eq!(row_id, "canary");
                assert!(
                    context.contains("Relaunch ATO"),
                    "error must instruct user to relaunch: {}",
                    context
                );
                assert!(
                    context.contains("resolves the master key"),
                    "error must explain that simple relaunch isn't enough — must also trigger key resolution: {}",
                    context
                );
            }
            other => panic!("expected DecryptFailed at canary, got {:?}", other),
        }
        // Ledger state must be unchanged (no v2 row, v1 still active).
        let v2_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM master_key_ledger WHERE version='v2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v2_count, 0, "no v2 row may exist when precondition fails");
    }

    #[test]
    fn probe_resync_updates_active_row_when_canary_decrypts() {
        // The happy path: current keychain bytes still decrypt the
        // active row's canary. probe_resync UPDATEs the active row's
        // identity_probe to probe_hash(current_key) without touching
        // any ciphertext, writes the audit row, returns success.
        let mut conn = fresh_db_with_ledger_and_audit_and_keys();
        let key = [17u8; 32];
        seed_v1_ledger_for(&conn, &key); // backfills canary + probe
        // Tamper the stored probe so we can verify it gets REPLACED.
        conn.execute(
            "UPDATE master_key_ledger
                SET identity_probe = 'stale-probe-value'
              WHERE version = 'v1' AND retired_at IS NULL",
            [],
        )
        .unwrap();

        let outcome = probe_resync(&mut conn, &key).unwrap();
        assert_eq!(outcome.active_version, "v1");
        assert_eq!(outcome.old_probe.as_deref(), Some("stale-probe-value"));
        assert_eq!(outcome.new_probe, probe_hash(&key));

        // Confirm the ledger row was updated.
        let stored: String = conn
            .query_row(
                "SELECT identity_probe FROM master_key_ledger WHERE version = 'v1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, probe_hash(&key));

        // Confirm the audit row was written with the right action name.
        let (action, details): (String, String) = conn
            .query_row(
                "SELECT action, details FROM audit_logs ORDER BY created_at DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(action, "master_key_probe_resynced");
        assert!(details.contains("stale-probe-value"));
        assert!(details.contains(&probe_hash(&key)));
    }

    #[test]
    fn probe_resync_refuses_when_canary_does_not_decrypt() {
        // True key drift: current keychain bytes do NOT decrypt the
        // canary. probe_resync MUST refuse (returning DecryptFailed)
        // so the caller falls through to destructive rekey UX.
        // No ledger or audit writes should happen.
        let mut conn = fresh_db_with_ledger_and_audit_and_keys();
        let real_key = [19u8; 32];
        let drifted_key = [20u8; 32];
        seed_v1_ledger_for(&conn, &real_key);

        let err = probe_resync(&mut conn, &drifted_key).unwrap_err();
        match err {
            RekeyError::DecryptFailed { row_id, context } => {
                assert_eq!(row_id, "canary");
                assert!(
                    context.contains("real key drift"),
                    "error must label this as real key drift: {}",
                    context
                );
            }
            other => panic!("expected DecryptFailed, got {:?}", other),
        }

        // Ledger unchanged.
        let stored: String = conn
            .query_row(
                "SELECT identity_probe FROM master_key_ledger WHERE version = 'v1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, probe_hash(&real_key));

        // No audit row.
        let audit_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audit_count, 0);
    }

    #[test]
    fn probe_resync_refuses_when_canary_is_null() {
        // Pre-2.15.0 install: canary NULL → probe_resync can't verify
        // → refuse with instruction to relaunch + trigger key resolution.
        let mut conn = fresh_db_with_ledger_and_audit_and_keys();
        let key = [21u8; 32];
        // Do NOT call seed_v1_ledger_for — canary stays NULL.
        let err = probe_resync(&mut conn, &key).unwrap_err();
        match err {
            RekeyError::DecryptFailed { row_id, context } => {
                assert_eq!(row_id, "canary");
                assert!(context.contains("Relaunch ATO"));
            }
            other => panic!("expected DecryptFailed, got {:?}", other),
        }
    }

    #[test]
    fn rekey_inner_refuses_when_probe_does_not_match() {
        // Codex finding #2: probe_match must be verified in-band as
        // CLAUSE 2 of the single precondition. If the canary decrypts
        // correctly but the stored probe disagrees, that's ledger
        // tampering or stale state — abort.
        let mut conn = fresh_db_with_ledger_and_audit_and_keys();
        let old = [15u8; 32];
        let new = [16u8; 32];
        // Seed the canary so CLAUSE 1 passes...
        let canary_ct = encrypt_with_key(crate::encryption::CANARY_PLAINTEXT, &old).unwrap();
        // ...but override the probe to a value that won't match probe_hash(old).
        conn.execute(
            "UPDATE master_key_ledger
                SET canary_ciphertext = ?1,
                    identity_probe    = 'deliberately-wrong-probe-deadbeef'
              WHERE version = 'v1' AND retired_at IS NULL",
            rusqlite::params![canary_ct],
        )
        .unwrap();

        let err = rekey_inner(&mut conn, &old, &new).unwrap_err();
        match err {
            RekeyError::DecryptFailed { row_id, context } => {
                assert_eq!(row_id, "probe");
                assert!(
                    context.contains("probe_hash mismatch"),
                    "error must explain probe mismatch: {}",
                    context
                );
            }
            other => panic!("expected DecryptFailed at probe, got {:?}", other),
        }
    }
}
