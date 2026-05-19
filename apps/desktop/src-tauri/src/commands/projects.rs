// commands/projects.rs — Project Manager: discover, list, add, update,
// delete, set/get active.
//
// PR 16 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (7 commands):
//   - discover_projects         — scan common dev directories for agent configs
//   - list_projects             — list saved projects from DB
//   - add_project               — register a project path in the DB
//   - update_project            — rename a project
//   - delete_project            — remove a project (does NOT delete files)
//   - set_active_project        — flip is_active + bump last_accessed
//   - get_active_project        — current is_active=1 row (or None)
//
// Plus the Project + DiscoveredProject data shapes.
//
// Cross-domain helpers that *callers* of this module need —
// `count_project_skills`, `project_root`, `discover_project_roots` —
// stay in commands/mod.rs because skills/configs/runtime callers in
// other domains also use them. They will find their natural home when
// PR 27 (skills_mcps) and PR 28 (agents) extract.
//
// The big `get_project_bundle` command and its ProjectBundle wire
// type also stay in mod.rs for now: the bundle composer pulls
// helpers from across the codebase (file_ref, collect_skills_for_project,
// parse_sandbox_config, parse_approval_policies, parse_openclaw_workspace…)
// that themselves haven't moved yet. It travels with PR 27/28.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::State;

use crate::{home_dir, DbState};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub is_active: bool,
    pub skill_count: u32,
    pub last_accessed: Option<String>,
    pub created_at: String,
    // Computed fields (not stored in DB)
    pub has_claude: bool,
    pub has_codex: bool,
    pub has_hermes: bool,
    pub has_openclaw: bool,
    pub has_gemini: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredProject {
    pub path: String,
    pub name: String,  // Directory name
    pub skill_count: u32,
    pub runtimes: Vec<String>,  // Which runtimes have configs here
}

/// Discover projects on the system that have agent configurations
#[tauri::command]
pub fn discover_projects() -> Result<Vec<DiscoveredProject>, String> {
    let home = home_dir();
    let mut projects = Vec::new();
    let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Common development directories to scan
    let scan_dirs = vec![
        home.clone(),
        home.join("Documents"),
        home.join("Developer"),
        home.join("Projects"),
        home.join("Code"),
        home.join("repos"),
        home.join("src"),
        home.join("work"),
        home.join("dev"),
    ];

    for scan_dir in scan_dirs {
        if !scan_dir.exists() {
            continue;
        }

        // Only scan one level deep in these directories
        if let Ok(entries) = fs::read_dir(&scan_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let path_str = path.to_string_lossy().to_string();
                if seen_paths.contains(&path_str) {
                    continue;
                }

                // Check if this directory has any agent config
                let has_claude = path.join(".claude").exists() || path.join("CLAUDE.md").exists();
                let has_codex = path.join(".codex").exists() || path.join("AGENTS.md").exists();
                let has_hermes = path.join(".hermes").exists() || path.join("SOUL.md").exists();
                let has_openclaw = path.join("SOUL.md").exists() && path.join("TOOLS.md").exists();
                let has_gemini = path.join(".gemini").exists() || path.join("GEMINI.md").exists();

                if has_claude || has_codex || has_hermes || has_openclaw || has_gemini {
                    let mut runtimes = Vec::new();
                    if has_claude { runtimes.push("claude".to_string()); }
                    if has_codex { runtimes.push("codex".to_string()); }
                    if has_gemini { runtimes.push("gemini".to_string()); }
                    if has_hermes { runtimes.push("hermes".to_string()); }
                    if has_openclaw { runtimes.push("openclaw".to_string()); }

                    // Count skills
                    let skill_count = super::count_project_skills(&path);

                    let name = path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path_str.clone());

                    projects.push(DiscoveredProject {
                        path: path_str.clone(),
                        name,
                        skill_count,
                        runtimes,
                    });

                    seen_paths.insert(path_str);
                }
            }
        }
    }

    // Sort by name
    projects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(projects)
}

/// List all saved projects
#[tauri::command]
pub fn list_projects(db: State<'_, DbState>) -> Result<Vec<Project>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, name, path, is_active, skill_count, last_accessed, created_at FROM projects ORDER BY is_active DESC, last_accessed DESC"
    ).map_err(|e| e.to_string())?;

    let projects = stmt.query_map([], |row| {
        let path: String = row.get(2)?;
        let path_buf = PathBuf::from(&path);

        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            path: path.clone(),
            is_active: row.get::<_, i32>(3)? != 0,
            skill_count: row.get::<_, u32>(4)?,
            last_accessed: row.get(5)?,
            created_at: row.get(6)?,
            has_claude: path_buf.join(".claude").exists() || path_buf.join("CLAUDE.md").exists(),
            has_codex: path_buf.join(".codex").exists() || path_buf.join("AGENTS.md").exists(),
            has_hermes: path_buf.join(".hermes").exists() || path_buf.join("SOUL.md").exists(),
            has_openclaw: path_buf.join("SOUL.md").exists() && path_buf.join("TOOLS.md").exists(),
            has_gemini: path_buf.join(".gemini").exists() || path_buf.join("GEMINI.md").exists(),
        })
    }).map_err(|e| e.to_string())?;

    projects.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Add a project to the list
#[tauri::command]
pub fn add_project(db: State<'_, DbState>, name: String, path: String) -> Result<Project, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let path_buf = PathBuf::from(&path);

    if !path_buf.exists() {
        return Err("Project path does not exist".to_string());
    }

    let id = uuid::Uuid::new_v4().to_string();
    let skill_count = super::count_project_skills(&path_buf);
    let created_at = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO projects (id, name, path, is_active, skill_count, created_at) VALUES (?1, ?2, ?3, 0, ?4, ?5)",
        params![id, name, path, skill_count, created_at],
    ).map_err(|e| e.to_string())?;

    Ok(Project {
        id,
        name,
        path: path.clone(),
        is_active: false,
        skill_count,
        last_accessed: None,
        created_at,
        has_claude: path_buf.join(".claude").exists() || path_buf.join("CLAUDE.md").exists(),
        has_codex: path_buf.join(".codex").exists() || path_buf.join("AGENTS.md").exists(),
        has_hermes: path_buf.join(".hermes").exists() || path_buf.join("SOUL.md").exists(),
        has_openclaw: path_buf.join("SOUL.md").exists() && path_buf.join("TOOLS.md").exists(),
        has_gemini: path_buf.join(".gemini").exists() || path_buf.join("GEMINI.md").exists(),
    })
}

/// Update a project's name
#[tauri::command]
pub fn update_project(db: State<'_, DbState>, project_id: String, name: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE projects SET name = ?1 WHERE id = ?2",
        params![name, project_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Delete a project from the list (doesn't delete files)
#[tauri::command]
pub fn delete_project(db: State<'_, DbState>, project_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM projects WHERE id = ?1",
        params![project_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Set the active project
#[tauri::command]
pub fn set_active_project(db: State<'_, DbState>, project_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();

    // Deactivate all projects
    conn.execute("UPDATE projects SET is_active = 0", []).map_err(|e| e.to_string())?;

    // Activate the selected project and update last_accessed
    conn.execute(
        "UPDATE projects SET is_active = 1, last_accessed = ?1 WHERE id = ?2",
        params![now, project_id],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

/// Get the active project
#[tauri::command]
pub fn get_active_project(db: State<'_, DbState>) -> Result<Option<Project>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let result = conn.query_row(
        "SELECT id, name, path, is_active, skill_count, last_accessed, created_at FROM projects WHERE is_active = 1",
        [],
        |row| {
            let path: String = row.get(2)?;
            let path_buf = PathBuf::from(&path);

            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: path.clone(),
                is_active: true,
                skill_count: row.get::<_, u32>(4)?,
                last_accessed: row.get(5)?,
                created_at: row.get(6)?,
                has_claude: path_buf.join(".claude").exists() || path_buf.join("CLAUDE.md").exists(),
                has_codex: path_buf.join(".codex").exists() || path_buf.join("AGENTS.md").exists(),
                has_hermes: path_buf.join(".hermes").exists() || path_buf.join("SOUL.md").exists(),
                has_openclaw: path_buf.join("SOUL.md").exists() && path_buf.join("TOOLS.md").exists(),
                has_gemini: path_buf.join(".gemini").exists() || path_buf.join("GEMINI.md").exists(),
            })
        },
    );

    match result {
        Ok(project) => Ok(Some(project)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
