mod openclaw_ws;
mod log_watcher;
mod health_poller;
mod telemetry;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{State, Manager, Emitter};
pub use log_watcher::LogWatcherState;
pub use health_poller::HealthPollerState;
pub use telemetry::TelemetryState;
use lettre::{
    Message, SmtpTransport, Transport,
    message::{header::ContentType, Mailbox},
    transport::smtp::authentication::Credentials,
};

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

// ── Audit Logging Types ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogEntry {
    pub id: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub resource_name: Option<String>,
    pub details: Option<String>,
    pub created_at: String,
}

// ── LLM API Key Types ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmApiKey {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub key_preview: String,
    pub project_id: Option<String>,
    pub runtime: Option<String>,
    pub is_active: bool,
    pub last_used: Option<String>,
    pub usage_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

// ── Real-time Monitoring Types ─────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub id: String,
    pub runtime: String,
    pub status: String,
    pub prompt: Option<String>,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub duration_ms: Option<i64>,
    pub skill_name: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringSnapshot {
    pub active_sessions: Vec<AgentSession>,
    pub recent_sessions: Vec<AgentSession>,
    pub total_tokens_today: i64,
    pub total_sessions_today: i64,
    pub errors_today: i64,
    pub avg_duration_ms: f64,
    pub runtimes_online: Vec<String>,
    pub runtimes_offline: Vec<String>,
    pub token_rate_per_hour: f64,
    pub alerts: Vec<MonitoringAlert>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringAlert {
    pub id: String,
    pub level: String,
    pub message: String,
    pub runtime: Option<String>,
    pub created_at: String,
}

// ── Database ─────────────────────────────────────────────────────────────

pub struct DbState(pub Mutex<Connection>);

pub fn get_db_path() -> PathBuf {
    let mut path = home_dir();
    path.push(".ato");
    fs::create_dir_all(&path).ok();
    path.push("local.db");
    path
}

pub fn home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
    } else if let Ok(profile) = std::env::var("USERPROFILE") {
        PathBuf::from(profile)
    } else {
        PathBuf::from(".")
    }
}

pub fn init_database(conn: &Connection) {
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
        CREATE TABLE IF NOT EXISTS audit_logs (
            id            TEXT PRIMARY KEY,
            action        TEXT NOT NULL,
            resource_type TEXT NOT NULL,
            resource_id   TEXT,
            resource_name TEXT,
            details       TEXT,
            created_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_audit_logs_action ON audit_logs(action);
        CREATE INDEX IF NOT EXISTS idx_audit_logs_created ON audit_logs(created_at);
        CREATE INDEX IF NOT EXISTS idx_audit_logs_resource ON audit_logs(resource_type, resource_id);
        CREATE TABLE IF NOT EXISTS llm_api_keys (
            id            TEXT PRIMARY KEY,
            provider      TEXT NOT NULL,
            name          TEXT NOT NULL,
            key_preview   TEXT NOT NULL,
            encrypted_key TEXT NOT NULL,
            project_id    TEXT,
            runtime       TEXT,
            is_active     INTEGER NOT NULL DEFAULT 1,
            last_used     TEXT,
            usage_count   INTEGER NOT NULL DEFAULT 0,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_llm_keys_provider ON llm_api_keys(provider);
        CREATE INDEX IF NOT EXISTS idx_llm_keys_project ON llm_api_keys(project_id);
        ",
    )
    .expect("Failed to initialize database tables");
}


pub mod commands;
pub use commands::*;

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
        .manage(TelemetryState::new())
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
            preview_write_agent_config_file,
            validate_settings_json,
            get_project_bundle,
            list_backups,
            restore_backup,
            detect_ollama,
            list_ollama_models,
            get_ollama_config,
            write_sandbox_config,
            write_approval_policies,
            write_toml_config,
            parse_openclaw_workspace,
            parse_gemini_agent,
            watch_project_files,
            stop_watching_project,
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
            // v0.5.5: Notifications
            save_notification_channel,
            list_notification_channels,
            delete_notification_channel,
            toggle_notification_channel,
            send_notification,
            test_notification_channel,
            // v1.0.0: Telemetry & Analytics
            get_telemetry_settings,
            update_telemetry_settings,
            track_event,
            get_queued_events,
            export_telemetry_events,
            get_analytics_summary,
            // v1.0.0: Audit Logging
            add_audit_log,
            get_audit_logs,
            get_audit_log_stats,
            clear_audit_logs,
            // v1.0.0: LLM API Key Management
            save_llm_api_key,
            list_llm_api_keys,
            get_llm_api_key_value,
            rotate_llm_api_key,
            toggle_llm_api_key,
            delete_llm_api_key,
            // v1.0.0: Real-time Agent Monitoring
            get_monitoring_snapshot,
            get_token_timeline,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex(b"hello world");
        assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn test_sha256_hex_empty() {
        let hash = sha256_hex(b"");
        assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn test_compute_diff_identical() {
        let (diff, added, removed) = compute_diff("hello\nworld", "hello\nworld");
        assert_eq!(added, 0);
        assert_eq!(removed, 0);
        assert!(diff.is_empty() || diff.iter().all(|d| d.kind == "context"));
    }

    #[test]
    fn test_compute_diff_addition() {
        let (diff, added, removed) = compute_diff("line1\nline3", "line1\nline2\nline3");
        // prefix/suffix algorithm: "line1" is common prefix, "line3" is common suffix,
        // middle is "nothing" (old) vs "line2" (new) → 1 added, 0 removed
        assert_eq!(added, 1);
        assert_eq!(removed, 0);
        assert!(diff.iter().any(|d| d.kind == "add"));
    }

    #[test]
    fn test_compute_diff_removal() {
        let (diff, added, removed) = compute_diff("a\nb\nc", "a\nc");
        assert!(removed > 0);
        assert!(diff.iter().any(|d| d.kind == "remove"));
    }

    #[test]
    fn test_validate_settings_json_valid() {
        let result = validate_settings_json(r#"{"permissions": {"allow": ["Read"]}}"#.to_string()).unwrap();
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_settings_json_invalid_json() {
        let result = validate_settings_json("not json".to_string()).unwrap();
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_validate_settings_json_bad_permissions() {
        let result = validate_settings_json(r#"{"permissions": "bad"}"#.to_string()).unwrap();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.field == "permissions"));
    }

    #[test]
    fn test_validate_settings_json_bad_mcp_server() {
        let result = validate_settings_json(
            r#"{"mcpServers": {"test": {"noCommand": true}}}"#.to_string()
        ).unwrap();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.field.contains("mcpServers")));
    }

    #[test]
    fn test_validate_settings_json_valid_mcp() {
        let result = validate_settings_json(
            r#"{"mcpServers": {"fs": {"command": "npx", "args": ["mcp-fs"]}}}"#.to_string()
        ).unwrap();
        assert!(result.valid);
    }

    #[test]
    fn test_validate_settings_json_unknown_keys_ok() {
        let result = validate_settings_json(
            r#"{"customKey": "value", "another": 42}"#.to_string()
        ).unwrap();
        assert!(result.valid);
    }

    #[test]
    fn test_parse_toml_to_json_basic() {
        let result = parse_toml_to_json("[model]\nname = \"gpt-4\"\ntemperature = 0.7\n");
        let model = result.get("model").unwrap();
        assert_eq!(model.get("name").unwrap().as_str().unwrap(), "gpt-4");
        assert!((model.get("temperature").unwrap().as_f64().unwrap() - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_parse_toml_to_json_nested() {
        let result = parse_toml_to_json("[a.b]\nc = true\n");
        assert_eq!(result["a"]["b"]["c"].as_bool().unwrap(), true);
    }

    #[test]
    fn test_parse_toml_to_json_array() {
        let result = parse_toml_to_json("ports = [80, 443, 8080]\n");
        let ports = result["ports"].as_array().unwrap();
        assert_eq!(ports.len(), 3);
    }

    #[test]
    fn test_parse_toml_to_json_invalid() {
        let result = parse_toml_to_json("this is not toml [[[");
        assert!(result.get("_parse_error").is_some());
    }

    #[test]
    fn test_json_to_toml_roundtrip() {
        let json = serde_json::json!({"model": {"name": "gpt-4", "temperature": 0.7}});
        let toml_str = json_to_toml(&json).unwrap();
        assert!(toml_str.contains("gpt-4"));
        let back = parse_toml_to_json(&toml_str);
        assert_eq!(back["model"]["name"].as_str().unwrap(), "gpt-4");
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(400), 100);
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(3), 0);
    }

    #[test]
    fn test_file_ref_nonexistent() {
        let f = file_ref("test", PathBuf::from("/nonexistent/path/file.md"), "user");
        assert!(!f.exists);
        assert_eq!(f.size_bytes, 0);
        assert_eq!(f.token_estimate, 0);
    }

    #[test]
    fn test_file_ref_existing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "hello world").unwrap();
        let f = file_ref("test.md", file, "project");
        assert!(f.exists);
        assert_eq!(f.size_bytes, 11);
        assert_eq!(f.token_estimate, 2);
        assert_eq!(f.scope, "project");
    }

    #[test]
    fn test_backup_file_creates_backup() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.json");
        fs::write(&file, r#"{"key": "value"}"#).unwrap();
        let result = backup_file(&file);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_backup_file_nonexistent() {
        let result = backup_file(&PathBuf::from("/nonexistent/file.txt"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_parse_sandbox_config_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(parse_sandbox_config(&dir.path().to_path_buf()).is_none());
    }

    #[test]
    fn test_parse_sandbox_config_json() {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join(".codex");
        fs::create_dir_all(&codex).unwrap();
        fs::write(codex.join("sandbox.json"), r#"{"sandbox":{"enabled":true,"network_isolation":true,"filesystem_policy":"read-only","timeout_secs":300}}"#).unwrap();
        let c = parse_sandbox_config(&dir.path().to_path_buf()).unwrap();
        assert!(c.enabled);
        assert!(c.network_isolation);
        assert_eq!(c.filesystem_policy, "read-only");
        assert_eq!(c.timeout_secs, Some(300));
    }

    #[test]
    fn test_parse_approval_policies_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(parse_approval_policies(&dir.path().to_path_buf()).is_empty());
    }

    #[test]
    fn test_parse_approval_policies_json() {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join(".codex");
        fs::create_dir_all(&codex).unwrap();
        fs::write(codex.join("policies.json"), r#"{"approvalPolicies":{"file_write":"on-request","shell":"never"}}"#).unwrap();
        let r = parse_approval_policies(&dir.path().to_path_buf());
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn test_collect_hooks_from_settings() {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().join("settings.json");
        fs::write(&s, r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"echo pre"}]}]}}"#).unwrap();
        let r = collect_hooks_from_settings(&s, "project");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].event, "PreToolUse");
        assert_eq!(r[0].command, "echo pre");
    }

    #[test]
    fn test_parse_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().join("settings.json");
        fs::write(&s, r#"{"permissions":{"allow":["Read","Bash"],"deny":["Write"]}}"#).unwrap();
        let r = parse_permissions_from_settings(&s, "user");
        assert_eq!(r.allow, vec!["Read", "Bash"]);
        assert_eq!(r.deny, vec!["Write"]);
        assert!(r.ask.is_empty());
    }

    #[test]
    fn test_parse_mcp_stdio() {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().join("s.json");
        fs::write(&s, r#"{"mcpServers":{"fs":{"command":"npx","args":["mcp-fs"]}}}"#).unwrap();
        let r = parse_mcp_from_settings(&s, "user");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, "stdio");
    }

    #[test]
    fn test_parse_mcp_http() {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().join("s.json");
        fs::write(&s, r#"{"mcpServers":{"api":{"url":"https://mcp.example.com"}}}"#).unwrap();
        let r = parse_mcp_from_settings(&s, "project");
        assert_eq!(r[0].kind, "http");
    }

    #[test]
    fn test_nested_claude_md() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("packages").join("core");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("CLAUDE.md"), "nested").unwrap();
        let r = list_nested_claude_md(&dir.path().to_path_buf(), 4);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].scope, "nested");
    }

    #[test]
    fn test_nested_claude_md_depth_limit() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a").join("b").join("c").join("d").join("e");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("CLAUDE.md"), "too deep").unwrap();
        assert!(list_nested_claude_md(&dir.path().to_path_buf(), 3).is_empty());
    }

    #[test]
    fn test_directory_resolves_to_skill_md() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("my-skill");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "# My Skill").unwrap();
        let r = read_agent_config_file(skill.to_string_lossy().to_string()).unwrap();
        assert!(r.raw.contains("My Skill"));
        assert!(r.path.ends_with("SKILL.md"));
    }

    #[test]
    fn test_directory_no_skill_errors() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        let r = read_agent_config_file(empty.to_string_lossy().to_string());
        assert!(r.is_err());
    }

    #[test]
    fn test_validate_env_bad_value() {
        let r = validate_settings_json(r#"{"env":{"K":123}}"#.to_string()).unwrap();
        assert!(!r.valid);
    }

    #[test]
    fn test_diff_empty_to_content() {
        let (_, added, removed) = compute_diff("", "line1\nline2");
        assert_eq!(added, 2);
        assert_eq!(removed, 0);
    }
}
