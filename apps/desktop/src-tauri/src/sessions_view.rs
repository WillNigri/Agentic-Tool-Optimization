// v2.3.42 — Sessions view for the GUI.
//
// Phase 6 sessions (Slice A + A.2 + B) are CLI-first today: they live
// in the `sessions` + `session_turns` tables and the CLI exposes
// `ato sessions ...` and `ato bridge`. The desktop GUI never had a
// first-class surface for them — they only appeared incidentally
// under Execution Logs after v2.3.41's grouping.
//
// This module adds two Tauri commands:
//   - list_sessions_full   — overview rows for the Sessions tab list
//   - get_session_transcript — every turn for one session, ordered
//
// Both are read-only. Continuing a session from the GUI uses the
// existing prompt_agent flow with an extra session_id parameter
// (wired separately so this module stays narrowly scoped to the
// view layer).

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter, State};

use crate::DbState;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionListRow {
    pub id: String,
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub title: Option<String>,
    pub created_at: String,
    pub last_used_at: String,
    pub turn_count: i64,
    /// Distinct runtimes that appear in this session's turns. For a
    /// claude-only session this is `["claude"]`. For a Slice B
    /// cross-runtime conversation it's e.g. `["claude", "minimax"]`.
    /// Drives the runtime badges in the list UI.
    pub runtimes_used: Vec<String>,
    /// Last (assistant) turn's text, truncated. Gives the user a
    /// "what was this conversation about" preview without expanding.
    pub last_assistant_preview: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionTurn {
    pub turn_index: i64,
    pub role: String,
    pub text: String,
    pub runtime: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionTranscript {
    pub id: String,
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub title: Option<String>,
    pub turns: Vec<SessionTurn>,
}

#[tauri::command]
pub fn list_sessions_full(
    db: State<'_, DbState>,
    limit: Option<i64>,
) -> Result<Vec<SessionListRow>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_sessions_inner(&conn, limit.unwrap_or(50)).map_err(|e| e.to_string())
}

fn list_sessions_inner(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<SessionListRow>> {
    // Some installs have older `sessions` rows from before the v2.3.31
    // migration completed. Tolerate missing columns by SELECTing only
    // what we know is always there + joining session_turns for the
    // computed fields. Fallback paths if the tables don't exist at all
    // are folded into the `?` chain — caller surfaces the string error.
    let mut stmt = conn.prepare(
        "SELECT s.id, s.runtime, s.agent_slug, s.title, s.created_at, s.last_used_at, s.turn_count
           FROM sessions s
          ORDER BY s.last_used_at DESC
          LIMIT ?1",
    )?;
    let rows: Vec<SessionListRow> = stmt
        .query_map([limit], |r| {
            Ok(SessionListRow {
                id: r.get(0)?,
                runtime: r.get(1)?,
                agent_slug: r.get(2)?,
                title: r.get(3)?,
                created_at: r.get(4)?,
                last_used_at: r.get(5)?,
                turn_count: r.get(6)?,
                runtimes_used: Vec::new(),
                last_assistant_preview: None,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Enrich each row with computed fields. Two cheap follow-up queries
    // per session — fine for the default limit of 50, and the indexes
    // on session_turns(session_id, turn_index ASC) make them O(log N).
    let mut enriched = Vec::with_capacity(rows.len());
    for mut row in rows {
        // Distinct runtimes in this session's turns. For Slice B
        // cross-runtime sessions this is what makes the multi-badge UI
        // render correctly.
        let mut rt_stmt = conn.prepare_cached(
            "SELECT DISTINCT runtime FROM session_turns WHERE session_id = ?1 ORDER BY turn_index ASC",
        )?;
        let runtimes: Vec<String> = rt_stmt
            .query_map([&row.id], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        // Fall back to the session's anchor runtime when session_turns
        // is empty (e.g. a freshly opened session before its first
        // dispatch lands a turn).
        row.runtimes_used = if runtimes.is_empty() {
            vec![row.runtime.clone()]
        } else {
            runtimes
        };

        // Last assistant turn → preview. Order by turn_index DESC so we
        // get the chronologically last assistant message, not whichever
        // arrived first.
        let mut last_stmt = conn.prepare_cached(
            "SELECT text FROM session_turns
              WHERE session_id = ?1 AND role = 'assistant'
              ORDER BY turn_index DESC
              LIMIT 1",
        )?;
        let preview: Option<String> = last_stmt
            .query_row([&row.id], |r| r.get::<_, String>(0))
            .ok();
        row.last_assistant_preview = preview.map(|s| {
            // Trim to 160 chars max so list rows stay one-line on most
            // viewports. The full text is available in the transcript.
            if s.chars().count() > 160 {
                let truncated: String = s.chars().take(160).collect();
                format!("{}…", truncated)
            } else {
                s
            }
        });

        enriched.push(row);
    }
    Ok(enriched)
}

#[tauri::command]
pub fn get_session_transcript(
    db: State<'_, DbState>,
    session_id: String,
) -> Result<SessionTranscript, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let (runtime, agent_slug, title): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT runtime, agent_slug, title FROM sessions WHERE id = ?1",
            [&session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| format!("session not found: {}", e))?;

    let mut stmt = conn
        .prepare(
            "SELECT turn_index, role, text, runtime, created_at
               FROM session_turns
              WHERE session_id = ?1
              ORDER BY turn_index ASC",
        )
        .map_err(|e| e.to_string())?;

    let turns: Vec<SessionTurn> = stmt
        .query_map([&session_id], |r| {
            Ok(SessionTurn {
                turn_index: r.get(0)?,
                role: r.get(1)?,
                text: r.get(2)?,
                runtime: r.get(3)?,
                created_at: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(SessionTranscript {
        id: session_id,
        runtime,
        agent_slug,
        title,
        turns,
    })
}

// ───────────────────────────────────────────────────────────────────────
// v2.3.43 — Tauri commands for the New / Continue / Bridge buttons.
//
// Each shells out to the `ato` CLI binary, which is the canonical
// implementation of sessions / dispatch / bridge. The desktop's own
// prompt_agent path doesn't yet support --session natively (a deeper
// change); going through the CLI keeps these slices independent and
// the behavior provably identical to what an agent invoking
// `ato dispatch ... --session` would do.

#[derive(Debug, Deserialize)]
struct CliSessionNewOutput {
    id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchIntoSessionResult {
    pub run_id: String,
    pub status: String,
    pub response: Option<String>,
    pub error_message: Option<String>,
    pub duration_ms: Option<i64>,
}

fn resolve_ato_binary() -> Result<String, String> {
    // Prefer the bundled installation paths, then fall through to the
    // same PATH resolution other Tauri commands use. Falls back to bare
    // "ato" so the user's shell can locate it if installed elsewhere.
    if let Some(p) = crate::commands::which_cli("ato") {
        return Ok(p);
    }
    // Last resort: bare command name; Command::new will surface a clean
    // exec error if PATH doesn't include it.
    Ok("ato".to_string())
}

#[tauri::command]
pub fn create_session(
    runtime: String,
    title: Option<String>,
    agent_slug: Option<String>,
) -> Result<String, String> {
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["sessions", "new", "--runtime", &runtime]);
    if let Some(t) = &title {
        cmd.args(["--title", t]);
    }
    if let Some(slug) = &agent_slug {
        cmd.args(["--as", slug]);
    }
    let out = cmd.output().map_err(|e| format!("spawn ato: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!("ato sessions new failed: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: CliSessionNewOutput = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("parse ato output: {} (raw: {})", e, stdout))?;
    Ok(parsed.id)
}

#[tauri::command]
pub fn dispatch_into_session(
    runtime: String,
    prompt: String,
    session_id: String,
    model: Option<String>,
) -> Result<DispatchIntoSessionResult, String> {
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["dispatch", &runtime, &prompt, "--session", &session_id]);
    if let Some(m) = &model {
        cmd.args(["--model", m]);
    }
    let out = cmd.output().map_err(|e| format!("spawn ato: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    // The CLI exits non-zero only on a pre-flight error (quota / unknown
    // runtime). Real per-dispatch errors come back as a JSON payload
    // with status="error", so we still need to parse stdout when present.
    let raw = if stdout.trim().is_empty() {
        // No stdout — fall back to surfacing stderr to the user.
        return Err(format!(
            "ato dispatch produced no JSON output: {}",
            stderr.trim()
        ));
    } else {
        stdout
    };
    let v: serde_json::Value =
        serde_json::from_str(raw.trim()).map_err(|e| format!("parse ato output: {}", e))?;
    Ok(DispatchIntoSessionResult {
        run_id: v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        status: v
            .get("status")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
        response: v.get("response").and_then(|x| x.as_str()).map(String::from),
        error_message: v
            .get("error_message")
            .and_then(|x| x.as_str())
            .map(String::from),
        duration_ms: v.get("duration_ms").and_then(|x| x.as_i64()),
    })
}

#[tauri::command]
pub fn bridge_session(
    session_id: String,
    max_rounds: Option<u32>,
) -> Result<String, String> {
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["bridge", "--session", &session_id, "--human"]);
    if let Some(n) = max_rounds {
        cmd.args(["--max-rounds", &n.to_string()]);
    }
    let out = cmd.output().map_err(|e| format!("spawn ato: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        return Err(format!(
            "ato bridge failed (status {}): {}",
            out.status,
            stderr.trim()
        ));
    }
    // The bridge writes its progress as human-readable lines to stdout;
    // return the whole transcript so the UI can show it in a "bridge
    // result" panel.
    Ok(stdout.trim().to_string())
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ChunkEventPayload {
    session_id: String,
    text: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DoneEventPayload {
    session_id: String,
    result: serde_json::Value,
}

/// v2.3.48 — streaming dispatch into a session. Spawns the CLI with
/// `--stream-jsonl`, reads each line of stdout as a JSON event, and
/// emits Tauri events for the frontend to render:
///   - `session-stream-chunk` { sessionId, text } per chunk
///   - `session-stream-done`  { sessionId, result } at the end
/// Returns the final DispatchResult once the stream completes so the
/// caller can await it like a regular Tauri command. Errors propagate
/// as Tauri-command errors with stderr context.
#[tauri::command]
pub fn dispatch_into_session_streaming(
    app: AppHandle,
    runtime: String,
    prompt: String,
    session_id: String,
    model: Option<String>,
) -> Result<serde_json::Value, String> {
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args([
        "dispatch",
        &runtime,
        &prompt,
        "--session",
        &session_id,
        "--stream-jsonl",
    ]);
    if let Some(m) = &model {
        cmd.args(["--model", m]);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn ato dispatch --stream-jsonl: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "missing stdout pipe".to_string())?;
    let reader = BufReader::new(stdout);

    let mut final_result: Option<serde_json::Value> = None;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                return Err(format!("read CLI stdout: {}", e));
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // skip non-JSON lines defensively
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("chunk") => {
                let text = v
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let _ = app.emit(
                    "session-stream-chunk",
                    ChunkEventPayload {
                        session_id: session_id.clone(),
                        text,
                    },
                );
            }
            Some("done") => {
                let result = v.get("result").cloned().unwrap_or(serde_json::Value::Null);
                let _ = app.emit(
                    "session-stream-done",
                    DoneEventPayload {
                        session_id: session_id.clone(),
                        result: result.clone(),
                    },
                );
                final_result = Some(result);
            }
            _ => {}
        }
    }

    // Reap the child to surface any non-zero exit + stderr.
    let exit_status = child
        .wait()
        .map_err(|e| format!("wait CLI exit: {}", e))?;
    if !exit_status.success() {
        let mut stderr_buf = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            use std::io::Read;
            let _ = stderr.read_to_string(&mut stderr_buf);
        }
        return Err(format!(
            "ato dispatch exited with {}: {}",
            exit_status,
            stderr_buf.trim()
        ));
    }

    final_result.ok_or_else(|| "stream finished without a `done` event".to_string())
}
