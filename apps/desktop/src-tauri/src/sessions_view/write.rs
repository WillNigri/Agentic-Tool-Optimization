// sessions_view/write.rs — write + CLI-dispatch paths for the Sessions surface.
//
// Anything that spawns the `ato` CLI subprocess to do real work
// (create_session, dispatch_into_session, dispatch_war_room,
// close_session, reopen_session, bridge_session) lives here. Plus
// the streaming dispatch variant + its event payloads, the input
// validators, and the binary-resolver helper they all share.
//
// 2026-05-19 elegance war-room split (was 1635-line sessions_view.rs;
// codex flagged it before lazy row creation lands).
//
// Owned: dispatch_war_room, resolve_ato_binary, create_session,
// dispatch_into_session, bridge_session, dispatch_into_session_streaming,
// validate_session_id, validate_agent_slug, close_session,
// cancel_close_session, reopen_session.
// Local structs: WarRoomDispatchResult, CliSessionNewOutput,
// DispatchIntoSessionResult, ChunkEventPayload, DoneEventPayload.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter, State};

use super::CloseInflight;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WarRoomDispatchResult {
    pub war_room_id: String,
    pub round: i64,
}

/// First-Chat Wizard + multi-round war-rooms (PR 16-PR-B,
/// 2026-05-18). Fans out parallel dispatches across N runtimes
/// sharing a war_room_id + round. Two modes:
///
/// 1. `war_room_id = None` → mints a new UUID, dispatches at
///    round 1. This is the wizard's "start a war-room" entry.
/// 2. `war_room_id = Some(uuid), round = Some(N)` → continues an
///    existing war-room at round N. The CLI's
///    build_war_room_history_prefix synthesizes the prior-rounds
///    transcript on each seat's behalf before the LLM call.
///
/// Best-effort error handling: if a single seat fails (rate
/// limit, decrypt error), the war-room still surfaces the other
/// replies. Failures land in execution_logs with status="error"
/// and surface in the war-room detail view + the next round's
/// synthesis (per Will: humans need to understand what happened).
#[tauri::command]
pub fn dispatch_war_room(
    runtimes: Vec<String>,
    prompt: String,
    war_room_id: Option<String>,
    round: Option<i64>,
) -> Result<WarRoomDispatchResult, String> {
    if runtimes.is_empty() {
        return Err("dispatch_war_room: at least one runtime required".to_string());
    }
    if prompt.trim().is_empty() {
        return Err("dispatch_war_room: prompt cannot be empty".to_string());
    }
    let bin = resolve_ato_binary()?;
    let war_room_id = war_room_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let round = round.unwrap_or(1);
    let round_str = round.to_string();
    // Spawn all N dispatches in parallel. wait() on each child
    // collects exit status without serializing them. Stdout is
    // captured (not piped to terminal) since the CLI's --quiet
    // flag emits compact JSON we don't need to parse here — the
    // execution_logs row is the source of truth.
    let mut children: Vec<(String, std::process::Child)> = Vec::with_capacity(runtimes.len());
    for runtime in &runtimes {
        let mut cmd = Command::new(&bin);
        cmd.args([
            "dispatch",
            runtime,
            &prompt,
            "--war-room-id",
            &war_room_id,
            "--war-room-round",
            &round_str,
            "--quiet",
        ]);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        match cmd.spawn() {
            Ok(child) => children.push((runtime.clone(), child)),
            Err(e) => {
                // Don't fail the whole war-room if one runtime
                // can't even spawn — the others will still land
                // their replies. Frontend will see the missing
                // seat by virtue of it not appearing in
                // get_war_room_constituents.
                eprintln!(
                    "dispatch_war_room: spawn failed for runtime {}: {}",
                    runtime, e
                );
            }
        }
    }
    // Wait for all children. Any error here gets logged but
    // doesn't fail the command — partial war-rooms are still
    // valuable. The CLI itself records error rows when an
    // individual dispatch fails (timeout, quota, etc.).
    for (runtime, mut child) in children {
        match child.wait() {
            Ok(status) => {
                if !status.success() {
                    eprintln!(
                        "dispatch_war_room: runtime {} exited non-zero ({:?})",
                        runtime, status.code()
                    );
                }
            }
            Err(e) => {
                eprintln!(
                    "dispatch_war_room: wait failed for runtime {}: {}",
                    runtime, e
                );
            }
        }
    }
    Ok(WarRoomDispatchResult {
        war_room_id,
        round,
    })
}

/// PR 14c (2026-05-18) — war-room drill-in. Returns the
/// constituent execution_logs rows that share a war_room_id, each
/// as a SingleRunDetail. Frontend renders them as a list of
/// per-seat cards so the user can see what each seat actually
/// said. Ordered by created_at ASC so the read order mirrors the
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
    project_id: Option<String>,
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
    // PR 11 — pass the active project from the sidebar through to the
    // CLI. CLI's create_inner validates the id against the projects
    // table and silently drops unknown ids to None (UI cache may be
    // stale). Empty strings are also treated as None.
    if let Some(pid) = project_id.as_deref() {
        if !pid.is_empty() {
            cmd.args(["--project", pid]);
        }
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

// ───────────────────────────────────────────────────────────────────────
// v2.6 Slice C — close / reopen lifecycle.
//
// Both commands shell out to the CLI so the canonical logic stays in
// one place (and `ato sessions close <id>` works identically from a
// terminal). The desktop frontend awaits the close call — it's
// expected to block for a few seconds while the coordinator LLM
// produces a title + summary + tags. The frontend renders a
// "Coordinator is summarizing…" modal during the wait.

/// Reject values that would be interpreted by clap as a flag rather
/// than a positional/flag value. Without this, an attacker-controlled
/// session_id like `--model evil` would be parsed as two flags and
/// could redirect the summarizer to an arbitrary model. We pair this
/// with `--` after the subcommand name as a defense in depth so even
/// values that contain odd characters can't break the arg parser.
fn validate_session_id(v: &str) -> Result<(), String> {
    if v.is_empty() {
        return Err("session_id is empty".to_string());
    }
    // Session IDs are UUIDs in this codebase (create_inner uses
    // Uuid::new_v4). Accept anything matching 8-4-4-4-12 hex; reject
    // anything else with a clear error rather than letting it through.
    let bytes = v.as_bytes();
    if bytes.len() != 36 {
        return Err(format!("session_id is not a UUID: {}", v));
    }
    for (i, b) in bytes.iter().enumerate() {
        let expect_dash = matches!(i, 8 | 13 | 18 | 23);
        if expect_dash {
            if *b != b'-' {
                return Err(format!("session_id is not a UUID: {}", v));
            }
        } else if !b.is_ascii_hexdigit() {
            return Err(format!("session_id is not a UUID: {}", v));
        }
    }
    Ok(())
}

/// Agent slugs are user-provided but constrained to a kebab/snake set
/// across the rest of this codebase. Reject anything else before we
/// pass it as a flag value.
fn validate_agent_slug(v: &str) -> Result<(), String> {
    if v.is_empty() || v.len() > 64 {
        return Err(format!("agent slug length out of range: {}", v.len()));
    }
    for c in v.chars() {
        if !(c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(format!("agent slug contains invalid characters: {}", v));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn close_session(
    inflight: State<'_, CloseInflight>,
    session_id: String,
    agent_slug: Option<String>,
    model: Option<String>,
) -> Result<serde_json::Value, String> {
    validate_session_id(&session_id)?;
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["sessions", "close"]);
    // Flag-bearing options go BEFORE `--` so clap parses them as flags.
    // After `--`, clap treats the next tokens as positional values
    // regardless of leading dashes — defense-in-depth against a stray
    // `--foo` session_id getting parsed as a flag if validate_session_id
    // is ever weakened.
    if let Some(slug) = agent_slug.as_deref() {
        if !slug.is_empty() {
            validate_agent_slug(slug)?;
            cmd.args(["--as", slug]);
        }
    }
    if let Some(m) = model.as_deref() {
        if !m.is_empty() {
            cmd.args(["--model", m]);
        }
    }
    cmd.arg("--");
    cmd.arg(&session_id);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Spawn → register PID → wait. The PID registration enables the
    // Cancel button in the UI to send SIGTERM via cancel_close_session.
    // We MUST remove the PID from the map in both the success and
    // error paths so a subsequent close isn't blocked by a stale entry.
    let child = cmd.spawn().map_err(|e| format!("spawn ato: {}", e))?;
    let pid = child.id();
    {
        let mut map = inflight.0.lock().map_err(|e| e.to_string())?;
        map.insert(session_id.clone(), pid);
    }
    let result = child
        .wait_with_output()
        .map_err(|e| format!("wait ato: {}", e));
    // Always deregister, even on wait_with_output error.
    {
        let mut map = inflight.0.lock().map_err(|e| e.to_string())?;
        map.remove(&session_id);
    }
    let out = result?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        // SIGTERM produces a non-zero exit. Surface a distinguishable
        // error so the frontend can recognize the cancel case and not
        // confuse it with a real failure.
        if let Some(code) = out.status.code() {
            if code != 0 && stderr.trim().is_empty() && stdout.trim().is_empty() {
                return Err("__cancelled__".to_string());
            }
        }
        return Err(format!(
            "ato sessions close failed (status {}): {}",
            out.status,
            stderr.trim()
        ));
    }
    // Refuse to embed raw stdout in the error — it can contain
    // truncated LLM output from a failed close, which may include
    // transcript content (potentially pasted secrets).
    serde_json::from_str(stdout.trim())
        .map_err(|_| "ato sessions close returned unparseable JSON".to_string())
}

/// Send SIGTERM to an in-flight `ato sessions close` subprocess so
/// the user's Cancel click in the blocking modal aborts the LLM call.
/// No-op when no close is in flight for this session (e.g., the user
/// double-clicked Cancel and the first click already worked).
#[tauri::command]
pub fn cancel_close_session(
    inflight: State<'_, CloseInflight>,
    session_id: String,
) -> Result<bool, String> {
    validate_session_id(&session_id)?;
    let pid = {
        let map = inflight.0.lock().map_err(|e| e.to_string())?;
        map.get(&session_id).copied()
    };
    let Some(pid) = pid else {
        return Ok(false);
    };
    // Shell to `kill -TERM <pid>` instead of pulling in libc/nix as a
    // new dep. macOS + Linux both ship `kill` in /bin; the desktop
    // app targets these platforms (Windows support is roadmap-only).
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .map_err(|e| format!("spawn kill: {}", e))?;
    Ok(status.success())
}

#[tauri::command]
pub fn reopen_session(session_id: String) -> Result<serde_json::Value, String> {
    validate_session_id(&session_id)?;
    let bin = resolve_ato_binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["sessions", "reopen", "--", &session_id]);
    let out = cmd.output().map_err(|e| format!("spawn ato: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        return Err(format!(
            "ato sessions reopen failed (status {}): {}",
            out.status,
            stderr.trim()
        ));
    }
    serde_json::from_str(stdout.trim())
        .map_err(|_| "ato sessions reopen returned unparseable JSON".to_string())
}
