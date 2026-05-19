// commands/secrets.rs — OS-keychain-backed secrets manager.
//
// PR 25 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md), part A.
// Plan grouped secrets + env_vars + llm_api_keys (15 commands) into one
// PR 25; this lands the 5 secrets commands first. env_vars (4) and
// llm_api_keys (6) follow in PR 25b / 25c so each diff stays small.
//
// Scope (5 commands):
//   - list_secrets       — metadata only (name, type, runtime, project),
//                          has_value flag derived from keychain probe
//   - save_secret        — INSERT + write value into the OS keychain
//   - get_secret_value   — explicit-user-action read from keychain
//   - update_secret      — re-write keychain (if value passed) and/or
//                          rename in DB
//   - delete_secret      — remove from both keychain + DB
//
// Values live in the OS keychain via the `keyring` crate; only the
// metadata row is in SQLite. `Secret` struct lives in crate root.

use rusqlite::params;
use tauri::State;

use crate::{DbState, Secret};

const KEYCHAIN_SERVICE: &str = "ato-desktop";

/// List all secrets (metadata only, not values)
#[tauri::command]
pub fn list_secrets(db: State<'_, DbState>) -> Result<Vec<Secret>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, name, key_type, runtime, project_id, created_at, updated_at FROM secrets ORDER BY name",
        )
        .map_err(|e| e.to_string())?;

    let secrets = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;

            // Check if value exists in keychain
            let has_value = keyring::Entry::new(KEYCHAIN_SERVICE, &id)
                .map(|e| e.get_password().is_ok())
                .unwrap_or(false);

            Ok(Secret {
                id,
                name,
                key_type: row.get(2)?,
                runtime: row.get(3)?,
                project_id: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                has_value,
            })
        })
        .map_err(|e| e.to_string())?;

    secrets.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Create or update a secret
#[tauri::command]
pub fn save_secret(
    db: State<'_, DbState>,
    name: String,
    key_type: String,
    value: String,
    runtime: Option<String>,
    project_id: Option<String>,
) -> Result<Secret, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();

    // Store value in OS keychain
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &id)
        .map_err(|e| format!("Failed to create keychain entry: {}", e))?;
    entry
        .set_password(&value)
        .map_err(|e| format!("Failed to store secret in keychain: {}", e))?;

    // Store metadata in database
    conn.execute(
        "INSERT INTO secrets (id, name, key_type, runtime, project_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, name, key_type, runtime, project_id, now, now],
    ).map_err(|e| e.to_string())?;

    Ok(Secret {
        id,
        name,
        key_type,
        runtime,
        project_id,
        created_at: now.clone(),
        updated_at: now,
        has_value: true,
    })
}

/// Get a secret value (requires explicit user action)
#[tauri::command]
pub fn get_secret_value(secret_id: String) -> Result<String, String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &secret_id)
        .map_err(|e| format!("Failed to access keychain: {}", e))?;
    entry
        .get_password()
        .map_err(|e| format!("Failed to retrieve secret: {}", e))
}

/// Update a secret value
#[tauri::command]
pub fn update_secret(
    db: State<'_, DbState>,
    secret_id: String,
    name: Option<String>,
    value: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();

    // Update value in keychain if provided
    if let Some(new_value) = value {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &secret_id)
            .map_err(|e| format!("Failed to access keychain: {}", e))?;
        entry
            .set_password(&new_value)
            .map_err(|e| format!("Failed to update secret: {}", e))?;
    }

    // Update metadata if name changed
    if let Some(new_name) = name {
        conn.execute(
            "UPDATE secrets SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![new_name, now, secret_id],
        )
        .map_err(|e| e.to_string())?;
    } else {
        conn.execute(
            "UPDATE secrets SET updated_at = ?1 WHERE id = ?2",
            params![now, secret_id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Delete a secret
#[tauri::command]
pub fn delete_secret(db: State<'_, DbState>, secret_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Remove from keychain
    if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, &secret_id) {
        let _ = entry.delete_password();
    }

    // Remove from database
    conn.execute("DELETE FROM secrets WHERE id = ?1", params![secret_id])
        .map_err(|e| e.to_string())?;

    Ok(())
}
