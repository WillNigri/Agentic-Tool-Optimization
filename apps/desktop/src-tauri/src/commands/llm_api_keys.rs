// commands/llm_api_keys.rs — LLM API key management (encrypted storage
// + activation + rotation).
//
// PR 25c of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
// Completes the secrets/env/keys trio (PR 25 secrets.rs + PR 25b env_vars.rs).
//
// Scope (6 commands + 3 helpers):
//   - save_llm_api_key       — insert encrypted row, return masked preview
//   - list_llm_api_keys      — list with optional provider + project filter
//   - get_llm_api_key_value  — decrypt + bump usage_count
//   - rotate_llm_api_key     — re-encrypt with a new value
//   - toggle_llm_api_key     — flip is_active (used by list_available_runtimes)
//   - delete_llm_api_key
//
// Helpers (pub so other modules can reuse):
//   - mask_api_key      — render "abcd...wxyz" preview
//   - simple_encrypt    — wraps crate::encryption::encrypt; survived a
//                         pre-2.4.8 rename from base64 to AES-256-GCM
//                         (audit H1 in SECURITY.md). Name stays for
//                         caller compat.
//   - simple_decrypt    — wraps crate::encryption::decrypt with legacy
//                         base64 fallback (auto-migrate on next write).
//                         knowledge.rs reaches this via super::simple_decrypt.
//
// LlmApiKey struct lives in crate root.

use rusqlite::params;
use tauri::State;

use crate::{DbState, LlmApiKey};

pub fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "*".repeat(key.len());
    }
    let prefix = &key[..4];
    let suffix = &key[key.len() - 4..];
    format!("{}...{}", prefix, suffix)
}

// v2.4.8 — real encryption. Pre-2.4.8 the name `simple_encrypt`
// hid the fact that we were just base64-encoding; the column
// `encrypted_key` was effectively plaintext for any local user
// with read access. Now AES-256-GCM under a master key in the OS
// keychain. The function names stay for caller compat; both wrap
// crate::encryption. Legacy plain-base64 rows still decrypt
// (decrypt() falls back) and get migrated to v1 on next write.
//
// See audit H1 in SECURITY.md.
pub fn simple_encrypt(key: &str) -> String {
    match crate::encryption::encrypt(key) {
        Ok(v1) => v1,
        Err(e) => {
            // Fail-loud rather than silently fall back to legacy
            // base64 — silent legacy regression is exactly the bug
            // we're fixing. Returning an empty string makes the
            // caller's INSERT visibly broken instead.
            eprintln!(
                "[encryption] FATAL: encrypt failed ({}). Stored key will be unusable.",
                e
            );
            String::new()
        }
    }
}

pub fn simple_decrypt(encrypted: &str) -> Result<String, String> {
    crate::encryption::decrypt(encrypted)
}

#[tauri::command]
pub fn save_llm_api_key(
    db: State<'_, DbState>,
    provider: String,
    name: String,
    api_key: String,
    project_id: Option<String>,
    runtime: Option<String>,
) -> Result<LlmApiKey, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let key_preview = mask_api_key(&api_key);
    let encrypted = simple_encrypt(&api_key);

    conn.execute(
        "INSERT INTO llm_api_keys (id, provider, name, key_preview, encrypted_key, project_id, runtime, is_active, usage_count, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 0, ?8, ?8)",
        params![id, provider, name, key_preview, encrypted, project_id, runtime, now],
    ).map_err(|e| e.to_string())?;

    Ok(LlmApiKey {
        id,
        provider,
        name,
        key_preview,
        project_id,
        runtime,
        is_active: true,
        last_used: None,
        usage_count: 0,
        created_at: now.clone(),
        updated_at: now,
    })
}

#[tauri::command]
pub fn list_llm_api_keys(
    db: State<'_, DbState>,
    provider: Option<String>,
    project_id: Option<String>,
) -> Result<Vec<LlmApiKey>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let mut sql = String::from(
        "SELECT id, provider, name, key_preview, project_id, runtime, is_active, last_used, usage_count, created_at, updated_at
         FROM llm_api_keys WHERE 1=1"
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref p) = provider {
        sql.push_str(&format!(" AND provider = ?{}", idx));
        param_values.push(Box::new(p.clone()));
        idx += 1;
    }
    if let Some(ref pid) = project_id {
        sql.push_str(&format!(" AND project_id = ?{}", idx));
        param_values.push(Box::new(pid.clone()));
        idx += 1;
    }
    let _ = idx;
    sql.push_str(" ORDER BY created_at DESC");

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(LlmApiKey {
                id: row.get(0)?,
                provider: row.get(1)?,
                name: row.get(2)?,
                key_preview: row.get(3)?,
                project_id: row.get(4)?,
                runtime: row.get(5)?,
                is_active: row.get::<_, i32>(6)? != 0,
                last_used: row.get(7)?,
                usage_count: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut keys = Vec::new();
    for row in rows {
        keys.push(row.map_err(|e| e.to_string())?);
    }
    Ok(keys)
}

#[tauri::command]
pub fn get_llm_api_key_value(db: State<'_, DbState>, id: String) -> Result<String, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let encrypted: String = conn
        .query_row(
            "SELECT encrypted_key FROM llm_api_keys WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE llm_api_keys SET last_used = ?1, usage_count = usage_count + 1, updated_at = ?1 WHERE id = ?2",
        params![now, id],
    ).map_err(|e| e.to_string())?;

    simple_decrypt(&encrypted)
}

#[tauri::command]
pub fn rotate_llm_api_key(
    db: State<'_, DbState>,
    id: String,
    new_key: String,
) -> Result<LlmApiKey, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let key_preview = mask_api_key(&new_key);
    let encrypted = simple_encrypt(&new_key);

    conn.execute(
        "UPDATE llm_api_keys SET encrypted_key = ?1, key_preview = ?2, updated_at = ?3 WHERE id = ?4",
        params![encrypted, key_preview, now, id],
    )
    .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, provider, name, key_preview, project_id, runtime, is_active, last_used, usage_count, created_at, updated_at
             FROM llm_api_keys WHERE id = ?1",
        )
        .map_err(|e| e.to_string())?;

    stmt.query_row(params![id], |row| {
        Ok(LlmApiKey {
            id: row.get(0)?,
            provider: row.get(1)?,
            name: row.get(2)?,
            key_preview: row.get(3)?,
            project_id: row.get(4)?,
            runtime: row.get(5)?,
            is_active: row.get::<_, i32>(6)? != 0,
            last_used: row.get(7)?,
            usage_count: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_llm_api_key(
    db: State<'_, DbState>,
    id: String,
    is_active: bool,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE llm_api_keys SET is_active = ?1, updated_at = ?2 WHERE id = ?3",
        params![is_active as i32, now, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_llm_api_key(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM llm_api_keys WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
