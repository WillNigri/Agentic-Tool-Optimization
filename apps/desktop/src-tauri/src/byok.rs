// BYOK env-var passthrough for desktop spawn sites.
//
// Mirror of apps/cli/src/byok.rs. The CLI version owns the canonical
// implementation; this file duplicates the runtime → (env_var,
// provider_slug) mapping plus the llm_api_keys lookup because the
// desktop spawns claude/codex/gemini subprocesses directly without
// going through the CLI binary. Same encoding (plain base64), same
// precedence (process env wins, then llm_api_keys, then no env var
// set — subprocess falls through to its own OAuth credentials).
//
// The two copies diverge only in their DB access: the CLI uses
// `crate::db::open_readonly`, the desktop is invoked from Tauri
// commands that already hold a `rusqlite::Connection` via DbState.
// Both end up reading the same `~/.ato/local.db`.

use base64::Engine;
use rusqlite::Connection;
use std::process::Command;

fn runtime_byok_env(runtime_name: &str) -> Option<(&'static str, &'static str)> {
    match runtime_name {
        "claude" => Some(("ANTHROPIC_API_KEY", "anthropic")),
        "codex" => Some(("OPENAI_API_KEY", "openai")),
        "gemini" => Some(("GEMINI_API_KEY", "google")),
        _ => None,
    }
}

fn read_active_key(conn: &Connection, provider: &str) -> Option<String> {
    let encrypted: String = conn
        .query_row(
            "SELECT encrypted_key FROM llm_api_keys
              WHERE LOWER(provider) = ?1 AND is_active = 1
              ORDER BY updated_at DESC LIMIT 1",
            [provider],
            |r| r.get(0),
        )
        .ok()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encrypted.as_bytes())
        .ok()?;
    String::from_utf8(bytes).ok()
}

/// Return (env_var_name, decoded_key) if BYOK applies for this runtime
/// AND a key is configured. None means "fall through to subscription
/// auth" (either runtime has no BYOK mapping, or the user has no
/// stored key). Callers set the env on whichever Command flavor they
/// hold (std vs tokio) — that's why this returns the pair instead of
/// mutating a Command directly.
pub fn byok_env_value(conn: &Connection, runtime_name: &str) -> Option<(&'static str, String)> {
    let (env_var, provider_slug) = runtime_byok_env(runtime_name)?;
    if std::env::var(env_var)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        // Already set in ATO's process env — don't override; the
        // subprocess inherits via Command's normal env-inheritance.
        return None;
    }
    let key = read_active_key(conn, provider_slug)?;
    Some((env_var, key))
}

/// Path-based variant for spawn sites that don't have a Connection.
/// Opens a short-lived read-only handle on the default DB.
pub fn byok_env_value_from_path(
    db_path: &std::path::Path,
    runtime_name: &str,
) -> Option<(&'static str, String)> {
    let (env_var, _) = runtime_byok_env(runtime_name)?;
    if std::env::var(env_var)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return None;
    }
    let conn = Connection::open(db_path).ok()?;
    byok_env_value(&conn, runtime_name)
}

/// Convenience for std::process::Command spawn sites.
pub fn apply_byok_env(cmd: &mut Command, conn: &Connection, runtime_name: &str) {
    if let Some((var, key)) = byok_env_value(conn, runtime_name) {
        cmd.env(var, key);
    }
}

/// Convenience for std::process::Command + path lookup.
pub fn apply_byok_env_from_path(
    cmd: &mut Command,
    db_path: &std::path::Path,
    runtime_name: &str,
) {
    if let Some((var, key)) = byok_env_value_from_path(db_path, runtime_name) {
        cmd.env(var, key);
    }
}

/// Whether a BYOK key is configured for this runtime (env var OR stored).
/// Caller passes a Connection it already holds. Used by UI badges.
pub fn has_byok_key(conn: &Connection, runtime_name: &str) -> bool {
    let Some((env_var, provider_slug)) = runtime_byok_env(runtime_name) else {
        return false;
    };
    if std::env::var(env_var)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    read_active_key(conn, provider_slug).is_some()
}

/// Read-only variant: returns the provider slug used to look up the
/// key (anthropic / openai / google) for UI display. Doesn't expose
/// the key itself.
pub fn byok_provider_slug(runtime_name: &str) -> Option<&'static str> {
    runtime_byok_env(runtime_name).map(|(_, p)| p)
}
