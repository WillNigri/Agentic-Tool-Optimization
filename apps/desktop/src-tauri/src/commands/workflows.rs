// commands/workflows.rs — workflow persistence + template catalog.
//
// PR 12 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope:
//   - `list_workflows`           — enumerate ~/.ato/workflows/*.json
//   - `save_workflow`            — write one workflow JSON (id sanitized)
//   - `load_workflow`            — read one workflow JSON by id
//   - `delete_workflow`          — remove one workflow JSON
//   - `list_workflow_templates`  — built-in starter templates
//
// Plus the `WorkflowTemplate` struct (only consumer is
// list_workflow_templates) and the `workflows_dir()` helper (only
// callers are the four CRUD commands here — pulls in via the
// crate::home_dir reach-around).
//
// Out of scope (PR 13 / `workflow_webhooks.rs`):
//   - register_workflow_webhook / list_workflow_webhooks /
//     delete_workflow_webhook / toggle_workflow_webhook — those are
//     a different concept (inbound trigger registration) and travel
//     together when their domain extracts.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::read_file_lossy;
use crate::home_dir;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub version: String,
    pub is_built_in: bool,
    pub nodes: serde_json::Value,
    pub edges: serde_json::Value,
}

pub fn workflows_dir() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    path.push("workflows");
    fs::create_dir_all(&path).ok();
    path
}

#[tauri::command]
pub fn list_workflows() -> Result<Vec<serde_json::Value>, String> {
    let dir = workflows_dir();
    let mut workflows = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                if let Some(content) = read_file_lossy(&path) {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                        workflows.push(parsed);
                    }
                }
            }
        }
    }
    Ok(workflows)
}

#[tauri::command]
pub fn save_workflow(workflow: String) -> Result<(), String> {
    let parsed: serde_json::Value = serde_json::from_str(&workflow)
        .map_err(|e| format!("Invalid workflow JSON: {}", e))?;
    let id = parsed.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Workflow must have an id".to_string())?;

    // Sanitize filename
    let safe_id: String = id.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();

    let path = workflows_dir().join(format!("{}.json", safe_id));
    fs::write(&path, &workflow).map_err(|e| format!("Failed to write workflow: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn load_workflow(id: String) -> Result<serde_json::Value, String> {
    let safe_id: String = id.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let path = workflows_dir().join(format!("{}.json", safe_id));
    let content = read_file_lossy(&path)
        .ok_or_else(|| format!("Workflow not found: {}", id))?;
    serde_json::from_str::<serde_json::Value>(&content)
        .map_err(|e| format!("Invalid workflow JSON: {}", e))
}

#[tauri::command]
pub fn delete_workflow(id: String) -> Result<(), String> {
    let safe_id: String = id.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let path = workflows_dir().join(format!("{}.json", safe_id));
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Failed to delete workflow: {}", e))?;
    }
    Ok(())
}

/// List built-in workflow templates
#[tauri::command]
pub fn list_workflow_templates() -> Result<Vec<WorkflowTemplate>, String> {
    // Built-in templates defined in Rust (matching frontend templates)
    let templates = vec![
        WorkflowTemplate {
            id: "tpl-webhook-to-slack".to_string(),
            name: "Webhook to Slack".to_string(),
            description: "Receive webhook, process with Claude, post to Slack".to_string(),
            category: "Notifications".to_string(),
            tags: vec!["webhook".to_string(), "slack".to_string(), "notifications".to_string()],
            version: "1.0.0".to_string(),
            is_built_in: true,
            nodes: serde_json::json!([]),
            edges: serde_json::json!([]),
        },
        WorkflowTemplate {
            id: "tpl-parallel-deploy".to_string(),
            name: "Parallel Deployment".to_string(),
            description: "Deploy to multiple environments in parallel with retry".to_string(),
            category: "CI/CD".to_string(),
            tags: vec!["parallel".to_string(), "deployment".to_string(), "retry".to_string()],
            version: "1.0.0".to_string(),
            is_built_in: true,
            nodes: serde_json::json!([]),
            edges: serde_json::json!([]),
        },
        WorkflowTemplate {
            id: "tpl-error-handling".to_string(),
            name: "Error Handling Pipeline".to_string(),
            description: "Process data with error handling and fallback".to_string(),
            category: "Data Processing".to_string(),
            tags: vec!["error-handling".to_string(), "try-catch".to_string(), "fallback".to_string()],
            version: "1.0.0".to_string(),
            is_built_in: true,
            nodes: serde_json::json!([]),
            edges: serde_json::json!([]),
        },
        WorkflowTemplate {
            id: "tpl-data-transform".to_string(),
            name: "Data Transform Pipeline".to_string(),
            description: "Transform data with variables and conditional logic".to_string(),
            category: "Data Processing".to_string(),
            tags: vec!["variables".to_string(), "transform".to_string(), "decision".to_string()],
            version: "1.0.0".to_string(),
            is_built_in: true,
            nodes: serde_json::json!([]),
            edges: serde_json::json!([]),
        },
    ];

    Ok(templates)
}
