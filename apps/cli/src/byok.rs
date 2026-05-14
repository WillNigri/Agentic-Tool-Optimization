// BYOK env-var passthrough for CLI-runtime dispatches (claude / codex /
// gemini).
//
// Why this exists: the api-providers registry (apps/cli/src/api_dispatch.rs)
// already handles BYOK for the providers that go through direct HTTPS
// (minimax, grok, deepseek, qwen, openrouter, google). The CLI runtimes
// (claude --print, codex exec, gemini -p) spawn a subprocess that does
// its own auth — historically via the subscription OAuth credentials
// stored in ~/.claude / ~/.codex / etc. After 2026-06-15, claude --print
// counts against the Agent SDK credit instead of unlimited subscription;
// users who want predictable pay-as-you-go billing need to plug in an
// API key and have ATO pass it to the subprocess.
//
// The mechanism is dead simple: read the stored key from llm_api_keys,
// set the runtime's standard env var on the Command. The CLI subprocess
// (claude / codex / gemini) reads the env var and authenticates against
// the API account directly, bypassing the subscription. Anthropic /
// OpenAI / Google handle the billing distinction on their end — we just
// have to honour the env-var convention each vendor publishes.
//
// Precedence: an env var already set in ATO's process environment wins
// (so the user can `ANTHROPIC_API_KEY=sk-... ato dispatch claude ...`
// without touching the GUI). Falls back to the llm_api_keys table next.
// If nothing is configured, we don't set the env var — the subprocess
// falls through to its own OAuth credentials (subscription).

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use std::path::Path;
use std::process::Command;

/// Map a runtime slug to the env var the runtime's CLI honors plus the
/// `llm_api_keys.provider` value the desktop UI uses to store keys for
/// that vendor. Runtimes not in this map have no BYOK path (e.g. hermes,
/// openclaw — those have their own bespoke auth).
fn runtime_byok_env(runtime_name: &str) -> Option<(&'static str, &'static str)> {
    match runtime_name {
        "claude" => Some(("ANTHROPIC_API_KEY", "anthropic")),
        "codex" => Some(("OPENAI_API_KEY", "openai")),
        "gemini" => Some(("GEMINI_API_KEY", "google")),
        _ => None,
    }
}

/// True iff this runtime has a BYOK env-var mapping at all. Used by
/// callers that want to record `auth_mode` as `None` for hermes /
/// openclaw rather than misattributing them to "subscription".
/// (claude #2)
pub fn runtime_supports_byok(runtime_name: &str) -> bool {
    runtime_byok_env(runtime_name).is_some()
}

/// Resolve the active key for a provider slug as stored in the desktop's
/// `llm_api_keys` table. Returns the base64-decoded plaintext on success.
/// v2.4.8 audit H1 — uses crate::encryption::decrypt to handle both
/// AES-GCM v1 rows and the legacy plain-base64 format pre-2.4.8.
fn read_active_key(db_path: &Path, provider: &str) -> Result<String> {
    let conn = crate::db::open_readonly(db_path)?;
    let encrypted: String = conn
        .query_row(
            "SELECT encrypted_key FROM llm_api_keys
              WHERE LOWER(provider) = ?1 AND is_active = 1
              ORDER BY updated_at DESC LIMIT 1",
            [provider],
            |r| r.get(0),
        )
        .map_err(|e| anyhow!("no active key for provider '{}': {}", provider, e))?;
    crate::encryption::decrypt(&encrypted)
        .context("decrypt llm_api_keys.encrypted_key")
}

/// Per-runtime auth-mode preference, stored in `settings` as
/// `runtime_auth_mode.<runtime>`. "subscription" forces the
/// subscription path even when a key is configured; "api_key" forces
/// the API-key path (and errors if no key is found). Absent rows fall
/// back to "if key exists, use it" — matches behavior before this
/// preference existed so existing installs see no change on upgrade.
fn read_auth_mode_setting(db_path: &Path, runtime_name: &str) -> Option<String> {
    let conn = crate::db::open_readonly(db_path).ok()?;
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [format!("runtime_auth_mode.{}", runtime_name)],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// Resolve the BYOK env var + key for a runtime. Returns None when the
/// runtime has no mapping, the user has explicitly chosen subscription
/// mode, the process env var is already set (so we let it inherit
/// naturally), or no stored key is found.
///
/// The CLI caller forwards this to the subprocess AND keeps a copy of
/// `key` so it can redact stderr before persisting — vendor error
/// messages sometimes echo the bad key back, and that can't reach
/// execution_logs.error_message.
pub fn byok_env_value(db_path: &Path, runtime_name: &str) -> Option<(&'static str, String)> {
    let (env_var, provider_slug) = runtime_byok_env(runtime_name)?;
    // User-chosen auth mode wins over the implicit "key configured →
    // use key" default. "subscription" means "even if I have a key
    // stored, use the OAuth subscription credentials" — useful when
    // the user has both and wants to save the key for emergencies.
    if read_auth_mode_setting(db_path, runtime_name).as_deref() == Some("subscription") {
        return None;
    }
    if std::env::var(env_var)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return None;
    }
    let key = read_active_key(db_path, provider_slug).ok()?;
    Some((env_var, key))
}

/// Convenience: set the env var if BYOK applies. Most callers want
/// `byok_env_value` instead so they can capture the key for redaction.
#[allow(dead_code)] // kept for callers that don't need to redact stderr
pub fn apply_byok_env(cmd: &mut Command, db_path: &Path, runtime_name: &str) {
    if let Some((env_var, key)) = byok_env_value(db_path, runtime_name) {
        cmd.env(env_var, key);
    }
}

/// True iff a BYOK key is configured for the given runtime (either via
/// env var or stored in llm_api_keys). Used by UI badges and `ato
/// runtimes status` to surface auth mode without exposing the key.
#[allow(dead_code)] // wired in follow-up commit alongside per-runtime badge
pub fn has_byok_key(db_path: &Path, runtime_name: &str) -> bool {
    let Some((env_var, provider_slug)) = runtime_byok_env(runtime_name) else {
        return false;
    };
    if std::env::var(env_var)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    read_active_key(db_path, provider_slug).is_ok()
}

/// Redact any BYOK secret material from a string before it lands in a
/// log / DB row / UI surface. Two sources are checked: the key we just
/// forwarded via `apply_byok_env` (caller passes it in via `applied_key`),
/// and the env var the user may have set in ATO's shell. We redact
/// exact-substring matches only — no regex on prefixes — to keep the
/// blast radius narrow and avoid mangling unrelated bytes that happen
/// to look like a key. (minimax #1, HIGH)
pub fn redact_byok_secrets(text: &str, runtime_name: &str, applied_key: Option<&str>) -> String {
    let mut out = text.to_string();
    if let Some(k) = applied_key {
        if !k.trim().is_empty() {
            out = out.replace(k, "[REDACTED:API_KEY]");
        }
    }
    if let Some((env_var, _)) = runtime_byok_env(runtime_name) {
        if let Ok(v) = std::env::var(env_var) {
            if !v.trim().is_empty() && Some(v.as_str()) != applied_key {
                out = out.replace(&v, "[REDACTED:API_KEY]");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_byok_env_known_runtimes() {
        assert_eq!(
            runtime_byok_env("claude"),
            Some(("ANTHROPIC_API_KEY", "anthropic"))
        );
        assert_eq!(
            runtime_byok_env("codex"),
            Some(("OPENAI_API_KEY", "openai"))
        );
        assert_eq!(
            runtime_byok_env("gemini"),
            Some(("GEMINI_API_KEY", "google"))
        );
    }

    #[test]
    fn runtime_byok_env_unknown_returns_none() {
        assert_eq!(runtime_byok_env("hermes"), None);
        assert_eq!(runtime_byok_env("openclaw"), None);
        assert_eq!(runtime_byok_env(""), None);
    }

    #[test]
    fn redact_strips_applied_key() {
        let text = "auth failed: invalid key sk-ant-abc123 try again";
        let redacted = redact_byok_secrets(text, "claude", Some("sk-ant-abc123"));
        assert!(!redacted.contains("sk-ant-abc123"));
        assert!(redacted.contains("[REDACTED:API_KEY]"));
    }

    #[test]
    fn redact_handles_empty_applied_key() {
        // Empty key shouldn't cause `String::replace("", ...)` chaos
        // (which would expand to insert between every char).
        let text = "no key here";
        let redacted = redact_byok_secrets(text, "claude", Some(""));
        assert_eq!(redacted, text);
    }

    #[test]
    fn redact_no_op_for_unknown_runtime() {
        let text = "boring text";
        let redacted = redact_byok_secrets(text, "hermes", None);
        assert_eq!(redacted, text);
    }

    #[test]
    fn runtime_supports_byok_truth_table() {
        // Lock in the None-for-non-BYOK contract that the
        // credit-burn meter depends on. Changing this means changing
        // historical attribution and would need a data migration.
        assert!(runtime_supports_byok("claude"));
        assert!(runtime_supports_byok("codex"));
        assert!(runtime_supports_byok("gemini"));
        assert!(!runtime_supports_byok("hermes"));
        assert!(!runtime_supports_byok("openclaw"));
        assert!(!runtime_supports_byok(""));
        assert!(!runtime_supports_byok("CLAUDE")); // case-sensitive
    }
}
