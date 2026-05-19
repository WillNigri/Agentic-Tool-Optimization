// commands/env_vars.rs — Environment Variables manager (per-project,
// per-runtime, or global).
//
// PR 25b of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
// Companion to PR 25 (secrets.rs). PR 25c will be llm_api_keys.rs.
//
// Scope (4 commands):
//   - list_env_vars   — list with optional project_id + runtime filter
//   - save_env_var    — INSERT new row
//   - update_env_var  — UPDATE key and/or value
//   - delete_env_var  — DELETE by id
//
// Stored in plaintext in SQLite. EnvVar struct lives in crate root.

use rusqlite::params;
use tauri::State;

use crate::{DbState, EnvVar};

/// List environment variables
#[tauri::command]
pub fn list_env_vars(
    db: State<'_, DbState>,
    project_id: Option<String>,
    runtime: Option<String>,
) -> Result<Vec<EnvVar>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Build dynamic SQL
    let mut conditions = Vec::new();
    if project_id.is_some() {
        conditions.push("project_id = ?");
    }
    if runtime.is_some() {
        conditions.push("runtime = ?");
    }

    let sql = if conditions.is_empty() {
        "SELECT id, project_id, runtime, key, value, created_at FROM env_vars ORDER BY key".to_string()
    } else {
        format!(
            "SELECT id, project_id, runtime, key, value, created_at FROM env_vars WHERE {} ORDER BY key",
            conditions.join(" AND ")
        )
    };

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    // Collect parameters
    let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::new();
    if let Some(ref pid) = project_id {
        params_vec.push(pid);
    }
    if let Some(ref rt) = runtime {
        params_vec.push(rt);
    }

    let env_vars = stmt
        .query_map(params_vec.as_slice(), |row| {
            Ok(EnvVar {
                id: row.get(0)?,
                project_id: row.get(1)?,
                runtime: row.get(2)?,
                key: row.get(3)?,
                value: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;

    env_vars
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e: rusqlite::Error| e.to_string())
}

/// Save an environment variable
#[tauri::command]
pub fn save_env_var(
    db: State<'_, DbState>,
    key: String,
    value: String,
    project_id: Option<String>,
    runtime: Option<String>,
) -> Result<EnvVar, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO env_vars (id, project_id, runtime, key, value, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, project_id, runtime, key, value, now],
    ).map_err(|e| e.to_string())?;

    Ok(EnvVar {
        id,
        project_id,
        runtime,
        key,
        value,
        created_at: now,
    })
}

/// Update an environment variable
#[tauri::command]
pub fn update_env_var(
    db: State<'_, DbState>,
    env_id: String,
    key: Option<String>,
    value: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    if let Some(new_key) = key {
        conn.execute(
            "UPDATE env_vars SET key = ?1 WHERE id = ?2",
            params![new_key, env_id],
        )
        .map_err(|e| e.to_string())?;
    }

    if let Some(new_value) = value {
        conn.execute(
            "UPDATE env_vars SET value = ?1 WHERE id = ?2",
            params![new_value, env_id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Delete an environment variable
#[tauri::command]
pub fn delete_env_var(db: State<'_, DbState>, env_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM env_vars WHERE id = ?1", params![env_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
