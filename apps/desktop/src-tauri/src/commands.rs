// All Tauri command functions and helpers.
// Extracted from lib.rs for maintainability.

use crate::*;
use std::collections::HashMap;
use rusqlite::params;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use tauri::{State, Emitter};
use sha2::{Sha256, Digest};

// ── Helpers ──────────────────────────────────────────────────────────────

pub fn claude_home() -> PathBuf {
    home_dir().join(".claude")
}

pub fn gemini_home() -> PathBuf {
    home_dir().join(".gemini")
}

/// Find the project root by walking up from CWD looking for .git or .claude/
pub fn project_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    for _ in 0..10 {
        if dir.join(".git").exists() || dir.join(".claude").exists() || dir.join("CLAUDE.md").exists() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Discover all project directories that contain agent config (.claude/, .codex/, etc.)
/// Scans common development locations + user-configured paths.
pub fn discover_project_roots() -> Vec<PathBuf> {
    let home = home_dir();
    let mut roots = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Always include CWD project root
    let cwd_root = project_root();
    if cwd_root.join(".claude").exists() || cwd_root.join(".codex").exists()
       || cwd_root.join(".openclaw").exists() || cwd_root.join(".hermes").exists() {
        seen.insert(cwd_root.to_string_lossy().to_string());
        roots.push(cwd_root);
    }

    // Load user-configured project paths
    let config_path = home.join(".ato").join("projects.txt");
    if let Some(content) = read_file_lossy(&config_path) {
        for line in content.lines() {
            let p = PathBuf::from(line.trim());
            if p.exists() && !seen.contains(&p.to_string_lossy().to_string()) {
                seen.insert(p.to_string_lossy().to_string());
                roots.push(p);
            }
        }
    }

    // Scan common dev directories (1 level deep)
    let scan_dirs = vec![
        home.clone(),
        home.join("Documents"),
        home.join("Projects"),
        home.join("projects"),
        home.join("Desktop"),
        home.join("code"),
        home.join("Code"),
        home.join("dev"),
        home.join("Development"),
        home.join("workspace"),
        home.join("repos"),
        home.join("src"),
    ];

    for scan_dir in scan_dirs {
        if !scan_dir.exists() { continue; }
        if let Ok(entries) = fs::read_dir(&scan_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() { continue; }
                let key = path.to_string_lossy().to_string();
                if seen.contains(&key) { continue; }

                // Check if this directory has any agent config
                let has_agent_config = path.join(".claude").exists()
                    || path.join(".codex").exists()
                    || path.join(".openclaw").exists()
                    || path.join(".hermes").exists()
                    || path.join("CLAUDE.md").exists()
                    || path.join("AGENTS.md").exists();

                if has_agent_config {
                    seen.insert(key);
                    roots.push(path);
                }
            }
        }
    }

    roots
}

pub fn read_file_lossy(path: &PathBuf) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Estimate tokens from byte count (~4 bytes per token for English)
pub fn estimate_tokens(bytes: u64) -> u64 {
    bytes / 4
}

/// Simple hash of content for change detection
pub fn content_hash(content: &str) -> String {
    let mut hash: u64 = 5381;
    for byte in content.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    format!("{:x}", hash)
}

/// Parse YAML-like frontmatter from markdown content
pub fn parse_frontmatter(content: &str) -> (serde_json::Value, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        let desc = content.lines()
            .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .unwrap_or("")
            .trim()
            .to_string();
        return (serde_json::json!({"description": desc}), content.to_string());
    }

    let after_first = &trimmed[3..];
    if let Some(end_idx) = after_first.find("\n---") {
        let fm_str = &after_first[..end_idx].trim();
        let body = &after_first[end_idx + 4..];

        let mut fm = serde_json::Map::new();
        for line in fm_str.lines() {
            if let Some(colon_pos) = line.find(':') {
                let key = line[..colon_pos].trim().to_string();
                let value = line[colon_pos + 1..].trim().to_string();
                // Handle boolean
                if value == "true" {
                    fm.insert(key, serde_json::Value::Bool(true));
                } else if value == "false" {
                    fm.insert(key, serde_json::Value::Bool(false));
                } else {
                    fm.insert(key, serde_json::Value::String(value));
                }
            }
        }

        // Parse allowed-tools into array
        if let Some(tools_val) = fm.get("allowed-tools").cloned() {
            if let Some(tools_str) = tools_val.as_str() {
                let tools: Vec<serde_json::Value> = tools_str
                    .split(',')
                    .map(|t| serde_json::Value::String(t.trim().to_string()))
                    .filter(|v| v.as_str().map_or(false, |s| !s.is_empty()))
                    .collect();
                fm.insert("allowedTools".to_string(), serde_json::Value::Array(tools));
            }
        }

        (serde_json::Value::Object(fm), body.to_string())
    } else {
        (serde_json::json!({}), content.to_string())
    }
}

/// Collect skills from a directory, supporting single files, SKILL.md directories,
/// symlinks (gstack-style), and nested subdirectories (one level deep).
pub fn collect_skills(dir: &PathBuf, scope: &str, runtime: &str, db: &Connection) -> Vec<LocalSkill> {
    collect_skills_for_project(dir, scope, runtime, None, db)
}

pub fn collect_skills_for_project(dir: &PathBuf, scope: &str, runtime: &str, project: Option<&str>, db: &Connection) -> Vec<LocalSkill> {
    let mut skills = Vec::new();
    if !dir.exists() {
        return skills;
    }

    collect_skills_inner(dir, scope, runtime, project, db, &mut skills, 0);
    skills
}

pub fn collect_skills_inner(dir: &PathBuf, scope: &str, runtime: &str, project: Option<&str>, db: &Connection, skills: &mut Vec<LocalSkill>, depth: u32) {
    // Limit recursion to 2 levels (handles gstack's ~/.claude/skills/gstack/*/SKILL.md)
    if depth > 2 { return; }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name;
        let content;
        let file_path_str;

        if path.is_dir() {
            // Directory skill — look for SKILL.md
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                content = read_file_lossy(&skill_md).unwrap_or_default();
                file_path_str = format!("{}/", path.to_string_lossy());
            } else {
                // No SKILL.md — recurse into subdirectory (handles gstack/ nested dirs)
                collect_skills_inner(&path, scope, runtime, project, db, skills, depth + 1);
                continue;
            }
        } else if path.extension().map_or(false, |ext| ext == "md") {
            name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            content = read_file_lossy(&path).unwrap_or_default();
            file_path_str = path.to_string_lossy().to_string();
        } else {
            continue;
        }

        let (fm, _body) = parse_frontmatter(&content);
        let description = fm.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let hash = content_hash(&content);
        let tokens = estimate_tokens(content.len() as u64);

        // Check toggle state from DB
        let enabled: bool = db
            .query_row(
                "SELECT enabled FROM skill_toggles WHERE file_path = ?1",
                params![&file_path_str],
                |row| row.get(0),
            )
            .unwrap_or(true); // Default enabled

        let id = content_hash(&file_path_str);

        skills.push(LocalSkill {
            id,
            name,
            description,
            file_path: file_path_str,
            scope: scope.to_string(),
            runtime: runtime.to_string(),
            project: project.map(|s| s.to_string()),
            token_count: tokens,
            enabled,
            content_hash: hash,
        });
    }
}

pub fn list_subdir_files(dir: &PathBuf, subdir: &str) -> (bool, Vec<String>) {
    let path = dir.join(subdir);
    if !path.exists() || !path.is_dir() {
        return (false, Vec::new());
    }
    let files: Vec<String> = fs::read_dir(&path)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    (true, files)
}

// ── Tauri Commands ───────────────────────────────────────────────────────

#[tauri::command]
pub fn get_local_skills(db: State<'_, DbState>) -> Result<Vec<LocalSkill>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut skills = Vec::new();

    // ── Personal skills (global, always scanned) ──
    // Claude
    skills.extend(collect_skills(&claude_home().join("skills"), "personal", "claude", &conn));
    skills.extend(collect_skills(&PathBuf::from("/etc/claude/skills"), "enterprise", "claude", &conn));
    let plugins_dir = claude_home().join("plugins");
    if plugins_dir.exists() {
        if let Ok(entries) = fs::read_dir(&plugins_dir) {
            for entry in entries.flatten() {
                let plugin_skills = entry.path().join("skills");
                if plugin_skills.exists() {
                    skills.extend(collect_skills(&plugin_skills, "plugin", "claude", &conn));
                }
            }
        }
    }
    // Codex
    let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
        .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
    skills.extend(collect_skills(&codex_home.join("skills"), "personal", "codex", &conn));
    // OpenClaw
    let openclaw_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));
    skills.extend(collect_skills(&openclaw_home.join("skills"), "personal", "openclaw", &conn));
    // Hermes
    let hermes_home = home_dir().join(".hermes");
    let hermes_skills_dir = hermes_home.join("skills");
    skills.extend(collect_skills(&hermes_skills_dir, "personal", "hermes", &conn));
    if hermes_skills_dir.exists() {
        if let Ok(entries) = fs::read_dir(&hermes_skills_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    skills.extend(collect_skills(&entry.path(), "personal", "hermes", &conn));
                }
            }
        }
    }

    // ── Project skills (scan ALL discovered projects) ──
    let projects = discover_project_roots();
    for proj in &projects {
        let proj_name = proj.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| proj.to_string_lossy().to_string());

        // Claude project skills
        let claude_proj = proj.join(".claude").join("skills");
        if claude_proj.exists() {
            skills.extend(collect_skills_for_project(&claude_proj, "project", "claude", Some(&proj_name), &conn));
        }

        // Codex project skills
        for codex_dir in [proj.join(".codex").join("skills"), proj.join(".agents").join("skills")] {
            if codex_dir.exists() {
                skills.extend(collect_skills_for_project(&codex_dir, "project", "codex", Some(&proj_name), &conn));
            }
        }

        // OpenClaw project skills
        let oc_proj = proj.join(".openclaw").join("skills");
        if oc_proj.exists() {
            skills.extend(collect_skills_for_project(&oc_proj, "project", "openclaw", Some(&proj_name), &conn));
        }

        // Hermes project skills
        let hermes_proj = proj.join(".hermes").join("skills");
        if hermes_proj.exists() {
            skills.extend(collect_skills_for_project(&hermes_proj, "project", "hermes", Some(&proj_name), &conn));
        }
    }

    // ── OpenClaw workspace pseudo-skills (AGENTS.md, SOUL.md, TOOLS.md) ──
    skills.extend(collect_skills(&openclaw_home.join("workspace").join("skills"), "personal", "openclaw", &conn));
    let oc_workspace = openclaw_home.join("workspace");
    if oc_workspace.exists() {
        for fname in ["AGENTS.md", "SOUL.md", "TOOLS.md"] {
            let fpath = oc_workspace.join(fname);
            if fpath.exists() {
                if let Some(content) = read_file_lossy(&fpath) {
                    let (fm, _) = parse_frontmatter(&content);
                    let desc = fm.get("description").and_then(|v| v.as_str()).unwrap_or("OpenClaw workspace config").to_string();
                    let hash = content_hash(&content);
                    let fp_str = fpath.to_string_lossy().to_string();
                    let enabled: bool = conn.query_row(
                        "SELECT enabled FROM skill_toggles WHERE file_path = ?1",
                        params![&fp_str], |row| row.get(0),
                    ).unwrap_or(true);
                    skills.push(LocalSkill {
                        id: content_hash(&fp_str),
                        name: fname.replace(".md", "").to_string(),
                        description: desc,
                        file_path: fp_str,
                        scope: "personal".to_string(),
                        runtime: "openclaw".to_string(),
                        project: None,
                        token_count: estimate_tokens(content.len() as u64),
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
        if let Some(content) = read_file_lossy(&hermes_soul) {
            let hash = content_hash(&content);
            let fp_str = hermes_soul.to_string_lossy().to_string();
            let enabled: bool = conn.query_row(
                "SELECT enabled FROM skill_toggles WHERE file_path = ?1",
                params![&fp_str], |row| row.get(0),
            ).unwrap_or(true);
            skills.push(LocalSkill {
                id: content_hash(&fp_str),
                name: "SOUL".to_string(),
                description: "Hermes persona and identity".to_string(),
                file_path: fp_str,
                scope: "personal".to_string(),
                runtime: "hermes".to_string(),
                        project: None,
                token_count: estimate_tokens(content.len() as u64),
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
    all_skills.extend(collect_skills(&claude_home().join("skills"), "personal", "claude", &conn));
    all_skills.extend(collect_skills(&PathBuf::from("/etc/claude/skills"), "enterprise", "claude", &conn));
    // Codex personal
    let codex_home = PathBuf::from(std::env::var("CODEX_HOME").unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
    all_skills.extend(collect_skills(&codex_home.join("skills"), "personal", "codex", &conn));
    // OpenClaw personal
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));
    all_skills.extend(collect_skills(&oc_home.join("skills"), "personal", "openclaw", &conn));
    all_skills.extend(collect_skills(&oc_home.join("workspace").join("skills"), "personal", "openclaw", &conn));
    // Hermes personal
    let hermes_skills = home_dir().join(".hermes").join("skills");
    all_skills.extend(collect_skills(&hermes_skills, "personal", "hermes", &conn));
    if hermes_skills.exists() {
        if let Ok(entries) = fs::read_dir(&hermes_skills) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    all_skills.extend(collect_skills(&entry.path(), "personal", "hermes", &conn));
                }
            }
        }
    }
    // Project skills from ALL discovered projects
    let projects = discover_project_roots();
    for proj in &projects {
        all_skills.extend(collect_skills(&proj.join(".claude").join("skills"), "project", "claude", &conn));
        all_skills.extend(collect_skills(&proj.join(".agents").join("skills"), "project", "codex", &conn));
        all_skills.extend(collect_skills(&proj.join(".codex").join("skills"), "project", "codex", &conn));
    }

    let skill = all_skills.iter().find(|s| s.id == id)
        .ok_or_else(|| format!("Skill not found: {}", id))?;

    let is_directory = skill.file_path.ends_with('/');
    let base_path = PathBuf::from(&skill.file_path);

    let content = if is_directory {
        read_file_lossy(&base_path.join("SKILL.md")).unwrap_or_default()
    } else {
        read_file_lossy(&PathBuf::from(&skill.file_path)).unwrap_or_default()
    };

    let (frontmatter, _body) = parse_frontmatter(&content);

    let (has_scripts, scripts) = if is_directory { list_subdir_files(&base_path, "scripts") } else { (false, vec![]) };
    let (has_references, references) = if is_directory { list_subdir_files(&base_path, "references") } else { (false, vec![]) };
    let (has_assets, assets) = if is_directory { list_subdir_files(&base_path, "assets") } else { (false, vec![]) };

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
pub fn toggle_local_skill(db: State<'_, DbState>, file_path: String, enabled: bool) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO skill_toggles (file_path, enabled) VALUES (?1, ?2)
         ON CONFLICT(file_path) DO UPDATE SET enabled = excluded.enabled",
        params![file_path, enabled as i32],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_context_estimate() -> Result<ContextBreakdown, String> {
    let mut categories = Vec::new();
    let mut total: u64 = 0;

    // System prompts (estimated)
    let system_tokens: u64 = 28000;
    categories.push(ContextCategory { name: "System Prompts".into(), tokens: system_tokens, color: "#FF4466".into() });
    total += system_tokens;

    // Skills
    let personal_dir = claude_home().join("skills");
    let project_dir = project_root().join(".claude").join("skills");
    let mut skill_bytes: u64 = 0;
    for dir in [&personal_dir, &project_dir] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    skill_bytes += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                } else if p.is_dir() {
                    let sm = p.join("SKILL.md");
                    skill_bytes += fs::metadata(&sm).map(|m| m.len()).unwrap_or(0);
                }
            }
        }
    }
    let skill_tokens = estimate_tokens(skill_bytes);
    categories.push(ContextCategory { name: format!("Skills"), tokens: skill_tokens, color: "#00FFB2".into() });
    total += skill_tokens;

    // CLAUDE.md
    let claude_md = project_root().join("CLAUDE.md");
    let claude_md_tokens = fs::metadata(&claude_md)
        .map(|m| estimate_tokens(m.len()))
        .unwrap_or(0);
    categories.push(ContextCategory { name: "CLAUDE.md".into(), tokens: claude_md_tokens, color: "#FFB800".into() });
    total += claude_md_tokens;

    // MCP schemas (estimated based on config)
    let settings_path = claude_home().join("settings.json");
    let mcp_tokens: u64 = if settings_path.exists() {
        if let Some(content) = read_file_lossy(&settings_path) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                let server_count = parsed.get("mcpServers")
                    .and_then(|v| v.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                (server_count as u64) * 2500 // ~2500 tokens per MCP server schema
            } else { 0 }
        } else { 0 }
    } else { 0 };
    categories.push(ContextCategory { name: "MCP Schemas".into(), tokens: mcp_tokens, color: "#3b82f6".into() });
    total += mcp_tokens;

    // Conversation (estimated from recent session)
    let conv_tokens: u64 = 15000; // rough estimate
    categories.push(ContextCategory { name: "Conversation".into(), tokens: conv_tokens, color: "#a78bfa".into() });
    total += conv_tokens;

    Ok(ContextBreakdown {
        total_tokens: total,
        limit: 200000,
        categories,
    })
}

/// Estimate byte size of all .md files in a directory (recursively one level)
pub fn dir_skill_bytes(dir: &PathBuf) -> u64 {
    let mut bytes: u64 = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().map_or(false, |e| e == "md") {
                bytes += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            } else if p.is_dir() {
                let sm = p.join("SKILL.md");
                bytes += fs::metadata(&sm).map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    bytes
}

pub fn file_tokens(path: &PathBuf) -> u64 {
    fs::metadata(path).map(|m| estimate_tokens(m.len())).unwrap_or(0)
}

#[tauri::command]
pub fn get_context_for_runtime(runtime: String) -> Result<ContextBreakdown, String> {
    // Check if runtime is installed — return limit=0 if not (frontend shows "not connected")
    let is_available = match runtime.as_str() {
        "claude" => which_claude().is_some(),
        "codex" => which_cli("codex").is_some(),
        "openclaw" => {
            let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
                .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));
            oc_home.exists() || load_openclaw_ssh_config().is_ok()
        }
        "hermes" => {
            which_cli("hermes").is_some() || home_dir().join(".hermes").exists()
        }
        _ => false,
    };

    if !is_available {
        return Ok(ContextBreakdown {
            total_tokens: 0,
            limit: 0, // Frontend uses limit=0 to detect "not connected"
            categories: Vec::new(),
        });
    }

    let mut categories = Vec::new();
    let mut total: u64 = 0;

    match runtime.as_str() {
        "claude" => {
            // Always loaded
            let sys: u64 = 28000;
            categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
            total += sys;
            // CLAUDE.md — always loaded
            let cm = file_tokens(&project_root().join("CLAUDE.md"));
            categories.push(ContextCategory { name: "CLAUDE.md".into(), tokens: cm, color: "#FFB800".into() });
            total += cm;
            // MCP schemas — always loaded
            let settings_path = claude_home().join("settings.json");
            let mcp: u64 = read_file_lossy(&settings_path)
                .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                .and_then(|v| v.get("mcpServers").and_then(|s| s.as_object()).map(|m| m.len() as u64 * 2500))
                .unwrap_or(0);
            categories.push(ContextCategory { name: "MCP Schemas".into(), tokens: mcp, color: "#3b82f6".into() });
            total += mcp;
            // Conversation — estimated active
            categories.push(ContextCategory { name: "Conversation".into(), tokens: 15000, color: "#a78bfa".into() });
            total += 15000;
            // Skills — on-demand, NOT counted in total (loaded only when triggered)
            let skill_bytes = dir_skill_bytes(&claude_home().join("skills"))
                + dir_skill_bytes(&project_root().join(".claude").join("skills"));
            let st = estimate_tokens(skill_bytes);
            categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: st, color: "#00FFB233".into() });
            // NOT added to total — skills are loaded individually when invoked

            Ok(ContextBreakdown { total_tokens: total, limit: 200000, categories })
        }
        "codex" => {
            let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
                .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
            // Always loaded
            let sys: u64 = 20000;
            categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
            total += sys;
            let agents_md = file_tokens(&project_root().join("AGENTS.md"))
                + file_tokens(&codex_home.join("AGENTS.md"));
            categories.push(ContextCategory { name: "AGENTS.md".into(), tokens: agents_md, color: "#FFB800".into() });
            total += agents_md;
            let cfg = file_tokens(&codex_home.join("config.toml"));
            categories.push(ContextCategory { name: "config.toml".into(), tokens: cfg, color: "#3b82f6".into() });
            total += cfg;
            categories.push(ContextCategory { name: "Conversation".into(), tokens: 12000, color: "#a78bfa".into() });
            total += 12000;
            // Skills — on-demand
            let skill_bytes = dir_skill_bytes(&codex_home.join("skills"))
                + dir_skill_bytes(&project_root().join(".agents").join("skills"))
                + dir_skill_bytes(&project_root().join(".codex").join("skills"));
            let st = estimate_tokens(skill_bytes);
            categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: st, color: "#00FFB233".into() });

            Ok(ContextBreakdown { total_tokens: total, limit: 192000, categories })
        }
        "openclaw" => {
            let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
                .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));
            let ws = oc_home.join("workspace");
            let has_local = ws.exists();

            if has_local {
                // Local OpenClaw install
                let sys: u64 = 15000;
                categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
                total += sys;
                let agents = file_tokens(&ws.join("AGENTS.md"));
                categories.push(ContextCategory { name: "AGENTS.md".into(), tokens: agents, color: "#FFB800".into() });
                total += agents;
                let soul = file_tokens(&ws.join("SOUL.md"));
                categories.push(ContextCategory { name: "SOUL.md".into(), tokens: soul, color: "#f97316".into() });
                total += soul;
                let tools = file_tokens(&ws.join("TOOLS.md"));
                categories.push(ContextCategory { name: "TOOLS.md".into(), tokens: tools, color: "#06b6d4".into() });
                total += tools;
                let mem_dir = ws.join("memory");
                let mem_bytes = dir_skill_bytes(&mem_dir);
                let mem = estimate_tokens(mem_bytes);
                categories.push(ContextCategory { name: "Memory".into(), tokens: mem, color: "#a78bfa".into() });
                total += mem;
                let skill_bytes = dir_skill_bytes(&oc_home.join("skills"))
                    + dir_skill_bytes(&ws.join("skills"));
                let st = estimate_tokens(skill_bytes);
                categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: st, color: "#00FFB233".into() });
            } else {
                // Remote OpenClaw via SSH — fetch file sizes
                let sys: u64 = 15000;
                categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
                total += sys;

                if let Ok(result) = openclaw_ssh_command(
                    "exec 'echo $(wc -c < ~/.openclaw/workspace/SOUL.md 2>/dev/null || echo 0) $(wc -c < ~/.openclaw/workspace/AGENTS.md 2>/dev/null || echo 0) $(wc -c < ~/.openclaw/workspace/TOOLS.md 2>/dev/null || echo 0) $(wc -c < ~/.openclaw/workspace/MEMORY.md 2>/dev/null || echo 0) $(find ~/.openclaw/workspace/skills -name SKILL.md -exec wc -c {} + 2>/dev/null | tail -1 | awk \"{print \\$1}\" || echo 0) $(find ~/.openclaw/workspace/memory -type f -exec wc -c {} + 2>/dev/null | tail -1 | awk \"{print \\$1}\" || echo 0)'"
                ) {
                    // Parse the response — it should be a string with space-separated byte counts
                    let text = result.as_str().unwrap_or("").trim().to_string();
                    let parts: Vec<u64> = text.split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect();

                    let soul_bytes = parts.first().copied().unwrap_or(0);
                    let agents_bytes = parts.get(1).copied().unwrap_or(0);
                    let tools_bytes = parts.get(2).copied().unwrap_or(0);
                    let memory_bytes = parts.get(3).copied().unwrap_or(0);
                    let skills_bytes = parts.get(4).copied().unwrap_or(0);
                    let mem_dir_bytes = parts.get(5).copied().unwrap_or(0);

                    let soul = estimate_tokens(soul_bytes);
                    categories.push(ContextCategory { name: "SOUL.md".into(), tokens: soul, color: "#f97316".into() });
                    total += soul;
                    let agents = estimate_tokens(agents_bytes);
                    categories.push(ContextCategory { name: "AGENTS.md".into(), tokens: agents, color: "#FFB800".into() });
                    total += agents;
                    let tools = estimate_tokens(tools_bytes);
                    categories.push(ContextCategory { name: "TOOLS.md".into(), tokens: tools, color: "#06b6d4".into() });
                    total += tools;
                    let mem = estimate_tokens(memory_bytes + mem_dir_bytes);
                    categories.push(ContextCategory { name: "Memory".into(), tokens: mem, color: "#a78bfa".into() });
                    total += mem;
                    let st = estimate_tokens(skills_bytes);
                    categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: st, color: "#00FFB233".into() });
                } else {
                    // SSH failed — show estimate
                    categories.push(ContextCategory { name: "SOUL.md (estimated)".into(), tokens: 2000, color: "#f97316".into() });
                    total += 2000;
                    categories.push(ContextCategory { name: "AGENTS.md (estimated)".into(), tokens: 500, color: "#FFB800".into() });
                    total += 500;
                }
            }

            Ok(ContextBreakdown { total_tokens: total, limit: 200000, categories })
        }
        "hermes" => {
            let hermes_home = home_dir().join(".hermes");
            // Always loaded
            let sys: u64 = 12000;
            categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
            total += sys;
            // SOUL.md
            let soul = file_tokens(&hermes_home.join("SOUL.md"));
            categories.push(ContextCategory { name: "SOUL.md".into(), tokens: soul, color: "#f97316".into() });
            total += soul;
            // Memory — always loaded
            let mem_bytes = file_tokens(&hermes_home.join("memories").join("MEMORY.md"))
                + file_tokens(&hermes_home.join("memories").join("USER.md"));
            categories.push(ContextCategory { name: "Memory".into(), tokens: mem_bytes, color: "#a78bfa".into() });
            total += mem_bytes;
            // Config
            let cfg = file_tokens(&hermes_home.join("config.yaml"));
            categories.push(ContextCategory { name: "config.yaml".into(), tokens: cfg, color: "#3b82f6".into() });
            total += cfg;
            // Skills — on-demand
            let skills_dir = hermes_home.join("skills");
            let mut skill_bytes = dir_skill_bytes(&skills_dir);
            if skills_dir.exists() {
                if let Ok(entries) = fs::read_dir(&skills_dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            skill_bytes += dir_skill_bytes(&entry.path());
                        }
                    }
                }
            }
            let st = estimate_tokens(skill_bytes);
            categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: st, color: "#00FFB233".into() });

            Ok(ContextBreakdown { total_tokens: total, limit: 128000, categories })
        }
        _ => get_context_estimate(),
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// PHASE 4: Live Session Tracking from Claude Code Logs
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LiveSessionData {
    pub session_id: Option<String>,
    pub project_path: Option<String>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub message_count: u64,
    pub tool_call_count: u64,
    pub files_read: Vec<SessionFileRead>,
    pub started_at: Option<String>,
    pub last_activity: Option<String>,
    pub model: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionFileRead {
    pub path: String,
    pub timestamp: String,
    pub token_estimate: u64,
}

/// Find the most recent Claude Code session for the current project
pub fn find_current_session() -> Option<(String, PathBuf)> {
    let claude_dir = claude_home();
    let projects_dir = claude_dir.join("projects");

    if !projects_dir.exists() {
        return None;
    }

    // Get current project path
    let current_project = project_root();
    let project_hash = current_project.to_string_lossy()
        .replace("/", "-")
        .replace("\\", "-");

    // Look for project directory matching current project
    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                // Check if this directory matches our project
                if dir_name.contains(&project_hash) || dir_name.starts_with("-Users-") {
                    // Find the most recent .jsonl file in this directory
                    if let Ok(sub_entries) = fs::read_dir(&path) {
                        let mut jsonl_files: Vec<PathBuf> = sub_entries
                            .flatten()
                            .filter(|e| {
                                e.path().extension()
                                    .map(|ext| ext == "jsonl")
                                    .unwrap_or(false)
                            })
                            .map(|e| e.path())
                            .collect();

                        // Sort by modification time (most recent first)
                        jsonl_files.sort_by(|a, b| {
                            let a_time = fs::metadata(a).and_then(|m| m.modified()).ok();
                            let b_time = fs::metadata(b).and_then(|m| m.modified()).ok();
                            b_time.cmp(&a_time)
                        });

                        if let Some(latest) = jsonl_files.first() {
                            let session_id = latest.file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_string();
                            return Some((session_id, latest.clone()));
                        }
                    }
                }
            }
        }
    }

    None
}

/// Parse a Claude Code session JSONL file to extract token usage and activity
pub fn parse_session_jsonl(path: &PathBuf) -> Result<LiveSessionData, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read session file: {}", e))?;

    let mut data = LiveSessionData {
        session_id: path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()),
        project_path: None,
        total_input_tokens: 0,
        total_output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        message_count: 0,
        tool_call_count: 0,
        files_read: Vec::new(),
        started_at: None,
        last_activity: None,
        model: None,
        is_active: true,
    };

    let mut seen_files: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
            // Track timestamps
            if let Some(ts) = entry.get("timestamp").and_then(|v| v.as_str()) {
                if data.started_at.is_none() {
                    data.started_at = Some(ts.to_string());
                }
                data.last_activity = Some(ts.to_string());
            }

            // Track project path
            if data.project_path.is_none() {
                if let Some(cwd) = entry.get("cwd").and_then(|v| v.as_str()) {
                    data.project_path = Some(cwd.to_string());
                }
            }

            // Extract token usage from assistant messages
            if let Some(msg) = entry.get("message") {
                if let Some(usage) = msg.get("usage") {
                    if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                        data.total_input_tokens += input;
                    }
                    if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                        data.total_output_tokens += output;
                    }
                    if let Some(cache_read) = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()) {
                        data.cache_read_tokens += cache_read;
                    }
                    if let Some(cache_create) = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()) {
                        data.cache_creation_tokens += cache_create;
                    }
                }

                // Track model
                if data.model.is_none() {
                    if let Some(model) = msg.get("model").and_then(|v| v.as_str()) {
                        data.model = Some(model.to_string());
                    }
                }

                // Count messages
                data.message_count += 1;

                // Look for tool_use in content to count tool calls
                if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
                    for item in content {
                        if let Some(content_type) = item.get("type").and_then(|v| v.as_str()) {
                            if content_type == "tool_use" {
                                data.tool_call_count += 1;

                                // Check if it's a Read tool call
                                if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                                    if name == "Read" || name == "read" {
                                        if let Some(input) = item.get("input") {
                                            if let Some(file_path) = input.get("file_path").and_then(|v| v.as_str()) {
                                                if !seen_files.contains(file_path) {
                                                    seen_files.insert(file_path.to_string());
                                                    let token_estimate = fs::metadata(file_path)
                                                        .map(|m| estimate_tokens(m.len()))
                                                        .unwrap_or(0);
                                                    data.files_read.push(SessionFileRead {
                                                        path: file_path.to_string(),
                                                        timestamp: entry.get("timestamp")
                                                            .and_then(|v| v.as_str())
                                                            .unwrap_or("")
                                                            .to_string(),
                                                        token_estimate,
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Check if session is recent (within last hour)
    if let Some(ref last) = data.last_activity {
        if let Ok(last_time) = chrono::DateTime::parse_from_rfc3339(last) {
            let now = chrono::Utc::now();
            let diff = now.signed_duration_since(last_time);
            data.is_active = diff.num_hours() < 1;
        }
    }

    Ok(data)
}

#[tauri::command]
pub fn get_live_session_data() -> Result<LiveSessionData, String> {
    match find_current_session() {
        Some((_session_id, path)) => parse_session_jsonl(&path),
        None => Ok(LiveSessionData {
            session_id: None,
            project_path: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            message_count: 0,
            tool_call_count: 0,
            files_read: Vec::new(),
            started_at: None,
            last_activity: None,
            model: None,
            is_active: false,
        }),
    }
}

/// Get context breakdown with live session data for Claude runtime
#[tauri::command]
pub fn get_live_context_breakdown() -> Result<ContextBreakdown, String> {
    let mut categories = Vec::new();
    let mut total: u64 = 0;

    // System prompts (estimated)
    let system_tokens: u64 = 28000;
    categories.push(ContextCategory { name: "System Prompts".into(), tokens: system_tokens, color: "#FF4466".into() });
    total += system_tokens;

    // CLAUDE.md
    let claude_md = project_root().join("CLAUDE.md");
    let claude_md_tokens = fs::metadata(&claude_md)
        .map(|m| estimate_tokens(m.len()))
        .unwrap_or(0);
    categories.push(ContextCategory { name: "CLAUDE.md".into(), tokens: claude_md_tokens, color: "#FFB800".into() });
    total += claude_md_tokens;

    // MCP schemas
    let settings_path = claude_home().join("settings.json");
    let mcp_tokens: u64 = read_file_lossy(&settings_path)
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|v| v.get("mcpServers").and_then(|s| s.as_object()).map(|m| m.len() as u64 * 2500))
        .unwrap_or(0);
    categories.push(ContextCategory { name: "MCP Schemas".into(), tokens: mcp_tokens, color: "#3b82f6".into() });
    total += mcp_tokens;

    // Live conversation data
    if let Ok(session) = get_live_session_data() {
        // Real conversation tokens from session
        let conv_tokens = session.total_input_tokens + session.total_output_tokens;
        categories.push(ContextCategory {
            name: format!("Conversation ({} msgs)", session.message_count),
            tokens: conv_tokens,
            color: "#a78bfa".into(),
        });
        total += conv_tokens;

        // Files read in session
        if !session.files_read.is_empty() {
            let files_tokens: u64 = session.files_read.iter().map(|f| f.token_estimate).sum();
            categories.push(ContextCategory {
                name: format!("Files Read ({} files)", session.files_read.len()),
                tokens: files_tokens,
                color: "#22c55e".into(),
            });
            // Note: files read are already counted in input tokens, so we don't add to total
        }

        // Cache info
        if session.cache_read_tokens > 0 || session.cache_creation_tokens > 0 {
            categories.push(ContextCategory {
                name: "Cache (read)".into(),
                tokens: session.cache_read_tokens,
                color: "#06b6d4".into(),
            });
        }
    } else {
        // Fallback to estimated conversation
        categories.push(ContextCategory { name: "Conversation".into(), tokens: 15000, color: "#a78bfa".into() });
        total += 15000;
    }

    // Skills (on-demand)
    let skill_bytes = dir_skill_bytes(&claude_home().join("skills"))
        + dir_skill_bytes(&project_root().join(".claude").join("skills"));
    let skill_tokens = estimate_tokens(skill_bytes);
    categories.push(ContextCategory { name: "Skills (on-demand)".into(), tokens: skill_tokens, color: "#00FFB233".into() });

    Ok(ContextBreakdown {
        total_tokens: total,
        limit: 200000,
        categories,
    })
}

// ══════════════════════════════════════════════════════════════════════════════
// PHASE 4: Real MCP Tool Discovery
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDetails {
    pub server_name: String,
    pub server_version: Option<String>,
    pub protocol_version: Option<String>,
    pub tools: Vec<McpTool>,
    pub connected: bool,
    pub error: Option<String>,
}

/// Discover tools from an MCP server by spawning it and communicating via JSON-RPC
pub fn discover_mcp_tools_stdio(command: &str, args: &[&str], env: &std::collections::HashMap<String, String>) -> Result<McpServerDetails, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    // Build the command. CRITICAL: inject the user's full shell PATH so the
    // spawned MCP server (and any tools it calls — `npx`, `node`, `python`)
    // can be found. Without this, GUI-launched Tauri's narrow PATH means
    // `npx @modelcontextprotocol/server-*` can't even find npx, and we
    // misreport "0 tools" for every MCP. Felipe + Beatriz hit this on
    // v1.5.20 — every MCP showed Error / 0 tools after the inheritance gap
    // surfaced.
    let user_path = get_user_path();
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Capture stderr so we can surface real spawn errors instead of a
        // silent "Failed to read response" — previously we ate everything
        // the server logged on its way to crashing.
        .stderr(Stdio::piped())
        .env("PATH", &user_path)
        .envs(env);

    // Spawn the process
    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to spawn MCP server '{}': {}", command, e))?;

    let stdin = child.stdin.as_mut()
        .ok_or("Failed to open stdin")?;
    let stdout = child.stdout.take()
        .ok_or("Failed to open stdout")?;
    let stderr_pipe = child.stderr.take();

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    // Drain stderr on demand — used when the server exits before we get a
    // valid response. Without this we'd report a generic "Failed to parse
    // response" with no clue that the actual problem was e.g. `npx: command
    // not found` or a missing API key.
    let drain_stderr = |stderr_pipe: Option<std::process::ChildStderr>| -> String {
        if let Some(mut s) = stderr_pipe {
            use std::io::Read;
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            return buf.trim().to_string();
        }
        String::new()
    };

    // Send initialize request
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "ATO",
                "version": "0.2.0"
            }
        }
    });

    writeln!(stdin, "{}", init_request.to_string())
        .map_err(|e| format!("Failed to write initialize request: {}", e))?;
    stdin.flush().map_err(|e| format!("Failed to flush stdin: {}", e))?;

    // Read initialize response.
    let mut read_response = || -> Result<serde_json::Value, String> {
        line.clear();
        let n = reader.read_line(&mut line)
            .map_err(|e| format!("Failed to read response: {}", e))?;
        if n == 0 {
            // Server closed stdout before sending anything — usually means
            // it crashed during init. The real diagnostic is in stderr.
            return Err("server exited before sending a response".to_string());
        }
        serde_json::from_str(&line)
            .map_err(|e| format!("Failed to parse response (got: {:?}): {}", line.trim(), e))
    };

    let init_response = match read_response() {
        Ok(v) => v,
        Err(e) => {
            let _ = child.kill();
            let stderr_msg = drain_stderr(stderr_pipe);
            if stderr_msg.is_empty() {
                return Err(e);
            }
            return Err(format!("{}\nstderr: {}", e, stderr_msg));
        }
    };

    // Extract server info
    let server_info = init_response.get("result")
        .and_then(|r| r.get("serverInfo"));
    let server_name = server_info
        .and_then(|i| i.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let server_version = server_info
        .and_then(|i| i.get("version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let protocol_version = init_response.get("result")
        .and_then(|r| r.get("protocolVersion"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Send tools/list request
    let tools_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });

    writeln!(stdin, "{}", tools_request.to_string())
        .map_err(|e| format!("Failed to write tools/list request: {}", e))?;
    stdin.flush().map_err(|e| format!("Failed to flush stdin: {}", e))?;

    // Read tools response
    let tools_response = read_response()?;

    // Parse tools
    let tools: Vec<McpTool> = tools_response.get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter().filter_map(|tool| {
                let name = tool.get("name")?.as_str()?.to_string();
                let description = tool.get("description")
                    .and_then(|d| d.as_str())
                    .map(|s| s.to_string());
                let input_schema = tool.get("inputSchema").cloned();
                Some(McpTool { name, description, input_schema })
            }).collect()
        })
        .unwrap_or_default();

    // Clean up - try to terminate the process gracefully
    let _ = child.kill();

    Ok(McpServerDetails {
        server_name,
        server_version,
        protocol_version,
        tools,
        connected: true,
        error: None,
    })
}

/// Parse MCP server config and discover tools
#[tauri::command]
pub fn discover_mcp_server_tools(server_name: String) -> Result<McpServerDetails, String> {
    // Find server config
    let settings_path = claude_home().join("settings.json");
    let content = read_file_lossy(&settings_path)
        .ok_or("Could not read Claude settings")?;

    let parsed: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))?;

    let mcp_servers = parsed.get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or("No mcpServers found in settings")?;

    // Extract server name without source suffix
    let clean_name = server_name.split(" (").next().unwrap_or(&server_name);

    let server_config = mcp_servers.get(clean_name)
        .ok_or(format!("Server '{}' not found", clean_name))?;

    // Extract command and args
    let command = server_config.get("command")
        .and_then(|c| c.as_str())
        .ok_or("Server has no command")?;

    let args: Vec<&str> = server_config.get("args")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    // Extract environment variables
    let mut env: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(env_obj) = server_config.get("env").and_then(|e| e.as_object()) {
        for (key, val) in env_obj {
            if let Some(s) = val.as_str() {
                env.insert(key.clone(), s.to_string());
            }
        }
    }

    // Try to discover tools
    match discover_mcp_tools_stdio(command, &args, &env) {
        Ok(details) => Ok(details),
        Err(e) => Ok(McpServerDetails {
            server_name: clean_name.to_string(),
            server_version: None,
            protocol_version: None,
            tools: Vec::new(),
            connected: false,
            error: Some(e),
        }),
    }
}

/// Get all MCP servers with discovered tools (runs discovery in parallel)
#[tauri::command]
pub fn get_mcp_servers_with_tools() -> Result<Vec<McpServerDetails>, String> {
    let settings_path = claude_home().join("settings.json");
    let content = read_file_lossy(&settings_path)
        .ok_or("Could not read Claude settings")?;

    let parsed: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))?;

    let mcp_servers = match parsed.get("mcpServers").and_then(|v| v.as_object()) {
        Some(servers) => servers,
        None => return Ok(Vec::new()),
    };

    let mut results = Vec::new();

    for (name, config) in mcp_servers {
        // Extract command
        let command = match config.get("command").and_then(|c| c.as_str()) {
            Some(c) => c,
            None => {
                results.push(McpServerDetails {
                    server_name: name.clone(),
                    server_version: None,
                    protocol_version: None,
                    tools: Vec::new(),
                    connected: false,
                    error: Some("No command specified".to_string()),
                });
                continue;
            }
        };

        let args: Vec<&str> = config.get("args")
            .and_then(|a| a.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let mut env: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if let Some(env_obj) = config.get("env").and_then(|e| e.as_object()) {
            for (key, val) in env_obj {
                if let Some(s) = val.as_str() {
                    env.insert(key.clone(), s.to_string());
                }
            }
        }

        // Try discovery with a timeout
        match discover_mcp_tools_stdio(command, &args, &env) {
            Ok(mut details) => {
                details.server_name = name.clone();
                results.push(details);
            }
            Err(e) => {
                results.push(McpServerDetails {
                    server_name: name.clone(),
                    server_version: None,
                    protocol_version: None,
                    tools: Vec::new(),
                    connected: false,
                    error: Some(e),
                });
            }
        }
    }

    Ok(results)
}

// ══════════════════════════════════════════════════════════════════════════════
// PHASE 4: Hooks Read/Write from Settings Files
// ══════════════════════════════════════════════════════════════════════════════

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

#[tauri::command]
pub fn get_local_config() -> Result<Vec<LocalMcpServer>, String> {
    // Dedupe by `(runtime-family, server-name)`. Felipe's screenshot showed
    // every Claude MCP listed twice — once for the global `~/.claude/settings.json`
    // ("claude") and once for the per-project `.claude/settings.json`
    // ("claude-project"). The same MCP shouldn't render as two cards just
    // because it's referenced in both scopes.
    use std::collections::BTreeMap;
    let mut seen: BTreeMap<(String, String), LocalMcpServer> = BTreeMap::new();

    let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
        .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));

    // (path, source-tag, runtime-family, scope-label).
    // runtime-family is what we dedupe by; scope-label is what we show.
    let config_paths: Vec<(PathBuf, &str, &str, &str)> = vec![
        // Claude — dedupe global + project on the same name.
        (claude_home().join("settings.json"), "claude", "claude", "global"),
        (project_root().join(".claude").join("settings.json"), "claude-project", "claude", "project"),
        // Codex
        (codex_home.join("config.toml"), "codex", "codex", "global"),
        (project_root().join(".codex").join("config.toml"), "codex-project", "codex", "project"),
        // OpenClaw
        (oc_home.join("openclaw.json"), "openclaw", "openclaw", "global"),
        // Hermes
        (home_dir().join(".hermes").join("config.yaml"), "hermes", "hermes", "global"),
    ];

    for (settings_path, _source, runtime_family, scope_label) in &config_paths {
        let Some(content) = read_file_lossy(settings_path) else { continue };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else { continue };
        for key in ["mcpServers", "mcp_servers"] {
            let Some(mcp_servers) = parsed.get(key).and_then(|v| v.as_object()) else { continue };
            for (name, config) in mcp_servers {
                let key_pair = (runtime_family.to_string(), name.clone());
                let command = config.get("command").and_then(|v| v.as_str()).map(|s| s.to_string());
                let url_val = config.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
                let transport = if url_val.is_some() { "http" } else { "stdio" };

                if let Some(existing) = seen.get_mut(&key_pair) {
                    // Already listed in another scope — append to the
                    // displayed name instead of creating a duplicate row.
                    if !existing.name.contains(scope_label) {
                        // Replace `(claude · global)` → `(claude · global, project)`.
                        let new_name = if let Some(close) = existing.name.rfind(')') {
                            format!("{}, {})", &existing.name[..close], scope_label)
                        } else {
                            format!("{} ({} · {})", existing.name, runtime_family, scope_label)
                        };
                        existing.name = new_name;
                    }
                } else {
                    seen.insert(
                        key_pair,
                        LocalMcpServer {
                            id: content_hash(&format!("{}-{}", runtime_family, name)),
                            name: format!("{} ({} · {})", name, runtime_family, scope_label),
                            transport: transport.to_string(),
                            status: "running".to_string(),
                            tool_count: 0,
                            command,
                            url: url_val,
                        },
                    );
                }
            }
        }
    }

    Ok(seen.into_values().collect())
}

#[tauri::command]
pub fn get_local_usage() -> Result<UsageSummary, String> {
    // Return zeros — real usage tracking would parse Claude's session logs
    Ok(UsageSummary {
        today: UsagePeriod { input_tokens: 0, output_tokens: 0, cost_cents: 0 },
        week: UsagePeriod { input_tokens: 0, output_tokens: 0, cost_cents: 0 },
        month: UsagePeriod { input_tokens: 0, output_tokens: 0, cost_cents: 0 },
    })
}

#[tauri::command]
pub fn get_daily_usage(_days: u32) -> Result<Vec<DailyUsage>, String> {
    Ok(Vec::new())
}

#[tauri::command]
pub fn get_burn_rate() -> Result<BurnRate, String> {
    Ok(BurnRate {
        tokens_per_hour: 0,
        cost_per_hour: 0.0,
        estimated_hours_to_limit: None,
        limit: Some(200000),
    })
}

#[tauri::command]
pub fn get_config_files() -> Result<Vec<ConfigFile>, String> {
    let home = home_dir();
    let claude = claude_home();
    let codex = PathBuf::from(std::env::var("CODEX_HOME")
        .unwrap_or_else(|_| home.join(".codex").to_string_lossy().to_string()));
    let openclaw = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home.join(".openclaw").to_string_lossy().to_string()));
    let hermes = home.join(".hermes");

    let files = vec![
        // Claude
        ("~/.claude/settings.json", claude.join("settings.json"), "Claude — Global settings"),
        ("~/.claude/settings.local.json", claude.join("settings.local.json"), "Claude — Local settings"),
        ("~/.claude/skills/", claude.join("skills"), "Claude — Personal skills"),
        (".claude/settings.json", PathBuf::from(".claude/settings.json"), "Claude — Project settings"),
        (".claude/skills/", PathBuf::from(".claude/skills"), "Claude — Project skills"),
        ("CLAUDE.md", project_root().join("CLAUDE.md"), "Claude — Project context"),
        // Codex
        ("~/.codex/config.toml", codex.join("config.toml"), "Codex — Global config"),
        ("~/.codex/AGENTS.md", codex.join("AGENTS.md"), "Codex — Global instructions"),
        ("~/.codex/skills/", codex.join("skills"), "Codex — Personal skills"),
        (".codex/config.toml", PathBuf::from(".codex/config.toml"), "Codex — Project config"),
        ("AGENTS.md", project_root().join("AGENTS.md"), "Codex — Project instructions"),
        // OpenClaw
        ("~/.openclaw/openclaw.json", openclaw.join("openclaw.json"), "OpenClaw — Config"),
        ("~/.openclaw/workspace/AGENTS.md", openclaw.join("workspace/AGENTS.md"), "OpenClaw — Agent instructions"),
        ("~/.openclaw/workspace/SOUL.md", openclaw.join("workspace/SOUL.md"), "OpenClaw — Persona"),
        ("~/.openclaw/workspace/TOOLS.md", openclaw.join("workspace/TOOLS.md"), "OpenClaw — Tools config"),
        ("~/.openclaw/skills/", openclaw.join("skills"), "OpenClaw — Skills"),
        // Hermes
        ("~/.hermes/config.yaml", hermes.join("config.yaml"), "Hermes — Config"),
        ("~/.hermes/SOUL.md", hermes.join("SOUL.md"), "Hermes — Persona"),
        ("~/.hermes/skills/", hermes.join("skills"), "Hermes — Skills"),
        ("~/.hermes/memories/MEMORY.md", hermes.join("memories/MEMORY.md"), "Hermes — Memory"),
    ];

    Ok(files.iter().map(|(display, path, scope)| {
        ConfigFile {
            path: display.to_string(),
            exists: path.exists(),
            scope: scope.to_string(),
        }
    }).collect())
}

#[tauri::command]
pub fn get_sync_status(db: State<'_, DbState>) -> Result<SyncStatus, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let enabled: bool = conn
        .query_row("SELECT value FROM settings WHERE key = 'sync_enabled'", [], |row| {
            let val: String = row.get(0)?;
            Ok(val == "true")
        })
        .unwrap_or(false);

    Ok(SyncStatus { enabled, last_sync_at: None, cloud_url: None })
}

#[tauri::command]
pub fn set_sync_enabled(db: State<'_, DbState>, enabled: bool, _cloud_url: Option<String>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('sync_enabled', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![if enabled { "true" } else { "false" }],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn restart_mcp_server(_name: String) -> Result<(), String> {
    // Placeholder — would need to actually restart the process
    Ok(())
}

/// Resolve the skill directory for a given runtime + scope
pub fn skill_dir_for_runtime(runtime: &str, scope: &str) -> PathBuf {
    match (runtime, scope) {
        // Claude
        ("claude", "enterprise") => PathBuf::from("/etc/claude/skills"),
        ("claude", "personal") => claude_home().join("skills"),
        ("claude", "project") => project_root().join(".claude/skills"),
        ("claude", "plugin") => claude_home().join("plugins"),
        // Codex
        ("codex", "personal") => {
            let home = std::env::var("CODEX_HOME")
                .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string());
            PathBuf::from(home).join("skills")
        }
        ("codex", "project") => project_root().join(".codex").join("skills"),
        // OpenClaw
        ("openclaw", "personal") => {
            let home = std::env::var("OPENCLAW_HOME")
                .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string());
            PathBuf::from(home).join("skills")
        }
        ("openclaw", "project") => {
            let home = std::env::var("OPENCLAW_HOME")
                .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string());
            PathBuf::from(home).join("workspace").join("skills")
        }
        // Hermes
        ("hermes", _) => home_dir().join(".hermes").join("skills"),
        // Fallback
        (_, "personal") => claude_home().join("skills"),
        (_, "project") => project_root().join(".claude").join("skills"),
        _ => claude_home().join("skills"),
    }
}

#[tauri::command]
pub fn create_skill(data: String) -> Result<SkillDetail, String> {
    let parsed: serde_json::Value = serde_json::from_str(&data)
        .map_err(|e| format!("Invalid skill data: {}", e))?;

    let name = parsed.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed").to_string();
    let description = parsed.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let scope = parsed.get("scope").and_then(|v| v.as_str()).unwrap_or("personal");
    let runtime = parsed.get("runtime").and_then(|v| v.as_str()).unwrap_or("claude");
    let content = parsed.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let is_directory = parsed.get("isDirectory").and_then(|v| v.as_bool()).unwrap_or(false);

    let skills_dir = skill_dir_for_runtime(runtime, scope);
    fs::create_dir_all(&skills_dir).map_err(|e| format!("Failed to create skills directory: {}", e))?;

    let (file_path, file_path_str) = if is_directory {
        let dir_path = skills_dir.join(&name);
        fs::create_dir_all(&dir_path).map_err(|e| format!("Failed to create skill directory: {}", e))?;
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

    let (frontmatter, _) = parse_frontmatter(&content);
    let hash = content_hash(&content);

    Ok(SkillDetail {
        id: content_hash(&file_path_str),
        name,
        description,
        file_path: file_path_str,
        scope: scope.to_string(),
        runtime: runtime.to_string(),
        token_count: estimate_tokens(content.len() as u64),
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
    let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
        .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));

    let dirs = vec![
        claude_home().join("skills"),
        project_root().join(".claude").join("skills"),
        codex_home.join("skills"),
        project_root().join(".codex").join("skills"),
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

                if content_hash(&file_path_str) == id {
                    if path.is_dir() {
                        fs::remove_dir_all(&path).map_err(|e| format!("Failed to delete skill directory: {}", e))?;
                    } else {
                        fs::remove_file(&path).map_err(|e| format!("Failed to delete skill file: {}", e))?;
                    }
                    return Ok(());
                }
            }
        }
    }

    Err(format!("Skill not found: {}", id))
}

#[tauri::command]
pub fn update_skill(db: State<'_, DbState>, id: String, content: String) -> Result<(), String> {
    // Scan ALL runtime directories to find the matching skill by ID
    let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
        .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));

    let dirs = vec![
        // Claude
        claude_home().join("skills"),
        project_root().join(".claude").join("skills"),
        PathBuf::from("/etc/claude/skills"),
        // Codex
        codex_home.join("skills"),
        project_root().join(".agents").join("skills"),
        project_root().join(".codex").join("skills"),
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

                if content_hash(&file_path_str) == id {
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
                            let _ = snapshot_skill_version(&db, &write_path.to_string_lossy(), &prior, None);
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

#[derive(Debug, serde::Serialize)]
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
    let hash = content_hash(content);
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO skill_versions (id, file_path, content, content_hash, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, write_path, content, hash, note, now],
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
            let _ = snapshot_skill_version(&db, &write_path, &current, Some("auto-snapshot before restore"));
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

// ── Chat threads (v1.5 — sustained sessions) ─────────────────────────────

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatThread {
    pub id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub created_at: String,
    pub last_message_at: Option<String>,
    pub message_count: i64,
    pub archived: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub runtime: Option<String>,
    pub agent_slug: Option<String>,
    pub metadata: Option<String>,
    pub created_at: String,
}

#[tauri::command]
pub fn list_chat_threads(
    db: State<'_, DbState>,
    project_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<ChatThread>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let cap = limit.unwrap_or(50).clamp(1, 500);
    // When project_id is set, restrict to that project; when None, return
    // all (global + project-scoped). NULL match in SQL is a pain — split.
    let (sql, params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = match project_id {
        Some(p) => (
            "SELECT id, title, project_id, agent_id, created_at, last_message_at, message_count, archived
             FROM chat_threads
             WHERE project_id = ?1 AND archived = 0
             ORDER BY COALESCE(last_message_at, created_at) DESC
             LIMIT ?2",
            vec![Box::new(p), Box::new(cap)],
        ),
        None => (
            "SELECT id, title, project_id, agent_id, created_at, last_message_at, message_count, archived
             FROM chat_threads
             WHERE archived = 0
             ORDER BY COALESCE(last_message_at, created_at) DESC
             LIMIT ?1",
            vec![Box::new(cap)],
        ),
    };
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| &**b).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(param_refs.iter()), |row| {
            Ok(ChatThread {
                id: row.get(0)?,
                title: row.get(1)?,
                project_id: row.get(2)?,
                agent_id: row.get(3)?,
                created_at: row.get(4)?,
                last_message_at: row.get(5)?,
                message_count: row.get(6)?,
                archived: row.get::<_, i32>(7)? != 0,
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
pub fn create_chat_thread(
    db: State<'_, DbState>,
    title: String,
    project_id: Option<String>,
    agent_id: Option<String>,
) -> Result<ChatThread, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let trimmed = if title.trim().is_empty() {
        "New conversation".to_string()
    } else {
        title.trim().chars().take(120).collect()
    };
    conn.execute(
        "INSERT INTO chat_threads (id, title, project_id, agent_id, created_at, last_message_at, message_count, archived)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, 0, 0)",
        params![id, trimmed, project_id, agent_id, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(ChatThread {
        id,
        title: trimmed,
        project_id,
        agent_id,
        created_at: now,
        last_message_at: None,
        message_count: 0,
        archived: false,
    })
}

#[tauri::command]
pub fn rename_chat_thread(
    db: State<'_, DbState>,
    id: String,
    title: String,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let trimmed: String = title.trim().chars().take(120).collect();
    if trimmed.is_empty() {
        return Err("title-empty".into());
    }
    conn.execute(
        "UPDATE chat_threads SET title = ?1 WHERE id = ?2",
        params![trimmed, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_chat_thread(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // ON DELETE CASCADE on chat_messages handles the rows, but the FK is
    // only honored when foreign_keys = ON. Defense in depth: delete both.
    conn.execute("DELETE FROM chat_messages WHERE thread_id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM chat_threads WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_chat_thread_agent(
    db: State<'_, DbState>,
    id: String,
    agent_id: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE chat_threads SET agent_id = ?1 WHERE id = ?2",
        params![agent_id, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_chat_messages(
    db: State<'_, DbState>,
    thread_id: String,
) -> Result<Vec<ChatMessage>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, thread_id, role, content, runtime, agent_slug, metadata, created_at
             FROM chat_messages
             WHERE thread_id = ?1
             ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([&thread_id], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                runtime: row.get(4)?,
                agent_slug: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
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
pub fn append_chat_message(
    db: State<'_, DbState>,
    thread_id: String,
    role: String,
    content: String,
    runtime: Option<String>,
    agent_slug: Option<String>,
    metadata: Option<String>,
) -> Result<ChatMessage, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO chat_messages (id, thread_id, role, content, runtime, agent_slug, metadata, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, thread_id, role, content, runtime, agent_slug, metadata, now],
    )
    .map_err(|e| e.to_string())?;
    // Update thread aggregate fields.
    conn.execute(
        "UPDATE chat_threads
            SET last_message_at = ?1,
                message_count = message_count + 1
          WHERE id = ?2",
        params![now, thread_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(ChatMessage {
        id,
        thread_id,
        role,
        content,
        runtime,
        agent_slug,
        metadata,
        created_at: now,
    })
}

#[tauri::command]
pub fn delete_chat_message(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let thread_id: Option<String> = conn
        .query_row(
            "SELECT thread_id FROM chat_messages WHERE id = ?1",
            [&id],
            |r| r.get::<_, String>(0),
        )
        .ok();
    conn.execute("DELETE FROM chat_messages WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    if let Some(tid) = thread_id {
        // Recompute message_count rather than risk drift.
        conn.execute(
            "UPDATE chat_threads
                SET message_count = (SELECT COUNT(*) FROM chat_messages WHERE thread_id = ?1)
              WHERE id = ?1",
            [&tid],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn prompt_claude(prompt: String) -> Result<String, String> {
    use std::process::Command;

    // Find the claude CLI
    let claude_path = which_claude().ok_or_else(|| {
        "Claude Code CLI not found. Install it with: npm install -g @anthropic-ai/claude-code".to_string()
    })?;

    // Run claude with --print flag (non-interactive, uses subscription)
    // Use the user's full PATH so claude can find node, npm, etc.
    let user_path = get_user_path();
    let output = Command::new(&claude_path)
        .args(["--print", &prompt])
        .env("PATH", &user_path)
        .output()
        .map_err(|e| format!("Failed to run claude: {}", e))?;

    if output.status.success() {
        let response = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(response)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if stderr.contains("not logged in") || stderr.contains("authentication") {
            Err("Not logged in to Claude Code. Run `claude` in your terminal first to authenticate.".to_string())
        } else if stderr.is_empty() {
            // Sometimes claude outputs to stdout even on non-zero exit
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            if !stdout.is_empty() {
                Ok(stdout)
            } else {
                Err("Claude returned no output. Make sure Claude Code is installed and you're logged in.".to_string())
            }
        } else {
            Err(format!("Claude error: {}", stderr.lines().last().unwrap_or(&stderr)))
        }
    }
}

// ── Workflow Persistence ──────────────────────────────────────────────────

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

// ── Multi-Agent Runtime ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DetectedRuntime {
    pub runtime: String,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

/// Get the user's full shell PATH (Tauri apps launch with minimal env)
use std::sync::OnceLock;

/// Cached PATH resolution. Resolving the user's PATH spawns a shell on
/// Unix and PowerShell on Windows — neither is cheap, and on Windows the
/// PowerShell call pops a visible console window per invocation. v1.5.21
/// shipped without this cache and called get_user_path() once per MCP
/// discovery, which on Felipe's Windows install meant a stream of
/// flashing PowerShell windows. Caching the value at first call (the
/// shell's PATH doesn't change during app lifetime anyway) cuts both
/// the cost and the visual noise.
static USER_PATH_CACHE: OnceLock<String> = OnceLock::new();

#[cfg(target_os = "windows")]
fn no_window_flag() -> u32 {
    // CREATE_NO_WINDOW — keeps the PowerShell child invisible to the user.
    // Without this, every spawn pops a black PowerShell window briefly.
    0x08000000
}

pub fn get_user_path() -> String {
    USER_PATH_CACHE.get_or_init(resolve_user_path).clone()
}

fn resolve_user_path() -> String {
    // Windows takes a different code path: GUI-launched apps inherit the
    // PATH from when they were launched, which usually misses User-scope
    // PATH entries the user added later (npm-global, scoop shims, etc.).
    // Resolve via PowerShell which reads both Machine + User env at runtime.
    // Felipe hit this on v1.5.20: nothing connects on Windows because no
    // CLI was findable, even though `where claude` works in his terminal.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        if let Ok(output) = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "[Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User')",
            ])
            .creation_flags(no_window_flag())
            .output()
        {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return path;
                }
            }
        }
        // Fall through to the inherited PATH. Better than nothing.
        return std::env::var("PATH").unwrap_or_default();
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Try to get PATH from user's shell. The shell flag set is critical
        // for nvm-installed node: nvm.sh is sourced from ~/.bashrc and
        // ~/.zshrc (interactive init), NOT from ~/.bash_profile / ~/.profile
        // (login init). v1.5.21 only used `-l` (login) so Felipe's nvm node
        // never made it onto PATH and `npx` stayed unfound. Using `-l -i`
        // (login + interactive) sources both, which is what the user's
        // terminal does on every fresh tab.
        for shell in ["/bin/zsh", "/bin/bash"] {
            if let Ok(output) = std::process::Command::new(shell)
                .args(["-l", "-i", "-c", "echo $PATH"])
                .output()
            {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return path;
                    }
                }
            }
            // Fallback to login-only in case `-i` triggered a prompt that
            // blocked output (rare but possible with custom rc).
            if let Ok(output) = std::process::Command::new(shell)
                .args(["-l", "-c", "echo $PATH"])
                .output()
            {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return path;
                    }
                }
            }
        }
        std::env::var("PATH").unwrap_or_default()
    }
}

/// Build a `std::process::Command` from a CLI string that may be either
/// a plain path or a wrapper invocation. This lets users on Windows run
/// `wsl.exe -e /home/<user>/.local/bin/claude` as the override path —
/// the WSL → Linux Claude case Felipe hit. Quoting is naive (whitespace
/// split) but covers the common cases without pulling in a full shell
/// parser.
pub fn wrapper_command(spec: &str) -> std::process::Command {
    let trimmed = spec.trim();
    let mut parts = trimmed.split_whitespace();
    let exe = parts.next().unwrap_or(trimmed);
    let mut cmd = std::process::Command::new(exe);
    for arg in parts {
        cmd.arg(arg);
    }
    cmd
}

/// Async tokio counterpart for streaming dispatch paths.
pub fn wrapper_command_tokio(spec: &str) -> tokio::process::Command {
    let trimmed = spec.trim();
    let mut parts = trimmed.split_whitespace();
    let exe = parts.next().unwrap_or(trimmed);
    let mut cmd = tokio::process::Command::new(exe);
    for arg in parts {
        cmd.arg(arg);
    }
    cmd
}

/// Search for a CLI binary by name, checking common install paths + user shell + npx cache.
pub fn which_cli(name: &str) -> Option<String> {
    // HOME isn't set on Windows by default — USERPROFILE is. Falling back
    // to USERPROFILE keeps the candidate-path expansion working
    // cross-platform without forcing every caller to set HOME first.
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();

    // 1. Check user-configured override first (highest priority).
    //    The override may be a plain path OR a wrapper invocation
    //    (e.g. `wsl.exe -e /home/user/.local/bin/claude`). When it has
    //    a space, we only check the first token for existence — the
    //    rest are arguments. The override is returned verbatim so
    //    downstream callers can run it via `wrapper_command(...)`.
    let override_path = home_dir().join(".ato").join(format!("{}-path", name));
    if let Some(custom) = read_file_lossy(&override_path) {
        let trimmed = custom.trim().to_string();
        if !trimmed.is_empty() {
            let first_token = trimmed
                .split_whitespace()
                .next()
                .unwrap_or(&trimmed)
                .to_string();
            if std::path::Path::new(&first_token).exists() {
                return Some(trimmed);
            }
            // Allow command names that resolve through PATH (e.g.
            // `wsl.exe` on Windows is on PATH but not at a fixed
            // location). Try `which`/`where` resolution.
            if which_executable(&first_token).is_some() {
                return Some(trimmed);
            }
        }
    }

    // 2. Check common install locations.
    let mut candidates: Vec<String> = vec![
        format!("/usr/local/bin/{}", name),
        format!("/opt/homebrew/bin/{}", name),
        format!("{}/.npm-global/bin/{}", home, name),
        format!("{}/bin/{}", home, name),
        format!("{}/.local/bin/{}", home, name),
        format!("{}/.cargo/bin/{}", home, name),
    ];
    // Windows-specific candidates. npm shims land in %APPDATA%\npm\<name>.cmd
    // — `where` doesn't always pick these up if Tauri's GUI-launched PATH
    // misses %APPDATA%. Volta, scoop, and Cargo for Windows go elsewhere
    // again. Felipe's "nothing connects on Windows" was this set never
    // being checked.
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        let local_appdata = std::env::var("LOCALAPPDATA").unwrap_or_default();
        // Each candidate gets tried both as `<name>.cmd` and `<name>.exe`
        // — npm publishes .cmd shims, native installers ship .exe.
        for ext in ["cmd", "exe"] {
            if !appdata.is_empty() {
                candidates.push(format!(r"{}\npm\{}.{}", appdata, name, ext));
            }
            if !local_appdata.is_empty() {
                candidates.push(format!(r"{}\Programs\{}\{}.{}", local_appdata, name, name, ext));
                candidates.push(format!(r"{}\Volta\bin\{}.{}", local_appdata, name, ext));
            }
            if !home.is_empty() {
                candidates.push(format!(r"{}\.cargo\bin\{}.{}", home, name, ext));
                candidates.push(format!(r"{}\scoop\shims\{}.{}", home, name, ext));
            }
        }
    }

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.clone());
        }
    }

    // 3. Search npx cache directories (where `npx @anthropic-ai/claude-code` installs)
    let npx_cache = PathBuf::from(&home).join(".npm/_npx");
    if npx_cache.exists() {
        if let Ok(entries) = fs::read_dir(&npx_cache) {
            for entry in entries.flatten() {
                let bin_path = entry.path().join("node_modules").join(".bin").join(name);
                if bin_path.exists() {
                    return Some(bin_path.to_string_lossy().to_string());
                }
            }
        }
    }

    // 4. Fall through to platform-specific `which`/`where` resolution.
    which_executable(name)
}

/// Resolve a bare executable name through the user's shell PATH using
/// the platform-native lookup tool. Returns the absolute path on
/// success. Used both in `which_cli`'s fallback and to validate the
/// first token of a wrapper override.
fn which_executable(name: &str) -> Option<String> {
    let user_path = get_user_path();
    let lookup_cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
    if let Ok(output) = std::process::Command::new(lookup_cmd)
        .arg(name)
        .env("PATH", &user_path)
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // `where` on Windows can return multiple lines — take the first.
            let path = stdout.lines().next().unwrap_or("").trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    None
}

/// Save a custom CLI path for a runtime (used when auto-detect fails).
#[tauri::command]
pub fn set_runtime_path(runtime: String, path: String) -> Result<(), String> {
    let ato_dir = home_dir().join(".ato");
    fs::create_dir_all(&ato_dir).ok();
    let file_path = ato_dir.join(format!("{}-path", runtime));
    fs::write(&file_path, path.trim()).map_err(|e| format!("Failed to save runtime path: {}", e))
}

/// Get a saved custom CLI path for a runtime.
#[tauri::command]
pub fn get_runtime_path(runtime: String) -> Result<Option<String>, String> {
    let file_path = home_dir().join(".ato").join(format!("{}-path", runtime));
    Ok(read_file_lossy(&file_path).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
}

#[tauri::command]
pub fn detect_agent_runtimes() -> Result<Vec<DetectedRuntime>, String> {
    let runtimes = vec![
        ("claude", which_claude().or_else(|| which_cli("claude"))),
        ("codex", which_cli("codex")),
        ("openclaw", which_cli("openclaw")),
        ("hermes", which_cli("hermes")),
    ];

    Ok(runtimes
        .into_iter()
        .map(|(name, path)| {
            let available = path.is_some();
            DetectedRuntime {
                runtime: name.to_string(),
                available,
                version: if available { Some("CLI".to_string()) } else { None },
                path,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn prompt_agent(runtime: String, prompt: String, config: Option<String>) -> Result<String, String> {
    use std::process::Command;

    // Use the user's full shell PATH so CLIs can find node, npm, etc.
    let user_path = get_user_path();

    // F5 — extract model override from config, applied as `--model X` per
    // runtime. None → runtime default.
    let cfg_json: Option<serde_json::Value> = config
        .as_deref()
        .and_then(|c| serde_json::from_str(c).ok());
    let model_override: Option<String> = cfg_json
        .as_ref()
        .and_then(|c| c.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    match runtime.as_str() {
        "claude" => {
            let claude_path = which_claude().ok_or_else(|| {
                "Claude Code CLI not found".to_string()
            })?;
            let mut args: Vec<String> = vec!["--print".into(), prompt.clone()];
            if let Some(m) = &model_override {
                args.push("--model".into());
                args.push(m.clone());
            }
            let output = Command::new(&claude_path)
                .args(&args)
                .env("PATH", &user_path)
                .output()
                .map_err(|e| format!("Failed to run claude: {}", e))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(format!("Claude error: {}", String::from_utf8_lossy(&output.stderr)))
            }
        }
        "codex" => {
            let codex_path = which_cli("codex").ok_or_else(|| {
                "Codex CLI not found. Install it with: npm install -g @openai/codex".to_string()
            })?;
            // Codex's CLI shape is `codex [OPTIONS] [PROMPT]` with `exec` as
            // the non-interactive subcommand. It does NOT accept `--print`.
            // Always pass `--skip-git-repo-check` because ATO can dispatch
            // from any cwd (including non-repo dirs); without it Codex bails
            // with "Not inside a trusted directory" — Felipe's bug.
            let mut args: Vec<String> = vec!["exec".into(), "--skip-git-repo-check".into()];
            if let Some(m) = &model_override {
                args.push("--model".into());
                args.push(m.clone());
            }
            args.push(prompt.clone());
            let output = Command::new(&codex_path)
                .args(&args)
                .env("PATH", &user_path)
                .output()
                .map_err(|e| format!("Failed to run codex: {}", e))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(format!("Codex error: {}", String::from_utf8_lossy(&output.stderr)))
            }
        }
        "openclaw" => {
            let ssh_config: serde_json::Value = config
                .as_deref()
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or_default();
            let host = ssh_config.get("sshHost").and_then(|v| v.as_str()).unwrap_or("localhost");
            let port = ssh_config.get("sshPort").and_then(|v| v.as_u64()).unwrap_or(22);
            let user = ssh_config.get("sshUser").and_then(|v| v.as_str()).unwrap_or("root");
            let key_path = ssh_config.get("sshKeyPath").and_then(|v| v.as_str());

            let mut cmd = Command::new("ssh");
            cmd.env("PATH", &user_path);
            if let Some(key) = key_path {
                cmd.args(["-i", key]);
            }
            cmd.args([
                "-p", &port.to_string(),
                &format!("{}@{}", user, host),
                &format!("openclaw exec '{}'", prompt.replace('\'', "'\\''"))
            ]);

            let output = cmd.output()
                .map_err(|e| format!("Failed to SSH to OpenClaw host: {}", e))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(format!("OpenClaw error: {}", String::from_utf8_lossy(&output.stderr)))
            }
        }
        "hermes" => {
            let hermes_path = which_cli("hermes").ok_or_else(|| {
                "Hermes CLI not found".to_string()
            })?;
            let output = Command::new(&hermes_path)
                .args(["--execute", &prompt])
                .env("PATH", &user_path)
                .output()
                .map_err(|e| format!("Failed to run hermes: {}", e))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(format!("Hermes error: {}", String::from_utf8_lossy(&output.stderr)))
            }
        }
        "gemini" => {
            let gemini_path = which_cli("gemini").ok_or_else(|| {
                "Gemini CLI not found. Install: npm install -g @google/gemini-cli".to_string()
            })?;
            // gemini CLI: `gemini -p "<prompt>" [-m <model>]`
            let mut args: Vec<String> = vec!["-p".into(), prompt.clone()];
            if let Some(m) = &model_override {
                args.push("-m".into());
                args.push(m.clone());
            }
            let output = Command::new(&gemini_path)
                .args(&args)
                .env("PATH", &user_path)
                .output()
                .map_err(|e| format!("Failed to run gemini: {}", e))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(format!("Gemini error: {}", String::from_utf8_lossy(&output.stderr)))
            }
        }
        _ => Err(format!("Unknown runtime: {}", runtime)),
    }
}

// ── Cron Job Persistence ─────────────────────────────────────────────────

pub fn cron_jobs_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("cron-jobs.json");
    path
}

pub fn cron_history_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("cron-history.json");
    path
}

#[tauri::command]
pub fn list_cron_jobs() -> Result<Vec<serde_json::Value>, String> {
    let mut all_jobs: Vec<serde_json::Value> = Vec::new();

    // 1. ATO-created cron jobs
    let path = cron_jobs_path();
    if path.exists() {
        if let Some(content) = read_file_lossy(&path) {
            if let Ok(jobs) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                all_jobs.extend(jobs);
            }
        }
    }

    // 2. Claude Code native scheduled tasks (from ~/.claude/claudecron/tasks.db)
    let claude_cron_db = claude_home().join("claudecron").join("tasks.db");
    if claude_cron_db.exists() {
        if let Ok(conn) = rusqlite::Connection::open_with_flags(
            &claude_cron_db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) {
            // Try to read tasks from Claude's schema
            let query_result = conn.prepare(
                "SELECT id, name, schedule, prompt, enabled, created_at, last_run_at FROM tasks"
            );
            if let Ok(mut stmt) = query_result {
                let tasks = stmt.query_map([], |row| {
                    let id: String = row.get(0)?;
                    let name: String = row.get(1)?;
                    let schedule: String = row.get(2)?;
                    let prompt: String = row.get(3)?;
                    let enabled: bool = row.get(4)?;
                    let created_at: String = row.get(5)?;
                    let last_run_at: Option<String> = row.get(6)?;

                    Ok(serde_json::json!({
                        "id": format!("claude-native-{}", id),
                        "name": name,
                        "description": format!("Claude Code scheduled task"),
                        "schedule": schedule,
                        "runtime": "claude",
                        "prompt": prompt,
                        "enabled": enabled,
                        "status": if enabled { "healthy" } else { "paused" },
                        "source": "claude-code",
                        "createdAt": created_at,
                        "updatedAt": created_at,
                        "lastRunAt": last_run_at,
                    }))
                });

                if let Ok(rows) = tasks {
                    for task in rows.flatten() {
                        all_jobs.push(task);
                    }
                }
            }
        }
    }

    // 3. Claude Desktop Cowork scheduled tasks
    // macOS: ~/Library/Application Support/Claude/
    let claude_desktop_dir = home_dir()
        .join("Library")
        .join("Application Support")
        .join("Claude");
    if claude_desktop_dir.exists() {
        // Look for any task/schedule databases
        for db_name in ["tasks.db", "scheduled_tasks.db", "cowork.db"] {
            let db_path = claude_desktop_dir.join(db_name);
            if db_path.exists() {
                if let Ok(conn) = rusqlite::Connection::open_with_flags(
                    &db_path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                ) {
                    // Try common table names
                    for table in ["tasks", "scheduled_tasks", "dispatches"] {
                        let query = format!("SELECT * FROM {} LIMIT 50", table);
                        if let Ok(stmt) = conn.prepare(&query) {
                            let col_names: Vec<String> = (0..stmt.column_count())
                                .map(|i| stmt.column_name(i).unwrap_or("unknown").to_string())
                                .collect();
                            drop(stmt);

                            if let Ok(mut stmt2) = conn.prepare(&query) {
                                if let Ok(rows) = stmt2.query_map([], |row| {
                                    let mut obj = serde_json::Map::new();
                                    for (i, col_name) in col_names.iter().enumerate() {
                                        let val: String = row.get::<_, String>(i).unwrap_or_default();
                                        obj.insert(col_name.clone(), serde_json::Value::String(val));
                                    }
                                    obj.insert("source".to_string(), serde_json::Value::String("claude-desktop".to_string()));
                                    obj.insert("runtime".to_string(), serde_json::Value::String("claude".to_string()));
                                    Ok(serde_json::Value::Object(obj))
                                }) {
                                    for task in rows.flatten() {
                                        all_jobs.push(task);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(all_jobs)
}

#[tauri::command]
pub fn save_cron_job(job: String) -> Result<(), String> {
    let parsed: serde_json::Value = serde_json::from_str(&job)
        .map_err(|e| format!("Invalid cron job JSON: {}", e))?;
    let id = parsed.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Cron job must have an id".to_string())?;

    let path = cron_jobs_path();
    let mut jobs: Vec<serde_json::Value> = if path.exists() {
        let content = read_file_lossy(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Update or insert
    if let Some(idx) = jobs.iter().position(|j| j.get("id").and_then(|v| v.as_str()) == Some(id)) {
        jobs[idx] = parsed;
    } else {
        jobs.push(parsed);
    }

    let serialized = serde_json::to_string_pretty(&jobs)
        .map_err(|e| format!("Failed to serialize cron jobs: {}", e))?;
    fs::write(&path, serialized).map_err(|e| format!("Failed to write cron jobs: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn delete_cron_job(id: String) -> Result<(), String> {
    let path = cron_jobs_path();
    if !path.exists() {
        return Ok(());
    }

    let content = read_file_lossy(&path).unwrap_or_default();
    let mut jobs: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
    jobs.retain(|j| j.get("id").and_then(|v| v.as_str()) != Some(&id));

    let serialized = serde_json::to_string_pretty(&jobs)
        .map_err(|e| format!("Failed to serialize cron jobs: {}", e))?;
    fs::write(&path, serialized).map_err(|e| format!("Failed to write cron jobs: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn get_cron_history(job_id: String) -> Result<Vec<serde_json::Value>, String> {
    let path = cron_history_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = read_file_lossy(&path).unwrap_or_default();
    let all: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
    Ok(all.into_iter()
        .filter(|e| e.get("jobId").and_then(|v| v.as_str()) == Some(&job_id))
        .collect())
}

#[tauri::command]
pub async fn trigger_cron_job(
    db: State<'_, DbState>,
    id: String,
) -> Result<String, String> {
    // Read the job from disk.
    let path = cron_jobs_path();
    if !path.exists() {
        return Err("No cron jobs configured".to_string());
    }
    let content = read_file_lossy(&path).unwrap_or_default();
    let jobs: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
    let job = jobs.iter()
        .find(|j| j.get("id").and_then(|v| v.as_str()) == Some(&id))
        .ok_or_else(|| format!("Cron job not found: {}", id))?;

    let runtime = job.get("runtime").and_then(|v| v.as_str()).unwrap_or("claude").to_string();
    let prompt = job.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let config = job.get("runtimeConfig").map(|v| v.to_string());
    let agent_slug = job.get("agentSlug").and_then(|v| v.as_str()).map(|s| s.to_string());
    let group_slug = job.get("groupSlug").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Preferred dispatch order: group → agent → raw runtime+prompt.
    if let Some(slug) = group_slug {
        // Sequential pipelines & routed groups both go through dispatch_to_group;
        // it returns a stitched transcript suitable as a single string result.
        let result = dispatch_to_group(db, slug, prompt, config, None).await?;
        return Ok(result.response);
    }

    if let Some(slug) = agent_slug {
        // Look up the agent by slug → run via prompt_agent_with_context so
        // variables / hooks / role-models / memory policy all fire.
        let agent_id_runtime: Option<(String, String)> = {
            let conn = db.0.lock().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT id, runtime FROM agents WHERE slug = ?1",
                rusqlite::params![slug],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .ok()
        };
        match agent_id_runtime {
            Some((agent_id, agent_runtime)) => {
                // v2.1.0+ — prompt_agent_with_context now returns
                // DispatchResult{response, run_id}. Internal Rust callers
                // (cron, group dispatch) only need the response. The
                // run_id is consumed by frontend wrappers; here we just
                // unwrap and discard it.
                return prompt_agent_with_context(
                    db,
                    agent_id,
                    agent_runtime,
                    prompt,
                    config,
                    None,
                )
                .await
                .map(|r| r.response);
            }
            None => {
                return Err(format!(
                    "Cron job references agent '{}' which doesn't exist anymore",
                    slug
                ));
            }
        }
    }

    // Fallback: raw dispatch (legacy / advanced).
    prompt_agent(runtime, prompt, config).await
}

// ── Agent Status & Logging ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub runtime: String,
    pub available: bool,
    pub healthy: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub details: serde_json::Value,
}

#[tauri::command]
pub async fn query_agent_status(runtime: String, config: Option<String>) -> Result<AgentStatus, String> {
    use std::process::Command;

    match runtime.as_str() {
        "claude" => {
            let path = which_claude();
            let available = path.is_some();
            let mut version = None;
            let mut healthy = false;

            if let Some(ref cli) = path {
                // Get version
                if let Ok(output) = wrapper_command(cli).arg("--version").output() {
                    if output.status.success() {
                        version = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
                // Auth check — run a minimal prompt
                if let Ok(output) = wrapper_command(cli).args(["--print", "respond with OK"]).output() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    healthy = output.status.success() && !stderr.contains("not logged in");
                }
            }

            Ok(AgentStatus {
                runtime: "claude".into(),
                available,
                healthy,
                version,
                path,
                details: serde_json::json!({ "authenticated": healthy }),
            })
        }
        "codex" => {
            let path = which_cli("codex");
            let available = path.is_some();
            let mut version = None;
            let mut healthy = false;

            if let Some(ref cli) = path {
                if let Ok(output) = wrapper_command(cli).arg("--version").output() {
                    if output.status.success() {
                        version = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
                if let Ok(output) = wrapper_command(cli).arg("--help").output() {
                    healthy = output.status.success();
                }
            }

            let api_key_set = std::env::var("OPENAI_API_KEY").is_ok();

            Ok(AgentStatus {
                runtime: "codex".into(),
                available,
                healthy,
                version,
                path,
                details: serde_json::json!({ "apiKeyEnv": if api_key_set { "set" } else { "not set" } }),
            })
        }
        "openclaw" => {
            let ssh_config: serde_json::Value = config
                .as_deref()
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or_default();
            let host = ssh_config.get("sshHost").and_then(|v| v.as_str()).unwrap_or("");
            let port = ssh_config.get("sshPort").and_then(|v| v.as_u64()).unwrap_or(22);
            let user = ssh_config.get("sshUser").and_then(|v| v.as_str()).unwrap_or("root");
            let key_path = ssh_config.get("sshKeyPath").and_then(|v| v.as_str());

            if host.is_empty() {
                return Ok(AgentStatus {
                    runtime: "openclaw".into(),
                    available: false,
                    healthy: false,
                    version: None,
                    path: None,
                    details: serde_json::json!({ "error": "No SSH host configured" }),
                });
            }

            let mut cmd = Command::new("ssh");
            if let Some(key) = key_path {
                cmd.args(["-i", key]);
            }
            cmd.args([
                "-p", &port.to_string(),
                "-o", "ConnectTimeout=5",
                "-o", "StrictHostKeyChecking=no",
                "-o", "BatchMode=yes",
                &format!("{}@{}", user, host),
                "openclaw --version 2>/dev/null || echo NOT_FOUND"
            ]);

            let (available, version, healthy) = match cmd.output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let avail = output.status.success() && !stdout.contains("NOT_FOUND");
                    let ver = if avail { Some(stdout.lines().next().unwrap_or("").to_string()) } else { None };
                    (avail, ver, output.status.success())
                }
                Err(_) => (false, None, false),
            };

            Ok(AgentStatus {
                runtime: "openclaw".into(),
                available,
                healthy,
                version,
                path: Some(format!("{}@{}:{}", user, host, port)),
                details: serde_json::json!({
                    "sshHost": host,
                    "sshPort": port,
                    "sshUser": user,
                    "sshReachable": healthy,
                }),
            })
        }
        "hermes" => {
            let path = which_cli("hermes");
            let available = path.is_some();
            let mut version = None;
            let mut healthy = false;

            if let Some(ref cli) = path {
                if let Ok(output) = wrapper_command(cli).arg("--version").output() {
                    if output.status.success() {
                        version = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
                if let Ok(output) = wrapper_command(cli).arg("--help").output() {
                    healthy = output.status.success();
                }
            }

            // Check endpoint if configured
            let endpoint = config.as_deref()
                .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
                .and_then(|v| v.get("endpoint").and_then(|e| e.as_str().map(|s| s.to_string())));

            Ok(AgentStatus {
                runtime: "hermes".into(),
                available,
                healthy,
                version,
                path,
                details: serde_json::json!({
                    "cliAvailable": available,
                    "endpoint": endpoint,
                }),
            })
        }
        _ => Err(format!("Unknown runtime: {}", runtime)),
    }
}

#[tauri::command]
pub fn query_all_agent_statuses() -> Result<Vec<AgentStatus>, String> {
    // Check OpenClaw via saved config
    let oc_available = load_openclaw_ssh_config().is_ok();

    let runtimes = vec![
        ("claude", which_claude()),
        ("codex", which_cli("codex")),
        ("openclaw", if oc_available { Some("ssh".to_string()) } else { None }),
        ("hermes", which_cli("hermes")),
    ];

    Ok(runtimes.into_iter().map(|(name, path)| {
        let available = path.is_some();
        AgentStatus {
            runtime: name.to_string(),
            available,
            healthy: available, // assume healthy if available for fast check
            version: None,
            path,
            details: serde_json::json!({}),
        }
    }).collect())
}

pub fn agent_logs_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("agent-logs.jsonl");
    path
}

#[tauri::command]
pub fn append_agent_log(entry: String) -> Result<(), String> {
    use std::io::Write;
    let path = agent_logs_path();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Failed to open agent log: {}", e))?;
    writeln!(file, "{}", entry).map_err(|e| format!("Failed to write agent log: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn get_agent_logs(runtime: Option<String>, limit: Option<u32>) -> Result<Vec<serde_json::Value>, String> {
    let path = agent_logs_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = read_file_lossy(&path).unwrap_or_default();
    let limit = limit.unwrap_or(50) as usize;

    let mut logs: Vec<serde_json::Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|entry| {
            if let Some(ref rt) = runtime {
                entry.get("runtime").and_then(|v| v.as_str()) == Some(rt.as_str())
            } else {
                true
            }
        })
        .collect();

    // Return last N entries
    if logs.len() > limit {
        logs = logs.split_off(logs.len() - limit);
    }

    Ok(logs)
}

pub fn which_claude() -> Option<String> {
    // which_cli now handles all the search logic including npx cache
    // and user shell PATH. No need for a separate function.
    which_cli("claude")
}

// ── OpenClaw WebSocket + Runtime Config ───────────────────────────────────

/// Load OpenClaw SSH config from ~/.ato/openclaw-config.json
pub fn load_openclaw_ssh_config() -> Result<(String, u64, String, Option<String>), String> {
    let config_path = home_dir().join(".ato").join("openclaw-config.json");
    let content = read_file_lossy(&config_path)
        .ok_or("OpenClaw not configured. Go to Configuration to set SSH host.")?;
    let config: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let host = config.get("sshHost").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if host.is_empty() { return Err("No SSH host configured".into()); }
    let port = config.get("sshPort").and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64())).unwrap_or(22);
    let user = config.get("sshUser").and_then(|v| v.as_str()).unwrap_or("root").to_string();
    let key_path = config.get("sshKeyPath").and_then(|v| v.as_str()).map(|s| s.to_string()).filter(|s| !s.is_empty());
    Ok((host, port, user, key_path))
}

/// Build the base SSH command for OpenClaw
pub fn openclaw_ssh_base() -> Result<(std::process::Command, String, u64, String), String> {
    let (host, port, user, key_path) = load_openclaw_ssh_config()?;
    let user_path = get_user_path();
    let mut cmd = std::process::Command::new("ssh");
    cmd.env("PATH", &user_path);
    cmd.args(["-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=accept-new"]);
    if let Some(ref key) = key_path {
        cmd.args(["-i", key]);
    }
    cmd.args(["-p", &port.to_string(), &format!("{}@{}", user, host)]);
    Ok((cmd, host, port, user))
}

/// Run an openclaw CLI command via SSH and return the JSON output
pub fn openclaw_ssh_command(subcmd: &str) -> Result<serde_json::Value, String> {
    let (mut cmd, ..) = openclaw_ssh_base()?;
    cmd.arg(format!("openclaw {} 2>/dev/null", subcmd));
    let output = cmd.output().map_err(|e| format!("SSH failed: {}", e))?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(stdout.trim()).map_err(|e| format!("Invalid JSON from openclaw: {}", e))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("OpenClaw command failed: {}", stderr.trim()))
    }
}

/// Run a raw shell command via SSH and return plain text output
pub fn openclaw_ssh_raw(shell_cmd: &str) -> Result<String, String> {
    let (mut cmd, ..) = openclaw_ssh_base()?;
    cmd.arg(shell_cmd);
    let output = cmd.output().map_err(|e| format!("SSH failed: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("SSH command failed: {}", stderr.trim()))
    }
}

#[tauri::command]
pub async fn openclaw_gateway_status() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("status --json")
}

#[tauri::command]
pub async fn openclaw_list_cron_jobs() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("cron list --all --json")
}

#[tauri::command]
pub async fn openclaw_cron_status() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("cron status --json")
}

#[tauri::command]
pub async fn openclaw_list_agents() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("agents list --json")
}

#[tauri::command]
pub async fn openclaw_skills_status() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("skills status --json")
}

#[tauri::command]
pub async fn openclaw_list_sessions() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("sessions list --json")
}

#[tauri::command]
pub async fn openclaw_test_connection(ws_url: String, token: String) -> Result<serde_json::Value, String> {
    // Test via SSH instead of WebSocket since the gateway requires crypto auth
    let _ = (ws_url, token); // Unused - we use SSH config instead
    let (host, port, user, key_path) = load_openclaw_ssh_config()?;
    let user_path = get_user_path();
    let mut cmd = std::process::Command::new("ssh");
    cmd.env("PATH", &user_path);
    cmd.args(["-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=accept-new"]);
    if let Some(ref key) = key_path {
        cmd.args(["-i", key]);
    }
    cmd.args([
        "-p", &port.to_string(),
        &format!("{}@{}", user, host),
        "openclaw --version 2>/dev/null || echo UNKNOWN",
    ]);
    let output = cmd.output().map_err(|e| format!("SSH connection failed: {}", e))?;
    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(json!({"connected": true, "version": version, "host": host, "user": user}))
    } else {
        Err(format!("SSH to {}@{}:{} failed", user, host, port))
    }
}

#[tauri::command]
pub fn save_runtime_config(runtime: String, config: String) -> Result<(), String> {
    let ato_dir = home_dir().join(".ato");
    fs::create_dir_all(&ato_dir).ok();
    let file_path = ato_dir.join(format!("{}-config.json", runtime));
    fs::write(&file_path, config).map_err(|e| format!("Failed to save config: {}", e))
}

#[tauri::command]
pub fn load_runtime_config(runtime: String) -> Result<Option<String>, String> {
    let file_path = home_dir().join(".ato").join(format!("{}-config.json", runtime));
    Ok(read_file_lossy(&file_path))
}

#[tauri::command]
pub async fn test_runtime_connection(runtime: String, config: String) -> Result<serde_json::Value, String> {
    match runtime.as_str() {
        "openclaw" => {
            // Use SSH to test connection (gateway requires crypto auth for WebSocket)
            let (host, port, user, key_path) = load_openclaw_ssh_config()?;
            let user_path = get_user_path();
            let mut cmd = std::process::Command::new("ssh");
            cmd.env("PATH", &user_path);
            cmd.args(["-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=accept-new"]);
            if let Some(ref key) = key_path {
                cmd.args(["-i", key]);
            }
            cmd.args([
                "-p", &port.to_string(),
                &format!("{}@{}", user, host),
                "openclaw --version 2>/dev/null || echo UNKNOWN",
            ]);
            let output = cmd.output().map_err(|e| format!("SSH connection failed: {}", e))?;
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Ok(json!({"connected": true, "version": version, "host": host}))
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(format!("SSH to {}@{}:{} failed: {}", user, host, port, stderr))
            }
        }
        "claude" => {
            let path = which_cli("claude").ok_or("Claude CLI not found")?;
            let output = std::process::Command::new(&path).arg("--version").output().map_err(|e| e.to_string())?;
            Ok(json!({"connected": output.status.success(), "version": String::from_utf8_lossy(&output.stdout).trim().to_string()}))
        }
        "codex" => {
            let path = which_cli("codex").ok_or("Codex CLI not found")?;
            let output = std::process::Command::new(&path).arg("--version").output().map_err(|e| e.to_string())?;
            Ok(json!({"connected": output.status.success(), "version": String::from_utf8_lossy(&output.stdout).trim().to_string()}))
        }
        "hermes" => {
            let path = which_cli("hermes").ok_or("Hermes CLI not found")?;
            let output = std::process::Command::new(&path).arg("--version").output().map_err(|e| e.to_string())?;
            Ok(json!({"connected": output.status.success(), "version": String::from_utf8_lossy(&output.stdout).trim().to_string()}))
        }
        _ => Err(format!("Unknown runtime: {}", runtime))
    }
}

// ── OpenClaw Cron CRUD ────────────────────────────────────────────────────

#[tauri::command]
pub async fn openclaw_edit_cron_job(id: String, args: String) -> Result<serde_json::Value, String> {
    // args is a space-separated string of CLI flags like "--name foo --every 1h --message 'do stuff'"
    openclaw_ssh_command(&format!("cron edit {} {} --json", id, args))
}

#[tauri::command]
pub async fn openclaw_add_cron_job(args: String) -> Result<serde_json::Value, String> {
    openclaw_ssh_command(&format!("cron add {} --json", args))
}

#[tauri::command]
pub async fn openclaw_delete_cron_job(id: String) -> Result<serde_json::Value, String> {
    openclaw_ssh_command(&format!("cron rm {} --json", id))
}

#[tauri::command]
pub async fn openclaw_run_cron_job(id: String) -> Result<serde_json::Value, String> {
    openclaw_ssh_command(&format!("cron run {} --json", id))
}

#[tauri::command]
pub async fn openclaw_toggle_cron_job(id: String, enable: bool) -> Result<serde_json::Value, String> {
    let flag = if enable { "--enable" } else { "--disable" };
    openclaw_ssh_command(&format!("cron edit {} {} --json", id, flag))
}

// ── Remote OpenClaw Skills ────────────────────────────────────────────────

#[tauri::command]
pub async fn openclaw_list_skills() -> Result<Vec<LocalSkill>, String> {
    let mut skills = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Scan multiple known OpenClaw skill directories
    let dirs = [
        "~/.openclaw/skills",
        "~/.openclaw/workspace/skills",
    ];

    for dir in &dirs {
        let cmd = format!("ls {} 2>/dev/null", dir);
        if let Ok(text) = openclaw_ssh_raw(&cmd) {
            for name in text.lines().filter(|l| !l.is_empty()) {
                let name = name.trim().to_string();
                if seen.contains(&name) { continue; }
                seen.insert(name.clone());
                skills.push(LocalSkill {
                    id: format!("oc-skill-{}", name),
                    name: name.clone(),
                    description: format!("OpenClaw skill: {}", name),
                    file_path: format!("{}/{}", dir, name),
                    scope: "personal".to_string(),
                    runtime: "openclaw".to_string(),
                    project: None,
                    token_count: 0,
                    enabled: true,
                    content_hash: "".to_string(),
                });
            }
        }
    }

    // Also detect pseudo-skills from AGENTS.md, SOUL.md, TOOLS.md
    let special_files = ["AGENTS.md", "SOUL.md", "TOOLS.md"];
    for f in &special_files {
        let cmd = format!("test -f ~/.openclaw/workspace/{} && echo exists", f);
        if let Ok(text) = openclaw_ssh_raw(&cmd) {
            if text.contains("exists") {
                let name = f.trim_end_matches(".md").to_lowercase();
                if !seen.contains(&name) {
                    seen.insert(name.clone());
                    skills.push(LocalSkill {
                        id: format!("oc-skill-{}", name),
                        name,
                        description: format!("OpenClaw context: {}", f),
                        file_path: format!("~/.openclaw/workspace/{}", f),
                        scope: "personal".to_string(),
                        runtime: "openclaw".to_string(),
                        project: None,
                        token_count: 0,
                        enabled: true,
                        content_hash: "".to_string(),
                    });
                }
            }
        }
    }

    Ok(skills)
}

// ── Context Files (SOUL.md, AGENTS.md, etc.) ─────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContextFile {
    pub runtime: String,
    pub name: String,
    pub file_path: String,
    pub token_count: u64,
    pub exists: bool,
}

#[tauri::command]
pub fn list_context_files() -> Result<Vec<ContextFile>, String> {
    let mut files = Vec::new();

    // Claude context files
    let claude = claude_home();
    for name in ["CLAUDE.md", "settings.json", "settings.local.json"] {
        let p = claude.join(name);
        let exists = p.exists();
        let tokens = if exists { estimate_tokens(fs::metadata(&p).map(|m| m.len()).unwrap_or(0)) } else { 0 };
        files.push(ContextFile {
            runtime: "claude".into(), name: name.into(),
            file_path: p.to_string_lossy().into(), token_count: tokens, exists,
        });
    }
    // Project CLAUDE.md
    for proj in discover_project_roots() {
        let p = proj.join("CLAUDE.md");
        if p.exists() {
            let tokens = estimate_tokens(fs::metadata(&p).map(|m| m.len()).unwrap_or(0));
            let label = format!("CLAUDE.md ({})", proj.file_name().unwrap_or_default().to_string_lossy());
            files.push(ContextFile {
                runtime: "claude".into(), name: label,
                file_path: p.to_string_lossy().into(), token_count: tokens, exists: true,
            });
        }
    }

    // OpenClaw context files
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));
    let oc_workspace = oc_home.join("workspace");
    for name in ["SOUL.md", "AGENTS.md", "TOOLS.md", "config.yaml"] {
        let p = oc_workspace.join(name);
        let exists = p.exists();
        let tokens = if exists { estimate_tokens(fs::metadata(&p).map(|m| m.len()).unwrap_or(0)) } else { 0 };
        files.push(ContextFile {
            runtime: "openclaw".into(), name: name.into(),
            file_path: p.to_string_lossy().into(), token_count: tokens, exists,
        });
    }
    // Also check root .openclaw
    for name in ["SOUL.md", "config.yaml"] {
        let p = oc_home.join(name);
        if p.exists() && !files.iter().any(|f| f.file_path == p.to_string_lossy().to_string()) {
            let tokens = estimate_tokens(fs::metadata(&p).map(|m| m.len()).unwrap_or(0));
            files.push(ContextFile {
                runtime: "openclaw".into(), name: name.into(),
                file_path: p.to_string_lossy().into(), token_count: tokens, exists: true,
            });
        }
    }

    // Hermes context files
    let hermes_home = home_dir().join(".hermes");
    for name in ["SOUL.md", "config.yaml", "memories/MEMORY.md", "memories/USER.md"] {
        let p = hermes_home.join(name);
        let exists = p.exists();
        let tokens = if exists { estimate_tokens(fs::metadata(&p).map(|m| m.len()).unwrap_or(0)) } else { 0 };
        files.push(ContextFile {
            runtime: "hermes".into(), name: name.into(),
            file_path: p.to_string_lossy().into(), token_count: tokens, exists,
        });
    }

    // Filter to only existing files
    Ok(files.into_iter().filter(|f| f.exists).collect())
}

#[tauri::command]
pub fn read_context_file(file_path: String) -> Result<String, String> {
    read_file_lossy(&PathBuf::from(&file_path)).ok_or_else(|| format!("Cannot read: {}", file_path))
}

#[tauri::command]
pub fn write_context_file(file_path: String, content: String) -> Result<(), String> {
    fs::write(&file_path, &content).map_err(|e| format!("Failed to write: {}", e))
}

// ── Agent Configuration Manager ──────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigFile {
    pub path: String,
    pub scope: String,           // "global" | "project"
    pub runtime: String,         // "claude" | "codex" | "openclaw" | "hermes" | "shared"
    pub file_type: String,       // "skill" | "settings" | "project-config" | "mcp" | "soul"
    pub exists: bool,
    pub last_modified: Option<String>,
    pub token_count: Option<u64>,
    pub project_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ParsedConfigFile {
    pub path: String,
    pub format: String,          // "yaml-frontmatter" | "json" | "toml" | "yaml" | "markdown"
    pub content: serde_json::Value,  // Parsed content as JSON
    pub raw: String,             // Original file content
    pub content_hash: String,    // SHA-256 of raw content (hex) for conflict detection
    pub last_modified: Option<u64>, // Unix seconds
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    pub path: String,
    pub new_hash: String,
    pub bytes_written: u64,
    pub backup_path: Option<String>,
    pub added_lines: usize,
    pub removed_lines: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: String, // "add" | "remove" | "context"
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub line: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Permission {
    pub tool: String,
    pub pattern: Option<String>,
    pub allowed: bool,
    pub requires_approval: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContextPreviewSection {
    pub name: String,
    pub tokens: u64,
    pub files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContextPreview {
    pub total_tokens: u64,
    pub limit: u64,
    pub sections: Vec<ContextPreviewSection>,
}

// ── Skill Health Check ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub code: String,           // "MISSING_FRONTMATTER", "TOKEN_SIZE_WARNING", etc.
    pub severity: String,       // "error" | "warning"
    pub message: String,
    pub line: Option<u32>,
    pub suggestion: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillValidation {
    pub path: String,
    pub skill_name: Option<String>,
    pub valid: bool,
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
    pub token_count: u64,
}

// ── Profile Snapshots ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFile {
    pub path: String,           // Relative path from home or project
    pub content: String,
    pub scope: String,          // "global" | "project"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSnapshot {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub runtime: String,
    pub files: Vec<ProfileFile>,
    pub created_at: String,
}

// ── Skill Usage Analytics ───────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsageStat {
    pub skill_path: String,
    pub skill_name: String,
    pub trigger_count: u32,
    pub last_used: Option<String>,
    pub avg_tokens: Option<u32>,
}

// ── Onboarding Checklist ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingAction {
    pub action_type: String,    // "create_file" | "open_editor" | "run_command" | "external_link"
    pub target: String,         // Path or URL
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingItem {
    pub id: String,
    pub label: String,
    pub completed: bool,
    pub action: Option<OnboardingAction>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStatus {
    pub runtime: String,
    pub items: Vec<OnboardingItem>,
    pub completion_percent: u8,
}

// ── Project Manager ─────────────────────────────────────────────────────────

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

/// Scan all config files for all runtimes in both global and project scopes
/// Based on official documentation for Claude Code, Codex CLI, Hermes, and OpenClaw
#[tauri::command]
pub fn scan_agent_config_files(project_path: Option<String>) -> Result<Vec<AgentConfigFile>, String> {
    let home = home_dir();
    let mut configs = Vec::new();

    // Determine project roots to scan
    let project_roots: Vec<PathBuf> = if let Some(ref p) = project_path {
        vec![PathBuf::from(p)]
    } else {
        discover_project_roots()
    };

    // ══════════════════════════════════════════════════════════════════════════
    // CLAUDE CODE - Global Config Files
    // Docs: https://docs.anthropic.com/en/docs/claude-code
    // ══════════════════════════════════════════════════════════════════════════
    let claude_home = home.join(".claude");

    // Settings
    add_config_if_exists(&mut configs, claude_home.join("settings.json"), "global", "claude", "settings", None);

    // MCP servers, OAuth, preferences
    add_config_if_exists(&mut configs, home.join(".claude.json"), "global", "claude", "mcp", None);

    // User-level CLAUDE.md (personal instructions)
    add_config_if_exists(&mut configs, claude_home.join("CLAUDE.md"), "global", "claude", "project-config", None);

    // Keybindings
    add_config_if_exists(&mut configs, claude_home.join("keybindings.json"), "global", "claude", "settings", None);

    // Skills directory
    let claude_skills = claude_home.join("skills");
    if claude_skills.exists() {
        scan_skills_directory(&mut configs, &claude_skills, "global", "claude", None);
    }

    // Subagents directory (~/.claude/agents/*.md)
    let claude_agents = claude_home.join("agents");
    if claude_agents.exists() {
        scan_md_directory(&mut configs, &claude_agents, "global", "claude", "subagent", None);
    }

    // Rules directory (~/.claude/rules/*.md)
    let claude_rules = claude_home.join("rules");
    if claude_rules.exists() {
        scan_md_directory(&mut configs, &claude_rules, "global", "claude", "rules", None);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // CODEX CLI - Global Config Files
    // Docs: https://developers.openai.com/codex/config-reference
    // ══════════════════════════════════════════════════════════════════════════
    let codex_home = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".codex"));

    // Primary config (TOML format)
    add_config_if_exists(&mut configs, codex_home.join("config.toml"), "global", "codex", "settings", None);

    // Organization requirements
    add_config_if_exists(&mut configs, codex_home.join("requirements.toml"), "global", "codex", "settings", None);

    // System-wide config
    add_config_if_exists(&mut configs, PathBuf::from("/etc/codex/config.toml"), "global", "codex", "settings", None);

    // User-level skills (~/.agents/skills/ - shared with OpenClaw)
    let user_agents_skills = home.join(".agents").join("skills");
    if user_agents_skills.exists() {
        scan_skills_directory(&mut configs, &user_agents_skills, "global", "codex", None);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // HERMES - Global Config Files
    // Docs: https://hermes-agent.nousresearch.com/docs/
    // ══════════════════════════════════════════════════════════════════════════
    let hermes_home = std::env::var("HERMES_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".hermes"));

    // Primary config (YAML format)
    add_config_if_exists(&mut configs, hermes_home.join("config.yaml"), "global", "hermes", "settings", None);

    // Environment variables
    add_config_if_exists(&mut configs, hermes_home.join(".env"), "global", "hermes", "settings", None);

    // OAuth tokens
    add_config_if_exists(&mut configs, hermes_home.join("auth.json"), "global", "hermes", "settings", None);

    // Agent identity/personality
    add_config_if_exists(&mut configs, hermes_home.join("SOUL.md"), "global", "hermes", "soul", None);

    // Memories directory
    let hermes_memories = hermes_home.join("memories");
    add_config_if_exists(&mut configs, hermes_memories.join("MEMORY.md"), "global", "hermes", "memory", None);
    add_config_if_exists(&mut configs, hermes_memories.join("USER.md"), "global", "hermes", "memory", None);

    // Skills directory (with category subdirs)
    let hermes_skills = hermes_home.join("skills");
    if hermes_skills.exists() {
        scan_skills_directory_recursive(&mut configs, &hermes_skills, "global", "hermes", None);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // OPENCLAW - Global Config Files
    // Docs: https://docs.openclaw.ai/
    // ══════════════════════════════════════════════════════════════════════════
    let openclaw_home = std::env::var("OPENCLAW_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".openclaw"));

    // Main config (JSON5 format)
    add_config_if_exists(&mut configs, openclaw_home.join("openclaw.json"), "global", "openclaw", "settings", None);

    // Managed/local skills
    let openclaw_skills = openclaw_home.join("skills");
    if openclaw_skills.exists() {
        scan_skills_directory(&mut configs, &openclaw_skills, "global", "openclaw", None);
    }

    // Personal agent skills (~/.agents/skills/ - shared with Codex)
    // Already scanned above for Codex, add for OpenClaw too
    if user_agents_skills.exists() {
        scan_skills_directory(&mut configs, &user_agents_skills, "global", "openclaw", None);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // PROJECT-LEVEL CONFIG FILES
    // ══════════════════════════════════════════════════════════════════════════
    for project_root in project_roots {
        let project_name = project_root.file_name()
            .map(|n| n.to_string_lossy().to_string());

        // ── CLAUDE CODE - Project ──
        // Main project instructions
        add_config_if_exists(&mut configs, project_root.join("CLAUDE.md"), "project", "claude", "project-config", project_name.clone());
        // Alternative location
        add_config_if_exists(&mut configs, project_root.join(".claude").join("CLAUDE.md"), "project", "claude", "project-config", project_name.clone());
        // Local overrides (gitignored)
        add_config_if_exists(&mut configs, project_root.join("CLAUDE.local.md"), "project", "claude", "project-config", project_name.clone());
        // Shared settings
        add_config_if_exists(&mut configs, project_root.join(".claude").join("settings.json"), "project", "claude", "settings", project_name.clone());
        // Local settings (gitignored)
        add_config_if_exists(&mut configs, project_root.join(".claude").join("settings.local.json"), "project", "claude", "settings", project_name.clone());
        // Project MCP servers
        add_config_if_exists(&mut configs, project_root.join(".mcp.json"), "project", "claude", "mcp", project_name.clone());

        // Project skills
        let project_claude_skills = project_root.join(".claude").join("skills");
        if project_claude_skills.exists() {
            scan_skills_directory(&mut configs, &project_claude_skills, "project", "claude", project_name.clone());
        }
        // Project subagents
        let project_claude_agents = project_root.join(".claude").join("agents");
        if project_claude_agents.exists() {
            scan_md_directory(&mut configs, &project_claude_agents, "project", "claude", "subagent", project_name.clone());
        }
        // Project rules
        let project_claude_rules = project_root.join(".claude").join("rules");
        if project_claude_rules.exists() {
            scan_md_directory(&mut configs, &project_claude_rules, "project", "claude", "rules", project_name.clone());
        }

        // ── CODEX CLI - Project ──
        // Project instructions (Codex uses AGENTS.md)
        add_config_if_exists(&mut configs, project_root.join("AGENTS.md"), "project", "codex", "project-config", project_name.clone());
        add_config_if_exists(&mut configs, project_root.join("AGENTS.override.md"), "project", "codex", "project-config", project_name.clone());
        // Project config
        add_config_if_exists(&mut configs, project_root.join(".codex").join("config.toml"), "project", "codex", "settings", project_name.clone());
        // Project skills (.agents/skills/)
        let project_agents_skills = project_root.join(".agents").join("skills");
        if project_agents_skills.exists() {
            scan_skills_directory(&mut configs, &project_agents_skills, "project", "codex", project_name.clone());
        }

        // ── HERMES - Project ──
        // Hermes-specific project instructions (highest priority)
        add_config_if_exists(&mut configs, project_root.join(".hermes.md"), "project", "hermes", "project-config", project_name.clone());
        // Falls back to AGENTS.md (compatible)
        // AGENTS.md already added for Codex, mark as shared
        // Falls back to CLAUDE.md (compatible) - already added
        // Project config
        add_config_if_exists(&mut configs, project_root.join(".hermes").join("config.yaml"), "project", "hermes", "settings", project_name.clone());
        // Project skills
        let project_hermes_skills = project_root.join(".hermes").join("skills");
        if project_hermes_skills.exists() {
            scan_skills_directory_recursive(&mut configs, &project_hermes_skills, "project", "hermes", project_name.clone());
        }

        // ── OPENCLAW - Project/Workspace ──
        // SOUL.md - Agent personality (shared between Hermes & OpenClaw)
        add_config_if_exists(&mut configs, project_root.join("SOUL.md"), "project", "shared", "soul", project_name.clone());
        // AGENTS.md - Operating rules (already added for Codex)
        // USER.md - Personal user context
        add_config_if_exists(&mut configs, project_root.join("USER.md"), "project", "openclaw", "memory", project_name.clone());
        // IDENTITY.md - Agent name, emoji, avatar
        add_config_if_exists(&mut configs, project_root.join("IDENTITY.md"), "project", "openclaw", "project-config", project_name.clone());
        // TOOLS.md - Environment-specific tool notes
        add_config_if_exists(&mut configs, project_root.join("TOOLS.md"), "project", "openclaw", "project-config", project_name.clone());
        // MEMORY.md - Long-term memories
        add_config_if_exists(&mut configs, project_root.join("MEMORY.md"), "project", "openclaw", "memory", project_name.clone());
        // HEARTBEAT.md - Scheduled tasks
        add_config_if_exists(&mut configs, project_root.join("HEARTBEAT.md"), "project", "openclaw", "project-config", project_name.clone());
        // Workspace config
        add_config_if_exists(&mut configs, project_root.join(".openclaw").join("openclaw.json"), "project", "openclaw", "settings", project_name.clone());
        // Workspace skills (highest priority for OpenClaw)
        let project_openclaw_skills = project_root.join("skills");
        if project_openclaw_skills.exists() {
            scan_skills_directory(&mut configs, &project_openclaw_skills, "project", "openclaw", project_name.clone());
        }
        // .agents/skills/ for OpenClaw too
        if project_agents_skills.exists() {
            scan_skills_directory(&mut configs, &project_agents_skills, "project", "openclaw", project_name.clone());
        }
    }

    Ok(configs)
}

/// Scan a directory for .md files (used for agents/, rules/)
pub fn scan_md_directory(
    configs: &mut Vec<AgentConfigFile>,
    dir: &PathBuf,
    scope: &str,
    runtime: &str,
    file_type: &str,
    project_name: Option<String>,
) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                add_config_if_exists(configs, path, scope, runtime, file_type, project_name.clone());
            }
        }
    }
}

/// Scan skills directory recursively (for Hermes category subdirs)
pub fn scan_skills_directory_recursive(
    configs: &mut Vec<AgentConfigFile>,
    dir: &PathBuf,
    scope: &str,
    runtime: &str,
    project_name: Option<String>,
) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Check if this is a skill directory (has SKILL.md)
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    add_config_if_exists(configs, skill_file, scope, runtime, "skill", project_name.clone());
                } else {
                    // It's a category directory, recurse
                    scan_skills_directory_recursive(configs, &path, scope, runtime, project_name.clone());
                }
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                // Single file skill
                add_config_if_exists(configs, path, scope, runtime, "skill", project_name.clone());
            }
        }
    }
}

pub fn add_config_if_exists(
    configs: &mut Vec<AgentConfigFile>,
    path: PathBuf,
    scope: &str,
    runtime: &str,
    file_type: &str,
    project_name: Option<String>,
) {
    let exists = path.exists();
    let last_modified = if exists {
        fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| {
                let secs = d.as_secs();
                // Format as ISO 8601
                let datetime = chrono_lite(secs);
                datetime
            })
    } else {
        None
    };

    let token_count = if exists {
        fs::read_to_string(&path)
            .ok()
            .map(|content| estimate_tokens(content.len() as u64))
    } else {
        None
    };

    configs.push(AgentConfigFile {
        path: path.to_string_lossy().to_string(),
        scope: scope.to_string(),
        runtime: runtime.to_string(),
        file_type: file_type.to_string(),
        exists,
        last_modified,
        token_count,
        project_name,
    });
}

pub fn scan_skills_directory(
    configs: &mut Vec<AgentConfigFile>,
    dir: &PathBuf,
    scope: &str,
    runtime: &str,
    project_name: Option<String>,
) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Directory skill - look for SKILL.md
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    add_config_if_exists(configs, skill_file, scope, runtime, "skill", project_name.clone());
                }
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                // Single file skill
                add_config_if_exists(configs, path, scope, runtime, "skill", project_name.clone());
            }
        }
    }
}

/// Simple datetime formatter (avoid adding chrono dependency)
pub fn chrono_lite(unix_secs: u64) -> String {
    // Basic ISO 8601 format without full chrono dependency
    // Just return the unix timestamp as a string for now
    format!("{}", unix_secs)
}

/// SHA-256 hex digest of a byte slice
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Copy a file to ~/.ato/backups/<timestamp>-<sha8>-<filename>. Returns backup path.
/// Silently prunes backups older than 30 days on every call.
pub fn backup_file(path: &PathBuf) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let backups_dir = home_dir().join(".ato").join("backups");
    fs::create_dir_all(&backups_dir).map_err(|e| format!("backup dir: {}", e))?;

    let content = fs::read(path).map_err(|e| format!("read for backup: {}", e))?;
    let hash = sha256_hex(&content);
    let sha8 = &hash[..8];
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let backup_name = format!("{}-{}-{}", ts, sha8, filename);
    let backup_path = backups_dir.join(&backup_name);
    fs::write(&backup_path, &content).map_err(|e| format!("write backup: {}", e))?;

    // Prune >30d old (best-effort, ignore errors)
    let cutoff = ts.saturating_sub(30 * 24 * 60 * 60);
    if let Ok(entries) = fs::read_dir(&backups_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(ts_str) = name_str.split('-').next() {
                if let Ok(entry_ts) = ts_str.parse::<u64>() {
                    if entry_ts < cutoff {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
    }

    Ok(Some(backup_path.to_string_lossy().to_string()))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub backup_path: String,
    pub original_filename: String,
    pub timestamp: u64,         // Unix seconds
    pub sha8: String,           // First 8 chars of SHA-256
    pub size_bytes: u64,
}

/// List all backups in ~/.ato/backups/. If `original_path` is provided, filter to
/// backups whose filename matches that path's basename.
#[tauri::command]
pub fn list_backups(original_path: Option<String>) -> Result<Vec<BackupEntry>, String> {
    let backups_dir = home_dir().join(".ato").join("backups");
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }
    let filter_name = original_path.as_ref().and_then(|p| {
        PathBuf::from(p).file_name().and_then(|n| n.to_str()).map(String::from)
    });

    let mut entries: Vec<BackupEntry> = Vec::new();
    if let Ok(dir) = fs::read_dir(&backups_dir) {
        for entry in dir.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            // Expected format: <timestamp>-<sha8>-<filename>
            let parts: Vec<&str> = name.splitn(3, '-').collect();
            if parts.len() != 3 {
                continue;
            }
            let Ok(timestamp) = parts[0].parse::<u64>() else { continue };
            let sha8 = parts[1].to_string();
            let original_filename = parts[2].to_string();

            if let Some(ref want) = filter_name {
                if &original_filename != want {
                    continue;
                }
            }

            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            entries.push(BackupEntry {
                backup_path: path.to_string_lossy().to_string(),
                original_filename,
                timestamp,
                sha8,
                size_bytes,
            });
        }
    }

    // Newest first
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(entries)
}

/// Restore a backup by copying its contents to `target_path`. Goes through the
/// same safety pipeline (hash check, backup-current, audit) as a regular write.
#[tauri::command]
pub fn restore_backup(
    db: State<'_, DbState>,
    backup_path: String,
    target_path: String,
    expected_hash: Option<String>,
) -> Result<WriteResult, String> {
    let backup_pb = PathBuf::from(&backup_path);
    let content = fs::read_to_string(&backup_pb)
        .map_err(|e| format!("Failed to read backup: {}", e))?;
    write_agent_config_file(db, target_path, content, expected_hash, Some(true))
}

// ── Ollama Provider ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub running: bool,
    pub version: Option<String>,
    pub endpoint: String,
}

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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OllamaConfig {
    pub host: Option<String>,
    pub models_dir: Option<String>,
    pub keep_alive: Option<String>,
    pub flash_attention: Option<String>,
    pub cuda_visible_devices: Option<String>,
    pub num_parallel: Option<String>,
}

#[tauri::command]
pub async fn detect_ollama() -> Result<OllamaStatus, String> {
    let endpoint = std::env::var("OLLAMA_HOST")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let url = format!("{}/api/version", endpoint);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let version = body.get("version").and_then(|v| v.as_str()).map(String::from);
            Ok(OllamaStatus { running: true, version, endpoint })
        }
        _ => Ok(OllamaStatus { running: false, version: None, endpoint }),
    }
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

#[tauri::command]
pub fn get_ollama_config() -> OllamaConfig {
    OllamaConfig {
        host: std::env::var("OLLAMA_HOST").ok(),
        models_dir: std::env::var("OLLAMA_MODELS").ok(),
        keep_alive: std::env::var("OLLAMA_KEEP_ALIVE").ok(),
        flash_attention: std::env::var("OLLAMA_FLASH_ATTENTION").ok(),
        cuda_visible_devices: std::env::var("CUDA_VISIBLE_DEVICES").ok(),
        num_parallel: std::env::var("OLLAMA_NUM_PARALLEL").ok(),
    }
}

/// Simple line-by-line diff. Marks every line add/remove/context using LCS-free approach:
/// finds longest common prefix/suffix then marks the middle chunks.
pub fn compute_diff(old: &str, new: &str) -> (Vec<DiffLine>, usize, usize) {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Longest common prefix
    let mut prefix = 0;
    while prefix < old_lines.len() && prefix < new_lines.len() && old_lines[prefix] == new_lines[prefix] {
        prefix += 1;
    }
    // Longest common suffix (bounded)
    let mut suffix = 0;
    while suffix < old_lines.len() - prefix
        && suffix < new_lines.len() - prefix
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mut diff = Vec::new();
    let context_lines = 3usize;

    // Leading context
    let leading_start = prefix.saturating_sub(context_lines);
    for i in leading_start..prefix {
        diff.push(DiffLine {
            kind: "context".to_string(),
            old_line: Some(i + 1),
            new_line: Some(i + 1),
            text: old_lines[i].to_string(),
        });
    }

    // Removals
    let old_end = old_lines.len() - suffix;
    for i in prefix..old_end {
        diff.push(DiffLine {
            kind: "remove".to_string(),
            old_line: Some(i + 1),
            new_line: None,
            text: old_lines[i].to_string(),
        });
    }

    // Additions
    let new_end = new_lines.len() - suffix;
    for i in prefix..new_end {
        diff.push(DiffLine {
            kind: "add".to_string(),
            old_line: None,
            new_line: Some(i + 1),
            text: new_lines[i].to_string(),
        });
    }

    // Trailing context
    let trailing_end = (old_end + context_lines).min(old_lines.len());
    for i in old_end..trailing_end {
        diff.push(DiffLine {
            kind: "context".to_string(),
            old_line: Some(i + 1),
            new_line: Some(new_end + (i - old_end) + 1),
            text: old_lines[i].to_string(),
        });
    }

    let added = new_end.saturating_sub(prefix);
    let removed = old_end.saturating_sub(prefix);
    (diff, added, removed)
}

/// Validate Claude Code `settings.json` shape. Permissive on unknown keys;
/// strict on known structure (permissions, hooks, mcpServers, env).
#[tauri::command]
pub fn validate_settings_json(content: String) -> Result<ValidationResult, String> {
    let mut errors = Vec::new();

    let value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            errors.push(ValidationError {
                field: "$".to_string(),
                message: format!("Invalid JSON: {}", e),
                line: Some(e.line()),
            });
            return Ok(ValidationResult { valid: false, errors });
        }
    };

    if !value.is_object() {
        errors.push(ValidationError {
            field: "$".to_string(),
            message: "Root must be an object".to_string(),
            line: None,
        });
        return Ok(ValidationResult { valid: false, errors });
    }

    let obj = value.as_object().unwrap();

    // permissions: { allow?: string[], deny?: string[], ask?: string[] }
    if let Some(perms) = obj.get("permissions") {
        if !perms.is_object() {
            errors.push(ValidationError {
                field: "permissions".to_string(),
                message: "Must be an object".to_string(),
                line: None,
            });
        } else {
            for key in ["allow", "deny", "ask"] {
                if let Some(arr) = perms.get(key) {
                    if !arr.is_array() {
                        errors.push(ValidationError {
                            field: format!("permissions.{}", key),
                            message: "Must be an array of strings".to_string(),
                            line: None,
                        });
                    } else if let Some(items) = arr.as_array() {
                        for (i, item) in items.iter().enumerate() {
                            if !item.is_string() {
                                errors.push(ValidationError {
                                    field: format!("permissions.{}[{}]", key, i),
                                    message: "Must be a string".to_string(),
                                    line: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // hooks: { [event]: [{ matcher, hooks: [{ type, command }] }] }
    if let Some(hooks) = obj.get("hooks") {
        if !hooks.is_object() {
            errors.push(ValidationError {
                field: "hooks".to_string(),
                message: "Must be an object keyed by event name".to_string(),
                line: None,
            });
        }
    }

    // mcpServers: { [name]: { command, args?, env? } | { url, ... } }
    if let Some(mcp) = obj.get("mcpServers") {
        if !mcp.is_object() {
            errors.push(ValidationError {
                field: "mcpServers".to_string(),
                message: "Must be an object keyed by server name".to_string(),
                line: None,
            });
        } else if let Some(servers) = mcp.as_object() {
            for (name, server) in servers {
                if !server.is_object() {
                    errors.push(ValidationError {
                        field: format!("mcpServers.{}", name),
                        message: "Each MCP server must be an object".to_string(),
                        line: None,
                    });
                    continue;
                }
                let so = server.as_object().unwrap();
                let has_command = so.get("command").map(|v| v.is_string()).unwrap_or(false);
                let has_url = so.get("url").map(|v| v.is_string()).unwrap_or(false);
                if !has_command && !has_url {
                    errors.push(ValidationError {
                        field: format!("mcpServers.{}", name),
                        message: "Must have either 'command' (stdio) or 'url' (http/sse)".to_string(),
                        line: None,
                    });
                }
            }
        }
    }

    // env: { [key]: string }
    if let Some(env) = obj.get("env") {
        if !env.is_object() {
            errors.push(ValidationError {
                field: "env".to_string(),
                message: "Must be an object of string values".to_string(),
                line: None,
            });
        } else if let Some(vars) = env.as_object() {
            for (key, val) in vars {
                if !val.is_string() {
                    errors.push(ValidationError {
                        field: format!("env.{}", key),
                        message: "Env values must be strings".to_string(),
                        line: None,
                    });
                }
            }
        }
    }

    Ok(ValidationResult { valid: errors.is_empty(), errors })
}

/// Preview the diff + validation for a pending write without touching disk.
#[tauri::command]
pub fn preview_write_agent_config_file(path: String, new_content: String) -> Result<serde_json::Value, String> {
    let path_buf = PathBuf::from(&path);
    let old_content = if path_buf.exists() {
        fs::read_to_string(&path_buf).unwrap_or_default()
    } else {
        String::new()
    };
    let (diff, added, removed) = compute_diff(&old_content, &new_content);
    let current_hash = sha256_hex(old_content.as_bytes());
    let new_hash = sha256_hex(new_content.as_bytes());

    let mut validation: Option<ValidationResult> = None;
    let fname = path_buf.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if fname == "settings.json" || fname == "settings.local.json" {
        validation = Some(validate_settings_json(new_content.clone())?);
    }

    Ok(json!({
        "diff": diff,
        "addedLines": added,
        "removedLines": removed,
        "currentHash": current_hash,
        "newHash": new_hash,
        "validation": validation,
    }))
}

/// Read and parse a config file, handling different formats.
/// Returns content_hash (SHA-256) for conflict detection.
#[tauri::command]
pub fn read_agent_config_file(path: String) -> Result<ParsedConfigFile, String> {
    let mut path_buf = PathBuf::from(&path);
    // If path is a directory (e.g., a skill directory), look for SKILL.md or README.md inside
    if path_buf.is_dir() {
        let candidates = ["SKILL.md", "README.md", "index.md"];
        let mut found = false;
        for candidate in &candidates {
            let child = path_buf.join(candidate);
            if child.exists() {
                path_buf = child;
                found = true;
                break;
            }
        }
        if !found {
            // List directory contents as a fallback
            let entries: Vec<String> = fs::read_dir(&path_buf)
                .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
                .unwrap_or_default();
            return Err(format!("Path is a directory. Contents: {}", entries.join(", ")));
        }
    }
    let resolved_path = path_buf.to_string_lossy().to_string();

    let content = fs::read_to_string(&path_buf)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    let metadata = fs::metadata(&path_buf).ok();
    let last_modified = metadata
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
    let content_hash = sha256_hex(content.as_bytes());

    let extension = path_buf.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let (format, parsed) = match extension {
        "json" => {
            let value: serde_json::Value = serde_json::from_str(&content)
                .unwrap_or_else(|_| {
                    // Tolerate invalid JSON at read time so users can fix it in the editor.
                    let mut obj = serde_json::Map::new();
                    obj.insert("raw".to_string(), serde_json::Value::String(content.clone()));
                    serde_json::Value::Object(obj)
                });
            ("json".to_string(), value)
        }
        "toml" => {
            let parsed = parse_toml_to_json(&content);
            ("toml".to_string(), parsed)
        }
        "yaml" | "yml" => {
            let parsed = parse_simple_yaml(&content);
            ("yaml".to_string(), parsed)
        }
        "md" => {
            if content.trim_start().starts_with("---") {
                let (frontmatter, body) = parse_frontmatter(&content);
                let mut obj = serde_json::Map::new();
                obj.insert("frontmatter".to_string(), frontmatter);
                obj.insert("body".to_string(), serde_json::Value::String(body));
                ("yaml-frontmatter".to_string(), serde_json::Value::Object(obj))
            } else {
                let mut obj = serde_json::Map::new();
                obj.insert("body".to_string(), serde_json::Value::String(content.clone()));
                ("markdown".to_string(), serde_json::Value::Object(obj))
            }
        }
        _ => {
            let mut obj = serde_json::Map::new();
            obj.insert("raw".to_string(), serde_json::Value::String(content.clone()));
            ("unknown".to_string(), serde_json::Value::Object(obj))
        }
    };

    Ok(ParsedConfigFile {
        path: resolved_path,
        format,
        content: parsed,
        raw: content,
        content_hash,
        last_modified,
        size_bytes,
    })
}

// ── Project File Watcher (per-project) ──────────────────────────────────────

use std::sync::atomic::{AtomicBool, Ordering};

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

/// Parse TOML content using the full toml crate (handles nested tables, arrays, inline tables, etc.)
pub fn parse_toml_to_json(content: &str) -> serde_json::Value {
    match content.parse::<toml::Value>() {
        Ok(val) => serde_json::to_value(val).unwrap_or_default(),
        Err(_) => {
            let mut obj = serde_json::Map::new();
            obj.insert("_parse_error".to_string(), serde_json::Value::String("Invalid TOML".to_string()));
            obj.insert("raw".to_string(), serde_json::Value::String(content.to_string()));
            serde_json::Value::Object(obj)
        }
    }
}

/// Convert a JSON value back to TOML string
pub fn json_to_toml(value: &serde_json::Value) -> Result<String, String> {
    let toml_val: toml::Value = serde_json::from_value(value.clone())
        .map_err(|e| format!("Cannot convert to TOML: {}", e))?;
    toml::to_string_pretty(&toml_val)
        .map_err(|e| format!("Cannot serialize TOML: {}", e))
}

/// Simple YAML parser (basic key-value pairs)
pub fn parse_simple_yaml(content: &str) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    let mut current_key: Option<String> = None;
    let mut current_indent = 0;
    let mut stack: Vec<(String, serde_json::Map<String, serde_json::Value>, usize)> = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() || line.trim().starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();

        // Key: value pair
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos].trim().to_string();
            let value_str = trimmed[colon_pos+1..].trim();

            if value_str.is_empty() {
                // Nested object starts
                current_key = Some(key);
                current_indent = indent;
            } else {
                // Simple value
                let value = parse_yaml_value(value_str);
                obj.insert(key, value);
            }
        }
    }

    serde_json::Value::Object(obj)
}

pub fn parse_yaml_value(s: &str) -> serde_json::Value {
    let s = s.trim();
    // Handle quoted strings
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        return serde_json::Value::String(s[1..s.len()-1].to_string());
    }
    // Handle booleans
    if s == "true" || s == "yes" {
        return serde_json::Value::Bool(true);
    }
    if s == "false" || s == "no" {
        return serde_json::Value::Bool(false);
    }
    // Handle null
    if s == "null" || s == "~" {
        return serde_json::Value::Null;
    }
    // Handle numbers
    if let Ok(n) = s.parse::<i64>() {
        return serde_json::Value::Number(n.into());
    }
    if let Ok(n) = s.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(n) {
            return serde_json::Value::Number(num);
        }
    }
    // Default to string
    serde_json::Value::String(s.to_string())
}

/// Write a config file back to disk with company-grade safety:
/// - content-hash conflict detection (reject if on-disk file changed since read)
/// - automatic timestamped backup to ~/.ato/backups/
/// - audit log entry in audit_logs SQLite table
/// - optional pre-write validation for known schemas (settings.json)
#[tauri::command]
pub fn write_agent_config_file(
    db: State<'_, DbState>,
    path: String,
    content: String,
    expected_hash: Option<String>,
    skip_validation: Option<bool>,
) -> Result<WriteResult, String> {
    let path_buf = PathBuf::from(&path);

    // 1. Conflict detection: if caller provided expected_hash, verify current on-disk matches.
    let (current_content, current_hash) = if path_buf.exists() {
        let c = fs::read_to_string(&path_buf)
            .map_err(|e| format!("Failed to read current file: {}", e))?;
        let h = sha256_hex(c.as_bytes());
        (c, h)
    } else {
        (String::new(), sha256_hex(&[]))
    };

    if let Some(expected) = &expected_hash {
        if expected != &current_hash {
            return Err(format!(
                "CONFLICT: file changed on disk since it was loaded (expected hash {}, found {}). Reload before saving.",
                &expected[..8], &current_hash[..8]
            ));
        }
    }

    // 2. Schema validation for settings.json (skippable via flag for escape hatch).
    let skip = skip_validation.unwrap_or(false);
    if !skip {
        let fname = path_buf.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if fname == "settings.json" || fname == "settings.local.json" {
            let result = validate_settings_json(content.clone())?;
            if !result.valid {
                let msgs: Vec<String> = result.errors.iter()
                    .map(|e| format!("{}: {}", e.field, e.message))
                    .collect();
                return Err(format!("VALIDATION_FAILED: {}", msgs.join("; ")));
            }
        }
    }

    // 3. Create parent dirs if needed
    if let Some(parent) = path_buf.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    // 4. Backup current contents before overwriting (no-op if file doesn't exist yet)
    let backup_path = backup_file(&path_buf)?;

    // 5. Write
    fs::write(&path_buf, &content)
        .map_err(|e| format!("Failed to write file: {}", e))?;

    let new_hash = sha256_hex(content.as_bytes());
    let (_, added, removed) = compute_diff(&current_content, &content);
    let bytes_written = content.as_bytes().len() as u64;

    // 6. Audit log
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let fname = path_buf.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let details = json!({
            "path": &path,
            "oldHash": current_hash,
            "newHash": new_hash,
            "addedLines": added,
            "removedLines": removed,
            "bytesWritten": bytes_written,
            "backupPath": backup_path,
        }).to_string();
        let _ = conn.execute(
            "INSERT INTO audit_logs (id, action, resource_type, resource_id, resource_name, details, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, "file_write", "config_file", Some(&path), Some(&fname), Some(details), now],
        );
    }

    Ok(WriteResult {
        path,
        new_hash,
        bytes_written,
        backup_path,
        added_lines: added,
        removed_lines: removed,
    })
}

/// Create a new skill file from template
#[tauri::command]
pub fn create_agent_skill(runtime: String, name: String, scope: String, description: String) -> Result<String, String> {
    let home = home_dir();
    let skill_slug = name.replace(' ', "-").to_lowercase();

    // Determine base directory based on runtime and scope (per official docs)
    let base_dir = match (runtime.as_str(), scope.as_str()) {
        // Claude: ~/.claude/skills/ or .claude/skills/
        ("claude", "global") => home.join(".claude").join("skills"),
        ("claude", "project") => project_root().join(".claude").join("skills"),
        // Codex: ~/.agents/skills/ (shared) or .agents/skills/
        ("codex", "global") => home.join(".agents").join("skills"),
        ("codex", "project") => project_root().join(".agents").join("skills"),
        // Hermes: ~/.hermes/skills/ or .hermes/skills/
        ("hermes", "global") => home.join(".hermes").join("skills"),
        ("hermes", "project") => project_root().join(".hermes").join("skills"),
        // OpenClaw: ~/.openclaw/skills/ or workspace/skills/
        ("openclaw", "global") => home.join(".openclaw").join("skills"),
        ("openclaw", "project") => project_root().join("skills"),
        _ => return Err(format!("Unknown runtime/scope: {}/{}", runtime, scope)),
    };

    // Create skill as directory with SKILL.md (recommended structure)
    let skill_dir = base_dir.join(&skill_slug);
    fs::create_dir_all(&skill_dir)
        .map_err(|e| format!("Failed to create skill directory: {}", e))?;

    let skill_path = skill_dir.join("SKILL.md");

    // Generate template based on runtime (different formats per docs)
    let template = match runtime.as_str() {
        "claude" => format!(
r#"---
name: {}
description: {}
allowed-tools:
  - Read
  - Edit
  - Bash
user-invocable: true
---

# {}

{}

## When to Use

Trigger this skill when...

## Instructions

[Add your skill instructions here]

## Verification

How to confirm the skill worked correctly.
"#,
            skill_slug, description, name, description
        ),
        "codex" => format!(
r#"---
name: {}
description: {}
---

# {}

{}

## When to Use

Trigger this skill when...

## Instructions

[Add your skill instructions here]

## Verification

How to confirm the skill worked correctly.
"#,
            skill_slug, description, name, description
        ),
        "hermes" => format!(
r#"---
name: {}
description: {}
version: 1.0.0
metadata:
  hermes:
    tags: [Custom]
    category: custom
---

# {}

{}

## When to Use

Trigger conditions and use cases.

## Quick Reference

Common commands or shortcuts.

## Procedure

1. Step one
2. Step two
3. Step three

## Pitfalls

Known failure modes and solutions.

## Verification

How to confirm the skill worked correctly.
"#,
            skill_slug, description, name, description
        ),
        "openclaw" => format!(
r#"---
name: {}
description: {}
user-invocable: true
---

# {}

{}

## When to Use

Trigger this skill when...

## Instructions

[Add your skill instructions here]

## Verification

How to confirm the skill worked correctly.
"#,
            skill_slug, description, name, description
        ),
        _ => return Err(format!("Unknown runtime: {}", runtime)),
    };

    fs::write(&skill_path, &template)
        .map_err(|e| format!("Failed to create skill file: {}", e))?;

    Ok(skill_path.to_string_lossy().to_string())
}

/// Parse permissions from a settings file
#[tauri::command]
pub fn parse_agent_permissions(path: String) -> Result<Vec<Permission>, String> {
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let mut permissions = Vec::new();

    // Try to parse as JSON (Claude settings.json format)
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
        // Claude format: { "permissions": { "allow": ["Bash(git:*)", "Read"] } }
        if let Some(perms) = json.get("permissions") {
            if let Some(allow) = perms.get("allow").and_then(|v| v.as_array()) {
                for item in allow {
                    if let Some(s) = item.as_str() {
                        let (tool, pattern) = parse_permission_string(s);
                        permissions.push(Permission {
                            tool,
                            pattern,
                            allowed: true,
                            requires_approval: false,
                        });
                    }
                }
            }
            if let Some(deny) = perms.get("deny").and_then(|v| v.as_array()) {
                for item in deny {
                    if let Some(s) = item.as_str() {
                        let (tool, pattern) = parse_permission_string(s);
                        permissions.push(Permission {
                            tool,
                            pattern,
                            allowed: false,
                            requires_approval: false,
                        });
                    }
                }
            }
        }
    }

    Ok(permissions)
}

pub fn parse_permission_string(s: &str) -> (String, Option<String>) {
    // Parse "Bash(git:*)" -> ("Bash", Some("git:*"))
    if let Some(paren_start) = s.find('(') {
        if s.ends_with(')') {
            let tool = s[..paren_start].to_string();
            let pattern = s[paren_start+1..s.len()-1].to_string();
            return (tool, Some(pattern));
        }
    }
    (s.to_string(), None)
}

/// Get context preview showing what will be in the agent's context window
#[tauri::command]
pub fn get_agent_context_preview(runtime: String) -> Result<ContextPreview, String> {
    let home = home_dir();
    let project = project_root();
    let mut sections = Vec::new();
    let mut total_tokens: u64 = 0;

    // System prompt (estimated)
    let system_tokens = 30000u64; // Approximate system prompt size
    sections.push(ContextPreviewSection {
        name: "System Prompt".to_string(),
        tokens: system_tokens,
        files: vec!["(built-in)".to_string()],
    });
    total_tokens += system_tokens;

    // Project config (CLAUDE.md, AGENTS.md, etc.)
    let project_config_files: Vec<PathBuf> = match runtime.as_str() {
        "claude" => vec![project.join("CLAUDE.md")],
        "codex" => vec![project.join("AGENTS.md")],
        "hermes" | "openclaw" => vec![project.join("SOUL.md")],
        _ => vec![],
    };

    let mut config_tokens: u64 = 0;
    let mut config_files = Vec::new();
    for path in project_config_files {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                config_tokens += estimate_tokens(content.len() as u64);
                config_files.push(path.to_string_lossy().to_string());
            }
        }
    }
    if config_tokens > 0 {
        sections.push(ContextPreviewSection {
            name: "Project Config".to_string(),
            tokens: config_tokens,
            files: config_files,
        });
        total_tokens += config_tokens;
    }

    // Note: Skills are on-demand, not counted in context total
    // But we can show them as "available" with their token counts

    let limit = match runtime.as_str() {
        "claude" => 200000u64,
        "codex" => 128000u64,
        "gemini" => 1000000u64, // Gemini 1.5/2.x have 1M-token windows
        "hermes" => 128000u64,
        "openclaw" => 128000u64,
        _ => 100000u64,
    };

    Ok(ContextPreview {
        total_tokens,
        limit,
        sections,
    })
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 1: Skill Health Check / Linter
// ══════════════════════════════════════════════════════════════════════════════

const VALID_TOOLS: &[&str] = &[
    "Bash", "Read", "Write", "Edit", "Glob", "Grep", "WebFetch", "WebSearch",
    "Task", "TodoWrite", "NotebookEdit", "AskUserQuestion", "Skill", "KillShell",
    "mcp", "computer", "text_editor", "browser", "code_execution"
];

/// Validate a single skill file
#[tauri::command]
pub fn validate_skill(path: String) -> Result<SkillValidation, String> {
    let path_buf = PathBuf::from(&path);

    if !path_buf.exists() {
        return Ok(SkillValidation {
            path: path.clone(),
            skill_name: None,
            valid: false,
            errors: vec![ValidationIssue {
                code: "FILE_NOT_FOUND".to_string(),
                severity: "error".to_string(),
                message: "File does not exist".to_string(),
                line: None,
                suggestion: Some("Create the file or check the path".to_string()),
            }],
            warnings: vec![],
            token_count: 0,
        });
    }

    let content = fs::read_to_string(&path_buf)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let token_count = estimate_tokens(content.len() as u64);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut skill_name: Option<String> = None;

    // Check if it's a SKILL.md or similar markdown file
    let is_skill_file = path.ends_with("SKILL.md") ||
                        path.ends_with("CLAUDE.md") ||
                        path.ends_with("AGENTS.md") ||
                        path.ends_with("SOUL.md");

    if is_skill_file {
        // Check for YAML frontmatter
        if content.starts_with("---") {
            let parts: Vec<&str> = content.splitn(3, "---").collect();
            if parts.len() >= 3 {
                let frontmatter = parts[1].trim();

                // Try to parse YAML
                match serde_yaml::from_str::<serde_json::Value>(frontmatter) {
                    Ok(yaml) => {
                        // Check for name field
                        if let Some(name) = yaml.get("name").and_then(|n| n.as_str()) {
                            skill_name = Some(name.to_string());
                        } else {
                            warnings.push(ValidationIssue {
                                code: "MISSING_NAME".to_string(),
                                severity: "warning".to_string(),
                                message: "Skill has no 'name' field in frontmatter".to_string(),
                                line: Some(2),
                                suggestion: Some("Add 'name: my-skill' to frontmatter".to_string()),
                            });
                        }

                        // Check for description field
                        if yaml.get("description").is_none() {
                            warnings.push(ValidationIssue {
                                code: "MISSING_DESCRIPTION".to_string(),
                                severity: "warning".to_string(),
                                message: "Skill has no description — agents may not understand when to use it".to_string(),
                                line: Some(2),
                                suggestion: Some("Add 'description: What this skill does' to frontmatter".to_string()),
                            });
                        }

                        // Validate allowed-tools
                        if let Some(tools) = yaml.get("allowed-tools").and_then(|t| t.as_array()) {
                            for tool in tools {
                                if let Some(tool_str) = tool.as_str() {
                                    // Extract tool name (before any parentheses for patterns)
                                    let tool_name = tool_str.split('(').next().unwrap_or(tool_str);
                                    if !VALID_TOOLS.contains(&tool_name) {
                                        errors.push(ValidationIssue {
                                            code: "INVALID_TOOL".to_string(),
                                            severity: "error".to_string(),
                                            message: format!("Unknown tool '{}' in allowed-tools", tool_name),
                                            line: None,
                                            suggestion: Some(format!("Valid tools: {}", VALID_TOOLS.join(", "))),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(ValidationIssue {
                            code: "INVALID_FRONTMATTER".to_string(),
                            severity: "error".to_string(),
                            message: format!("Frontmatter YAML parse error: {}", e),
                            line: Some(2),
                            suggestion: Some("Check YAML syntax in frontmatter".to_string()),
                        });
                    }
                }

                // Check for empty content body
                let body = parts[2].trim();
                if body.is_empty() {
                    warnings.push(ValidationIssue {
                        code: "EMPTY_CONTENT".to_string(),
                        severity: "warning".to_string(),
                        message: "Skill has frontmatter but no content body".to_string(),
                        line: None,
                        suggestion: Some("Add instructions after the frontmatter".to_string()),
                    });
                }
            } else {
                errors.push(ValidationIssue {
                    code: "INCOMPLETE_FRONTMATTER".to_string(),
                    severity: "error".to_string(),
                    message: "Frontmatter not properly closed with '---'".to_string(),
                    line: Some(1),
                    suggestion: Some("Add closing '---' after frontmatter".to_string()),
                });
            }
        } else if path.ends_with("SKILL.md") {
            errors.push(ValidationIssue {
                code: "MISSING_FRONTMATTER".to_string(),
                severity: "error".to_string(),
                message: "SKILL.md missing YAML frontmatter".to_string(),
                line: Some(1),
                suggestion: Some("Add frontmatter starting with '---' at the top".to_string()),
            });
        }
    }

    // Token size warnings
    if token_count > 15000 {
        errors.push(ValidationIssue {
            code: "TOKEN_SIZE_ERROR".to_string(),
            severity: "error".to_string(),
            message: format!("Skill is ~{} tokens — too large, will consume significant context", token_count),
            line: None,
            suggestion: Some("Split into smaller, focused skills".to_string()),
        });
    } else if token_count > 8000 {
        warnings.push(ValidationIssue {
            code: "TOKEN_SIZE_WARNING".to_string(),
            severity: "warning".to_string(),
            message: format!("Skill is ~{} tokens — consider splitting for better context efficiency", token_count),
            line: None,
            suggestion: Some("Large skills reduce available context for conversation".to_string()),
        });
    }

    let valid = errors.is_empty();

    Ok(SkillValidation {
        path,
        skill_name,
        valid,
        errors,
        warnings,
        token_count,
    })
}

/// Validate all skill files across all runtimes
#[tauri::command]
pub fn validate_all_skills() -> Result<Vec<SkillValidation>, String> {
    let home = home_dir();
    let mut validations = Vec::new();

    // Skill directories to scan
    let skill_dirs = vec![
        home.join(".claude/skills"),
        home.join(".codex/skills"),
        home.join(".agents/skills"),
        home.join(".hermes/skills"),
        home.join(".openclaw/skills"),
    ];

    for dir in skill_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let skill_md = entry.path().join("SKILL.md");
                    if skill_md.exists() {
                        if let Ok(validation) = validate_skill(skill_md.to_string_lossy().to_string()) {
                            validations.push(validation);
                        }
                    }
                }
            }
        }
    }

    // Also check project skills
    let project = project_root();
    let project_skill_dirs = vec![
        project.join(".claude/skills"),
        project.join(".agents/skills"),
        project.join("skills"),
    ];

    for dir in project_skill_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let skill_md = entry.path().join("SKILL.md");
                    if skill_md.exists() {
                        if let Ok(validation) = validate_skill(skill_md.to_string_lossy().to_string()) {
                            validations.push(validation);
                        }
                    }
                }
            }
        }
    }

    Ok(validations)
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 2: Onboarding Checklist
// ══════════════════════════════════════════════════════════════════════════════

/// Get onboarding status for a specific runtime
#[tauri::command]
pub fn get_onboarding_status(runtime: String) -> Result<OnboardingStatus, String> {
    let home = home_dir();
    let project = project_root();
    let mut items = Vec::new();

    match runtime.as_str() {
        "claude" => {
            // Check CLI installed
            let cli_installed = which_sync("claude").is_some();
            items.push(OnboardingItem {
                id: "cli_installed".to_string(),
                label: "Claude Code CLI installed".to_string(),
                completed: cli_installed,
                action: if cli_installed { None } else { Some(OnboardingAction {
                    action_type: "external_link".to_string(),
                    target: "https://docs.anthropic.com/en/docs/claude-code".to_string(),
                }) },
            });

            // Check authenticated
            let claude_json = home.join(".claude.json");
            let has_auth = claude_json.exists() && fs::read_to_string(&claude_json)
                .map(|c| c.contains("oauth") || c.contains("apiKey"))
                .unwrap_or(false);
            items.push(OnboardingItem {
                id: "authenticated".to_string(),
                label: "Authenticated (API key or OAuth)".to_string(),
                completed: has_auth,
                action: if has_auth { None } else { Some(OnboardingAction {
                    action_type: "run_command".to_string(),
                    target: "claude auth".to_string(),
                }) },
            });

            // Check settings.json exists
            let settings = home.join(".claude/settings.json");
            items.push(OnboardingItem {
                id: "settings_created".to_string(),
                label: "Created ~/.claude/settings.json".to_string(),
                completed: settings.exists(),
                action: if settings.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: settings.to_string_lossy().to_string(),
                }) },
            });

            // Check CLAUDE.md exists in project
            let claude_md = project.join("CLAUDE.md");
            items.push(OnboardingItem {
                id: "project_config".to_string(),
                label: "Created CLAUDE.md for project".to_string(),
                completed: claude_md.exists(),
                action: if claude_md.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: claude_md.to_string_lossy().to_string(),
                }) },
            });

            // Check at least one skill
            let skills_dir = home.join(".claude/skills");
            let has_skills = skills_dir.exists() && fs::read_dir(&skills_dir)
                .map(|entries| entries.count() > 0)
                .unwrap_or(false);
            items.push(OnboardingItem {
                id: "has_skill".to_string(),
                label: "Added at least one skill".to_string(),
                completed: has_skills,
                action: if has_skills { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: skills_dir.join("my-skill/SKILL.md").to_string_lossy().to_string(),
                }) },
            });
        }
        "codex" => {
            // Check CLI installed
            let cli_installed = which_sync("codex").is_some();
            items.push(OnboardingItem {
                id: "cli_installed".to_string(),
                label: "Codex CLI installed".to_string(),
                completed: cli_installed,
                action: if cli_installed { None } else { Some(OnboardingAction {
                    action_type: "external_link".to_string(),
                    target: "https://github.com/openai/codex".to_string(),
                }) },
            });

            // Check OPENAI_API_KEY
            let has_api_key = std::env::var("OPENAI_API_KEY").is_ok();
            items.push(OnboardingItem {
                id: "api_key".to_string(),
                label: "OPENAI_API_KEY environment variable set".to_string(),
                completed: has_api_key,
                action: if has_api_key { None } else { Some(OnboardingAction {
                    action_type: "external_link".to_string(),
                    target: "https://platform.openai.com/api-keys".to_string(),
                }) },
            });

            // Check config.toml
            let config = home.join(".codex/config.toml");
            items.push(OnboardingItem {
                id: "config_created".to_string(),
                label: "Created ~/.codex/config.toml".to_string(),
                completed: config.exists(),
                action: if config.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: config.to_string_lossy().to_string(),
                }) },
            });

            // Check AGENTS.md
            let agents_md = project.join("AGENTS.md");
            items.push(OnboardingItem {
                id: "project_config".to_string(),
                label: "Created AGENTS.md for project".to_string(),
                completed: agents_md.exists(),
                action: if agents_md.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: agents_md.to_string_lossy().to_string(),
                }) },
            });

            // Check skills
            let skills_dir = home.join(".agents/skills");
            let has_skills = skills_dir.exists() && fs::read_dir(&skills_dir)
                .map(|entries| entries.count() > 0)
                .unwrap_or(false);
            items.push(OnboardingItem {
                id: "has_skill".to_string(),
                label: "Added at least one skill".to_string(),
                completed: has_skills,
                action: if has_skills { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: skills_dir.join("my-skill/SKILL.md").to_string_lossy().to_string(),
                }) },
            });
        }
        "hermes" => {
            // Check CLI installed
            let cli_installed = which_sync("hermes").is_some();
            items.push(OnboardingItem {
                id: "cli_installed".to_string(),
                label: "Hermes installed".to_string(),
                completed: cli_installed,
                action: if cli_installed { None } else { Some(OnboardingAction {
                    action_type: "external_link".to_string(),
                    target: "https://github.com/hermes-ai/hermes".to_string(),
                }) },
            });

            // Check config.yaml
            let config = home.join(".hermes/config.yaml");
            items.push(OnboardingItem {
                id: "config_created".to_string(),
                label: "Created ~/.hermes/config.yaml".to_string(),
                completed: config.exists(),
                action: if config.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: config.to_string_lossy().to_string(),
                }) },
            });

            // Check SOUL.md
            let soul_md = project.join("SOUL.md");
            items.push(OnboardingItem {
                id: "soul_created".to_string(),
                label: "Created SOUL.md".to_string(),
                completed: soul_md.exists(),
                action: if soul_md.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: soul_md.to_string_lossy().to_string(),
                }) },
            });

            // Check memories directory
            let memories = home.join(".hermes/memories");
            items.push(OnboardingItem {
                id: "memories_setup".to_string(),
                label: "Set up memories/ directory".to_string(),
                completed: memories.exists(),
                action: if memories.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: memories.join("MEMORY.md").to_string_lossy().to_string(),
                }) },
            });
        }
        "openclaw" => {
            // Check gateway config
            let config = home.join(".openclaw/openclaw.json");
            let config_valid = config.exists() && fs::read_to_string(&config)
                .map(|c| c.contains("gateway"))
                .unwrap_or(false);
            items.push(OnboardingItem {
                id: "gateway_configured".to_string(),
                label: "OpenClaw gateway configured".to_string(),
                completed: config_valid,
                action: if config_valid { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: config.to_string_lossy().to_string(),
                }) },
            });

            // Check SOUL.md
            let soul_md = project.join("SOUL.md");
            items.push(OnboardingItem {
                id: "soul_created".to_string(),
                label: "Created workspace SOUL.md".to_string(),
                completed: soul_md.exists(),
                action: if soul_md.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: soul_md.to_string_lossy().to_string(),
                }) },
            });

            // Check TOOLS.md
            let tools_md = project.join("TOOLS.md");
            items.push(OnboardingItem {
                id: "tools_created".to_string(),
                label: "Added TOOLS.md".to_string(),
                completed: tools_md.exists(),
                action: if tools_md.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: tools_md.to_string_lossy().to_string(),
                }) },
            });

            // Check skills
            let skills_dir = home.join(".openclaw/skills");
            let has_skills = skills_dir.exists() && fs::read_dir(&skills_dir)
                .map(|entries| entries.count() > 0)
                .unwrap_or(false);
            items.push(OnboardingItem {
                id: "has_skill".to_string(),
                label: "Added at least one skill".to_string(),
                completed: has_skills,
                action: if has_skills { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: skills_dir.join("my-skill/SKILL.md").to_string_lossy().to_string(),
                }) },
            });
        }
        _ => {}
    }

    let completed_count = items.iter().filter(|i| i.completed).count();
    let total = items.len();
    let completion_percent = if total > 0 {
        ((completed_count as f32 / total as f32) * 100.0) as u8
    } else {
        0
    };

    Ok(OnboardingStatus {
        runtime,
        items,
        completion_percent,
    })
}

/// Helper to check if a command exists in PATH
pub fn which_sync(cmd: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .filter_map(|dir| {
                let full_path = dir.join(cmd);
                if full_path.is_file() {
                    Some(full_path)
                } else {
                    None
                }
            })
            .next()
    })
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 3: Profile Snapshots
// ══════════════════════════════════════════════════════════════════════════════

/// Save current configuration as a profile snapshot
#[tauri::command]
pub fn save_profile_snapshot(
    db: State<'_, DbState>,
    name: String,
    description: Option<String>,
    runtime: String,
) -> Result<String, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let home = home_dir();
    let project = project_root();
    let mut files: Vec<ProfileFile> = Vec::new();

    // Collect files based on runtime
    let global_paths: Vec<PathBuf> = match runtime.as_str() {
        "claude" => vec![
            home.join(".claude/settings.json"),
            home.join(".claude.json"),
            home.join(".claude/CLAUDE.md"),
        ],
        "codex" => vec![
            home.join(".codex/config.toml"),
            home.join(".codex/requirements.toml"),
        ],
        "hermes" => vec![
            home.join(".hermes/config.yaml"),
            home.join(".hermes/.env"),
        ],
        "openclaw" => vec![
            home.join(".openclaw/openclaw.json"),
        ],
        _ => vec![],
    };

    // Read global files
    for path in global_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                let relative = path.strip_prefix(&home)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                files.push(ProfileFile {
                    path: relative,
                    content,
                    scope: "global".to_string(),
                });
            }
        }
    }

    // Collect skills
    let skills_dir = match runtime.as_str() {
        "claude" => home.join(".claude/skills"),
        "codex" => home.join(".agents/skills"),
        "hermes" => home.join(".hermes/skills"),
        "openclaw" => home.join(".openclaw/skills"),
        _ => home.join(".claude/skills"),
    };

    if skills_dir.exists() {
        if let Ok(entries) = fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                let skill_md = entry.path().join("SKILL.md");
                if skill_md.exists() {
                    if let Ok(content) = fs::read_to_string(&skill_md) {
                        let relative = skill_md.strip_prefix(&home)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| skill_md.to_string_lossy().to_string());
                        files.push(ProfileFile {
                            path: relative,
                            content,
                            scope: "global".to_string(),
                        });
                    }
                }
            }
        }
    }

    // Project files
    let project_paths: Vec<PathBuf> = match runtime.as_str() {
        "claude" => vec![project.join("CLAUDE.md"), project.join(".claude/settings.json")],
        "codex" => vec![project.join("AGENTS.md")],
        "hermes" | "openclaw" => vec![project.join("SOUL.md"), project.join("TOOLS.md")],
        _ => vec![],
    };

    for path in project_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                let relative = path.strip_prefix(&project)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                files.push(ProfileFile {
                    path: relative,
                    content,
                    scope: "project".to_string(),
                });
            }
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let files_json = serde_json::to_string(&files).map_err(|e| e.to_string())?;
    let created_at = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO profile_snapshots (id, name, description, runtime, files_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, name, description, runtime, files_json, created_at],
    ).map_err(|e| e.to_string())?;

    Ok(id)
}

/// List all profile snapshots
#[tauri::command]
pub fn list_profile_snapshots(db: State<'_, DbState>) -> Result<Vec<ProfileSnapshot>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, name, description, runtime, files_json, created_at FROM profile_snapshots ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;

    let profiles = stmt.query_map([], |row| {
        let files_json: String = row.get(4)?;
        let files: Vec<ProfileFile> = serde_json::from_str(&files_json).unwrap_or_default();
        Ok(ProfileSnapshot {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            runtime: row.get(3)?,
            files,
            created_at: row.get(5)?,
        })
    }).map_err(|e| e.to_string())?;

    profiles.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Load a profile snapshot (writes files to disk)
#[tauri::command]
pub fn load_profile_snapshot(db: State<'_, DbState>, profile_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let home = home_dir();
    let project = project_root();

    let files_json: String = conn.query_row(
        "SELECT files_json FROM profile_snapshots WHERE id = ?1",
        params![profile_id],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;

    let files: Vec<ProfileFile> = serde_json::from_str(&files_json).map_err(|e| e.to_string())?;

    for file in files {
        let full_path = if file.scope == "global" {
            home.join(&file.path)
        } else {
            project.join(&file.path)
        };

        // Create parent directories
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        // Write file
        fs::write(&full_path, &file.content).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Delete a profile snapshot
#[tauri::command]
pub fn delete_profile_snapshot(db: State<'_, DbState>, profile_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM profile_snapshots WHERE id = ?1",
        params![profile_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Export a profile snapshot as JSON
#[tauri::command]
pub fn export_profile_snapshot(db: State<'_, DbState>, profile_id: String) -> Result<String, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let profile: ProfileSnapshot = conn.query_row(
        "SELECT id, name, description, runtime, files_json, created_at FROM profile_snapshots WHERE id = ?1",
        params![profile_id],
        |row| {
            let files_json: String = row.get(4)?;
            let files: Vec<ProfileFile> = serde_json::from_str(&files_json).unwrap_or_default();
            Ok(ProfileSnapshot {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                runtime: row.get(3)?,
                files,
                created_at: row.get(5)?,
            })
        },
    ).map_err(|e| e.to_string())?;

    serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 4: Skill Usage Analytics
// ══════════════════════════════════════════════════════════════════════════════

/// Get usage statistics for all skills
#[tauri::command]
pub fn get_skill_usage_stats() -> Result<Vec<SkillUsageStat>, String> {
    let home = home_dir();
    let logs_path = home.join(".ato/agent-logs.jsonl");
    let mut usage_map: std::collections::HashMap<String, (u32, Option<String>, Vec<u32>)> = std::collections::HashMap::new();

    // Parse agent logs for skill invocations
    if logs_path.exists() {
        if let Ok(content) = fs::read_to_string(&logs_path) {
            for line in content.lines() {
                if let Ok(log) = serde_json::from_str::<serde_json::Value>(line) {
                    // Look for skill invocations in the logs
                    if let Some(skill_name) = log.get("skill").and_then(|s| s.as_str()) {
                        let timestamp = log.get("timestamp")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string());
                        let tokens = log.get("tokens")
                            .and_then(|t| t.as_u64())
                            .map(|t| t as u32)
                            .unwrap_or(0);

                        let entry = usage_map.entry(skill_name.to_string()).or_insert((0, None, Vec::new()));
                        entry.0 += 1;
                        entry.1 = timestamp.or(entry.1.clone());
                        if tokens > 0 {
                            entry.2.push(tokens);
                        }
                    }

                    // Also check for skill references in prompt content
                    if let Some(prompt) = log.get("prompt").and_then(|p| p.as_str()) {
                        // Simple heuristic: look for /skill-name patterns
                        for word in prompt.split_whitespace() {
                            if word.starts_with('/') && word.len() > 1 {
                                let skill_name = word.trim_start_matches('/');
                                let entry = usage_map.entry(skill_name.to_string()).or_insert((0, None, Vec::new()));
                                entry.0 += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    // Build list of all known skills
    let mut all_skills: Vec<SkillUsageStat> = Vec::new();
    let skill_dirs = vec![
        (home.join(".claude/skills"), "claude"),
        (home.join(".agents/skills"), "codex"),
        (home.join(".hermes/skills"), "hermes"),
        (home.join(".openclaw/skills"), "openclaw"),
    ];

    for (dir, _runtime) in skill_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let skill_name = entry.file_name().to_string_lossy().to_string();
                        let skill_path = entry.path().join("SKILL.md").to_string_lossy().to_string();

                        let (trigger_count, last_used, tokens_vec) = usage_map
                            .get(&skill_name)
                            .cloned()
                            .unwrap_or((0, None, Vec::new()));

                        let avg_tokens = if tokens_vec.is_empty() {
                            None
                        } else {
                            Some((tokens_vec.iter().sum::<u32>() / tokens_vec.len() as u32) as u32)
                        };

                        all_skills.push(SkillUsageStat {
                            skill_path,
                            skill_name,
                            trigger_count,
                            last_used,
                            avg_tokens,
                        });
                    }
                }
            }
        }
    }

    // Sort by trigger count (most used first)
    all_skills.sort_by(|a, b| b.trigger_count.cmp(&a.trigger_count));

    Ok(all_skills)
}

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 6: Project Manager
// ══════════════════════════════════════════════════════════════════════════════

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
                    let skill_count = count_project_skills(&path);

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

/// Count skills in a project directory
pub fn count_project_skills(project_path: &PathBuf) -> u32 {
    let mut count = 0u32;

    let skill_dirs = vec![
        project_path.join(".claude/skills"),
        project_path.join(".codex/skills"),
        project_path.join(".agents/skills"),
        project_path.join(".hermes/skills"),
        project_path.join("skills"),
    ];

    for dir in skill_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() && entry.path().join("SKILL.md").exists() {
                        count += 1;
                    }
                }
            }
        }
    }

    count
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

// ── Project Bundle (all-in-one view for Projects dashboard) ─────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileRef {
    pub label: String,
    pub path: String,
    pub scope: String,        // "user" | "project" | "nested"
    pub exists: bool,
    pub size_bytes: u64,
    pub token_estimate: u64,
    pub last_modified: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHookSummary {
    pub event: String,
    pub matcher: Option<String>,
    pub command: String,
    pub scope: String,   // "user" | "project"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMcpSummary {
    pub name: String,
    pub kind: String,        // "stdio" | "http" | "sse" | "unknown"
    pub command_or_url: String,
    pub scope: String,       // "user" | "project"
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPermissions {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub ask: Vec<String>,
    pub scope: String,       // "user" | "project" | "merged"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBundle {
    pub project_path: String,
    pub project_name: String,
    pub has_claude: bool,
    pub has_codex: bool,
    pub has_hermes: bool,
    pub has_openclaw: bool,
    pub has_gemini: bool,

    pub memory_files: Vec<ProjectFileRef>,     // CLAUDE.md hierarchy (user, project, nested)
    pub subagents: Vec<ProjectFileRef>,         // .claude/agents/*.md (global + project)
    pub commands: Vec<ProjectFileRef>,          // .claude/commands/*.md (global + project)
    pub settings_files: Vec<ProjectFileRef>,    // settings.json, settings.local.json, .mcp.json

    pub skills: Vec<LocalSkill>,                // Filtered to this project + inherited globals
    pub hooks: Vec<ProjectHookSummary>,
    pub permissions_user: ProjectPermissions,
    pub permissions_project: ProjectPermissions,
    pub mcp_servers: Vec<ProjectMcpSummary>,

    // Per-runtime file bundles for Codex / OpenClaw / Hermes
    pub codex_files: Vec<ProjectFileRef>,       // AGENTS.md (user+project), config.toml (user+project)
    pub codex_skills: Vec<LocalSkill>,
    pub openclaw_files: Vec<ProjectFileRef>,    // SOUL.md, TOOLS.md, workspace AGENTS.md, openclaw.json
    pub openclaw_skills: Vec<LocalSkill>,
    pub hermes_files: Vec<ProjectFileRef>,      // SOUL.md, memories/MEMORY.md, memories/USER.md, config.yaml
    pub hermes_skills: Vec<LocalSkill>,

    // Gemini CLI / ADK
    pub gemini_files: Vec<ProjectFileRef>,     // GEMINI.md, settings.json, root_agent.yaml
    pub gemini_skills: Vec<LocalSkill>,

    // OpenAI Agents SDK (extends Codex)
    pub sandbox_config: Option<SandboxConfig>,
    pub approval_policies: Vec<ApprovalPolicy>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SandboxConfig {
    pub enabled: bool,
    pub network_isolation: bool,
    pub allowed_ports: Vec<u16>,
    pub filesystem_policy: String,
    pub timeout_secs: Option<u64>,
    pub snapshot_enabled: bool,
    pub source_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalPolicy {
    pub tool_name: String,
    pub policy: String,
    pub scope: String,
}

pub fn file_ref(label: &str, path: PathBuf, scope: &str) -> ProjectFileRef {
    let metadata = fs::metadata(&path).ok();
    let exists = metadata.is_some();
    let size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
    let last_modified = metadata
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    ProjectFileRef {
        label: label.to_string(),
        path: path.to_string_lossy().to_string(),
        scope: scope.to_string(),
        exists,
        size_bytes,
        token_estimate: estimate_tokens(size_bytes),
        last_modified,
    }
}

pub fn list_nested_claude_md(project_path: &PathBuf, max_depth: u32) -> Vec<ProjectFileRef> {
    let mut out = Vec::new();
    fn walk(dir: &PathBuf, root: &PathBuf, depth: u32, max_depth: u32, out: &mut Vec<ProjectFileRef>) {
        if depth > max_depth {
            return;
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist" {
                    continue;
                }
                if p.is_dir() {
                    walk(&p, root, depth + 1, max_depth, out);
                } else if name == "CLAUDE.md" && depth > 0 {
                    let rel = p.strip_prefix(root).map(|r| r.to_string_lossy().to_string())
                        .unwrap_or_else(|_| p.to_string_lossy().to_string());
                    out.push(file_ref(&rel, p.clone(), "nested"));
                }
            }
        }
    }
    walk(project_path, project_path, 0, max_depth, &mut out);
    out
}

pub fn list_dir_md_files(dir: &PathBuf, scope: &str) -> Vec<ProjectFileRef> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    if ext == "md" {
                        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("unnamed").to_string();
                        out.push(file_ref(&name, p, scope));
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    out
}

pub fn parse_permissions_from_settings(path: &PathBuf, scope: &str) -> ProjectPermissions {
    let mut out = ProjectPermissions { scope: scope.to_string(), ..Default::default() };
    if !path.exists() {
        return out;
    }
    let Ok(text) = fs::read_to_string(path) else { return out };
    let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else { return out };
    if let Some(perms) = value.get("permissions") {
        for (key, dest) in [("allow", &mut out.allow), ("deny", &mut out.deny), ("ask", &mut out.ask)] {
            if let Some(arr) = perms.get(key).and_then(|v| v.as_array()) {
                *dest = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            }
        }
    }
    out
}

pub fn parse_mcp_from_settings(path: &PathBuf, scope: &str) -> Vec<ProjectMcpSummary> {
    let mut out = Vec::new();
    if !path.exists() {
        return out;
    }
    let Ok(text) = fs::read_to_string(path) else { return out };
    let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else { return out };
    // .mcp.json keys servers at root under "mcpServers" OR is the object itself. Support both.
    let servers_obj = value.get("mcpServers").cloned().unwrap_or(value.clone());
    if let Some(map) = servers_obj.as_object() {
        for (name, cfg) in map {
            let (kind, command_or_url) = if let Some(cmd) = cfg.get("command").and_then(|v| v.as_str()) {
                ("stdio", cmd.to_string())
            } else if let Some(url) = cfg.get("url").and_then(|v| v.as_str()) {
                let kind = if cfg.get("type").and_then(|v| v.as_str()) == Some("sse") { "sse" } else { "http" };
                (kind, url.to_string())
            } else {
                ("unknown", String::new())
            };
            out.push(ProjectMcpSummary {
                name: name.clone(),
                kind: kind.to_string(),
                command_or_url,
                scope: scope.to_string(),
            });
        }
    }
    out
}

pub fn collect_hooks_from_settings(path: &PathBuf, scope: &str) -> Vec<ProjectHookSummary> {
    let mut out = Vec::new();
    if !path.exists() {
        return out;
    }
    let Ok(text) = fs::read_to_string(path) else { return out };
    let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else { return out };
    let Some(hooks) = value.get("hooks").and_then(|v| v.as_object()) else { return out };
    for (event, triggers) in hooks {
        if let Some(arr) = triggers.as_array() {
            for trigger in arr {
                let matcher = trigger.get("matcher").and_then(|v| v.as_str()).map(String::from);
                if let Some(hook_arr) = trigger.get("hooks").and_then(|v| v.as_array()) {
                    for h in hook_arr {
                        if let Some(cmd) = h.get("command").and_then(|v| v.as_str()) {
                            out.push(ProjectHookSummary {
                                event: event.clone(),
                                matcher: matcher.clone(),
                                command: cmd.to_string(),
                                scope: scope.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

pub fn parse_sandbox_config(project_path: &PathBuf) -> Option<SandboxConfig> {
    // Look for sandbox config in config.toml or codex.json
    let candidates = [
        project_path.join(".codex").join("sandbox.json"),
        project_path.join("codex.json"),
    ];
    for path in &candidates {
        if !path.exists() { continue; }
        let Ok(text) = fs::read_to_string(path) else { continue };
        let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else { continue };
        let sandbox = value.get("sandbox").unwrap_or(&value);
        if sandbox.is_object() {
            return Some(SandboxConfig {
                enabled: sandbox.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                network_isolation: sandbox.get("network_isolation")
                    .or_else(|| sandbox.get("networkIsolation"))
                    .and_then(|v| v.as_bool()).unwrap_or(false),
                allowed_ports: sandbox.get("allowed_ports")
                    .or_else(|| sandbox.get("allowedPorts"))
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as u16)).collect())
                    .unwrap_or_default(),
                filesystem_policy: sandbox.get("filesystem_policy")
                    .or_else(|| sandbox.get("filesystemPolicy"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("read-write").to_string(),
                timeout_secs: sandbox.get("timeout_secs")
                    .or_else(|| sandbox.get("timeoutSecs"))
                    .and_then(|v| v.as_u64()),
                snapshot_enabled: sandbox.get("snapshot_enabled")
                    .or_else(|| sandbox.get("snapshotEnabled"))
                    .and_then(|v| v.as_bool()).unwrap_or(false),
                source_path: path.to_string_lossy().to_string(),
            });
        }
    }
    // Also check config.toml for [sandbox] section
    let toml_path = project_path.join(".codex").join("config.toml");
    if toml_path.exists() {
        if let Ok(text) = fs::read_to_string(&toml_path) {
            let parsed = parse_toml_to_json(&text);
            if let Some(sandbox) = parsed.get("sandbox") {
                return Some(SandboxConfig {
                    enabled: sandbox.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                    network_isolation: sandbox.get("network_isolation").and_then(|v| v.as_bool()).unwrap_or(false),
                    allowed_ports: Vec::new(),
                    filesystem_policy: sandbox.get("filesystem_policy").and_then(|v| v.as_str()).unwrap_or("read-write").to_string(),
                    timeout_secs: sandbox.get("timeout_secs").and_then(|v| v.as_u64()),
                    snapshot_enabled: sandbox.get("snapshot_enabled").and_then(|v| v.as_bool()).unwrap_or(false),
                    source_path: toml_path.to_string_lossy().to_string(),
                });
            }
        }
    }
    None
}

pub fn parse_approval_policies(project_path: &PathBuf) -> Vec<ApprovalPolicy> {
    let mut out = Vec::new();
    let candidates = [
        (project_path.join(".codex").join("policies.json"), "project"),
        (home_dir().join(".codex").join("policies.json"), "user"),
        (project_path.join("codex.json"), "project"),
    ];
    for (path, scope) in &candidates {
        if !path.exists() { continue; }
        let Ok(text) = fs::read_to_string(path) else { continue };
        let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else { continue };
        let policies = value.get("approval_policies")
            .or_else(|| value.get("approvalPolicies"))
            .or_else(|| value.get("policies"));
        if let Some(policies) = policies.and_then(|v| v.as_object()) {
            for (tool, policy_val) in policies {
                let policy_str = policy_val.as_str()
                    .unwrap_or_else(|| policy_val.get("level").and_then(|v| v.as_str()).unwrap_or("on-request"));
                out.push(ApprovalPolicy {
                    tool_name: tool.clone(),
                    policy: policy_str.to_string(),
                    scope: scope.to_string(),
                });
            }
        }
    }
    out
}

/// Write sandbox config to .codex/sandbox.json via the safe write pipeline.
#[tauri::command]
pub fn write_sandbox_config(
    db: State<'_, DbState>,
    project_path: String,
    config: SandboxConfig,
) -> Result<WriteResult, String> {
    let dest = PathBuf::from(&project_path).join(".codex").join("sandbox.json");
    let content = serde_json::to_string_pretty(&json!({
        "sandbox": {
            "enabled": config.enabled,
            "network_isolation": config.network_isolation,
            "filesystem_policy": config.filesystem_policy,
            "timeout_secs": config.timeout_secs,
            "snapshot_enabled": config.snapshot_enabled,
            "allowed_ports": config.allowed_ports,
        }
    })).unwrap_or_default();
    write_agent_config_file(db, dest.to_string_lossy().to_string(), content + "\n", None, Some(true))
}

/// Write approval policies to .codex/policies.json via the safe write pipeline.
#[tauri::command]
pub fn write_approval_policies(
    db: State<'_, DbState>,
    project_path: String,
    policies: Vec<ApprovalPolicy>,
) -> Result<WriteResult, String> {
    let dest = PathBuf::from(&project_path).join(".codex").join("policies.json");
    let mut map = serde_json::Map::new();
    for p in &policies {
        map.insert(p.tool_name.clone(), serde_json::Value::String(p.policy.clone()));
    }
    let content = serde_json::to_string_pretty(&json!({
        "approvalPolicies": serde_json::Value::Object(map)
    })).unwrap_or_default();
    write_agent_config_file(db, dest.to_string_lossy().to_string(), content + "\n", None, Some(true))
}

/// Write a TOML config file from JSON value via the safe write pipeline.
#[tauri::command]
pub fn write_toml_config(
    db: State<'_, DbState>,
    path: String,
    value: serde_json::Value,
) -> Result<WriteResult, String> {
    let content = json_to_toml(&value)?;
    write_agent_config_file(db, path, content, None, Some(true))
}

// ── OpenClaw Workspace Parsing ──────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawWorkspace {
    pub soul: OpenClawSoul,
    pub tools: Vec<OpenClawTool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawSoul {
    pub name: Option<String>,
    pub role: Option<String>,
    pub traits: Vec<String>,
    pub raw_content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawTool {
    pub name: String,
    pub description: String,
}

#[tauri::command]
pub fn parse_openclaw_workspace(project_path: String) -> Result<OpenClawWorkspace, String> {
    let pb = PathBuf::from(&project_path);
    let openclaw_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));

    // SOUL.md — check project then global
    let soul_path = if pb.join("SOUL.md").exists() { pb.join("SOUL.md") }
        else { openclaw_home.join("workspace").join("SOUL.md") };
    let soul_raw = read_file_lossy(&soul_path).unwrap_or_default();
    let mut soul = OpenClawSoul { name: None, role: None, traits: Vec::new(), raw_content: soul_raw.clone() };
    // Parse frontmatter or first heading
    for line in soul_raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") { soul.name = Some(trimmed[2..].trim().to_string()); }
        if trimmed.to_lowercase().starts_with("role:") { soul.role = Some(trimmed[5..].trim().to_string()); }
        if trimmed.starts_with("- ") && soul.name.is_some() { soul.traits.push(trimmed[2..].trim().to_string()); }
    }

    // TOOLS.md — parse ## headings as tool names
    let tools_path = if pb.join("TOOLS.md").exists() { pb.join("TOOLS.md") }
        else { openclaw_home.join("workspace").join("TOOLS.md") };
    let tools_raw = read_file_lossy(&tools_path).unwrap_or_default();
    let mut tools = Vec::new();
    let mut current_tool: Option<String> = None;
    let mut current_desc = String::new();
    for line in tools_raw.lines() {
        if line.starts_with("## ") {
            if let Some(name) = current_tool.take() {
                tools.push(OpenClawTool { name, description: current_desc.trim().to_string() });
            }
            current_tool = Some(line[3..].trim().to_string());
            current_desc = String::new();
        } else if current_tool.is_some() {
            current_desc.push_str(line.trim());
            current_desc.push(' ');
        }
    }
    if let Some(name) = current_tool {
        tools.push(OpenClawTool { name, description: current_desc.trim().to_string() });
    }

    Ok(OpenClawWorkspace { soul, tools })
}

// ── Gemini Agent YAML Parsing ──────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiAgentDef {
    pub name: Option<String>,
    pub model: Option<String>,
    pub instruction: Option<String>,
    pub sub_agents: Vec<GeminiSubAgent>,
    pub tools: Vec<GeminiToolRef>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiSubAgent {
    pub name: String,
    pub model: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiToolRef {
    pub name: String,
    pub kind: Option<String>,
}

#[tauri::command]
pub fn parse_gemini_agent(path: String) -> Result<GeminiAgentDef, String> {
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read: {}", e))?;
    let value: serde_yaml::Value = serde_yaml::from_str(&content)
        .map_err(|e| format!("Invalid YAML: {}", e))?;

    let name = value.get("name").and_then(|v| v.as_str()).map(String::from);
    let model = value.get("model").and_then(|v| v.as_str()).map(String::from);
    let instruction = value.get("instruction").and_then(|v| v.as_str()).map(|s| {
        if s.len() > 200 { format!("{}…", &s[..200]) } else { s.to_string() }
    });

    let sub_agents = value.get("sub_agents")
        .or_else(|| value.get("subAgents"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|a| {
            let name = a.get("name").or_else(|| a.get("agent")).and_then(|v| v.as_str())?;
            Some(GeminiSubAgent {
                name: name.to_string(),
                model: a.get("model").and_then(|v| v.as_str()).map(String::from),
                description: a.get("description").and_then(|v| v.as_str()).map(String::from),
            })
        }).collect())
        .unwrap_or_default();

    let tools = value.get("tools")
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|t| {
            if let Some(s) = t.as_str() {
                return Some(GeminiToolRef { name: s.to_string(), kind: None });
            }
            let name = t.get("name").and_then(|v| v.as_str())?;
            let kind = t.get("type").and_then(|v| v.as_str()).map(String::from);
            Some(GeminiToolRef { name: name.to_string(), kind })
        }).collect())
        .unwrap_or_default();

    Ok(GeminiAgentDef { name, model, instruction, sub_agents, tools })
}

/// Full per-project bundle: memory hierarchy, skills, subagents, commands, hooks, permissions, MCP.
/// Claude Code-first; other runtimes in Batch 3.
#[tauri::command]
pub fn get_project_bundle(
    db: State<'_, DbState>,
    project_path: String,
) -> Result<ProjectBundle, String> {
    let project_pb = PathBuf::from(&project_path);
    if !project_pb.exists() {
        return Err(format!("Project path does not exist: {}", project_path));
    }
    let project_name = project_pb.file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .unwrap_or_else(|| project_path.clone());

    let home = home_dir();

    // Runtime detection (same logic as list_projects)
    let has_claude = project_pb.join(".claude").exists() || project_pb.join("CLAUDE.md").exists();
    let has_codex = project_pb.join(".codex").exists() || project_pb.join("AGENTS.md").exists();
    let has_hermes = project_pb.join(".hermes").exists() || project_pb.join("SOUL.md").exists();
    let has_openclaw = project_pb.join("SOUL.md").exists() && project_pb.join("TOOLS.md").exists();
    let has_gemini = project_pb.join(".gemini").exists() || project_pb.join("GEMINI.md").exists();

    // Memory files: user CLAUDE.md, project CLAUDE.md, nested CLAUDE.md
    let mut memory_files = Vec::new();
    memory_files.push(file_ref("~/.claude/CLAUDE.md", home.join(".claude").join("CLAUDE.md"), "user"));
    memory_files.push(file_ref("CLAUDE.md", project_pb.join("CLAUDE.md"), "project"));
    memory_files.extend(list_nested_claude_md(&project_pb, 4));

    // Subagents
    let mut subagents = Vec::new();
    subagents.extend(list_dir_md_files(&home.join(".claude").join("agents"), "user"));
    subagents.extend(list_dir_md_files(&project_pb.join(".claude").join("agents"), "project"));

    // Commands
    let mut commands = Vec::new();
    commands.extend(list_dir_md_files(&home.join(".claude").join("commands"), "user"));
    commands.extend(list_dir_md_files(&project_pb.join(".claude").join("commands"), "project"));

    // Settings files
    let user_settings = home.join(".claude").join("settings.json");
    let user_settings_local = home.join(".claude").join("settings.local.json");
    let project_settings = project_pb.join(".claude").join("settings.json");
    let project_settings_local = project_pb.join(".claude").join("settings.local.json");
    let project_mcp = project_pb.join(".mcp.json");

    let mut settings_files = Vec::new();
    settings_files.push(file_ref("~/.claude/settings.json", user_settings.clone(), "user"));
    settings_files.push(file_ref("~/.claude/settings.local.json", user_settings_local, "user"));
    settings_files.push(file_ref(".claude/settings.json", project_settings.clone(), "project"));
    settings_files.push(file_ref(".claude/settings.local.json", project_settings_local, "project"));
    settings_files.push(file_ref(".mcp.json", project_mcp.clone(), "project"));

    // Skills (global Claude + project Claude)
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut skills = Vec::new();
    skills.extend(collect_skills_for_project(
        &home.join(".claude").join("skills"), "personal", "claude", None, &conn,
    ));
    skills.extend(collect_skills_for_project(
        &project_pb.join(".claude").join("skills"), "project", "claude",
        Some(&project_name),
        &conn,
    ));
    drop(conn);

    // Hooks from settings.json (user + project)
    let mut hooks = Vec::new();
    hooks.extend(collect_hooks_from_settings(&user_settings, "user"));
    hooks.extend(collect_hooks_from_settings(&project_settings, "project"));

    // Permissions (user + project, separate)
    let permissions_user = parse_permissions_from_settings(&user_settings, "user");
    let permissions_project = parse_permissions_from_settings(&project_settings, "project");

    // MCP: from user settings.json .mcpServers + project .mcp.json
    let mut mcp_servers = Vec::new();
    mcp_servers.extend(parse_mcp_from_settings(&user_settings, "user"));
    mcp_servers.extend(parse_mcp_from_settings(&project_mcp, "project"));

    // ── Codex ────────────────────────────────────────────────────────────
    let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
        .unwrap_or_else(|_| home.join(".codex").to_string_lossy().to_string()));
    let mut codex_files = Vec::new();
    codex_files.push(file_ref("~/.codex/AGENTS.md", codex_home.join("AGENTS.md"), "user"));
    codex_files.push(file_ref("~/.codex/config.toml", codex_home.join("config.toml"), "user"));
    codex_files.push(file_ref("AGENTS.md", project_pb.join("AGENTS.md"), "project"));
    codex_files.push(file_ref(".codex/config.toml", project_pb.join(".codex").join("config.toml"), "project"));

    let conn2 = db.0.lock().map_err(|e| e.to_string())?;
    let mut codex_skills = Vec::new();
    codex_skills.extend(collect_skills_for_project(
        &codex_home.join("skills"), "personal", "codex", None, &conn2,
    ));
    codex_skills.extend(collect_skills_for_project(
        &home.join(".agents").join("skills"), "personal", "codex", None, &conn2,
    ));
    codex_skills.extend(collect_skills_for_project(
        &project_pb.join(".codex").join("skills"), "project", "codex",
        Some(&project_name), &conn2,
    ));
    codex_skills.extend(collect_skills_for_project(
        &project_pb.join(".agents").join("skills"), "project", "codex",
        Some(&project_name), &conn2,
    ));

    // ── OpenClaw ─────────────────────────────────────────────────────────
    let openclaw_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home.join(".openclaw").to_string_lossy().to_string()));
    let openclaw_workspace = openclaw_home.join("workspace");
    let mut openclaw_files = Vec::new();
    openclaw_files.push(file_ref("~/.openclaw/openclaw.json", openclaw_home.join("openclaw.json"), "user"));
    openclaw_files.push(file_ref("~/.openclaw/workspace/SOUL.md", openclaw_workspace.join("SOUL.md"), "user"));
    openclaw_files.push(file_ref("~/.openclaw/workspace/TOOLS.md", openclaw_workspace.join("TOOLS.md"), "user"));
    openclaw_files.push(file_ref("~/.openclaw/workspace/AGENTS.md", openclaw_workspace.join("AGENTS.md"), "user"));
    openclaw_files.push(file_ref("SOUL.md", project_pb.join("SOUL.md"), "project"));
    openclaw_files.push(file_ref("TOOLS.md", project_pb.join("TOOLS.md"), "project"));

    let mut openclaw_skills = Vec::new();
    openclaw_skills.extend(collect_skills_for_project(
        &openclaw_home.join("skills"), "personal", "openclaw", None, &conn2,
    ));
    openclaw_skills.extend(collect_skills_for_project(
        &project_pb.join(".openclaw").join("skills"), "project", "openclaw",
        Some(&project_name), &conn2,
    ));
    openclaw_skills.extend(collect_skills_for_project(
        &project_pb.join("skills"), "project", "openclaw",
        Some(&project_name), &conn2,
    ));

    // ── Hermes ───────────────────────────────────────────────────────────
    let hermes_home = home.join(".hermes");
    let mut hermes_files = Vec::new();
    hermes_files.push(file_ref("~/.hermes/SOUL.md", hermes_home.join("SOUL.md"), "user"));
    hermes_files.push(file_ref("~/.hermes/config.yaml", hermes_home.join("config.yaml"), "user"));
    hermes_files.push(file_ref("~/.hermes/memories/MEMORY.md", hermes_home.join("memories").join("MEMORY.md"), "user"));
    hermes_files.push(file_ref("~/.hermes/memories/USER.md", hermes_home.join("memories").join("USER.md"), "user"));
    // Scan for additional memory files beyond MEMORY.md and USER.md
    let memories_dir = hermes_home.join("memories");
    if memories_dir.exists() {
        if let Ok(entries) = fs::read_dir(&memories_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    if p.is_file() && name.ends_with(".md") && name != "MEMORY.md" && name != "USER.md" {
                        hermes_files.push(file_ref(
                            &format!("~/.hermes/memories/{}", name),
                            p, "user",
                        ));
                    }
                }
            }
        }
    }

    let mut hermes_skills = Vec::new();
    hermes_skills.extend(collect_skills_for_project(
        &hermes_home.join("skills"), "personal", "hermes", None, &conn2,
    ));
    hermes_skills.extend(collect_skills_for_project(
        &project_pb.join(".hermes").join("skills"), "project", "hermes",
        Some(&project_name), &conn2,
    ));

    // ── Gemini CLI / ADK ─────────────────────────────────────────────────
    let gemini_hm = gemini_home();
    let mut gemini_files = Vec::new();
    gemini_files.push(file_ref("~/.gemini/GEMINI.md", gemini_hm.join("GEMINI.md"), "user"));
    gemini_files.push(file_ref("~/.gemini/settings.json", gemini_hm.join("settings.json"), "user"));
    gemini_files.push(file_ref("GEMINI.md", project_pb.join("GEMINI.md"), "project"));
    gemini_files.push(file_ref(".gemini/settings.json", project_pb.join(".gemini").join("settings.json"), "project"));
    gemini_files.push(file_ref("root_agent.yaml", project_pb.join("root_agent.yaml"), "project"));

    // Gemini skills/agents (not yet a convention — check .gemini/agents/ if present)
    let mut gemini_skills = Vec::new();
    gemini_skills.extend(collect_skills_for_project(
        &project_pb.join(".gemini").join("agents"), "project", "gemini",
        Some(&project_name), &conn2,
    ));

    drop(conn2);

    // ── OpenAI Agents SDK (enriches Codex) ───────────────────────────────
    let sandbox_config = if has_codex { parse_sandbox_config(&project_pb) } else { None };
    let approval_policies = if has_codex { parse_approval_policies(&project_pb) } else { Vec::new() };

    Ok(ProjectBundle {
        project_path,
        project_name,
        has_claude,
        has_codex,
        has_hermes,
        has_openclaw,
        has_gemini,
        memory_files,
        subagents,
        commands,
        settings_files,
        skills,
        hooks,
        permissions_user,
        permissions_project,
        mcp_servers,
        codex_files,
        codex_skills,
        openclaw_files,
        openclaw_skills,
        hermes_files,
        hermes_skills,
        gemini_files,
        gemini_skills,
        sandbox_config,
        approval_policies,
    })
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
    let skill_count = count_project_skills(&path_buf);
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

/// Get skills for a specific project
#[tauri::command]
pub fn get_project_skills(project_path: String) -> Result<Vec<LocalSkill>, String> {
    let path_buf = PathBuf::from(&project_path);
    let project_name = path_buf.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let mut skills = Vec::new();

    let skill_dirs = vec![
        (path_buf.join(".claude/skills"), "claude"),
        (path_buf.join(".codex/skills"), "codex"),
        (path_buf.join(".agents/skills"), "codex"),
        (path_buf.join(".hermes/skills"), "hermes"),
        (path_buf.join("skills"), "shared"),
    ];

    for (dir, runtime) in skill_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let skill_path = entry.path();
                    if skill_path.is_dir() {
                        let skill_md = skill_path.join("SKILL.md");
                        if skill_md.exists() {
                            if let Ok(content) = fs::read_to_string(&skill_md) {
                                let (fm, _body) = parse_frontmatter(&content);
                                let name = fm.get("name")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());
                                let description = fm.get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let token_count = estimate_tokens(content.len() as u64);
                                let hash = content_hash(&content);

                                skills.push(LocalSkill {
                                    id: format!("{}:{}", runtime, skill_md.to_string_lossy()),
                                    name,
                                    description,
                                    file_path: skill_md.to_string_lossy().to_string(),
                                    scope: "project".to_string(),
                                    runtime: runtime.to_string(),
                                    project: Some(project_name.clone()),
                                    token_count,
                                    enabled: true,
                                    content_hash: hash,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(skills)
}

/// Clone a skill from one project to another
#[tauri::command]
pub fn clone_skill(
    source_skill_path: String,
    target_project_path: String,
    target_runtime: String,
) -> Result<String, String> {
    let source_path = PathBuf::from(&source_skill_path);
    let target_project = PathBuf::from(&target_project_path);

    if !source_path.exists() {
        return Err("Source skill does not exist".to_string());
    }

    // Read source skill content
    let content = fs::read_to_string(&source_path)
        .map_err(|e| format!("Failed to read source skill: {}", e))?;

    // Determine target skills directory
    let target_skills_dir = match target_runtime.as_str() {
        "claude" => target_project.join(".claude/skills"),
        "codex" => target_project.join(".agents/skills"),
        "hermes" => target_project.join(".hermes/skills"),
        "openclaw" => target_project.join("skills"),
        _ => target_project.join(".claude/skills"),
    };

    // Get skill name from source path
    let skill_name = source_path.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "cloned-skill".to_string());

    // Create target directory
    let target_skill_dir = target_skills_dir.join(&skill_name);
    fs::create_dir_all(&target_skill_dir)
        .map_err(|e| format!("Failed to create target directory: {}", e))?;

    // Write skill file
    let target_skill_path = target_skill_dir.join("SKILL.md");
    fs::write(&target_skill_path, &content)
        .map_err(|e| format!("Failed to write skill: {}", e))?;

    Ok(target_skill_path.to_string_lossy().to_string())
}

/// Refresh skill count for a project
#[tauri::command]
pub fn refresh_project_skills(db: State<'_, DbState>, project_id: String) -> Result<u32, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Get project path
    let path: String = conn.query_row(
        "SELECT path FROM projects WHERE id = ?1",
        params![project_id],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;

    let skill_count = count_project_skills(&PathBuf::from(&path));

    // Update in database
    conn.execute(
        "UPDATE projects SET skill_count = ?1 WHERE id = ?2",
        params![skill_count, project_id],
    ).map_err(|e| e.to_string())?;

    Ok(skill_count)
}

// ── Secrets Manager ──────────────────────────────────────────────────────

const KEYCHAIN_SERVICE: &str = "ato-desktop";

/// List all secrets (metadata only, not values)
#[tauri::command]
pub fn list_secrets(db: State<'_, DbState>) -> Result<Vec<Secret>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, name, key_type, runtime, project_id, created_at, updated_at FROM secrets ORDER BY name"
    ).map_err(|e| e.to_string())?;

    let secrets = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;

        // Check if value exists in keychain
        let has_value = keyring::Entry::new(KEYCHAIN_SERVICE, &id)
            .map(|e| e.get_password().is_ok())
            .unwrap_or(false);

        Ok(Secret {
            id,
            name,
            key_type: row.get(2)?,
            runtime: row.get(3)?,
            project_id: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            has_value,
        })
    }).map_err(|e| e.to_string())?;

    secrets.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Create or update a secret
#[tauri::command]
pub fn save_secret(
    db: State<'_, DbState>,
    name: String,
    key_type: String,
    value: String,
    runtime: Option<String>,
    project_id: Option<String>,
) -> Result<Secret, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();

    // Store value in OS keychain
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &id)
        .map_err(|e| format!("Failed to create keychain entry: {}", e))?;
    entry.set_password(&value)
        .map_err(|e| format!("Failed to store secret in keychain: {}", e))?;

    // Store metadata in database
    conn.execute(
        "INSERT INTO secrets (id, name, key_type, runtime, project_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, name, key_type, runtime, project_id, now, now],
    ).map_err(|e| e.to_string())?;

    Ok(Secret {
        id,
        name,
        key_type,
        runtime,
        project_id,
        created_at: now.clone(),
        updated_at: now,
        has_value: true,
    })
}

/// Get a secret value (requires explicit user action)
#[tauri::command]
pub fn get_secret_value(secret_id: String) -> Result<String, String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &secret_id)
        .map_err(|e| format!("Failed to access keychain: {}", e))?;
    entry.get_password()
        .map_err(|e| format!("Failed to retrieve secret: {}", e))
}

/// Update a secret value
#[tauri::command]
pub fn update_secret(
    db: State<'_, DbState>,
    secret_id: String,
    name: Option<String>,
    value: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();

    // Update value in keychain if provided
    if let Some(new_value) = value {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &secret_id)
            .map_err(|e| format!("Failed to access keychain: {}", e))?;
        entry.set_password(&new_value)
            .map_err(|e| format!("Failed to update secret: {}", e))?;
    }

    // Update metadata if name changed
    if let Some(new_name) = name {
        conn.execute(
            "UPDATE secrets SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![new_name, now, secret_id],
        ).map_err(|e| e.to_string())?;
    } else {
        conn.execute(
            "UPDATE secrets SET updated_at = ?1 WHERE id = ?2",
            params![now, secret_id],
        ).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Delete a secret
#[tauri::command]
pub fn delete_secret(db: State<'_, DbState>, secret_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Remove from keychain
    if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, &secret_id) {
        let _ = entry.delete_password();
    }

    // Remove from database
    conn.execute("DELETE FROM secrets WHERE id = ?1", params![secret_id])
        .map_err(|e| e.to_string())?;

    Ok(())
}

// ── Environment Variables Manager ────────────────────────────────────────

/// List environment variables
#[tauri::command]
pub fn list_env_vars(db: State<'_, DbState>, project_id: Option<String>, runtime: Option<String>) -> Result<Vec<EnvVar>, String> {
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
        format!("SELECT id, project_id, runtime, key, value, created_at FROM env_vars WHERE {} ORDER BY key", conditions.join(" AND "))
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

    let env_vars = stmt.query_map(params_vec.as_slice(), |row| {
        Ok(EnvVar {
            id: row.get(0)?,
            project_id: row.get(1)?,
            runtime: row.get(2)?,
            key: row.get(3)?,
            value: row.get(4)?,
            created_at: row.get(5)?,
        })
    }).map_err(|e| e.to_string())?;

    env_vars.collect::<Result<Vec<_>, _>>().map_err(|e: rusqlite::Error| e.to_string())
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
pub fn update_env_var(db: State<'_, DbState>, env_id: String, key: Option<String>, value: Option<String>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    if let Some(new_key) = key {
        conn.execute("UPDATE env_vars SET key = ?1 WHERE id = ?2", params![new_key, env_id])
            .map_err(|e| e.to_string())?;
    }

    if let Some(new_value) = value {
        conn.execute("UPDATE env_vars SET value = ?1 WHERE id = ?2", params![new_value, env_id])
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

/// Import environment variables from a .env file
#[tauri::command]
pub fn import_env_file(db: State<'_, DbState>, file_path: String, project_id: Option<String>, runtime: Option<String>) -> Result<Vec<EnvVar>, String> {
    let content = fs::read_to_string(&file_path)
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

// ── Model Configuration ──────────────────────────────────────────────────

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

// ── Execution Logs ───────────────────────────────────────────────────────

/// Get execution logs with filtering
#[tauri::command]
pub fn get_execution_logs(
    db: State<'_, DbState>,
    runtime: Option<String>,
    status: Option<String>,
    limit: Option<i32>,
) -> Result<Vec<ExecutionLog>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(100);

    let sql = match (&runtime, &status) {
        (Some(_), Some(_)) => "SELECT id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, created_at FROM execution_logs WHERE runtime = ?1 AND status = ?2 ORDER BY created_at DESC LIMIT ?3",
        (Some(_), None) => "SELECT id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, created_at FROM execution_logs WHERE runtime = ?1 ORDER BY created_at DESC LIMIT ?2",
        (None, Some(_)) => "SELECT id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, created_at FROM execution_logs WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2",
        (None, None) => "SELECT id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, created_at FROM execution_logs ORDER BY created_at DESC LIMIT ?1",
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let logs = match (&runtime, &status) {
        (Some(rt), Some(st)) => stmt.query_map(params![rt, st, limit], map_execution_log),
        (Some(rt), None) => stmt.query_map(params![rt, limit], map_execution_log),
        (None, Some(st)) => stmt.query_map(params![st, limit], map_execution_log),
        (None, None) => stmt.query_map(params![limit], map_execution_log),
    }.map_err(|e| e.to_string())?;

    logs.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn map_execution_log(row: &rusqlite::Row) -> Result<ExecutionLog, rusqlite::Error> {
    Ok(ExecutionLog {
        id: row.get(0)?,
        runtime: row.get(1)?,
        prompt: row.get(2)?,
        response: row.get(3)?,
        tokens_in: row.get(4)?,
        tokens_out: row.get(5)?,
        duration_ms: row.get(6)?,
        status: row.get(7)?,
        error_message: row.get(8)?,
        skill_name: row.get(9)?,
        created_at: row.get(10)?,
    })
}

/// Add an execution log entry
#[tauri::command]
pub fn add_execution_log(
    db: State<'_, DbState>,
    runtime: String,
    prompt: Option<String>,
    response: Option<String>,
    tokens_in: Option<i32>,
    tokens_out: Option<i32>,
    duration_ms: Option<i32>,
    status: String,
    error_message: Option<String>,
    skill_name: Option<String>,
) -> Result<ExecutionLog, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO execution_logs (id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, now],
    ).map_err(|e| e.to_string())?;

    Ok(ExecutionLog {
        id,
        runtime,
        prompt,
        response,
        tokens_in,
        tokens_out,
        duration_ms,
        status,
        error_message,
        skill_name,
        created_at: now,
    })
}

// ── Health Checks ────────────────────────────────────────────────────────

/// Get health status for all runtimes
#[tauri::command]
pub fn get_health_status(db: State<'_, DbState>) -> Result<Vec<RuntimeHealth>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let runtimes = vec!["claude", "codex", "gemini", "hermes", "openclaw"];
    let mut health_list = Vec::new();

    for runtime in runtimes {
        // Get latest health check
        let latest: Option<HealthCheck> = conn.query_row(
            "SELECT id, runtime, status, latency_ms, error_message, checked_at FROM health_checks WHERE runtime = ?1 ORDER BY checked_at DESC LIMIT 1",
            params![runtime],
            |row| {
                Ok(HealthCheck {
                    id: row.get(0)?,
                    runtime: row.get(1)?,
                    status: row.get(2)?,
                    latency_ms: row.get(3)?,
                    error_message: row.get(4)?,
                    checked_at: row.get(5)?,
                })
            },
        ).ok();

        // Calculate uptime (last 24 hours)
        let uptime: Option<f64> = conn.query_row(
            "SELECT CAST(SUM(CASE WHEN status = 'healthy' THEN 1 ELSE 0 END) AS REAL) / COUNT(*) * 100 FROM health_checks WHERE runtime = ?1 AND checked_at > datetime('now', '-24 hours')",
            params![runtime],
            |row| row.get(0),
        ).ok().flatten();

        health_list.push(RuntimeHealth {
            runtime: runtime.to_string(),
            status: latest.as_ref().map(|h| h.status.clone()).unwrap_or_else(|| "unknown".to_string()),
            latency_ms: latest.as_ref().and_then(|h| h.latency_ms),
            uptime_percent: uptime,
            last_check: latest.as_ref().map(|h| h.checked_at.clone()),
            error_message: latest.and_then(|h| h.error_message),
        });
    }

    Ok(health_list)
}

/// Record a health check
#[tauri::command]
pub fn record_health_check(
    db: State<'_, DbState>,
    runtime: String,
    status: String,
    latency_ms: Option<i32>,
    error_message: Option<String>,
) -> Result<HealthCheck, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO health_checks (id, runtime, status, latency_ms, error_message, checked_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, runtime, status, latency_ms, error_message, now],
    ).map_err(|e| e.to_string())?;

    // Clean up old health checks (keep last 7 days)
    conn.execute(
        "DELETE FROM health_checks WHERE checked_at < datetime('now', '-7 days')",
        [],
    ).ok();

    Ok(HealthCheck {
        id,
        runtime,
        status,
        latency_ms,
        error_message,
        checked_at: now,
    })
}

// ── Phase 2: Real-time Monitoring Commands ─────────────────────────────────

/// Start the log file watcher for real-time updates
#[tauri::command]
pub fn start_log_watcher(
    app: tauri::AppHandle,
    watcher_state: State<'_, LogWatcherState>,
) -> Result<bool, String> {
    let mut watcher = watcher_state.0.lock().map_err(|e| e.to_string())?;
    watcher.start(app)?;
    Ok(true)
}

/// Stop the log file watcher
#[tauri::command]
pub fn stop_log_watcher(watcher_state: State<'_, LogWatcherState>) -> Result<bool, String> {
    let mut watcher = watcher_state.0.lock().map_err(|e| e.to_string())?;
    watcher.stop();
    Ok(true)
}

/// Check if log watcher is running
#[tauri::command]
pub fn is_log_watcher_running(watcher_state: State<'_, LogWatcherState>) -> Result<bool, String> {
    let watcher = watcher_state.0.lock().map_err(|e| e.to_string())?;
    Ok(watcher.is_watching())
}

/// Start the background health poller
#[tauri::command]
pub fn start_health_poller(
    app: tauri::AppHandle,
    poller_state: State<'_, HealthPollerState>,
) -> Result<bool, String> {
    let poller = poller_state.0.lock().map_err(|e| e.to_string())?;
    let db_path = get_db_path().to_string_lossy().to_string();
    poller.start(app, db_path);
    Ok(true)
}

/// Stop the background health poller
#[tauri::command]
pub fn stop_health_poller(poller_state: State<'_, HealthPollerState>) -> Result<bool, String> {
    let poller = poller_state.0.lock().map_err(|e| e.to_string())?;
    poller.stop();
    Ok(true)
}

/// Check if health poller is running
#[tauri::command]
pub fn is_health_poller_running(poller_state: State<'_, HealthPollerState>) -> Result<bool, String> {
    let poller = poller_state.0.lock().map_err(|e| e.to_string())?;
    Ok(poller.is_running())
}

/// Get health check history for charts (last 24 hours)
#[tauri::command]
pub fn get_health_history(
    db: State<'_, DbState>,
    runtime: Option<String>,
    hours: Option<i32>,
) -> Result<Vec<RuntimeHealthHistory>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let hours = hours.unwrap_or(24);
    let interval = format!("-{} hours", hours);

    let runtimes: Vec<String> = if let Some(rt) = runtime {
        vec![rt]
    } else {
        vec!["claude".to_string(), "codex".to_string(), "hermes".to_string(), "openclaw".to_string()]
    };

    let mut results = Vec::new();

    for rt in runtimes {
        // Get data points
        let mut stmt = conn.prepare(
            "SELECT checked_at, latency_ms, status FROM health_checks
             WHERE runtime = ?1 AND checked_at > datetime('now', ?2)
             ORDER BY checked_at ASC"
        ).map_err(|e| e.to_string())?;

        let data_points: Vec<HealthHistoryPoint> = stmt
            .query_map(params![&rt, &interval], |row| {
                Ok(HealthHistoryPoint {
                    timestamp: row.get(0)?,
                    latency_ms: row.get(1)?,
                    status: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        // Calculate stats
        let total_checks = data_points.len() as i32;
        let healthy_checks = data_points.iter().filter(|p| p.status == "healthy").count() as f64;
        let uptime_percent = if total_checks > 0 {
            (healthy_checks / total_checks as f64) * 100.0
        } else {
            0.0
        };

        let latencies: Vec<i32> = data_points.iter().filter_map(|p| p.latency_ms).collect();
        let avg_latency_ms = if !latencies.is_empty() {
            Some(latencies.iter().sum::<i32>() as f64 / latencies.len() as f64)
        } else {
            None
        };

        results.push(RuntimeHealthHistory {
            runtime: rt,
            data_points,
            avg_latency_ms,
            uptime_percent,
            total_checks,
        });
    }

    Ok(results)
}

/// Get aggregated usage metrics
#[tauri::command]
pub fn get_usage_metrics(
    db: State<'_, DbState>,
    days: Option<i32>,
) -> Result<UsageMetrics, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let days = days.unwrap_or(30);
    let interval = format!("-{} days", days);

    // Total counts
    let (total, successful, failed): (i64, i64, i64) = conn.query_row(
        "SELECT
            COUNT(*),
            SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END),
            SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END)
         FROM execution_logs
         WHERE created_at > datetime('now', ?1)",
        params![&interval],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap_or((0, 0, 0));

    // Token counts and avg duration
    let (tokens_in, tokens_out, avg_duration): (i64, i64, Option<f64>) = conn.query_row(
        "SELECT
            COALESCE(SUM(tokens_in), 0),
            COALESCE(SUM(tokens_out), 0),
            AVG(duration_ms)
         FROM execution_logs
         WHERE created_at > datetime('now', ?1)",
        params![&interval],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap_or((0, 0, None));

    // Executions by runtime
    let mut stmt = conn.prepare(
        "SELECT runtime,
                COUNT(*),
                SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END)
         FROM execution_logs
         WHERE created_at > datetime('now', ?1)
         GROUP BY runtime"
    ).map_err(|e| e.to_string())?;

    let executions_by_runtime: Vec<RuntimeExecutionCount> = stmt
        .query_map(params![&interval], |row| {
            Ok(RuntimeExecutionCount {
                runtime: row.get(0)?,
                count: row.get(1)?,
                success_count: row.get(2)?,
                error_count: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Executions by day
    let mut stmt = conn.prepare(
        "SELECT DATE(created_at),
                COUNT(*),
                SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END)
         FROM execution_logs
         WHERE created_at > datetime('now', ?1)
         GROUP BY DATE(created_at)
         ORDER BY DATE(created_at) ASC"
    ).map_err(|e| e.to_string())?;

    let executions_by_day: Vec<DailyExecutionCount> = stmt
        .query_map(params![&interval], |row| {
            Ok(DailyExecutionCount {
                date: row.get(0)?,
                count: row.get(1)?,
                success_count: row.get(2)?,
                error_count: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(UsageMetrics {
        total_executions: total,
        successful_executions: successful,
        failed_executions: failed,
        total_tokens_in: tokens_in,
        total_tokens_out: tokens_out,
        avg_duration_ms: avg_duration,
        executions_by_runtime,
        executions_by_day,
    })
}

// ── v0.8.0: Workflow Webhooks & Templates ─────────────────────────────────

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

// ── v0.5.5: Notifications Service ─────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NotificationChannel {
    pub id: String,
    pub provider: String,  // slack, discord, telegram, email
    pub name: String,
    pub config: serde_json::Value,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created_at: String,
    pub last_sent_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SendNotificationRequest {
    pub event_type: String,
    pub title: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NotificationResult {
    pub channel_id: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Save a notification channel configuration
#[tauri::command]
pub fn save_notification_channel(
    state: State<DbState>,
    channel: NotificationChannel,
) -> Result<NotificationChannel, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;

    // Ensure table exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS notification_channels (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            name TEXT NOT NULL,
            config TEXT NOT NULL,
            events TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_sent_at TEXT
        )",
        [],
    ).map_err(|e| e.to_string())?;

    let config_json = serde_json::to_string(&channel.config).map_err(|e| e.to_string())?;
    let events_json = serde_json::to_string(&channel.events).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT OR REPLACE INTO notification_channels (id, provider, name, config, events, enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            &channel.id,
            &channel.provider,
            &channel.name,
            &config_json,
            &events_json,
            if channel.enabled { 1 } else { 0 },
            &channel.created_at,
        ],
    ).map_err(|e| e.to_string())?;

    Ok(channel)
}

/// List all notification channels
#[tauri::command]
pub fn list_notification_channels(state: State<DbState>) -> Result<Vec<NotificationChannel>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;

    // Ensure table exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS notification_channels (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            name TEXT NOT NULL,
            config TEXT NOT NULL,
            events TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_sent_at TEXT
        )",
        [],
    ).map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, provider, name, config, events, enabled, created_at, last_sent_at
         FROM notification_channels
         ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;

    let channels = stmt
        .query_map([], |row| {
            let config_str: String = row.get(3)?;
            let events_str: String = row.get(4)?;
            Ok(NotificationChannel {
                id: row.get(0)?,
                provider: row.get(1)?,
                name: row.get(2)?,
                config: serde_json::from_str(&config_str).unwrap_or(serde_json::json!({})),
                events: serde_json::from_str(&events_str).unwrap_or(vec![]),
                enabled: row.get::<_, i32>(5)? == 1,
                created_at: row.get(6)?,
                last_sent_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(channels)
}

/// Delete a notification channel
#[tauri::command]
pub fn delete_notification_channel(state: State<DbState>, channel_id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM notification_channels WHERE id = ?1",
        params![&channel_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Toggle notification channel enabled state
#[tauri::command]
pub fn toggle_notification_channel(state: State<DbState>, channel_id: String, enabled: bool) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE notification_channels SET enabled = ?1 WHERE id = ?2",
        params![if enabled { 1 } else { 0 }, &channel_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Send a notification to all enabled channels that match the event type
#[tauri::command]
pub async fn send_notification(
    state: State<'_, DbState>,
    request: SendNotificationRequest,
) -> Result<Vec<NotificationResult>, String> {
    let channels = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;

        let mut stmt = conn.prepare(
            "SELECT id, provider, name, config, events, enabled, created_at, last_sent_at
             FROM notification_channels
             WHERE enabled = 1"
        ).map_err(|e| e.to_string())?;

        let channels: Vec<NotificationChannel> = stmt
            .query_map([], |row| {
                let config_str: String = row.get(3)?;
                let events_str: String = row.get(4)?;
                Ok(NotificationChannel {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    name: row.get(2)?,
                    config: serde_json::from_str(&config_str).unwrap_or(serde_json::json!({})),
                    events: serde_json::from_str(&events_str).unwrap_or(vec![]),
                    enabled: true,
                    created_at: row.get(6)?,
                    last_sent_at: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        channels
    };

    let mut results = Vec::new();

    for channel in channels {
        // Check if channel is subscribed to this event type
        if !channel.events.contains(&request.event_type) {
            continue;
        }

        let result = match channel.provider.as_str() {
            "slack" => send_slack_notification(&channel, &request).await,
            "discord" => send_discord_notification(&channel, &request).await,
            "telegram" => send_telegram_notification(&channel, &request).await,
            "email" => send_email_notification(&channel, &request).await,
            _ => Err(format!("Unknown provider: {}", channel.provider)),
        };

        let notification_result = NotificationResult {
            channel_id: channel.id.clone(),
            success: result.is_ok(),
            error: result.err(),
        };

        // Update last_sent_at if successful
        if notification_result.success {
            let conn = state.0.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE notification_channels SET last_sent_at = datetime('now') WHERE id = ?1",
                params![&channel.id],
            ).ok();
        }

        results.push(notification_result);
    }

    Ok(results)
}

/// Send Slack webhook notification
pub async fn send_slack_notification(
    channel: &NotificationChannel,
    request: &SendNotificationRequest,
) -> Result<(), String> {
    let webhook_url = channel.config.get("webhookUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing webhookUrl in Slack config".to_string())?;

    let payload = serde_json::json!({
        "text": format!("*{}*\n{}", request.title, request.message),
        "blocks": [
            {
                "type": "header",
                "text": { "type": "plain_text", "text": &request.title }
            },
            {
                "type": "section",
                "text": { "type": "mrkdwn", "text": &request.message }
            }
        ]
    });

    let client = reqwest::Client::new();
    let response = client
        .post(webhook_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send Slack notification: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Slack API error {}: {}", status, body));
    }

    Ok(())
}

/// Send Discord webhook notification
pub async fn send_discord_notification(
    channel: &NotificationChannel,
    request: &SendNotificationRequest,
) -> Result<(), String> {
    let webhook_url = channel.config.get("webhookUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing webhookUrl in Discord config".to_string())?;

    let payload = serde_json::json!({
        "embeds": [{
            "title": &request.title,
            "description": &request.message,
            "color": 5814783  // ATO accent color
        }]
    });

    let client = reqwest::Client::new();
    let response = client
        .post(webhook_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send Discord notification: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Discord API error {}: {}", status, body));
    }

    Ok(())
}

/// Send Telegram bot notification
pub async fn send_telegram_notification(
    channel: &NotificationChannel,
    request: &SendNotificationRequest,
) -> Result<(), String> {
    let bot_token = channel.config.get("botToken")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing botToken in Telegram config".to_string())?;

    let chat_id = channel.config.get("chatId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing chatId in Telegram config".to_string())?;

    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    let text = format!("*{}*\n\n{}", request.title, request.message);

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .form(&[
            ("chat_id", chat_id),
            ("text", &text),
            ("parse_mode", "Markdown"),
        ])
        .send()
        .await
        .map_err(|e| format!("Failed to send Telegram notification: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Telegram API error {}: {}", status, body));
    }

    Ok(())
}

/// Send email notification via SMTP
pub async fn send_email_notification(
    channel: &NotificationChannel,
    request: &SendNotificationRequest,
) -> Result<(), String> {
    // Extract SMTP configuration
    let host = channel.config.get("host")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing SMTP host in Email config".to_string())?;

    let port = channel.config.get("port")
        .map(|v| {
            // Handle both number and string values
            v.as_u64().unwrap_or_else(|| {
                v.as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(587)
            })
        })
        .unwrap_or(587) as u16;

    let username = channel.config.get("authUser")
        .or_else(|| channel.config.get("username"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing SMTP username in Email config".to_string())?;

    let password = channel.config.get("authPass")
        .or_else(|| channel.config.get("password"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing SMTP password in Email config".to_string())?;

    let from_email = channel.config.get("from")
        .and_then(|v| v.as_str())
        .unwrap_or(username);

    let from_name = channel.config.get("fromName")
        .and_then(|v| v.as_str())
        .unwrap_or("ATO Notifications");

    let to_email = channel.config.get("to")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing 'to' address in Email config".to_string())?;

    let use_tls = channel.config.get("useTls")
        .map(|v| {
            // Handle both boolean and string values
            v.as_bool().unwrap_or_else(|| {
                v.as_str().map(|s| s == "true").unwrap_or(true)
            })
        })
        .unwrap_or(true);

    // Build HTML email body
    let html_body = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0a0a0f; color: #e5e5e5; padding: 20px; }}
        .container {{ max-width: 600px; margin: 0 auto; background: #111116; border-radius: 8px; padding: 24px; border: 1px solid #222; }}
        .header {{ color: #00FFB2; font-size: 24px; font-weight: 600; margin-bottom: 16px; }}
        .event-badge {{ display: inline-block; background: #00FFB2; color: #0a0a0f; padding: 4px 12px; border-radius: 4px; font-size: 12px; font-weight: 600; margin-bottom: 16px; }}
        .content {{ color: #b3b3b3; line-height: 1.6; }}
        .footer {{ margin-top: 24px; padding-top: 16px; border-top: 1px solid #222; color: #666; font-size: 12px; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="event-badge">{}</div>
        <div class="header">{}</div>
        <div class="content">{}</div>
        <div class="footer">Sent by ATO (Agentic Tool Optimization)</div>
    </div>
</body>
</html>"#,
        request.event_type.to_uppercase(),
        request.title,
        request.message.replace("\n", "<br>")
    );

    // Parse email addresses
    let from_mailbox: Mailbox = format!("{} <{}>", from_name, from_email)
        .parse()
        .map_err(|e| format!("Invalid 'from' email address: {}", e))?;

    let to_mailbox: Mailbox = to_email
        .parse()
        .map_err(|e| format!("Invalid 'to' email address: {}", e))?;

    // Build the email message
    let email = Message::builder()
        .from(from_mailbox)
        .to(to_mailbox)
        .subject(format!("[ATO] {}", request.title))
        .header(ContentType::TEXT_HTML)
        .body(html_body)
        .map_err(|e| format!("Failed to build email: {}", e))?;

    // Build SMTP transport with credentials
    let creds = Credentials::new(username.to_string(), password.to_string());

    let mailer = if use_tls {
        SmtpTransport::starttls_relay(host)
            .map_err(|e| format!("Failed to create SMTP transport: {}", e))?
            .port(port)
            .credentials(creds)
            .build()
    } else {
        SmtpTransport::builder_dangerous(host)
            .port(port)
            .credentials(creds)
            .build()
    };

    // Send the email
    mailer.send(&email)
        .map_err(|e| format!("Failed to send email: {}", e))?;

    Ok(())
}

/// Test a notification channel configuration
#[tauri::command]
pub async fn test_notification_channel(channel: NotificationChannel) -> Result<NotificationResult, String> {
    let test_request = SendNotificationRequest {
        event_type: "test".to_string(),
        title: "Test Notification".to_string(),
        message: format!("This is a test notification from ATO to verify your {} configuration.", channel.provider),
        data: None,
    };

    let result = match channel.provider.as_str() {
        "slack" => send_slack_notification(&channel, &test_request).await,
        "discord" => send_discord_notification(&channel, &test_request).await,
        "telegram" => send_telegram_notification(&channel, &test_request).await,
        "email" => send_email_notification(&channel, &test_request).await,
        _ => Err(format!("Unknown provider: {}", channel.provider)),
    };

    Ok(NotificationResult {
        channel_id: channel.id,
        success: result.is_ok(),
        error: result.err(),
    })
}

// ── Telemetry Commands ───────────────────────────────────────────────────

use telemetry::{TelemetryState, TelemetryEvent, TelemetrySettings};

/// Get telemetry settings
#[tauri::command]
pub fn get_telemetry_settings(
    state: State<'_, TelemetryState>,
) -> Result<TelemetrySettings, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

/// Update telemetry settings
#[tauri::command]
pub fn update_telemetry_settings(
    state: State<'_, TelemetryState>,
    enabled: bool,
    endpoint: Option<String>,
) -> Result<TelemetrySettings, String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.enabled = enabled;
    settings.endpoint = endpoint;

    // Persist to config file
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ato");
    let _ = std::fs::create_dir_all(&config_dir);
    let settings_path = config_dir.join("telemetry.json");
    let _ = std::fs::write(&settings_path, serde_json::to_string_pretty(&*settings).unwrap_or_default());

    Ok(settings.clone())
}

/// Track a telemetry event
#[tauri::command]
pub async fn track_event(
    state: State<'_, TelemetryState>,
    event_type: String,
    properties: std::collections::HashMap<String, serde_json::Value>,
) -> Result<(), String> {
    // Extract all needed data from the lock, then drop it before any .await
    let (enabled, device_id, endpoint) = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        (settings.enabled, settings.device_id.clone(), settings.endpoint.clone())
    };

    if !enabled {
        return Ok(());
    }

    let event = TelemetryEvent {
        event_type,
        properties,
        timestamp: chrono::Utc::now().to_rfc3339(),
        session_id: state.session_id.clone(),
        device_id,
    };

    if let Some(endpoint) = endpoint {
        state.client
            .post(&endpoint)
            .json(&event)
            .send()
            .await
            .map_err(|e| e.to_string())?;
    } else {
        let mut queue = state.events_queue.lock().map_err(|e| e.to_string())?;
        queue.push(event);

        if queue.len() > 1000 {
            queue.drain(0..500);
        }
    }

    Ok(())
}

/// Get queued telemetry events (for debugging/export)
#[tauri::command]
pub fn get_queued_events(
    state: State<'_, TelemetryState>,
) -> Result<Vec<TelemetryEvent>, String> {
    let queue = state.events_queue.lock().map_err(|e| e.to_string())?;
    Ok(queue.clone())
}

/// Export telemetry events to JSON file
#[tauri::command]
pub fn export_telemetry_events(
    state: State<'_, TelemetryState>,
    path: String,
) -> Result<usize, String> {
    let queue = state.events_queue.lock().map_err(|e| e.to_string())?;
    let count = queue.len();

    let json = serde_json::to_string_pretty(&*queue)
        .map_err(|e| format!("Failed to serialize events: {}", e))?;

    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(count)
}

/// Get aggregated usage statistics for analytics dashboard
#[tauri::command]
pub fn get_analytics_summary(
    db: State<'_, DbState>,
) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Get skill counts
    let skill_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM skills",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    // Get workflow counts
    let workflow_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workflows",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    // Get notification channel counts
    let channel_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM notification_channels WHERE enabled = 1",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    // Get cron job counts
    let cron_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cron_jobs WHERE enabled = 1",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    // Get recent execution counts (last 7 days)
    let recent_executions: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cron_executions WHERE executed_at > datetime('now', '-7 days')",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    Ok(json!({
        "skills": skill_count,
        "workflows": workflow_count,
        "notificationChannels": channel_count,
        "cronJobs": cron_count,
        "recentExecutions": recent_executions,
        "sessionId": uuid::Uuid::new_v4().to_string(),
        "generatedAt": chrono::Utc::now().to_rfc3339()
    }))
}

// ── Audit Logging Commands ──────────────────────────────────────────────

#[tauri::command]
pub fn add_audit_log(
    db: State<'_, DbState>,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    resource_name: Option<String>,
    details: Option<String>,
) -> Result<AuditLogEntry, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO audit_logs (id, action, resource_type, resource_id, resource_name, details, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, action, resource_type, resource_id, resource_name, details, now],
    ).map_err(|e| e.to_string())?;

    Ok(AuditLogEntry {
        id, action, resource_type, resource_id, resource_name, details, created_at: now,
    })
}

#[tauri::command]
pub fn get_audit_logs(
    db: State<'_, DbState>,
    action: Option<String>,
    resource_type: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<AuditLogEntry>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(100);
    let offset = offset.unwrap_or(0);

    let mut sql = String::from(
        "SELECT id, action, resource_type, resource_id, resource_name, details, created_at
         FROM audit_logs WHERE 1=1"
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref a) = action {
        sql.push_str(&format!(" AND action = ?{}", param_idx));
        param_values.push(Box::new(a.clone()));
        param_idx += 1;
    }
    if let Some(ref rt) = resource_type {
        sql.push_str(&format!(" AND resource_type = ?{}", param_idx));
        param_values.push(Box::new(rt.clone()));
        param_idx += 1;
    }

    sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}", param_idx, param_idx + 1));
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(AuditLogEntry {
            id: row.get(0)?,
            action: row.get(1)?,
            resource_type: row.get(2)?,
            resource_id: row.get(3)?,
            resource_name: row.get(4)?,
            details: row.get(5)?,
            created_at: row.get(6)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut logs = Vec::new();
    for row in rows {
        logs.push(row.map_err(|e| e.to_string())?);
    }
    Ok(logs)
}

#[tauri::command]
pub fn get_audit_log_stats(
    db: State<'_, DbState>,
) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let total: i64 = conn.query_row("SELECT COUNT(*) FROM audit_logs", [], |row| row.get(0)).unwrap_or(0);
    let today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM audit_logs WHERE created_at > datetime('now', '-1 day')", [], |row| row.get(0)
    ).unwrap_or(0);
    let this_week: i64 = conn.query_row(
        "SELECT COUNT(*) FROM audit_logs WHERE created_at > datetime('now', '-7 days')", [], |row| row.get(0)
    ).unwrap_or(0);

    let mut stmt = conn.prepare(
        "SELECT action, COUNT(*) as cnt FROM audit_logs GROUP BY action ORDER BY cnt DESC LIMIT 10"
    ).map_err(|e| e.to_string())?;
    let top_actions: Vec<serde_json::Value> = stmt.query_map([], |row| {
        Ok(json!({ "action": row.get::<_, String>(0)?, "count": row.get::<_, i64>(1)? }))
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    Ok(json!({ "total": total, "today": today, "thisWeek": this_week, "topActions": top_actions }))
}

#[tauri::command]
pub fn clear_audit_logs(
    db: State<'_, DbState>,
    before_date: Option<String>,
) -> Result<u64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let deleted = if let Some(date) = before_date {
        conn.execute("DELETE FROM audit_logs WHERE created_at < ?1", params![date])
    } else {
        conn.execute("DELETE FROM audit_logs", [])
    }.map_err(|e| e.to_string())?;
    Ok(deleted as u64)
}

// ── LLM API Key Management Commands ────────────────────────────────────

pub fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "*".repeat(key.len());
    }
    let prefix = &key[..4];
    let suffix = &key[key.len()-4..];
    format!("{}...{}", prefix, suffix)
}

pub fn simple_encrypt(key: &str) -> String {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD.encode(key.as_bytes())
}

pub fn simple_decrypt(encrypted: &str) -> Result<String, String> {
    use base64::{Engine as _, engine::general_purpose};
    let bytes = general_purpose::STANDARD.decode(encrypted)
        .map_err(|e| format!("Decryption failed: {}", e))?;
    String::from_utf8(bytes).map_err(|e| format!("Invalid UTF-8: {}", e))
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
        id, provider, name, key_preview, project_id, runtime,
        is_active: true, last_used: None, usage_count: 0,
        created_at: now.clone(), updated_at: now,
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

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
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
    }).map_err(|e| e.to_string())?;

    let mut keys = Vec::new();
    for row in rows {
        keys.push(row.map_err(|e| e.to_string())?);
    }
    Ok(keys)
}

#[tauri::command]
pub fn get_llm_api_key_value(
    db: State<'_, DbState>,
    id: String,
) -> Result<String, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let encrypted: String = conn.query_row(
        "SELECT encrypted_key FROM llm_api_keys WHERE id = ?1",
        params![id], |row| row.get(0),
    ).map_err(|e| e.to_string())?;

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
    ).map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, provider, name, key_preview, project_id, runtime, is_active, last_used, usage_count, created_at, updated_at
         FROM llm_api_keys WHERE id = ?1"
    ).map_err(|e| e.to_string())?;

    stmt.query_row(params![id], |row| {
        Ok(LlmApiKey {
            id: row.get(0)?, provider: row.get(1)?, name: row.get(2)?,
            key_preview: row.get(3)?, project_id: row.get(4)?, runtime: row.get(5)?,
            is_active: row.get::<_, i32>(6)? != 0, last_used: row.get(7)?,
            usage_count: row.get(8)?, created_at: row.get(9)?, updated_at: row.get(10)?,
        })
    }).map_err(|e| e.to_string())
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
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_llm_api_key(
    db: State<'_, DbState>,
    id: String,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM llm_api_keys WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Real-time Agent Monitoring Commands ─────────────────────────────────

#[tauri::command]
pub fn get_monitoring_snapshot(
    db: State<'_, DbState>,
) -> Result<MonitoringSnapshot, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let mut active_stmt = conn.prepare(
        "SELECT id, runtime, status, prompt, tokens_in, tokens_out, duration_ms, skill_name, created_at
         FROM execution_logs WHERE status = 'running' ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;

    let active_sessions: Vec<AgentSession> = active_stmt.query_map([], |row| {
        Ok(AgentSession {
            id: row.get(0)?, runtime: row.get(1)?, status: row.get(2)?,
            prompt: row.get(3)?, tokens_in: row.get::<_, i64>(4).unwrap_or(0),
            tokens_out: row.get::<_, i64>(5).unwrap_or(0), duration_ms: row.get(6)?,
            skill_name: row.get(7)?, started_at: row.get(8)?, ended_at: None,
        })
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    let mut recent_stmt = conn.prepare(
        "SELECT id, runtime, status, prompt, tokens_in, tokens_out, duration_ms, skill_name, created_at
         FROM execution_logs WHERE status != 'running' ORDER BY created_at DESC LIMIT 20"
    ).map_err(|e| e.to_string())?;

    let recent_sessions: Vec<AgentSession> = recent_stmt.query_map([], |row| {
        Ok(AgentSession {
            id: row.get(0)?, runtime: row.get(1)?, status: row.get(2)?,
            prompt: row.get(3)?, tokens_in: row.get::<_, i64>(4).unwrap_or(0),
            tokens_out: row.get::<_, i64>(5).unwrap_or(0), duration_ms: row.get(6)?,
            skill_name: row.get(7)?, started_at: row.get(8)?, ended_at: None,
        })
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    let total_tokens_today: i64 = conn.query_row(
        "SELECT COALESCE(SUM(COALESCE(tokens_in,0) + COALESCE(tokens_out,0)), 0) FROM execution_logs WHERE created_at > datetime('now', '-1 day')",
        [], |row| row.get(0)
    ).unwrap_or(0);

    let total_sessions_today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM execution_logs WHERE created_at > datetime('now', '-1 day')",
        [], |row| row.get(0)
    ).unwrap_or(0);

    let errors_today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM execution_logs WHERE status = 'error' AND created_at > datetime('now', '-1 day')",
        [], |row| row.get(0)
    ).unwrap_or(0);

    let avg_duration_ms: f64 = conn.query_row(
        "SELECT COALESCE(AVG(duration_ms), 0) FROM execution_logs WHERE duration_ms IS NOT NULL AND created_at > datetime('now', '-1 day')",
        [], |row| row.get(0)
    ).unwrap_or(0.0);

    let tokens_last_hour: i64 = conn.query_row(
        "SELECT COALESCE(SUM(COALESCE(tokens_in,0) + COALESCE(tokens_out,0)), 0) FROM execution_logs WHERE created_at > datetime('now', '-1 hour')",
        [], |row| row.get(0)
    ).unwrap_or(0);
    let token_rate_per_hour = tokens_last_hour as f64;

    let mut online_runtimes = Vec::new();
    let mut offline_runtimes = Vec::new();
    let mut health_stmt = conn.prepare(
        "SELECT runtime, status FROM health_checks
         WHERE rowid IN (SELECT MAX(rowid) FROM health_checks GROUP BY runtime)"
    ).map_err(|e| e.to_string())?;

    let _ = health_stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .for_each(|(runtime, status)| {
        if status == "healthy" || status == "online" {
            online_runtimes.push(runtime);
        } else {
            offline_runtimes.push(runtime);
        }
    });

    let mut alerts = Vec::new();
    if errors_today > 5 {
        alerts.push(MonitoringAlert {
            id: uuid::Uuid::new_v4().to_string(), level: "warning".to_string(),
            message: format!("{} errors in the last 24 hours", errors_today),
            runtime: None, created_at: chrono::Utc::now().to_rfc3339(),
        });
    }
    if token_rate_per_hour > 100000.0 {
        alerts.push(MonitoringAlert {
            id: uuid::Uuid::new_v4().to_string(), level: "warning".to_string(),
            message: format!("High token usage: {:.0} tokens/hour", token_rate_per_hour),
            runtime: None, created_at: chrono::Utc::now().to_rfc3339(),
        });
    }
    for rt in &offline_runtimes {
        alerts.push(MonitoringAlert {
            id: uuid::Uuid::new_v4().to_string(), level: "error".to_string(),
            message: format!("{} runtime is offline", rt),
            runtime: Some(rt.clone()), created_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    Ok(MonitoringSnapshot {
        active_sessions, recent_sessions, total_tokens_today, total_sessions_today,
        errors_today, avg_duration_ms, runtimes_online: online_runtimes,
        runtimes_offline: offline_runtimes, token_rate_per_hour, alerts,
    })
}

#[tauri::command]
pub fn get_token_timeline(
    db: State<'_, DbState>,
    hours: Option<u32>,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let hours = hours.unwrap_or(24);

    let mut stmt = conn.prepare(&format!(
        "SELECT strftime('%Y-%m-%dT%H:00:00Z', created_at) as hour,
                runtime,
                COALESCE(SUM(tokens_in), 0) as total_in,
                COALESCE(SUM(tokens_out), 0) as total_out,
                COUNT(*) as session_count
         FROM execution_logs
         WHERE created_at > datetime('now', '-{} hours')
         GROUP BY hour, runtime
         ORDER BY hour ASC",
        hours
    )).map_err(|e| e.to_string())?;

    let rows: Vec<serde_json::Value> = stmt.query_map([], |row| {
        Ok(json!({
            "hour": row.get::<_, String>(0)?,
            "runtime": row.get::<_, String>(1)?,
            "tokensIn": row.get::<_, i64>(2).unwrap_or(0),
            "tokensOut": row.get::<_, i64>(3).unwrap_or(0),
            "sessions": row.get::<_, i64>(4).unwrap_or(0)
        }))
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    Ok(rows)
}

// ── Agents (v1.3.0 T3) ────────────────────────────────────────────────────
//
// Records produced by the Create Agent wizard. Each record represents a
// runtime-specific agent file written to disk plus metadata for fast lookup
// from Home / Agents list.
//
// File-writing contract per runtime (kept minimal for v1.3.0 — Claude is the
// canonical path; other runtimes write a stub markdown placeholder so the
// agent record is real-on-disk, then v1.3.x ships richer per-runtime layouts):
//
//   claude    → ~/.claude/agents/<slug>.md
//   codex     → ~/.codex/agents/<slug>/AGENTS.md
//   gemini    → <project>/.gemini/agents/<slug>.yaml  (falls back to ~/.gemini)
//   openclaw  → ~/.openclaw/agents/<slug>/SOUL.md
//   hermes    → ~/.hermes/agents/<slug>/AGENT.md

fn slugify(input: &str) -> String {
    let s: String = input
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // collapse repeated dashes and trim
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.chars() {
        if c == '-' {
            if !prev_dash && !out.is_empty() {
                out.push('-');
            }
            prev_dash = true;
        } else {
            out.push(c);
            prev_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

fn agent_file_path(runtime: &str, slug: &str) -> Result<PathBuf, String> {
    let home = home_dir();
    let path = match runtime {
        "claude" => home.join(".claude").join("agents").join(format!("{}.md", slug)),
        "codex" => home.join(".codex").join("agents").join(slug).join("AGENTS.md"),
        "gemini" => home.join(".gemini").join("agents").join(format!("{}.yaml", slug)),
        "openclaw" => home.join(".openclaw").join("agents").join(slug).join("SOUL.md"),
        "hermes" => home.join(".hermes").join("agents").join(slug).join("AGENT.md"),
        other => return Err(format!("Unsupported runtime: {}", other)),
    };
    Ok(path)
}

fn render_agent_file(runtime: &str, agent: &Agent) -> String {
    match runtime {
        "claude" => render_claude_agent(agent),
        "codex" => render_codex_agent(agent),
        "gemini" => render_gemini_agent(agent),
        "openclaw" => render_openclaw_agent(agent),
        "hermes" => render_hermes_agent(agent),
        _ => String::new(),
    }
}

fn render_claude_agent(agent: &Agent) -> String {
    // Claude Code agent format: frontmatter + system prompt body.
    // See: https://docs.claude.com/en/docs/claude-code/sub-agents
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", agent.slug));
    if let Some(desc) = &agent.description {
        out.push_str(&format!("description: {}\n", desc));
    }
    if let Some(model) = &agent.model {
        out.push_str(&format!("model: {}\n", model));
    }
    out.push_str("---\n\n");
    out.push_str(&format!("# {}\n\n", agent.display_name));
    if let Some(prompt) = &agent.system_prompt {
        if !prompt.trim().is_empty() {
            out.push_str(prompt);
            out.push_str("\n");
        }
    }
    if let Some(goal) = &agent.goal {
        if agent.system_prompt.as_deref().unwrap_or("").trim().is_empty() {
            out.push_str(&format!(
                "You are an agent designed to: {}\n",
                goal
            ));
        }
    }
    out
}

fn render_codex_agent(agent: &Agent) -> String {
    // Codex / OpenAI Agents SDK uses AGENTS.md.
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", agent.display_name));
    if let Some(desc) = &agent.description {
        out.push_str(&format!("> {}\n\n", desc));
    }
    if let Some(model) = &agent.model {
        out.push_str(&format!("**Model:** `{}`\n\n", model));
    }
    if let Some(prompt) = &agent.system_prompt {
        out.push_str("## Instructions\n\n");
        out.push_str(prompt);
        out.push_str("\n");
    }
    out
}

fn render_gemini_agent(agent: &Agent) -> String {
    // Minimal root_agent-shaped YAML; user can extend later.
    let mut out = String::new();
    out.push_str(&format!("name: {}\n", agent.slug));
    out.push_str(&format!("display_name: \"{}\"\n", agent.display_name));
    if let Some(model) = &agent.model {
        out.push_str(&format!("model: {}\n", model));
    } else {
        out.push_str("model: gemini-2.0-flash-exp\n");
    }
    if let Some(prompt) = &agent.system_prompt {
        out.push_str("instruction: |\n");
        for line in prompt.lines() {
            out.push_str(&format!("  {}\n", line));
        }
    } else if let Some(goal) = &agent.goal {
        out.push_str("instruction: |\n");
        out.push_str(&format!("  You are an agent designed to: {}\n", goal));
    }
    out
}

fn render_openclaw_agent(agent: &Agent) -> String {
    // OpenClaw uses SOUL.md as the agent identity file.
    let mut out = String::new();
    out.push_str(&format!("# Soul: {}\n\n", agent.display_name));
    if let Some(desc) = &agent.description {
        out.push_str(&format!("{}\n\n", desc));
    }
    if let Some(prompt) = &agent.system_prompt {
        out.push_str("## Identity\n\n");
        out.push_str(prompt);
        out.push_str("\n");
    }
    out
}

fn render_hermes_agent(agent: &Agent) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Hermes Agent: {}\n\n", agent.display_name));
    if let Some(desc) = &agent.description {
        out.push_str(&format!("{}\n\n", desc));
    }
    if let Some(prompt) = &agent.system_prompt {
        out.push_str("## System\n\n");
        out.push_str(prompt);
        out.push_str("\n");
    }
    out
}

#[tauri::command]
pub fn create_agent(
    db: State<'_, DbState>,
    display_name: String,
    runtime: String,
    description: Option<String>,
    model: Option<String>,
    project_id: Option<String>,
    system_prompt: Option<String>,
    permissions: Option<Vec<String>>,
    skills: Option<Vec<String>>,
    mcps: Option<Vec<String>>,
    goal: Option<String>,
    write_file: Option<bool>,
    kind: Option<String>,
) -> Result<Agent, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    if display_name.trim().is_empty() {
        return Err("display_name cannot be empty".to_string());
    }
    let allowed = ["claude", "codex", "gemini", "openclaw", "hermes"];
    if !allowed.contains(&runtime.as_str()) {
        return Err(format!("Unsupported runtime: {}", runtime));
    }

    let slug = slugify(&display_name);
    if slug.is_empty() {
        return Err("display_name must contain at least one alphanumeric character".to_string());
    }

    // v2.0.0 — internal/external kind. External agents auto-lock to a read-only
    // permission set (no shell, no fs writes) so customer-facing deployments
    // can't accidentally execute arbitrary commands. The caller can still pass
    // `permissions` to override after creation if they know what they're doing.
    let kind_val = match kind.as_deref() {
        Some("external") => "external",
        Some("internal") | None => "internal",
        Some(other) => return Err(format!("Unsupported agent kind: {}", other)),
    }.to_string();

    let effective_permissions = if kind_val == "external" && permissions.is_none() {
        Some(vec![
            "Read".to_string(),
            "Grep".to_string(),
            "Glob".to_string(),
            "WebFetch".to_string(),
        ])
    } else {
        permissions
    };

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let permissions_json = effective_permissions.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());
    let skills_json = skills.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());
    let mcps_json = mcps.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());

    let mut agent = Agent {
        id: id.clone(),
        slug: slug.clone(),
        display_name: display_name.clone(),
        description: description.clone(),
        runtime: runtime.clone(),
        model: model.clone(),
        project_id: project_id.clone(),
        system_prompt: system_prompt.clone(),
        permissions: permissions_json.clone(),
        skills: skills_json.clone(),
        mcps: mcps_json.clone(),
        goal: goal.clone(),
        file_path: None,
        created_at: now.clone(),
        last_used_at: None,
        role_models: None,
        memory_policy: None,
        kind: Some(kind_val.clone()),
    };

    // Optionally write the agent file to disk. External agents skip this — they
    // live in the cloud / customer infra after deploy, not on the dev's laptop.
    let should_write_file = write_file.unwrap_or(true) && kind_val == "internal";
    if should_write_file {
        let path = agent_file_path(&runtime, &slug)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create agent directory: {}", e))?;
        }
        let contents = render_agent_file(&runtime, &agent);
        fs::write(&path, &contents)
            .map_err(|e| format!("Failed to write agent file: {}", e))?;
        agent.file_path = Some(path.to_string_lossy().to_string());
    }

    // Insert into DB.
    conn.execute(
        "INSERT INTO agents (id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at, kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            agent.id, agent.slug, agent.display_name, agent.description, agent.runtime, agent.model,
            agent.project_id, agent.system_prompt, agent.permissions, agent.skills, agent.mcps,
            agent.goal, agent.file_path, agent.created_at, agent.last_used_at, kind_val
        ],
    ).map_err(|e| {
        // SQLite UNIQUE violation → friendly message
        let msg = e.to_string();
        if msg.contains("UNIQUE") {
            format!("An agent named \"{}\" already exists for runtime {}", slug, runtime)
        } else {
            msg
        }
    })?;

    Ok(agent)
}

#[tauri::command]
pub fn list_agents(
    db: State<'_, DbState>,
    runtime: Option<String>,
    project_id: Option<String>,
) -> Result<Vec<Agent>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let mut sql = String::from(
        "SELECT id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at, role_models_json, memory_policy_json, kind FROM agents",
    );
    let mut conditions: Vec<&str> = Vec::new();
    if runtime.is_some() {
        conditions.push("runtime = ?");
    }
    if project_id.is_some() {
        conditions.push("project_id = ?");
    }
    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    sql.push_str(" ORDER BY COALESCE(last_used_at, created_at) DESC");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut bindings: Vec<&dyn rusqlite::ToSql> = Vec::new();
    if let Some(r) = &runtime {
        bindings.push(r);
    }
    if let Some(p) = &project_id {
        bindings.push(p);
    }

    let rows = stmt
        .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
            Ok(Agent {
                id: row.get(0)?,
                slug: row.get(1)?,
                display_name: row.get(2)?,
                description: row.get(3)?,
                runtime: row.get(4)?,
                model: row.get(5)?,
                project_id: row.get(6)?,
                system_prompt: row.get(7)?,
                permissions: row.get(8)?,
                skills: row.get(9)?,
                mcps: row.get(10)?,
                goal: row.get(11)?,
                file_path: row.get(12)?,
                created_at: row.get(13)?,
                last_used_at: row.get(14)?,
                role_models: row.get(15).ok(),
                memory_policy: row.get(16).ok(),
                kind: row.get(17).ok(),
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_agent(db: State<'_, DbState>, id: String) -> Result<Agent, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at, role_models_json, memory_policy_json, kind FROM agents WHERE id = ?1",
        params![id],
        |row| {
            Ok(Agent {
                id: row.get(0)?,
                slug: row.get(1)?,
                display_name: row.get(2)?,
                description: row.get(3)?,
                runtime: row.get(4)?,
                model: row.get(5)?,
                project_id: row.get(6)?,
                system_prompt: row.get(7)?,
                permissions: row.get(8)?,
                skills: row.get(9)?,
                mcps: row.get(10)?,
                goal: row.get(11)?,
                file_path: row.get(12)?,
                created_at: row.get(13)?,
                last_used_at: row.get(14)?,
                role_models: row.get(15).ok(),
                memory_policy: row.get(16).ok(),
                kind: row.get(17).ok(),
            })
        },
    )
    .map_err(|e| e.to_string())
}

/// v2.0.0 — flip an existing agent between internal and external. Switching to
/// `external` does NOT auto-rewrite permissions on existing agents (caller is
/// expected to review and adjust); the auto-lock behavior only fires at create
/// time. This way users who deliberately broadened permissions don't lose them
/// silently when they flip the toggle to share via embed.
#[tauri::command]
pub fn update_agent_kind(
    db: State<'_, DbState>,
    id: String,
    kind: String,
) -> Result<(), String> {
    if kind != "internal" && kind != "external" {
        return Err(format!("Unsupported agent kind: {}", kind));
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE agents SET kind = ?1 WHERE id = ?2",
        params![kind, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── v2.0.0 Wave 2 — Local knowledge ──────────────────────────────────────
//
// Embedding via OpenAI text-embedding-3-small. Free choice of provider for
// the LLM at deploy time, but for embeddings we standardize on a single
// model so chunks ingested today are still retrievable when the user
// changes LLM providers tomorrow. The customer's OpenAI key (from
// `llm_api_keys` where provider='openai') is the only secret needed.
//
// Chunks are stored locally in `agent_knowledge_chunks`. Cloud sync is
// v2.1's job — for v2.0 alpha we inline chunks into the deployed bundle
// at generation time, so the deployed agent is fully self-contained.

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeChunk {
    pub id: String,
    pub agent_id: String,
    pub source: String,
    pub content: String,
    pub tokens: i64,
    pub position: i64,
    pub embed_model: String,
    pub created_at: String,
    /// Embedding as a flat f32 array. Decoded from the BLOB on read.
    /// Skipped when listing chunks (UI doesn't need 1536 floats per row);
    /// included when generating deploy bundles.
    pub embedding: Option<Vec<f32>>,
}

// v2.0.0 — multi-provider embeddings.
//
// We support five providers, in this preference order. Auto-detected based
// on which API key the user has in `llm_api_keys` (so a user with no
// OpenAI key but a Voyage one gets Voyage automatically — they don't have
// to pick or configure anything).
//
// Each chunk row records `embed_model` so retrieval is always done with
// the same provider that ingested the chunk — vector spaces don't
// interoperate across providers.
//
// Ollama is the offline fallback: needs no key, runs on the user's
// machine, but requires the user to have pulled `nomic-embed-text` first.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbedProvider {
    OpenAI,    // text-embedding-3-small — 1536 dims, $0.02/1M tokens
    Voyage,    // voyage-3 — 1024 dims, ~$0.06/1M tokens
    Gemini,    // text-embedding-004 — 768 dims, free tier available
    Cohere,    // embed-multilingual-light-v3.0 — 384 dims
    Ollama,    // nomic-embed-text — 768 dims, free, local
}

impl EmbedProvider {
    fn provider_id(&self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Voyage => "voyage",
            Self::Gemini => "gemini",
            Self::Cohere => "cohere",
            Self::Ollama => "ollama",
        }
    }
    fn default_model(&self) -> &'static str {
        match self {
            Self::OpenAI => "text-embedding-3-small",
            Self::Voyage => "voyage-3",
            Self::Gemini => "text-embedding-004",
            Self::Cohere => "embed-multilingual-light-v3.0",
            Self::Ollama => "nomic-embed-text",
        }
    }
    fn dims(&self) -> usize {
        match self {
            Self::OpenAI => 1536,
            Self::Voyage => 1024,
            Self::Gemini => 768,
            Self::Cohere => 384,
            Self::Ollama => 768,
        }
    }
}

/// Pick an embedding provider based on what's available. Auto-detection
/// avoids forcing every user through a picker — the most common case is
/// "I added an OpenAI key" (or a Voyage one) and embeddings should just
/// work. If multiple keys exist we prefer the cheapest first-tier
/// (OpenAI), then Voyage, then Gemini, then Cohere, then Ollama.
fn pick_embed_provider(conn: &rusqlite::Connection) -> Result<(EmbedProvider, Option<String>), String> {
    for p in [EmbedProvider::OpenAI, EmbedProvider::Voyage, EmbedProvider::Gemini, EmbedProvider::Cohere] {
        if let Ok(key) = read_provider_active_key(conn, p.provider_id()) {
            return Ok((p, Some(key)));
        }
    }
    // No cloud key — fall back to local Ollama (no key required). Caller
    // hits localhost:11434, which fails fast if Ollama isn't running.
    Ok((EmbedProvider::Ollama, None))
}

fn read_provider_active_key(
    conn: &rusqlite::Connection,
    provider: &str,
) -> Result<String, String> {
    match conn.query_row::<String, _, _>(
        "SELECT encrypted_key FROM llm_api_keys WHERE provider = ?1 AND is_active = 1 ORDER BY created_at DESC LIMIT 1",
        params![provider],
        |row| row.get(0),
    ) {
        Ok(encrypted) => simple_decrypt(&encrypted),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(format!("no {} key", provider)),
        Err(e) => Err(e.to_string()),
    }
}

const EMBED_MODEL: &str = "text-embedding-3-small";
const EMBED_DIMS: usize = 1536;
/// Hard cap so a runaway paste doesn't try to embed an entire book in one
/// request. Beyond this we'd need to batch — keep it simple for v2.0.
const MAX_CHARS_PER_INGEST: usize = 200_000;
/// Target chunk size in characters. ~375 tokens for English text. Small
/// enough that 5–8 chunks fit in any LLM context, large enough that a
/// chunk has actual context.
const CHUNK_CHARS: usize = 1500;
const CHUNK_OVERLAP: usize = 200;

fn chunk_text(text: &str) -> Vec<String> {
    // Naive char-window chunker with overlap. Splits on paragraph boundary
    // when one is within the overlap region so chunks don't tear sentences.
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let end = (i + CHUNK_CHARS).min(chars.len());
        // Try to back off to a paragraph break (\n\n) within the overlap
        // region so we don't tear mid-sentence.
        let mut split_at = end;
        if end < chars.len() {
            let lookback_start = end.saturating_sub(CHUNK_OVERLAP);
            for j in (lookback_start..end).rev() {
                if chars[j] == '\n' && j > 0 && chars[j - 1] == '\n' {
                    split_at = j + 1;
                    break;
                }
            }
        }
        let slice: String = chars[i..split_at].iter().collect();
        let trimmed = slice.trim().to_string();
        if !trimmed.is_empty() {
            chunks.push(trimmed);
        }
        if split_at >= chars.len() {
            break;
        }
        i = split_at.saturating_sub(CHUNK_OVERLAP);
    }
    chunks
}

fn f32_vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for &f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn blob_to_f32_vec(blob: &[u8]) -> Vec<f32> {
    let n = blob.len() / 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let bytes = [blob[i * 4], blob[i * 4 + 1], blob[i * 4 + 2], blob[i * 4 + 3]];
        out.push(f32::from_le_bytes(bytes));
    }
    out
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

// (former read_openai_key removed — replaced by pick_embed_provider +
// read_provider_active_key which auto-detect across 5 providers.)

#[derive(Debug, Serialize)]
struct OpenAIEmbeddingRequest<'a> {
    input: &'a [String],
    model: &'a str,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingResponse {
    data: Vec<OpenAIEmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingItem {
    embedding: Vec<f32>,
    index: usize,
}

async fn embed_batch(
    provider: EmbedProvider,
    api_key: Option<&str>,
    inputs: &[String],
) -> Result<Vec<Vec<f32>>, String> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let dims = provider.dims();
    let client = reqwest::Client::new();

    match provider {
        EmbedProvider::OpenAI => {
            let key = api_key.ok_or("OpenAI embedder requires an API key")?;
            let payload = OpenAIEmbeddingRequest { input: inputs, model: provider.default_model() };
            let r = client
                .post("https://api.openai.com/v1/embeddings")
                .bearer_auth(key)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("OpenAI embeddings request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("OpenAI embeddings {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let parsed: OpenAIEmbeddingResponse = r.json().await.map_err(|e| e.to_string())?;
            let mut out: Vec<Vec<f32>> = vec![Vec::new(); inputs.len()];
            for item in parsed.data {
                if item.index < out.len() && item.embedding.len() == dims {
                    out[item.index] = item.embedding;
                }
            }
            if out.iter().any(|v| v.len() != dims) {
                return Err("OpenAI embeddings: missing/wrong-dim vector".to_string());
            }
            Ok(out)
        }

        EmbedProvider::Voyage => {
            // Voyage's API is OpenAI-compatible-ish but NOT identical — uses
            // `input` array, model name `voyage-3`. Returns same `data[].embedding`.
            let key = api_key.ok_or("Voyage embedder requires an API key")?;
            let payload = serde_json::json!({
                "input": inputs,
                "model": provider.default_model(),
            });
            let r = client
                .post("https://api.voyageai.com/v1/embeddings")
                .bearer_auth(key)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("Voyage embeddings request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("Voyage embeddings {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            let arr = body.get("data").and_then(|d| d.as_array())
                .ok_or("Voyage: missing `data` array")?;
            let mut out: Vec<Vec<f32>> = vec![Vec::new(); inputs.len()];
            for item in arr {
                let idx = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let emb = item.get("embedding").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect::<Vec<_>>())
                    .unwrap_or_default();
                if idx < out.len() && emb.len() == dims {
                    out[idx] = emb;
                }
            }
            if out.iter().any(|v| v.len() != dims) {
                return Err("Voyage embeddings: missing/wrong-dim vector".to_string());
            }
            Ok(out)
        }

        EmbedProvider::Gemini => {
            // Gemini exposes batch embeddings via `:batchEmbedContents`. Single
            // request, parallel embedding requests in the body.
            let key = api_key.ok_or("Gemini embedder requires an API key")?;
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:batchEmbedContents?key={}",
                provider.default_model(),
                key,
            );
            let requests: Vec<serde_json::Value> = inputs.iter().map(|t| serde_json::json!({
                "model": format!("models/{}", provider.default_model()),
                "content": { "parts": [{ "text": t }] },
            })).collect();
            let payload = serde_json::json!({ "requests": requests });
            let r = client.post(&url).json(&payload).send().await
                .map_err(|e| format!("Gemini embeddings request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("Gemini embeddings {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            let arr = body.get("embeddings").and_then(|d| d.as_array())
                .ok_or("Gemini: missing `embeddings` array")?;
            let mut out: Vec<Vec<f32>> = Vec::with_capacity(inputs.len());
            for item in arr {
                let emb = item.get("values").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect::<Vec<_>>())
                    .unwrap_or_default();
                out.push(emb);
            }
            if out.len() != inputs.len() || out.iter().any(|v| v.len() != dims) {
                return Err("Gemini embeddings: count or dim mismatch".to_string());
            }
            Ok(out)
        }

        EmbedProvider::Cohere => {
            let key = api_key.ok_or("Cohere embedder requires an API key")?;
            let payload = serde_json::json!({
                "texts": inputs,
                "model": provider.default_model(),
                "input_type": "search_document",
            });
            let r = client
                .post("https://api.cohere.com/v2/embed")
                .bearer_auth(key)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("Cohere embeddings request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("Cohere embeddings {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            // Cohere returns {embeddings: {float: [[...]]}} or {embeddings: [[...]]}
            let arr = body.pointer("/embeddings/float")
                .or_else(|| body.get("embeddings"))
                .and_then(|d| d.as_array())
                .ok_or("Cohere: missing embeddings array")?;
            let mut out: Vec<Vec<f32>> = Vec::with_capacity(inputs.len());
            for item in arr {
                let emb = item.as_array()
                    .map(|a| a.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect::<Vec<_>>())
                    .unwrap_or_default();
                out.push(emb);
            }
            if out.len() != inputs.len() || out.iter().any(|v| v.len() != dims) {
                return Err("Cohere embeddings: count or dim mismatch".to_string());
            }
            Ok(out)
        }

        EmbedProvider::Ollama => {
            // Local fallback. Hits localhost:11434/api/embed. User must have
            // run `ollama pull nomic-embed-text` once, otherwise this errors.
            // The deployed bundle CAN'T use Ollama (it's local) — only ingest
            // works with this provider for now.
            let model = provider.default_model();
            let mut out: Vec<Vec<f32>> = Vec::with_capacity(inputs.len());
            for input in inputs {
                let r = client
                    .post("http://localhost:11434/api/embeddings")
                    .json(&serde_json::json!({ "model": model, "prompt": input }))
                    .send()
                    .await
                    .map_err(|e| format!("Ollama not reachable on localhost:11434 — start it with `ollama serve` and pull the model with `ollama pull {}`. Underlying: {}", model, e))?;
                if !r.status().is_success() {
                    return Err(format!("Ollama embeddings {}: {}", r.status(), r.text().await.unwrap_or_default()));
                }
                let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
                let emb = body.get("embedding").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect::<Vec<_>>())
                    .unwrap_or_default();
                if emb.len() != dims {
                    return Err(format!("Ollama embeddings: model returned {} dims, expected {}", emb.len(), dims));
                }
                out.push(emb);
            }
            Ok(out)
        }
    }
}

/// Approximate token count — char count / 4 for English text. Matches
/// OpenAI's rough rule of thumb close enough for the UI's storage display.
fn approx_tokens(s: &str) -> i64 {
    (s.chars().count() / 4) as i64
}

/// Ingest a chunk of plain text (typically a .md or .txt file's contents).
/// Replaces any prior chunks for the same `source` so re-uploading the same
/// file overwrites instead of duplicating.
#[tauri::command]
pub async fn ingest_knowledge_text(
    db: State<'_, DbState>,
    agent_id: String,
    source: String,
    content: String,
) -> Result<Vec<KnowledgeChunk>, String> {
    if content.is_empty() {
        return Err("content cannot be empty".to_string());
    }
    if content.len() > MAX_CHARS_PER_INGEST {
        return Err(format!(
            "content too large ({} chars, max {}). Split the file before uploading.",
            content.len(),
            MAX_CHARS_PER_INGEST
        ));
    }

    let chunks = chunk_text(&content);
    if chunks.is_empty() {
        return Err("nothing to embed — the file is whitespace only".to_string());
    }

    // Pick provider + key BEFORE the network call so we fail fast on
    // a misconfigured machine. Auto-detects based on which provider key
    // the user has on file. Ollama is used as the offline fallback.
    let (provider, api_key) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        pick_embed_provider(&conn)?
    };

    let embeddings = embed_batch(provider, api_key.as_deref(), &chunks).await?;
    if embeddings.len() != chunks.len() {
        return Err("embedder returned the wrong number of vectors".to_string());
    }

    let model_id = provider.default_model();
    let now = chrono::Utc::now().to_rfc3339();
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // Replace prior chunks from this same source so re-uploading overwrites.
    conn.execute(
        "DELETE FROM agent_knowledge_chunks WHERE agent_id = ?1 AND source = ?2",
        params![agent_id, source],
    )
    .map_err(|e| e.to_string())?;

    let mut out: Vec<KnowledgeChunk> = Vec::with_capacity(chunks.len());
    for (i, (text, embedding)) in chunks.into_iter().zip(embeddings.into_iter()).enumerate() {
        let id = uuid::Uuid::new_v4().to_string();
        let tokens = approx_tokens(&text);
        let blob = f32_vec_to_blob(&embedding);
        conn.execute(
            "INSERT INTO agent_knowledge_chunks
             (id, agent_id, source, content, tokens, position, embedding, embed_model, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, agent_id, source, text, tokens, i as i64, blob, model_id, now],
        )
        .map_err(|e| e.to_string())?;
        out.push(KnowledgeChunk {
            id,
            agent_id: agent_id.clone(),
            source: source.clone(),
            content: text,
            tokens,
            position: i as i64,
            embed_model: model_id.to_string(),
            created_at: now.clone(),
            embedding: Some(embedding),
        });
    }
    Ok(out)
}

/// List chunks for an agent. By default `include_embedding=false` so the UI
/// gets a fast list view; deploy-bundle generation passes `true`.
#[tauri::command]
pub fn list_agent_knowledge(
    db: State<'_, DbState>,
    agent_id: String,
    include_embedding: Option<bool>,
) -> Result<Vec<KnowledgeChunk>, String> {
    let with_embed = include_embedding.unwrap_or(false);
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, agent_id, source, content, tokens, position, embedding, embed_model, created_at
             FROM agent_knowledge_chunks
             WHERE agent_id = ?1
             ORDER BY source, position",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![agent_id], |row| {
            let blob: Vec<u8> = row.get(6)?;
            let embedding = if with_embed {
                Some(blob_to_f32_vec(&blob))
            } else {
                None
            };
            Ok(KnowledgeChunk {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                source: row.get(2)?,
                content: row.get(3)?,
                tokens: row.get(4)?,
                position: row.get(5)?,
                embed_model: row.get(7)?,
                created_at: row.get(8)?,
                embedding,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_knowledge_chunk(
    db: State<'_, DbState>,
    chunk_id: String,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM agent_knowledge_chunks WHERE id = ?1",
        params![chunk_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_knowledge_source(
    db: State<'_, DbState>,
    agent_id: String,
    source: String,
) -> Result<u64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let n = conn
        .execute(
            "DELETE FROM agent_knowledge_chunks WHERE agent_id = ?1 AND source = ?2",
            params![agent_id, source],
        )
        .map_err(|e| e.to_string())?;
    Ok(n as u64)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalHit {
    pub chunk: KnowledgeChunk,
    pub score: f32,
}

/// Retrieval — embed the query and return the top-K matching chunks. Used
/// by the "Test retrieval" panel in the UI; deploy bundles do their own
/// cosine-sim at request time using the inlined chunks.
#[tauri::command]
pub async fn retrieve_knowledge(
    db: State<'_, DbState>,
    agent_id: String,
    query: String,
    k: Option<u32>,
) -> Result<Vec<RetrievalHit>, String> {
    let k = k.unwrap_or(5).max(1).min(20) as usize;
    if query.trim().is_empty() {
        return Err("query cannot be empty".to_string());
    }

    // Pick provider — must match whichever was used to ingest the chunks
    // we're retrieving against. For v2 alpha we route via the same
    // auto-detect; if the user changed key sets between ingest and
    // retrieve, the cosine scores won't be meaningful (different vector
    // spaces). The chunk's stored `embed_model` is the source of truth
    // for "which provider should retrieve use" — wiring that lookup is a
    // v2.0.x follow-up.
    let (provider, api_key) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        pick_embed_provider(&conn)?
    };

    let query_embeddings = embed_batch(provider, api_key.as_deref(), &[query.clone()]).await?;
    let query_vec = query_embeddings
        .into_iter()
        .next()
        .ok_or_else(|| "embedder returned no vector for the query".to_string())?;

    let chunks = list_agent_knowledge(db, agent_id, Some(true))?;
    let mut scored: Vec<RetrievalHit> = chunks
        .into_iter()
        .filter_map(|c| {
            let v = c.embedding.clone().unwrap_or_default();
            if v.is_empty() {
                None
            } else {
                let s = cosine_similarity(&query_vec, &v);
                Some(RetrievalHit { chunk: c, score: s })
            }
        })
        .collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    Ok(scored)
}

/// v1.4.0 F3 — persist memory policy JSON for the agent.
#[tauri::command]
pub fn update_agent_memory_policy(
    db: State<'_, DbState>,
    id: String,
    policy_json: Option<String>,
) -> Result<(), String> {
    if let Some(ref s) = policy_json {
        // Validate JSON shape but don't constrain content — schema lives in TS.
        if !s.trim().is_empty() {
            serde_json::from_str::<serde_json::Value>(s)
                .map_err(|e| format!("Invalid memory_policy JSON: {}", e))?;
        }
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE agents SET memory_policy_json = ?1 WHERE id = ?2",
        params![policy_json, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// v1.4.0 F5 — persist per-task model selection for the agent.
#[tauri::command]
pub fn update_agent_role_models(
    db: State<'_, DbState>,
    id: String,
    role_models_json: Option<String>,
) -> Result<(), String> {
    if let Some(ref s) = role_models_json {
        if !s.trim().is_empty() {
            serde_json::from_str::<serde_json::Value>(s)
                .map_err(|e| format!("Invalid role_models JSON: {}", e))?;
        }
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE agents SET role_models_json = ?1 WHERE id = ?2",
        params![role_models_json, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Update the MCPs attached to an agent. Stored as a JSON-encoded string
/// array in `agents.mcps`. Used by the one-click "Add browser tools" button
/// and any future "attach MCP to agent" UX.
#[tauri::command]
pub fn update_agent_mcps(
    db: State<'_, DbState>,
    id: String,
    mcps: Vec<String>,
) -> Result<(), String> {
    let json = serde_json::to_string(&mcps).map_err(|e| e.to_string())?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE agents SET mcps = ?1 WHERE id = ?2",
        params![json, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_agent(db: State<'_, DbState>, id: String, delete_file: Option<bool>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    if delete_file.unwrap_or(true) {
        if let Ok(file_path) = conn.query_row(
            "SELECT file_path FROM agents WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        ) {
            if let Some(p) = file_path {
                let _ = fs::remove_file(&p);
            }
        }
    }

    conn.execute("DELETE FROM agents WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn touch_agent_last_used(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE agents SET last_used_at = ?1 WHERE id = ?2",
        params![now, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Agent Variables (v1.4.0 F1) ──────────────────────────────────────────
//
// Dynamic prompt resolvers per agent. The article's central insight: prompts
// are templates with `{var}` placeholders. Each variable has a "kind" + a
// kind-specific config_json. At dispatch time, we resolve all variables and
// substitute their values into the system + user prompts.
//
// Kinds (Free): static, env, project-path, file
// Kinds (Pro):  db-query, mcp-call, computed
//
// Pro resolvers are stubbed for Wave 2.1 — they return a clearly-flagged
// "Configure {{var}} to use Pro resolver" placeholder so the user sees that
// the gate exists. Wave 2.2 fills in the actual Pro implementations.

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentVariable {
    pub id: String,
    pub agent_id: String,
    pub name: String,
    pub kind: String,
    /// JSON-encoded resolver config. Shape depends on `kind`:
    ///   static       → { "value": "..." }
    ///   env          → { "var": "OPENAI_API_KEY" }
    ///   project-path → {}  (resolves to the active project's path)
    ///   file         → { "path": "/abs/or/~/path", "maxBytes": 8192 }
    ///   db-query     → { "connection": "...", "sql": "...", "column": 0 }
    ///   mcp-call     → { "server": "...", "tool": "...", "args": {...} }
    ///   computed     → { "expr": "..." }
    pub config_json: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[tauri::command]
pub fn list_agent_variables(
    db: State<'_, DbState>,
    agent_id: String,
) -> Result<Vec<AgentVariable>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, agent_id, name, kind, config_json, enabled, created_at, updated_at
             FROM agent_variables WHERE agent_id = ?1 ORDER BY name",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![agent_id], |row| {
            Ok(AgentVariable {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                config_json: row.get(4)?,
                enabled: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_agent_variable(
    db: State<'_, DbState>,
    id: Option<String>,
    agent_id: String,
    name: String,
    kind: String,
    config_json: String,
    enabled: Option<bool>,
) -> Result<AgentVariable, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    if name.trim().is_empty() {
        return Err("Variable name cannot be empty".into());
    }
    let allowed_kinds = ["static", "env", "project-path", "file", "db-query", "mcp-call", "computed"];
    if !allowed_kinds.contains(&kind.as_str()) {
        return Err(format!("Unsupported variable kind: {}", kind));
    }
    // Sanity-check name. Variables are referenced as {name} in prompts; allow
    // alphanumeric + underscore so substitution stays unambiguous.
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(
            "Variable name must contain only letters, digits, and underscores".into(),
        );
    }
    // Validate config_json parses.
    serde_json::from_str::<serde_json::Value>(&config_json)
        .map_err(|e| format!("Invalid config JSON: {}", e))?;

    let now = chrono::Utc::now().to_rfc3339();
    let final_id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let enabled_int: i32 = if enabled.unwrap_or(true) { 1 } else { 0 };

    conn.execute(
        "INSERT INTO agent_variables (id, agent_id, name, kind, config_json, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           kind = excluded.kind,
           config_json = excluded.config_json,
           enabled = excluded.enabled,
           updated_at = excluded.updated_at",
        params![final_id, agent_id, name, kind, config_json, enabled_int, now],
    )
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") {
            format!("Variable '{}' already exists for this agent", name)
        } else {
            msg
        }
    })?;

    Ok(AgentVariable {
        id: final_id,
        agent_id,
        name,
        kind,
        config_json,
        enabled: enabled.unwrap_or(true),
        created_at: now.clone(),
        updated_at: now,
    })
}

#[tauri::command]
pub fn delete_agent_variable(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM agent_variables WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Resolve every variable for an agent and return name→value map.
/// Disabled variables are skipped. Resolution failures are caught and the
/// variable resolves to a `{var:resolution-failed}` marker so the user sees
/// the failure in the rendered prompt rather than getting a silent miss.
pub fn resolve_agent_variables(
    conn: &Connection,
    agent_id: &str,
    active_project_path: Option<&str>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();

    let mut stmt = match conn.prepare(
        "SELECT name, kind, config_json FROM agent_variables
         WHERE agent_id = ?1 AND enabled = 1",
    ) {
        Ok(s) => s,
        Err(_) => return out,
    };

    let rows = match stmt.query_map(params![agent_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) {
        Ok(r) => r,
        Err(_) => return out,
    };

    for row in rows.flatten() {
        let (name, kind, config_json) = row;
        let value = resolve_one_variable(&kind, &config_json, active_project_path)
            .unwrap_or_else(|err| format!("{{{}:{}}}", name, err));
        out.insert(name, value);
    }
    out
}

fn resolve_one_variable(
    kind: &str,
    config_json: &str,
    active_project_path: Option<&str>,
) -> Result<String, String> {
    let cfg: serde_json::Value =
        serde_json::from_str(config_json).map_err(|_| "bad-config".to_string())?;
    match kind {
        "static" => Ok(cfg
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()),
        "env" => {
            let var = cfg
                .get("var")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing-var".to_string())?;
            std::env::var(var).map_err(|_| "env-not-set".to_string())
        }
        "project-path" => Ok(active_project_path
            .map(|s| s.to_string())
            .unwrap_or_else(|| "no-active-project".to_string())),
        "file" => {
            let path = cfg
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing-path".to_string())?;
            let max_bytes = cfg
                .get("maxBytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(8 * 1024) as usize;
            let expanded = expand_tilde(path);
            let contents = fs::read_to_string(&expanded).map_err(|_| "read-failed".to_string())?;
            if contents.len() > max_bytes {
                Ok(format!("{}…[truncated]", &contents[..max_bytes]))
            } else {
                Ok(contents)
            }
        }
        // Pro: read-only SQLite query against a path-configured database.
        // Tier gating happens in the UI — the resolver itself is local and
        // just needs the file. Postgres/MySQL deferred to a follow-up.
        "db-query" => resolve_db_query(&cfg),
        // Pro: constrained expression evaluator. Supports literals, var refs,
        // string concat with `+`, and basic arithmetic. No arbitrary JS.
        "computed" => resolve_computed(&cfg, active_project_path),
        // mcp-call still stubbed — needs an embedded MCP client. Tracked
        // separately; ship when we wire the MCP client into Rust.
        "mcp-call" => Err("mcp-call-not-yet-implemented".to_string()),
        _ => Err(format!("unknown-kind-{}", kind)),
    }
}

/// Run a read-only SELECT against a SQLite file. Refuses anything that
/// looks like a write — we don't want a misconfigured variable to delete
/// the user's data.
fn resolve_db_query(cfg: &serde_json::Value) -> Result<String, String> {
    let path = cfg
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing-path".to_string())?;
    let sql = cfg
        .get("sql")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing-sql".to_string())?;
    let max_rows = cfg
        .get("maxRows")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(500) as usize;

    // Reject anything that isn't a SELECT/WITH. Cheap heuristic, but the
    // OPEN_READ_ONLY flag below is the actual safety net.
    let trimmed = sql.trim_start().to_ascii_uppercase();
    if !(trimmed.starts_with("SELECT") || trimmed.starts_with("WITH")) {
        return Err("only-select-allowed".to_string());
    }

    let expanded = expand_tilde(path);
    let conn = rusqlite::Connection::open_with_flags(
        &expanded,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("open-failed: {}", e))?;

    let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare-failed: {}", e))?;
    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or("").to_string())
        .collect();

    let rows = stmt
        .query_map([], |row| {
            let mut obj = serde_json::Map::new();
            for (i, name) in col_names.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                let json = match val {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(n) => serde_json::Value::from(n),
                    rusqlite::types::Value::Real(f) => serde_json::Value::from(f),
                    rusqlite::types::Value::Text(s) => serde_json::Value::from(s),
                    rusqlite::types::Value::Blob(_) => serde_json::Value::String("(blob)".into()),
                };
                obj.insert(name.clone(), json);
            }
            Ok(serde_json::Value::Object(obj))
        })
        .map_err(|e| format!("query-failed: {}", e))?;

    let mut collected: Vec<serde_json::Value> = Vec::new();
    for r in rows {
        if collected.len() >= max_rows {
            break;
        }
        collected.push(r.map_err(|e| format!("row-failed: {}", e))?);
    }

    serde_json::to_string(&collected).map_err(|e| format!("serialize-failed: {}", e))
}

/// Tiny expression evaluator. Supports:
///   - string and number literals
///   - variable references (`{var_name}` is replaced before evaluation)
///   - string concat with `+`
///   - integer/float arithmetic: + - * /
/// Recognized identifiers: project_path() function returns the active project path.
fn resolve_computed(
    cfg: &serde_json::Value,
    active_project_path: Option<&str>,
) -> Result<String, String> {
    let expr = cfg
        .get("expr")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing-expr".to_string())?;

    // Substitute project_path() with the active project before parsing.
    let with_project = expr.replace(
        "project_path()",
        &format!("\"{}\"", active_project_path.unwrap_or("")),
    );

    eval_simple_expr(&with_project)
}

#[derive(Debug, Clone)]
enum ExprValue {
    Num(f64),
    Str(String),
}

impl ExprValue {
    fn to_render(&self) -> String {
        match self {
            ExprValue::Num(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    format!("{}", *n as i64)
                } else {
                    format!("{}", n)
                }
            }
            ExprValue::Str(s) => s.clone(),
        }
    }
}

/// Evaluator strictly limited to:
///   literal "..." | literal '...' | number | (expr) op (expr)
/// Operators: + - * /. Strings only support `+` (concat).
fn eval_simple_expr(input: &str) -> Result<String, String> {
    let tokens = tokenize_expr(input)?;
    let mut iter = tokens.into_iter().peekable();
    let value = parse_expr(&mut iter)?;
    if iter.next().is_some() {
        return Err("trailing-tokens".to_string());
    }
    Ok(value.to_render())
}

#[derive(Debug, Clone)]
enum ExprToken {
    Num(f64),
    Str(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

fn tokenize_expr(s: &str) -> Result<Vec<ExprToken>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '+' => { out.push(ExprToken::Plus); i += 1; }
            '-' => { out.push(ExprToken::Minus); i += 1; }
            '*' => { out.push(ExprToken::Star); i += 1; }
            '/' => { out.push(ExprToken::Slash); i += 1; }
            '(' => { out.push(ExprToken::LParen); i += 1; }
            ')' => { out.push(ExprToken::RParen); i += 1; }
            '"' | '\'' => {
                let quote = c;
                i += 1;
                let mut buf = String::new();
                while i < chars.len() && chars[i] != quote {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        buf.push(chars[i + 1]);
                        i += 2;
                    } else {
                        buf.push(chars[i]);
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err("unterminated-string".to_string());
                }
                i += 1; // consume closing quote
                out.push(ExprToken::Str(buf));
            }
            d if d.is_ascii_digit() || d == '.' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || chars[i] == '.')
                {
                    i += 1;
                }
                let lit: String = chars[start..i].iter().collect();
                let n: f64 = lit.parse().map_err(|_| format!("bad-number-{}", lit))?;
                out.push(ExprToken::Num(n));
            }
            _ => return Err(format!("unexpected-char-{}", c)),
        }
    }
    Ok(out)
}

type ExprIter = std::iter::Peekable<std::vec::IntoIter<ExprToken>>;

fn parse_expr(it: &mut ExprIter) -> Result<ExprValue, String> {
    parse_add(it)
}

fn parse_add(it: &mut ExprIter) -> Result<ExprValue, String> {
    let mut left = parse_mul(it)?;
    loop {
        match it.peek() {
            Some(ExprToken::Plus) => {
                it.next();
                let right = parse_mul(it)?;
                left = match (left, right) {
                    (ExprValue::Num(a), ExprValue::Num(b)) => ExprValue::Num(a + b),
                    (ExprValue::Str(a), ExprValue::Str(b)) => ExprValue::Str(format!("{}{}", a, b)),
                    (ExprValue::Str(a), ExprValue::Num(b)) => {
                        ExprValue::Str(format!("{}{}", a, ExprValue::Num(b).to_render()))
                    }
                    (ExprValue::Num(a), ExprValue::Str(b)) => {
                        ExprValue::Str(format!("{}{}", ExprValue::Num(a).to_render(), b))
                    }
                };
            }
            Some(ExprToken::Minus) => {
                it.next();
                let right = parse_mul(it)?;
                match (left, right) {
                    (ExprValue::Num(a), ExprValue::Num(b)) => left = ExprValue::Num(a - b),
                    _ => return Err("subtract-non-numbers".to_string()),
                }
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_mul(it: &mut ExprIter) -> Result<ExprValue, String> {
    let mut left = parse_atom(it)?;
    loop {
        match it.peek() {
            Some(ExprToken::Star) => {
                it.next();
                let right = parse_atom(it)?;
                match (left, right) {
                    (ExprValue::Num(a), ExprValue::Num(b)) => left = ExprValue::Num(a * b),
                    _ => return Err("multiply-non-numbers".to_string()),
                }
            }
            Some(ExprToken::Slash) => {
                it.next();
                let right = parse_atom(it)?;
                match (left, right) {
                    (ExprValue::Num(_), ExprValue::Num(b)) if b == 0.0 => {
                        return Err("divide-by-zero".to_string());
                    }
                    (ExprValue::Num(a), ExprValue::Num(b)) => left = ExprValue::Num(a / b),
                    _ => return Err("divide-non-numbers".to_string()),
                }
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_atom(it: &mut ExprIter) -> Result<ExprValue, String> {
    match it.next() {
        Some(ExprToken::Num(n)) => Ok(ExprValue::Num(n)),
        Some(ExprToken::Str(s)) => Ok(ExprValue::Str(s)),
        Some(ExprToken::LParen) => {
            let v = parse_expr(it)?;
            match it.next() {
                Some(ExprToken::RParen) => Ok(v),
                _ => Err("missing-rparen".to_string()),
            }
        }
        Some(ExprToken::Minus) => {
            let v = parse_atom(it)?;
            match v {
                ExprValue::Num(n) => Ok(ExprValue::Num(-n)),
                _ => Err("unary-minus-on-string".to_string()),
            }
        }
        _ => Err("unexpected-token".to_string()),
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    if path == "~" {
        return home_dir();
    }
    PathBuf::from(path)
}

/// Substitute `{var}` placeholders in a string with values from a map.
/// Unknown placeholders are left as-is so the user can see what's missing.
/// Identifiers must match `[A-Za-z_][A-Za-z0-9_]*` — anything else (e.g. JSON
/// `{ "key": ... }`) is left alone. Implemented as a single-pass scanner so
/// we don't pull in a regex dependency.
pub fn substitute_variables(template: &str, values: &HashMap<String, String>) -> String {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Look for matching identifier + closing '}'.
            let start = i + 1;
            let mut j = start;
            // First char must be letter or underscore.
            if j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'_') {
                j += 1;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_')
                {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'}' {
                    let name = &template[start..j];
                    match values.get(name) {
                        Some(v) => out.push_str(v),
                        None => out.push_str(&template[i..=j]),
                    }
                    i = j + 1;
                    continue;
                }
            }
        }
        // Push one UTF-8 codepoint at a time so we don't slice mid-character.
        let ch_end = next_char_boundary(template, i);
        out.push_str(&template[i..ch_end]);
        i = ch_end;
    }
    out
}

fn next_char_boundary(s: &str, mut i: usize) -> usize {
    i += 1;
    while !s.is_char_boundary(i) && i < s.len() {
        i += 1;
    }
    i
}

// ── Agent Hooks (v1.4.0 F2) ──────────────────────────────────────────────
//
// Pre-call context hooks. Each hook fetches data (file / webhook / mcp / db /
// computed) and the executor formats all results into a single <context>
// block that gets prepended to the user prompt before dispatch.

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentHook {
    pub id: String,
    pub agent_id: String,
    pub position: i32,
    pub name: String,
    pub kind: String,
    /// JSON-encoded config:
    ///   file     → { "path": "...", "maxBytes": 8192 }
    ///   webhook  → { "url": "...", "headers": {...}, "maxBytes": 8192 }
    ///   mcp-call → { "server": "...", "tool": "...", "args": {...} }
    ///   db-query → { "connection": "...", "sql": "..." }
    ///   computed → { "expr": "..." }
    ///
    /// v2.0.0 — When fire_mode != 'always', the config additionally
    /// carries fire-evaluation knobs:
    ///   keyword     → { ..., "whenKeywords": ["billing", "invoice"] }
    ///   llm-decides → { ..., "whenDescription": "user asks about billing",
    ///                   "classifierModel": "claude-haiku-4-5",
    ///                   "classifierProvider": "anthropic" }
    pub config_json: String,
    pub enabled: bool,
    pub created_at: String,
    /// 'always' (default) | 'keyword' | 'llm-decides'.
    /// Read in `run_pre_call_hooks` to decide whether to actually run
    /// the hook for a given user message — saves wasted API calls and
    /// noisy <context> blocks when the data isn't relevant.
    pub fire_mode: String,
}

#[tauri::command]
pub fn list_agent_hooks(
    db: State<'_, DbState>,
    agent_id: String,
) -> Result<Vec<AgentHook>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, agent_id, position, name, kind, config_json, enabled, created_at, fire_mode
             FROM agent_hooks WHERE agent_id = ?1 ORDER BY position ASC, created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![agent_id], |row| {
            Ok(AgentHook {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                position: row.get(2)?,
                name: row.get(3)?,
                kind: row.get(4)?,
                config_json: row.get(5)?,
                enabled: row.get::<_, i32>(6)? != 0,
                created_at: row.get(7)?,
                fire_mode: row.get::<_, Option<String>>(8)?.unwrap_or_else(|| "always".to_string()),
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_agent_hook(
    db: State<'_, DbState>,
    id: Option<String>,
    agent_id: String,
    position: Option<i32>,
    name: String,
    kind: String,
    config_json: String,
    enabled: Option<bool>,
    fire_mode: Option<String>,
) -> Result<AgentHook, String> {
    let allowed = ["file", "webhook", "mcp-call", "db-query", "computed"];
    if !allowed.contains(&kind.as_str()) {
        return Err(format!("Unsupported hook kind: {}", kind));
    }
    if name.trim().is_empty() {
        return Err("Hook name cannot be empty".into());
    }
    serde_json::from_str::<serde_json::Value>(&config_json)
        .map_err(|e| format!("Invalid hook config JSON: {}", e))?;

    let fire_mode_val = fire_mode.unwrap_or_else(|| "always".to_string());
    if !["always", "keyword", "llm-decides"].contains(&fire_mode_val.as_str()) {
        return Err(format!("Unsupported hook fire_mode: {}", fire_mode_val));
    }

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let final_id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let final_pos = position.unwrap_or_else(|| {
        // Append at end if no position given.
        conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM agent_hooks WHERE agent_id = ?1",
            params![agent_id],
            |r| r.get::<_, i32>(0),
        )
        .unwrap_or(0)
    });
    let enabled_int: i32 = if enabled.unwrap_or(true) { 1 } else { 0 };

    conn.execute(
        "INSERT INTO agent_hooks (id, agent_id, position, name, kind, config_json, enabled, created_at, fire_mode)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET
           position = excluded.position,
           name = excluded.name,
           kind = excluded.kind,
           config_json = excluded.config_json,
           enabled = excluded.enabled,
           fire_mode = excluded.fire_mode",
        params![final_id, agent_id, final_pos, name, kind, config_json, enabled_int, now, fire_mode_val],
    )
    .map_err(|e| e.to_string())?;

    Ok(AgentHook {
        id: final_id,
        agent_id,
        position: final_pos,
        name,
        kind,
        config_json,
        enabled: enabled.unwrap_or(true),
        created_at: now,
        fire_mode: fire_mode_val,
    })
}

#[tauri::command]
pub fn delete_agent_hook(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM agent_hooks WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Decide whether a hook should fire for THIS particular user message.
/// Returns true if the hook should run, false to skip it. Skipped hooks
/// don't contribute to the `<context>` block — saves API cost and keeps
/// the prompt tight when data isn't relevant. Beatriz's design (2026-05-08).
async fn should_fire_hook(hook: &AgentHook, user_prompt: &str) -> bool {
    let mode = hook.fire_mode.as_str();
    if mode == "always" {
        return true;
    }
    // Parse the JSON config once — the fire-eval knobs live here too.
    let cfg: serde_json::Value = match serde_json::from_str(&hook.config_json) {
        Ok(v) => v,
        // Malformed config falls back to firing — better to inject possibly
        // stale data than silently skip and have the agent ignorant.
        Err(_) => return true,
    };

    if mode == "keyword" {
        let keywords = cfg
            .get("whenKeywords")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_lowercase))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if keywords.is_empty() {
            return false; // no rules → never fires
        }
        let lower = user_prompt.to_lowercase();
        return keywords.iter().any(|k| lower.contains(k));
    }

    if mode == "llm-decides" {
        let when_desc = cfg
            .get("whenDescription")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if when_desc.is_empty() {
            return false; // no rule → never fires
        }
        let model = cfg
            .get("classifierModel")
            .and_then(|v| v.as_str())
            .unwrap_or("claude-haiku-4-5")
            .to_string();
        let provider = cfg
            .get("classifierProvider")
            .and_then(|v| v.as_str())
            .unwrap_or("anthropic")
            .to_string();
        match classify_should_fire(&provider, &model, when_desc, user_prompt).await {
            Ok(should) => should,
            // Classifier outage → fail-safe to firing the hook so the
            // agent doesn't suddenly lose data context.
            Err(_) => true,
        }
    } else {
        true
    }
}

/// Run all enabled hooks for an agent and return a formatted `<context>`
/// block. Failures don't break dispatch — they're surfaced as inline error
/// notes inside the same block so the model sees what couldn't be fetched.
async fn run_pre_call_hooks(
    hooks: Vec<AgentHook>,
    user_prompt: &str,
) -> String {
    if hooks.is_empty() {
        return String::new();
    }
    let mut sections: Vec<String> = Vec::new();
    for hook in hooks {
        if !hook.enabled {
            continue;
        }
        if !should_fire_hook(&hook, user_prompt).await {
            continue;
        }
        let result = execute_hook(&hook).await;
        let section = match result {
            Ok(content) => format!("<{name}>\n{body}\n</{name}>", name = hook.name, body = content),
            Err(e) => format!(
                "<{name} status=\"failed\">\n{body}\n</{name}>",
                name = hook.name,
                body = format!("Hook \"{}\" failed: {}", hook.name, e)
            ),
        };
        sections.push(section);
    }
    if sections.is_empty() {
        String::new()
    } else {
        format!("<context>\n{}\n</context>\n\n", sections.join("\n\n"))
    }
}

/// Lightweight LLM classifier — asks "should the hook fire?" and parses
/// the response. Designed for cheap fast models (Haiku, GPT-4o-mini,
/// Gemini Flash, etc.). Cost per call is in the order of $0.0001.
async fn classify_should_fire(
    provider: &str,
    model: &str,
    when_description: &str,
    user_prompt: &str,
) -> Result<bool, String> {
    // Use the provider's stored API key — we expect the same key that
    // powers the agent's chat dispatch to be on file.
    let api_key = read_provider_api_key(provider)?;
    let system = "You are a fast classifier. Respond with ONLY \"YES\" or \"NO\" (no other text). Decide whether the data described by the rule is relevant to the user's message.";
    let user = format!(
        "Rule: this data should fire when: {when_description}\n\nUser message: {user_prompt}\n\nShould the data fire? Reply YES or NO."
    );

    let client = reqwest::Client::new();
    let text = match provider {
        "anthropic" => {
            let payload = serde_json::json!({
                "model": model,
                "max_tokens": 8,
                "system": system,
                "messages": [{ "role": "user", "content": user }],
            });
            let r = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("classifier request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("classifier {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            body.get("content")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string()
        }
        // OpenAI-compatible chat completions for the rest. Covers OpenAI,
        // Groq, xAI, Mistral, DeepSeek, Together, Fireworks. Gemini uses
        // its own format and isn't supported as classifier in v2.0 alpha.
        _ => {
            let url = match provider {
                "openai"   => "https://api.openai.com/v1/chat/completions",
                "groq"     => "https://api.groq.com/openai/v1/chat/completions",
                "xai"      => "https://api.x.ai/v1/chat/completions",
                "mistral"  => "https://api.mistral.ai/v1/chat/completions",
                "deepseek" => "https://api.deepseek.com/v1/chat/completions",
                "together" => "https://api.together.xyz/v1/chat/completions",
                "fireworks"=> "https://api.fireworks.ai/inference/v1/chat/completions",
                _ => return Err(format!("classifier provider not supported: {}", provider)),
            };
            let payload = serde_json::json!({
                "model": model,
                "max_tokens": 8,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
            });
            let r = client
                .post(url)
                .bearer_auth(&api_key)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("classifier request failed: {}", e))?;
            if !r.status().is_success() {
                return Err(format!("classifier {}: {}", r.status(), r.text().await.unwrap_or_default()));
            }
            let body: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            body.get("choices")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string()
        }
    };
    Ok(text.to_uppercase().contains("YES"))
}

/// Look up the active API key for a given provider in `llm_api_keys`,
/// decrypted. Returns the most recently-created key. Used by the
/// classifier — same provider system as the agent's chat dispatch.
fn read_provider_api_key(provider: &str) -> Result<String, String> {
    use rusqlite::Connection;
    let path = home_dir().join(".ato").join("local.db");
    let conn = Connection::open(&path).map_err(|e| e.to_string())?;
    match conn.query_row::<String, _, _>(
        "SELECT encrypted_key FROM llm_api_keys WHERE provider = ?1 AND is_active = 1 ORDER BY created_at DESC LIMIT 1",
        params![provider],
        |row| row.get(0),
    ) {
        Ok(encrypted) => simple_decrypt(&encrypted),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(format!(
            "No {} API key on file. Add one in Settings → API Keys (or in the create-agent wizard).",
            provider
        )),
        Err(e) => Err(e.to_string()),
    }
}

async fn execute_hook(hook: &AgentHook) -> Result<String, String> {
    let cfg: serde_json::Value =
        serde_json::from_str(&hook.config_json).map_err(|_| "bad-config".to_string())?;
    match hook.kind.as_str() {
        "file" => {
            let path = cfg
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing-path".to_string())?;
            let max_bytes = cfg
                .get("maxBytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(8 * 1024) as usize;
            let expanded = expand_tilde(path);
            let contents = fs::read_to_string(&expanded).map_err(|e| e.to_string())?;
            if contents.len() > max_bytes {
                Ok(format!("{}…[truncated]", &contents[..max_bytes]))
            } else {
                Ok(contents)
            }
        }
        "webhook" => {
            let url = cfg
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing-url".to_string())?;
            let max_bytes = cfg
                .get("maxBytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(16 * 1024) as usize;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .map_err(|e| e.to_string())?;
            let mut req = client.get(url);
            if let Some(headers) = cfg.get("headers").and_then(|v| v.as_object()) {
                for (k, v) in headers {
                    if let Some(s) = v.as_str() {
                        req = req.header(k, s);
                    }
                }
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let body = resp.text().await.map_err(|e| e.to_string())?;
            if body.len() > max_bytes {
                Ok(format!("{}…[truncated]", &body[..max_bytes]))
            } else {
                Ok(body)
            }
        }
        // Reuse the variable resolvers — same kinds, same configs.
        "db-query" => resolve_db_query(&cfg),
        "computed" => resolve_computed(&cfg, None),
        "mcp-call" => Err("mcp-call-not-yet-implemented".to_string()),
        other => Err(format!("unknown-kind-{}", other)),
    }
}

/// Tauri command that wraps prompt_agent: resolves the agent's variables and
/// substitutes them in the prompt before dispatching. Used by Quick Test and
/// (future) cron jobs.
///
/// v2.1.0+ — returns a structured result so the frontend can pick up
/// the run_id (used for overlap-evidence lookup) without a second
/// registry round-trip. Only one direct invoke caller
/// (agentVariables.ts), so the shape change is contained.
#[derive(serde::Serialize)]
pub struct DispatchResult {
    pub response: String,
    /// Active-runs registry id assigned at dispatch start. The
    /// frontend uses it to fetch overlap evidence + compose the
    /// trace upload metadata.
    #[serde(rename = "runId")]
    pub run_id: String,
}

#[tauri::command]
pub async fn prompt_agent_with_context(
    db: State<'_, DbState>,
    agent_id: String,
    runtime: String,
    prompt: String,
    config: Option<String>,
    active_project_path: Option<String>,
) -> Result<DispatchResult, String> {
    // Step 1: resolve variables + load hooks + read role-model preferences
    // (single short-lived lock). Also pull the agent slug for the
    // active-runs registry (Phase 4) — Beatriz: showing slugs in the
    // Live panel matters more than UUIDs.
    let (resolved, hooks, response_model, fallback_model, agent_slug) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let resolved = resolve_agent_variables(&conn, &agent_id, active_project_path.as_deref());
        let hooks = load_agent_hooks(&conn, &agent_id);
        let (rm, fb) = load_agent_response_model(&conn, &agent_id);
        let slug: Option<String> = conn
            .query_row(
                "SELECT slug FROM agents WHERE id = ?1",
                rusqlite::params![&agent_id],
                |r| r.get::<_, String>(0),
            )
            .ok();
        (resolved, hooks, rm, fb, slug)
    };

    // Step 2: substitute into the prompt.
    let rendered_prompt = substitute_variables(&prompt, &resolved);

    // Step 3: run pre-call hooks → format as <context> block.
    let context_block = run_pre_call_hooks(hooks, &prompt).await;

    // Step 4: prepend context block to the user prompt.
    let final_prompt = if context_block.is_empty() {
        rendered_prompt
    } else {
        format!("{}{}", context_block, rendered_prompt)
    };

    // Step 5 (F5): merge the agent's response model into the runtime config
    // unless the caller already passed one. roleModels.response wins over
    // agents.model — that's the whole point of per-task models.
    let merged_config = merge_model_into_config(config, response_model, fallback_model);

    // Phase 4: register in the active-runs map for the duration of the
    // dispatch. Always finish_run via a guard so panics + early returns
    // don't leak entries.
    let run_id = crate::active_runs::begin_run(
        &runtime,
        agent_slug.as_deref(),
        active_project_path.as_deref(),
        Some("desktop:context-dispatch"),
    );
    let result = prompt_agent(runtime, final_prompt, merged_config).await;
    // Note: do NOT finish_run yet. Frontend needs to call
    // get_overlap_evidence(run_id) before the slot is removed; it
    // will then call list_active_runs again at its leisure (registry
    // self-heals after a stale entry timeout, but the explicit
    // contract is: caller is responsible for finish).
    //
    // Rationale: keeping finish_run on the Rust side would race the
    // frontend's overlap fetch. Instead we return run_id and let the
    // wrapper finish_run after upload. Worst case (frontend crashes):
    // entry stays until next call to begin_run with same workspace.
    match result {
        Ok(response) => Ok(DispatchResult { response, run_id }),
        Err(e) => {
            // On error we still tidy up — no overlap upload happens
            // for failed dispatches today, so the slot has no further
            // use.
            crate::active_runs::finish_run(&run_id);
            Err(e)
        }
    }
}

// ── Conversation summarization (F3) ──────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    /// "user" | "assistant" | "system" | "summary"
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemoryPolicyParsed {
    #[serde(default = "default_summarize_after")]
    summarize_after: usize,
    #[serde(default = "default_keep_last_k")]
    keep_last_k: usize,
    #[serde(default)]
    summarizer_model: String,
}

fn default_summarize_after() -> usize { 30 }
fn default_keep_last_k() -> usize { 5 }

impl Default for MemoryPolicyParsed {
    fn default() -> Self {
        Self {
            summarize_after: default_summarize_after(),
            keep_last_k: default_keep_last_k(),
            summarizer_model: String::new(),
        }
    }
}

fn load_memory_policy(conn: &Connection, agent_id: &str) -> MemoryPolicyParsed {
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT memory_policy_json FROM agents WHERE id = ?1",
            params![agent_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok();
    row.flatten()
        .and_then(|s| serde_json::from_str::<MemoryPolicyParsed>(&s).ok())
        .unwrap_or_default()
}

fn load_agent_summarizer_model(conn: &Connection, agent_id: &str) -> Option<String> {
    let rm_json: Option<Option<String>> = conn
        .query_row(
            "SELECT role_models_json FROM agents WHERE id = ?1",
            params![agent_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok();
    rm_json
        .flatten()
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("summarizer").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
}

/// Decide whether to summarize. Returns (older_to_summarize, recent_kept_verbatim).
/// If we don't need to summarize, the first slice is empty.
fn split_history_for_summarization(
    history: &[AgentMessage],
    policy: &MemoryPolicyParsed,
) -> (Vec<AgentMessage>, Vec<AgentMessage>) {
    if history.len() <= policy.summarize_after {
        return (Vec::new(), history.to_vec());
    }
    let keep_k = policy.keep_last_k.min(history.len());
    let split = history.len() - keep_k;
    (history[..split].to_vec(), history[split..].to_vec())
}

fn build_summarizer_prompt(older: &[AgentMessage]) -> String {
    let mut s = String::from(
        "Summarize the following conversation between a user and an AI agent. \
Keep concrete facts, decisions, names, identifiers, and any open questions. \
Drop pleasantries. Output 5-10 bullet points, no preamble.\n\n",
    );
    for m in older {
        s.push_str(&format!("[{}]: {}\n", m.role, m.content));
    }
    s.push_str("\nReturn the summary now.");
    s
}

fn build_final_prompt(
    summary: Option<&str>,
    recent: &[AgentMessage],
    new_user_prompt: &str,
) -> String {
    let mut out = String::new();
    if let Some(s) = summary {
        out.push_str("<conversation_summary>\n");
        out.push_str(s.trim());
        out.push_str("\n</conversation_summary>\n\n");
    }
    for m in recent {
        out.push_str(&format!("[{}]: {}\n", m.role, m.content));
    }
    out.push_str(&format!("\n[user]: {}\n", new_user_prompt));
    out
}

#[tauri::command]
pub async fn prompt_agent_with_history(
    db: State<'_, DbState>,
    agent_id: String,
    runtime: String,
    history: Vec<AgentMessage>,
    new_prompt: String,
    config: Option<String>,
    active_project_path: Option<String>,
) -> Result<String, String> {
    // Load all the dispatch-time inputs under one lock.
    let (resolved, hooks, response_model, fallback_model, policy, summarizer_model) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let resolved = resolve_agent_variables(&conn, &agent_id, active_project_path.as_deref());
        let hooks = load_agent_hooks(&conn, &agent_id);
        let (rm, fb) = load_agent_response_model(&conn, &agent_id);
        let policy = load_memory_policy(&conn, &agent_id);
        let summ = load_agent_summarizer_model(&conn, &agent_id);
        (resolved, hooks, rm, fb, policy, summ)
    };

    // Summarize if history exceeds the threshold.
    let (older, recent) = split_history_for_summarization(&history, &policy);
    let summary: Option<String> = if !older.is_empty() {
        let summarizer_prompt = build_summarizer_prompt(&older);
        // Pick summarizer model: explicit policy > role_models.summarizer >
        // none (runtime default).
        let chosen_summarizer = if !policy.summarizer_model.is_empty() {
            Some(policy.summarizer_model.clone())
        } else {
            summarizer_model
        };
        let summ_cfg = chosen_summarizer.map(|m| {
            serde_json::json!({ "model": m }).to_string()
        });
        match prompt_agent(runtime.clone(), summarizer_prompt, summ_cfg).await {
            Ok(s) => Some(s),
            // Summarization failure shouldn't block dispatch — fall back to
            // dropping the older history entirely. The agent loses memory
            // for this turn, which is the same as if we never summarized.
            Err(_) => None,
        }
    } else {
        None
    };

    // Resolve variables in the user's new prompt.
    let rendered_new = substitute_variables(&new_prompt, &resolved);

    // Stitch everything together.
    let stitched = build_final_prompt(summary.as_deref(), &recent, &rendered_new);

    // Pre-call hooks. fire_mode evaluation uses the new turn's user
    // message (`new_prompt`), not the stitched history — keyword/LLM
    // gating cares about what THIS turn is asking for.
    let context_block = run_pre_call_hooks(hooks, &new_prompt).await;
    let final_prompt = if context_block.is_empty() {
        stitched
    } else {
        format!("{}{}", context_block, stitched)
    };

    let merged_config = merge_model_into_config(config, response_model, fallback_model);
    prompt_agent(runtime, final_prompt, merged_config).await
}

/// Returns (role_models.response, agents.model). Either may be None.
fn load_agent_response_model(
    conn: &Connection,
    agent_id: &str,
) -> (Option<String>, Option<String>) {
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT role_models_json, model FROM agents WHERE id = ?1",
            params![agent_id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .ok();
    let (rm_json, agent_model) = row.unwrap_or((None, None));
    let response = rm_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("response").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .filter(|s| !s.is_empty());
    (response, agent_model.filter(|s| !s.is_empty()))
}

/// Merges a `model` override into the existing `config` JSON (or creates a
/// new one). The caller's existing config wins — we only set model when the
/// caller didn't.
fn merge_model_into_config(
    config: Option<String>,
    response_model: Option<String>,
    fallback_model: Option<String>,
) -> Option<String> {
    let chosen = response_model.or(fallback_model);
    let chosen = match chosen {
        Some(m) => m,
        None => return config,
    };

    let mut obj: serde_json::Map<String, serde_json::Value> = config
        .as_deref()
        .and_then(|c| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(c).ok())
        .unwrap_or_default();

    // Don't overwrite an explicit caller-supplied model.
    let already_set = obj
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !already_set {
        obj.insert("model".into(), serde_json::Value::String(chosen));
    }

    serde_json::to_string(&obj).ok()
}

fn load_agent_hooks(conn: &Connection, agent_id: &str) -> Vec<AgentHook> {
    let mut stmt = match conn.prepare(
        "SELECT id, agent_id, position, name, kind, config_json, enabled, created_at, fire_mode
         FROM agent_hooks WHERE agent_id = ?1 ORDER BY position ASC, created_at ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(params![agent_id], |row| {
        Ok(AgentHook {
            id: row.get(0)?,
            agent_id: row.get(1)?,
            position: row.get(2)?,
            name: row.get(3)?,
            kind: row.get(4)?,
            config_json: row.get(5)?,
            enabled: row.get::<_, i32>(6).unwrap_or(1) != 0,
            created_at: row.get(7)?,
            fire_mode: row.get::<_, Option<String>>(8)?.unwrap_or_else(|| "always".to_string()),
        })
    }) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.flatten().collect()
}

#[cfg(test)]
mod variable_tests {
    use super::*;

    #[test]
    fn substitute_handles_known_and_unknown() {
        let mut vals = HashMap::new();
        vals.insert("name".to_string(), "Beatriz".to_string());
        vals.insert("plan".to_string(), "Pro".to_string());
        let out = substitute_variables(
            "Hello {name}, your {plan} plan expires in {days} days.",
            &vals,
        );
        assert_eq!(
            out,
            "Hello Beatriz, your Pro plan expires in {days} days."
        );
    }

    #[test]
    fn resolve_static_returns_configured_value() {
        let v = resolve_one_variable("static", r#"{"value":"hi"}"#, None).unwrap();
        assert_eq!(v, "hi");
    }

    #[test]
    fn merge_model_uses_response_when_no_caller_model() {
        let merged = merge_model_into_config(None, Some("sonnet".into()), Some("opus".into()));
        let v: serde_json::Value = serde_json::from_str(&merged.unwrap()).unwrap();
        assert_eq!(v.get("model").unwrap().as_str().unwrap(), "sonnet");
    }

    #[test]
    fn merge_model_falls_back_to_agent_model() {
        let merged = merge_model_into_config(None, None, Some("opus".into()));
        let v: serde_json::Value = serde_json::from_str(&merged.unwrap()).unwrap();
        assert_eq!(v.get("model").unwrap().as_str().unwrap(), "opus");
    }

    #[test]
    fn merge_model_respects_caller_supplied_model() {
        let caller = r#"{"model":"haiku","sshHost":"foo"}"#;
        let merged = merge_model_into_config(Some(caller.into()), Some("sonnet".into()), Some("opus".into()));
        let v: serde_json::Value = serde_json::from_str(&merged.unwrap()).unwrap();
        assert_eq!(v.get("model").unwrap().as_str().unwrap(), "haiku");
        assert_eq!(v.get("sshHost").unwrap().as_str().unwrap(), "foo");
    }

    #[test]
    fn merge_model_returns_none_when_no_choice() {
        assert!(merge_model_into_config(None, None, None).is_none());
    }

    fn msg(role: &str, content: &str) -> AgentMessage {
        AgentMessage { role: role.into(), content: content.into() }
    }

    #[test]
    fn split_returns_all_recent_below_threshold() {
        let h = vec![msg("user", "hi"), msg("assistant", "hello")];
        let policy = MemoryPolicyParsed { summarize_after: 30, keep_last_k: 5, summarizer_model: "".into() };
        let (older, recent) = split_history_for_summarization(&h, &policy);
        assert!(older.is_empty());
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn split_keeps_last_k_when_over_threshold() {
        let mut h = Vec::new();
        for i in 0..40 { h.push(msg("user", &format!("m{}", i))); }
        let policy = MemoryPolicyParsed { summarize_after: 30, keep_last_k: 5, summarizer_model: "".into() };
        let (older, recent) = split_history_for_summarization(&h, &policy);
        assert_eq!(older.len(), 35);
        assert_eq!(recent.len(), 5);
        assert_eq!(recent[0].content, "m35");
        assert_eq!(recent[4].content, "m39");
    }

    #[test]
    fn build_final_prompt_wraps_summary_in_block() {
        let recent = vec![msg("user", "ping")];
        let out = build_final_prompt(Some("we discussed X"), &recent, "what's next?");
        assert!(out.contains("<conversation_summary>"));
        assert!(out.contains("we discussed X"));
        assert!(out.contains("</conversation_summary>"));
        assert!(out.contains("[user]: what's next?"));
    }

    #[test]
    fn resolve_project_path_uses_active() {
        let v = resolve_one_variable("project-path", "{}", Some("/work/repo")).unwrap();
        assert_eq!(v, "/work/repo");
    }

    #[test]
    fn resolve_env_missing_returns_error() {
        let v = resolve_one_variable("env", r#"{"var":"DEFINITELY_NOT_SET_VAR"}"#, None);
        assert!(v.is_err());
    }

    #[test]
    fn mcp_call_remains_stubbed() {
        let v = resolve_one_variable("mcp-call", "{}", None);
        assert!(matches!(
            v,
            Err(ref s) if s == "mcp-call-not-yet-implemented"
        ));
    }

    #[test]
    fn db_query_rejects_writes() {
        let cfg = r#"{"path":"/tmp/x.db","sql":"DELETE FROM users"}"#;
        let v = resolve_one_variable("db-query", cfg, None);
        assert!(matches!(v, Err(ref s) if s == "only-select-allowed"));
    }

    #[test]
    fn computed_evaluates_arithmetic() {
        let v = resolve_one_variable("computed", r#"{"expr":"2 + 3 * 4"}"#, None).unwrap();
        assert_eq!(v, "14");
    }

    #[test]
    fn computed_concatenates_strings() {
        let v = resolve_one_variable(
            "computed",
            r#"{"expr":"\"hello \" + \"world\""}"#,
            None,
        )
        .unwrap();
        assert_eq!(v, "hello world");
    }

    #[test]
    fn computed_uses_project_path() {
        let v = resolve_one_variable(
            "computed",
            r#"{"expr":"project_path() + \"/CLAUDE.md\""}"#,
            Some("/work/proj"),
        )
        .unwrap();
        assert_eq!(v, "/work/proj/CLAUDE.md");
    }

    #[test]
    fn computed_rejects_unknown_chars() {
        let v = resolve_one_variable("computed", r#"{"expr":"foo()"}"#, None);
        assert!(v.is_err());
    }
}

// ── MCP Install (v1.3.0 T4 follow-up) ────────────────────────────────────
//
// Writes an MCP server entry into a runtime's config file.
// Supported runtimes today:
//   - claude  → ~/.claude/settings.json `mcpServers.<name>`
//   - gemini  → ~/.gemini/settings.json `mcpServers.<name>`
//   - codex   → ~/.codex/config.toml [mcp_servers.<name>]
// Unsupported runtimes return a clear error so the UI can fall back to the
// "copy snippet" flow.

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct McpInstallEntry {
    pub name: String,
    pub command: Option<String>,    // for stdio
    pub args: Option<Vec<String>>,  // for stdio
    pub env: Option<HashMap<String, String>>, // for stdio
    pub url: Option<String>,        // for sse/http
    pub transport: String,          // "stdio" | "sse" | "http"
}

fn mcp_settings_path(runtime: &str) -> Result<PathBuf, String> {
    match runtime {
        "claude" => Ok(claude_home().join("settings.json")),
        "gemini" => Ok(gemini_home().join("settings.json")),
        "codex" => {
            let codex_home = PathBuf::from(
                std::env::var("CODEX_HOME")
                    .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()),
            );
            Ok(codex_home.join("config.toml"))
        }
        "openclaw" => {
            let oc_home = PathBuf::from(
                std::env::var("OPENCLAW_HOME")
                    .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()),
            );
            Ok(oc_home.join("openclaw.json"))
        }
        "hermes" => Ok(home_dir().join(".hermes").join("config.yaml")),
        other => Err(format!(
            "Runtime '{}' does not support MCP install yet — copy the snippet manually.",
            other
        )),
    }
}

fn build_mcp_json_value(entry: &McpInstallEntry) -> serde_json::Value {
    if entry.transport == "stdio" {
        let mut obj = serde_json::Map::new();
        if let Some(cmd) = &entry.command {
            obj.insert("command".into(), serde_json::Value::String(cmd.clone()));
        }
        if let Some(args) = &entry.args {
            obj.insert(
                "args".into(),
                serde_json::Value::Array(
                    args.iter().map(|s| serde_json::Value::String(s.clone())).collect(),
                ),
            );
        }
        if let Some(env) = &entry.env {
            let mut env_obj = serde_json::Map::new();
            for (k, v) in env {
                env_obj.insert(k.clone(), serde_json::Value::String(v.clone()));
            }
            obj.insert("env".into(), serde_json::Value::Object(env_obj));
        }
        serde_json::Value::Object(obj)
    } else {
        let mut obj = serde_json::Map::new();
        if let Some(url) = &entry.url {
            obj.insert("url".into(), serde_json::Value::String(url.clone()));
        }
        serde_json::Value::Object(obj)
    }
}

#[tauri::command]
pub fn install_mcp_server(runtime: String, entry: McpInstallEntry) -> Result<String, String> {
    if entry.name.trim().is_empty() {
        return Err("MCP server name cannot be empty".to_string());
    }

    let path = mcp_settings_path(&runtime)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {}", e))?;
    }

    if runtime == "hermes" {
        // YAML path: load (or create), ensure mcp_servers map exists, insert.
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let mut doc: serde_yaml::Value = if existing.trim().is_empty() {
            serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
        } else {
            serde_yaml::from_str(&existing)
                .map_err(|e| format!("Invalid YAML in {:?}: {}", path, e))?
        };

        let map = doc
            .as_mapping_mut()
            .ok_or_else(|| format!("Config root in {:?} must be a mapping", path))?;
        let key = serde_yaml::Value::String("mcp_servers".to_string());
        let servers = map
            .entry(key)
            .or_insert(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        let servers_map = servers
            .as_mapping_mut()
            .ok_or("`mcp_servers` exists but isn't a mapping")?;

        let mut server_map = serde_yaml::Mapping::new();
        if entry.transport == "stdio" {
            if let Some(cmd) = &entry.command {
                server_map.insert(
                    serde_yaml::Value::String("command".into()),
                    serde_yaml::Value::String(cmd.clone()),
                );
            }
            if let Some(args) = &entry.args {
                server_map.insert(
                    serde_yaml::Value::String("args".into()),
                    serde_yaml::Value::Sequence(
                        args.iter().map(|s| serde_yaml::Value::String(s.clone())).collect(),
                    ),
                );
            }
            if let Some(env) = &entry.env {
                let mut env_map = serde_yaml::Mapping::new();
                for (k, v) in env {
                    env_map.insert(
                        serde_yaml::Value::String(k.clone()),
                        serde_yaml::Value::String(v.clone()),
                    );
                }
                server_map.insert(
                    serde_yaml::Value::String("env".into()),
                    serde_yaml::Value::Mapping(env_map),
                );
            }
        } else if let Some(url) = &entry.url {
            server_map.insert(
                serde_yaml::Value::String("url".into()),
                serde_yaml::Value::String(url.clone()),
            );
        }

        servers_map.insert(
            serde_yaml::Value::String(entry.name.clone()),
            serde_yaml::Value::Mapping(server_map),
        );

        let serialized = serde_yaml::to_string(&doc)
            .map_err(|e| format!("Failed to serialize YAML: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    } else if runtime == "codex" {
        // TOML path: load (or create) the document, add an [mcp_servers.<name>] table.
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let mut doc: toml::Value = if existing.trim().is_empty() {
            toml::Value::Table(toml::value::Table::new())
        } else {
            toml::from_str(&existing).map_err(|e| format!("Invalid TOML in {:?}: {}", path, e))?
        };

        let table = doc.as_table_mut().ok_or("Codex config root must be a table")?;
        let servers = table
            .entry("mcp_servers".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        let servers_table = servers
            .as_table_mut()
            .ok_or("`mcp_servers` already exists but is not a table")?;

        let mut server_table = toml::value::Table::new();
        if entry.transport == "stdio" {
            if let Some(cmd) = &entry.command {
                server_table.insert("command".into(), toml::Value::String(cmd.clone()));
            }
            if let Some(args) = &entry.args {
                server_table.insert(
                    "args".into(),
                    toml::Value::Array(args.iter().map(|s| toml::Value::String(s.clone())).collect()),
                );
            }
            if let Some(env) = &entry.env {
                let mut env_table = toml::value::Table::new();
                for (k, v) in env {
                    env_table.insert(k.clone(), toml::Value::String(v.clone()));
                }
                server_table.insert("env".into(), toml::Value::Table(env_table));
            }
        } else if let Some(url) = &entry.url {
            server_table.insert("url".into(), toml::Value::String(url.clone()));
        }
        servers_table.insert(entry.name.clone(), toml::Value::Table(server_table));

        let serialized = toml::to_string_pretty(&doc)
            .map_err(|e| format!("Failed to serialize TOML: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    } else {
        // JSON path: load (or create) the document, ensure mcpServers exists, add entry.
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let mut doc: serde_json::Value = if existing.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&existing).map_err(|e| format!("Invalid JSON in {:?}: {}", path, e))?
        };

        if !doc.is_object() {
            return Err(format!("Config root in {:?} must be an object", path));
        }
        let obj = doc.as_object_mut().unwrap();
        if !obj.contains_key("mcpServers") {
            obj.insert("mcpServers".into(), serde_json::json!({}));
        }
        let servers = obj
            .get_mut("mcpServers")
            .and_then(|v| v.as_object_mut())
            .ok_or("`mcpServers` already exists but is not an object")?;
        servers.insert(entry.name.clone(), build_mcp_json_value(&entry));

        let serialized = serde_json::to_string_pretty(&doc)
            .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    }

    Ok(path.to_string_lossy().to_string())
}

/// Counterpart to `install_mcp_server` — drops the named server from the
/// runtime's config file. Felipe's feedback: "preciso de uma opcao de
/// editar ou deletar os mcps". For edit we just delete + reinstall from
/// the frontend; both flows go through here.
#[tauri::command]
pub fn uninstall_mcp_server(runtime: String, name: String) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("MCP server name cannot be empty".to_string());
    }

    let path = mcp_settings_path(&runtime)?;
    if !path.exists() {
        return Ok(path.to_string_lossy().to_string()); // nothing to remove
    }

    if runtime == "hermes" {
        let existing = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
        let mut doc: serde_yaml::Value = serde_yaml::from_str(&existing)
            .map_err(|e| format!("Invalid YAML in {:?}: {}", path, e))?;
        if let Some(map) = doc.as_mapping_mut() {
            if let Some(serde_yaml::Value::Mapping(servers)) =
                map.get_mut(serde_yaml::Value::String("mcp_servers".into()))
            {
                servers.remove(&serde_yaml::Value::String(name.clone()));
            }
        }
        let serialized = serde_yaml::to_string(&doc)
            .map_err(|e| format!("Failed to serialize YAML: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    } else if runtime == "codex" {
        let existing = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
        let mut doc: toml::Value = toml::from_str(&existing)
            .map_err(|e| format!("Invalid TOML in {:?}: {}", path, e))?;
        if let Some(table) = doc.as_table_mut() {
            if let Some(servers) = table.get_mut("mcp_servers").and_then(|v| v.as_table_mut()) {
                servers.remove(&name);
            }
        }
        let serialized = toml::to_string_pretty(&doc)
            .map_err(|e| format!("Failed to serialize TOML: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    } else {
        // JSON path (claude, gemini, openclaw)
        let existing = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
        let mut doc: serde_json::Value = serde_json::from_str(&existing)
            .map_err(|e| format!("Invalid JSON in {:?}: {}", path, e))?;
        if let Some(obj) = doc.as_object_mut() {
            if let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                servers.remove(&name);
            }
        }
        let serialized = serde_json::to_string_pretty(&doc)
            .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
        write_with_perm_hint(&path, serialized.as_bytes())?;
    }

    Ok(path.to_string_lossy().to_string())
}

/// Wrap fs::write so the error spells out the failing path AND, on
/// permission denied, points users at the most likely cause (Felipe ran
/// into this on WSL). This is what made his marketplace installs fail
/// silently — the error was buried under "Failed to write" with no
/// actionable guidance.
fn write_with_perm_hint(path: &PathBuf, content: &[u8]) -> Result<(), String> {
    match fs::write(path, content) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Err(format!(
            "Permission denied writing {:?}. On WSL/Linux this usually means the file is owned by another user (e.g., root) — try `sudo chown $USER {:?}` or delete the file so ATO can recreate it.",
            path, path
        )),
        Err(e) => Err(format!("Failed to write {:?}: {}", path, e)),
    }
}

#[cfg(test)]
mod mcp_install_tests {
    use super::*;

    #[test]
    fn unsupported_runtime_errors() {
        // Now we support claude / codex / gemini / openclaw / hermes.
        assert!(mcp_settings_path("hermes").is_ok());
        assert!(mcp_settings_path("openclaw").is_ok());
        assert!(mcp_settings_path("nonsense-runtime").is_err());
    }

    #[test]
    fn build_stdio_value_has_command_args() {
        let entry = McpInstallEntry {
            name: "fs".into(),
            command: Some("npx".into()),
            args: Some(vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into()]),
            env: None,
            url: None,
            transport: "stdio".into(),
        };
        let v = build_mcp_json_value(&entry);
        assert_eq!(v["command"], "npx");
        assert_eq!(v["args"][0], "-y");
    }
}

#[cfg(test)]
mod agent_tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("PR Reviewer"), "pr-reviewer");
        assert_eq!(slugify("My Agent!!"), "my-agent");
        assert_eq!(slugify("  spaced   out  "), "spaced-out");
        assert_eq!(slugify("---weird---"), "weird");
    }

    #[test]
    fn claude_path_uses_md_file() {
        let p = agent_file_path("claude", "pr-reviewer").unwrap();
        let s = p.to_string_lossy();
        assert!(s.ends_with(".claude/agents/pr-reviewer.md"));
    }

    #[test]
    fn codex_path_uses_agents_md() {
        let p = agent_file_path("codex", "doc-writer").unwrap();
        let s = p.to_string_lossy();
        assert!(s.ends_with(".codex/agents/doc-writer/AGENTS.md"));
    }

    #[test]
    fn unsupported_runtime_errors() {
        assert!(agent_file_path("nonsense", "x").is_err());
    }

    #[test]
    fn render_claude_agent_includes_frontmatter() {
        let a = Agent {
            id: "test".into(),
            slug: "pr-reviewer".into(),
            display_name: "PR Reviewer".into(),
            description: Some("Reviews PRs".into()),
            runtime: "claude".into(),
            model: Some("claude-sonnet-4-6".into()),
            project_id: None,
            system_prompt: Some("You review pull requests.".into()),
            permissions: None,
            skills: None,
            mcps: None,
            goal: None,
            file_path: None,
            created_at: "2026-04-30T00:00:00Z".into(),
            last_used_at: None,
            role_models: None,
            memory_policy: None,
            kind: Some("internal".into()),
        };
        let out = render_claude_agent(&a);
        assert!(out.contains("name: pr-reviewer"));
        assert!(out.contains("description: Reviews PRs"));
        assert!(out.contains("model: claude-sonnet-4-6"));
        assert!(out.contains("# PR Reviewer"));
        assert!(out.contains("You review pull requests."));
    }
}

// ── Agent Groups (v1.4.0 F4) ─────────────────────────────────────────────
//
// Multi-agent groups. The article's headline pattern: instead of one agent
// with 30 tools, you have a router that dispatches to N specialized children
// with 5-8 tools each. ATO stores group metadata in SQLite (`agent_groups`
// + `agent_group_members`) AND mirrors it to a portable file at
// `~/.ato/groups/<slug>/group.json` so groups can be shared, version-
// controlled, and discovered by the standalone MCP server.

fn group_file_path(slug: &str) -> PathBuf {
    home_dir().join(".ato").join("groups").join(slug).join("group.json")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupMemberInput {
    /// Slug of an existing agent. We look up the id by slug at save time.
    pub agent_slug: String,
    pub role: String, // "router" | "child"
    pub position: i32,
}

fn load_group_members(conn: &Connection, group_id: &str) -> Vec<AgentGroupMember> {
    let mut stmt = match conn.prepare(
        "SELECT m.agent_id, a.slug, a.display_name, m.role, m.position, a.runtime
         FROM agent_group_members m
         JOIN agents a ON a.id = m.agent_id
         WHERE m.group_id = ?1
         ORDER BY m.position ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(params![group_id], |row| {
        Ok(AgentGroupMember {
            agent_id: row.get(0)?,
            agent_slug: row.get(1)?,
            agent_display_name: row.get(2)?,
            role: row.get(3)?,
            position: row.get(4)?,
            agent_runtime: row.get(5)?,
        })
    }) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.flatten().collect()
}

fn write_group_file(group: &AgentGroup) -> Result<String, String> {
    let path = group_file_path(&group.slug);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create groups dir: {}", e))?;
    }
    let snapshot = serde_json::json!({
        "slug": group.slug,
        "displayName": group.display_name,
        "description": group.description,
        "runtime": group.runtime,
        "routerConfig": group.router_config
            .as_ref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .unwrap_or_else(|| serde_json::json!({})),
        "members": group.members.iter().map(|m| serde_json::json!({
            "agent": m.agent_slug,
            "role": m.role,
            "position": m.position,
        })).collect::<Vec<_>>(),
    });
    let serialized = serde_json::to_string_pretty(&snapshot)
        .map_err(|e| format!("Failed to serialize group: {}", e))?;
    fs::write(&path, serialized).map_err(|e| format!("Failed to write group file: {}", e))?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn create_agent_group(
    db: State<'_, DbState>,
    display_name: String,
    runtime: String,
    description: Option<String>,
    router_config_json: Option<String>,
    members: Vec<GroupMemberInput>,
    // "routed" (default — router picks one child) or "sequential" (children
    // run in order; previous output flows into next input).
    dispatch_kind: Option<String>,
) -> Result<AgentGroup, String> {
    if display_name.trim().is_empty() {
        return Err("display_name cannot be empty".into());
    }
    let allowed_runtimes = ["claude", "codex", "gemini", "openclaw", "hermes"];
    if !allowed_runtimes.contains(&runtime.as_str()) {
        return Err(format!("Unsupported runtime: {}", runtime));
    }
    let dispatch_kind = dispatch_kind.unwrap_or_else(|| "routed".to_string());
    if dispatch_kind != "routed" && dispatch_kind != "sequential" {
        return Err(format!("Unsupported dispatch_kind: {}", dispatch_kind));
    }
    if let Some(ref cfg) = router_config_json {
        serde_json::from_str::<serde_json::Value>(cfg)
            .map_err(|e| format!("Invalid router_config JSON: {}", e))?;
    }

    let slug = slugify(&display_name);
    if slug.is_empty() {
        return Err("display_name must produce a non-empty slug".into());
    }

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // Resolve member slugs → agent IDs. Must all exist; runtime must match.
    let mut resolved_members: Vec<AgentGroupMember> = Vec::new();
    for m in &members {
        let row = conn.query_row(
            "SELECT id, slug, display_name, runtime FROM agents WHERE slug = ?1",
            params![m.agent_slug],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        );
        match row {
            Ok((agent_id, slug_, display, agent_runtime)) => {
                // Routed groups: router runs once on group.runtime, so all
                //   children MUST share that runtime.
                // Sequential groups: each child runs on its OWN runtime in
                //   turn, so cross-runtime pipelines (Claude → Codex) work.
                if dispatch_kind != "sequential" && agent_runtime != runtime {
                    return Err(format!(
                        "Member '{}' uses runtime '{}', but group runtime is '{}'",
                        slug_, agent_runtime, runtime
                    ));
                }
                resolved_members.push(AgentGroupMember {
                    agent_id,
                    agent_slug: slug_,
                    agent_display_name: display,
                    role: m.role.clone(),
                    position: m.position,
                    agent_runtime: agent_runtime.clone(),
                });
            }
            Err(_) => return Err(format!("Agent with slug '{}' not found", m.agent_slug)),
        }
    }

    // Insert group + members atomically.
    let tx_result: Result<(), String> = (|| {
        conn.execute(
            "INSERT INTO agent_groups (id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, NULL, ?8)",
            params![id, slug, display_name, description, runtime, router_config_json, now, dispatch_kind],
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                format!("A group named \"{}\" already exists", slug)
            } else {
                msg
            }
        })?;

        for m in &resolved_members {
            conn.execute(
                "INSERT INTO agent_group_members (group_id, agent_id, role, position)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, m.agent_id, m.role, m.position],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();

    if let Err(e) = tx_result {
        // Best-effort rollback by deleting partial state.
        let _ = conn.execute("DELETE FROM agent_groups WHERE id = ?1", params![id]);
        return Err(e);
    }

    let mut group = AgentGroup {
        id: id.clone(),
        slug,
        display_name,
        description,
        runtime,
        router_config: router_config_json,
        file_path: None,
        created_at: now,
        last_used_at: None,
        members: resolved_members,
        dispatch_kind,
    };

    // Persist the file mirror; non-fatal on failure (agent still works in-DB).
    match write_group_file(&group) {
        Ok(path) => {
            group.file_path = Some(path.clone());
            let _ = conn.execute(
                "UPDATE agent_groups SET file_path = ?1 WHERE id = ?2",
                params![path, id],
            );
        }
        Err(e) => eprintln!("write_group_file: {}", e),
    }

    Ok(group)
}

#[tauri::command]
pub fn list_agent_groups(
    db: State<'_, DbState>,
    runtime: Option<String>,
) -> Result<Vec<AgentGroup>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let (sql, has_filter) = if runtime.is_some() {
        (
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind
             FROM agent_groups WHERE runtime = ?1
             ORDER BY COALESCE(last_used_at, created_at) DESC".to_string(),
            true,
        )
    } else {
        (
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind
             FROM agent_groups
             ORDER BY COALESCE(last_used_at, created_at) DESC".to_string(),
            false,
        )
    };
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let row_to_group = |row: &rusqlite::Row| -> rusqlite::Result<AgentGroup> {
        Ok(AgentGroup {
            id: row.get(0)?,
            slug: row.get(1)?,
            display_name: row.get(2)?,
            description: row.get(3)?,
            runtime: row.get(4)?,
            router_config: row.get(5)?,
            file_path: row.get(6)?,
            created_at: row.get(7)?,
            last_used_at: row.get(8)?,
            dispatch_kind: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "routed".to_string()),
            members: Vec::new(), // filled in below
        })
    };
    let mut groups: Vec<AgentGroup> = if has_filter {
        let r = runtime.unwrap();
        stmt.query_map(params![r], row_to_group)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    } else {
        stmt.query_map([], row_to_group)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };

    for g in &mut groups {
        g.members = load_group_members(&conn, &g.id);
    }
    Ok(groups)
}

#[tauri::command]
pub fn get_agent_group(db: State<'_, DbState>, slug: String) -> Result<AgentGroup, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut group = conn
        .query_row(
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind
             FROM agent_groups WHERE slug = ?1",
            params![slug],
            |row| {
                Ok(AgentGroup {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    display_name: row.get(2)?,
                    description: row.get(3)?,
                    runtime: row.get(4)?,
                    router_config: row.get(5)?,
                    file_path: row.get(6)?,
                    created_at: row.get(7)?,
                    last_used_at: row.get(8)?,
                    dispatch_kind: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "routed".to_string()),
                    members: Vec::new(),
                })
            },
        )
        .map_err(|e| e.to_string())?;
    group.members = load_group_members(&conn, &group.id);
    Ok(group)
}

#[tauri::command]
pub fn update_agent_group(
    db: State<'_, DbState>,
    id: String,
    description: Option<String>,
    router_config_json: Option<String>,
    members: Option<Vec<GroupMemberInput>>,
) -> Result<AgentGroup, String> {
    if let Some(ref cfg) = router_config_json {
        serde_json::from_str::<serde_json::Value>(cfg)
            .map_err(|e| format!("Invalid router_config JSON: {}", e))?;
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Fetch the existing group to know runtime/slug for member resolution.
    let (group_runtime, group_slug): (String, String) = conn.query_row(
        "SELECT runtime, slug FROM agent_groups WHERE id = ?1",
        params![id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    ).map_err(|e| e.to_string())?;

    if let Some(desc) = &description {
        conn.execute(
            "UPDATE agent_groups SET description = ?1 WHERE id = ?2",
            params![desc, id],
        ).map_err(|e| e.to_string())?;
    }
    if let Some(cfg) = &router_config_json {
        conn.execute(
            "UPDATE agent_groups SET router_config = ?1 WHERE id = ?2",
            params![cfg, id],
        ).map_err(|e| e.to_string())?;
    }
    if let Some(new_members) = &members {
        // Replace member list atomically.
        conn.execute("DELETE FROM agent_group_members WHERE group_id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        for m in new_members {
            let agent_row = conn.query_row(
                "SELECT id, runtime FROM agents WHERE slug = ?1",
                params![m.agent_slug],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            );
            match agent_row {
                Ok((agent_id, agent_runtime)) => {
                    if agent_runtime != group_runtime {
                        return Err(format!(
                            "Member '{}' uses runtime '{}', but group runtime is '{}'",
                            m.agent_slug, agent_runtime, group_runtime
                        ));
                    }
                    conn.execute(
                        "INSERT INTO agent_group_members (group_id, agent_id, role, position)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![id, agent_id, m.role, m.position],
                    ).map_err(|e| e.to_string())?;
                }
                Err(_) => return Err(format!("Agent with slug '{}' not found", m.agent_slug)),
            }
        }
    }

    drop(conn);
    // Re-read the group + members through the public command so the file
    // mirror always reflects the freshly-saved state.
    let _ = group_slug; // borrowed only for clarity; not used further.
    let group = get_agent_group(db, group_slug.clone())?;
    let _ = write_group_file(&group);
    Ok(group)
}

#[tauri::command]
pub fn delete_agent_group(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // Look up the slug so we can clean up the file mirror.
    if let Ok(slug) = conn.query_row(
        "SELECT slug FROM agent_groups WHERE id = ?1",
        params![id],
        |r| r.get::<_, String>(0),
    ) {
        let path = group_file_path(&slug);
        let _ = fs::remove_file(&path);
        // Best-effort prune of the parent directory if empty.
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
    conn.execute("DELETE FROM agent_groups WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Router execution (v1.4.0 F4 — dispatch_to_group) ─────────────────────

/// Decide which child agent a prompt should route to. Two-stage:
///   1. Apply rules (declarative, fast, cheap, predictable).
///   2. If no rule matches AND llmFallback is enabled, ask the runtime's
///      cheap classifier model to pick a child.
/// Returns (chosen_child_slug, routing_reason).
async fn route_prompt_to_child(
    group: &AgentGroup,
    prompt: &str,
) -> Result<(String, String), String> {
    let cfg: serde_json::Value = group
        .router_config
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let children: Vec<&AgentGroupMember> = group
        .members
        .iter()
        .filter(|m| m.role == "child")
        .collect();
    if children.is_empty() {
        return Err("Group has no children to route to".into());
    }

    // Stage 1: rules.
    if let Some(rules) = cfg.get("rules").and_then(|r| r.as_array()) {
        let lower = prompt.to_lowercase();
        for rule in rules {
            let then_slug = rule.get("then").and_then(|v| v.as_str()).unwrap_or("");
            let if_block = rule.get("if").cloned().unwrap_or_else(|| serde_json::json!({}));
            // keyword match (any of the listed strings)
            if let Some(keywords) = if_block.get("keyword").and_then(|v| v.as_array()) {
                for kw in keywords {
                    if let Some(s) = kw.as_str() {
                        if !s.is_empty() && lower.contains(&s.to_lowercase()) {
                            // Verify the child exists in this group.
                            if children.iter().any(|c| c.agent_slug == then_slug) {
                                return Ok((
                                    then_slug.to_string(),
                                    format!("rule: keyword '{}' matched", s),
                                ));
                            }
                        }
                    }
                }
            }
            // regex match
            if let Some(pattern) = if_block.get("regex").and_then(|v| v.as_str()) {
                // Tiny shim: use the same single-pass approach as substitute_variables
                // to avoid a regex dep — only supports literal substring for now.
                // (Wave 3.2 will add proper regex.)
                if !pattern.is_empty() && prompt.contains(pattern) {
                    if children.iter().any(|c| c.agent_slug == then_slug) {
                        return Ok((
                            then_slug.to_string(),
                            format!("rule: pattern '{}' matched (literal)", pattern),
                        ));
                    }
                }
            }
        }
    }

    // Stage 2: LLM fallback.
    let llm_fb = cfg.get("llmFallback");
    let llm_enabled = llm_fb
        .and_then(|v| v.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if llm_enabled {
        let descriptions: Vec<String> = children
            .iter()
            .map(|c| format!("- {}: {}", c.agent_slug, c.agent_display_name))
            .collect();
        let classifier_prompt = format!(
            "You are a router. Pick the single agent slug that should handle the user's message.\n\
             Available agents:\n{}\n\
             User message: {}\n\
             Reply with ONLY the slug — nothing else.",
            descriptions.join("\n"),
            prompt
        );
        // Reuse prompt_agent on the group's runtime.
        match prompt_agent(group.runtime.clone(), classifier_prompt, None).await {
            Ok(reply) => {
                let pick = reply.trim().lines().next().unwrap_or("").trim().to_string();
                if let Some(matched) =
                    children.iter().find(|c| c.agent_slug == pick).map(|c| c.agent_slug.clone())
                {
                    return Ok((matched, "llm-fallback".to_string()));
                }
                // Classifier returned nothing useful; fall through to default.
            }
            Err(e) => {
                eprintln!("router LLM fallback failed: {}", e);
            }
        }
    }

    // Default: first child.
    let first = children[0].agent_slug.clone();
    Ok((first, "default: first child".into()))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupStageResult {
    pub agent_slug: String,
    pub runtime: String,
    pub response: String,
    pub ok: bool,
    /// v2.1.0 Phase 7 — start time of this stage (ISO 8601 UTC).
    /// Frontend uses it to upload one trace per stage with the correct
    /// per-stage timing rather than approximating from the group total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// Wall-clock duration of this stage in ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Error string when ok=false. Lets the frontend upload a precise
    /// per-stage error instead of repeating the rolled-up group error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupDispatchResult {
    /// Stitched transcript of all stages (or single response for routed
    /// groups). Frontend may render this OR walk `stages` to render each
    /// stage as its own message.
    pub response: String,
    pub routed_to: String,
    pub routing_reason: String,
    /// One entry per stage. Routed groups have exactly one; sequential
    /// groups have one per child in pipeline order.
    #[serde(default)]
    pub stages: Vec<GroupStageResult>,
}

/// Tauri command: dispatch a prompt through a group's router.
#[tauri::command]
pub async fn dispatch_to_group(
    db: State<'_, DbState>,
    slug: String,
    prompt: String,
    config: Option<String>,
    active_project_path: Option<String>,
) -> Result<GroupDispatchResult, String> {
    // Load the group once (under a short-lived lock).
    let group = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mut group = conn.query_row(
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind
             FROM agent_groups WHERE slug = ?1",
            params![slug],
            |row| {
                Ok(AgentGroup {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    display_name: row.get(2)?,
                    description: row.get(3)?,
                    runtime: row.get(4)?,
                    router_config: row.get(5)?,
                    file_path: row.get(6)?,
                    created_at: row.get(7)?,
                    last_used_at: row.get(8)?,
                    dispatch_kind: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "routed".to_string()),
                    members: Vec::new(),
                })
            },
        ).map_err(|e| format!("Group '{}' not found: {}", slug, e))?;
        group.members = load_group_members(&conn, &group.id);
        group
    };

    // Branch on dispatch kind. Sequential walks every child in position
    // order, feeding the previous output as input to the next; final
    // response is a stitched transcript so the user sees each stage.
    if group.dispatch_kind == "sequential" {
        return run_sequential_dispatch(&group, &prompt, config.as_deref()).await;
    }

    // Routed (default): router picks a single child.
    let (child_slug, reason) = route_prompt_to_child(&group, &prompt).await?;

    // Find the child agent's id so we can use prompt_agent_with_context.
    let child_agent_id = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id FROM agents WHERE slug = ?1",
            params![child_slug],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| format!("Child agent '{}' not found: {}", child_slug, e))?
    };

    // Resolve variables + run hooks for the child + dispatch. Group
    // dispatch only needs the response string — the run_id from the
    // DispatchResult is consumed by the FRONTEND wrappers, not here.
    let response = prompt_agent_with_context(
        db.clone(),
        child_agent_id,
        group.runtime.clone(),
        prompt,
        config,
        active_project_path,
    )
    .await?
    .response;

    // Bump last_used_at.
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let _ = conn.execute(
            "UPDATE agent_groups SET last_used_at = ?1 WHERE id = ?2",
            params![now, group.id],
        );
    }

    Ok(GroupDispatchResult {
        response: response.clone(),
        routed_to: child_slug.clone(),
        routing_reason: reason,
        stages: vec![GroupStageResult {
            agent_slug: child_slug,
            runtime: group.runtime.clone(),
            response,
            ok: true,
            started_at: None,
            duration_ms: None,
            error: None,
        }],
    })
}

/// Sequential / "automation" dispatch: walk children in `position` order,
/// feed the prompt to the first child, then feed each output as input to
/// the next. Returns a stitched transcript so the user sees what each stage
/// produced.
async fn run_sequential_dispatch(
    group: &AgentGroup,
    user_prompt: &str,
    config: Option<&str>,
) -> Result<GroupDispatchResult, String> {
    let mut children: Vec<&AgentGroupMember> = group
        .members
        .iter()
        .filter(|m| m.role == "child")
        .collect();
    children.sort_by_key(|m| m.position);

    if children.is_empty() {
        return Err("Sequential group has no children".into());
    }

    let mut transcript = String::new();
    let mut stage_results: Vec<GroupStageResult> = Vec::new();
    let mut last_output = user_prompt.to_string();

    for (i, child) in children.iter().enumerate() {
        let stage_prompt = if i == 0 {
            user_prompt.to_string()
        } else {
            format!(
                "Previous step produced this output:\n\n{}\n\n---\n\nOriginal task: {}\n\nYour task: act on the previous output per your instructions.",
                last_output, user_prompt
            )
        };

        // Each child runs on its OWN runtime. Sequential groups can chain
        // Claude → Codex → Gemini etc. — that's the whole point.
        let child_runtime = if child.agent_runtime.is_empty() {
            group.runtime.clone()
        } else {
            child.agent_runtime.clone()
        };
        let stage_start = std::time::Instant::now();
        let stage_started_at = chrono::Utc::now().to_rfc3339();
        let (stage_response, ok, stage_error) = match prompt_agent(
            child_runtime.clone(),
            stage_prompt,
            config.map(|s| s.to_string()),
        )
        .await
        {
            Ok(r) => (r, true, None),
            Err(e) => (
                format!("(stage '{}' on {} failed: {})", child.agent_slug, child_runtime, e),
                false,
                Some(e),
            ),
        };
        let stage_duration_ms = stage_start.elapsed().as_millis() as u64;

        if !transcript.is_empty() {
            transcript.push_str("\n\n---\n\n");
        }
        transcript.push_str(&format!(
            "**@{}** _({})_\n\n{}",
            child.agent_slug, child_runtime, stage_response
        ));
        stage_results.push(GroupStageResult {
            agent_slug: child.agent_slug.clone(),
            runtime: child_runtime,
            response: stage_response.clone(),
            ok,
            started_at: Some(stage_started_at),
            duration_ms: Some(stage_duration_ms),
            error: stage_error,
        });
        last_output = stage_response;
    }

    let stage_labels: Vec<String> = stage_results
        .iter()
        .map(|s| format!("{} ({})", s.agent_slug, s.runtime))
        .collect();
    let routed_to = children.last().map(|c| c.agent_slug.clone()).unwrap_or_default();
    let routing_reason = format!("Sequential pipeline: {}", stage_labels.join(" → "));

    Ok(GroupDispatchResult {
        response: transcript,
        routed_to,
        routing_reason,
        stages: stage_results,
    })
}

// ── Agent Observability (v1.4.0 F6) ──────────────────────────────────────
//
// Reads `~/.ato/agent-logs.jsonl` — the unified trace log every dispatch path
// (desktop Run button, Quick Test, MCP run_agent, group routing, cron jobs)
// appends to. Surfaces metrics + per-trace details for the Insights panel.

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentTraceLine {
    pub ts: Option<String>,
    pub duration_ms: Option<i64>,
    pub runtime: Option<String>,
    pub slug: Option<String>,
    pub file_path: Option<String>,
    pub prompt_preview: Option<String>,
    pub response_preview: Option<String>,
    pub ok: Option<bool>,
    pub error: Option<String>,
    pub source: Option<String>,
    /// Set when this dispatch was a group routed through its router (F4).
    pub routed_to: Option<String>,
    /// Future fields land here without breaking the type.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentTraceFilter {
    pub agent_slug: Option<String>,
    pub runtime: Option<String>,
    /// "ok" | "error" | "all" (default all).
    pub status: Option<String>,
    /// ISO-8601; only return traces with `ts >= since`.
    pub since: Option<String>,
    /// Hard cap to avoid pulling huge files.
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentMetrics {
    pub total_runs: usize,
    pub successful: usize,
    pub failed: usize,
    pub success_rate: f64,
    pub p50_latency_ms: Option<i64>,
    pub p95_latency_ms: Option<i64>,
    pub avg_latency_ms: Option<i64>,
    /// Per-agent breakdown so the dashboard can render a list. Sorted by
    /// most-recent-first.
    pub per_agent: Vec<PerAgentMetrics>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PerAgentMetrics {
    pub slug: String,
    pub runtime: Option<String>,
    pub total_runs: usize,
    pub successful: usize,
    pub failed: usize,
    pub success_rate: f64,
    pub p50_latency_ms: Option<i64>,
    pub p95_latency_ms: Option<i64>,
    pub last_run_at: Option<String>,
}

fn load_agent_log_lines(filter: &AgentTraceFilter) -> Vec<AgentTraceLine> {
    let path = home_dir().join(".ato").join("agent-logs.jsonl");
    if !path.exists() {
        return Vec::new();
    }
    let content = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut all: Vec<AgentTraceLine> = content
        .lines()
        .rev() // newest first (file is append-only)
        .filter_map(|line| serde_json::from_str::<AgentTraceLine>(line).ok())
        .collect();

    // Apply filters in-place.
    if let Some(slug) = &filter.agent_slug {
        all.retain(|t| t.slug.as_deref() == Some(slug));
    }
    if let Some(runtime) = &filter.runtime {
        all.retain(|t| t.runtime.as_deref() == Some(runtime));
    }
    if let Some(status) = &filter.status {
        match status.as_str() {
            "ok" => all.retain(|t| t.ok == Some(true)),
            "error" => all.retain(|t| t.ok == Some(false)),
            _ => {} // "all" or unknown → keep
        }
    }
    if let Some(since) = &filter.since {
        all.retain(|t| t.ts.as_deref().map(|ts| ts >= since.as_str()).unwrap_or(false));
    }
    if let Some(limit) = filter.limit {
        if all.len() > limit {
            all.truncate(limit);
        }
    }
    all
}

fn percentile(sorted: &[i64], pct: f64) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let idx = ((sorted.len() as f64 - 1.0) * pct).round() as usize;
    sorted.get(idx).copied()
}

#[tauri::command]
pub fn read_agent_traces(filter: AgentTraceFilter) -> Result<Vec<AgentTraceLine>, String> {
    Ok(load_agent_log_lines(&filter))
}

#[tauri::command]
pub fn get_agent_metrics(filter: AgentTraceFilter) -> Result<AgentMetrics, String> {
    // For aggregations we want every line that matches the runtime/status/
    // since filters but ignoring `limit` so totals are accurate.
    let aggregate_filter = AgentTraceFilter {
        agent_slug: filter.agent_slug.clone(),
        runtime: filter.runtime.clone(),
        status: filter.status.clone(),
        since: filter.since.clone(),
        limit: None,
    };
    let lines = load_agent_log_lines(&aggregate_filter);

    let total = lines.len();
    let mut successful = 0usize;
    let mut failed = 0usize;
    let mut latencies: Vec<i64> = Vec::with_capacity(lines.len());

    // Per-agent rollups
    let mut per_agent_map: HashMap<String, PerAgentRollup> = HashMap::new();

    for t in &lines {
        match t.ok {
            Some(true) => successful += 1,
            Some(false) => failed += 1,
            None => {}
        }
        if let Some(d) = t.duration_ms {
            latencies.push(d);
        }

        if let Some(slug) = &t.slug {
            let entry = per_agent_map
                .entry(slug.clone())
                .or_insert_with(|| PerAgentRollup {
                    slug: slug.clone(),
                    runtime: t.runtime.clone(),
                    total: 0,
                    successful: 0,
                    failed: 0,
                    latencies: Vec::new(),
                    last_run: None,
                });
            entry.total += 1;
            match t.ok {
                Some(true) => entry.successful += 1,
                Some(false) => entry.failed += 1,
                None => {}
            }
            if let Some(d) = t.duration_ms {
                entry.latencies.push(d);
            }
            if let Some(ts) = &t.ts {
                entry.last_run = Some(match &entry.last_run {
                    Some(prev) if prev > ts => prev.clone(),
                    _ => ts.clone(),
                });
            }
        }
    }

    latencies.sort_unstable();
    let avg_latency_ms = if latencies.is_empty() {
        None
    } else {
        Some(latencies.iter().sum::<i64>() / latencies.len() as i64)
    };

    let mut per_agent: Vec<PerAgentMetrics> = per_agent_map
        .into_values()
        .map(|mut r| {
            r.latencies.sort_unstable();
            PerAgentMetrics {
                slug: r.slug,
                runtime: r.runtime,
                total_runs: r.total,
                successful: r.successful,
                failed: r.failed,
                success_rate: if r.total == 0 { 0.0 } else { r.successful as f64 / r.total as f64 },
                p50_latency_ms: percentile(&r.latencies, 0.5),
                p95_latency_ms: percentile(&r.latencies, 0.95),
                last_run_at: r.last_run,
            }
        })
        .collect();
    // Most-recent-first.
    per_agent.sort_by(|a, b| b.last_run_at.cmp(&a.last_run_at));

    Ok(AgentMetrics {
        total_runs: total,
        successful,
        failed,
        success_rate: if total == 0 { 0.0 } else { successful as f64 / total as f64 },
        p50_latency_ms: percentile(&latencies, 0.5),
        p95_latency_ms: percentile(&latencies, 0.95),
        avg_latency_ms,
        per_agent,
    })
}

struct PerAgentRollup {
    slug: String,
    runtime: Option<String>,
    total: usize,
    successful: usize,
    failed: usize,
    latencies: Vec<i64>,
    last_run: Option<String>,
}

// ── Evaluators (v1.4.0 F7 — heuristic only in this wave; LLM-as-judge in
//    Wave 4.5) ────────────────────────────────────────────────────────────
//
// Evaluators answer "did this run succeed?" as code or as a small LLM call.
// Stored in agent_evaluators (new table — added in init_database below
// idempotently). Heuristic evaluators run locally; LLM-as-judge runs through
// `prompt_agent` with a cheap model. Manual + scheduled batch only — never
// live on every dispatch.

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvaluator {
    pub id: String,
    pub agent_slug: String,
    pub name: String,
    pub kind: String, // 'contains' | 'not-contains' | 'length-range' | 'tool-called' | 'llm-judge'
    pub config_json: String,
    pub enabled: bool,
    pub created_at: String,
}

fn ensure_evaluator_table(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_evaluators (
            id          TEXT PRIMARY KEY,
            agent_slug  TEXT NOT NULL,
            name        TEXT NOT NULL,
            kind        TEXT NOT NULL,
            config_json TEXT NOT NULL,
            enabled     INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_evaluators_slug ON agent_evaluators(agent_slug);",
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_agent_evaluators(
    db: State<'_, DbState>,
    agent_slug: String,
) -> Result<Vec<AgentEvaluator>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    ensure_evaluator_table(&conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, agent_slug, name, kind, config_json, enabled, created_at
             FROM agent_evaluators WHERE agent_slug = ?1 ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![agent_slug], |row| {
            Ok(AgentEvaluator {
                id: row.get(0)?,
                agent_slug: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                config_json: row.get(4)?,
                enabled: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_agent_evaluator(
    db: State<'_, DbState>,
    id: Option<String>,
    agent_slug: String,
    name: String,
    kind: String,
    config_json: String,
    enabled: Option<bool>,
) -> Result<AgentEvaluator, String> {
    let allowed = ["contains", "not-contains", "length-range", "tool-called", "llm-judge"];
    if !allowed.contains(&kind.as_str()) {
        return Err(format!("Unsupported evaluator kind: {}", kind));
    }
    if name.trim().is_empty() {
        return Err("Evaluator name cannot be empty".into());
    }
    serde_json::from_str::<serde_json::Value>(&config_json)
        .map_err(|e| format!("Invalid evaluator config JSON: {}", e))?;

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    ensure_evaluator_table(&conn)?;
    let now = chrono::Utc::now().to_rfc3339();
    let final_id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let enabled_int: i32 = if enabled.unwrap_or(true) { 1 } else { 0 };

    conn.execute(
        "INSERT INTO agent_evaluators (id, agent_slug, name, kind, config_json, enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           kind = excluded.kind,
           config_json = excluded.config_json,
           enabled = excluded.enabled",
        params![final_id, agent_slug, name, kind, config_json, enabled_int, now],
    )
    .map_err(|e| e.to_string())?;

    Ok(AgentEvaluator {
        id: final_id,
        agent_slug,
        name,
        kind,
        config_json,
        enabled: enabled.unwrap_or(true),
        created_at: now,
    })
}

#[tauri::command]
pub fn delete_agent_evaluator(db: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    ensure_evaluator_table(&conn)?;
    conn.execute("DELETE FROM agent_evaluators WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationResult {
    pub evaluator_id: String,
    pub kind: String,
    pub verdict: String, // "pass" | "fail" | "partial" | "unknown"
    pub score: f64,      // 0.0 – 1.0
    pub reason: String,
}

/// Run an evaluator against a single trace line. Heuristic kinds run locally
/// in Rust; `llm-judge` is stubbed in this wave (returns an "unknown" verdict)
/// because it'd ideally call a Pro cloud endpoint with budget controls.
fn run_evaluator(eval: &AgentEvaluator, trace: &AgentTraceLine) -> EvaluationResult {
    let cfg: serde_json::Value =
        serde_json::from_str(&eval.config_json).unwrap_or_else(|_| serde_json::json!({}));
    let response = trace.response_preview.clone().unwrap_or_default();

    match eval.kind.as_str() {
        "contains" => {
            let needle = cfg.get("needle").and_then(|v| v.as_str()).unwrap_or("");
            let case_sensitive = cfg
                .get("caseSensitive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let hay = if case_sensitive {
                response.clone()
            } else {
                response.to_lowercase()
            };
            let pin = if case_sensitive {
                needle.to_string()
            } else {
                needle.to_lowercase()
            };
            let hit = !needle.is_empty() && hay.contains(&pin);
            EvaluationResult {
                evaluator_id: eval.id.clone(),
                kind: eval.kind.clone(),
                verdict: if hit { "pass".into() } else { "fail".into() },
                score: if hit { 1.0 } else { 0.0 },
                reason: if hit {
                    format!("Response contains '{}'", needle)
                } else {
                    format!("Response missing '{}'", needle)
                },
            }
        }
        "not-contains" => {
            let needle = cfg.get("needle").and_then(|v| v.as_str()).unwrap_or("");
            let case_sensitive = cfg
                .get("caseSensitive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let hay = if case_sensitive {
                response.clone()
            } else {
                response.to_lowercase()
            };
            let pin = if case_sensitive {
                needle.to_string()
            } else {
                needle.to_lowercase()
            };
            let hit = !needle.is_empty() && hay.contains(&pin);
            EvaluationResult {
                evaluator_id: eval.id.clone(),
                kind: eval.kind.clone(),
                verdict: if hit { "fail".into() } else { "pass".into() },
                score: if hit { 0.0 } else { 1.0 },
                reason: if hit {
                    format!("Response contains forbidden '{}'", needle)
                } else {
                    format!("Response correctly omits '{}'", needle)
                },
            }
        }
        "length-range" => {
            let min = cfg.get("min").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let max = cfg
                .get("max")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
                .unwrap_or(usize::MAX);
            let len = response.chars().count();
            let pass = len >= min && len <= max;
            EvaluationResult {
                evaluator_id: eval.id.clone(),
                kind: eval.kind.clone(),
                verdict: if pass { "pass".into() } else { "fail".into() },
                score: if pass { 1.0 } else { 0.0 },
                reason: format!("Response is {} chars (target {}–{})", len, min, max),
            }
        }
        "tool-called" => {
            let tool = cfg.get("tool").and_then(|v| v.as_str()).unwrap_or("");
            let lower = response.to_lowercase();
            let hit = !tool.is_empty() && lower.contains(&tool.to_lowercase());
            EvaluationResult {
                evaluator_id: eval.id.clone(),
                kind: eval.kind.clone(),
                verdict: if hit { "pass".into() } else { "fail".into() },
                score: if hit { 1.0 } else { 0.0 },
                reason: if hit {
                    format!("Response references tool '{}'", tool)
                } else {
                    format!("Response did not invoke tool '{}'", tool)
                },
            }
        }
        "llm-judge" => EvaluationResult {
            evaluator_id: eval.id.clone(),
            kind: eval.kind.clone(),
            verdict: "unknown".into(),
            score: 0.0,
            reason: "LLM-as-judge runs server-side in Wave 4.5 (Pro tier).".into(),
        },
        other => EvaluationResult {
            evaluator_id: eval.id.clone(),
            kind: other.to_string(),
            verdict: "unknown".into(),
            score: 0.0,
            reason: format!("Unknown evaluator kind: {}", other),
        },
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedTrace {
    pub trace: AgentTraceLine,
    pub results: Vec<EvaluationResult>,
}

/// Run all enabled evaluators for an agent against the most-recent N traces
/// for that agent. Used by the dashboard's "Evaluate last N runs" button.
#[tauri::command]
pub fn evaluate_recent_traces(
    db: State<'_, DbState>,
    agent_slug: String,
    last_n: usize,
) -> Result<Vec<EvaluatedTrace>, String> {
    let evaluators: Vec<AgentEvaluator> = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        ensure_evaluator_table(&conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, agent_slug, name, kind, config_json, enabled, created_at
                 FROM agent_evaluators WHERE agent_slug = ?1 AND enabled = 1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![agent_slug], |row| {
                Ok(AgentEvaluator {
                    id: row.get(0)?,
                    agent_slug: row.get(1)?,
                    name: row.get(2)?,
                    kind: row.get(3)?,
                    config_json: row.get(4)?,
                    enabled: row.get::<_, i32>(5)? != 0,
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };

    let traces = load_agent_log_lines(&AgentTraceFilter {
        agent_slug: Some(agent_slug),
        runtime: None,
        status: None,
        since: None,
        limit: Some(last_n),
    });

    let evaluated: Vec<EvaluatedTrace> = traces
        .into_iter()
        .map(|t| EvaluatedTrace {
            results: evaluators.iter().map(|e| run_evaluator(e, &t)).collect(),
            trace: t,
        })
        .collect();

    Ok(evaluated)
}

// ── Streaming dispatch (v1.5.0) ─────────────────────────────────────────
//
// Mirrors prompt_agent / prompt_agent_with_history but streams stdout
// through a Tauri Channel so the chat pane can render tokens as they
// arrive. Each chunk is whatever bytes the CLI flushes; we don't try to
// parse newlines or JSON — that's the runtime's contract.

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StreamEvent {
    Chunk { text: String },
    Done { full: String },
    Error { message: String },
}

/// Stream a single-shot dispatch. Caller must keep the channel alive until
/// it observes a `done` or `error` event.
#[tauri::command]
pub async fn prompt_agent_stream(
    runtime: String,
    prompt: String,
    config: Option<String>,
    on_event: tauri::ipc::Channel<StreamEvent>,
) -> Result<(), String> {
    // Ad-hoc — no agent context. Registry will show "no slug, runtime X".
    spawn_streaming_dispatch(&runtime, &prompt, config.as_deref(), on_event, None, None).await
}

/// Stream a multi-turn dispatch. Resolves variables / hooks / role models
/// up-front (sync work), then streams the response.
#[tauri::command]
pub async fn prompt_agent_with_history_stream(
    db: State<'_, DbState>,
    agent_id: String,
    runtime: String,
    history: Vec<AgentMessage>,
    new_prompt: String,
    config: Option<String>,
    active_project_path: Option<String>,
    on_event: tauri::ipc::Channel<StreamEvent>,
) -> Result<(), String> {
    // Same prelude as prompt_agent_with_history — keep them in sync if you
    // change one, change the other.
    let (resolved, hooks, response_model, fallback_model, policy, summarizer_model, agent_slug) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let resolved = resolve_agent_variables(&conn, &agent_id, active_project_path.as_deref());
        let hooks = load_agent_hooks(&conn, &agent_id);
        let (rm, fb) = load_agent_response_model(&conn, &agent_id);
        let policy = load_memory_policy(&conn, &agent_id);
        let summ = load_agent_summarizer_model(&conn, &agent_id);
        // v2.1.0 Phase 4 — fetch the slug for active-runs registry labeling.
        let slug: Option<String> = conn
            .query_row(
                "SELECT slug FROM agents WHERE id = ?1",
                rusqlite::params![&agent_id],
                |r| r.get::<_, String>(0),
            )
            .ok();
        (resolved, hooks, rm, fb, policy, summ, slug)
    };

    // Summarize older history (best-effort, non-streaming — summaries are
    // small and we want them in one shot).
    let (older, recent) = split_history_for_summarization(&history, &policy);
    let summary: Option<String> = if !older.is_empty() {
        let summarizer_prompt = build_summarizer_prompt(&older);
        let chosen_summarizer = if !policy.summarizer_model.is_empty() {
            Some(policy.summarizer_model.clone())
        } else {
            summarizer_model
        };
        let summ_cfg = chosen_summarizer.map(|m| serde_json::json!({ "model": m }).to_string());
        prompt_agent(runtime.clone(), summarizer_prompt, summ_cfg).await.ok()
    } else {
        None
    };

    let rendered_new = substitute_variables(&new_prompt, &resolved);
    let stitched = build_final_prompt(summary.as_deref(), &recent, &rendered_new);
    // fire_mode evaluation uses the current turn's user message.
    let context_block = run_pre_call_hooks(hooks, &new_prompt).await;
    let final_prompt = if context_block.is_empty() {
        stitched
    } else {
        format!("{}{}", context_block, stitched)
    };
    let merged_config = merge_model_into_config(config, response_model, fallback_model);

    spawn_streaming_dispatch(
        &runtime,
        &final_prompt,
        merged_config.as_deref(),
        on_event,
        agent_slug.as_deref(),
        active_project_path.as_deref(),
    ).await
}

async fn spawn_streaming_dispatch(
    runtime: &str,
    prompt: &str,
    config: Option<&str>,
    on_event: tauri::ipc::Channel<StreamEvent>,
    // v2.1.0 Phase 4 — context for the active-runs registry. Either
    // can be None for ad-hoc dispatches that don't have the info
    // (e.g. plain prompt_agent_stream from the chat pane without a
    // selected agent).
    agent_slug: Option<&str>,
    workspace: Option<&str>,
) -> Result<(), String> {
    use std::process::Stdio;
    use std::sync::Arc;
    use tokio::io::AsyncReadExt;
    use tokio::process::Command as TokioCommand;
    // (was tokio::sync::Mutex; replaced by oneshot channel for kill.)

    let user_path = get_user_path();
    let cfg_json: Option<serde_json::Value> = config.and_then(|c| serde_json::from_str(c).ok());
    let model_override: Option<String> = cfg_json
        .as_ref()
        .and_then(|c| c.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    let mut cmd = match runtime {
        "claude" => {
            let claude_path = which_claude().ok_or_else(|| "Claude Code CLI not found".to_string())?;
            let mut c = TokioCommand::new(claude_path);
            c.arg("--print").arg(prompt);
            if let Some(m) = &model_override {
                c.arg("--model").arg(m);
            }
            c
        }
        "codex" => {
            let codex_path = which_cli("codex")
                .ok_or_else(|| "Codex CLI not found. Install: npm install -g @openai/codex".to_string())?;
            let mut c = TokioCommand::new(codex_path);
            // Codex uses `exec` as the headless subcommand; the prompt is a
            // positional argument. `--print` is invalid for codex.
            // `--skip-git-repo-check` mirrors the non-streaming dispatch —
            // ATO can be run from any cwd, including non-repo dirs, and
            // Codex bails with "Not inside a trusted directory" otherwise.
            c.arg("exec").arg("--skip-git-repo-check");
            if let Some(m) = &model_override {
                c.arg("--model").arg(m);
            }
            c.arg(prompt);
            c
        }
        "openclaw" => {
            let ssh_config: serde_json::Value = config
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or_default();
            let host = ssh_config.get("sshHost").and_then(|v| v.as_str()).unwrap_or("localhost");
            let port = ssh_config.get("sshPort").and_then(|v| v.as_u64()).unwrap_or(22);
            let user = ssh_config.get("sshUser").and_then(|v| v.as_str()).unwrap_or("root");
            let key_path = ssh_config.get("sshKeyPath").and_then(|v| v.as_str());

            let mut c = TokioCommand::new("ssh");
            if let Some(key) = key_path {
                c.args(["-i", key]);
            }
            c.args([
                "-p",
                &port.to_string(),
                &format!("{}@{}", user, host),
                &format!("openclaw exec '{}'", prompt.replace('\'', "'\\''")),
            ]);
            c
        }
        "hermes" => {
            let hermes_path = which_cli("hermes").ok_or_else(|| "Hermes CLI not found".to_string())?;
            let mut c = TokioCommand::new(hermes_path);
            c.arg("--execute").arg(prompt);
            c
        }
        "gemini" => {
            let gemini_path = which_cli("gemini")
                .ok_or_else(|| "Gemini CLI not found. Install: npm install -g @google/gemini-cli".to_string())?;
            let mut c = TokioCommand::new(gemini_path);
            // Gemini CLI: `gemini -p "<prompt>" [-m <model>]`
            c.arg("-p").arg(prompt);
            if let Some(m) = &model_override {
                c.arg("-m").arg(m);
            }
            c
        }
        other => {
            let _ = on_event.send(StreamEvent::Error {
                message: format!("Unknown runtime: {}", other),
            });
            return Ok(());
        }
    };

    cmd.env("PATH", &user_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // kill_on_drop ensures the child dies if we panic or the task
        // is aborted before we get to wait — important for keeping the
        // registry honest about what's actually running.
        .kill_on_drop(true);

    // Register BEFORE spawn so that even a spawn failure lights up
    // the registry briefly (next finish_run cleans it up). Beatriz's
    // model of "intent first, outcome second" — the user clicked the
    // dispatch button, so the run exists conceptually even if the
    // process never started.
    let run_id = crate::active_runs::begin_run(
        runtime,
        agent_slug,
        workspace,
        Some("desktop:stream"),
    );
    // Guard so we always finish_run on early returns / errors.
    struct FinishOnDrop(String);
    impl Drop for FinishOnDrop {
        fn drop(&mut self) {
            crate::active_runs::finish_run(&self.0);
        }
    }
    let _finish_guard = FinishOnDrop(run_id.clone());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = on_event.send(StreamEvent::Error {
                message: format!("Failed to spawn {}: {}", runtime, e),
            });
            return Ok(());
        }
    };

    // Kill plumbing via oneshot channel. Earlier design wrapped the
    // child in a mutex and tried to lock + kill from the closure —
    // but the dispatch path takes the child out of the mutex to own
    // its stdout, so by the time a user clicks Kill the mutex holds
    // None and the closure no-ops silently (Beatriz: "stayed
    // spinning but still ended responding", 2026-05-09). The
    // oneshot pattern decouples them: the closure signals intent;
    // the dispatch loop's select! reacts by killing the child
    // inline (where it actually owns the handle).
    let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();
    let kill_tx_holder: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>> =
        Arc::new(std::sync::Mutex::new(Some(kill_tx)));
    let kill_tx_for_handler = kill_tx_holder.clone();
    crate::active_runs::attach_kill_handler(&run_id, move || {
        // Pure sync: lock, take, send. No tokio runtime needed inside
        // the closure — fixes the panic that crashed the app earlier.
        let mut guard = match kill_tx_for_handler.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if let Some(tx) = guard.take() {
            // Send may fail if the receiver dropped (dispatch already
            // finished); fine — kill becomes a no-op.
            let _ = tx.send(());
        }
    });
    let mut kill_rx = kill_rx;

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = on_event.send(StreamEvent::Error {
                message: "stdout-pipe-missing".into(),
            });
            return Ok(());
        }
    };

    // Read stdout in chunks, emitting each as a Chunk event. The buffer is
    // small enough that the user sees tokens flowing within a few hundred
    // ms, even if the runtime writes line-buffered.
    //
    // The select! gives the kill_rx receiver a chance to fire between
    // reads. When the user clicks Kill, the closure sends on the
    // oneshot, this branch wins, we kill the child + emit an error,
    // and return. Without this, the read loop would happily drain the
    // child's already-buffered stdout to completion even after the
    // kill request.
    let mut reader = stdout;
    let mut buf = [0u8; 1024];
    let mut full = String::new();
    loop {
        tokio::select! {
            biased;
            _ = &mut kill_rx => {
                // User clicked Kill. SIGKILL the child, surface a
                // clean "killed by user" error to the UI, and stop.
                let _ = child.kill().await;
                let _ = on_event.send(StreamEvent::Error {
                    message: "killed by user".into(),
                });
                return Ok(());
            }
            read_result = reader.read(&mut buf) => match read_result {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                    full.push_str(&chunk);
                    let _ = on_event.send(StreamEvent::Chunk { text: chunk });
                }
                Err(e) => {
                    let _ = on_event.send(StreamEvent::Error {
                        message: format!("read-failed: {}", e),
                    });
                    let _ = child.kill().await;
                    return Ok(());
                }
            },
        }
    }

    let status = match child.wait().await {
        Ok(s) => s,
        Err(e) => {
            let _ = on_event.send(StreamEvent::Error {
                message: format!("wait-failed: {}", e),
            });
            return Ok(());
        }
    };

    if status.success() {
        let _ = on_event.send(StreamEvent::Done { full });
    } else {
        // Drain stderr for the error message — best-effort.
        let mut err_text = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut err_text).await;
        }
        let _ = on_event.send(StreamEvent::Error {
            message: if err_text.is_empty() {
                format!("{} exited with status {}", runtime, status)
            } else {
                err_text
            },
        });
    }

    Ok(())
}

// ── Headless cron dispatch (v1.6 wake-from-sleep groundwork) ─────────────
//
// `ato-desktop --run-cron <id>` invokes this from outside the GUI. Used by
// OS-level schedulers (launchd on macOS today; systemd / Task Scheduler
// later) so jobs fire even when the app isn't open.
//
// Mirrors trigger_cron_job's logic but runs against a freshly-opened DB
// connection, blocks on a tokio runtime, and exits with an integer status
// code so launchd records success/failure.

pub fn run_cron_headless(job_id: String) -> i32 {
    let log_dir = home_dir().join(".ato").join("cron-logs");
    let _ = fs::create_dir_all(&log_dir);
    let log_path = log_dir.join(format!("{}.log", job_id));

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = fs::write(&log_path, format!("[error] tokio init: {}\n", e));
            return 1;
        }
    };

    let result = runtime.block_on(async { dispatch_cron_headless(&job_id).await });

    let now = chrono::Utc::now().to_rfc3339();
    match result {
        Ok(response) => {
            let entry = format!("[{}] [ok] job={}\n{}\n", now, job_id, response);
            let _ = append_to_file(&log_path, &entry);
            0
        }
        Err(e) => {
            let entry = format!("[{}] [err] job={}: {}\n", now, job_id, e);
            let _ = append_to_file(&log_path, &entry);
            1
        }
    }
}

fn append_to_file(path: &PathBuf, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

async fn dispatch_cron_headless(job_id: &str) -> Result<String, String> {
    // Read the job from disk (same shape as trigger_cron_job).
    let path = cron_jobs_path();
    if !path.exists() {
        return Err("No cron jobs configured".into());
    }
    let content = read_file_lossy(&path).unwrap_or_default();
    let jobs: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
    let job = jobs
        .iter()
        .find(|j| j.get("id").and_then(|v| v.as_str()) == Some(job_id))
        .ok_or_else(|| format!("Cron job not found: {}", job_id))?;

    let runtime = job.get("runtime").and_then(|v| v.as_str()).unwrap_or("claude").to_string();
    let prompt = job.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let config = job.get("runtimeConfig").map(|v| v.to_string());
    let agent_slug = job.get("agentSlug").and_then(|v| v.as_str()).map(String::from);
    let group_slug = job.get("groupSlug").and_then(|v| v.as_str()).map(String::from);

    // Open the DB ourselves — we're outside the Tauri State context.
    let db_path = crate::get_db_path();
    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => return Err(format!("open db: {}", e)),
    };

    if let Some(slug) = group_slug {
        // Replicate dispatch_to_group's logic without needing State<DbState>.
        return headless_dispatch_group(&conn, &slug, &prompt, config.as_deref()).await;
    }

    if let Some(slug) = agent_slug {
        let agent_lookup: Option<(String, String)> = conn
            .query_row(
                "SELECT id, runtime FROM agents WHERE slug = ?1",
                params![slug],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .ok();
        match agent_lookup {
            Some((agent_id, agent_runtime)) => {
                return headless_dispatch_agent(&conn, &agent_id, &agent_runtime, &prompt, config.as_deref()).await;
            }
            None => return Err(format!("Cron references missing agent slug '{}'", slug)),
        }
    }

    prompt_agent(runtime, prompt, config).await
}

async fn headless_dispatch_agent(
    conn: &Connection,
    agent_id: &str,
    runtime: &str,
    prompt: &str,
    config: Option<&str>,
) -> Result<String, String> {
    // Same shape as prompt_agent_with_context but doesn't need State<DbState>.
    let resolved = resolve_agent_variables(conn, agent_id, None);
    let hooks = load_agent_hooks(conn, agent_id);
    let (response_model, fallback_model) = load_agent_response_model(conn, agent_id);

    let rendered = substitute_variables(prompt, &resolved);
    let context_block = run_pre_call_hooks(hooks, &prompt).await;
    let final_prompt = if context_block.is_empty() {
        rendered
    } else {
        format!("{}{}", context_block, rendered)
    };

    let merged_config = merge_model_into_config(
        config.map(|s| s.to_string()),
        response_model,
        fallback_model,
    );
    prompt_agent(runtime.to_string(), final_prompt, merged_config).await
}

async fn headless_dispatch_group(
    conn: &Connection,
    slug: &str,
    prompt: &str,
    config: Option<&str>,
) -> Result<String, String> {
    let mut group = conn
        .query_row(
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind
             FROM agent_groups WHERE slug = ?1",
            params![slug],
            |row| {
                Ok(AgentGroup {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    display_name: row.get(2)?,
                    description: row.get(3)?,
                    runtime: row.get(4)?,
                    router_config: row.get(5)?,
                    file_path: row.get(6)?,
                    created_at: row.get(7)?,
                    last_used_at: row.get(8)?,
                    dispatch_kind: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "routed".to_string()),
                    members: Vec::new(),
                })
            },
        )
        .map_err(|e| format!("Group '{}' not found: {}", slug, e))?;
    group.members = load_group_members(conn, &group.id);

    if group.dispatch_kind == "sequential" {
        return run_sequential_dispatch(&group, prompt, config)
            .await
            .map(|r| r.response);
    }

    let (child_slug, _reason) = route_prompt_to_child(&group, prompt).await?;
    let child_agent: Option<(String, String)> = conn
        .query_row(
            "SELECT id, runtime FROM agents WHERE slug = ?1",
            params![child_slug],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .ok();
    match child_agent {
        Some((agent_id, agent_runtime)) => {
            headless_dispatch_agent(conn, &agent_id, &agent_runtime, prompt, config).await
        }
        None => Err(format!("Routed child '{}' not found", child_slug)),
    }
}

// ── Cron → launchd (macOS) ───────────────────────────────────────────────
//
// Translate the user's cron expression into one or more
// StartCalendarInterval entries that launchd understands. launchd doesn't
// support cron's full grammar (no ranges/steps/lists) — we expand to a
// cross-product of concrete entries. Common cases (fixed time daily,
// weekday-only, hourly, every-N-minutes) work; exotic expressions return
// an error and the user gets the in-app scheduler instead.

#[derive(Debug, Clone, Default)]
struct CalInterval {
    minute: Option<u32>,
    hour: Option<u32>,
    day: Option<u32>,
    month: Option<u32>,
    weekday: Option<u32>,
}

fn parse_cron_field(field: &str, min: u32, max_excl: u32) -> Result<Vec<Option<u32>>, String> {
    if field == "*" {
        return Ok(vec![None]);
    }
    let mut out: Vec<u32> = Vec::new();
    for chunk in field.split(',') {
        if let Some(stripped) = chunk.strip_prefix("*/") {
            // Step: */N
            let step: u32 = stripped.parse().map_err(|_| format!("bad step: {}", chunk))?;
            if step == 0 {
                return Err("step cannot be 0".into());
            }
            let mut v = min;
            while v < max_excl {
                out.push(v);
                v += step;
            }
        } else if let Some((lo, hi)) = chunk.split_once('-') {
            let lo: u32 = lo.parse().map_err(|_| format!("bad range start: {}", chunk))?;
            let hi: u32 = hi.parse().map_err(|_| format!("bad range end: {}", chunk))?;
            if lo > hi || hi >= max_excl || lo < min {
                return Err(format!("range out of bounds: {}", chunk));
            }
            for v in lo..=hi {
                out.push(v);
            }
        } else {
            let v: u32 = chunk.parse().map_err(|_| format!("bad field: {}", chunk))?;
            if v < min || v >= max_excl {
                return Err(format!("value out of bounds: {}", chunk));
            }
            out.push(v);
        }
    }
    Ok(out.into_iter().map(Some).collect())
}

fn cron_to_launchd_intervals(cron: &str) -> Result<Vec<CalInterval>, String> {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return Err("cron must have 5 fields (minute hour day month weekday)".into());
    }
    let minutes = parse_cron_field(parts[0], 0, 60)?;
    let hours = parse_cron_field(parts[1], 0, 24)?;
    let days = parse_cron_field(parts[2], 1, 32)?;
    let months = parse_cron_field(parts[3], 1, 13)?;
    // launchd weekday: 0 (Sunday) - 6 (Saturday). Cron same.
    let weekdays = parse_cron_field(parts[4], 0, 7)?;

    let mut out = Vec::new();
    for &m in &minutes {
        for &h in &hours {
            for &d in &days {
                for &mon in &months {
                    for &w in &weekdays {
                        out.push(CalInterval {
                            minute: m,
                            hour: h,
                            day: d,
                            month: mon,
                            weekday: w,
                        });
                    }
                }
            }
        }
    }
    if out.len() > 100 {
        return Err(format!(
            "cron expression expands to {} launchd entries (max 100)",
            out.len()
        ));
    }
    Ok(out)
}

fn interval_to_plist_dict(iv: &CalInterval) -> String {
    let mut out = String::from("    <dict>\n");
    if let Some(v) = iv.minute  { out.push_str(&format!("      <key>Minute</key><integer>{}</integer>\n", v)); }
    if let Some(v) = iv.hour    { out.push_str(&format!("      <key>Hour</key><integer>{}</integer>\n", v)); }
    if let Some(v) = iv.day     { out.push_str(&format!("      <key>Day</key><integer>{}</integer>\n", v)); }
    if let Some(v) = iv.month   { out.push_str(&format!("      <key>Month</key><integer>{}</integer>\n", v)); }
    if let Some(v) = iv.weekday { out.push_str(&format!("      <key>Weekday</key><integer>{}</integer>\n", v)); }
    out.push_str("    </dict>\n");
    out
}

fn build_launchd_plist(job_id: &str, ato_binary: &str, cron: &str, log_dir: &str) -> Result<String, String> {
    let intervals = cron_to_launchd_intervals(cron)?;
    let label = format!("ai.agentictool.cron-{}", job_id);

    let interval_xml = if intervals.len() == 1 {
        interval_to_plist_dict(&intervals[0])
    } else {
        let mut s = String::from("    <array>\n");
        for iv in &intervals {
            // Indent one extra level inside the array.
            for line in interval_to_plist_dict(iv).lines() {
                s.push_str("    ");
                s.push_str(line);
                s.push('\n');
            }
        }
        s.push_str("    </array>\n");
        s
    };

    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{binary}</string>
    <string>--run-cron</string>
    <string>{job_id}</string>
  </array>
  <key>StartCalendarInterval</key>
{intervals}  <key>RunAtLoad</key>
  <false/>
  <key>StandardOutPath</key>
  <string>{log_dir}/{job_id}.out.log</string>
  <key>StandardErrorPath</key>
  <string>{log_dir}/{job_id}.err.log</string>
</dict>
</plist>
"#,
        label = label,
        binary = ato_binary,
        job_id = job_id,
        intervals = interval_xml,
        log_dir = log_dir,
    ))
}

fn launchd_plist_path(job_id: &str) -> PathBuf {
    home_dir()
        .join("Library")
        .join("LaunchAgents")
        .join(format!("ai.agentictool.cron-{}.plist", job_id))
}

fn current_ato_binary_path() -> Result<String, String> {
    // The path of the running binary. When the OS scheduler later invokes
    // the unit, it'll exec this same binary with --run-cron <id>.
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
    Ok(exe.to_string_lossy().to_string())
}

// ── Linux: systemd --user timers ─────────────────────────────────────────
//
// Each cron job becomes a (.service, .timer) pair under
// `~/.config/systemd/user/`. The timer's OnCalendar field is derived from
// the cron expression — systemd's calendar grammar is a superset of cron
// (supports `*`, ranges with `..`, lists, and steps), so the mapping is
// mostly direct. Wake-from-sleep (`WakeSystem=true`) requires polkit + a
// configured RTC and isn't always honored — we set it as best-effort and
// rely on systemd to fire on next-boot via `Persistent=true` for any
// firings that were missed during sleep.

fn cron_to_systemd_oncalendar(cron: &str) -> Result<String, String> {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return Err("cron must have 5 fields (minute hour day month weekday)".into());
    }
    // Validate each field by reusing the launchd parser — same grammar.
    parse_cron_field(parts[0], 0, 60)?;
    parse_cron_field(parts[1], 0, 24)?;
    parse_cron_field(parts[2], 1, 32)?;
    parse_cron_field(parts[3], 1, 13)?;
    parse_cron_field(parts[4], 0, 7)?;

    let translate_step = |field: &str| field.replace("*/", "*/");
    let minute = translate_step(parts[0]);
    let hour = translate_step(parts[1]);
    let day = if parts[2] == "*" { "*".into() } else { parts[2].replace('-', "..") };
    let month = if parts[3] == "*" { "*".into() } else { parts[3].replace('-', "..") };

    // systemd weekdays are names: Mon..Fri, Sat,Sun. Translate the cron
    // numeric weekday (0=Sun, 6=Sat) to systemd names.
    let weekday_part = if parts[4] == "*" {
        String::new()
    } else {
        let names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let translate_one = |s: &str| -> Result<String, String> {
            let n: usize = s.parse().map_err(|_| format!("bad weekday: {}", s))?;
            if n >= 7 {
                return Err(format!("bad weekday: {}", s));
            }
            Ok(names[n].to_string())
        };
        let translated: Result<Vec<String>, String> = parts[4]
            .split(',')
            .map(|piece| {
                if let Some((lo, hi)) = piece.split_once('-') {
                    Ok(format!("{}..{}", translate_one(lo)?, translate_one(hi)?))
                } else {
                    translate_one(piece)
                }
            })
            .collect();
        let joined = translated?.join(",");
        format!("{} ", joined)
    };

    // Format: [WEEKDAY ]*-MM-DD HH:MM:SS
    Ok(format!(
        "{wd}*-{mo}-{d} {h}:{m}:00",
        wd = weekday_part,
        mo = month,
        d = day,
        h = hour,
        m = minute,
    ))
}

fn build_systemd_service(job_id: &str, ato_binary: &str) -> String {
    format!(
        r#"[Unit]
Description=ATO scheduled agent dispatch — {job_id}

[Service]
Type=oneshot
ExecStart={binary} --run-cron {job_id}
"#,
        job_id = job_id,
        binary = ato_binary,
    )
}

fn build_systemd_timer(job_id: &str, oncalendar: &str) -> String {
    format!(
        r#"[Unit]
Description=ATO scheduled agent timer — {job_id}

[Timer]
OnCalendar={oncalendar}
Persistent=true
WakeSystem=true

[Install]
WantedBy=timers.target
"#,
        job_id = job_id,
        oncalendar = oncalendar,
    )
}

#[allow(dead_code)] // only used on Linux; kept compiled elsewhere for parity.
fn systemd_user_dir() -> PathBuf {
    home_dir().join(".config").join("systemd").join("user")
}

#[allow(dead_code)]
fn systemd_unit_paths(job_id: &str) -> (PathBuf, PathBuf) {
    let dir = systemd_user_dir();
    (
        dir.join(format!("ato-cron-{}.service", job_id)),
        dir.join(format!("ato-cron-{}.timer", job_id)),
    )
}

#[cfg(target_os = "linux")]
fn register_systemd(job_id: &str, cron: &str) -> Result<String, String> {
    let binary = current_ato_binary_path()?;
    let oncalendar = cron_to_systemd_oncalendar(cron)?;
    let dir = systemd_user_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir systemd dir: {}", e))?;

    let (service_path, timer_path) = systemd_unit_paths(job_id);
    fs::write(&service_path, build_systemd_service(job_id, &binary))
        .map_err(|e| format!("write service: {}", e))?;
    fs::write(&timer_path, build_systemd_timer(job_id, &oncalendar))
        .map_err(|e| format!("write timer: {}", e))?;

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    let timer_unit = format!("ato-cron-{}.timer", job_id);
    let enable = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", &timer_unit])
        .output()
        .map_err(|e| format!("systemctl enable: {}", e))?;
    if !enable.status.success() {
        return Err(format!(
            "systemctl --user enable --now {} failed: {}",
            timer_unit,
            String::from_utf8_lossy(&enable.stderr)
        ));
    }
    Ok(timer_path.to_string_lossy().to_string())
}

#[cfg(target_os = "linux")]
fn unregister_systemd(job_id: &str) {
    let timer_unit = format!("ato-cron-{}.timer", job_id);
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", &timer_unit])
        .output();
    let (service_path, timer_path) = systemd_unit_paths(job_id);
    let _ = fs::remove_file(&service_path);
    let _ = fs::remove_file(&timer_path);
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
}

// ── Windows: schtasks via Task Scheduler XML ─────────────────────────────
//
// We generate a Task Scheduler XML file that captures the cron schedule
// (using calendar/time triggers) and `WakeToRun=true` so the laptop wakes
// to fire the job. `schtasks /Create /XML <file> /TN <name> /F` registers
// it; /Delete removes it.

fn cron_to_schtasks_xml_trigger(cron: &str) -> Result<String, String> {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return Err("cron must have 5 fields (minute hour day month weekday)".into());
    }
    let minutes = parse_cron_field(parts[0], 0, 60)?;
    let hours = parse_cron_field(parts[1], 0, 24)?;
    let days = parse_cron_field(parts[2], 1, 32)?;
    let months = parse_cron_field(parts[3], 1, 13)?;
    let weekdays = parse_cron_field(parts[4], 0, 7)?;

    // Pick a representative start time. Task Scheduler triggers have one
    // start time + a repetition pattern, so for cron expressions like
    // `*/15 * * * *` we use StartBoundary at midnight + Repetition every 15min.
    let first_minute = minutes.first().and_then(|m| *m).unwrap_or(0);
    let first_hour = hours.first().and_then(|h| *h).unwrap_or(0);
    let start_boundary = format!("2024-01-01T{:02}:{:02}:00", first_hour, first_minute);

    // Decide trigger type based on what's specified.
    let weekday_specified = parts[4] != "*";
    let day_specified = parts[2] != "*";
    let monthly = day_specified && !weekday_specified;
    let weekly = weekday_specified;
    let multi_minute = minutes.len() > 1;
    let multi_hour = hours.len() > 1;

    if multi_minute || multi_hour {
        // Use a Time trigger with a Repetition. Repetition interval: smallest
        // step we can detect.
        let interval = if multi_minute {
            // assume even step
            if minutes.len() >= 2 {
                let m0 = minutes[0].unwrap_or(0);
                let m1 = minutes[1].unwrap_or(0);
                format!("PT{}M", m1.saturating_sub(m0).max(1))
            } else {
                "PT15M".to_string()
            }
        } else {
            "PT1H".to_string()
        };
        return Ok(format!(
            r#"    <TimeTrigger>
      <Repetition>
        <Interval>{interval}</Interval>
        <Duration>P1D</Duration>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
    </TimeTrigger>
"#,
            interval = interval,
            start = start_boundary,
        ));
    }

    if weekly {
        let names = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        let mut day_xml = String::new();
        for w in weekdays.iter().filter_map(|w| *w) {
            if let Some(name) = names.get(w as usize) {
                day_xml.push_str(&format!("        <{0} />\n", name));
            }
        }
        return Ok(format!(
            r#"    <CalendarTrigger>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByWeek>
        <DaysOfWeek>
{days}        </DaysOfWeek>
        <WeeksInterval>1</WeeksInterval>
      </ScheduleByWeek>
    </CalendarTrigger>
"#,
            start = start_boundary,
            days = day_xml,
        ));
    }

    if monthly {
        let mut day_xml = String::new();
        for d in days.iter().filter_map(|d| *d) {
            day_xml.push_str(&format!("        <Day>{}</Day>\n", d));
        }
        let mut month_xml = String::new();
        let month_names = ["", "January", "February", "March", "April", "May", "June",
                           "July", "August", "September", "October", "November", "December"];
        for m in months.iter().filter_map(|m| *m) {
            if let Some(name) = month_names.get(m as usize) {
                month_xml.push_str(&format!("          <{0} />\n", name));
            }
        }
        let months_block = if month_xml.is_empty() {
            String::new()
        } else {
            format!("        <Months>\n{}        </Months>\n", month_xml)
        };
        return Ok(format!(
            r#"    <CalendarTrigger>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByMonth>
        <DaysOfMonth>
{days}        </DaysOfMonth>
{months_block}      </ScheduleByMonth>
    </CalendarTrigger>
"#,
            start = start_boundary,
            days = day_xml,
            months_block = months_block,
        ));
    }

    // Default: daily at the specified time.
    Ok(format!(
        r#"    <CalendarTrigger>
      <StartBoundary>{start}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByDay>
        <DaysInterval>1</DaysInterval>
      </ScheduleByDay>
    </CalendarTrigger>
"#,
        start = start_boundary,
    ))
}

fn build_schtasks_xml(job_id: &str, ato_binary: &str, cron: &str) -> Result<String, String> {
    let trigger = cron_to_schtasks_xml_trigger(cron)?;
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>ATO scheduled agent dispatch — {job_id}</Description>
  </RegistrationInfo>
  <Triggers>
{trigger}  </Triggers>
  <Settings>
    <WakeToRun>true</WakeToRun>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <Enabled>true</Enabled>
  </Settings>
  <Actions>
    <Exec>
      <Command>{binary}</Command>
      <Arguments>--run-cron {job_id}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        job_id = job_id,
        binary = ato_binary,
        trigger = trigger,
    ))
}

#[cfg(target_os = "windows")]
fn register_schtasks(job_id: &str, cron: &str) -> Result<String, String> {
    let binary = current_ato_binary_path()?;
    let xml = build_schtasks_xml(job_id, &binary, cron)?;

    // Write XML to a temp file. schtasks expects UTF-16 LE with BOM —
    // construct it explicitly so the encoding declaration in the XML
    // header isn't a lie.
    let temp_dir = std::env::temp_dir();
    let xml_path = temp_dir.join(format!("ato-cron-{}.xml", job_id));
    let mut bytes = vec![0xFF, 0xFE]; // UTF-16 LE BOM
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    fs::write(&xml_path, &bytes).map_err(|e| format!("write xml: {}", e))?;

    let task_name = format!("ATO\\Cron\\{}", job_id);
    let create = std::process::Command::new("schtasks")
        .args(["/Create", "/F", "/XML", &xml_path.to_string_lossy(), "/TN", &task_name])
        .output()
        .map_err(|e| format!("schtasks /Create: {}", e))?;
    let _ = fs::remove_file(&xml_path);
    if !create.status.success() {
        return Err(format!(
            "schtasks /Create failed: {}",
            String::from_utf8_lossy(&create.stderr)
        ));
    }
    Ok(task_name)
}

#[cfg(target_os = "windows")]
fn unregister_schtasks(job_id: &str) {
    let task_name = format!("ATO\\Cron\\{}", job_id);
    let _ = std::process::Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", &task_name])
        .output();
}

#[cfg(target_os = "windows")]
fn is_schtasks_registered(job_id: &str) -> bool {
    let task_name = format!("ATO\\Cron\\{}", job_id);
    std::process::Command::new("schtasks")
        .args(["/Query", "/TN", &task_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Public Tauri commands — OS-agnostic façade ───────────────────────────
//
// Renamed from the original `*_cron_launchd` to be honest about what they
// do across platforms. The old launchd-specific helpers are wrapped here.

#[tauri::command]
pub fn cron_os_scheduler_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux", target_os = "windows"))
}

#[tauri::command]
pub fn cron_os_scheduler_kind() -> &'static str {
    if cfg!(target_os = "macos") {
        "launchd"
    } else if cfg!(target_os = "linux") {
        "systemd-user"
    } else if cfg!(target_os = "windows") {
        "schtasks"
    } else {
        "unsupported"
    }
}

#[tauri::command]
pub fn register_cron_os_scheduler(job_id: String, cron: String) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let binary = current_ato_binary_path()?;
        let log_dir = home_dir().join(".ato").join("cron-logs");
        fs::create_dir_all(&log_dir).map_err(|e| format!("mkdir cron-logs: {}", e))?;
        let plist = build_launchd_plist(&job_id, &binary, &cron, &log_dir.to_string_lossy())?;
        let path = launchd_plist_path(&job_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir LaunchAgents: {}", e))?;
        }
        fs::write(&path, &plist).map_err(|e| format!("write plist: {}", e))?;
        let _ = std::process::Command::new("launchctl")
            .args(["unload", &path.to_string_lossy()])
            .output();
        let load = std::process::Command::new("launchctl")
            .args(["load", &path.to_string_lossy()])
            .output()
            .map_err(|e| format!("launchctl load: {}", e))?;
        if !load.status.success() {
            return Err(format!(
                "launchctl load failed: {}",
                String::from_utf8_lossy(&load.stderr)
            ));
        }
        return Ok(path.to_string_lossy().to_string());
    }
    #[cfg(target_os = "linux")]
    {
        return register_systemd(&job_id, &cron);
    }
    #[cfg(target_os = "windows")]
    {
        return register_schtasks(&job_id, &cron);
    }
    #[allow(unreachable_code)]
    Err(format!("OS-level cron not implemented on this platform (job {})", job_id))
}

#[tauri::command]
pub fn unregister_cron_os_scheduler(job_id: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let path = launchd_plist_path(&job_id);
        if path.exists() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", &path.to_string_lossy()])
                .output();
            let _ = fs::remove_file(&path);
        }
    }
    #[cfg(target_os = "linux")]
    {
        unregister_systemd(&job_id);
    }
    #[cfg(target_os = "windows")]
    {
        unregister_schtasks(&job_id);
    }
    Ok(())
}

#[tauri::command]
pub fn is_cron_os_scheduler_registered(job_id: String) -> bool {
    #[cfg(target_os = "macos")]
    {
        return launchd_plist_path(&job_id).exists();
    }
    #[cfg(target_os = "linux")]
    {
        let (_, timer_path) = systemd_unit_paths(&job_id);
        return timer_path.exists();
    }
    #[cfg(target_os = "windows")]
    {
        return is_schtasks_registered(&job_id);
    }
    #[allow(unreachable_code)]
    false
}

#[cfg(test)]
mod cron_launchd_tests {
    use super::*;

    #[test]
    fn parses_simple_daily_schedule() {
        let intervals = cron_to_launchd_intervals("0 7 * * *").unwrap();
        assert_eq!(intervals.len(), 1);
        assert_eq!(intervals[0].minute, Some(0));
        assert_eq!(intervals[0].hour, Some(7));
        assert_eq!(intervals[0].day, None);
        assert_eq!(intervals[0].weekday, None);
    }

    #[test]
    fn expands_weekday_range() {
        let intervals = cron_to_launchd_intervals("0 9 * * 1-5").unwrap();
        assert_eq!(intervals.len(), 5);
        assert!(intervals.iter().all(|i| i.minute == Some(0) && i.hour == Some(9)));
        let weekdays: Vec<u32> = intervals.iter().filter_map(|i| i.weekday).collect();
        assert_eq!(weekdays, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn expands_step_minutes() {
        let intervals = cron_to_launchd_intervals("*/15 * * * *").unwrap();
        assert_eq!(intervals.len(), 4);
        let minutes: Vec<u32> = intervals.iter().filter_map(|i| i.minute).collect();
        assert_eq!(minutes, vec![0, 15, 30, 45]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(cron_to_launchd_intervals("not a cron").is_err());
        assert!(cron_to_launchd_intervals("60 * * * *").is_err());
    }

    #[test]
    fn plist_xml_contains_label_and_binary() {
        let plist = build_launchd_plist("abc-123", "/Applications/ATO.app/Contents/MacOS/ato-desktop", "0 7 * * *", "/tmp").unwrap();
        assert!(plist.contains("ai.agentictool.cron-abc-123"));
        assert!(plist.contains("/Applications/ATO.app/Contents/MacOS/ato-desktop"));
        assert!(plist.contains("--run-cron"));
        assert!(plist.contains("<integer>7</integer>"));
    }

    #[test]
    fn systemd_oncalendar_daily() {
        let cal = cron_to_systemd_oncalendar("0 7 * * *").unwrap();
        assert_eq!(cal, "*-*-* 7:0:00");
    }

    #[test]
    fn systemd_oncalendar_weekday_range() {
        let cal = cron_to_systemd_oncalendar("0 9 * * 1-5").unwrap();
        assert_eq!(cal, "Mon..Fri *-*-* 9:0:00");
    }

    #[test]
    fn systemd_oncalendar_step_minute() {
        // systemd OnCalendar accepts */15 syntax verbatim — we just pass it through.
        let cal = cron_to_systemd_oncalendar("*/15 * * * *").unwrap();
        assert!(cal.starts_with("*-*-* *:*/15:00"));
    }

    #[test]
    fn systemd_unit_files_have_required_sections() {
        let svc = build_systemd_service("abc-123", "/usr/local/bin/ato-desktop");
        assert!(svc.contains("[Unit]"));
        assert!(svc.contains("[Service]"));
        assert!(svc.contains("--run-cron abc-123"));

        let timer = build_systemd_timer("abc-123", "*-*-* 09:00:00");
        assert!(timer.contains("[Timer]"));
        assert!(timer.contains("OnCalendar=*-*-* 09:00:00"));
        assert!(timer.contains("Persistent=true"));
        assert!(timer.contains("WakeSystem=true"));
    }

    #[test]
    fn schtasks_xml_weekly_includes_days() {
        let xml = build_schtasks_xml("abc-123", "C:\\ato\\ato-desktop.exe", "0 9 * * 1-5").unwrap();
        assert!(xml.contains("WakeToRun>true"));
        assert!(xml.contains("--run-cron abc-123"));
        assert!(xml.contains("<Monday />"));
        assert!(xml.contains("<Friday />"));
        assert!(xml.contains("CalendarTrigger"));
    }

    #[test]
    fn schtasks_xml_daily() {
        let xml = build_schtasks_xml("xyz", "C:\\ato\\ato-desktop.exe", "0 7 * * *").unwrap();
        assert!(xml.contains("ScheduleByDay"));
        assert!(xml.contains("StartBoundary>2024-01-01T07:00:00"));
    }

    #[test]
    fn schtasks_xml_step_uses_repetition() {
        let xml = build_schtasks_xml("xyz", "C:\\ato\\ato-desktop.exe", "*/15 * * * *").unwrap();
        assert!(xml.contains("<Repetition>"));
        assert!(xml.contains("<Interval>PT15M</Interval>"));
    }
}

// ── Configuration export / import (Polish-T4) ────────────────────────────
//
// JSON snapshots of the user's local config so they can move between
// machines or roll back. We deliberately exclude the *contents* of secrets
// and API keys — those live in the OS keychain or on disk in a way the user
// already controls. The backup carries metadata only (preview, name, kind),
// so importing on a new machine surfaces what's missing without leaking
// values out of the keychain.

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackup {
    pub version: u32,
    pub exported_at: String,
    pub agents: Vec<serde_json::Value>,
    pub agent_variables: Vec<serde_json::Value>,
    pub agent_hooks: Vec<serde_json::Value>,
    pub agent_groups: Vec<serde_json::Value>,
    pub agent_group_members: Vec<serde_json::Value>,
    pub projects: Vec<serde_json::Value>,
    pub env_vars: Vec<serde_json::Value>,
    pub model_configs: Vec<serde_json::Value>,
    pub secrets_meta: Vec<serde_json::Value>,
    pub llm_api_keys_meta: Vec<serde_json::Value>,
    pub settings: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub agents: usize,
    pub agent_variables: usize,
    pub agent_hooks: usize,
    pub agent_groups: usize,
    pub agent_group_members: usize,
    pub projects: usize,
    pub env_vars: usize,
    pub model_configs: usize,
    pub secrets_meta: usize,
    pub llm_api_keys_meta: usize,
    pub settings: usize,
}

fn dump_table(
    conn: &rusqlite::Connection,
    sql: &str,
    columns: &[&str],
) -> Result<Vec<serde_json::Value>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let mut obj = serde_json::Map::new();
            for (i, col) in columns.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                obj.insert((*col).to_string(), match val {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(n) => serde_json::Value::from(n),
                    rusqlite::types::Value::Real(f) => serde_json::Value::from(f),
                    rusqlite::types::Value::Text(s) => serde_json::Value::from(s),
                    rusqlite::types::Value::Blob(_) => serde_json::Value::Null,
                });
            }
            Ok(serde_json::Value::Object(obj))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn export_configuration(db: State<'_, DbState>) -> Result<ConfigBackup, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(ConfigBackup {
        version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        agents: dump_table(
            &conn,
            "SELECT id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at, role_models_json, memory_policy_json FROM agents",
            &["id","slug","displayName","description","runtime","model","projectId","systemPrompt","permissions","skills","mcps","goal","filePath","createdAt","lastUsedAt","roleModels","memoryPolicy"],
        )?,
        agent_variables: dump_table(
            &conn,
            "SELECT id, agent_id, name, kind, config_json, enabled, created_at, updated_at FROM agent_variables",
            &["id","agentId","name","kind","config","enabled","createdAt","updatedAt"],
        )?,
        agent_hooks: dump_table(
            &conn,
            "SELECT id, agent_id, position, name, kind, config_json, enabled, created_at FROM agent_hooks",
            &["id","agentId","position","name","kind","config","enabled","createdAt"],
        )?,
        agent_groups: dump_table(
            &conn,
            "SELECT id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind FROM agent_groups",
            &["id","slug","displayName","description","runtime","routerConfig","filePath","createdAt","lastUsedAt","dispatchKind"],
        )?,
        agent_group_members: dump_table(
            &conn,
            "SELECT group_id, agent_id, role, position FROM agent_group_members",
            &["groupId","agentId","role","position"],
        )?,
        projects: dump_table(
            &conn,
            "SELECT id, name, path, is_active, skill_count, last_accessed, created_at FROM projects",
            &["id","name","path","isActive","skillCount","lastAccessed","createdAt"],
        )?,
        env_vars: dump_table(
            &conn,
            "SELECT id, project_id, runtime, key, value, created_at FROM env_vars",
            &["id","projectId","runtime","key","value","createdAt"],
        )?,
        model_configs: dump_table(
            &conn,
            "SELECT id, runtime, project_id, model_id, max_tokens, temperature, created_at, updated_at FROM model_configs",
            &["id","runtime","projectId","modelId","maxTokens","temperature","createdAt","updatedAt"],
        )?,
        // Secrets metadata only — never the encrypted blob.
        secrets_meta: dump_table(
            &conn,
            "SELECT id, name, key_type, runtime, project_id, created_at, updated_at FROM secrets",
            &["id","name","keyType","runtime","projectId","createdAt","updatedAt"],
        )?,
        // LLM API keys metadata only.
        llm_api_keys_meta: dump_table(
            &conn,
            "SELECT id, provider, name, key_preview, project_id, runtime, is_active, last_used, usage_count, created_at, updated_at FROM llm_api_keys",
            &["id","provider","name","keyPreview","projectId","runtime","isActive","lastUsed","usageCount","createdAt","updatedAt"],
        )?,
        settings: dump_table(
            &conn,
            "SELECT key, value FROM settings",
            &["key","value"],
        )?,
    })
}

fn obj_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}
fn obj_i64(v: &serde_json::Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}
fn obj_f64(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}

#[tauri::command]
pub fn import_configuration(
    db: State<'_, DbState>,
    backup_json: String,
) -> Result<ImportSummary, String> {
    let backup: ConfigBackup =
        serde_json::from_str(&backup_json).map_err(|e| format!("invalid backup: {}", e))?;
    if backup.version != 1 {
        return Err(format!("unsupported backup version: {}", backup.version));
    }

    let mut conn = db.0.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    let mut s = ImportSummary {
        agents: 0,
        agent_variables: 0,
        agent_hooks: 0,
        agent_groups: 0,
        agent_group_members: 0,
        projects: 0,
        env_vars: 0,
        model_configs: 0,
        secrets_meta: 0,
        llm_api_keys_meta: 0,
        settings: 0,
    };

    for a in &backup.agents {
        tx.execute(
            "INSERT OR REPLACE INTO agents (id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at, role_models_json, memory_policy_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                obj_str(a, "id"), obj_str(a, "slug"), obj_str(a, "displayName"), obj_str(a, "description"),
                obj_str(a, "runtime"), obj_str(a, "model"), obj_str(a, "projectId"), obj_str(a, "systemPrompt"),
                obj_str(a, "permissions"), obj_str(a, "skills"), obj_str(a, "mcps"), obj_str(a, "goal"),
                obj_str(a, "filePath"), obj_str(a, "createdAt"), obj_str(a, "lastUsedAt"),
                obj_str(a, "roleModels"), obj_str(a, "memoryPolicy"),
            ],
        ).map_err(|e| e.to_string())?;
        s.agents += 1;
    }

    for v in &backup.agent_variables {
        tx.execute(
            "INSERT OR REPLACE INTO agent_variables (id, agent_id, name, kind, config_json, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                obj_str(v, "id"), obj_str(v, "agentId"), obj_str(v, "name"), obj_str(v, "kind"),
                obj_str(v, "config"), obj_i64(v, "enabled").unwrap_or(1),
                obj_str(v, "createdAt"), obj_str(v, "updatedAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.agent_variables += 1;
    }

    for h in &backup.agent_hooks {
        tx.execute(
            "INSERT OR REPLACE INTO agent_hooks (id, agent_id, position, name, kind, config_json, enabled, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                obj_str(h, "id"), obj_str(h, "agentId"),
                obj_i64(h, "position").unwrap_or(0),
                obj_str(h, "name"), obj_str(h, "kind"),
                obj_str(h, "config"), obj_i64(h, "enabled").unwrap_or(1),
                obj_str(h, "createdAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.agent_hooks += 1;
    }

    for g in &backup.agent_groups {
        tx.execute(
            "INSERT OR REPLACE INTO agent_groups (id, slug, display_name, description, runtime, router_config, file_path, created_at, last_used_at, dispatch_kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, COALESCE(?10, 'routed'))",
            params![
                obj_str(g, "id"), obj_str(g, "slug"), obj_str(g, "displayName"), obj_str(g, "description"),
                obj_str(g, "runtime"), obj_str(g, "routerConfig"), obj_str(g, "filePath"),
                obj_str(g, "createdAt"), obj_str(g, "lastUsedAt"),
                obj_str(g, "dispatchKind"),
            ],
        ).map_err(|e| e.to_string())?;
        s.agent_groups += 1;
    }

    for m in &backup.agent_group_members {
        tx.execute(
            "INSERT OR REPLACE INTO agent_group_members (group_id, agent_id, role, position) VALUES (?1, ?2, ?3, ?4)",
            params![
                obj_str(m, "groupId"), obj_str(m, "agentId"),
                obj_str(m, "role"), obj_i64(m, "position").unwrap_or(0),
            ],
        ).map_err(|e| e.to_string())?;
        s.agent_group_members += 1;
    }

    for p in &backup.projects {
        tx.execute(
            "INSERT OR REPLACE INTO projects (id, name, path, is_active, skill_count, last_accessed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                obj_str(p, "id"), obj_str(p, "name"), obj_str(p, "path"),
                obj_i64(p, "isActive").unwrap_or(0),
                obj_i64(p, "skillCount").unwrap_or(0),
                obj_str(p, "lastAccessed"), obj_str(p, "createdAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.projects += 1;
    }

    for e in &backup.env_vars {
        tx.execute(
            "INSERT OR REPLACE INTO env_vars (id, project_id, runtime, key, value, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                obj_str(e, "id"), obj_str(e, "projectId"), obj_str(e, "runtime"),
                obj_str(e, "key"), obj_str(e, "value"), obj_str(e, "createdAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.env_vars += 1;
    }

    for m in &backup.model_configs {
        tx.execute(
            "INSERT OR REPLACE INTO model_configs (id, runtime, project_id, model_id, max_tokens, temperature, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                obj_str(m, "id"), obj_str(m, "runtime"), obj_str(m, "projectId"),
                obj_str(m, "modelId"),
                obj_i64(m, "maxTokens"),
                obj_f64(m, "temperature"),
                obj_str(m, "createdAt"), obj_str(m, "updatedAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.model_configs += 1;
    }

    // Secrets/keys: metadata only — re-create rows with empty encrypted_key.
    // The user has to re-enter the values on the new machine. We surface
    // this in ImportSummary so the UI can prompt them.
    for k in &backup.secrets_meta {
        tx.execute(
            "INSERT OR IGNORE INTO secrets (id, name, key_type, runtime, project_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                obj_str(k, "id"), obj_str(k, "name"), obj_str(k, "keyType"),
                obj_str(k, "runtime"), obj_str(k, "projectId"),
                obj_str(k, "createdAt"), obj_str(k, "updatedAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.secrets_meta += 1;
    }

    for k in &backup.llm_api_keys_meta {
        tx.execute(
            "INSERT OR IGNORE INTO llm_api_keys (id, provider, name, key_preview, encrypted_key, project_id, runtime, is_active, last_used, usage_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                obj_str(k, "id"), obj_str(k, "provider"), obj_str(k, "name"),
                obj_str(k, "keyPreview"), "",
                obj_str(k, "projectId"), obj_str(k, "runtime"),
                obj_i64(k, "isActive").unwrap_or(0),
                obj_str(k, "lastUsed"),
                obj_i64(k, "usageCount").unwrap_or(0),
                obj_str(k, "createdAt"), obj_str(k, "updatedAt"),
            ],
        ).map_err(|e| e.to_string())?;
        s.llm_api_keys_meta += 1;
    }

    for setting in &backup.settings {
        tx.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![obj_str(setting, "key"), obj_str(setting, "value")],
        ).map_err(|e| e.to_string())?;
        s.settings += 1;
    }

    tx.commit().map_err(|e| e.to_string())?;
    Ok(s)
}

#[cfg(test)]
mod observability_tests {
    use super::*;

    #[test]
    fn percentile_handles_empty_and_single() {
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[42], 0.5), Some(42));
        assert_eq!(percentile(&[1, 2, 3, 4, 5], 0.5), Some(3));
    }

    fn make_eval(id: &str, kind: &str, cfg: &str) -> AgentEvaluator {
        AgentEvaluator {
            id: id.into(),
            agent_slug: "test".into(),
            name: "test-eval".into(),
            kind: kind.into(),
            config_json: cfg.into(),
            enabled: true,
            created_at: "2026-05-04T00:00:00Z".into(),
        }
    }

    fn make_trace(response: &str) -> AgentTraceLine {
        let mut t = AgentTraceLine::default();
        t.response_preview = Some(response.into());
        t
    }

    #[test]
    fn contains_evaluator_passes_when_response_has_substring() {
        let e = make_eval("e1", "contains", r#"{"needle":"success"}"#);
        let t = make_trace("Operation completed with SUCCESS");
        let r = run_evaluator(&e, &t);
        assert_eq!(r.verdict, "pass");
        assert_eq!(r.score, 1.0);
    }

    #[test]
    fn not_contains_evaluator_fails_when_forbidden_substring_present() {
        let e = make_eval("e1", "not-contains", r#"{"needle":"error"}"#);
        let t = make_trace("Encountered an Error during dispatch");
        let r = run_evaluator(&e, &t);
        assert_eq!(r.verdict, "fail");
    }

    #[test]
    fn length_range_evaluator_passes_when_within_bounds() {
        let e = make_eval("e1", "length-range", r#"{"min":5,"max":50}"#);
        let t = make_trace("hello world");
        let r = run_evaluator(&e, &t);
        assert_eq!(r.verdict, "pass");
    }

    #[test]
    fn llm_judge_returns_unknown_for_now() {
        let e = make_eval("e1", "llm-judge", r#"{"prompt":"is this good?"}"#);
        let t = make_trace("anything");
        let r = run_evaluator(&e, &t);
        assert_eq!(r.verdict, "unknown");
    }
}
