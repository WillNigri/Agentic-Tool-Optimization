// CLI mirror of the master_key_v2 lifecycle. PR-6.
//
// Today's only subcommand: `ato master-key export` — print the
// current OS-keychain master key to stdout, base64-encoded. Used
// to populate PR-5's "paste the old key" textarea on a different
// machine / install where the user can't drop to `security
// find-generic-password` directly (e.g. headless servers, Linux,
// Windows).
//
// Safety: `--confirm-i-understand-this-prints-the-key` flag
// required. The key in shell history is a real leakage risk; the
// flag exists so an `ato master-key` typo never accidentally
// exposes it. Output goes to stdout (not stderr) so it can be
// piped to `pbcopy` / `xclip` cleanly without the warning.
//
// PR-5's "rekey" UX consumes this output. Future PR-7 could add
// `ato master-key rekey --from-stdin` for headless rekey without
// the desktop UI; held until a real dogfood demand surfaces.

use crate::encryption;
use crate::output::Opts;
use anyhow::{anyhow, Context, Result};
use rusqlite::Connection;

/// Print the master key (base64). Confirms the flag was set; refuses
/// otherwise with a hint about the safety rationale. Emits to stdout
/// only — the warning preamble goes to stderr so a pipe to
/// `pbcopy` / `xclip` captures ONLY the key bytes.
pub fn export(confirm: bool, _opts: &Opts) -> Result<()> {
    if !confirm {
        return Err(anyhow!(
            "refusing to print master key without `--confirm-i-understand-this-prints-the-key`.\n\
             \n\
             The base64 key prints to stdout and lands in shell history. Only run this if\n\
             you immediately pipe it to a secure paste destination (PR-5 rekey textarea,\n\
             pbcopy, etc.) and DON'T leave it in your scrollback.\n\
             \n\
             Re-run with: ato master-key export --confirm-i-understand-this-prints-the-key"
        ));
    }
    // Use the PR-6 wrapper rather than poking at private master_key().
    // Same keychain + cache + ATO_MASTER_KEY_B64 bypass paths apply.
    let b64 = encryption::export_master_key_b64()?;
    // Warning to stderr so stdout stays clean for piping.
    eprintln!(
        "[master-key export] Printing master_key_v1 (32 bytes, base64). \
         This key decrypts every stored llm_api_keys row. Paste it into \
         PR-5's rekey textarea + then clear your shell history."
    );
    // Key to stdout.
    println!("{}", b64);
    Ok(())
}

/// Reserved for a future `ato master-key rekey --from-stdin` that
/// mirrors PR-4's desktop rekey transaction for headless contexts.
/// Held — PR-5's UI covers every dogfood case today; ship CLI rekey
/// when a real headless-server dogfood asks for it.
#[allow(dead_code)]
pub fn rekey_from_stdin(_conn: &Connection, _opts: &Opts) -> Result<()> {
    Err(anyhow!(
        "ato master-key rekey is not yet implemented. Use the desktop \
         app's rekey banner (PR-5) — or open an issue with your headless \
         dogfood scenario."
    ))
}

/// v2.15.x — `ato master-key heal-orphans`.
///
/// Walk `llm_api_keys` and re-encrypt every row whose `key_version`
/// disagrees with the active ledger row. The bug that creates orphans
/// (cross-process stale-cache during a pre-`f740381` dev build save,
/// 2026-06-11) is fixed in the read path; this is the one-shot data-
/// side migration so users don't have to re-enter every key.
///
/// For each orphan row:
///   1. Look up the keychain account name the row's key_version
///      originally pointed at (e.g. `master_key_v1`).
///   2. Read that keychain entry — only works if it still exists
///      (we never auto-delete retired entries; the ledger only
///      marks them retired).
///   3. Decrypt the row's `encrypted_key` with the retired key.
///   4. Re-encrypt the plaintext with the ACTIVE key (the normal
///      `encryption::encrypt()` path, which goes through the
///      ledger-validated cache).
///   5. Persist the new ciphertext + bump `key_version` + bump
///      `updated_at`.
///
/// Behaviour:
///   - Dry-run: prints what would change; touches nothing.
///   - Idempotent: re-running after a successful heal is a no-op.
///   - Per-row resilience: a row that fails to decrypt under its
///     declared old account is reported but does NOT abort the rest
///     of the run.
///   - Skips rows with no ledger row for their declared version
///     (unrecoverable data — the user must re-enter, separate UX work).
pub fn heal_orphans(conn: &Connection, dry_run: bool, opts: &Opts) -> Result<()> {
    // R1 codex #2 fix — refuse live heals when ATO_MASTER_KEY_B64 is
    // set. encrypt() honors that env var first; if it's stale or
    // arbitrary, we'd write fresh ciphertext under the wrong key and
    // stamp it as the active version. Lethal for a repair tool. The
    // dry-run path is still allowed so a user can verify what would
    // happen before unsetting the env var; live writes are blocked.
    if !dry_run && std::env::var("ATO_MASTER_KEY_B64").map(|s| !s.trim().is_empty()).unwrap_or(false) {
        return Err(anyhow!(
            "ATO_MASTER_KEY_B64 is set in the environment. heal-orphans \
             re-encrypts via the normal encrypt() path, which honors that \
             env var ahead of the ledger. If the env var's key is stale \
             or arbitrary, the heal would write ciphertext that no future \
             process can read.\n\n\
             Either:\n\
              • `unset ATO_MASTER_KEY_B64` and re-run (recommended), OR\n\
              • run with --dry-run to see what would change without writing."
        ));
    }

    let active_version = crate::encryption::read_active_master_key_version_from(conn)
        .context("read active master_key_ledger version")?;

    // SELECT the candidate rows. We pull every row whose key_version
    // != active so callers can see "nothing to do" reports clearly
    // (rather than a silent zero-effect run).
    let mut stmt = conn.prepare(
        "SELECT id, provider, key_preview, key_version, encrypted_key
           FROM llm_api_keys
          WHERE key_version != ?1
          ORDER BY created_at",
    )?;
    let rows: Vec<(String, String, String, String, String)> = stmt
        .query_map([&active_version], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut results: Vec<serde_json::Value> = Vec::with_capacity(rows.len());
    let mut healed = 0usize;
    let mut failed = 0usize;

    for (id, provider, key_preview, row_version, encrypted_key) in &rows {
        // 1. Resolve the keychain account that row_version originally pointed at.
        //    R1 codex #3 — use the caller's connection so --db is honored.
        let old_account = match crate::encryption::keychain_account_for_version_from(conn, row_version) {
            Ok(s) => s,
            Err(e) => {
                failed += 1;
                results.push(serde_json::json!({
                    "id": id, "provider": provider, "key_preview": key_preview,
                    "from_version": row_version, "to_version": active_version,
                    "action": "skipped",
                    "reason": format!("no ledger row for version {}: {}", row_version, e),
                }));
                continue;
            }
        };

        // 2. Read the retired keychain entry — strict read-only path
        //    (R1 codex #1 fix). NEVER writes the first-run sentinel,
        //    NEVER generates a new key. Returns Ok(None) when the
        //    entry doesn't exist so we can skip cleanly.
        let old_key = match crate::encryption::read_keychain_key_for_account_readonly(&old_account) {
            Ok(Some(k)) => k,
            Ok(None) => {
                failed += 1;
                results.push(serde_json::json!({
                    "id": id, "provider": provider, "key_preview": key_preview,
                    "from_version": row_version, "to_version": active_version,
                    "old_account": old_account,
                    "action": "skipped",
                    "reason": format!(
                        "keychain entry {} does not exist (retired key was \
                         manually deleted or never created on this machine); \
                         row is unrecoverable — delete + re-enter via Settings",
                        old_account
                    ),
                }));
                continue;
            }
            Err(e) => {
                failed += 1;
                results.push(serde_json::json!({
                    "id": id, "provider": provider, "key_preview": key_preview,
                    "from_version": row_version, "to_version": active_version,
                    "old_account": old_account,
                    "action": "skipped",
                    "reason": format!("keychain read failed for {}: {}", old_account, e),
                }));
                continue;
            }
        };

        // 3. Decrypt under the retired key.
        let plaintext = match crate::encryption::decrypt_v1_with_key(encrypted_key, &old_key) {
            Ok(p) => p,
            Err(e) => {
                failed += 1;
                results.push(serde_json::json!({
                    "id": id, "provider": provider, "key_preview": key_preview,
                    "from_version": row_version, "to_version": active_version,
                    "old_account": old_account,
                    "action": "skipped",
                    "reason": format!("decrypt under retired key failed: {} \
                        (the row may have been encrypted with a third key — \
                        re-enter via Settings)", e),
                }));
                continue;
            }
        };

        if dry_run {
            healed += 1;
            results.push(serde_json::json!({
                "id": id, "provider": provider, "key_preview": key_preview,
                "from_version": row_version, "to_version": active_version,
                "old_account": old_account,
                "action": "would_heal",
            }));
            continue;
        }

        // 4. Re-encrypt under the active key (normal encryption path).
        let new_ct = match crate::encryption::encrypt(&plaintext) {
            Ok(c) => c,
            Err(e) => {
                failed += 1;
                results.push(serde_json::json!({
                    "id": id, "provider": provider, "key_preview": key_preview,
                    "from_version": row_version, "to_version": active_version,
                    "old_account": old_account,
                    "action": "failed",
                    "reason": format!("re-encrypt under active key failed: {}", e),
                }));
                continue;
            }
        };

        // 5. Write back. CURRENT_TIMESTAMP keeps the row's updated_at
        //    in sync with the heal so anyone auditing the rotation
        //    can see when each orphan was migrated.
        match conn.execute(
            "UPDATE llm_api_keys
                SET encrypted_key = ?1,
                    key_version   = ?2,
                    updated_at    = CURRENT_TIMESTAMP
              WHERE id = ?3",
            rusqlite::params![new_ct, active_version, id],
        ) {
            Ok(_) => {
                healed += 1;
                results.push(serde_json::json!({
                    "id": id, "provider": provider, "key_preview": key_preview,
                    "from_version": row_version, "to_version": active_version,
                    "old_account": old_account,
                    "action": "healed",
                }));
            }
            Err(e) => {
                failed += 1;
                results.push(serde_json::json!({
                    "id": id, "provider": provider, "key_preview": key_preview,
                    "from_version": row_version, "to_version": active_version,
                    "old_account": old_account,
                    "action": "failed",
                    "reason": format!("UPDATE failed: {}", e),
                }));
            }
        }
    }

    let report = serde_json::json!({
        "active_version": active_version,
        "dry_run": dry_run,
        "candidates": rows.len(),
        "healed": healed,
        "failed": failed,
        "rows": results,
    });

    if opts.human {
        eprintln!(
            "[heal-orphans] active={}  candidates={}  healed={}  failed={}{}",
            active_version,
            rows.len(),
            healed,
            failed,
            if dry_run { "  (dry-run, no writes)" } else { "" }
        );
        for r in &results {
            let id = r.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let provider = r.get("provider").and_then(|v| v.as_str()).unwrap_or("?");
            let from_v = r.get("from_version").and_then(|v| v.as_str()).unwrap_or("?");
            let to_v = r.get("to_version").and_then(|v| v.as_str()).unwrap_or("?");
            let action = r.get("action").and_then(|v| v.as_str()).unwrap_or("?");
            let reason = r.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            eprintln!("  {} provider={} {}→{} action={} {}", id, provider, from_v, to_v, action, reason);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&report)?);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Opts;

    #[test]
    fn export_refuses_without_confirm_flag() {
        let opts = Opts { human: false, quiet: false };
        let err = export(false, &opts).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--confirm-i-understand-this-prints-the-key"),
            "error must name the safety flag for the user: {}",
            msg
        );
        assert!(
            msg.contains("shell history"),
            "error must mention the leakage concern: {}",
            msg
        );
    }

    #[test]
    fn rekey_stub_returns_not_implemented() {
        let conn = Connection::open_in_memory().unwrap();
        let opts = Opts { human: false, quiet: false };
        let err = rekey_from_stdin(&conn, &opts).unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }
}
