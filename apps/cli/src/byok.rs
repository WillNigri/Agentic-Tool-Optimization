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

/// Resolve the active key for a provider slug as stored in the desktop's
/// `llm_api_keys` table. Returns the base64-decoded plaintext on success.
/// Mirrors `api_dispatch::resolve_api_key` so the two code paths agree on
/// the encoding (plain base64, no real encryption — the GUI banner says
/// so explicitly).
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
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encrypted.as_bytes())
        .context("decode llm_api_keys.encrypted_key (base64)")?;
    String::from_utf8(bytes).context("decoded key is not UTF-8")
}

/// Apply BYOK env var to a subprocess `Command` if (a) the runtime has
/// a BYOK mapping, (b) the user has stored a key for that vendor in
/// llm_api_keys, and (c) the env var isn't already populated in ATO's
/// own process environment. No-op for unsupported runtimes — those fall
/// through to their existing auth path.
pub fn apply_byok_env(cmd: &mut Command, db_path: &Path, runtime_name: &str) {
    let Some((env_var, provider_slug)) = runtime_byok_env(runtime_name) else {
        return;
    };
    if std::env::var(env_var)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return;
    }
    if let Ok(key) = read_active_key(db_path, provider_slug) {
        cmd.env(env_var, key);
    }
}

/// True iff a BYOK key is configured for the given runtime (either via
/// env var or stored in llm_api_keys). Used by UI badges and `ato
/// runtimes status` to surface auth mode without exposing the key.
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
