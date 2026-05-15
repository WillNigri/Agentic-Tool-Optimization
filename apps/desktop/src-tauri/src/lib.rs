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
pub mod recipes_engine;

use rusqlite::{Connection, params};
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
        -- v2.5.1 — per-runtime monitored toggle. The Insights → Health
        -- panel only renders cards for runtimes the user opted into
        -- monitoring. First launch seeds this table by detecting which
        -- runtimes are installed (via which_cli) so the user doesn't
        -- start with red cards for runtimes they've never touched.
        -- Adding a new runtime is just a row with monitored=1.
        CREATE TABLE IF NOT EXISTS runtime_preferences (
            runtime   TEXT PRIMARY KEY,
            monitored INTEGER NOT NULL DEFAULT 1,
            updated_at TEXT NOT NULL
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
            cloud_trace_id TEXT,
            created_at   TEXT NOT NULL
        );
        -- v2.1.0 Replay infra. One row per replay dispatch the user
        -- triggered. source_execution_log_id references the original
        -- prompt; status drives the polling UI. Response capped at
        -- 64KB for the same reason execution_logs.response is.
        CREATE TABLE IF NOT EXISTS replay_jobs (
            id                       TEXT PRIMARY KEY,
            source_execution_log_id  TEXT NOT NULL,
            source_cloud_trace_id    TEXT,
            source_runtime           TEXT NOT NULL,
            source_model             TEXT,
            target_runtime           TEXT NOT NULL,
            target_model             TEXT,
            status                   TEXT NOT NULL,
            response                 TEXT,
            duration_ms              INTEGER,
            error_message            TEXT,
            started_at               TEXT NOT NULL,
            finished_at              TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_replay_jobs_source
            ON replay_jobs(source_execution_log_id, started_at DESC);
        CREATE INDEX IF NOT EXISTS idx_replay_jobs_cloud_trace
            ON replay_jobs(source_cloud_trace_id, started_at DESC);
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
        CREATE TABLE IF NOT EXISTS agents (
            id            TEXT PRIMARY KEY,
            slug          TEXT NOT NULL,
            display_name  TEXT NOT NULL,
            description   TEXT,
            runtime       TEXT NOT NULL,
            model         TEXT,
            project_id    TEXT,
            system_prompt TEXT,
            permissions   TEXT,
            skills        TEXT,
            mcps          TEXT,
            goal          TEXT,
            file_path     TEXT,
            created_at    TEXT NOT NULL,
            last_used_at  TEXT,
            UNIQUE (runtime, slug)
        );
        CREATE INDEX IF NOT EXISTS idx_agents_runtime ON agents(runtime);
        CREATE INDEX IF NOT EXISTS idx_agents_last_used ON agents(last_used_at DESC);
        CREATE INDEX IF NOT EXISTS idx_agents_project ON agents(project_id);
        -- v1.4.0 — Production-Grade Agent Authoring (context engineering).
        -- F1: Dynamic prompts with variables. Each row is one named variable
        --     belonging to an agent, with a kind-specific resolver config.
        CREATE TABLE IF NOT EXISTS agent_variables (
            id          TEXT PRIMARY KEY,
            agent_id    TEXT NOT NULL,
            name        TEXT NOT NULL,
            kind        TEXT NOT NULL,            -- static | env | project-path | file | db-query | mcp-call | computed
            config_json TEXT NOT NULL,
            enabled     INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            UNIQUE (agent_id, name)
        );
        CREATE INDEX IF NOT EXISTS idx_agent_vars_agent ON agent_variables(agent_id);
        -- F2: Pre-call context hooks. Ordered list of resolvers that run
        --     before each LLM turn and inject results into the user message.
        CREATE TABLE IF NOT EXISTS agent_hooks (
            id          TEXT PRIMARY KEY,
            agent_id    TEXT NOT NULL,
            position    INTEGER NOT NULL,
            name        TEXT NOT NULL,
            kind        TEXT NOT NULL,            -- mcp-call | file | db-query | webhook | computed
            config_json TEXT NOT NULL,
            enabled     INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_agent_hooks_agent ON agent_hooks(agent_id, position);
        -- F4: Multi-agent groups (router + children).
        CREATE TABLE IF NOT EXISTS agent_groups (
            id            TEXT PRIMARY KEY,
            slug          TEXT NOT NULL UNIQUE,
            display_name  TEXT NOT NULL,
            description   TEXT,
            runtime       TEXT NOT NULL,
            router_config TEXT,
            file_path     TEXT,
            created_at    TEXT NOT NULL,
            last_used_at  TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_groups_runtime ON agent_groups(runtime);
        CREATE TABLE IF NOT EXISTS agent_group_members (
            group_id    TEXT NOT NULL,
            agent_id    TEXT NOT NULL,
            role        TEXT NOT NULL,             -- 'router' | 'child'
            position    INTEGER NOT NULL,
            PRIMARY KEY (group_id, agent_id),
            FOREIGN KEY (group_id) REFERENCES agent_groups(id) ON DELETE CASCADE,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_group_members_agent ON agent_group_members(agent_id);
        -- v1.4.0 Polish-T2 — Skill version history. We snapshot a SKILL.md's
        -- contents on edit so the user can scroll back through prior versions
        -- and restore one. Versions live in SQLite (not on disk) — they are
        -- recovery state, not a vcs.
        CREATE TABLE IF NOT EXISTS skill_versions (
            id            TEXT PRIMARY KEY,
            file_path     TEXT NOT NULL,
            content       TEXT NOT NULL,
            content_hash  TEXT NOT NULL,
            note          TEXT,
            created_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_skill_versions_path ON skill_versions(file_path, created_at DESC);
        -- v1.5.0 — Persistent chat threads. Makes the bottom Chat pane a
        -- destination instead of an ephemeral input. A thread isn't bound to
        -- a runtime: each message records which runtime answered it, so the
        -- same conversation can hop runtimes mid-flight. project_id is
        -- optional — threads can be global.
        CREATE TABLE IF NOT EXISTS chat_threads (
            id              TEXT PRIMARY KEY,
            title           TEXT NOT NULL,
            project_id      TEXT,
            agent_id        TEXT,                       -- last-used agent (sticky default)
            created_at      TEXT NOT NULL,
            last_message_at TEXT,
            message_count   INTEGER NOT NULL DEFAULT 0,
            archived        INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_chat_threads_project
            ON chat_threads(project_id, last_message_at DESC);
        CREATE INDEX IF NOT EXISTS idx_chat_threads_recent
            ON chat_threads(last_message_at DESC);

        CREATE TABLE IF NOT EXISTS chat_messages (
            id          TEXT PRIMARY KEY,
            thread_id   TEXT NOT NULL,
            role        TEXT NOT NULL,                  -- 'user' | 'assistant' | 'system' | 'attachment' | 'error'
            content     TEXT NOT NULL,
            runtime     TEXT,                           -- which runtime produced this turn (assistant only)
            agent_slug  TEXT,                           -- which agent (if any) handled the dispatch
            metadata    TEXT,                           -- JSON: file path for attachments, etc.
            created_at  TEXT NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES chat_threads(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_chat_messages_thread
            ON chat_messages(thread_id, created_at ASC);
        ",
    )
    .expect("Failed to initialize database tables");

    // F3 + F5 — additive columns on the existing `agents` table. Wrapped in
    // separate calls so existing local.db files upgrade without complaint.
    // SQLite returns "duplicate column" if the column already exists; ignore.
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN role_models_json TEXT", []);
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN memory_policy_json TEXT", []);
    // v1.5.0 — dispatch kind on agent groups: "routed" (router picks one
    // child) vs "sequential" (children run in order, output of N is input
    // to N+1). Default keeps existing groups behaving as before.
    let _ = conn.execute(
        "ALTER TABLE agent_groups ADD COLUMN dispatch_kind TEXT NOT NULL DEFAULT 'routed'",
        [],
    );
    // v2.0.0 — Internal vs External agent kind.
    let _ = conn.execute(
        "ALTER TABLE agents ADD COLUMN kind TEXT NOT NULL DEFAULT 'internal'",
        [],
    );
    // v2.1.0 — execution_logs links to its corresponding cloud
    // agent_traces row when the dispatch was uploaded. Powers replay
    // ("look up the local prompt for this cloud trace ID"). Existing
    // rows stay NULL and won't be replayable, which is honest — they
    // predate the link plumbing.
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN cloud_trace_id TEXT", []);
    // v2.0.0 Wave 2 — Local knowledge for external agents. Each row is one
    // chunk of text + its OpenAI text-embedding-3-small vector. Embedding
    // stored as a BLOB of f32 bytes (1536 floats = 6144 bytes per chunk).
    // Storage trade-off: keeping the embedding alongside the text means the
    // bundle inliner doesn't have to re-embed at deploy time, AND retrieval
    // testing in the UI is just a SELECT + cosine sim in Rust.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_knowledge_chunks (
            id          TEXT PRIMARY KEY,
            agent_id    TEXT NOT NULL,
            source      TEXT NOT NULL,
            content     TEXT NOT NULL,
            tokens      INTEGER NOT NULL,
            position    INTEGER NOT NULL,
            embedding   BLOB NOT NULL,
            embed_model TEXT NOT NULL,
            created_at  TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_kchunks_agent ON agent_knowledge_chunks(agent_id, position)",
        [],
    );
    // v2.0.0 Wave 4 — fire-mode for context hooks.
    // 'always'      = current behavior, hook fires every turn
    // 'keyword'     = fire only when user_prompt matches one of the
    //                 keywords stored in config_json.whenKeywords[]
    // 'llm-decides' = ask config_json.classifierModel "should this hook
    //                 fire?" given config_json.whenDescription
    let _ = conn.execute(
        "ALTER TABLE agent_hooks ADD COLUMN fire_mode TEXT NOT NULL DEFAULT 'always'",
        [],
    );
    // v2.2.0 — captured cost per dispatch. execution_logs already has
    // tokens_in / tokens_out; we add the computed USD value alongside so
    // panels can read a single column instead of recomputing on every
    // render. replay_jobs gets token + cost columns from scratch (the
    // table is v2.1.0 and was shipped without them).
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN cost_usd_estimated REAL", []);
    let _ = conn.execute("ALTER TABLE replay_jobs ADD COLUMN input_tokens INTEGER", []);
    let _ = conn.execute("ALTER TABLE replay_jobs ADD COLUMN output_tokens INTEGER", []);
    let _ = conn.execute("ALTER TABLE replay_jobs ADD COLUMN cost_usd_estimated REAL", []);
    // v2.3.2 — Phase 2: local-mode regressions + cost recommendations.
    // The cloud computes both over agent_traces × agent_config_changes;
    // for the offline-first surface we mirror enough locally to run
    // the same algorithm without a sign-in. Two additions to
    // execution_logs (agent_slug + model) make per-agent + per-model
    // aggregation possible; a new agent_config_changes table holds
    // the ledger.
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN agent_slug TEXT", []);
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN model TEXT", []);
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_agent_slug ON execution_logs(agent_slug, created_at DESC)",
        [],
    );
    // v2.3.41 — session_id on execution_logs lets the History panel
    // group multi-turn conversations under one collapsible header
    // instead of scattering them. NULL for standalone (non --session)
    // dispatches; populated by dispatch::run when --session is passed.
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN session_id TEXT", []);
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_session_id ON execution_logs(session_id, created_at ASC)",
        [],
    );
    // v2.4.5 — tool-call telemetry for Tier 2 review. Lets the GUI
    // distinguish "this reviewer verified findings via N tool calls"
    // from "prompt-only". tool_calls_summary is a JSON array of
    // {name, args_brief, is_error} so the Runs panel can render a
    // chronological list without re-parsing the response text.
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN tool_calls_count INTEGER", []);
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN tool_calls_summary TEXT", []);
    // 2026-05-14 — record which auth path the dispatch used so the
    // credit-burn meter can split "subscription" cost (counts against
    // Anthropic's Agent SDK credit pool starting June 15) from
    // "api_key" cost (billed directly to the user's API account).
    // Pre-migration rows have NULL; the analytics query treats NULL
    // as "unknown" rather than guessing.
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN auth_mode TEXT", []);
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_config_changes (
            id          TEXT PRIMARY KEY,
            agent_slug  TEXT NOT NULL,
            field       TEXT NOT NULL,
            old_value   TEXT,
            new_value   TEXT,
            actor       TEXT,
            changed_at  TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_config_changes_slug_time ON agent_config_changes(agent_slug, changed_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_config_changes_field ON agent_config_changes(field, changed_at DESC)",
        [],
    );

    // v2.3.0 — live_runs SQLite mirror of the in-memory active_runs
    // registry. The registry stays authoritative; this mirror exists
    // so the `ato` CLI (a separate process) can read what's currently
    // running without IPC. Rows are best-effort INSERT'd by
    // active_runs::begin_run and DELETE'd by finish_run; if the writes
    // fail (DB locked, etc) the in-memory map is unaffected.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS live_runs (
            run_id      TEXT PRIMARY KEY,
            agent_slug  TEXT,
            runtime     TEXT NOT NULL,
            workspace   TEXT,
            source      TEXT,
            started_at  TEXT NOT NULL,
            status      TEXT NOT NULL DEFAULT 'running',
            child_pid   INTEGER
        )",
        [],
    );
    // Backfill for installs that already created live_runs before the
    // child_pid column existed.
    let _ = conn.execute("ALTER TABLE live_runs ADD COLUMN child_pid INTEGER", []);
    // v2.6 PR-A — observatory columns mirrored on live_runs so the chip
    // in the Live tab can render for active dispatches without a join.
    // Passive rows synthesized from the watcher are NOT written into
    // live_runs (they aren't kill-able processes); they only land in
    // execution_logs. Defaults make existing rows behave as before.
    let _ = conn.execute(
        "ALTER TABLE live_runs ADD COLUMN dispatch_kind TEXT NOT NULL DEFAULT 'active'",
        [],
    );
    let _ = conn.execute("ALTER TABLE live_runs ADD COLUMN billing_surface TEXT", []);
    // Clear stale rows from a previous desktop run. We're booting; if
    // any live_runs survived a prior crash, they're dead by definition.
    let _ = conn.execute("DELETE FROM live_runs", []);

    // v2.6 PR-A — observatory columns on execution_logs so passive
    // observations of foreign CLI sessions (claude code, codex, …) can
    // be persisted alongside ATO's own dispatches.
    //   dispatch_kind: 'active'  = ATO fired it
    //                  'passive_observation' = watcher saw it happen
    //   billing_surface: which auth path the upstream CLI used —
    //     claude_code_subscription / anthropic_api / codex_cli_subscription
    //     / openai_api / gemini_cli_subscription / gemini_api / ollama_local
    //     / unknown
    //   provider_session_id: upstream CLI's own session UUID; pairs
    //     with sequence_within_session for INSERT OR IGNORE dedup.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN dispatch_kind TEXT NOT NULL DEFAULT 'active'",
        [],
    );
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN billing_surface TEXT", []);
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN provider_session_id TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN sequence_within_session INTEGER",
        [],
    );
    // Dedup unique index — partial so non-watcher rows (NULL session id)
    // don't conflict with each other.
    let _ = conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_logs_session_seq \
            ON execution_logs(provider_session_id, sequence_within_session) \
            WHERE provider_session_id IS NOT NULL",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_dispatch_kind \
            ON execution_logs(dispatch_kind, created_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_billing_surface \
            ON execution_logs(billing_surface, created_at DESC)",
        [],
    );

    // v2.6 PR-A — watcher_state. One row per (source, file_path).
    // byte_offset = where the next read should start so re-ingest is
    // idempotent across desktop restarts. last_seq = the largest
    // sequence_within_session emitted from this file, so a hard crash
    // mid-line never re-emits the prior turn.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS watcher_state (
            source       TEXT NOT NULL,
            file_path    TEXT NOT NULL,
            byte_offset  INTEGER NOT NULL DEFAULT 0,
            last_seq     INTEGER NOT NULL DEFAULT 0,
            updated_at   TEXT NOT NULL,
            PRIMARY KEY (source, file_path)
        )",
        [],
    );

    // v2.3.7 Phase 4 — Ops recipes (user-authored trigger→action
    // workflows). trigger_config / action_config are TEXT (JSON
    // serialization of the typed enums in recipes.rs). Indexed by
    // trigger_type so the execution engine's dispatch path is O(log n)
    // when an event fires.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS ops_recipes (
            id              TEXT PRIMARY KEY,
            slug            TEXT NOT NULL UNIQUE,
            name            TEXT NOT NULL,
            description     TEXT,
            trigger_type    TEXT NOT NULL,
            trigger_config  TEXT NOT NULL,
            action_type     TEXT NOT NULL,
            action_config   TEXT NOT NULL,
            enabled         INTEGER NOT NULL DEFAULT 1,
            created_at      TEXT NOT NULL,
            updated_at      TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ops_recipes_trigger ON ops_recipes(trigger_type, enabled)",
        [],
    );

    // v2.3.8 Phase 4.2 — Event audit log. Every event published on
    // events::bus is persisted here. Powers `ato events recent` and
    // gives the execution engine a deterministic re-read path when a
    // subscriber lagged (RecvError::Lagged). event_seq mirrors the
    // monotonic counter from events::bus::next_seq.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS events_log (
            event_seq   INTEGER PRIMARY KEY,
            event_type  TEXT NOT NULL,
            payload     TEXT NOT NULL,
            occurred_at TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_events_log_type_time ON events_log(event_type, occurred_at DESC)",
        [],
    );
    // v2.3.15 Phase 4.9 — composite index for `ato events watch
    // --type X` (codex 4.8 nit). The "type, occurred_at DESC" index
    // doesn't support the watch query shape (WHERE event_type = ? AND
    // event_seq > ? ORDER BY event_seq ASC) without an extra sort
    // step on large ledgers. (event_type, event_seq) does.
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_events_log_type_seq ON events_log(event_type, event_seq)",
        [],
    );

    // v2.3.8 Phase 4.2 — Recipe execution audit. Every action the
    // engine runs leaves a row here so users can see "what did my
    // recipes actually do, when, did they succeed?" The trigger payload
    // is captured so re-runs are reproducible if we ever build a
    // "replay this recipe run" tool.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS ops_recipe_runs (
            id              TEXT PRIMARY KEY,
            recipe_id       TEXT NOT NULL,
            recipe_slug     TEXT NOT NULL,
            event_seq       INTEGER NOT NULL,
            event_type      TEXT NOT NULL,
            event_payload   TEXT NOT NULL,
            action_type     TEXT NOT NULL,
            status          TEXT NOT NULL,
            result          TEXT,
            error_message   TEXT,
            started_at      TEXT NOT NULL,
            finished_at     TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ops_recipe_runs_slug_time ON ops_recipe_runs(recipe_slug, started_at DESC)",
        [],
    );
    // v2.3.19 Phase 5.4 — RequestApproval support. recipe_runs with
    // a RequestApproval action park in status='awaiting_approval'
    // and store the ApprovalRequest post id; the resume watcher
    // updates `decision` + `decision_post_id` when an
    // ApprovalDecision post lands. Best-effort ALTER TABLE since
    // ADD COLUMN fails if the column already exists.
    let _ = conn.execute(
        "ALTER TABLE ops_recipe_runs ADD COLUMN awaiting_approval_request_post_id TEXT",
        [],
    );
    let _ = conn.execute("ALTER TABLE ops_recipe_runs ADD COLUMN decision TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE ops_recipe_runs ADD COLUMN decision_post_id TEXT",
        [],
    );
    // Indexed because the resume watcher scans by status every 5s.
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ops_recipe_runs_status ON ops_recipe_runs(status)",
        [],
    );

    // v2.3.16 Phase 5.1 — Activity feed. A single chronological
    // stream where humans, agents, and the system post. NotifyHuman
    // recipe action writes here; users post via `ato posts add` or
    // the GUI; the system can auto-post when events fire.
    //
    // payload is optional structured JSON for approval kinds and
    // expanded agent responses. related_event_seq lets the GUI link
    // an event_notice post back to its events_log row.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS activity_posts (
            id                TEXT PRIMARY KEY,
            created_at        TEXT NOT NULL,
            author_kind       TEXT NOT NULL,
            author_slug       TEXT,
            kind              TEXT NOT NULL,
            text              TEXT NOT NULL,
            related_event_seq INTEGER,
            payload           TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_activity_posts_created_at ON activity_posts(created_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_activity_posts_kind_created ON activity_posts(kind, created_at DESC)",
        [],
    );
    // v2.3.31 Phase 6 Slice A — sticky multi-turn sessions per runtime.
    // ATO assigns its own session id; the dispatch path passes it
    // through to the runtime CLI via --resume (claude) / similar.
    // runtime_session_id is the runtime's NATIVE token (captured from
    // claude's --output-format json metadata on the first dispatch);
    // the ATO id is a stable handle users + agents can refer to.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS sessions (
            id                  TEXT PRIMARY KEY,
            runtime             TEXT NOT NULL,
            agent_slug          TEXT,
            runtime_session_id  TEXT,
            title               TEXT,
            created_at          TEXT NOT NULL,
            last_used_at        TEXT NOT NULL,
            turn_count          INTEGER NOT NULL DEFAULT 0
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_runtime_lastused
            ON sessions(runtime, last_used_at DESC)",
        [],
    );
    // v2.6 Phase 6 Slice C — explicit session lifecycle (open → closed →
    // reopened). On close, the session's coordinator (the agent at
    // sessions.agent_slug, falling back to the anchor runtime) generates
    // a title, summary, topic tags, and an inferred project_id. Reopen
    // flips status back; the next close overwrites the summary with the
    // refreshed transcript. ALTER TABLE on each column individually so
    // older DBs upgrade in place; "duplicate column" errors are expected
    // on a fresh install where the columns already exist and are ignored.
    // Status is constrained to {'open', 'closed'} at the DB level so a
    // future write of a stray string (typo, branch like 'archived')
    // fails loudly rather than corrupting the invariant the UI relies
    // on. SQLite supports column-level CHECK on ADD COLUMN since 3.37.
    // Already-installed dev builds that added this column without the
    // CHECK silently fail the ALTER (duplicate column) and rely on the
    // application-layer enforcement in sessions.rs close/reopen.
    let _ = conn.execute(
        "ALTER TABLE sessions ADD COLUMN status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed'))",
        [],
    );
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN closed_at TEXT", []);
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN summary TEXT", []);
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN auto_title TEXT", []);
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN tags_json TEXT", []);
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN project_id TEXT", []);
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_project
            ON sessions(project_id, last_used_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_status_lastused
            ON sessions(status, last_used_at DESC)",
        [],
    );
    // v2.3.32 Phase 6 Slice A.2 — unified turn history. Stateful
    // runtimes (claude --resume) and stateless API providers
    // (minimax etc.) both dual-write into this table on every
    // dispatch in a session, so:
    //   - History-replay providers can rebuild the messages array
    //   - Slice B (cross-runtime mid-session switching) sees a
    //     unified log to feed into whichever runtime takes the
    //     next turn
    // turn_index is monotonic per session (max+1 on insert).
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS session_turns (
            session_id  TEXT NOT NULL,
            turn_index  INTEGER NOT NULL,
            role        TEXT NOT NULL,
            text        TEXT NOT NULL,
            runtime     TEXT NOT NULL,
            created_at  TEXT NOT NULL,
            PRIMARY KEY (session_id, turn_index)
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_session_turns_session
            ON session_turns(session_id, turn_index ASC)",
        [],
    );

    // v2.3.27 Phase 6.x — Runtime quota visibility. Stores parsed
    // "rate limit until X" timestamps surfaced from dispatch errors.
    // One row per runtime; UPSERT on new captures. The dispatch
    // pre-flight reads this to short-circuit "try again at <ts>"
    // without burning another quota probe.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS runtime_quotas (
            runtime     TEXT PRIMARY KEY,
            resets_at   TEXT NOT NULL,
            source      TEXT NOT NULL,
            captured_at TEXT NOT NULL
        )",
        [],
    );

    // v2.3.32 Phase 6.x-J — SSH-backed remote runtimes. Each row is a
    // user-registered remote that `ato dispatch <slug> "..."` should
    // route to via `ssh -i <key> -p <port> user@host '<binary> <args>'`
    // instead of spawning a local CLI. Triggered by @iamknownasfesal's
    // X question about laptop ↔ server Claude bridging. One-way only:
    // the laptop initiates; the remote runs the binary; stdout comes
    // back into execution_logs like any other dispatch. The Phase 7+
    // bi-directional mesh (daemons discovering each other) is roadmap
    // but out of scope here.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS remote_runtimes (
            slug          TEXT PRIMARY KEY,
            host          TEXT NOT NULL,
            port          INTEGER NOT NULL DEFAULT 22,
            ssh_user      TEXT,
            key_path      TEXT,
            runtime       TEXT NOT NULL,
            binary_path   TEXT NOT NULL,
            extra_args    TEXT,
            created_at    TEXT NOT NULL
        )",
        [],
    );

    // v2.4.0 Phase 7.0 — Bi-directional mesh: peer registry +
    // pending invites. Each peer has an Ed25519 public key; messages
    // (post_completion) are signed by the sender and verified before
    // the recipient writes them into session_turns / events_log.
    //
    // mesh_invites are short-lived (5 min) single-use codes used for
    // the initial pairing handshake when mDNS doesn't discover the
    // peer (typical for VLAN-isolated setups). After consumption,
    // the row stays around with `consumed=1` for auditability.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS mesh_peers (
            peer_id      TEXT PRIMARY KEY,
            public_key   TEXT NOT NULL,
            name         TEXT NOT NULL,
            paired_at    TEXT NOT NULL,
            last_seen_at TEXT,
            notes        TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS mesh_invites (
            code         TEXT PRIMARY KEY,
            issued_at    TEXT NOT NULL,
            expires_at   TEXT NOT NULL,
            consumed     INTEGER NOT NULL DEFAULT 0
        )",
        [],
    );
    // session_turns.sender_peer_id distinguishes a turn that landed
    // via the mesh (sender_peer_id matches a mesh_peers row) from a
    // locally-dispatched turn (NULL). The History panel + transcripts
    // render a peer badge when set.
    let _ = conn.execute(
        "ALTER TABLE session_turns ADD COLUMN sender_peer_id TEXT",
        [],
    );

    // v2.4.1 Phase 7.0 step 2 — mDNS-discovered peers (transient).
    // Separate from mesh_peers (which holds *trusted* peers post-
    // pairing). Discoveries are upserted by peer_id as the daemon's
    // mDNS browser sees them; rows older than ~5 min get pruned so
    // a stale discovery doesn't survive a peer going offline.
    //
    // Discovery DOES NOT imply trust — `ato mesh discovered` shows
    // "what's on the network"; promoting a row to mesh_peers
    // happens via the pairing handshake in step 4.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS mesh_discovered (
            peer_id      TEXT PRIMARY KEY,
            name         TEXT NOT NULL,
            version      TEXT,
            addr         TEXT NOT NULL,
            last_seen_at TEXT NOT NULL
        )",
        [],
    );

    // v2.3.39 Phase 6.x-K — Eval-score ratchet.
    //
    // Inspired by Garry Tan's "AI Agent Complexity Ratchet" (2026-05).
    // Locks a quality floor per target (agent / runtime / global).
    // `ato ratchet check` compares the floor against the current
    // success-rate window and exits non-zero if breached — designed
    // to drop into CI / pre-deploy hooks so a config change that
    // regresses an agent's quality fails the build.
    //
    // Metric for v1 is `success_rate` (0.0–1.0), computed from
    // execution_logs.status. Cloud eval_score can layer on later
    // as a second metric without schema migration: add a `metric`
    // discriminator column and the same table holds both.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS eval_ratchets (
            target_kind          TEXT NOT NULL,
            target_value         TEXT NOT NULL,
            metric               TEXT NOT NULL DEFAULT 'success_rate',
            baseline_value       REAL NOT NULL,
            baseline_window_days INTEGER NOT NULL,
            threshold            REAL NOT NULL DEFAULT 0.05,
            locked_at            TEXT NOT NULL,
            locked_by            TEXT,
            notes                TEXT,
            PRIMARY KEY (target_kind, target_value, metric)
        )",
        [],
    );

    // v2.3.18 Phase 5.3 — partial UNIQUE index enforcing
    // one-ApprovalDecision-per-ApprovalRequest at the storage layer.
    // Codex 5.3 round-1 caught that the CLI's check-then-insert was
    // a race window; concurrent approve/deny would both succeed. The
    // SQL UNIQUE constraint serializes writers without needing a
    // transaction-level lock in app code.
    //
    // Codex round-2 caught that `let _ = conn.execute(...)` would
    // silently swallow the creation failure on DBs that already
    // have duplicates from the pre-fix race. We surface it here
    // so the user (or a future migration) can clean up before
    // relying on the protection.
    if let Err(e) = conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_activity_posts_decision_request
            ON activity_posts(json_extract(payload, '$.request_post_id'))
          WHERE kind = 'approval_decision'",
        [],
    ) {
        eprintln!(
            "WARN: failed to create unique approval-decision index: {} \
             (likely a pre-existing duplicate from a v2.3.17-or-earlier race). \
             Run `sqlite3 ~/.ato/local.db` and inspect duplicate \
             json_extract(payload,'$.request_post_id') values, then retry.",
            e
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
