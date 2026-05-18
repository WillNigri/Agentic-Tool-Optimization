// commands/files_paths.rs — project file watching + .env import.
//
// PR 7 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope:
//   - `watch_project_files`     — start a per-project notify-backed watcher
//   - `stop_watching_project`   — stop one (or all) watchers
//   - `import_env_file`         — parse a .env file into env_vars rows
//
// `stop_watching_project` isn't in the plan's `files_paths` enumeration
// but it shares the `WATCHER_MAP` static + `get_watcher_map` helper
// with `watch_project_files` — splitting them across files would tear
// the watcher's state across two modules. Cohesion wins; the plan's
// 2-command enumeration was non-exhaustive on the watcher's pair.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::params;
use tauri::{Emitter as _, State};

use crate::{DbState, EnvVar};

// ── Project File Watcher (per-project) ──────────────────────────────────────

static WATCHER_MAP: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);

pub fn get_watcher_map() -> std::sync::MutexGuard<'static, Option<HashMap<String, bool>>> {
    let mut guard = WATCHER_MAP.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
}

#[tauri::command]
pub fn watch_project_files(app: tauri::AppHandle, project_path: String) -> Result<(), String> {
    {
        let mut map = get_watcher_map();
        let map = map.as_mut().unwrap();
        if map.get(&project_path) == Some(&true) {
            return Ok(());
        }
        map.insert(project_path.clone(), true);
    }

    let path = PathBuf::from(&project_path);
    if !path.exists() {
        let mut map = get_watcher_map();
        map.as_mut().unwrap().remove(&project_path);
        return Err("Project path does not exist".to_string());
    }

    let watched_path = project_path.clone();
    std::thread::spawn(move || {
        use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(_) => {
                let mut map = get_watcher_map();
                map.as_mut().unwrap().remove(&watched_path);
                return;
            }
        };
        let watch_paths = [
            path.join(".claude"),
            path.join(".codex"),
            path.join(".gemini"),
            path.join(".mcp.json"),
            path.join("CLAUDE.md"),
            path.join("GEMINI.md"),
            path.join("AGENTS.md"),
        ];
        for wp in &watch_paths {
            if wp.exists() {
                let mode = if wp.is_dir() { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
                let _ = watcher.watch(wp, mode);
            }
        }
        let mut last_emit = std::time::Instant::now();
        loop {
            let active = {
                let map = get_watcher_map();
                map.as_ref().unwrap().get(&watched_path) == Some(&true)
            };
            if !active { break; }

            match rx.recv_timeout(std::time::Duration::from_secs(1)) {
                Ok(Ok(_event)) => {
                    if last_emit.elapsed() > std::time::Duration::from_millis(500) {
                        let _ = app.emit("project-files-changed", &watched_path);
                        last_emit = std::time::Instant::now();
                    }
                }
                Ok(Err(_)) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let mut map = get_watcher_map();
        map.as_mut().unwrap().remove(&watched_path);
    });
    Ok(())
}

#[tauri::command]
pub fn stop_watching_project(project_path: Option<String>) {
    let mut map = get_watcher_map();
    if let Some(map) = map.as_mut() {
        match project_path {
            Some(path) => { map.remove(&path); }
            None => { map.clear(); }
        }
    }
}

/// Import environment variables from a .env file
#[tauri::command]
pub fn import_env_file(db: State<'_, DbState>, file_path: String, project_id: Option<String>, runtime: Option<String>) -> Result<Vec<EnvVar>, String> {
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut imported = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let value = line[eq_pos + 1..].trim().trim_matches('"').to_string();
            let id = uuid::Uuid::new_v4().to_string();

            conn.execute(
                "INSERT OR REPLACE INTO env_vars (id, project_id, runtime, key, value, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, project_id, runtime, key, value, now],
            ).map_err(|e| e.to_string())?;

            imported.push(EnvVar {
                id,
                project_id: project_id.clone(),
                runtime: runtime.clone(),
                key,
                value,
                created_at: now.clone(),
            });
        }
    }

    Ok(imported)
}
