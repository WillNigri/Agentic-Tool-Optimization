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
