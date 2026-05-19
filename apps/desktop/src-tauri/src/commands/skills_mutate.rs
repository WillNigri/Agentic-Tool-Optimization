// commands/skills_mutate.rs — Skill mutation + version history.
//
// PR 27c of the commands.rs split (see COMMANDS_SPLIT_PLAN.md), third
// slice of the skills_mcps domain. Read commands moved in PR 27b; this
// PR adds the write surface. MCP discovery + install (PR 27d) and
// openclaw_list_skills + project-skills helpers (PR 27e) follow.
//
// Scope (6 commands + 1 helper + 1 struct):
//   - create_skill              — write a new SKILL.md (or .md file) +
//                                 frontmatter to the per-runtime skills
//                                 dir; uses skill_dir_for_runtime to
//                                 resolve the path.
//   - delete_skill              — find by content-hash id across all
//                                 runtime dirs, remove file or directory.
//   - update_skill              — find by id, snapshot prior contents
//                                 into skill_versions, then overwrite.
//   - list_skill_versions       — paged version history for one
//                                 file_path (newest first, cap 100).
//   - restore_skill_version     — load a snapshot, auto-snapshot current
//                                 before restoring (reversible).
//   - delete_skill_version      — drop one snapshot.
//   - skill_dir_for_runtime     — pub helper; resolves
//                                 (runtime, scope) → PathBuf.
//   - SkillVersion struct       — row shape for the skill_versions table.
//   - snapshot_skill_version    — private helper used by create/update.

use rusqlite::params;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use tauri::State;

use crate::{home_dir, DbState, SkillDetail};

/// Resolve the skill directory for a given runtime + scope
pub fn skill_dir_for_runtime(runtime: &str, scope: &str) -> PathBuf {
    match (runtime, scope) {
        // Claude
        ("claude", "enterprise") => PathBuf::from("/etc/claude/skills"),
        ("claude", "personal") => super::claude_home().join("skills"),
        ("claude", "project") => super::project_root().join(".claude/skills"),
        ("claude", "plugin") => super::claude_home().join("plugins"),
        // Codex
        ("codex", "personal") => {
            let home = std::env::var("CODEX_HOME")
                .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string());
            PathBuf::from(home).join("skills")
        }
        ("codex", "project") => super::project_root().join(".codex").join("skills"),
        // OpenClaw
        ("openclaw", "personal") => {
            let home = std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| {
                home_dir().join(".openclaw").to_string_lossy().to_string()
            });
            PathBuf::from(home).join("skills")
        }
        ("openclaw", "project") => {
            let home = std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| {
                home_dir().join(".openclaw").to_string_lossy().to_string()
            });
            PathBuf::from(home).join("workspace").join("skills")
        }
        // Hermes
        ("hermes", _) => home_dir().join(".hermes").join("skills"),
        // Fallback
        (_, "personal") => super::claude_home().join("skills"),
        (_, "project") => super::project_root().join(".claude").join("skills"),
        _ => super::claude_home().join("skills"),
    }
}

#[tauri::command]
pub fn create_skill(data: String) -> Result<SkillDetail, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(&data).map_err(|e| format!("Invalid skill data: {}", e))?;

    let name = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unnamed")
        .to_string();
    let description = parsed
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let scope = parsed.get("scope").and_then(|v| v.as_str()).unwrap_or("personal");
    let runtime = parsed
        .get("runtime")
        .and_then(|v| v.as_str())
        .unwrap_or("claude");
    let content = parsed
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let is_directory = parsed
        .get("isDirectory")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let skills_dir = skill_dir_for_runtime(runtime, scope);
    fs::create_dir_all(&skills_dir)
        .map_err(|e| format!("Failed to create skills directory: {}", e))?;

    let (file_path, file_path_str) = if is_directory {
        let dir_path = skills_dir.join(&name);
        fs::create_dir_all(&dir_path)
            .map_err(|e| format!("Failed to create skill directory: {}", e))?;
        // Create subdirectories
        fs::create_dir_all(dir_path.join("scripts")).ok();
        fs::create_dir_all(dir_path.join("references")).ok();
        fs::create_dir_all(dir_path.join("assets")).ok();
        let skill_md = dir_path.join("SKILL.md");
        let fp_str = format!("{}/", dir_path.to_string_lossy());
        (skill_md, fp_str)
    } else {
        let file = skills_dir.join(format!("{}.md", name));
        let fp_str = file.to_string_lossy().to_string();
        (file, fp_str)
    };

    fs::write(&file_path, &content).map_err(|e| format!("Failed to write skill: {}", e))?;

    let (frontmatter, _) = super::parse_frontmatter(&content);
    let hash = super::content_hash(&content);

    Ok(SkillDetail {
        id: super::content_hash(&file_path_str),
        name,
        description,
        file_path: file_path_str,
        scope: scope.to_string(),
        runtime: runtime.to_string(),
        token_count: super::estimate_tokens(content.len() as u64),
        enabled: true,
        content_hash: hash,
        content,
        frontmatter,
        has_scripts: is_directory,
        has_references: is_directory,
        has_assets: is_directory,
        scripts: vec![],
        references: vec![],
        assets: vec![],
        is_directory,
    })
}

#[tauri::command]
pub fn delete_skill(id: String) -> Result<(), String> {
    // Scan ALL runtime directories to find the skill
    let codex_home = PathBuf::from(
        std::env::var("CODEX_HOME")
            .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()),
    );
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| {
        home_dir().join(".openclaw").to_string_lossy().to_string()
    }));

    let dirs = vec![
        super::claude_home().join("skills"),
        super::project_root().join(".claude").join("skills"),
        codex_home.join("skills"),
        super::project_root().join(".codex").join("skills"),
        oc_home.join("skills"),
        oc_home.join("workspace").join("skills"),
        home_dir().join(".hermes").join("skills"),
    ];

    for dir in dirs {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_path_str = if path.is_dir() {
                    format!("{}/", path.to_string_lossy())
                } else {
                    path.to_string_lossy().to_string()
                };

                if super::content_hash(&file_path_str) == id {
                    if path.is_dir() {
                        fs::remove_dir_all(&path)
                            .map_err(|e| format!("Failed to delete skill directory: {}", e))?;
                    } else {
                        fs::remove_file(&path)
                            .map_err(|e| format!("Failed to delete skill file: {}", e))?;
                    }
                    return Ok(());
                }
            }
        }
    }

    Err(format!("Skill not found: {}", id))
}

#[tauri::command]
pub fn update_skill(
    db: State<'_, DbState>,
    id: String,
    content: String,
) -> Result<(), String> {
    // Scan ALL runtime directories to find the matching skill by ID
    let codex_home = PathBuf::from(
        std::env::var("CODEX_HOME")
            .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()),
    );
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| {
        home_dir().join(".openclaw").to_string_lossy().to_string()
    }));

    let dirs = vec![
        // Claude
        super::claude_home().join("skills"),
        super::project_root().join(".claude").join("skills"),
        PathBuf::from("/etc/claude/skills"),
        // Codex
        codex_home.join("skills"),
        super::project_root().join(".agents").join("skills"),
        super::project_root().join(".codex").join("skills"),
        // OpenClaw
        oc_home.join("skills"),
        oc_home.join("workspace").join("skills"),
        // Hermes
        home_dir().join(".hermes").join("skills"),
    ];

    for dir in dirs {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_path_str = if path.is_dir() {
                    format!("{}/", path.to_string_lossy())
                } else {
                    path.to_string_lossy().to_string()
                };

                if super::content_hash(&file_path_str) == id {
                    let write_path = if path.is_dir() {
                        path.join("SKILL.md")
                    } else {
                        path
                    };

                    // Snapshot the prior contents into skill_versions before
                    // we overwrite. Best-effort — failures don't block the
                    // edit; the user came here to save, not to back up.
                    if let Ok(prior) = fs::read_to_string(&write_path) {
                        if prior != content {
                            let _ = snapshot_skill_version(
                                &db,
                                &write_path.to_string_lossy(),
                                &prior,
                                None,
                            );
                        }
                    }

                    fs::write(&write_path, &content).map_err(|e| e.to_string())?;
                    return Ok(());
                }
            }
        }
    }

    Err(format!("Skill not found: {}", id))
}

// ── Skill version history (Polish-T2) ────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillVersion {
    pub id: String,
    pub file_path: String,
    pub content: String,
    pub content_hash: String,
    pub note: Option<String>,
    pub created_at: String,
}

fn snapshot_skill_version(
    db: &State<'_, DbState>,
    write_path: &str,
    content: &str,
    note: Option<&str>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let hash = super::content_hash(content);
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO skill_versions (id, file_path, content, content_hash, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, write_path, content, hash, note, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn list_skill_versions(
    db: State<'_, DbState>,
    file_path: String,
) -> Result<Vec<SkillVersion>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, file_path, content, content_hash, note, created_at
             FROM skill_versions
             WHERE file_path = ?1
             ORDER BY created_at DESC
             LIMIT 100",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([&file_path], |row| {
            Ok(SkillVersion {
                id: row.get(0)?,
                file_path: row.get(1)?,
                content: row.get(2)?,
                content_hash: row.get(3)?,
                note: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn restore_skill_version(
    db: State<'_, DbState>,
    version_id: String,
) -> Result<(), String> {
    // Pull the snapshot.
    let (write_path, content) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT file_path, content FROM skill_versions WHERE id = ?1",
            [&version_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "Skill version not found".to_string(),
            other => other.to_string(),
        })?
    };

    // Snapshot the current contents so the restore is itself reversible.
    let path = PathBuf::from(&write_path);
    if let Ok(current) = fs::read_to_string(&path) {
        if current != content {
            let _ = snapshot_skill_version(
                &db,
                &write_path,
                &current,
                Some("auto-snapshot before restore"),
            );
        }
    }

    fs::write(&path, &content).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_skill_version(
    db: State<'_, DbState>,
    version_id: String,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM skill_versions WHERE id = ?1", [&version_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
