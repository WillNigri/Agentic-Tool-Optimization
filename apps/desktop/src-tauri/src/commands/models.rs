// commands/models.rs — model configuration + Ollama model listing.
//
// PR 2 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md). Proof-
// of-pattern domain — small (4 commands) so the extraction recipe is
// settled before the larger files (cron / skills_mcps / agents) land.
//
// Scope:
//   - `list_ollama_models`     — fetch Ollama's /api/tags
//   - `list_model_configs`     — list rows in `model_configs` table
//   - `save_model_config`      — upsert a model config per (runtime, project)
//   - `get_model_config`       — read the active model config for a runtime
//
// Out of scope (stays in commands/mod.rs until PR 23 `runtimes.rs`):
//   - `detect_ollama`           — runtime-availability surface
//   - `get_ollama_config`       — env-var introspection
//
// The `OllamaStatus` / `OllamaConfig` structs that those two commands
// use stay with them in mod.rs. Only `OllamaModel` migrates here
// because it's only referenced by `list_ollama_models`.
//
// `ModelConfig` itself is defined in `crate::lib.rs` (line ~250); we
// import it via `crate::ModelConfig` rather than re-declaring.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{DbState, ModelConfig};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModel {
    pub name: String,
    pub size: u64,
    pub digest: String,
    pub modified_at: String,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
}

#[tauri::command]
pub async fn list_ollama_models(endpoint: Option<String>) -> Result<Vec<OllamaModel>, String> {
    let base = endpoint.unwrap_or_else(|| {
        std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string())
    });
    let url = format!("{}/api/tags", base);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(&url).send().await
        .map_err(|e| format!("Failed to reach Ollama: {}", e))?;
    let body: serde_json::Value = resp.json().await
        .map_err(|e| format!("Invalid response: {}", e))?;

    let models = body.get("models").and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().filter_map(|m| {
                let name = m.get("name").and_then(|v| v.as_str())?;
                Some(OllamaModel {
                    name: name.to_string(),
                    size: m.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
                    digest: m.get("digest").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    modified_at: m.get("modified_at").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    parameter_size: m.get("details")
                        .and_then(|d| d.get("parameter_size"))
                        .and_then(|v| v.as_str()).map(String::from),
                    quantization: m.get("details")
                        .and_then(|d| d.get("quantization_level"))
                        .and_then(|v| v.as_str()).map(String::from),
                })
            }).collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// List model configurations
#[tauri::command]
pub fn list_model_configs(db: State<'_, DbState>) -> Result<Vec<ModelConfig>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, runtime, project_id, model_id, max_tokens, temperature, created_at, updated_at FROM model_configs ORDER BY runtime"
    ).map_err(|e| e.to_string())?;

    let configs = stmt.query_map([], |row| {
        Ok(ModelConfig {
            id: row.get(0)?,
            runtime: row.get(1)?,
            project_id: row.get(2)?,
            model_id: row.get(3)?,
            max_tokens: row.get(4)?,
            temperature: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    }).map_err(|e| e.to_string())?;

    configs.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Save or update model configuration
#[tauri::command]
pub fn save_model_config(
    db: State<'_, DbState>,
    runtime: String,
    model_id: String,
    project_id: Option<String>,
    max_tokens: Option<i32>,
    temperature: Option<f64>,
) -> Result<ModelConfig, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();

    // Check if config exists
    let existing: Option<String> = conn.query_row(
        "SELECT id FROM model_configs WHERE runtime = ?1 AND (project_id = ?2 OR (project_id IS NULL AND ?2 IS NULL))",
        params![runtime, project_id],
        |row| row.get(0),
    ).ok();

    let id = if let Some(existing_id) = existing {
        // Update existing
        conn.execute(
            "UPDATE model_configs SET model_id = ?1, max_tokens = ?2, temperature = ?3, updated_at = ?4 WHERE id = ?5",
            params![model_id, max_tokens, temperature, now, existing_id],
        ).map_err(|e| e.to_string())?;
        existing_id
    } else {
        // Insert new
        let new_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO model_configs (id, runtime, project_id, model_id, max_tokens, temperature, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![new_id, runtime, project_id, model_id, max_tokens, temperature, now, now],
        ).map_err(|e| e.to_string())?;
        new_id
    };

    Ok(ModelConfig {
        id,
        runtime,
        project_id,
        model_id,
        max_tokens,
        temperature,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Get model config for a runtime
#[tauri::command]
pub fn get_model_config(db: State<'_, DbState>, runtime: String, project_id: Option<String>) -> Result<Option<ModelConfig>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let result = conn.query_row(
        "SELECT id, runtime, project_id, model_id, max_tokens, temperature, created_at, updated_at FROM model_configs WHERE runtime = ?1 AND (project_id = ?2 OR (project_id IS NULL AND ?2 IS NULL))",
        params![runtime, project_id],
        |row| {
            Ok(ModelConfig {
                id: row.get(0)?,
                runtime: row.get(1)?,
                project_id: row.get(2)?,
                model_id: row.get(3)?,
                max_tokens: row.get(4)?,
                temperature: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    );

    match result {
        Ok(config) => Ok(Some(config)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// v2.15.0 Slice C — list models the stored API key can call.
/// Resolves the user's plaintext key for the provider, calls the
/// shared `ato-list-models` crate (which caches in-process for 10
/// minutes), and returns the response. The frontend's React Query
/// hook keeps its own 1h cache in front of this.
#[tauri::command]
pub async fn list_provider_models(
    db: State<'_, DbState>,
    slug: String,
    no_cache: bool,
) -> Result<ato_list_models::ModelListResponse, String> {
    let slug_lc = slug.to_ascii_lowercase();
    let provider = ato_api_providers::find_provider(&slug_lc)
        .ok_or_else(|| format!("Unknown provider slug '{}'", slug))?;

    // Resolve the API key in a scoped sync block so the Connection
    // drops before any .await (same pattern as api_dispatch::dispatch).
    let api_key = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        crate::api_dispatch::resolve_api_key(provider, &conn)
            .map_err(|e| format!("resolve API key for '{}': {}", provider.slug, e))?
    };

    if no_cache {
        ato_list_models::invalidate_cache().await;
    }
    ato_list_models::list_models(provider, &api_key)
        .await
        .map_err(|e| e.to_string())
}
