// commands/settings_config.rs — Sync + telemetry settings.
//
// PR 24 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (4 commands — small + coherent subset; rest of the plan's
// settings_config follows in a later PR):
//   - get_sync_status         — read cloud-sync enabled flag from
//                               settings table
//   - set_sync_enabled        — flip the flag
//   - get_telemetry_settings  — current TelemetrySettings (anonymous
//                               event opt-in + custom endpoint)
//   - update_telemetry_settings — write back + persist JSON
//
// Deferred to a follow-up:
//   - validate_settings_json (~150 lines, lives by JSON-tree validators)
//   - get_ollama_config (env-var aggregator; lives by ollama_models)
//   - write_sandbox_config / write_toml_config (call write_agent_config_file
//     which has other callers in mod.rs)
//
// SyncStatus, TelemetryState, TelemetrySettings live in crate root /
// telemetry module and come in via use crate::*.

use rusqlite::params;
use tauri::State;

use crate::{telemetry::TelemetryState, DbState, SyncStatus};

#[tauri::command]
pub fn get_sync_status(db: State<'_, DbState>) -> Result<SyncStatus, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let enabled: bool = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'sync_enabled'",
            [],
            |row| {
                let val: String = row.get(0)?;
                Ok(val == "true")
            },
        )
        .unwrap_or(false);

    Ok(SyncStatus {
        enabled,
        last_sync_at: None,
        cloud_url: None,
    })
}

#[tauri::command]
pub fn set_sync_enabled(
    db: State<'_, DbState>,
    enabled: bool,
    _cloud_url: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('sync_enabled', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![if enabled { "true" } else { "false" }],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Get telemetry settings
#[tauri::command]
pub fn get_telemetry_settings(
    state: State<'_, TelemetryState>,
) -> Result<crate::telemetry::TelemetrySettings, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

/// Update telemetry settings
#[tauri::command]
pub fn update_telemetry_settings(
    state: State<'_, TelemetryState>,
    enabled: bool,
    endpoint: Option<String>,
) -> Result<crate::telemetry::TelemetrySettings, String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.enabled = enabled;
    settings.endpoint = endpoint;

    // Persist to config file
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ato");
    let _ = std::fs::create_dir_all(&config_dir);
    let settings_path = config_dir.join("telemetry.json");
    let _ = std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&*settings).unwrap_or_default(),
    );

    Ok(settings.clone())
}

// ── v2.15.3 (war_room 27522371) — exhaustion-policy Settings UI commands ──
//
// Thin Tauri wrappers around the helpers added to quota.rs in v2.15.2.
// CLI dispatch path reads policy on every cached-exhaustion hit; this
// module is the desktop-side surface for the Resilience Settings tab
// + the FirstChatWizard onboarding step.
//
// Per codex's alternative-design verdict: when the user picks
// fallback-chain, persist an `authorized_auto_swap_at` timestamp so the
// dispatch path can verify the user explicitly consented to automatic
// runtime swaps (two-stage consent model).

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum ExhaustionPolicyView {
    Ask,
    StopAndNotify,
    FallbackChain,
    PauseAndWake,
}

impl ExhaustionPolicyView {
    fn as_str(&self) -> &'static str {
        match self {
            ExhaustionPolicyView::Ask => "ask",
            ExhaustionPolicyView::StopAndNotify => "stop-and-notify",
            ExhaustionPolicyView::FallbackChain => "fallback-chain",
            ExhaustionPolicyView::PauseAndWake => "pause-and-wake",
        }
    }
    fn from_str(s: &str) -> Self {
        match s {
            "stop-and-notify" => ExhaustionPolicyView::StopAndNotify,
            "fallback-chain" => ExhaustionPolicyView::FallbackChain,
            "pause-and-wake" => ExhaustionPolicyView::PauseAndWake,
            _ => ExhaustionPolicyView::Ask,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExhaustionPolicyState {
    pub policy: ExhaustionPolicyView,
    /// Set when policy=fallback-chain and the user has clicked through
    /// the explicit consent prompt. NULL otherwise. Per codex's
    /// two-stage consent design.
    pub authorized_auto_swap_at: Option<String>,
}

#[tauri::command]
pub fn get_exhaustion_policy(db: State<'_, DbState>) -> Result<ExhaustionPolicyState, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'exhaustion_policy'",
            [],
            |r| r.get(0),
        )
        .ok();
    let auth: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'exhaustion_authorized_auto_swap_at'",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(ExhaustionPolicyState {
        policy: raw
            .as_deref()
            .map(ExhaustionPolicyView::from_str)
            .unwrap_or(ExhaustionPolicyView::Ask),
        authorized_auto_swap_at: auth,
    })
}

#[tauri::command]
pub fn set_exhaustion_policy(
    db: State<'_, DbState>,
    policy: ExhaustionPolicyView,
    confirm_auto_swap: bool,
) -> Result<ExhaustionPolicyState, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('exhaustion_policy', ?1)",
        params![policy.as_str()],
    )
    .map_err(|e| e.to_string())?;

    let new_auth: Option<String> = match (&policy, confirm_auto_swap) {
        (ExhaustionPolicyView::FallbackChain, true) => {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('exhaustion_authorized_auto_swap_at', ?1)",
                params![&now],
            )
            .map_err(|e| e.to_string())?;
            Some(now)
        }
        _ => {
            conn.execute(
                "DELETE FROM settings WHERE key = 'exhaustion_authorized_auto_swap_at'",
                [],
            )
            .map_err(|e| e.to_string())?;
            None
        }
    };
    Ok(ExhaustionPolicyState {
        policy,
        authorized_auto_swap_at: new_auth,
    })
}

#[tauri::command]
pub fn get_fallback_order(db: State<'_, DbState>) -> Result<Vec<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'exhaustion_fallback_order'",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(raw
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

#[tauri::command]
pub fn set_fallback_order(
    db: State<'_, DbState>,
    slugs: Vec<String>,
) -> Result<Vec<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let joined = slugs.join(",");
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('exhaustion_fallback_order', ?1)",
        params![joined],
    )
    .map_err(|e| e.to_string())?;
    Ok(slugs)
}
