mod openclaw_ws;
mod log_watcher;
mod health_poller;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{State, Manager};
use log_watcher::LogWatcherState;
use health_poller::HealthPollerState;

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
    pub project: Option<String>, // project directory name for project-scoped skills
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

// ── Secrets & Config Types ───────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    pub id: String,
    pub name: String,
    pub key_type: String,      // "api_key", "ssh_key", "token"
    pub runtime: Option<String>,
    pub project_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub has_value: bool,       // Whether a value is stored in keychain
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub id: String,
    pub project_id: Option<String>,
    pub runtime: Option<String>,
    pub key: String,
    pub value: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    pub id: String,
    pub runtime: String,
    pub project_id: Option<String>,
    pub model_id: String,
    pub max_tokens: Option<i32>,
    pub temperature: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionLog {
    pub id: String,
    pub runtime: String,
    pub prompt: Option<String>,
    pub response: Option<String>,
    pub tokens_in: Option<i32>,
    pub tokens_out: Option<i32>,
    pub duration_ms: Option<i32>,
    pub status: String,        // "success", "error", "timeout"
    pub error_message: Option<String>,
    pub skill_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    pub id: String,
    pub runtime: String,
    pub status: String,        // "healthy", "degraded", "offline"
    pub latency_ms: Option<i32>,
    pub error_message: Option<String>,
    pub checked_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealth {
    pub runtime: String,
    pub status: String,
    pub latency_ms: Option<i32>,
    pub uptime_percent: Option<f64>,
    pub last_check: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HealthHistoryPoint {
    pub timestamp: String,
    pub latency_ms: Option<i32>,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealthHistory {
    pub runtime: String,
    pub data_points: Vec<HealthHistoryPoint>,
    pub avg_latency_ms: Option<f64>,
    pub uptime_percent: f64,
    pub total_checks: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetrics {
    pub total_executions: i64,
    pub successful_executions: i64,
    pub failed_executions: i64,
    pub total_tokens_in: i64,
    pub total_tokens_out: i64,
    pub avg_duration_ms: Option<f64>,
    pub executions_by_runtime: Vec<RuntimeExecutionCount>,
    pub executions_by_day: Vec<DailyExecutionCount>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeExecutionCount {
    pub runtime: String,
    pub count: i64,
    pub success_count: i64,
    pub error_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DailyExecutionCount {
    pub date: String,
    pub count: i64,
    pub success_count: i64,
    pub error_count: i64,
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
        CREATE TABLE IF NOT EXISTS profile_snapshots (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            description TEXT,
            runtime     TEXT NOT NULL,
            files_json  TEXT NOT NULL,
            created_at  TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS projects (
            id           TEXT PRIMARY KEY,
            name         TEXT NOT NULL,
            path         TEXT NOT NULL UNIQUE,
            is_active    INTEGER NOT NULL DEFAULT 0,
            skill_count  INTEGER NOT NULL DEFAULT 0,
            last_accessed TEXT,
            created_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS secrets (
            id           TEXT PRIMARY KEY,
            name         TEXT NOT NULL,
            key_type     TEXT NOT NULL,
            runtime      TEXT,
            project_id   TEXT,
            created_at   TEXT NOT NULL,
            updated_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS env_vars (
            id           TEXT PRIMARY KEY,
            project_id   TEXT,
            runtime      TEXT,
            key          TEXT NOT NULL,
            value        TEXT NOT NULL,
            created_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS model_configs (
            id           TEXT PRIMARY KEY,
            runtime      TEXT NOT NULL,
            project_id   TEXT,
            model_id     TEXT NOT NULL,
            max_tokens   INTEGER,
            temperature  REAL,
            created_at   TEXT NOT NULL,
            updated_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS execution_logs (
            id           TEXT PRIMARY KEY,
            runtime      TEXT NOT NULL,
            prompt       TEXT,
            response     TEXT,
            tokens_in    INTEGER,
            tokens_out   INTEGER,
            duration_ms  INTEGER,
            status       TEXT NOT NULL,
            error_message TEXT,
            skill_name   TEXT,
            created_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS health_checks (
            id           TEXT PRIMARY KEY,
            runtime      TEXT NOT NULL,
            status       TEXT NOT NULL,
            latency_ms   INTEGER,
            error_message TEXT,
            checked_at   TEXT NOT NULL
        );
        ",
    )
    .expect("Failed to initialize database tables");
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn claude_home() -> PathBuf {
    home_dir().join(".claude")
}

/// Find the project root by walking up from CWD looking for .git or .claude/
fn project_root() -> PathBuf {
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
fn discover_project_roots() -> Vec<PathBuf> {
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

/// Collect skills from a directory, supporting single files, SKILL.md directories,
/// symlinks (gstack-style), and nested subdirectories (one level deep).
fn collect_skills(dir: &PathBuf, scope: &str, runtime: &str, db: &Connection) -> Vec<LocalSkill> {
    collect_skills_for_project(dir, scope, runtime, None, db)
}

fn collect_skills_for_project(dir: &PathBuf, scope: &str, runtime: &str, project: Option<&str>, db: &Connection) -> Vec<LocalSkill> {
    let mut skills = Vec::new();
    if !dir.exists() {
        return skills;
    }

    collect_skills_inner(dir, scope, runtime, project, db, &mut skills, 0);
    skills
}

fn collect_skills_inner(dir: &PathBuf, scope: &str, runtime: &str, project: Option<&str>, db: &Connection, skills: &mut Vec<LocalSkill>, depth: u32) {
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
fn get_skill_detail(db: State<'_, DbState>, id: String) -> Result<SkillDetail, String> {
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
fn find_current_session() -> Option<(String, PathBuf)> {
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
fn parse_session_jsonl(path: &PathBuf) -> Result<LiveSessionData, String> {
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
fn get_live_session_data() -> Result<LiveSessionData, String> {
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
fn get_live_context_breakdown() -> Result<ContextBreakdown, String> {
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
fn discover_mcp_tools_stdio(command: &str, args: &[&str], env: &std::collections::HashMap<String, String>) -> Result<McpServerDetails, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    // Build the command
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .envs(env);

    // Spawn the process
    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to spawn MCP server: {}", e))?;

    let stdin = child.stdin.as_mut()
        .ok_or("Failed to open stdin")?;
    let stdout = child.stdout.take()
        .ok_or("Failed to open stdout")?;

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

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

    // Read initialize response with timeout
    let mut read_response = || -> Result<serde_json::Value, String> {
        line.clear();
        reader.read_line(&mut line)
            .map_err(|e| format!("Failed to read response: {}", e))?;
        serde_json::from_str(&line)
            .map_err(|e| format!("Failed to parse response: {}", e))
    };

    let init_response = read_response()?;

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
fn discover_mcp_server_tools(server_name: String) -> Result<McpServerDetails, String> {
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
fn get_mcp_servers_with_tools() -> Result<Vec<McpServerDetails>, String> {
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
fn get_hooks() -> Result<Vec<HookConfig>, String> {
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

fn parse_hooks_from_settings(
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
fn save_hook(hook: HookConfig) -> Result<(), String> {
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
fn delete_hook(hook_id: String) -> Result<(), String> {
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
        (project_root().join(".claude").join("settings.json"), "claude-project"),
        // Codex
        (codex_home.join("config.toml"), "codex"),
        (project_root().join(".codex").join("config.toml"), "codex-project"),
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

/// Resolve the skill directory for a given runtime + scope
fn skill_dir_for_runtime(runtime: &str, scope: &str) -> PathBuf {
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
fn create_skill(data: String) -> Result<SkillDetail, String> {
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
fn delete_skill(id: String) -> Result<(), String> {
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
fn update_skill(id: String, content: String) -> Result<(), String> {
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

/// Get the user's full shell PATH (Tauri apps launch with minimal env)
fn get_user_path() -> String {
    // Try to get PATH from user's shell
    if let Ok(output) = std::process::Command::new("/bin/zsh")
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
    if let Ok(output) = std::process::Command::new("/bin/bash")
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
    std::env::var("PATH").unwrap_or_default()
}

/// Search for a CLI binary by name, checking common install paths + user shell + npx cache.
fn which_cli(name: &str) -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();

    // 1. Check user-configured override first (highest priority)
    let override_path = home_dir().join(".ato").join(format!("{}-path", name));
    if let Some(custom) = read_file_lossy(&override_path) {
        let trimmed = custom.trim().to_string();
        if !trimmed.is_empty() && std::path::Path::new(&trimmed).exists() {
            return Some(trimmed);
        }
    }

    // 2. Check common install locations
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

    // 4. Use `which` from the user's full shell PATH (not Tauri's minimal env)
    let user_path = get_user_path();
    if let Ok(output) = std::process::Command::new("which")
        .arg(name)
        .env("PATH", &user_path)
        .output()
    {
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

    // Use the user's full shell PATH so CLIs can find node, npm, etc.
    let user_path = get_user_path();

    match runtime.as_str() {
        "claude" => {
            let claude_path = which_claude().ok_or_else(|| {
                "Claude Code CLI not found".to_string()
            })?;
            let output = Command::new(&claude_path)
                .args(["--print", &prompt])
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
            let output = Command::new(&codex_path)
                .args(["--print", &prompt])
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
    // which_cli now handles all the search logic including npx cache
    // and user shell PATH. No need for a separate function.
    which_cli("claude")
}

// ── OpenClaw WebSocket + Runtime Config ───────────────────────────────────

/// Load OpenClaw SSH config from ~/.ato/openclaw-config.json
fn load_openclaw_ssh_config() -> Result<(String, u64, String, Option<String>), String> {
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
fn openclaw_ssh_base() -> Result<(std::process::Command, String, u64, String), String> {
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
fn openclaw_ssh_command(subcmd: &str) -> Result<serde_json::Value, String> {
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
fn openclaw_ssh_raw(shell_cmd: &str) -> Result<String, String> {
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
async fn openclaw_gateway_status() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("status --json")
}

#[tauri::command]
async fn openclaw_list_cron_jobs() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("cron list --all --json")
}

#[tauri::command]
async fn openclaw_cron_status() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("cron status --json")
}

#[tauri::command]
async fn openclaw_list_agents() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("agents list --json")
}

#[tauri::command]
async fn openclaw_skills_status() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("skills status --json")
}

#[tauri::command]
async fn openclaw_list_sessions() -> Result<serde_json::Value, String> {
    openclaw_ssh_command("sessions list --json")
}

#[tauri::command]
async fn openclaw_test_connection(ws_url: String, token: String) -> Result<serde_json::Value, String> {
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
fn save_runtime_config(runtime: String, config: String) -> Result<(), String> {
    let ato_dir = home_dir().join(".ato");
    fs::create_dir_all(&ato_dir).ok();
    let file_path = ato_dir.join(format!("{}-config.json", runtime));
    fs::write(&file_path, config).map_err(|e| format!("Failed to save config: {}", e))
}

#[tauri::command]
fn load_runtime_config(runtime: String) -> Result<Option<String>, String> {
    let file_path = home_dir().join(".ato").join(format!("{}-config.json", runtime));
    Ok(read_file_lossy(&file_path))
}

#[tauri::command]
async fn test_runtime_connection(runtime: String, config: String) -> Result<serde_json::Value, String> {
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
async fn openclaw_edit_cron_job(id: String, args: String) -> Result<serde_json::Value, String> {
    // args is a space-separated string of CLI flags like "--name foo --every 1h --message 'do stuff'"
    openclaw_ssh_command(&format!("cron edit {} {} --json", id, args))
}

#[tauri::command]
async fn openclaw_add_cron_job(args: String) -> Result<serde_json::Value, String> {
    openclaw_ssh_command(&format!("cron add {} --json", args))
}

#[tauri::command]
async fn openclaw_delete_cron_job(id: String) -> Result<serde_json::Value, String> {
    openclaw_ssh_command(&format!("cron rm {} --json", id))
}

#[tauri::command]
async fn openclaw_run_cron_job(id: String) -> Result<serde_json::Value, String> {
    openclaw_ssh_command(&format!("cron run {} --json", id))
}

#[tauri::command]
async fn openclaw_toggle_cron_job(id: String, enable: bool) -> Result<serde_json::Value, String> {
    let flag = if enable { "--enable" } else { "--disable" };
    openclaw_ssh_command(&format!("cron edit {} {} --json", id, flag))
}

// ── Remote OpenClaw Skills ────────────────────────────────────────────────

#[tauri::command]
async fn openclaw_list_skills() -> Result<Vec<LocalSkill>, String> {
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
fn list_context_files() -> Result<Vec<ContextFile>, String> {
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
fn read_context_file(file_path: String) -> Result<String, String> {
    read_file_lossy(&PathBuf::from(&file_path)).ok_or_else(|| format!("Cannot read: {}", file_path))
}

#[tauri::command]
fn write_context_file(file_path: String, content: String) -> Result<(), String> {
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
fn scan_agent_config_files(project_path: Option<String>) -> Result<Vec<AgentConfigFile>, String> {
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
fn scan_md_directory(
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
fn scan_skills_directory_recursive(
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

fn add_config_if_exists(
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

fn scan_skills_directory(
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
fn chrono_lite(unix_secs: u64) -> String {
    // Basic ISO 8601 format without full chrono dependency
    // Just return the unix timestamp as a string for now
    format!("{}", unix_secs)
}

/// Read and parse a config file, handling different formats
#[tauri::command]
fn read_agent_config_file(path: String) -> Result<ParsedConfigFile, String> {
    let path_buf = PathBuf::from(&path);
    let content = fs::read_to_string(&path_buf)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let extension = path_buf.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let (format, parsed) = match extension {
        "json" => {
            let value: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| format!("Invalid JSON: {}", e))?;
            ("json".to_string(), value)
        }
        "toml" => {
            // Simple TOML parsing - convert to JSON-like structure
            let parsed = parse_simple_toml(&content);
            ("toml".to_string(), parsed)
        }
        "yaml" | "yml" => {
            // Simple YAML parsing
            let parsed = parse_simple_yaml(&content);
            ("yaml".to_string(), parsed)
        }
        "md" => {
            // Check for YAML frontmatter
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
        path,
        format,
        content: parsed,
        raw: content,
    })
}

/// Simple TOML parser (without full toml crate dependency)
fn parse_simple_toml(content: &str) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    let mut current_section = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Section header
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = trimmed[1..trimmed.len()-1].to_string();
            if !obj.contains_key(&current_section) {
                obj.insert(current_section.clone(), serde_json::Value::Object(serde_json::Map::new()));
            }
            continue;
        }

        // Key = value
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim().to_string();
            let value_str = trimmed[eq_pos+1..].trim();
            let value = parse_toml_value(value_str);

            if current_section.is_empty() {
                obj.insert(key, value);
            } else if let Some(section) = obj.get_mut(&current_section) {
                if let serde_json::Value::Object(ref mut section_map) = section {
                    section_map.insert(key, value);
                }
            }
        }
    }

    serde_json::Value::Object(obj)
}

fn parse_toml_value(s: &str) -> serde_json::Value {
    // Handle quoted strings
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        return serde_json::Value::String(s[1..s.len()-1].to_string());
    }
    // Handle booleans
    if s == "true" {
        return serde_json::Value::Bool(true);
    }
    if s == "false" {
        return serde_json::Value::Bool(false);
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
    // Handle arrays (simple case)
    if s.starts_with('[') && s.ends_with(']') {
        let inner = s[1..s.len()-1].trim();
        let items: Vec<serde_json::Value> = inner
            .split(',')
            .map(|item| parse_toml_value(item.trim()))
            .collect();
        return serde_json::Value::Array(items);
    }
    // Default to string
    serde_json::Value::String(s.to_string())
}

/// Simple YAML parser (basic key-value pairs)
fn parse_simple_yaml(content: &str) -> serde_json::Value {
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

fn parse_yaml_value(s: &str) -> serde_json::Value {
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

/// Write a config file back to disk
#[tauri::command]
fn write_agent_config_file(path: String, content: String) -> Result<(), String> {
    // Create parent directories if needed
    let path_buf = PathBuf::from(&path);
    if let Some(parent) = path_buf.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    fs::write(&path, &content)
        .map_err(|e| format!("Failed to write file: {}", e))
}

/// Create a new skill file from template
#[tauri::command]
fn create_agent_skill(runtime: String, name: String, scope: String, description: String) -> Result<String, String> {
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
fn parse_agent_permissions(path: String) -> Result<Vec<Permission>, String> {
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

fn parse_permission_string(s: &str) -> (String, Option<String>) {
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
fn get_agent_context_preview(runtime: String) -> Result<ContextPreview, String> {
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
fn validate_skill(path: String) -> Result<SkillValidation, String> {
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
fn validate_all_skills() -> Result<Vec<SkillValidation>, String> {
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
fn get_onboarding_status(runtime: String) -> Result<OnboardingStatus, String> {
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
fn which_sync(cmd: &str) -> Option<PathBuf> {
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
fn save_profile_snapshot(
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
fn list_profile_snapshots(db: State<'_, DbState>) -> Result<Vec<ProfileSnapshot>, String> {
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
fn load_profile_snapshot(db: State<'_, DbState>, profile_id: String) -> Result<(), String> {
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
fn delete_profile_snapshot(db: State<'_, DbState>, profile_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM profile_snapshots WHERE id = ?1",
        params![profile_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Export a profile snapshot as JSON
#[tauri::command]
fn export_profile_snapshot(db: State<'_, DbState>, profile_id: String) -> Result<String, String> {
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
fn get_skill_usage_stats() -> Result<Vec<SkillUsageStat>, String> {
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
fn discover_projects() -> Result<Vec<DiscoveredProject>, String> {
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

                if has_claude || has_codex || has_hermes || has_openclaw {
                    let mut runtimes = Vec::new();
                    if has_claude { runtimes.push("claude".to_string()); }
                    if has_codex { runtimes.push("codex".to_string()); }
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
fn count_project_skills(project_path: &PathBuf) -> u32 {
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
fn list_projects(db: State<'_, DbState>) -> Result<Vec<Project>, String> {
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
        })
    }).map_err(|e| e.to_string())?;

    projects.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Add a project to the list
#[tauri::command]
fn add_project(db: State<'_, DbState>, name: String, path: String) -> Result<Project, String> {
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
    })
}

/// Update a project's name
#[tauri::command]
fn update_project(db: State<'_, DbState>, project_id: String, name: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE projects SET name = ?1 WHERE id = ?2",
        params![name, project_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Delete a project from the list (doesn't delete files)
#[tauri::command]
fn delete_project(db: State<'_, DbState>, project_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM projects WHERE id = ?1",
        params![project_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Set the active project
#[tauri::command]
fn set_active_project(db: State<'_, DbState>, project_id: String) -> Result<(), String> {
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
fn get_active_project(db: State<'_, DbState>) -> Result<Option<Project>, String> {
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
fn get_project_skills(project_path: String) -> Result<Vec<LocalSkill>, String> {
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
fn clone_skill(
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
fn refresh_project_skills(db: State<'_, DbState>, project_id: String) -> Result<u32, String> {
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
fn list_secrets(db: State<'_, DbState>) -> Result<Vec<Secret>, String> {
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
fn save_secret(
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
fn get_secret_value(secret_id: String) -> Result<String, String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &secret_id)
        .map_err(|e| format!("Failed to access keychain: {}", e))?;
    entry.get_password()
        .map_err(|e| format!("Failed to retrieve secret: {}", e))
}

/// Update a secret value
#[tauri::command]
fn update_secret(
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
fn delete_secret(db: State<'_, DbState>, secret_id: String) -> Result<(), String> {
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
fn list_env_vars(db: State<'_, DbState>, project_id: Option<String>, runtime: Option<String>) -> Result<Vec<EnvVar>, String> {
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
fn save_env_var(
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
fn update_env_var(db: State<'_, DbState>, env_id: String, key: Option<String>, value: Option<String>) -> Result<(), String> {
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
fn delete_env_var(db: State<'_, DbState>, env_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM env_vars WHERE id = ?1", params![env_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Import environment variables from a .env file
#[tauri::command]
fn import_env_file(db: State<'_, DbState>, file_path: String, project_id: Option<String>, runtime: Option<String>) -> Result<Vec<EnvVar>, String> {
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
fn list_model_configs(db: State<'_, DbState>) -> Result<Vec<ModelConfig>, String> {
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
fn save_model_config(
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
fn get_model_config(db: State<'_, DbState>, runtime: String, project_id: Option<String>) -> Result<Option<ModelConfig>, String> {
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
fn get_execution_logs(
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

fn map_execution_log(row: &rusqlite::Row) -> Result<ExecutionLog, rusqlite::Error> {
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
fn add_execution_log(
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
fn get_health_status(db: State<'_, DbState>) -> Result<Vec<RuntimeHealth>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let runtimes = vec!["claude", "codex", "hermes", "openclaw"];
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
fn record_health_check(
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
fn start_log_watcher(
    app: tauri::AppHandle,
    watcher_state: State<'_, LogWatcherState>,
) -> Result<bool, String> {
    let mut watcher = watcher_state.0.lock().map_err(|e| e.to_string())?;
    watcher.start(app)?;
    Ok(true)
}

/// Stop the log file watcher
#[tauri::command]
fn stop_log_watcher(watcher_state: State<'_, LogWatcherState>) -> Result<bool, String> {
    let mut watcher = watcher_state.0.lock().map_err(|e| e.to_string())?;
    watcher.stop();
    Ok(true)
}

/// Check if log watcher is running
#[tauri::command]
fn is_log_watcher_running(watcher_state: State<'_, LogWatcherState>) -> Result<bool, String> {
    let watcher = watcher_state.0.lock().map_err(|e| e.to_string())?;
    Ok(watcher.is_watching())
}

/// Start the background health poller
#[tauri::command]
fn start_health_poller(
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
fn stop_health_poller(poller_state: State<'_, HealthPollerState>) -> Result<bool, String> {
    let poller = poller_state.0.lock().map_err(|e| e.to_string())?;
    poller.stop();
    Ok(true)
}

/// Check if health poller is running
#[tauri::command]
fn is_health_poller_running(poller_state: State<'_, HealthPollerState>) -> Result<bool, String> {
    let poller = poller_state.0.lock().map_err(|e| e.to_string())?;
    Ok(poller.is_running())
}

/// Get health check history for charts (last 24 hours)
#[tauri::command]
fn get_health_history(
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
fn get_usage_metrics(
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
fn register_workflow_webhook(
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
fn list_workflow_webhooks(state: State<DbState>) -> Result<Vec<WorkflowWebhook>, String> {
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
fn delete_workflow_webhook(state: State<DbState>, webhook_id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM workflow_webhooks WHERE id = ?1",
        params![&webhook_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Toggle webhook enabled state
#[tauri::command]
fn toggle_workflow_webhook(state: State<DbState>, webhook_id: String, enabled: bool) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE workflow_webhooks SET enabled = ?1 WHERE id = ?2",
        params![if enabled { 1 } else { 0 }, &webhook_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// List built-in workflow templates
#[tauri::command]
fn list_workflow_templates() -> Result<Vec<WorkflowTemplate>, String> {
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

// ── App Entry ────────────────────────────────────────────────────────────

pub fn run() {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path).expect("Failed to open SQLite database");
    init_database(&conn);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(DbState(Mutex::new(conn)))
        .manage(LogWatcherState::new())
        .manage(HealthPollerState::new())
        .setup(|app| {
            // Auto-start health poller on app launch
            let db_path_str = get_db_path().to_string_lossy().to_string();
            let poller_state = app.state::<HealthPollerState>();
            let poller = poller_state.0.lock().unwrap();
            poller.start(app.handle().clone(), db_path_str);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_local_skills,
            get_skill_detail,
            toggle_local_skill,
            get_context_estimate,
            get_context_for_runtime,
            get_live_session_data,
            get_live_context_breakdown,
            discover_mcp_server_tools,
            get_mcp_servers_with_tools,
            get_hooks,
            save_hook,
            delete_hook,
            get_local_config,
            get_local_usage,
            get_daily_usage,
            get_burn_rate,
            get_config_files,
            get_sync_status,
            set_sync_enabled,
            restart_mcp_server,
            create_skill,
            update_skill,
            delete_skill,
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
            openclaw_gateway_status,
            openclaw_list_cron_jobs,
            openclaw_cron_status,
            openclaw_list_agents,
            openclaw_skills_status,
            openclaw_list_sessions,
            openclaw_test_connection,
            openclaw_edit_cron_job,
            openclaw_add_cron_job,
            openclaw_delete_cron_job,
            openclaw_run_cron_job,
            openclaw_toggle_cron_job,
            save_runtime_config,
            load_runtime_config,
            test_runtime_connection,
            openclaw_list_skills,
            list_context_files,
            read_context_file,
            write_context_file,
            // Agent Configuration Manager
            scan_agent_config_files,
            read_agent_config_file,
            write_agent_config_file,
            create_agent_skill,
            parse_agent_permissions,
            get_agent_context_preview,
            // Skill Health Check
            validate_skill,
            validate_all_skills,
            // Onboarding Checklist
            get_onboarding_status,
            // Profile Snapshots
            save_profile_snapshot,
            list_profile_snapshots,
            load_profile_snapshot,
            delete_profile_snapshot,
            export_profile_snapshot,
            // Skill Usage Analytics
            get_skill_usage_stats,
            // Project Manager
            discover_projects,
            list_projects,
            add_project,
            update_project,
            delete_project,
            set_active_project,
            get_active_project,
            get_project_skills,
            clone_skill,
            refresh_project_skills,
            // Secrets Manager
            list_secrets,
            save_secret,
            get_secret_value,
            update_secret,
            delete_secret,
            // Environment Variables
            list_env_vars,
            save_env_var,
            update_env_var,
            delete_env_var,
            import_env_file,
            // Model Configuration
            list_model_configs,
            save_model_config,
            get_model_config,
            // Execution Logs
            get_execution_logs,
            add_execution_log,
            // Health Checks
            get_health_status,
            record_health_check,
            // Phase 2: Real-time Monitoring
            start_log_watcher,
            stop_log_watcher,
            is_log_watcher_running,
            start_health_poller,
            stop_health_poller,
            is_health_poller_running,
            get_health_history,
            get_usage_metrics,
            // v0.8.0: Workflow Webhooks & Templates
            register_workflow_webhook,
            list_workflow_webhooks,
            delete_workflow_webhook,
            toggle_workflow_webhook,
            list_workflow_templates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
