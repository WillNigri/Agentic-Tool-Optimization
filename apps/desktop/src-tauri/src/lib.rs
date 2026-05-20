mod openclaw_ws;
mod byok;
mod encryption;
mod log_watcher;
mod passive_observer;
mod health_poller;
mod telemetry;
mod file_attribution;
mod active_runs;
mod ratchet_view;
mod remote_runtimes_view;
mod runtime_health;
mod sessions_view;
pub mod pty;
pub mod local_insights;
pub mod events;
pub mod recipes;
pub mod posts;
pub mod api_dispatch;
pub mod api_dispatch_tools;
pub mod recipes_engine;
mod schema;

use rusqlite::{Connection, params};
use schema::init_database;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{State, Manager, Emitter};
pub use log_watcher::LogWatcherState;
pub use passive_observer::PassiveObserverState;
pub use health_poller::HealthPollerState;
pub use telemetry::TelemetryState;
pub use pty::PtyState;
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

// v1.4.0 F4 — Multi-agent groups. A router agent + N specialized children.
// The router decides which child handles each incoming prompt; specialization
// keeps each child's tool set + prompt small + focused.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentGroup {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
    pub runtime: String,
    /// JSON-encoded {rules: [...], llmFallback: {enabled, model}}.
    pub router_config: Option<String>,
    pub file_path: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub members: Vec<AgentGroupMember>,
    /// "routed" (router picks one) | "sequential" (children run in order,
    /// each receiving the previous output as input). Defaults to "routed"
    /// for backwards compatibility with existing groups.
    pub dispatch_kind: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentGroupMember {
    pub agent_id: String,
    pub agent_slug: String,
    pub agent_display_name: String,
    /// The child agent's runtime — useful for sequential dispatch where
    /// each child can run on its own runtime (Claude → Codex pipelines).
    /// Optional for backwards compat with serialized state that lacked it.
    #[serde(default)]
    pub agent_runtime: String,
    pub role: String, // 'router' | 'child'
    pub position: i32,
}

// v1.3.0 — Agents (T3). Records produced by the Create Agent wizard.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
    pub runtime: String,                  // claude | codex | gemini | openclaw | hermes
    pub model: Option<String>,
    pub project_id: Option<String>,
    pub system_prompt: Option<String>,
    pub permissions: Option<String>,      // JSON-encoded array of allowed tools
    pub skills: Option<String>,           // JSON-encoded array of skill IDs
    pub mcps: Option<String>,             // JSON-encoded array of MCP server names
    pub goal: Option<String>,             // original "what do you want?" text
    pub file_path: Option<String>,        // where the agent file landed on disk
    pub created_at: String,
    pub last_used_at: Option<String>,
    // v1.4.0 additions (column added via ALTER TABLE in init_database).
    pub role_models: Option<String>,      // JSON {router?, summarizer?, response?, evaluator?}
    pub memory_policy: Option<String>,    // JSON {summarizeAfter, keepLastK, summarizerModel}
    // v2.0.0 — "internal" runs on the developer's laptop via local CLI; "external"
    // is designed for customer-facing deployment (embed widget, Cloudflare Worker,
    // etc.) and locks the agent down to a read-only permission set.
    pub kind: Option<String>,             // 'internal' | 'external' (default 'internal')
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
    /// v2.3.41 — links the row to a Phase 6 session. NULL for
    /// standalone dispatches. The History panel groups rows that
    /// share a session_id under one collapsible header so multi-turn
    /// conversations read like a chat.
    pub session_id: Option<String>,
    /// v2.4.5 — Tier 2 review audit. Number of function-calling tool
    /// invocations this dispatch made (read_file / grep / git_log).
    /// NULL for non-tool dispatches. 0 means "tools were offered but
    /// the model declined." The GUI badges this so reviewers can see
    /// at a glance "verified via N tool calls" vs "prompt-only."
    pub tool_calls_count: Option<i64>,
    /// JSON array of {name, args_brief, is_error} for each call.
    pub tool_calls_summary: Option<String>,
    /// v2.4.6 — agent persona when this dispatch was driven by a
    /// specialist agent (e.g. `@security-specialist`). The GUI
    /// renders persona + the underlying runtime/model together so a
    /// "no findings from @security-specialist" reads as "Gemini in a
    /// security frame found nothing," not as expert validation.
    pub agent_slug: Option<String>,
    pub model: Option<String>,
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

/// v2.4.8 audit H1 — migrate legacy plain-base64 llm_api_keys rows
/// into AES-GCM v1 format. Scans rows that don't start with the
/// v1 prefix; for each, decrypts as legacy (= base64-decode),
/// re-encrypts via crate::encryption::encrypt, and UPDATEs the row.
/// Errors are logged + skipped — a missing keychain or a corrupted
/// row shouldn't block the rest of the migration or app startup.
pub(crate) fn migrate_legacy_api_keys(conn: &Connection) {
    // Read-side first: collect candidate (id, current_value) pairs
    // so we don't hold a read statement open during writes.
    let candidates: Vec<(String, String)> = match conn.prepare(
        "SELECT id, encrypted_key FROM llm_api_keys WHERE encrypted_key NOT LIKE 'v1:%'",
    ) {
        Ok(mut stmt) => stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default(),
        Err(e) => {
            eprintln!("[security] migrate_legacy_api_keys: prepare failed: {}", e);
            return;
        }
    };
    if candidates.is_empty() {
        return;
    }
    let mut migrated = 0usize;
    for (id, legacy) in &candidates {
        let plain = match crate::encryption::decrypt(legacy) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[security] migrate_legacy_api_keys: skip id={} (legacy decode failed: {})", id, e);
                continue;
            }
        };
        let v1 = match crate::encryption::encrypt(&plain) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[security] migrate_legacy_api_keys: encrypt failed (keychain unavailable?): {}", e);
                // No point trying further rows; they'll all fail.
                return;
            }
        };
        let now = chrono::Utc::now().to_rfc3339();
        if let Err(e) = conn.execute(
            "UPDATE llm_api_keys SET encrypted_key = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![v1, now, id],
        ) {
            eprintln!("[security] migrate_legacy_api_keys: UPDATE failed id={}: {}", id, e);
            continue;
        }
        migrated += 1;
    }
    if migrated > 0 {
        eprintln!(
            "[security] migrated {} legacy llm_api_keys row(s) to AES-GCM v1",
            migrated
        );
    }
}



pub mod commands;
pub use commands::*;

// ── App Entry ────────────────────────────────────────────────────────────

pub fn run() {
    // Headless cron entry — when launchd / cron / Task Scheduler invokes
    // `ato-desktop --run-cron <id>`, we dispatch the job and exit without
    // opening any window. Detected before tauri::Builder runs so the GUI
    // never tries to spin up.
    let args: Vec<String> = std::env::args().collect();
    if let Some(idx) = args.iter().position(|a| a == "--run-cron") {
        if let Some(id) = args.get(idx + 1).cloned() {
            let exit_code = commands::run_cron_headless(id);
            std::process::exit(exit_code);
        }
        eprintln!("--run-cron requires a job id");
        std::process::exit(2);
    }

    let db_path = get_db_path();
    let conn = Connection::open(&db_path).expect("Failed to open SQLite database");
    // v2.3.7 — 5s busy_timeout. With the CLI now also writing to the
    // same DB, overlap is common; without this, both sides see
    // transient "database is locked" errors on first contention.
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    // v2.4.8 audit H2 — restrict DB file perms to 600 on Unix. The
    // file contains llm_api_keys (now AES-encrypted, but other rows
    // like cloud_traces hold prompt content), execution_logs, and
    // session data. World-readable was the default until this
    // commit; existing files are chmod'd on every startup so the
    // upgrade lands without user action.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&db_path) {
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o600 {
                let mut perms = meta.permissions();
                perms.set_mode(0o600);
                if let Err(e) = std::fs::set_permissions(&db_path, perms) {
                    eprintln!("[security] could not chmod 600 {}: {}", db_path.display(), e);
                }
            }
        }
    }
    // v2.3.8 Phase 4.2 — seed the in-memory event sequence counter
    // from the highest event_seq already persisted, so the counter
    // stays strictly increasing across desktop restarts. Must happen
    // after init_database has created (or migrated) the events_log
    // table, before any event is published.
    events::bus::init_seq_from_db(&db_path);
    init_database(&conn);
    // v2.4.8 audit H1 migration — re-encrypt legacy plain-base64
    // llm_api_keys rows into AES-GCM v1. Best-effort: a keychain
    // miss (e.g. headless / first launch ever) means we leave the
    // rows as-is for now; the next launch with a working keychain
    // will migrate them. Old rows stay decryptable in the meantime
    // via the encryption module's legacy fallback path.
    migrate_legacy_api_keys(&conn);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(DbState(Mutex::new(conn)))
        .manage(sessions_view::CloseInflight::new())
        .manage(LogWatcherState::new())
        .manage(PassiveObserverState::new())
        .manage(HealthPollerState::new())
        .manage(TelemetryState::new())
        .manage(PtyState::new())
        .setup(|app| {
            // Auto-start health poller on app launch
            let db_path_str = get_db_path().to_string_lossy().to_string();
            let poller_state = app.state::<HealthPollerState>();
            let poller = poller_state.0.lock().unwrap();
            poller.start(app.handle().clone(), db_path_str);
            // v2.3.8 Phase 4.2 — start the recipe execution engine.
            // Tokio task lives for the duration of the desktop process,
            // subscribes to events::bus, runs matching recipe actions.
            recipes_engine::start();
            // v2.6 PR-A — start the passive observer. Watches
            // ~/.claude/projects + ~/.codex/sessions and turns every
            // user→assistant turn from external CLI sessions into a
            // `dispatch_kind='passive_observation'` execution_logs row.
            // Missing directories aren't an error — the watcher
            // simply registers no source for that CLI and stays idle.
            let observer_state = app.state::<PassiveObserverState>();
            if let Ok(mut obs) = observer_state.0.lock() {
                if let Err(e) = obs.start(get_db_path()) {
                    eprintln!("passive_observer: start failed: {}", e);
                }
            }
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
            list_skill_versions,
            restore_skill_version,
            delete_skill_version,
            export_configuration,
            import_configuration,
            list_chat_threads,
            search_chat_threads,
            create_chat_thread,
            rename_chat_thread,
            delete_chat_thread,
            set_chat_thread_agent,
            get_chat_messages,
            append_chat_message,
            delete_chat_message,
            prompt_agent_stream,
            prompt_agent_with_history_stream,
            prompt_claude,
            list_workflows,
            save_workflow,
            load_workflow,
            delete_workflow,
            detect_agent_runtimes,
            set_runtime_path,
            get_runtime_path,
            list_runtime_preferences,
            set_runtime_monitored,
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
            cron_os_scheduler_supported,
            cron_os_scheduler_kind,
            register_cron_os_scheduler,
            unregister_cron_os_scheduler,
            is_cron_os_scheduler_registered,
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
            // v2.1.0 Replay infra
            link_execution_log_to_cloud_trace,
            start_replay,
            get_replay_job,
            list_replays_for_trace,
            get_execution_log_response_by_cloud_trace_id,
            get_execution_log_io_by_cloud_trace_id,
            // v2.3.2 Phase 2 — local-mode insights
            compute_regressions_local,
            compute_cost_recommendations_local,
            record_local_config_change,
            // v2.6 PR-A — observatory summary
            compute_billing_surface_summary,
            // v2.3.36 Phase 6.x-I.2 — runtime-binary health
            runtime_health::runtime_health_check,
            runtime_health::runtime_health_run_fix,
            // v2.3.42 — sessions view (Phase 6 surface in the GUI)
            sessions_view::list_sessions_full,
            sessions_view::get_session_transcript,
            // PR 5c — single-shot single-run dispatch detail (read of
            // execution_logs rows that don't have a session)
            sessions_view::get_single_run_detail,
            // PR 14c — war-room drill-in: returns the participating
            // execution_logs for a given war_room_id
            sessions_view::get_war_room_constituents,
            // First-Chat Wizard (2026-05-18) — fan-out parallel
            // dispatches across N runtimes sharing a war_room_id
            sessions_view::dispatch_war_room,
            // v2.3.43 — sessions GUI completion: New / Continue / Bridge
            sessions_view::create_session,
            sessions_view::dispatch_into_session,
            sessions_view::bridge_session,
            // v2.3.48 — streaming dispatch (Phase 6.x-F GUI render)
            sessions_view::dispatch_into_session_streaming,
            // v2.6 Slice C — explicit session close/reopen lifecycle
            sessions_view::close_session,
            sessions_view::cancel_close_session,
            sessions_view::reopen_session,
            sessions_view::search_session_turns,
            // 2026-05-16 — cost receipts panel
            sessions_view::get_session_cost_breakdown,
            // v2.3.45 — ratchet view (Phase 6.x-K surface in the GUI)
            ratchet_view::list_ratchets,
            ratchet_view::list_ratchet_breaches,
            // v2.3.49 — ratchet lock/unlock from the GUI
            ratchet_view::lock_ratchet,
            ratchet_view::unlock_ratchet,
            // v2.3.52 — Settings → Runtimes → Remote (Phase 6.x-J GUI)
            remote_runtimes_view::list_remote_runtimes,
            remote_runtimes_view::add_remote_runtime,
            remote_runtimes_view::remove_remote_runtime,
            remote_runtimes_view::list_ssh_key_candidates,
            // BYOK per-runtime auth mode picker
            byok::get_runtime_auth_info,
            byok::set_runtime_auth_mode,
            byok::get_credit_burn_summary,
            // v2.3.7 Phase 4 — ops recipes
            recipes_list,
            recipes_get,
            recipes_create,
            recipes_set_enabled,
            recipes_delete,
            recipes_templates,
            recipes_install_template,
            // v2.3.20 Phase 5.5 — Activity feed (posts)
            posts_list,
            posts_create,
            posts_pending,
            posts_decide,
            // v2.3.24 Phase 5.6 — sidebar badge
            posts_pending_count,
            // v2.3.23 Phase 6.x-B — unified runtime picker
            list_available_runtimes,
            // v2.3.26 Phase 6.x-C — GUI dispatch for API providers
            prompt_api_provider,
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
            // v1.3.0: Agents (T3)
            create_agent,
            list_agents,
            get_agent,
            delete_agent,
            touch_agent_last_used,
            // v1.4.0 F1: Agent variables (dynamic prompt resolvers)
            list_agent_variables,
            save_agent_variable,
            delete_agent_variable,
            prompt_agent_with_context,
            prompt_agent_with_history,
            // v1.4.0 F2: Pre-call context hooks
            list_agent_hooks,
            save_agent_hook,
            delete_agent_hook,
            // v1.4.0 F3: Memory policy
            update_agent_memory_policy,
            // v2.0.0: Internal vs External agent kind
            update_agent_kind,
            // v2.0.0 Wave 2: Local knowledge ingestion + retrieval
            ingest_knowledge_text,
            list_agent_knowledge,
            delete_knowledge_chunk,
            delete_knowledge_source,
            retrieve_knowledge,
            // v1.4.0 F5: Per-task model selection
            update_agent_role_models,
            // v1.5.0: Update MCPs attached to an agent (one-click browser tools etc.)
            update_agent_mcps,
            // v2.7.9 Felipe P5: default dispatch prompt
            update_agent_default_prompt,
            get_agent_default_prompt,
            // v1.4.0 F4: Multi-agent groups (router + children)
            create_agent_group,
            list_agent_groups,
            get_agent_group,
            update_agent_group,
            delete_agent_group,
            dispatch_to_group,
            // v1.4.0 F6: Observability (reads ~/.ato/agent-logs.jsonl)
            read_agent_traces,
            get_agent_metrics,
            // v1.4.0 F7: Evaluators (heuristic local; LLM-as-judge stub)
            list_agent_evaluators,
            save_agent_evaluator,
            delete_agent_evaluator,
            evaluate_recent_traces,
            // v1.3.0: Embedded terminal (T5)
            pty::pty_spawn,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_kill,
            pty::pty_list,
            // v1.3.0: MCP install (T4 follow-up)
            install_mcp_server,
            uninstall_mcp_server,
            delete_llm_api_key,
            // v1.0.0: Real-time Agent Monitoring
            get_monitoring_snapshot,
            get_token_timeline,
            // v2.1.0: Per-dispatch file attribution — answers "which agent
            // touched which files" by mtime-snapshotting the project root
            // before/after each dispatch.
            file_attribution::snapshot_project_files,
            file_attribution::diff_project_files,
            // v2.1.0 Phase 4: Active runs registry — answers "which
            // runtime is running where" + enables one-click kill from
            // the dashboard, no terminal-buffer hunting required.
            active_runs::list_active_runs,
            active_runs::kill_active_run,
            active_runs::get_overlap_evidence,
            active_runs::finish_active_run,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── H1 migration smoke test ───────────────────────────────────
    //
    // Verifies the end-to-end "legacy plain-base64 row → AES-GCM v1"
    // path:
    //   1. Build a temp DB with the llm_api_keys schema
    //   2. Insert a row with `encrypted_key = base64(plaintext)` —
    //      the pre-2.4.8 format
    //   3. Run migrate_legacy_api_keys
    //   4. Read the row back, assert the prefix is now "v1:"
    //   5. Decrypt through the same module the dispatch path uses,
    //      assert the round-trip equals the original plaintext
    //
    // Gated on ATO_ENCRYPTION_TESTS=1 because the migration touches
    // the OS keychain (no DBus on CI Linux runners, no Keychain on
    // headless macOS). Set the env var on a dev machine.
    #[test]
    fn migrate_legacy_row_to_v1() {
        use base64::Engine as _;
        if std::env::var("ATO_ENCRYPTION_TESTS").ok().as_deref() != Some("1") {
            eprintln!("skipping migration smoke (set ATO_ENCRYPTION_TESTS=1 to run)");
            return;
        }
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let conn = rusqlite::Connection::open(tmp.path()).expect("open temp db");
        conn.execute_batch(
            "CREATE TABLE llm_api_keys (
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
            );",
        )
        .expect("schema");

        let plaintext = "sk-ant-test-migration-key-do-not-leak-xyz";
        let legacy = base64::engine::general_purpose::STANDARD.encode(plaintext.as_bytes());
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO llm_api_keys
               (id, provider, name, key_preview, encrypted_key,
                project_id, runtime, is_active, usage_count, created_at, updated_at)
             VALUES ('row-1', 'anthropic', 'test-key', 'sk-ant...xyz', ?1,
                     NULL, NULL, 1, 0, ?2, ?2)",
            rusqlite::params![&legacy, &now],
        )
        .expect("insert legacy row");

        // Pre-migration assertion: row is NOT in v1 format yet.
        let before: String = conn
            .query_row(
                "SELECT encrypted_key FROM llm_api_keys WHERE id = 'row-1'",
                [],
                |r| r.get(0),
            )
            .expect("pre-migration read");
        assert!(
            !crate::encryption::is_v1(&before),
            "expected legacy format before migration, got {}",
            before
        );

        // Run the migration.
        migrate_legacy_api_keys(&conn);

        // After: row should be v1, and a fresh decrypt round-trips
        // to the original plaintext.
        let after: String = conn
            .query_row(
                "SELECT encrypted_key FROM llm_api_keys WHERE id = 'row-1'",
                [],
                |r| r.get(0),
            )
            .expect("post-migration read");
        assert!(
            crate::encryption::is_v1(&after),
            "expected v1 prefix after migration, got {}",
            after
        );
        let round_tripped = crate::encryption::decrypt(&after).expect("decrypt migrated row");
        assert_eq!(round_tripped, plaintext, "round-trip mismatch");

        // Idempotence: running the migration again should be a no-op
        // (no rows match the WHERE NOT LIKE 'v1:%' filter).
        migrate_legacy_api_keys(&conn);
        let after2: String = conn
            .query_row(
                "SELECT encrypted_key FROM llm_api_keys WHERE id = 'row-1'",
                [],
                |r| r.get(0),
            )
            .expect("idempotence read");
        assert_eq!(after, after2, "migration was not idempotent");
    }

    // ── H2 perm-tightening smoke test ────────────────────────────
    //
    // Verifies the chmod 600 logic on a synthetic file. Unix-only;
    // doesn't need the keychain so this can run anywhere.
    #[test]
    #[cfg(unix)]
    fn db_perm_tightening_to_0600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        // Force the file to a world-readable state, matching what a
        // pre-2.4.8 install would have on disk.
        let mut perms = tmp.as_file().metadata().expect("metadata").permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(tmp.path(), perms).expect("set 0644");
        let before = tmp.as_file().metadata().expect("metadata").permissions().mode() & 0o777;
        assert_eq!(before, 0o644, "precondition: file should be 0644");

        // Inline the same chmod block from startup() so we test the
        // *behavior* without coupling the test to that big fn's
        // initialization order.
        if let Ok(meta) = std::fs::metadata(tmp.path()) {
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o600 {
                let mut p = meta.permissions();
                p.set_mode(0o600);
                std::fs::set_permissions(tmp.path(), p).expect("chmod 0600");
            }
        }
        let after = tmp.as_file().metadata().expect("metadata").permissions().mode() & 0o777;
        assert_eq!(after, 0o600, "post-chmod: file should be 0600");
    }

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
