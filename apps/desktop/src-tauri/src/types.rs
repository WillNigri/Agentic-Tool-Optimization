// v2.7.14 — extracted from lib.rs (ROADMAP v2.8.0 item: split lib.rs).
// All frontend-facing type definitions live here. lib.rs keeps mod
// declarations, DB helpers, and the run() entry point.
//
// Re-exported from lib.rs via `pub use types::*;` so existing call sites
// (commands/mod.rs, sessions_view, etc.) compile unchanged.

use serde::{Deserialize, Serialize};

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
