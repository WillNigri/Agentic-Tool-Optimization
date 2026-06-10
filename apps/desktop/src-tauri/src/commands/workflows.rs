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

// ── v2.14 migration ── file-based workflows → SQLite loops ──────────────
//
// The v2.13 Automations tab persisted workflows as `~/.ato/workflows/*.json`
// (one file per workflow, written by `save_workflow` above). v2.14 promoted
// loops to a first-class SQLite entity in `loops` / `loop_runs` / etc.
// (commands/loops.rs). This migration runs once on first launch with v2.14
// installed: for each on-disk workflow that hasn't already been migrated, it
// inserts an equivalent row in `loops` with `source='migrated-from-automations'`
// and `source_ref=<original workflow id>`. Idempotent — re-running is a no-op
// because we skip workflow ids whose source_ref already exists in `loops`.
//
// Original JSON files are LEFT IN PLACE (not deleted, not renamed). The
// migrated `loops` row is the new canonical record; the user can clean up
// `~/.ato/workflows/` by hand once they're confident the migration looks
// right. This is deliberately conservative — migration bugs are easier to
// recover from when the source data still exists.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowMigrationReport {
    pub scanned: usize,
    pub migrated: usize,
    pub skipped_already_migrated: usize,
    pub skipped_parse_error: usize,
}

#[tauri::command]
pub fn migrate_workflows_to_loops(db: tauri::State<'_, crate::DbState>) -> Result<WorkflowMigrationReport, String> {
    let dir = workflows_dir();
    let mut report = WorkflowMigrationReport {
        scanned: 0,
        migrated: 0,
        skipped_already_migrated: 0,
        skipped_parse_error: 0,
    };

    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(report), // No directory = nothing to migrate.
    };

    let conn = db.0.lock().map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().map_or(false, |ext| ext == "json") {
            continue;
        }
        report.scanned += 1;

        let raw = match read_file_lossy(&path) {
            Some(s) => s,
            None => {
                report.skipped_parse_error += 1;
                continue;
            }
        };
        let workflow: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => {
                report.skipped_parse_error += 1;
                continue;
            }
        };

        let original_id = match workflow.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => {
                report.skipped_parse_error += 1;
                continue;
            }
        };

        // Idempotency — skip if we already migrated this workflow id.
        let already: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM loops
                  WHERE source = 'migrated-from-automations' AND source_ref = ?1",
                rusqlite::params![original_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if already > 0 {
            report.skipped_already_migrated += 1;
            continue;
        }

        let name = workflow
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&original_id)
            .to_string();
        let description = workflow
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);
        let enabled = workflow
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let nodes = workflow
            .get("nodes")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let edges = workflow
            .get("edges")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let graph = serde_json::json!({ "nodes": nodes, "edges": edges });
        let graph_str = serde_json::to_string(&graph).unwrap_or_else(|_| "{}".into());

        // Slugify the original id into a valid slug for the loops table.
        // UNIQUE constraint on slug — auto-suffix on collision.
        let base_slug: String = original_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .chars()
            .take(64)
            .collect();
        let base_slug = if base_slug.is_empty() {
            format!("migrated-{}", report.migrated + 1)
        } else {
            base_slug
        };
        let slug = unique_loop_slug(&conn, &base_slug);

        let new_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let enabled_int: i32 = if enabled { 1 } else { 0 };

        let insert_result = conn.execute(
            "INSERT INTO loops (
                id, slug, name, description, enabled, graph, variables,
                trigger_kind, trigger_config, source, source_ref,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 'manual', NULL,
                      'migrated-from-automations', ?7, ?8, ?8)",
            rusqlite::params![
                new_id,
                slug,
                name,
                description,
                enabled_int,
                graph_str,
                original_id,
                now,
            ],
        );
        match insert_result {
            Ok(_) => report.migrated += 1,
            Err(_) => report.skipped_parse_error += 1,
        }
    }

    Ok(report)
}

fn unique_loop_slug(conn: &rusqlite::Connection, base: &str) -> String {
    let mut candidate = base.to_string();
    let mut suffix = 2;
    loop {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM loops WHERE slug = ?1",
                rusqlite::params![candidate],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return candidate;
        }
        candidate = format!("{}-{}", base, suffix);
        suffix += 1;
        if suffix > 1000 {
            return format!("{}-{}", base, uuid::Uuid::new_v4());
        }
    }
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
