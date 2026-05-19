// commands/execution_logs.rs — Core execution_logs CRUD: list/read +
// insert.
//
// PR 22 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (2 commands + 1 helper):
//   - get_execution_logs   — list with runtime + status filters; default
//                            limit 100; selects v2.3.41 session_id +
//                            tool_calls + agent_slug + model
//   - add_execution_log    — write a single execution row (legacy path;
//                            most dispatches now write inline)
//   - map_execution_log    — shared row mapper helper (pub for downstream
//                            modules; map_execution_log_row may want to
//                            move too in a future pass)
//
// What's NOT here yet (deferred to a follow-up PR):
//   - AgentTraceLine / AgentTraceFilter structs + load_agent_log_lines
//     helper + read_agent_traces / get_agent_metrics commands. They form
//     a coherent block at ~10500 in mod.rs and travel together; can also
//     end up living next to agent code in PR 28 (agents.rs).
//   - link_execution_log_to_cloud_trace,
//     get_execution_log_response_by_cloud_trace_id,
//     get_execution_log_io_by_cloud_trace_id,
//     list_replays_for_trace — cloud-trace lookups scattered across mod.rs
//     (lines 2574 / 2888 / 2916 / 2935). They share a sub-domain
//     (cloud-trace ID lookups) and travel together later.
//
// ExecutionLog struct lives in crate root (lib.rs) — same as the other
// log-related types.

use rusqlite::params;
use tauri::State;

use crate::{DbState, ExecutionLog};

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

    // v2.3.41 — include session_id so the History panel can group
    // multi-turn conversations under one collapsible header.
    let sql = match (&runtime, &status) {
        (Some(_), Some(_)) => "SELECT id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, created_at, session_id, tool_calls_count, tool_calls_summary, agent_slug, model FROM execution_logs WHERE runtime = ?1 AND status = ?2 ORDER BY created_at DESC LIMIT ?3",
        (Some(_), None) => "SELECT id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, created_at, session_id, tool_calls_count, tool_calls_summary, agent_slug, model FROM execution_logs WHERE runtime = ?1 ORDER BY created_at DESC LIMIT ?2",
        (None, Some(_)) => "SELECT id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, created_at, session_id, tool_calls_count, tool_calls_summary, agent_slug, model FROM execution_logs WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2",
        (None, None) => "SELECT id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, created_at, session_id, tool_calls_count, tool_calls_summary, agent_slug, model FROM execution_logs ORDER BY created_at DESC LIMIT ?1",
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let logs = match (&runtime, &status) {
        (Some(rt), Some(st)) => stmt.query_map(params![rt, st, limit], map_execution_log),
        (Some(rt), None) => stmt.query_map(params![rt, limit], map_execution_log),
        (None, Some(st)) => stmt.query_map(params![st, limit], map_execution_log),
        (None, None) => stmt.query_map(params![limit], map_execution_log),
    }
    .map_err(|e| e.to_string())?;

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
        session_id: row.get(11).ok(),
        tool_calls_count: row.get(12).ok(),
        tool_calls_summary: row.get(13).ok(),
        agent_slug: row.get(14).ok(),
        model: row.get(15).ok(),
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
        session_id: None,
        tool_calls_count: None,
        tool_calls_summary: None,
        agent_slug: None,
        model: None,
    })
}
