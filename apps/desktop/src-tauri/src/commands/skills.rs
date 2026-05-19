// commands/skills.rs — Local skills read surface.
//
// PR 27b of the commands.rs split (see COMMANDS_SPLIT_PLAN.md), second
// slice of the skills_mcps domain. The read commands move first
// (PR 27b); skill mutation (`create_skill`, `delete_skill`, `update_skill`,
// version commands) follows in PR 27c, and MCP discovery + install
// follows in PR 27d. The split keeps each PR auditable.
//
// Scope (3 commands):
//   - get_local_skills      — scan home + project dirs across all 5
//                             runtimes (claude/codex/openclaw/hermes/
//                             gemini); merge OpenClaw workspace pseudo-
//                             skills (AGENTS.md / SOUL.md / TOOLS.md)
//                             and Hermes SOUL.md.
//   - get_skill_detail      — single skill by id; re-runs the scan to
//                             find it (same logic as list), then loads
//                             content + frontmatter + scripts/refs/assets.
//   - toggle_local_skill    — UPSERT into the skill_toggles table.
//
// Cross-domain helpers reached via super::* — they have callers outside
// the skills domain (project_bundle, agents) so they stay in mod.rs:
//   collect_skills, collect_skills_for_project, discover_project_roots,
//   claude_home, read_file_lossy, parse_frontmatter, content_hash,
//   list_subdir_files, estimate_tokens.

use rusqlite::params;
use std::fs;
use std::path::PathBuf;
use tauri::State;

use crate::{home_dir, DbState, LocalSkill, SkillDetail};

#[tauri::command]
pub fn get_local_skills(db: State<'_, DbState>) -> Result<Vec<LocalSkill>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut skills = Vec::new();

    // ── Personal skills (global, always scanned) ──
    // Claude
    skills.extend(super::collect_skills(
        &super::claude_home().join("skills"),
        "personal",
        "claude",
        &conn,
    ));
    skills.extend(super::collect_skills(
        &PathBuf::from("/etc/claude/skills"),
        "enterprise",
        "claude",
        &conn,
    ));
    let plugins_dir = super::claude_home().join("plugins");
    if plugins_dir.exists() {
        if let Ok(entries) = fs::read_dir(&plugins_dir) {
            for entry in entries.flatten() {
                let plugin_skills = entry.path().join("skills");
                if plugin_skills.exists() {
                    skills.extend(super::collect_skills(
                        &plugin_skills,
                        "plugin",
                        "claude",
                        &conn,
                    ));
                }
            }
        }
    }
    // Codex
    let codex_home = PathBuf::from(
        std::env::var("CODEX_HOME")
            .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()),
    );
    skills.extend(super::collect_skills(&codex_home.join("skills"), "personal", "codex", &conn));
    // OpenClaw
    let openclaw_home = PathBuf::from(std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| {
        home_dir()
            .join(".openclaw")
            .to_string_lossy()
            .to_string()
    }));
    skills.extend(super::collect_skills(
        &openclaw_home.join("skills"),
        "personal",
        "openclaw",
        &conn,
    ));
    // Hermes
    let hermes_home = home_dir().join(".hermes");
    let hermes_skills_dir = hermes_home.join("skills");
    skills.extend(super::collect_skills(&hermes_skills_dir, "personal", "hermes", &conn));
    if hermes_skills_dir.exists() {
        if let Ok(entries) = fs::read_dir(&hermes_skills_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    skills.extend(super::collect_skills(
                        &entry.path(),
                        "personal",
                        "hermes",
                        &conn,
                    ));
                }
            }
        }
    }

    // ── Project skills (scan ALL discovered projects) ──
    let projects = super::discover_project_roots();
    for proj in &projects {
        let proj_name = proj
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| proj.to_string_lossy().to_string());

        // Claude project skills
        let claude_proj = proj.join(".claude").join("skills");
        if claude_proj.exists() {
            skills.extend(super::collect_skills_for_project(
                &claude_proj,
                "project",
                "claude",
                Some(&proj_name),
                &conn,
            ));
        }

        // Codex project skills
        for codex_dir in [proj.join(".codex").join("skills"), proj.join(".agents").join("skills")] {
            if codex_dir.exists() {
                skills.extend(super::collect_skills_for_project(
                    &codex_dir,
                    "project",
                    "codex",
                    Some(&proj_name),
                    &conn,
                ));
            }
        }

        // OpenClaw project skills
        let oc_proj = proj.join(".openclaw").join("skills");
        if oc_proj.exists() {
            skills.extend(super::collect_skills_for_project(
                &oc_proj,
                "project",
                "openclaw",
                Some(&proj_name),
                &conn,
            ));
        }

        // Hermes project skills
        let hermes_proj = proj.join(".hermes").join("skills");
        if hermes_proj.exists() {
            skills.extend(super::collect_skills_for_project(
                &hermes_proj,
                "project",
                "hermes",
                Some(&proj_name),
                &conn,
            ));
        }
    }

    // ── OpenClaw workspace pseudo-skills (AGENTS.md, SOUL.md, TOOLS.md) ──
    skills.extend(super::collect_skills(
        &openclaw_home.join("workspace").join("skills"),
        "personal",
        "openclaw",
        &conn,
    ));
    let oc_workspace = openclaw_home.join("workspace");
    if oc_workspace.exists() {
        for fname in ["AGENTS.md", "SOUL.md", "TOOLS.md"] {
            let fpath = oc_workspace.join(fname);
            if fpath.exists() {
                if let Some(content) = super::read_file_lossy(&fpath) {
                    let (fm, _) = super::parse_frontmatter(&content);
                    let desc = fm
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("OpenClaw workspace config")
                        .to_string();
                    let hash = super::content_hash(&content);
                    let fp_str = fpath.to_string_lossy().to_string();
                    let enabled: bool = conn
                        .query_row(
                            "SELECT enabled FROM skill_toggles WHERE file_path = ?1",
                            params![&fp_str],
                            |row| row.get(0),
                        )
                        .unwrap_or(true);
                    skills.push(LocalSkill {
                        id: super::content_hash(&fp_str),
                        name: fname.replace(".md", "").to_string(),
                        description: desc,
                        file_path: fp_str,
                        scope: "personal".to_string(),
                        runtime: "openclaw".to_string(),
                        project: None,
                        token_count: super::estimate_tokens(content.len() as u64),
                        enabled,
                        content_hash: hash,
                    });
                }
            }
        }
    }

    // ── Hermes pseudo-skills (SOUL.md) ──
    let hermes_soul = hermes_home.join("SOUL.md");
    if hermes_soul.exists() {
        if let Some(content) = super::read_file_lossy(&hermes_soul) {
            let hash = super::content_hash(&content);
            let fp_str = hermes_soul.to_string_lossy().to_string();
            let enabled: bool = conn
                .query_row(
                    "SELECT enabled FROM skill_toggles WHERE file_path = ?1",
                    params![&fp_str],
                    |row| row.get(0),
                )
                .unwrap_or(true);
            skills.push(LocalSkill {
                id: super::content_hash(&fp_str),
                name: "SOUL".to_string(),
                description: "Hermes persona and identity".to_string(),
                file_path: fp_str,
                scope: "personal".to_string(),
                runtime: "hermes".to_string(),
                project: None,
                token_count: super::estimate_tokens(content.len() as u64),
                enabled,
                content_hash: hash,
            });
        }
    }

    Ok(skills)
}

#[tauri::command]
pub fn get_skill_detail(db: State<'_, DbState>, id: String) -> Result<SkillDetail, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Reuse the same scanning logic as get_local_skills so we find skills
    // from all discovered projects (not just CWD-based project_root)
    let mut all_skills = Vec::new();
    // Claude personal + enterprise
    all_skills.extend(super::collect_skills(
        &super::claude_home().join("skills"),
        "personal",
        "claude",
        &conn,
    ));
    all_skills.extend(super::collect_skills(
        &PathBuf::from("/etc/claude/skills"),
        "enterprise",
        "claude",
        &conn,
    ));
    // Codex personal
    let codex_home = PathBuf::from(
        std::env::var("CODEX_HOME")
            .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()),
    );
    all_skills.extend(super::collect_skills(&codex_home.join("skills"), "personal", "codex", &conn));
    // OpenClaw personal
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| {
        home_dir()
            .join(".openclaw")
            .to_string_lossy()
            .to_string()
    }));
    all_skills.extend(super::collect_skills(&oc_home.join("skills"), "personal", "openclaw", &conn));
    all_skills.extend(super::collect_skills(
        &oc_home.join("workspace").join("skills"),
        "personal",
        "openclaw",
        &conn,
    ));
    // Hermes personal
    let hermes_skills = home_dir().join(".hermes").join("skills");
    all_skills.extend(super::collect_skills(&hermes_skills, "personal", "hermes", &conn));
    if hermes_skills.exists() {
        if let Ok(entries) = fs::read_dir(&hermes_skills) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    all_skills.extend(super::collect_skills(
                        &entry.path(),
                        "personal",
                        "hermes",
                        &conn,
                    ));
                }
            }
        }
    }
    // Project skills from ALL discovered projects
    let projects = super::discover_project_roots();
    for proj in &projects {
        all_skills.extend(super::collect_skills(
            &proj.join(".claude").join("skills"),
            "project",
            "claude",
            &conn,
        ));
        all_skills.extend(super::collect_skills(
            &proj.join(".agents").join("skills"),
            "project",
            "codex",
            &conn,
        ));
        all_skills.extend(super::collect_skills(
            &proj.join(".codex").join("skills"),
            "project",
            "codex",
            &conn,
        ));
    }

    let skill = all_skills
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("Skill not found: {}", id))?;

    let is_directory = skill.file_path.ends_with('/');
    let base_path = PathBuf::from(&skill.file_path);

    let content = if is_directory {
        super::read_file_lossy(&base_path.join("SKILL.md")).unwrap_or_default()
    } else {
        super::read_file_lossy(&PathBuf::from(&skill.file_path)).unwrap_or_default()
    };

    let (frontmatter, _body) = super::parse_frontmatter(&content);

    let (has_scripts, scripts) = if is_directory {
        super::list_subdir_files(&base_path, "scripts")
    } else {
        (false, vec![])
    };
    let (has_references, references) = if is_directory {
        super::list_subdir_files(&base_path, "references")
    } else {
        (false, vec![])
    };
    let (has_assets, assets) = if is_directory {
        super::list_subdir_files(&base_path, "assets")
    } else {
        (false, vec![])
    };

    Ok(SkillDetail {
        id: skill.id.clone(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        file_path: skill.file_path.clone(),
        scope: skill.scope.clone(),
        runtime: skill.runtime.clone(),
        token_count: skill.token_count,
        enabled: skill.enabled,
        content_hash: skill.content_hash.clone(),
        content,
        frontmatter,
        has_scripts,
        has_references,
        has_assets,
        scripts,
        references,
        assets,
        is_directory,
    })
}

#[tauri::command]
pub fn toggle_local_skill(
    db: State<'_, DbState>,
    file_path: String,
    enabled: bool,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO skill_toggles (file_path, enabled) VALUES (?1, ?2)
         ON CONFLICT(file_path) DO UPDATE SET enabled = excluded.enabled",
        params![file_path, enabled as i32],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
