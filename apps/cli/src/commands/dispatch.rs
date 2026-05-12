// `ato dispatch <runtime> "<prompt>" [--model M]`
//
// Fires a single-shot dispatch against any supported runtime CLI.
// Captures stdout/stderr, persists to execution_logs with token + cost
// estimates, returns JSON describing the result.
//
// Why this lives in the CLI (rather than calling out to the desktop):
// agents shouldn't depend on the GUI being open. The CLI is self-
// sufficient — same dispatch logic, same execution_logs schema, just
// no live-runs registration or streaming UI. Run from any shell, with
// or without the desktop running.

use crate::db;
use crate::output::{emit_human, emit_json, Opts};
use crate::runtime;
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct DispatchResult {
    pub id: String,
    pub runtime: String,
    pub model: Option<String>,
    pub status: String,
    pub response: Option<String>,
    pub error_message: Option<String>,
    pub duration_ms: i64,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub cost_usd_estimated: Option<f64>,
    pub created_at: String,
}

pub fn run(
    runtime_name: &str,
    prompt: &str,
    model: Option<String>,
    agent_slug_for_event: Option<String>,
    session_id: Option<String>,
    stream: bool,
    stream_jsonl: bool,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    // v2.3.31 Phase 6 Slice A — sticky session resolution.
    // If --session was passed, look up the session, validate the
    // runtime matches, and (if we have a captured runtime_session_id)
    // tell claude to resume. claude --output-format json prints the
    // session_id in metadata; we capture + persist after the dispatch.
    // v2.3.33 Phase 6 Slice B — sessions can now host turns from
    // multiple runtimes (the whole point of the cross-runtime
    // bridge). The session.runtime field stays as the *anchor*
    // (the runtime that started the conversation, which keeps
    // native --resume working for claude). When the active dispatch
    // runtime differs, we fall back to history replay for that
    // turn, and append the new turn tagged with the active runtime.
    let session = if let Some(ref sid) = session_id {
        let conn = db::open_readonly(db_path)?;
        let s = crate::commands::sessions::lookup(&conn, sid)?;
        if s.runtime != runtime_name && opts.human {
            // Informational only — Slice B intentionally allows
            // cross-runtime continuation. The note helps the user
            // realize that --resume won't be used here (history
            // replay covers it instead).
            crate::output::emit_human(&format!(
                "Note: session {} is anchored to '{}'; this turn runs '{}' via history replay (Phase 6 Slice B).",
                sid, s.runtime, runtime_name
            ));
        }
        Some(s)
    } else {
        None
    };
    // v2.3.27 Phase 6.x — quota pre-flight. If we previously parsed
    // a "try again at <ts>" out of an error and the ts is still
    // future, short-circuit without burning another dispatch attempt.
    // Caller sees a stable, scriptable error early instead of a real
    // 4xx with the same info.
    if let Ok(Some(resets_at)) = crate::quota::lookup_future(db_path, runtime_name) {
        anyhow::bail!(
            "Runtime '{}' is rate-limited until {} (cached from previous error). Try again after that time.",
            runtime_name,
            resets_at
        );
    }

    // v2.3.46 Phase 6.x-K — ratchet pre-flight (soft warning).
    // If the runtime has a locked floor AND the rolling window is
    // already at-or-near the floor-tolerance, warn the user before
    // we fire. Doesn't block — the gate is `ato ratchet check`, not
    // this — but surfaces the risk in human mode where they can
    // still cancel. Quiet in JSON output so scripts don't see
    // unexpected stderr noise.
    if opts.human {
        if let Ok(ro_conn) = db::open_readonly(db_path) {
            if let Ok(rows) = crate::commands::ratchet::compute_success_rate(
                &ro_conn,
                "runtime",
                runtime_name,
                7,
            ) {
                let (current, samples) = rows;
                // Look up the locked floor for runtime:<name>.
                let floor: Option<(f64, f64)> = ro_conn
                    .query_row(
                        "SELECT baseline_value, threshold FROM eval_ratchets
                          WHERE target_kind = 'runtime' AND target_value = ?1
                            AND metric = 'success_rate'",
                        [runtime_name],
                        |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?)),
                    )
                    .ok();
                if let (Some(c), Some((baseline, threshold))) = (current, floor) {
                    let floor_tol = (baseline - threshold).max(0.0);
                    if c <= floor_tol + 0.01 && samples >= 3 {
                        // Within 1pp of the floor — one more failure
                        // could trip the CI gate.
                        crate::output::emit_human(&format!(
                            "⚠  Ratchet warning: runtime:{} current rate is {:.1}% (floor-tol {:.1}%, baseline {:.1}%). A failure on this dispatch may breach the lock.",
                            runtime_name,
                            c * 100.0,
                            floor_tol * 100.0,
                            baseline * 100.0,
                        ));
                    }
                }
            }
        }
    }

    // v2.3.32 Phase 6.x-J — SSH-backed remote runtime. The slug the
    // user typed (e.g. `claude-server`) may resolve to a row in
    // remote_runtimes, in which case we route over SSH instead of
    // spawning a local CLI. Checked before find_provider so a user
    // who happens to name their remote after a provider (uncommon)
    // gets the remote, since that's a more specific intent.
    if let Some(remote) = crate::remote_runtime::lookup_in_db(db_path, runtime_name)? {
        return run_remote(
            remote,
            prompt,
            model,
            agent_slug_for_event,
            session_id,
            db_path,
            opts,
        );
    }

    // v2.3.21 Phase 6.x — API-key providers (MiniMax, Grok, Qwen, ...)
    // take a different path: no CLI binary to resolve, key comes from
    // env var or llm_api_keys, response over HTTPS. Persistence and
    // output shape are identical so downstream tools (events, audits)
    // don't need to care which transport was used.
    if let Some(provider) = crate::api_dispatch::find_provider(runtime_name) {
        return run_api(
            provider,
            prompt,
            model,
            agent_slug_for_event,
            session,
            stream,
            stream_jsonl,
            db_path,
            opts,
        );
    }
    let cli_path = runtime::resolve_runtime_cli(runtime_name)?;

    // v2.3.25 Phase 6.x — register in live_runs so the desktop's
    // Live tab shows this dispatch while it's in flight. Best-effort:
    // a missing table or locked DB just means the run is invisible
    // to the GUI, not that the dispatch fails. MiniMax round-1: use
    // a Drop guard so cleanup runs on every exit path (including
    // early `?` returns on spawn failure, panics, etc.).
    let live_run_id = uuid::Uuid::new_v4().to_string();
    let _ = crate::live_runs::insert(
        db_path,
        &live_run_id,
        runtime_name,
        agent_slug_for_event.as_deref(),
        None,
        "cli",
    );
    let _live_run_guard = crate::live_runs::LiveRunGuard::new(db_path, live_run_id);

    // v2.3.33 Phase 6 Slice B — when this CLI dispatch is part of a
    // cross-runtime session (session anchored to a different runtime),
    // claude --resume / codex --continue aren't usable. Build a
    // text-transcript prefix from session_turns and prepend it so the
    // runtime sees the conversation so far. For claude-on-claude
    // sessions, native --resume still owns continuity — we skip the
    // prefix to avoid duplicating context.
    let effective_prompt: String = if let Some(s) = &session {
        if s.runtime == runtime_name {
            prompt.to_string()
        } else {
            match db::open_readonly(db_path)
                .and_then(|c| crate::commands::sessions::fetch_turns(&c, &s.id))
            {
                Ok(turns) if !turns.is_empty() => {
                    let mut buf = String::from("=== Previous conversation ===\n");
                    for t in turns {
                        buf.push_str(&format!(
                            "[{} @{}] {}\n\n",
                            t.role, t.runtime, t.text
                        ));
                    }
                    buf.push_str("=== End previous conversation ===\n\n");
                    buf.push_str(prompt);
                    buf
                }
                _ => prompt.to_string(),
            }
        }
    } else {
        prompt.to_string()
    };

    let mut cmd = Command::new(&cli_path);
    match runtime_name {
        "claude" => {
            cmd.arg("--print").arg(&effective_prompt);
            if let Some(m) = &model {
                cmd.arg("--model").arg(m);
            }
            // v2.3.31 Slice A — wire claude --resume when the session
            // has a captured runtime_session_id, and switch output to
            // JSON so we can read back the session id metadata for
            // first-turn capture. Without --output-format json, claude
            // emits plain text and we can't reliably attribute the
            // turn to its session.
            //
            // v2.3.33 Slice B — only use --resume if the session is
            // *anchored* to claude. A session anchored to e.g. minimax
            // that's bridging to claude shouldn't try to resume — there
            // is no claude-native session id to resume from. The
            // text-transcript prefix above covers that case.
            if let Some(s) = &session {
                cmd.arg("--output-format").arg("json");
                if s.runtime == "claude" {
                    if let Some(rsid) = &s.runtime_session_id {
                        cmd.arg("--resume").arg(rsid);
                    }
                }
            }
        }
        "codex" => {
            // Codex requires `exec` + skip-git-repo-check (mirrors the
            // desktop's behaviour). Model goes before the prompt arg.
            cmd.arg("exec").arg("--skip-git-repo-check");
            if let Some(m) = &model {
                cmd.arg("--model").arg(m);
            }
            cmd.arg(&effective_prompt);
        }
        "gemini" => {
            cmd.arg("-p").arg(&effective_prompt);
            if let Some(m) = &model {
                cmd.arg("-m").arg(m);
            }
        }
        "hermes" => {
            cmd.arg("--execute").arg(&effective_prompt);
        }
        "openclaw" => {
            // OpenClaw's local CLI just takes `exec <prompt>` directly
            // when invoked without SSH. SSH-style remote dispatch is
            // a desktop-only feature for now (needs the ssh_config the
            // desktop loads from agent records).
            cmd.arg("exec").arg(&effective_prompt);
        }
        other => {
            anyhow::bail!("Unsupported runtime: {}", other);
        }
    }

    let started = Instant::now();
    let output = cmd
        .output()
        .with_context(|| format!("Failed to spawn {} CLI", runtime_name))?;
    let duration_ms = started.elapsed().as_millis() as i64;
    // _live_run_guard above will Drop at end of this fn / on any
    // early ? return; no manual delete call needed here.

    let response_text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();

    // v2.3.31 Slice A — when claude was invoked with --output-format
    // json (because --session was passed), the stdout is a JSON
    // envelope, not the raw model text. Pull `.result` for the
    // user-visible response and `.session_id` for sticky tracking.
    // For other runtimes (or no --session), stdout is the model
    // text directly.
    let (extracted_response, captured_runtime_session_id) = if session.is_some()
        && runtime_name == "claude"
        && output.status.success()
    {
        match serde_json::from_str::<serde_json::Value>(response_text.trim()) {
            Ok(v) => {
                let r = v["result"].as_str().map(|s| s.to_string());
                let sid = v["session_id"].as_str().map(|s| s.to_string());
                (r.unwrap_or_else(|| response_text.clone()), sid)
            }
            Err(_) => (response_text.clone(), None),
        }
    } else {
        (response_text.clone(), None)
    };

    let (status, response_persisted, error_persisted): (&str, Option<String>, Option<String>) =
        if output.status.success() {
            ("success", Some(truncate(&extracted_response)), None)
        } else {
            let msg = if stderr_text.is_empty() {
                format!("{} exited with status {}", runtime_name, output.status)
            } else {
                stderr_text
            };
            ("error", None, Some(truncate(&msg)))
        };

    // Compute usage estimates against the effective model (override or
    // runtime default). Mirrors the desktop's persist_execution_log.
    let effective_model = model
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| runtime::default_model_for_runtime(runtime_name).map(String::from));

    // v2.3.6 — Token estimates only. CLI dispatches always go through
    // the runtime CLI (claude --print, codex exec, gemini -p), which
    // means subscription billing. We don't pretend to know the dollar
    // cost; let the cost panels treat NULL as "subscription" cleanly.
    // See persist_execution_log in the desktop crate for the matching
    // rationale. Tokens are char-count based — populated regardless of
    // whether we have an effective_model, so runtimes without a default
    // model (openclaw, hermes) still get token rows.
    let response_for_cost = response_persisted.as_deref().unwrap_or("");
    let tokens_in = Some(runtime::estimate_text_tokens(prompt));
    let tokens_out = Some(runtime::estimate_text_tokens(response_for_cost));
    let cost_usd: Option<f64> = None;

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let conn = db::open_readwrite(db_path)?;
    // v2.3.41 — write session_id when present so the History panel
    // can group multi-turn conversations under one header.
    let session_id_for_log: Option<&str> = session.as_ref().map(|s| s.id.as_str());
    conn.execute(
        "INSERT INTO execution_logs (id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, cloud_trace_id, created_at, cost_usd_estimated, session_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?11, ?12)",
        rusqlite::params![
            id,
            runtime_name,
            truncate(prompt),
            response_persisted,
            tokens_in,
            tokens_out,
            duration_ms,
            status,
            error_persisted,
            now,
            cost_usd,
            session_id_for_log,
        ],
    ).context("Failed to write execution_logs row")?;

    // v2.3.27 Phase 6.x — quota capture. On error, try to parse a
    // reset time from the message and persist it so the next
    // dispatch's pre-flight can short-circuit. On success, clear
    // any stale quota row (the runtime is obviously not blocked).
    if status == "error" {
        if let Some(msg) = error_persisted.as_deref() {
            if let Some((resets_at, source)) = crate::quota::parse_reset_time(msg) {
                // MiniMax round-1 6.x: log instead of silently
                // swallowing. If upsert fails, future pre-flights
                // probe the API instead of short-circuiting — the
                // user should know why.
                if let Err(e) =
                    crate::quota::upsert(db_path, runtime_name, &resets_at, source)
                {
                    eprintln!(
                        "ato dispatch: failed to persist quota for '{}': {}",
                        runtime_name, e
                    );
                }
            }
        }
    } else if status == "success" {
        if let Err(e) = crate::quota::clear(db_path, runtime_name) {
            eprintln!(
                "ato dispatch: failed to clear quota for '{}': {}",
                runtime_name, e
            );
        }
    }

    // v2.3.9 Phase 4.3 — publish a DispatchFailed event to events_log
    // so the desktop's engine poll loop can pick it up and run matching
    // recipes. CLI dispatches don't go through the in-memory bus
    // (different process); events_log is the cross-process channel.
    if status == "error" {
        crate::events_publisher::publish_dispatch_failed(
            &conn,
            &id,
            agent_slug_for_event.as_deref(),
            runtime_name,
            error_persisted.as_deref().unwrap_or(""),
            duration_ms,
            &now,
        );
    }

    // v2.3.31 Slice A — if this dispatch belongs to a sticky session,
    // bump turn_count + last_used_at, and persist the captured
    // runtime_session_id when it's the first turn. COALESCE in the
    // UPDATE keeps the original session id stable across turns.
    // v2.3.32 Slice A.2 — ALSO append the turn to session_turns so
    // Slice B (cross-runtime switching) sees unified history. Claude
    // uses --resume on its own side, but we mirror here too.
    if let Some(s) = &session {
        let _ = crate::commands::sessions::append_turn(
            &conn,
            &s.id,
            "user",
            prompt,
            runtime_name,
        );
        if status == "success" {
            if let Some(resp) = response_persisted.as_deref() {
                let _ = crate::commands::sessions::append_turn(
                    &conn,
                    &s.id,
                    "assistant",
                    resp,
                    runtime_name,
                );
            }
        }
        if let Err(e) = crate::commands::sessions::record_turn(
            &conn,
            &s.id,
            captured_runtime_session_id.as_deref(),
        ) {
            eprintln!("ato dispatch: failed to record session turn: {}", e);
        }
    }

    let result = DispatchResult {
        id: id.clone(),
        runtime: runtime_name.to_string(),
        model: effective_model,
        status: status.to_string(),
        response: response_persisted,
        error_message: error_persisted,
        duration_ms,
        tokens_in,
        tokens_out,
        cost_usd_estimated: cost_usd,
        created_at: now,
    };

    if opts.human {
        let cost = result
            .cost_usd_estimated
            .map(|c| format!("${:.4}", c))
            .unwrap_or_else(|| "—".to_string());
        let head = format!(
            "[{}] {} {} ({}ms, {}, {})",
            result.status,
            result.runtime,
            result.model.as_deref().unwrap_or("?"),
            result.duration_ms,
            &result.id[..8.min(result.id.len())],
            cost
        );
        emit_human(&head);
        if let Some(r) = &result.response {
            emit_human("\n--- Response ---");
            emit_human(r);
        }
        if let Some(e) = &result.error_message {
            emit_human("\n--- Error ---");
            emit_human(e);
        }
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

/// 64 KB cap matching the desktop's truncate_for_log.
fn truncate(s: &str) -> String {
    const MAX: usize = 64 * 1024;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…[truncated]", &s[..MAX])
    }
}

/// v2.3.21 Phase 6.x — API-provider dispatch path. Same persistence
/// shape as the CLI path so execution_logs / events stay uniform.
/// v2.3.32 Slice A.2 — when `session` is Some, we fetch prior turns
/// and dispatch with full history (stateless providers can't resume
/// otherwise), then append the new user prompt + assistant response
/// as the next two turns.
fn run_api(
    provider: &crate::api_dispatch::ApiProvider,
    prompt: &str,
    model_override: Option<String>,
    agent_slug_for_event: Option<String>,
    session: Option<crate::commands::sessions::Session>,
    stream: bool,
    stream_jsonl: bool,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    // Quota pre-flight (same shape as the CLI-runtime path).
    if let Ok(Some(resets_at)) = crate::quota::lookup_future(db_path, provider.slug) {
        anyhow::bail!(
            "Provider '{}' is rate-limited until {} (cached). Try again after.",
            provider.slug,
            resets_at
        );
    }
    let conn = db::open_readwrite(db_path)?;

    // v2.3.25 Phase 6.x — register in live_runs so the desktop's
    // Live tab sees this API-provider dispatch in flight. Drop
    // guard handles cleanup on every exit path.
    let live_run_id = uuid::Uuid::new_v4().to_string();
    let _ = crate::live_runs::insert(
        db_path,
        &live_run_id,
        provider.slug,
        agent_slug_for_event.as_deref(),
        None,
        "cli",
    );
    let _live_run_guard = crate::live_runs::LiveRunGuard::new(db_path, live_run_id);

    // v2.3.32 Slice A.2 — if this dispatch is in a sticky session,
    // fetch the prior turns and replay them as the messages array.
    // Stateless providers (minimax / grok / deepseek / qwen /
    // openrouter) don't maintain session state on their end, so the
    // history HAS to come from us.
    let history: Vec<crate::api_dispatch::Message> = match &session {
        Some(s) => crate::commands::sessions::fetch_turns(&conn, &s.id)
            .unwrap_or_default()
            .into_iter()
            .map(|t| crate::api_dispatch::Message {
                role: t.role,
                content: t.text,
            })
            .collect(),
        None => Vec::new(),
    };
    // v2.3.47 Phase 6.x-F — streaming. Three output modes:
    //   - --human + --stream: write raw chunks to stdout as they
    //     arrive, then print the normal footer at the end.
    //   - --stream-jsonl (any --human setting): emit one JSON line
    //     per chunk {"type":"chunk","text":"..."} for desktop GUI
    //     / wrappers, then a {"type":"done","result":{...}} at end.
    //   - --stream alone in JSON mode: chunks suppressed; final
    //     DispatchResult JSON is the only stdout output (scripted
    //     callers' contract).
    let outcome = if stream {
        if opts.human && !stream_jsonl {
            emit_human(&format!(
                "[streaming from {} — chunks below]",
                provider.slug
            ));
            emit_human("");
        }
        use std::io::Write;
        crate::api_dispatch::dispatch_with_history_streaming(
            provider,
            &history,
            prompt,
            model_override.as_deref(),
            &conn,
            |chunk| {
                if stream_jsonl {
                    let event = serde_json::json!({
                        "type": "chunk",
                        "text": chunk,
                    });
                    println!("{}", event);
                    let _ = std::io::stdout().flush();
                } else if opts.human {
                    let mut out = std::io::stdout().lock();
                    let _ = out.write_all(chunk.as_bytes());
                    let _ = out.flush();
                }
            },
        )
    } else {
        crate::api_dispatch::dispatch_with_history(
            provider,
            &history,
            prompt,
            model_override.as_deref(),
            &conn,
        )
    };
    if stream && opts.human && !stream_jsonl {
        // Final newline after the stream so the next emit_human
        // doesn't run-on with the last chunk.
        println!();
    }

    let (status, response_persisted, error_persisted, duration_ms, model_used, tokens_in, tokens_out) =
        match outcome {
            Ok(o) => {
                let status = if o.response.is_some() { "success" } else { "error" };
                (
                    status,
                    o.response.map(|s| truncate(&s)),
                    o.error_message.map(|s| truncate(&s)),
                    o.duration_ms,
                    Some(o.model_used),
                    o.tokens_in,
                    o.tokens_out,
                )
            }
            Err(e) => (
                "error",
                None,
                Some(truncate(&e.to_string())),
                0_i64,
                None,
                None,
                None,
            ),
        };

    // Fall back to char-count estimate when the provider didn't return
    // a usage block (or when the call failed before reaching one).
    let tokens_in = tokens_in.or_else(|| Some(runtime::estimate_text_tokens(prompt)));
    let response_for_cost = response_persisted.as_deref().unwrap_or("");
    let tokens_out = tokens_out.or_else(|| Some(runtime::estimate_text_tokens(response_for_cost)));

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    // v2.3.41 — link the api-provider dispatch back to its session
    // so History grouping works for cross-runtime conversations.
    let session_id_for_log: Option<&str> = session.as_ref().map(|s| s.id.as_str());
    conn.execute(
        "INSERT INTO execution_logs (id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, cloud_trace_id, created_at, cost_usd_estimated, session_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, NULL, ?11)",
        rusqlite::params![
            id,
            provider.slug,
            truncate(prompt),
            response_persisted,
            tokens_in,
            tokens_out,
            duration_ms,
            status,
            error_persisted,
            now,
            session_id_for_log,
        ],
    )
    .context("Failed to write execution_logs row")?;

    if status == "error" {
        crate::events_publisher::publish_dispatch_failed(
            &conn,
            &id,
            agent_slug_for_event.as_deref(),
            provider.slug,
            error_persisted.as_deref().unwrap_or(""),
            duration_ms,
            &now,
        );
        if let Some(msg) = error_persisted.as_deref() {
            if let Some((resets_at, source)) = crate::quota::parse_reset_time(msg) {
                if let Err(e) =
                    crate::quota::upsert(db_path, provider.slug, &resets_at, source)
                {
                    eprintln!(
                        "ato dispatch: failed to persist quota for '{}': {}",
                        provider.slug, e
                    );
                }
            }
        }
    } else if status == "success" {
        if let Err(e) = crate::quota::clear(db_path, provider.slug) {
            eprintln!(
                "ato dispatch: failed to clear quota for '{}': {}",
                provider.slug, e
            );
        }
    }

    // v2.3.32 Slice A.2 — log this turn into session_turns for the
    // history replay path and bump session metadata. Only on
    // success-with-real-response do we append the assistant turn;
    // we still log the user turn so subsequent retries see what
    // was attempted.
    if let Some(s) = &session {
        let _ = crate::commands::sessions::append_turn(
            &conn,
            &s.id,
            "user",
            prompt,
            provider.slug,
        );
        if status == "success" {
            if let Some(resp) = response_persisted.as_deref() {
                let _ = crate::commands::sessions::append_turn(
                    &conn,
                    &s.id,
                    "assistant",
                    resp,
                    provider.slug,
                );
            }
        }
        // record_turn updates last_used_at + turn_count. For API
        // providers there's no runtime_session_id (stateless), so
        // pass None.
        if let Err(e) = crate::commands::sessions::record_turn(&conn, &s.id, None) {
            eprintln!("ato dispatch: failed to record session turn: {}", e);
        }
    }

    let result = DispatchResult {
        id: id.clone(),
        runtime: provider.slug.to_string(),
        model: model_used,
        status: status.to_string(),
        response: response_persisted,
        error_message: error_persisted,
        duration_ms,
        tokens_in,
        tokens_out,
        cost_usd_estimated: None,
        created_at: now,
    };

    if stream_jsonl {
        // v2.3.48 — final done event for the JSONL stream. Wraps the
        // same DispatchResult shape `emit_json` would emit so a
        // wrapper can use the line as a drop-in result.
        let done = serde_json::json!({"type": "done", "result": result});
        println!("{}", done);
    } else if opts.human {
        let head = format!(
            "[{}] {} {} ({}ms, {}, subscription)",
            result.status,
            result.runtime,
            result.model.as_deref().unwrap_or("?"),
            result.duration_ms,
            &result.id[..8.min(result.id.len())],
        );
        emit_human(&head);
        if let Some(r) = &result.response {
            emit_human("\n--- Response ---");
            emit_human(r);
        }
        if let Some(e) = &result.error_message {
            emit_human("\n--- Error ---");
            emit_human(e);
        }
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

/// v2.3.32 Phase 6.x-J — Remote runtime dispatch. Routes prompt to a
/// remote machine over SSH, captures stdout/stderr like a local
/// dispatch, persists to execution_logs with the *remote's slug* as
/// the runtime field. That way `ato dispatches list` shows the slug
/// the user typed (`claude-server`) instead of the base runtime
/// (`claude`), preserving the laptop-vs-server distinction in audits.
///
/// Sessions are intentionally NOT supported in this slice. Slice A
/// session storage assumes the base runtime can resume locally; the
/// remote-side equivalent (passing `--resume <rsid>` over SSH) needs
/// its own dogfood pass before we promise it works. Bails with a
/// clear error if the user passes --session.
fn run_remote(
    remote: crate::remote_runtime::RemoteRuntime,
    prompt: &str,
    model: Option<String>,
    agent_slug_for_event: Option<String>,
    session_id: Option<String>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    if session_id.is_some() {
        anyhow::bail!(
            "Sessions aren't supported on remote runtimes yet (Phase 6.x-J ships stateless dispatch only). Drop --session for one-shot remote calls."
        );
    }

    let live_run_id = uuid::Uuid::new_v4().to_string();
    let _ = crate::live_runs::insert(
        db_path,
        &live_run_id,
        &remote.slug,
        agent_slug_for_event.as_deref(),
        None,
        "cli",
    );
    let _live_run_guard = crate::live_runs::LiveRunGuard::new(db_path, live_run_id);

    let started = Instant::now();
    let output = crate::remote_runtime::exec(&remote, prompt, model.as_deref())?;
    let duration_ms = started.elapsed().as_millis() as i64;

    let response_text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();

    let (status, response_persisted, error_persisted): (&str, Option<String>, Option<String>) =
        if output.status.success() {
            ("success", Some(truncate(&response_text)), None)
        } else {
            let msg = if stderr_text.is_empty() {
                format!(
                    "{} (remote) exited with status {}",
                    remote.slug, output.status
                )
            } else {
                stderr_text
            };
            ("error", None, Some(truncate(&msg)))
        };

    let tokens_in = Some(crate::runtime::estimate_text_tokens(prompt));
    let tokens_out = Some(crate::runtime::estimate_text_tokens(
        response_persisted.as_deref().unwrap_or(""),
    ));
    let cost_usd: Option<f64> = None;

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let conn = db::open_readwrite(db_path)?;
    conn.execute(
        "INSERT INTO execution_logs (id, runtime, prompt, response, tokens_in, tokens_out, duration_ms, status, error_message, skill_name, cloud_trace_id, created_at, cost_usd_estimated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?11)",
        rusqlite::params![
            id,
            remote.slug,
            truncate(prompt),
            response_persisted,
            tokens_in,
            tokens_out,
            duration_ms,
            status,
            error_persisted,
            now,
            cost_usd,
        ],
    )
    .context("Failed to write execution_logs row (remote)")?;

    let result = DispatchResult {
        id: id.clone(),
        runtime: remote.slug.clone(),
        model: model.clone(),
        status: status.to_string(),
        response: response_persisted.clone(),
        error_message: error_persisted.clone(),
        duration_ms,
        tokens_in,
        tokens_out,
        cost_usd_estimated: cost_usd,
        created_at: now,
    };

    if opts.human {
        let head = format!(
            "[{}] {} (ssh→{}) model={} dur={}ms id={}",
            result.status,
            result.runtime,
            remote.host,
            result.model.as_deref().unwrap_or("?"),
            result.duration_ms,
            &result.id[..8.min(result.id.len())],
        );
        emit_human(&head);
        if let Some(r) = &result.response {
            emit_human("\n--- Response ---");
            emit_human(r);
        }
        if let Some(e) = &result.error_message {
            emit_human("\n--- Error ---");
            emit_human(e);
        }
    } else {
        emit_json(&result)?;
    }
    Ok(())
}
