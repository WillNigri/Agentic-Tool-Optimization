// commands/workflow_webhooks.rs — Claude settings-file hooks +
// inbound webhook triggers for workflows.
//
// PR 13 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md). Per
// codex's Round 1 review, this is the "workflow side" of the old
// hooks_evals domain — agent-evaluator + agent-hook commands land
// later (PR 17 / agent_hooks_evals.rs) since they're a different
// subsystem entirely.
//
// Scope (7 commands):
//   - `get_hooks`                    — read Claude settings.json hooks
//   - `save_hook`                    — write one hook to settings.json
//   - `delete_hook`                  — remove one hook from settings.json
//   - `register_workflow_webhook`    — add a webhook trigger for a workflow
//   - `list_workflow_webhooks`       — enumerate webhook triggers
//   - `delete_workflow_webhook`      — remove a webhook trigger
//   - `toggle_workflow_webhook`      — flip the enabled flag
//
// Plus the HookConfig + WorkflowWebhook structs and the
// parse_hooks_from_settings helper that only get_hooks uses.

use std::fs;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use crate::DbState;
use super::{claude_home, project_root, read_file_lossy};

// ── Claude settings.json hooks ─────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HookConfig {
    pub id: String,
    pub name: String,
    pub event: String,
    pub command: String,
    pub matcher: Option<String>,
    pub timeout: Option<u64>,
    pub scope: String,
    pub enabled: bool,
}

/// Read hooks from Claude settings files (both global and project)
#[tauri::command]
pub fn get_hooks() -> Result<Vec<HookConfig>, String> {
    let mut hooks = Vec::new();

    // Check global settings
    let global_path = claude_home().join("settings.json");
    if let Some(content) = read_file_lossy(&global_path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(hooks_obj) = parsed.get("hooks").and_then(|h| h.as_object()) {
                parse_hooks_from_settings(hooks_obj, "global", &mut hooks);
            }
        }
    }

    // Check project settings
    let project_path = project_root().join(".claude").join("settings.json");
    if let Some(content) = read_file_lossy(&project_path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(hooks_obj) = parsed.get("hooks").and_then(|h| h.as_object()) {
                parse_hooks_from_settings(hooks_obj, "project", &mut hooks);
            }
        }
    }

    // Also check local settings
    let local_path = claude_home().join("settings.local.json");
    if let Some(content) = read_file_lossy(&local_path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(hooks_obj) = parsed.get("hooks").and_then(|h| h.as_object()) {
                parse_hooks_from_settings(hooks_obj, "global", &mut hooks);
            }
        }
    }

    Ok(hooks)
}

pub fn parse_hooks_from_settings(
    hooks_obj: &serde_json::Map<String, serde_json::Value>,
    scope: &str,
    hooks: &mut Vec<HookConfig>,
) {
    // Claude hooks format:
    // "hooks": {
    //   "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "..." }] }]
    // }
    for (event_name, event_hooks) in hooks_obj {
        if let Some(hook_array) = event_hooks.as_array() {
            for (idx, hook_group) in hook_array.iter().enumerate() {
                let matcher = hook_group.get("matcher")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string());

                if let Some(inner_hooks) = hook_group.get("hooks").and_then(|h| h.as_array()) {
                    for (inner_idx, inner_hook) in inner_hooks.iter().enumerate() {
                        if let Some(command) = inner_hook.get("command").and_then(|c| c.as_str()) {
                            let id = format!("{}-{}-{}-{}", scope, event_name, idx, inner_idx);
                            let name = matcher.clone().unwrap_or_else(|| format!("{} hook {}", event_name, idx + 1));

                            let timeout = inner_hook.get("timeout")
                                .and_then(|t| t.as_u64());

                            hooks.push(HookConfig {
                                id,
                                name,
                                event: event_name.clone(),
                                command: command.to_string(),
                                matcher: matcher.clone(),
                                timeout,
                                scope: scope.to_string(),
                                enabled: true, // Claude doesn't have enabled flag, all hooks are enabled
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Save a hook to the appropriate settings file
#[tauri::command]
pub fn save_hook(hook: HookConfig) -> Result<(), String> {
    let settings_path = if hook.scope == "global" {
        claude_home().join("settings.json")
    } else {
        project_root().join(".claude").join("settings.json")
    };

    // Ensure directory exists
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    // Read existing settings or create new
    let mut settings: serde_json::Value = read_file_lossy(&settings_path)
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_else(|| json!({}));

    // Ensure hooks object exists
    if settings.get("hooks").is_none() {
        settings["hooks"] = json!({});
    }

    let hooks_obj = settings.get_mut("hooks").unwrap().as_object_mut().unwrap();

    // Ensure event array exists
    if !hooks_obj.contains_key(&hook.event) {
        hooks_obj.insert(hook.event.clone(), json!([]));
    }

    let event_hooks = hooks_obj.get_mut(&hook.event).unwrap().as_array_mut().unwrap();

    // Find existing hook group with same matcher or create new
    let matcher_val = hook.matcher.as_deref();
    let mut found = false;

    for hook_group in event_hooks.iter_mut() {
        let group_matcher = hook_group.get("matcher").and_then(|m| m.as_str());
        if group_matcher == matcher_val {
            // Update existing hook group
            if let Some(inner_hooks) = hook_group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                // Look for existing command or add new
                let mut updated = false;
                for inner_hook in inner_hooks.iter_mut() {
                    if inner_hook.get("command").and_then(|c| c.as_str()) == Some(&hook.command) {
                        // Update timeout if present
                        if let Some(timeout) = hook.timeout {
                            inner_hook["timeout"] = json!(timeout);
                        }
                        updated = true;
                        break;
                    }
                }
                if !updated {
                    let mut new_hook = json!({ "type": "command", "command": hook.command });
                    if let Some(timeout) = hook.timeout {
                        new_hook["timeout"] = json!(timeout);
                    }
                    inner_hooks.push(new_hook);
                }
            }
            found = true;
            break;
        }
    }

    if !found {
        // Create new hook group
        let mut new_group = json!({});
        if let Some(ref matcher) = hook.matcher {
            new_group["matcher"] = json!(matcher);
        }
        let mut new_hook = json!({ "type": "command", "command": hook.command });
        if let Some(timeout) = hook.timeout {
            new_hook["timeout"] = json!(timeout);
        }
        new_group["hooks"] = json!([new_hook]);
        event_hooks.push(new_group);
    }

    // Write back
    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&settings_path, content)
        .map_err(|e| format!("Failed to write settings: {}", e))?;

    Ok(())
}

/// Delete a hook from settings file
#[tauri::command]
pub fn delete_hook(hook_id: String) -> Result<(), String> {
    // Parse hook ID to determine scope, event, and indices
    let parts: Vec<&str> = hook_id.split('-').collect();
    if parts.len() < 4 {
        return Err("Invalid hook ID".to_string());
    }

    let scope = parts[0];
    let event = parts[1];

    let settings_path = if scope == "global" {
        claude_home().join("settings.json")
    } else {
        project_root().join(".claude").join("settings.json")
    };

    let content = read_file_lossy(&settings_path)
        .ok_or("Could not read settings file")?;

    let mut settings: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))?;

    // Get hooks for this event
    if let Some(hooks_obj) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        if let Some(event_hooks) = hooks_obj.get_mut(event).and_then(|e| e.as_array_mut()) {
            // Find and remove the hook - rebuild without the target hook
            // This is a simplified approach - in production you'd want more precise matching
            let group_idx: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let inner_idx: usize = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);

            if let Some(hook_group) = event_hooks.get_mut(group_idx) {
                if let Some(inner_hooks) = hook_group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                    if inner_idx < inner_hooks.len() {
                        inner_hooks.remove(inner_idx);
                    }

                    // If no more hooks in group, remove the group
                    if inner_hooks.is_empty() {
                        event_hooks.remove(group_idx);
                    }
                }
            }

            // If no more hooks for event, remove the event
            if event_hooks.is_empty() {
                hooks_obj.remove(event);
            }
        }
    }

    // Write back
    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&settings_path, content)
        .map_err(|e| format!("Failed to write settings: {}", e))?;

    Ok(())
}

// ── v0.8.0: Workflow Webhooks ─────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowWebhook {
    pub id: String,
    pub workflow_id: String,
    pub path: String,
    pub method: String,
    pub secret: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub last_triggered_at: Option<String>,
    pub trigger_count: u64,
}

/// Register a webhook for a workflow
#[tauri::command]
pub fn register_workflow_webhook(
    state: State<DbState>,
    workflow_id: String,
    path: String,
    method: String,
    secret: Option<String>,
) -> Result<WorkflowWebhook, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;

    // Ensure table exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS workflow_webhooks (
            id TEXT PRIMARY KEY,
            workflow_id TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            method TEXT NOT NULL DEFAULT 'POST',
            secret TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_triggered_at TEXT,
            trigger_count INTEGER NOT NULL DEFAULT 0
        )",
        [],
    ).map_err(|e| e.to_string())?;

    let id = format!("wh-{}", chrono::Utc::now().timestamp_millis());
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO workflow_webhooks (id, workflow_id, path, method, secret, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![&id, &workflow_id, &path, &method, &secret, &now],
    ).map_err(|e| e.to_string())?;

    Ok(WorkflowWebhook {
        id,
        workflow_id,
        path,
        method,
        secret,
        enabled: true,
        created_at: now,
        last_triggered_at: None,
        trigger_count: 0,
    })
}

/// List all registered webhooks
#[tauri::command]
pub fn list_workflow_webhooks(state: State<DbState>) -> Result<Vec<WorkflowWebhook>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;

    // Ensure table exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS workflow_webhooks (
            id TEXT PRIMARY KEY,
            workflow_id TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            method TEXT NOT NULL DEFAULT 'POST',
            secret TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_triggered_at TEXT,
            trigger_count INTEGER NOT NULL DEFAULT 0
        )",
        [],
    ).map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, workflow_id, path, method, secret, enabled, created_at, last_triggered_at, trigger_count
         FROM workflow_webhooks
         ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;

    let webhooks = stmt
        .query_map([], |row| {
            Ok(WorkflowWebhook {
                id: row.get(0)?,
                workflow_id: row.get(1)?,
                path: row.get(2)?,
                method: row.get(3)?,
                secret: row.get(4)?,
                enabled: row.get::<_, i32>(5)? == 1,
                created_at: row.get(6)?,
                last_triggered_at: row.get(7)?,
                trigger_count: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(webhooks)
}

/// Delete a webhook
#[tauri::command]
pub fn delete_workflow_webhook(state: State<DbState>, webhook_id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM workflow_webhooks WHERE id = ?1",
        params![&webhook_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Toggle webhook enabled state
#[tauri::command]
pub fn toggle_workflow_webhook(state: State<DbState>, webhook_id: String, enabled: bool) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE workflow_webhooks SET enabled = ?1 WHERE id = ?2",
        params![if enabled { 1 } else { 0 }, &webhook_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}
