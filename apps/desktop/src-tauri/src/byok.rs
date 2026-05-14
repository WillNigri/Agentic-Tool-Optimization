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
// Connection handling: the desktop's current spawn sites
// (prompt_claude, prompt_agent_inner, query_agent_status,
// spawn_streaming_dispatch) all use the path-based `*_from_path`
// variants because the corresponding Tauri commands don't have
// `DbState` in scope at the spawn point. The Connection-flavored
// variants are kept for the future case where a caller does have
// `DbState` and wants to skip the extra open() — wiring them up is
// a perf cleanup, not a correctness change.
//
// Error handling: silently swallows decode errors with `.ok()` so a
// corrupted key in llm_api_keys falls through to subscription auth
// rather than blocking the dispatch. The CLI's mirror surfaces those
// errors via `Result` — the divergence is intentional: a desktop
// user has no chance to see a stderr-style error mid-dispatch, so
// fail-open is a better UX than fail-closed. (minimax #3)

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

/// Per-runtime auth-mode setting key in the `settings` table. Mirror
/// of the CLI's read_auth_mode_setting — "subscription" forces the
/// OAuth path even when a key is stored; "api_key" forces BYOK;
/// absent falls back to "use key if stored."
fn read_auth_mode(conn: &Connection, runtime_name: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [format!("runtime_auth_mode.{}", runtime_name)],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// Return (env_var_name, decoded_key) if BYOK applies for this runtime
/// AND a key is configured. None means "fall through to subscription
/// auth" (either runtime has no BYOK mapping, the user chose
/// subscription explicitly, the env var is already set, or no key
/// stored).
pub fn byok_env_value(conn: &Connection, runtime_name: &str) -> Option<(&'static str, String)> {
    let (env_var, provider_slug) = runtime_byok_env(runtime_name)?;
    // User-chosen subscription mode wins over key-presence.
    if read_auth_mode(conn, runtime_name).as_deref() == Some("subscription") {
        return None;
    }
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
#[allow(dead_code)] // surface via DbState-aware Tauri commands in follow-up
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

/// Path-based has_byok_key — matches the badge semantic the UI needs.
/// Returns true if EITHER the env var is set OR a key is stored. (Used
/// by query_agent_status's auth_mode badge — see claude #1.)
pub fn has_byok_key_from_path(db_path: &std::path::Path, runtime_name: &str) -> bool {
    let Some((env_var, _)) = runtime_byok_env(runtime_name) else {
        return false;
    };
    if std::env::var(env_var)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    byok_env_value_from_path(db_path, runtime_name).is_some()
}

/// What mode would the dispatch path actually use right now? Combines
/// stored key presence + user's explicit setting. Returns one of:
///   "subscription" — runtime supports BYOK but no key OR user picked subscription
///   "api_key"      — key present AND (user picked api_key OR no preference set)
///   None           — runtime has no BYOK mapping at all (hermes / openclaw).
///                    Lets the credit-burn meter avoid misattributing
///                    those dispatches to either bucket. (claude #2)
///
/// Used by the UI badge and the AuthMethodMatrix radio so they show
/// the *real* outcome of the next dispatch, not just user intent.
///
/// NOTE: the "subscription" default for unconfigured users is
/// intentional — changing it would change historical attribution and
/// would require a data migration to backfill existing rows.
pub fn effective_auth_mode_from_path(
    db_path: &std::path::Path,
    runtime_name: &str,
) -> Option<&'static str> {
    if runtime_byok_env(runtime_name).is_none() {
        return None;
    }
    if byok_env_value_from_path(db_path, runtime_name).is_some() {
        Some("api_key")
    } else {
        Some("subscription")
    }
}

/// Read the user's stored preference, if any. None means "no
/// preference set — fall through to default behavior."
pub fn get_user_auth_mode_from_path(
    db_path: &std::path::Path,
    runtime_name: &str,
) -> Option<String> {
    let conn = Connection::open(db_path).ok()?;
    read_auth_mode(&conn, runtime_name)
}

// ─── Tauri commands ───────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAuthInfo {
    pub runtime: String,
    /// User's explicit choice ("subscription" | "api_key") or None
    /// when they haven't picked a side.
    pub user_choice: Option<String>,
    /// What dispatch would actually use right now.
    pub effective: String,
    /// True iff a stored key OR env var exists for this runtime.
    pub has_key: bool,
    /// True iff this runtime has a BYOK mapping at all (claude /
    /// codex / gemini do; hermes / openclaw don't).
    pub supports_byok: bool,
}

#[tauri::command]
pub fn get_runtime_auth_info(runtime: String) -> Result<RuntimeAuthInfo, String> {
    let db_path = crate::get_db_path();
    Ok(RuntimeAuthInfo {
        runtime: runtime.clone(),
        user_choice: get_user_auth_mode_from_path(&db_path, &runtime),
        effective: effective_auth_mode_from_path(&db_path, &runtime)
            .unwrap_or("subscription")
            .to_string(),
        has_key: has_byok_key_from_path(&db_path, &runtime),
        supports_byok: runtime_byok_env(&runtime).is_some(),
    })
}

#[tauri::command]
pub fn set_runtime_auth_mode(runtime: String, mode: String) -> Result<(), String> {
    let db_path = crate::get_db_path();
    set_user_auth_mode_from_path(&db_path, &runtime, &mode)
}

// ─── Credit-burn meter ─────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCostRow {
    pub runtime: String,
    pub auth_mode: Option<String>, // "subscription" | "api_key" | NULL (pre-migration)
    pub dispatch_count: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd_estimated: f64,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditBurnSummary {
    pub since: String,
    pub until: String,
    pub total_cost_usd: f64,
    pub total_dispatches: i64,
    pub api_key_cost_usd: f64,    // real billing (user's API account)
    pub subscription_cost_usd: f64, // API-equivalent of subscription-path dispatches
    pub rows: Vec<RuntimeCostRow>,
}

/// Aggregate execution_logs cost for the current month (UTC). Splits
/// by (runtime, auth_mode) so the UI can show "$X on API keys (real
/// billing) and $Y subscription-equivalent (would be billed at API
/// rates if BYOK was on)". Pre-migration rows with NULL auth_mode are
/// reported separately as "unknown" — the UI surfaces them but doesn't
/// attribute them.
#[tauri::command]
pub fn get_credit_burn_summary() -> Result<CreditBurnSummary, String> {
    let db_path = crate::get_db_path();
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    // Calendar-month window in UTC. Anthropic's Agent SDK credit is a
    // monthly pool, so this matches the natural billing rhythm.
    use chrono::Datelike;
    let now = chrono::Utc::now();
    let nd = now.naive_utc().date();
    let month_start_date = chrono::NaiveDate::from_ymd_opt(nd.year(), nd.month(), 1)
        .ok_or_else(|| "month_start".to_string())?;
    let month_start_naive = month_start_date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "month_start hms".to_string())?;
    let month_start = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        month_start_naive,
        chrono::Utc,
    );
    let since = month_start.to_rfc3339();
    let until = now.to_rfc3339();

    let mut stmt = conn
        .prepare(
            "SELECT runtime, auth_mode,
                    COUNT(*) AS n,
                    COALESCE(SUM(tokens_in), 0) AS tin,
                    COALESCE(SUM(tokens_out), 0) AS tout,
                    COALESCE(SUM(cost_usd_estimated), 0) AS cost
               FROM execution_logs
              WHERE created_at >= ?1
                AND created_at <  ?2
                AND status = 'success'
              GROUP BY runtime, auth_mode
              ORDER BY cost DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows: Vec<RuntimeCostRow> = stmt
        .query_map([&since, &until], |r| {
            Ok(RuntimeCostRow {
                runtime: r.get(0)?,
                auth_mode: r.get(1).ok(),
                dispatch_count: r.get(2)?,
                tokens_in: r.get(3)?,
                tokens_out: r.get(4)?,
                cost_usd_estimated: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let mut api_key_cost = 0.0;
    let mut subscription_cost = 0.0;
    let mut total_cost = 0.0;
    let mut total_n = 0i64;
    for row in &rows {
        total_cost += row.cost_usd_estimated;
        total_n += row.dispatch_count;
        match row.auth_mode.as_deref() {
            Some("api_key") => api_key_cost += row.cost_usd_estimated,
            Some("subscription") => subscription_cost += row.cost_usd_estimated,
            _ => {} // NULL → unattributed, included in total but not split
        }
    }

    Ok(CreditBurnSummary {
        since,
        until,
        total_cost_usd: total_cost,
        total_dispatches: total_n,
        api_key_cost_usd: api_key_cost,
        subscription_cost_usd: subscription_cost,
        rows,
    })
}

/// Write the user's preference. Validates the value to keep the
/// settings table from filling with typos.
pub fn set_user_auth_mode_from_path(
    db_path: &std::path::Path,
    runtime_name: &str,
    mode: &str,
) -> Result<(), String> {
    if !matches!(mode, "subscription" | "api_key" | "clear") {
        return Err(format!("invalid auth mode '{}'", mode));
    }
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let key = format!("runtime_auth_mode.{}", runtime_name);
    if mode == "clear" {
        conn.execute("DELETE FROM settings WHERE key = ?1", [&key])
            .map_err(|e| e.to_string())?;
    } else {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [&key, &mode.to_string()],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Read-only variant: returns the provider slug used to look up the
/// key (anthropic / openai / google) for UI display. Doesn't expose
/// the key itself.
#[allow(dead_code)] // wired alongside the auth-mode badge in follow-up
pub fn byok_provider_slug(runtime_name: &str) -> Option<&'static str> {
    runtime_byok_env(runtime_name).map(|(_, p)| p)
}

/// Redact any BYOK secret material from a string before it lands in a
/// log / DB row / UI surface. Mirrors apps/cli/src/byok.rs of the same
/// name. (minimax #1, HIGH)
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
