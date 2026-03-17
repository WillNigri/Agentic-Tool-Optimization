use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

// ── Types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Skill {
    pub name: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalConfig {
    pub settings_path: String,
    pub settings: serde_json::Value,
    pub projects: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UsageEntry {
    pub timestamp: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub session_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextEstimate {
    pub total_files: usize,
    pub total_bytes: u64,
    pub estimated_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncStatus {
    pub enabled: bool,
    pub last_synced: Option<String>,
}

// ── Database ───────────────────────────────────────────────────────────────

pub struct DbState(pub Mutex<Connection>);

fn get_db_path() -> PathBuf {
    let mut path = dirs_fallback();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("local.db");
    path
}

/// Fallback for home directory detection without the `dirs` crate.
fn dirs_fallback() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
    } else if let Ok(profile) = std::env::var("USERPROFILE") {
        PathBuf::from(profile)
    } else {
        PathBuf::from(".")
    }
}

fn home_dir() -> PathBuf {
    dirs_fallback()
}

fn init_database(conn: &Connection) {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS usage_cache (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp     TEXT NOT NULL,
            model         TEXT NOT NULL DEFAULT '',
            input_tokens  INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cost_usd      REAL NOT NULL DEFAULT 0.0,
            session_id    TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS skills_cache (
            name    TEXT PRIMARY KEY,
            path    TEXT NOT NULL,
            content TEXT NOT NULL,
            updated TEXT NOT NULL DEFAULT (datetime('now'))
        );
        ",
    )
    .expect("Failed to initialize database tables");
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn claude_home() -> PathBuf {
    home_dir().join(".claude")
}

fn read_file_lossy(path: &PathBuf) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn collect_skills_from(dir: &PathBuf) -> Vec<Skill> {
    let mut skills = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let content = read_file_lossy(&path).unwrap_or_default();
                skills.push(Skill {
                    name,
                    path: path.to_string_lossy().to_string(),
                    content,
                });
            }
        }
    }
    skills
}

// ── Tauri Commands ─────────────────────────────────────────────────────────

#[tauri::command]
fn get_local_skills() -> Result<Vec<Skill>, String> {
    let mut skills = Vec::new();

    // Global skills: ~/.claude/skills/
    let global_skills_dir = claude_home().join("skills");
    skills.extend(collect_skills_from(&global_skills_dir));

    // Project-local skills: .claude/skills/ (cwd)
    let local_skills_dir = PathBuf::from(".claude").join("skills");
    skills.extend(collect_skills_from(&local_skills_dir));

    Ok(skills)
}

#[tauri::command]
fn get_local_config() -> Result<LocalConfig, String> {
    let claude_dir = claude_home();

    // Try reading the main settings file (~/.claude/settings.json or ~/.claude.json)
    let settings_candidates = vec![
        claude_dir.join("settings.json"),
        home_dir().join(".claude.json"),
    ];

    let mut settings_path = String::new();
    let mut settings = serde_json::Value::Null;

    for candidate in &settings_candidates {
        if let Some(content) = read_file_lossy(&candidate) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                settings_path = candidate.to_string_lossy().to_string();
                settings = parsed;
                break;
            }
        }
    }

    // Discover project directories by scanning ~/.claude/projects/
    let mut projects = Vec::new();
    let projects_dir = claude_dir.join("projects");
    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                projects.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }

    Ok(LocalConfig {
        settings_path,
        settings,
        projects,
    })
}

#[tauri::command]
fn get_local_usage() -> Result<Vec<UsageEntry>, String> {
    let logs_dir = claude_home().join("logs");
    let mut entries = Vec::new();

    if !logs_dir.exists() {
        return Ok(entries);
    }

    let mut log_files: Vec<PathBuf> = Vec::new();
    if let Ok(dir_entries) = fs::read_dir(&logs_dir) {
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "jsonl") {
                log_files.push(path);
            }
        }
    }

    // Sort by name (typically date-based) descending, take recent files
    log_files.sort();
    log_files.reverse();
    let recent_files = &log_files[..log_files.len().min(30)];

    for file_path in recent_files {
        if let Some(content) = read_file_lossy(file_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
                    let entry = UsageEntry {
                        timestamp: obj
                            .get("timestamp")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        model: obj
                            .get("model")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        input_tokens: obj
                            .get("input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        output_tokens: obj
                            .get("output_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        cost_usd: obj
                            .get("cost_usd")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0),
                        session_id: obj
                            .get("session_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    };
                    entries.push(entry);
                }
            }
        }
    }

    Ok(entries)
}

#[tauri::command]
fn get_context_estimate() -> Result<ContextEstimate, String> {
    let claude_dir = claude_home();
    let mut total_files: usize = 0;
    let mut total_bytes: u64 = 0;

    // Estimate based on files that Claude typically loads into context
    let scan_dirs = vec![
        claude_dir.join("skills"),
        PathBuf::from(".claude").join("skills"),
    ];

    let config_files = vec![
        claude_dir.join("settings.json"),
        home_dir().join(".claude.json"),
        PathBuf::from("CLAUDE.md"),
        PathBuf::from(".claude/CLAUDE.md"),
    ];

    for dir in &scan_dirs {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(meta) = fs::metadata(&path) {
                        total_files += 1;
                        total_bytes += meta.len();
                    }
                }
            }
        }
    }

    for file in &config_files {
        if let Ok(meta) = fs::metadata(file) {
            if meta.is_file() {
                total_files += 1;
                total_bytes += meta.len();
            }
        }
    }

    // Rough estimate: ~4 bytes per token for English text
    let estimated_tokens = total_bytes / 4;

    Ok(ContextEstimate {
        total_files,
        total_bytes,
        estimated_tokens,
    })
}

#[tauri::command]
fn get_sync_status(db: State<'_, DbState>) -> Result<SyncStatus, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let enabled: bool = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'sync_enabled'",
            [],
            |row| {
                let val: String = row.get(0)?;
                Ok(val == "true")
            },
        )
        .unwrap_or(false);

    let last_synced: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'last_synced'",
            [],
            |row| row.get(0),
        )
        .ok();

    Ok(SyncStatus {
        enabled,
        last_synced,
    })
}

#[tauri::command]
fn set_sync_enabled(db: State<'_, DbState>, enabled: bool) -> Result<SyncStatus, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('sync_enabled', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![if enabled { "true" } else { "false" }],
    )
    .map_err(|e| e.to_string())?;

    let last_synced: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'last_synced'",
            [],
            |row| row.get(0),
        )
        .ok();

    Ok(SyncStatus {
        enabled,
        last_synced,
    })
}

// ── App Entry ──────────────────────────────────────────────────────────────

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
            get_local_config,
            get_local_usage,
            get_context_estimate,
            get_sync_status,
            set_sync_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
