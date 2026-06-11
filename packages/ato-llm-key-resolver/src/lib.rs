// ato-llm-key-resolver — shared resolver for API-provider keys.
//
// Why this crate exists: pre-v2.15.0, both apps/cli/src/api_dispatch.rs
// and apps/desktop/src-tauri/src/api_dispatch.rs each carried their own
// copy of `resolve_api_key`. They drifted on:
//   - error type (anyhow::Result vs Result<_, String>)
//   - SELECT columns (CLI: encrypted_key + is_active; desktop: id +
//     encrypted_key)
//   - usage_count + last_used update (desktop yes, CLI no)
//   - error message wording
// Codex flagged this in war_room 0D398F74 as a real symmetry hazard.
//
// What this crate does:
//   1. Check the env-var bypass (provider.env_var). If set + non-empty,
//      return the plaintext key with `KeySource::Env`.
//   2. Else SELECT the active llm_api_keys row, return the ENCRYPTED
//      ciphertext + key_id with `KeySource::Stored`. Caller decrypts
//      using their own encryption module (which owns the OS keychain
//      and can't itself be shared without dragging keychain access
//      into a shared dep, which we explicitly don't want).
//   3. After successful caller-side decrypt, `touch_usage_count(key_id)`
//      bumps the API Keys panel's "X uses" counter so the same surface
//      ticks for both CLI and desktop dispatches.

use ato_api_providers::ApiProvider;
use rusqlite::Connection;

#[derive(Debug, Clone, PartialEq)]
pub enum KeySource {
    /// Plaintext key supplied via the provider's `env_var`. Caller
    /// uses `key` directly — no decrypt needed.
    Env { var_name: String },
    /// Encrypted ciphertext from the llm_api_keys table. Caller
    /// passes `encrypted_key` through their own encryption::decrypt()
    /// to get the plaintext.
    Stored { key_id: String },
}

#[derive(Debug, Clone)]
pub struct ResolvedKeyMaterial {
    /// For `Env`, this is plaintext. For `Stored`, this is the
    /// encrypted ciphertext — caller MUST decrypt before using.
    pub material: String,
    pub source: KeySource,
}

#[derive(Debug)]
pub enum ResolveError {
    /// No env var set AND no row in llm_api_keys for this provider.
    NoKey {
        provider: &'static str,
        env_var: &'static str,
    },
    /// SQL failed.
    Db(rusqlite::Error),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NoKey { provider, env_var } => write!(
                f,
                "No active API key for provider '{}'. Set ${} or add one in Settings → API Keys.",
                provider, env_var
            ),
            ResolveError::Db(e) => write!(f, "DB error resolving API key: {}", e),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Look up the key material for a provider. Returns the env-var
/// value (if set) or the encrypted DB row (if not). Caller decrypts.
pub fn resolve_key_material(
    provider: &ApiProvider,
    conn: &Connection,
) -> Result<ResolvedKeyMaterial, ResolveError> {
    // 1. Env-var precedence.
    if let Ok(v) = std::env::var(provider.env_var) {
        if !v.trim().is_empty() {
            return Ok(ResolvedKeyMaterial {
                material: v,
                source: KeySource::Env {
                    var_name: provider.env_var.to_string(),
                },
            });
        }
    }
    // 2. Stored encrypted row.
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT id, encrypted_key FROM llm_api_keys
              WHERE LOWER(provider) = ?1
                AND is_active = 1
              ORDER BY updated_at DESC LIMIT 1",
            [provider.slug],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(ResolveError::Db(other)),
        })?;
    let (key_id, encrypted) = row.ok_or_else(|| ResolveError::NoKey {
        provider: provider.slug,
        env_var: provider.env_var,
    })?;
    Ok(ResolvedKeyMaterial {
        material: encrypted,
        source: KeySource::Stored { key_id },
    })
}

/// Bump the usage_count + last_used timestamp on the llm_api_keys row.
/// Best-effort; failure is logged by the caller (or ignored). The
/// desktop's API Keys panel surfaces this counter; making this shared
/// means CLI dispatches also tick the counter, so the panel reflects
/// total usage instead of UI-only usage.
pub fn touch_usage_count(conn: &Connection, key_id: &str) -> rusqlite::Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE llm_api_keys
            SET last_used = ?1, usage_count = usage_count + 1, updated_at = ?1
          WHERE id = ?2",
        rusqlite::params![now, key_id],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ato_api_providers::find_provider;

    fn setup_db() -> Connection {
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
            );",
        )
        .unwrap();
        conn
    }

    fn insert_key(conn: &Connection, id: &str, provider: &str, encrypted: &str) {
        conn.execute(
            "INSERT INTO llm_api_keys
                (id, provider, name, key_preview, encrypted_key,
                 project_id, runtime, is_active, last_used,
                 usage_count, created_at, updated_at, key_version)
             VALUES (?1, ?2, 'fix', 'sk-…fix', ?3, NULL, NULL,
                     1, NULL, 0, '2026-06-11', '2026-06-11', 'v1')",
            rusqlite::params![id, provider, encrypted],
        )
        .unwrap();
    }

    #[test]
    fn env_var_beats_stored_row() {
        let conn = setup_db();
        insert_key(&conn, "stored-id", "google", "v1:stored-ct");
        let provider = find_provider("google").unwrap();
        // Set env var to a sentinel value.
        std::env::set_var("GEMINI_API_KEY", "env-sentinel-value");
        let resolved = resolve_key_material(provider, &conn).unwrap();
        std::env::remove_var("GEMINI_API_KEY");
        assert_eq!(resolved.material, "env-sentinel-value");
        assert!(matches!(resolved.source, KeySource::Env { .. }));
    }

    #[test]
    fn stored_row_used_when_no_env_var() {
        let conn = setup_db();
        insert_key(&conn, "stored-id", "google", "v1:stored-ct");
        std::env::remove_var("GEMINI_API_KEY");
        let provider = find_provider("google").unwrap();
        let resolved = resolve_key_material(provider, &conn).unwrap();
        assert_eq!(resolved.material, "v1:stored-ct");
        match resolved.source {
            KeySource::Stored { key_id } => assert_eq!(key_id, "stored-id"),
            _ => panic!("expected Stored"),
        }
    }

    #[test]
    fn no_env_no_row_returns_NoKey() {
        let conn = setup_db();
        std::env::remove_var("GEMINI_API_KEY");
        let provider = find_provider("google").unwrap();
        match resolve_key_material(provider, &conn) {
            Err(ResolveError::NoKey { provider, env_var }) => {
                assert_eq!(provider, "google");
                assert_eq!(env_var, "GEMINI_API_KEY");
            }
            other => panic!("expected NoKey, got {:?}", other),
        }
    }

    #[test]
    fn touch_usage_count_bumps_counter() {
        let conn = setup_db();
        insert_key(&conn, "key-1", "google", "v1:ct");
        touch_usage_count(&conn, "key-1").unwrap();
        let (count, last_used): (i64, Option<String>) = conn
            .query_row(
                "SELECT usage_count, last_used FROM llm_api_keys WHERE id = 'key-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert!(last_used.is_some());
    }
}
