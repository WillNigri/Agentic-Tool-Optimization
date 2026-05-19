// commands/agent_hooks_evals.rs — Agent hooks (pre-call context fetchers)
// and agent evaluators (heuristic pass/fail scorers).
//
// PR 17 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (7 commands):
//   - list_agent_hooks            — CRUD for the agent_hooks table
//   - save_agent_hook             — upsert + position bump
//   - delete_agent_hook
//   - list_agent_evaluators       — CRUD for the agent_evaluators table
//   - save_agent_evaluator        — upsert with kind validation
//   - delete_agent_evaluator
//   - evaluate_recent_traces      — run all enabled evaluators against
//                                   the most-recent N traces for an agent
//
// Plus the data shapes (AgentHook, AgentEvaluator, EvaluationResult,
// EvaluatedTrace), the local evaluator runner (`run_evaluator` —
// heuristic kinds only; LLM-judge stub returns "unknown"), and the
// schema-init helper (`ensure_evaluator_table`).
//
// The hook *executor* helpers (should_fire_hook, run_pre_call_hooks,
// execute_hook, load_agent_hooks) stay in mod.rs — they're called by
// the dispatcher path and travel with PR 28 (agents.rs).
//
// `AgentTraceLine` (used by run_evaluator) lives in mod.rs today — it's
// shared with the agent-log domain (load_agent_log_lines, filters, etc.)
// and moves with PR 22 (execution_logs.rs).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::DbState;

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

pub fn ensure_evaluator_table(conn: &Connection) -> Result<(), String> {
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
pub fn run_evaluator(eval: &AgentEvaluator, trace: &super::AgentTraceLine) -> EvaluationResult {
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
    pub trace: super::AgentTraceLine,
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

    let traces = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        super::load_agent_log_lines(
            &conn,
            &super::AgentTraceFilter {
                agent_slug: Some(agent_slug),
                runtime: None,
                status: None,
                since: None,
                limit: Some(last_n),
            },
        )
    };

    let evaluated: Vec<EvaluatedTrace> = traces
        .into_iter()
        .map(|t| EvaluatedTrace {
            results: evaluators.iter().map(|e| run_evaluator(e, &t)).collect(),
            trace: t,
        })
        .collect();

    Ok(evaluated)
}
