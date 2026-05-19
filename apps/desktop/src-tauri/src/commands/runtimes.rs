// commands/runtimes.rs — Runtime detection, preferences, config, and
// connection-test surface.
//
// PR 23 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (9 commands):
//   - set_runtime_path           — save custom CLI path (auto-detect fallback)
//   - get_runtime_path           — read it back
//   - list_runtime_preferences   — v2.5.1 monitored-runtimes preference
//                                  table (Health panel filters by this)
//   - set_runtime_monitored      — toggle the monitored flag
//   - list_available_runtimes    — unified runtime picker source (CLI + API)
//   - detect_agent_runtimes      — CLI binary detection
//   - save_runtime_config        — persist a runtime's config to disk
//   - load_runtime_config        — read it back
//   - test_runtime_connection    — runtime-specific health probe (claude/
//                                  codex/hermes use --version; openclaw uses
//                                  SSH against the configured gateway)
//
// Plus the three small data types (DetectedRuntime, RuntimePreference,
// AvailableRuntime) and the seed helper.
//
// `is_runtime_monitored` is `pub` so health_poller can call it without
// taking the Tauri-managed lock; it lives here too.
//
// Cross-domain helpers stay in mod.rs and are reached via super::*:
//   which_cli, which_claude, home_dir, read_file_lossy, get_user_path,
//   load_openclaw_ssh_config.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;

use crate::{get_db_path, home_dir};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DetectedRuntime {
    pub runtime: String,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct RuntimePreference {
    pub runtime: String,
    pub monitored: bool,
}

/// One row in the runtime picker.
#[derive(Debug, Clone, Serialize)]
pub struct AvailableRuntime {
    /// Stable slug used for dispatch (claude, codex, minimax, grok, ...).
    pub slug: String,
    /// Human label for the dropdown.
    pub label: String,
    /// "cli" — needs a binary on PATH; "api" — needs an active
    /// llm_api_keys row.
    pub kind: String,
    /// Did the gate check pass? Frontend hides rows where this is false.
    pub available: bool,
    /// "no_binary" / "no_key" / "ok" — surfaceable hint for the UI's
    /// "why not?" tooltip on a disabled row.
    pub reason: String,
}

const KNOWN_RUNTIMES: [&str; 5] = ["claude", "codex", "gemini", "openclaw", "hermes"];

/// Save a custom CLI path for a runtime (used when auto-detect fails).
#[tauri::command]
pub fn set_runtime_path(runtime: String, path: String) -> Result<(), String> {
    let ato_dir = home_dir().join(".ato");
    fs::create_dir_all(&ato_dir).ok();
    let file_path = ato_dir.join(format!("{}-path", runtime));
    fs::write(&file_path, path.trim()).map_err(|e| format!("Failed to save runtime path: {}", e))
}

/// Get a saved custom CLI path for a runtime.
#[tauri::command]
pub fn get_runtime_path(runtime: String) -> Result<Option<String>, String> {
    let file_path = home_dir().join(".ato").join(format!("{}-path", runtime));
    Ok(super::read_file_lossy(&file_path)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty()))
}

// ─── v2.5.1 monitored-runtimes preference ────────────────────────────
//
// Will surfaced 2026-05-14: Hermes (never installed) shows "Down" red
// and OpenClaw (uninstalled long ago) still lingers — both because
// the Health panel renders every known runtime regardless of whether
// the user uses it. The fix is a per-runtime monitored flag in a
// `runtime_preferences` table; the panel filters on this flag.

/// First-launch seed: for every known runtime not yet in the
/// preferences table, set monitored = (which_cli detected it).
/// Idempotent — re-running never overwrites an existing row.
fn ensure_runtime_preferences_seeded(conn: &Connection) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    for &runtime in &KNOWN_RUNTIMES {
        // v2.5.1 review Tier 1 — Finding 3: previously `SELECT COUNT(*)
        // … .unwrap_or(0)` mapped both "row missing" and "DB error" to
        // 0, causing repeated INSERTs whenever the DB was locked. The
        // SELECT-1-OR-FALSE pattern distinguishes the two: rusqlite
        // returns Err(QueryReturnedNoRows) for missing rows (→ false,
        // safe to insert) and Err(other) for real DB errors. We bail
        // out of the entire seed on a real error rather than blindly
        // retrying.
        let exists: bool = match conn.query_row(
            "SELECT 1 FROM runtime_preferences WHERE runtime = ?1",
            [runtime],
            |_| Ok(true),
        ) {
            Ok(_) => true,
            Err(rusqlite::Error::QueryReturnedNoRows) => false,
            Err(e) => return Err(e),
        };
        if exists {
            continue;
        }
        let detected = super::which_cli(runtime).is_some();
        conn.execute(
            "INSERT INTO runtime_preferences (runtime, monitored, updated_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![runtime, if detected { 1 } else { 0 }, now],
        )?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_runtime_preferences() -> Result<Vec<RuntimePreference>, String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    let _ = ensure_runtime_preferences_seeded(&conn);
    let mut stmt = conn
        .prepare("SELECT runtime, monitored FROM runtime_preferences ORDER BY runtime")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(RuntimePreference {
                runtime: r.get(0)?,
                monitored: r.get::<_, i64>(1)? != 0,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|x| x.ok())
        .collect();
    Ok(rows)
}

#[tauri::command]
pub fn set_runtime_monitored(runtime: String, monitored: bool) -> Result<(), String> {
    if !KNOWN_RUNTIMES.contains(&runtime.as_str()) {
        return Err(format!("Unknown runtime: {}", runtime));
    }
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO runtime_preferences (runtime, monitored, updated_at)
              VALUES (?1, ?2, ?3)
         ON CONFLICT(runtime) DO UPDATE SET monitored = excluded.monitored, updated_at = excluded.updated_at",
        rusqlite::params![runtime, if monitored { 1 } else { 0 }, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Returns true if the runtime should be probed/displayed in the
/// Health panel. Used by health_poller to skip un-monitored runtimes
/// (no point probing Hermes if the user never installed it).
pub fn is_runtime_monitored(runtime: &str) -> bool {
    let conn = match Connection::open(get_db_path()) {
        Ok(c) => c,
        Err(_) => return true, // fail open — surface health if DB is unreadable
    };
    let _ = ensure_runtime_preferences_seeded(&conn);
    conn.query_row(
        "SELECT monitored FROM runtime_preferences WHERE runtime = ?1",
        [runtime],
        |r| r.get::<_, i64>(0),
    )
    .map(|v| v != 0)
    .unwrap_or(true)
}

// v2.3.23 Phase 6.x-B — unified runtime picker source. The PromptBar /
// agent-creation dropdowns surface the union of CLI runtimes + API
// providers the user has keys for; this command is the single source
// of truth for that picker. The frontend filters by `available=true`.
// v2.3.28 Phase 6.x-E — provider slugs+labels now come from the shared
// ato-api-providers crate.

#[tauri::command]
pub fn list_available_runtimes() -> Result<Vec<AvailableRuntime>, String> {
    let mut out: Vec<AvailableRuntime> = Vec::new();

    // CLI runtimes — same set detect_agent_runtimes inspects. Each is
    // "available" iff the binary resolves.
    for (slug, label, path) in [
        (
            "claude",
            "Claude",
            super::which_claude().or_else(|| super::which_cli("claude")),
        ),
        ("codex", "Codex", super::which_cli("codex")),
        ("gemini", "Gemini", super::which_cli("gemini")),
        ("openclaw", "OpenClaw", super::which_cli("openclaw")),
        ("hermes", "Hermes", super::which_cli("hermes")),
    ] {
        let available = path.is_some();
        out.push(AvailableRuntime {
            slug: slug.to_string(),
            label: label.to_string(),
            kind: "cli".to_string(),
            available,
            reason: if available {
                "ok".to_string()
            } else {
                "no_binary".to_string()
            },
        });
    }

    // API providers — "available" iff llm_api_keys has at least one
    // active row whose provider matches (case-insensitive). Single
    // query gathers them all so the picker is O(1) trips to SQLite.
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT LOWER(provider) FROM llm_api_keys
              WHERE is_active = 1
           GROUP BY LOWER(provider)",
        )
        .map_err(|e| e.to_string())?;
    let active_providers: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    for (slug, label) in ato_api_providers::slugs_and_labels() {
        let available = active_providers.contains(slug);
        out.push(AvailableRuntime {
            slug: slug.to_string(),
            label: label.to_string(),
            kind: "api".to_string(),
            available,
            reason: if available {
                "ok".to_string()
            } else {
                "no_key".to_string()
            },
        });
    }

    Ok(out)
}

#[tauri::command]
pub fn detect_agent_runtimes() -> Result<Vec<DetectedRuntime>, String> {
    let runtimes = vec![
        (
            "claude",
            super::which_claude().or_else(|| super::which_cli("claude")),
        ),
        ("codex", super::which_cli("codex")),
        ("openclaw", super::which_cli("openclaw")),
        ("hermes", super::which_cli("hermes")),
    ];

    Ok(runtimes
        .into_iter()
        .map(|(name, path)| {
            let available = path.is_some();
            DetectedRuntime {
                runtime: name.to_string(),
                available,
                version: if available { Some("CLI".to_string()) } else { None },
                path,
            }
        })
        .collect())
}

#[tauri::command]
pub fn save_runtime_config(runtime: String, config: String) -> Result<(), String> {
    let ato_dir = home_dir().join(".ato");
    fs::create_dir_all(&ato_dir).ok();
    let file_path = ato_dir.join(format!("{}-config.json", runtime));
    fs::write(&file_path, config).map_err(|e| format!("Failed to save config: {}", e))
}

#[tauri::command]
pub fn load_runtime_config(runtime: String) -> Result<Option<String>, String> {
    let file_path = home_dir().join(".ato").join(format!("{}-config.json", runtime));
    Ok(super::read_file_lossy(&file_path))
}

#[tauri::command]
pub async fn test_runtime_connection(
    runtime: String,
    config: String,
) -> Result<serde_json::Value, String> {
    let _ = config; // currently unused — kept for future SSH-cred test payload
    match runtime.as_str() {
        "openclaw" => {
            // Use SSH to test connection (gateway requires crypto auth for WebSocket)
            let (host, port, user, key_path) = super::load_openclaw_ssh_config()?;
            let user_path = super::get_user_path();
            let mut cmd = std::process::Command::new("ssh");
            cmd.env("PATH", &user_path);
            cmd.args([
                "-o",
                "ConnectTimeout=5",
                "-o",
                "StrictHostKeyChecking=accept-new",
            ]);
            if let Some(ref key) = key_path {
                cmd.args(["-i", key]);
            }
            cmd.args([
                "-p",
                &port.to_string(),
                &format!("{}@{}", user, host),
                "openclaw --version 2>/dev/null || echo UNKNOWN",
            ]);
            let output = cmd
                .output()
                .map_err(|e| format!("SSH connection failed: {}", e))?;
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Ok(json!({"connected": true, "version": version, "host": host}))
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(format!("SSH to {}@{}:{} failed: {}", user, host, port, stderr))
            }
        }
        "claude" => {
            let path = super::which_cli("claude").ok_or("Claude CLI not found")?;
            let output = std::process::Command::new(&path)
                .arg("--version")
                .output()
                .map_err(|e| e.to_string())?;
            Ok(json!({
                "connected": output.status.success(),
                "version": String::from_utf8_lossy(&output.stdout).trim().to_string()
            }))
        }
        "codex" => {
            let path = super::which_cli("codex").ok_or("Codex CLI not found")?;
            let output = std::process::Command::new(&path)
                .arg("--version")
                .output()
                .map_err(|e| e.to_string())?;
            Ok(json!({
                "connected": output.status.success(),
                "version": String::from_utf8_lossy(&output.stdout).trim().to_string()
            }))
        }
        "hermes" => {
            let path = super::which_cli("hermes").ok_or("Hermes CLI not found")?;
            let output = std::process::Command::new(&path)
                .arg("--version")
                .output()
                .map_err(|e| e.to_string())?;
            Ok(json!({
                "connected": output.status.success(),
                "version": String::from_utf8_lossy(&output.stdout).trim().to_string()
            }))
        }
        _ => Err(format!("Unknown runtime: {}", runtime)),
    }
}
