use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

// ── Types matching frontend expectations ─────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocalSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub file_path: String,
    pub scope: String,       // "enterprise" | "personal" | "project" | "plugin"
    pub runtime: String,     // "claude" | "codex" | "openclaw" | "hermes"
    pub token_count: u64,
    pub enabled: bool,
    pub content_hash: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetail {
    pub id: String,
    pub name: String,
    pub description: String,
    pub file_path: String,
    pub scope: String,
    pub runtime: String,
    pub token_count: u64,
    pub enabled: bool,
    pub content_hash: String,
    pub content: String,
    pub frontmatter: serde_json::Value,
    pub has_scripts: bool,
    pub has_references: bool,
    pub has_assets: bool,
    pub scripts: Vec<String>,
    pub references: Vec<String>,
    pub assets: Vec<String>,
    pub is_directory: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContextBreakdown {
    pub total_tokens: u64,
    pub limit: u64,
    pub categories: Vec<ContextCategory>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextCategory {
    pub name: String,
    pub tokens: u64,
    pub color: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocalMcpServer {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub status: String,
    pub tool_count: u64,
    pub command: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub today: UsagePeriod,
    pub week: UsagePeriod,
    pub month: UsagePeriod,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsagePeriod {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_cents: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsage {
    pub date: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BurnRate {
    pub tokens_per_hour: u64,
    pub cost_per_hour: f64,
    pub estimated_hours_to_limit: Option<f64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConfigFile {
    pub path: String,
    pub exists: bool,
    pub scope: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncStatus {
    pub enabled: bool,
    #[serde(rename = "lastSyncAt")]
    pub last_sync_at: Option<String>,
    #[serde(rename = "cloudUrl")]
    pub cloud_url: Option<String>,
}

// ── Database ─────────────────────────────────────────────────────────────

pub struct DbState(pub Mutex<Connection>);

fn get_db_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("local.db");
    path
}

fn home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
    } else if let Ok(profile) = std::env::var("USERPROFILE") {
        PathBuf::from(profile)
    } else {
        PathBuf::from(".")
    }
}

fn init_database(conn: &Connection) {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS skill_toggles (
            file_path TEXT PRIMARY KEY,
            enabled   INTEGER NOT NULL DEFAULT 1
        );
        CREATE TABLE IF NOT EXISTS cron_alerts (
            id         TEXT PRIMARY KEY,
            job_id     TEXT NOT NULL,
            type       TEXT NOT NULL,
            message    TEXT NOT NULL,
            created_at TEXT NOT NULL,
            acknowledged INTEGER NOT NULL DEFAULT 0
        );
        ",
    )
    .expect("Failed to initialize database tables");
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn claude_home() -> PathBuf {
    home_dir().join(".claude")
}

fn read_file_lossy(path: &PathBuf) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Estimate tokens from byte count (~4 bytes per token for English)
fn estimate_tokens(bytes: u64) -> u64 {
    bytes / 4
}

/// Simple hash of content for change detection
fn content_hash(content: &str) -> String {
    let mut hash: u64 = 5381;
    for byte in content.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    format!("{:x}", hash)
}

/// Parse YAML-like frontmatter from markdown content
fn parse_frontmatter(content: &str) -> (serde_json::Value, String) {
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

/// Collect skills from a directory, supporting both single files and SKILL.md directories
fn collect_skills(dir: &PathBuf, scope: &str, runtime: &str, db: &Connection) -> Vec<LocalSkill> {
    let mut skills = Vec::new();
    if !dir.exists() {
        return skills;
    }

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name;
            let content;
            let file_path_str;
            if path.is_dir() {
                // Directory skill — look for SKILL.md
                let skill_md = path.join("SKILL.md");
                if !skill_md.exists() {
                    continue;
                }
                name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                content = read_file_lossy(&skill_md).unwrap_or_default();
                file_path_str = format!("{}/", path.to_string_lossy());
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
                token_count: tokens,
                enabled,
                content_hash: hash,
            });
        }
    }

    skills
}

fn list_subdir_files(dir: &PathBuf, subdir: &str) -> (bool, Vec<String>) {
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
fn get_local_skills(db: State<'_, DbState>) -> Result<Vec<LocalSkill>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut skills = Vec::new();

    // ── Claude skills ──
    // Personal: ~/.claude/skills/
    let personal_dir = claude_home().join("skills");
    skills.extend(collect_skills(&personal_dir, "personal", "claude", &conn));

    // Project: .claude/skills/ (cwd)
    let project_dir = PathBuf::from(".claude").join("skills");
    skills.extend(collect_skills(&project_dir, "project", "claude", &conn));

    // Enterprise: /etc/claude/skills/
    let enterprise_dir = PathBuf::from("/etc/claude/skills");
    skills.extend(collect_skills(&enterprise_dir, "enterprise", "claude", &conn));

    // Plugins: ~/.claude/plugins/*/skills/
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

    // ── Codex skills (OpenAI) ──
    // CODEX_HOME defaults to ~/.codex
    let codex_home_str = std::env::var("CODEX_HOME").unwrap_or_else(|_| {
        home_dir().join(".codex").to_string_lossy().to_string()
    });
    let codex_home = PathBuf::from(&codex_home_str);
    // Personal: ~/.codex/skills/**/SKILL.md
    skills.extend(collect_skills(&codex_home.join("skills"), "personal", "codex", &conn));
    // Project: .agents/skills/ (Codex convention)
    skills.extend(collect_skills(&PathBuf::from(".agents").join("skills"), "project", "codex", &conn));
    // Project: .codex/skills/
    skills.extend(collect_skills(&PathBuf::from(".codex").join("skills"), "project", "codex", &conn));

    // ── OpenClaw skills ──
    // OPENCLAW_HOME env or defaults to ~/.openclaw
    let openclaw_home_str = std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| {
        home_dir().join(".openclaw").to_string_lossy().to_string()
    });
    let openclaw_home = PathBuf::from(&openclaw_home_str);
    // Personal: ~/.openclaw/skills/
    skills.extend(collect_skills(&openclaw_home.join("skills"), "personal", "openclaw", &conn));
    // Workspace: ~/.openclaw/workspace/skills/
    skills.extend(collect_skills(&openclaw_home.join("workspace").join("skills"), "personal", "openclaw", &conn));
    // Workspace config files as pseudo-skills (AGENTS.md, SOUL.md)
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
                        token_count: estimate_tokens(content.len() as u64),
                        enabled,
                        content_hash: hash,
                    });
                }
            }
        }
    }

    // ── Hermes skills (NousResearch) ──
    // ~/.hermes/skills/ with category subdirectories
    let hermes_home = home_dir().join(".hermes");
    let hermes_skills_dir = hermes_home.join("skills");
    skills.extend(collect_skills(&hermes_skills_dir, "personal", "hermes", &conn));
    // Also scan category subdirectories: ~/.hermes/skills/mlops/, ~/.hermes/skills/devops/, etc.
    if hermes_skills_dir.exists() {
        if let Ok(entries) = fs::read_dir(&hermes_skills_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Category directory — scan for skills inside
                    skills.extend(collect_skills(&path, "personal", "hermes", &conn));
                }
            }
        }
    }
    // Hermes memories as pseudo-skills (SOUL.md)
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
                token_count: estimate_tokens(content.len() as u64),
                enabled,
                content_hash: hash,
            });
        }
    }

    Ok(skills)
}

#[tauri::command]
fn get_skill_detail(db: State<'_, DbState>, id: String) -> Result<SkillDetail, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // Scan all directories — just scan Claude dirs for detail lookup since
    // the skill ID will only match one location regardless
    let mut all_skills = Vec::new();
    // Claude
    all_skills.extend(collect_skills(&claude_home().join("skills"), "personal", "claude", &conn));
    all_skills.extend(collect_skills(&PathBuf::from(".claude").join("skills"), "project", "claude", &conn));
    all_skills.extend(collect_skills(&PathBuf::from("/etc/claude/skills"), "enterprise", "claude", &conn));
    // Codex
    let codex_home = PathBuf::from(std::env::var("CODEX_HOME").unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
    all_skills.extend(collect_skills(&codex_home.join("skills"), "personal", "codex", &conn));
    all_skills.extend(collect_skills(&PathBuf::from(".agents").join("skills"), "project", "codex", &conn));
    all_skills.extend(collect_skills(&PathBuf::from(".codex").join("skills"), "project", "codex", &conn));
    // OpenClaw
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME").unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));
    all_skills.extend(collect_skills(&oc_home.join("skills"), "personal", "openclaw", &conn));
    all_skills.extend(collect_skills(&oc_home.join("workspace").join("skills"), "personal", "openclaw", &conn));
    // Hermes
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
fn toggle_local_skill(db: State<'_, DbState>, file_path: String, enabled: bool) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO skill_toggles (file_path, enabled) VALUES (?1, ?2)
         ON CONFLICT(file_path) DO UPDATE SET enabled = excluded.enabled",
        params![file_path, enabled as i32],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_context_estimate() -> Result<ContextBreakdown, String> {
    let mut categories = Vec::new();
    let mut total: u64 = 0;

    // System prompts (estimated)
    let system_tokens: u64 = 28000;
    categories.push(ContextCategory { name: "System Prompts".into(), tokens: system_tokens, color: "#FF4466".into() });
    total += system_tokens;

    // Skills
    let personal_dir = claude_home().join("skills");
    let project_dir = PathBuf::from(".claude").join("skills");
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
    let claude_md = PathBuf::from("CLAUDE.md");
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
fn dir_skill_bytes(dir: &PathBuf) -> u64 {
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

fn file_tokens(path: &PathBuf) -> u64 {
    fs::metadata(path).map(|m| estimate_tokens(m.len())).unwrap_or(0)
}

#[tauri::command]
fn get_context_for_runtime(runtime: String) -> Result<ContextBreakdown, String> {
    let mut categories = Vec::new();
    let mut total: u64 = 0;

    match runtime.as_str() {
        "claude" => {
            // System prompts
            let sys: u64 = 28000;
            categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
            total += sys;
            // Skills
            let skill_bytes = dir_skill_bytes(&claude_home().join("skills"))
                + dir_skill_bytes(&PathBuf::from(".claude").join("skills"));
            let st = estimate_tokens(skill_bytes);
            categories.push(ContextCategory { name: "Skills".into(), tokens: st, color: "#00FFB2".into() });
            total += st;
            // CLAUDE.md
            let cm = file_tokens(&PathBuf::from("CLAUDE.md"));
            categories.push(ContextCategory { name: "CLAUDE.md".into(), tokens: cm, color: "#FFB800".into() });
            total += cm;
            // MCP schemas
            let settings_path = claude_home().join("settings.json");
            let mcp: u64 = read_file_lossy(&settings_path)
                .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                .and_then(|v| v.get("mcpServers").and_then(|s| s.as_object()).map(|m| m.len() as u64 * 2500))
                .unwrap_or(0);
            categories.push(ContextCategory { name: "MCP Schemas".into(), tokens: mcp, color: "#3b82f6".into() });
            total += mcp;
            // Conversation
            categories.push(ContextCategory { name: "Conversation".into(), tokens: 15000, color: "#a78bfa".into() });
            total += 15000;

            Ok(ContextBreakdown { total_tokens: total, limit: 200000, categories })
        }
        "codex" => {
            let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
                .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
            // System prompts
            let sys: u64 = 20000;
            categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
            total += sys;
            // Skills
            let skill_bytes = dir_skill_bytes(&codex_home.join("skills"))
                + dir_skill_bytes(&PathBuf::from(".agents").join("skills"))
                + dir_skill_bytes(&PathBuf::from(".codex").join("skills"));
            let st = estimate_tokens(skill_bytes);
            categories.push(ContextCategory { name: "Skills".into(), tokens: st, color: "#00FFB2".into() });
            total += st;
            // AGENTS.md
            let agents_md = file_tokens(&PathBuf::from("AGENTS.md"))
                + file_tokens(&codex_home.join("AGENTS.md"));
            categories.push(ContextCategory { name: "AGENTS.md".into(), tokens: agents_md, color: "#FFB800".into() });
            total += agents_md;
            // config.toml
            let cfg = file_tokens(&codex_home.join("config.toml"));
            categories.push(ContextCategory { name: "config.toml".into(), tokens: cfg, color: "#3b82f6".into() });
            total += cfg;
            // Conversation
            categories.push(ContextCategory { name: "Conversation".into(), tokens: 12000, color: "#a78bfa".into() });
            total += 12000;

            Ok(ContextBreakdown { total_tokens: total, limit: 192000, categories })
        }
        "openclaw" => {
            let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
                .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));
            let ws = oc_home.join("workspace");
            // System prompts
            let sys: u64 = 15000;
            categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
            total += sys;
            // AGENTS.md
            let agents = file_tokens(&ws.join("AGENTS.md"));
            categories.push(ContextCategory { name: "AGENTS.md".into(), tokens: agents, color: "#FFB800".into() });
            total += agents;
            // SOUL.md
            let soul = file_tokens(&ws.join("SOUL.md"));
            categories.push(ContextCategory { name: "SOUL.md".into(), tokens: soul, color: "#f97316".into() });
            total += soul;
            // TOOLS.md
            let tools = file_tokens(&ws.join("TOOLS.md"));
            categories.push(ContextCategory { name: "TOOLS.md".into(), tokens: tools, color: "#06b6d4".into() });
            total += tools;
            // Skills
            let skill_bytes = dir_skill_bytes(&oc_home.join("skills"))
                + dir_skill_bytes(&ws.join("skills"));
            let st = estimate_tokens(skill_bytes);
            categories.push(ContextCategory { name: "Skills".into(), tokens: st, color: "#00FFB2".into() });
            total += st;
            // Memory
            let mem_dir = ws.join("memory");
            let mem_bytes = dir_skill_bytes(&mem_dir);
            let mem = estimate_tokens(mem_bytes);
            categories.push(ContextCategory { name: "Memory".into(), tokens: mem, color: "#a78bfa".into() });
            total += mem;

            Ok(ContextBreakdown { total_tokens: total, limit: 200000, categories })
        }
        "hermes" => {
            let hermes_home = home_dir().join(".hermes");
            // System prompts
            let sys: u64 = 12000;
            categories.push(ContextCategory { name: "System Prompts".into(), tokens: sys, color: "#FF4466".into() });
            total += sys;
            // SOUL.md
            let soul = file_tokens(&hermes_home.join("SOUL.md"));
            categories.push(ContextCategory { name: "SOUL.md".into(), tokens: soul, color: "#f97316".into() });
            total += soul;
            // Skills (with category subdirs)
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
            categories.push(ContextCategory { name: "Skills".into(), tokens: st, color: "#00FFB2".into() });
            total += st;
            // Memory
            let mem_bytes = file_tokens(&hermes_home.join("memories").join("MEMORY.md"))
                + file_tokens(&hermes_home.join("memories").join("USER.md"));
            categories.push(ContextCategory { name: "Memory".into(), tokens: mem_bytes, color: "#a78bfa".into() });
            total += mem_bytes;
            // Config
            let cfg = file_tokens(&hermes_home.join("config.yaml"));
            categories.push(ContextCategory { name: "config.yaml".into(), tokens: cfg, color: "#3b82f6".into() });
            total += cfg;

            Ok(ContextBreakdown { total_tokens: total, limit: 128000, categories })
        }
        _ => get_context_estimate(),
    }
}

#[tauri::command]
fn get_local_config() -> Result<Vec<LocalMcpServer>, String> {
    let mut servers = Vec::new();

    // Scan config files from ALL runtimes for MCP server definitions
    let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
        .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));

    let config_paths: Vec<(PathBuf, &str)> = vec![
        // Claude
        (claude_home().join("settings.json"), "claude"),
        (PathBuf::from(".claude").join("settings.json"), "claude-project"),
        // Codex
        (codex_home.join("config.toml"), "codex"),
        (PathBuf::from(".codex").join("config.toml"), "codex-project"),
        // OpenClaw
        (oc_home.join("openclaw.json"), "openclaw"),
        // Hermes
        (home_dir().join(".hermes").join("config.yaml"), "hermes"),
    ];

    for (settings_path, source) in &config_paths {
        if let Some(content) = read_file_lossy(settings_path) {
            // Try JSON parsing (Claude, OpenClaw)
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                // Check common MCP server config keys
                for key in ["mcpServers", "mcp_servers"] {
                    if let Some(mcp_servers) = parsed.get(key).and_then(|v| v.as_object()) {
                        for (name, config) in mcp_servers {
                            let command = config.get("command").and_then(|v| v.as_str()).map(|s| s.to_string());
                            let url_val = config.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
                            let transport = if url_val.is_some() { "http" } else { "stdio" };

                            servers.push(LocalMcpServer {
                                id: content_hash(&format!("{}-{}", source, name)),
                                name: format!("{} ({})", name, source),
                                transport: transport.to_string(),
                                status: "running".to_string(),
                                tool_count: 0,
                                command,
                                url: url_val,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(servers)
}

#[tauri::command]
fn get_local_usage() -> Result<UsageSummary, String> {
    // Return zeros — real usage tracking would parse Claude's session logs
    Ok(UsageSummary {
        today: UsagePeriod { input_tokens: 0, output_tokens: 0, cost_cents: 0 },
        week: UsagePeriod { input_tokens: 0, output_tokens: 0, cost_cents: 0 },
        month: UsagePeriod { input_tokens: 0, output_tokens: 0, cost_cents: 0 },
    })
}

#[tauri::command]
fn get_daily_usage(_days: u32) -> Result<Vec<DailyUsage>, String> {
    Ok(Vec::new())
}

#[tauri::command]
fn get_burn_rate() -> Result<BurnRate, String> {
    Ok(BurnRate {
        tokens_per_hour: 0,
        cost_per_hour: 0.0,
        estimated_hours_to_limit: None,
        limit: Some(200000),
    })
}

#[tauri::command]
fn get_config_files() -> Result<Vec<ConfigFile>, String> {
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
        ("CLAUDE.md", PathBuf::from("CLAUDE.md"), "Claude — Project context"),
        // Codex
        ("~/.codex/config.toml", codex.join("config.toml"), "Codex — Global config"),
        ("~/.codex/AGENTS.md", codex.join("AGENTS.md"), "Codex — Global instructions"),
        ("~/.codex/skills/", codex.join("skills"), "Codex — Personal skills"),
        (".codex/config.toml", PathBuf::from(".codex/config.toml"), "Codex — Project config"),
        ("AGENTS.md", PathBuf::from("AGENTS.md"), "Codex — Project instructions"),
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
fn get_sync_status(db: State<'_, DbState>) -> Result<SyncStatus, String> {
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
fn set_sync_enabled(db: State<'_, DbState>, enabled: bool, _cloud_url: Option<String>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('sync_enabled', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![if enabled { "true" } else { "false" }],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn restart_mcp_server(_name: String) -> Result<(), String> {
    // Placeholder — would need to actually restart the process
    Ok(())
}

#[tauri::command]
fn update_skill(id: String, content: String) -> Result<(), String> {
    // Scan ALL runtime directories to find the matching skill by ID
    let codex_home = PathBuf::from(std::env::var("CODEX_HOME")
        .unwrap_or_else(|_| home_dir().join(".codex").to_string_lossy().to_string()));
    let oc_home = PathBuf::from(std::env::var("OPENCLAW_HOME")
        .unwrap_or_else(|_| home_dir().join(".openclaw").to_string_lossy().to_string()));

    let dirs = vec![
        // Claude
        claude_home().join("skills"),
        PathBuf::from(".claude").join("skills"),
        PathBuf::from("/etc/claude/skills"),
        // Codex
        codex_home.join("skills"),
        PathBuf::from(".agents").join("skills"),
        PathBuf::from(".codex").join("skills"),
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
                    fs::write(&write_path, &content).map_err(|e| e.to_string())?;
                    return Ok(());
                }
            }
        }
    }

    Err(format!("Skill not found: {}", id))
}

#[tauri::command]
async fn prompt_claude(prompt: String) -> Result<String, String> {
    use std::process::Command;

    // Find the claude CLI
    let claude_path = which_claude().ok_or_else(|| {
        "Claude Code CLI not found. Install it with: npm install -g @anthropic-ai/claude-code".to_string()
    })?;

    // Run claude with --print flag (non-interactive, uses subscription)
    let output = Command::new(&claude_path)
        .args(["--print", &prompt])
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

fn workflows_dir() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    path.push("workflows");
    fs::create_dir_all(&path).ok();
    path
}

#[tauri::command]
fn list_workflows() -> Result<Vec<serde_json::Value>, String> {
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
fn save_workflow(workflow: String) -> Result<(), String> {
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
fn load_workflow(id: String) -> Result<serde_json::Value, String> {
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
fn delete_workflow(id: String) -> Result<(), String> {
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

/// Search for a CLI binary by name, checking common install paths + `which`.
fn which_cli(name: &str) -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();

    // Check common install locations
    let candidates: Vec<String> = vec![
        format!("/usr/local/bin/{}", name),
        format!("/opt/homebrew/bin/{}", name),
        format!("{}/.npm-global/bin/{}", home, name),
        format!("{}/bin/{}", home, name),
        format!("{}/.local/bin/{}", home, name),
        format!("{}/.cargo/bin/{}", home, name),
    ];

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.clone());
        }
    }

    // Check user-configured override from settings
    let override_path = home_dir().join(".ato").join(format!("{}-path", name));
    if let Some(custom) = read_file_lossy(&override_path) {
        let trimmed = custom.trim().to_string();
        if !trimmed.is_empty() && std::path::Path::new(&trimmed).exists() {
            return Some(trimmed);
        }
    }

    // Fall back to `which`
    if let Ok(output) = std::process::Command::new("which").arg(name).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }

    None
}

/// Save a custom CLI path for a runtime (used when auto-detect fails).
#[tauri::command]
fn set_runtime_path(runtime: String, path: String) -> Result<(), String> {
    let ato_dir = home_dir().join(".ato");
    fs::create_dir_all(&ato_dir).ok();
    let file_path = ato_dir.join(format!("{}-path", runtime));
    fs::write(&file_path, path.trim()).map_err(|e| format!("Failed to save runtime path: {}", e))
}

/// Get a saved custom CLI path for a runtime.
#[tauri::command]
fn get_runtime_path(runtime: String) -> Result<Option<String>, String> {
    let file_path = home_dir().join(".ato").join(format!("{}-path", runtime));
    Ok(read_file_lossy(&file_path).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
}

#[tauri::command]
fn detect_agent_runtimes() -> Result<Vec<DetectedRuntime>, String> {
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
async fn prompt_agent(runtime: String, prompt: String, config: Option<String>) -> Result<String, String> {
    use std::process::Command;

    match runtime.as_str() {
        "claude" => {
            let claude_path = which_claude().ok_or_else(|| {
                "Claude Code CLI not found".to_string()
            })?;
            let output = Command::new(&claude_path)
                .args(["--print", &prompt])
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
            let output = Command::new(&codex_path)
                .args(["--print", &prompt])
                .output()
                .map_err(|e| format!("Failed to run codex: {}", e))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(format!("Codex error: {}", String::from_utf8_lossy(&output.stderr)))
            }
        }
        "openclaw" => {
            // Parse SSH config from the config JSON
            let ssh_config: serde_json::Value = config
                .as_deref()
                .and_then(|c| serde_json::from_str(c).ok())
                .unwrap_or_default();
            let host = ssh_config.get("sshHost").and_then(|v| v.as_str()).unwrap_or("localhost");
            let port = ssh_config.get("sshPort").and_then(|v| v.as_u64()).unwrap_or(22);
            let user = ssh_config.get("sshUser").and_then(|v| v.as_str()).unwrap_or("root");
            let key_path = ssh_config.get("sshKeyPath").and_then(|v| v.as_str());

            let mut cmd = Command::new("ssh");
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
                .output()
                .map_err(|e| format!("Failed to run hermes: {}", e))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(format!("Hermes error: {}", String::from_utf8_lossy(&output.stderr)))
            }
        }
        _ => Err(format!("Unknown runtime: {}", runtime)),
    }
}

// ── Cron Job Persistence ─────────────────────────────────────────────────

fn cron_jobs_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("cron-jobs.json");
    path
}

fn cron_history_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("cron-history.json");
    path
}

#[tauri::command]
fn list_cron_jobs() -> Result<Vec<serde_json::Value>, String> {
    let path = cron_jobs_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = read_file_lossy(&path).unwrap_or_default();
    serde_json::from_str::<Vec<serde_json::Value>>(&content)
        .map_err(|e| format!("Invalid cron jobs JSON: {}", e))
}

#[tauri::command]
fn save_cron_job(job: String) -> Result<(), String> {
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
fn delete_cron_job(id: String) -> Result<(), String> {
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
fn get_cron_history(job_id: String) -> Result<Vec<serde_json::Value>, String> {
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
async fn trigger_cron_job(id: String) -> Result<String, String> {
    // Read the job to get its prompt and runtime
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
async fn query_agent_status(runtime: String, config: Option<String>) -> Result<AgentStatus, String> {
    use std::process::Command;

    match runtime.as_str() {
        "claude" => {
            let path = which_claude();
            let available = path.is_some();
            let mut version = None;
            let mut healthy = false;

            if let Some(ref cli) = path {
                // Get version
                if let Ok(output) = Command::new(cli).arg("--version").output() {
                    if output.status.success() {
                        version = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
                // Auth check — run a minimal prompt
                if let Ok(output) = Command::new(cli).args(["--print", "respond with OK"]).output() {
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
                if let Ok(output) = Command::new(cli).arg("--version").output() {
                    if output.status.success() {
                        version = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
                if let Ok(output) = Command::new(cli).arg("--help").output() {
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
                if let Ok(output) = Command::new(cli).arg("--version").output() {
                    if output.status.success() {
                        version = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
                if let Ok(output) = Command::new(cli).arg("--help").output() {
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
fn query_all_agent_statuses() -> Result<Vec<AgentStatus>, String> {
    // Synchronous check — just detect availability (no auth checks for speed)
    let runtimes = vec![
        ("claude", which_claude()),
        ("codex", which_cli("codex")),
        ("openclaw", None::<String>), // needs config
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

fn agent_logs_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("agent-logs.jsonl");
    path
}

#[tauri::command]
fn append_agent_log(entry: String) -> Result<(), String> {
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
fn get_agent_logs(runtime: Option<String>, limit: Option<u32>) -> Result<Vec<serde_json::Value>, String> {
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

fn which_claude() -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let npm_global = format!("{}/.npm-global/bin/claude", home);
    let user_bin = format!("{}/bin/claude", home);
    let nvm_path = format!("{}/.nvm/versions/node/*/bin/claude", home);

    let candidates = vec![
        "/usr/local/bin/claude",
        "/opt/homebrew/bin/claude",
        npm_global.as_str(),
        user_bin.as_str(),
        nvm_path.as_str(),
    ];

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    // Try `which claude` as fallback
    if let Ok(output) = std::process::Command::new("which").arg("claude").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }

    None
}

// ── App Entry ────────────────────────────────────────────────────────────

pub fn run() {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path).expect("Failed to open SQLite database");
    init_database(&conn);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(DbState(Mutex::new(conn)))
        .invoke_handler(tauri::generate_handler![
            get_local_skills,
            get_skill_detail,
            toggle_local_skill,
            get_context_estimate,
            get_context_for_runtime,
            get_local_config,
            get_local_usage,
            get_daily_usage,
            get_burn_rate,
            get_config_files,
            get_sync_status,
            set_sync_enabled,
            restart_mcp_server,
            update_skill,
            prompt_claude,
            list_workflows,
            save_workflow,
            load_workflow,
            delete_workflow,
            detect_agent_runtimes,
            set_runtime_path,
            get_runtime_path,
            prompt_agent,
            query_agent_status,
            query_all_agent_statuses,
            append_agent_log,
            get_agent_logs,
            list_cron_jobs,
            save_cron_job,
            delete_cron_job,
            get_cron_history,
            trigger_cron_job,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
