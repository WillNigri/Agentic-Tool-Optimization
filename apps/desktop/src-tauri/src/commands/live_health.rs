// commands/live_health.rs — Live Claude Code session tracking + runtime
// health monitoring.
//
// PR 18 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (8 commands):
//   Live session (2):
//     - get_live_session_data        — parses the most-recent Claude Code
//                                      session .jsonl for tokens, tools,
//                                      files read
//     - get_live_context_breakdown   — builds a ContextBreakdown using live
//                                      session data + system / CLAUDE.md /
//                                      MCP / skills estimates
//   Health (6):
//     - get_health_status            — latest health-check row + 24h uptime
//                                      per runtime (skips un-monitored)
//     - record_health_check          — INSERT a check row + prune >7d
//     - start_health_poller          — kick off the background poller
//     - stop_health_poller           — terminate it
//     - is_health_poller_running     — state probe
//     - get_health_history           — chartable points for the last N hours
//
// Plus the two non-tauri helpers (`find_current_session`,
// `parse_session_jsonl`) and the LiveSessionData / SessionFileRead data
// shapes. Health structs (RuntimeHealth, HealthCheck,
// RuntimeHealthHistory, HealthHistoryPoint) and the HealthPollerState
// type live in crate root (lib.rs) and are pulled in via `crate::*`.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::State;

use crate::{
    get_db_path, ContextBreakdown, ContextCategory, DbState, HealthCheck,
    HealthHistoryPoint, HealthPollerState, RuntimeHealth, RuntimeHealthHistory,
};

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
    let claude_dir = super::claude_home();
    let projects_dir = claude_dir.join("projects");

    if !projects_dir.exists() {
        return None;
    }

    // Get current project path
    let current_project = super::project_root();
    let project_hash = current_project
        .to_string_lossy()
        .replace("/", "-")
        .replace("\\", "-");

    // Look for project directory matching current project
    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                // Check if this directory matches our project
                if dir_name.contains(&project_hash) || dir_name.starts_with("-Users-") {
                    // Find the most recent .jsonl file in this directory
                    if let Ok(sub_entries) = fs::read_dir(&path) {
                        let mut jsonl_files: Vec<PathBuf> = sub_entries
                            .flatten()
                            .filter(|e| {
                                e.path()
                                    .extension()
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
                            let session_id = latest
                                .file_stem()
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
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read session file: {}", e))?;

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
                    if let Some(cache_read) =
                        usage.get("cache_read_input_tokens").and_then(|v| v.as_u64())
                    {
                        data.cache_read_tokens += cache_read;
                    }
                    if let Some(cache_create) =
                        usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64())
                    {
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
                                            if let Some(file_path) =
                                                input.get("file_path").and_then(|v| v.as_str())
                                            {
                                                if !seen_files.contains(file_path) {
                                                    seen_files.insert(file_path.to_string());
                                                    let token_estimate =
                                                        fs::metadata(file_path)
                                                            .map(|m| super::estimate_tokens(m.len()))
                                                            .unwrap_or(0);
                                                    data.files_read.push(SessionFileRead {
                                                        path: file_path.to_string(),
                                                        timestamp: entry
                                                            .get("timestamp")
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
    categories.push(ContextCategory {
        name: "System Prompts".into(),
        tokens: system_tokens,
        color: "#FF4466".into(),
    });
    total += system_tokens;

    // CLAUDE.md
    let claude_md = super::project_root().join("CLAUDE.md");
    let claude_md_tokens = fs::metadata(&claude_md)
        .map(|m| super::estimate_tokens(m.len()))
        .unwrap_or(0);
    categories.push(ContextCategory {
        name: "CLAUDE.md".into(),
        tokens: claude_md_tokens,
        color: "#FFB800".into(),
    });
    total += claude_md_tokens;

    // MCP schemas
    let settings_path = super::claude_home().join("settings.json");
    let mcp_tokens: u64 = super::read_file_lossy(&settings_path)
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|v| {
            v.get("mcpServers")
                .and_then(|s| s.as_object())
                .map(|m| m.len() as u64 * 2500)
        })
        .unwrap_or(0);
    categories.push(ContextCategory {
        name: "MCP Schemas".into(),
        tokens: mcp_tokens,
        color: "#3b82f6".into(),
    });
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
        categories.push(ContextCategory {
            name: "Conversation".into(),
            tokens: 15000,
            color: "#a78bfa".into(),
        });
        total += 15000;
    }

    // Skills (on-demand)
    let skill_bytes = super::dir_skill_bytes(&super::claude_home().join("skills"))
        + super::dir_skill_bytes(&super::project_root().join(".claude").join("skills"));
    let skill_tokens = super::estimate_tokens(skill_bytes);
    categories.push(ContextCategory {
        name: "Skills (on-demand)".into(),
        tokens: skill_tokens,
        color: "#00FFB233".into(),
    });

    Ok(ContextBreakdown {
        total_tokens: total,
        limit: 200000,
        categories,
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
        // v2.5.1 — un-monitored runtimes are completely hidden from
        // the Health panel (no card at all). The poller already skips
        // probing them; we also filter at the read site so any stale
        // pre-toggle-off rows don't surface. Inlining the query
        // (rather than calling is_runtime_monitored) so we reuse the
        // already-held `db.0` lock and avoid a second Connection::open
        // that could contend in WAL mode.
        let monitored: bool = conn
            .query_row(
                "SELECT monitored FROM runtime_preferences WHERE runtime = ?1",
                [runtime],
                |r| r.get::<_, i64>(0),
            )
            .map(|v| v != 0)
            // Default to true if the row is missing — first launch may
            // have not seeded yet when this is called.
            .unwrap_or(true);
        if !monitored {
            continue;
        }
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
            status: latest
                .as_ref()
                .map(|h| h.status.clone())
                .unwrap_or_else(|| "unknown".to_string()),
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
    )
    .ok();

    Ok(HealthCheck {
        id,
        runtime,
        status,
        latency_ms,
        error_message,
        checked_at: now,
    })
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
pub fn is_health_poller_running(
    poller_state: State<'_, HealthPollerState>,
) -> Result<bool, String> {
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
        vec![
            "claude".to_string(),
            "codex".to_string(),
            "hermes".to_string(),
            "openclaw".to_string(),
        ]
    };

    let mut results = Vec::new();

    for rt in runtimes {
        // Get data points
        let mut stmt = conn
            .prepare(
                "SELECT checked_at, latency_ms, status FROM health_checks
             WHERE runtime = ?1 AND checked_at > datetime('now', ?2)
             ORDER BY checked_at ASC",
            )
            .map_err(|e| e.to_string())?;

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
