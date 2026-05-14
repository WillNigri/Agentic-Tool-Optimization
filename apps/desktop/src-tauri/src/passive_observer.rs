// v2.6 PR-A — Tier 1 passive observer for terminal LLM CLI sessions.
//
// ATO's observatory positioning re-centered 2026-05-14: the user has
// to be able to see *everything* multi-LLM on this machine — what ATO
// dispatched AND what they fired themselves from any other CLI.
// Without this module a 56-minute Claude Code session is invisible
// from ATO's perspective while the user is paying the real cost.
//
// What this does:
//   1. Watches each known CLI's session directory (notify v6).
//   2. On each file event (and once at startup) re-scans changed
//      JSONL files from `byte_offset` stored in `watcher_state`.
//   3. Parses lines into (user_prompt, assistant_response) pairs and
//      writes them as `dispatch_kind='passive_observation'` rows
//      via the existing `persist_execution_log` helper.
//   4. Dedup is enforced by a UNIQUE index on
//      (provider_session_id, sequence_within_session) — re-ingesting
//      the same file produces zero new rows.
//
// What this does NOT do (deferred to later PRs in the v2.6 train):
//   - PR-A.follow-ups: Gemini CLI, Aider, Continue, Cursor, OpenCode
//   - PR-B: cloud-side polling of provider usage APIs
//   - PR-C: opt-in local mitmproxy capture
//
// Honesty stance for v2.6: we cannot peek inside the upstream CLI's
// process to read its env vars, so for billing_surface we use the
// "primary path of this CLI" (subscription) as the default. The
// auth_mode column on the ATO-side dispatch path still records the
// truth for *ATO's own* dispatches; this is purely an observer.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::Connection;
use serde_json::Value;

use crate::commands::{persist_execution_log, ObservationTag};

/// Top-level state held in Tauri's State<> for app lifetime. Start
/// is idempotent — a second call after `start` is a no-op so the
/// `setup` hook can re-fire on hot reload without leaking watchers.
pub struct PassiveObserver {
    started: bool,
    // Keep the watchers alive — dropping them stops the OS-level FS
    // event subscription.
    _watchers: Vec<RecommendedWatcher>,
}

impl PassiveObserver {
    pub fn new() -> Self {
        Self { started: false, _watchers: Vec::new() }
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    /// Idempotent start. Spawns one background thread that owns the
    /// file scanner + parser state. The watcher hands inbound file
    /// events to the same thread over an mpsc channel so all the
    /// SQLite I/O happens serially (cheap, avoids lock contention).
    pub fn start(&mut self, db_path: PathBuf) -> Result<(), String> {
        if self.started {
            return Ok(());
        }
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => {
                // Review Finding #1 — set started before bailing so a
                // re-fired setup hook doesn't re-eprintln every time on
                // a pathological HOME=unset machine. Idempotency wins.
                self.started = true;
                return Err("home directory unknown".to_string());
            }
        };

        let sources = discover_sources(&home);
        if sources.is_empty() {
            // No supported CLI directories present yet — that's fine.
            // The user may install Claude Code / Codex later; today's
            // watcher set stays empty until next desktop launch. A
            // future iteration could watch parent dirs for
            // create-of-the-CLI-dir, but that's overkill for v2.6.
            self.started = true;
            return Ok(());
        }

        let (tx, rx) = channel::<ScanRequest>();

        let mut watchers: Vec<RecommendedWatcher> = Vec::new();
        for src in &sources {
            // 2s poll matches the live-runs query cadence; tighter
            // would burn CPU watching mostly-idle dirs.
            let tx_clone = tx.clone();
            let src_clone = src.clone();
            let mut w = RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(ev) = res {
                        on_fs_event(&src_clone, ev, &tx_clone);
                    }
                },
                Config::default().with_poll_interval(Duration::from_secs(2)),
            )
            .map_err(|e| format!("failed to create watcher for {:?}: {}", src.root, e))?;
            if src.root.exists() {
                w.watch(&src.root, RecursiveMode::Recursive)
                    .map_err(|e| format!("failed to watch {:?}: {}", src.root, e))?;
            }
            watchers.push(w);
        }

        // Initial sweep: process every existing jsonl from its stored
        // offset so the user's pre-launch history catches up the moment
        // ATO starts. After this, fs events drive incremental updates.
        let initial_paths: Vec<(SourceKind, PathBuf)> = sources
            .iter()
            .flat_map(|s| enumerate_existing(s).into_iter().map(move |p| (s.kind, p)))
            .collect();
        for (kind, path) in initial_paths {
            let _ = tx.send(ScanRequest { kind, path });
        }

        let db = db_path.clone();
        std::thread::Builder::new()
            .name("ato-passive-observer".to_string())
            .spawn(move || {
                worker_loop(db, rx);
            })
            .map_err(|e| format!("failed to spawn watcher worker: {}", e))?;

        self._watchers = watchers;
        self.started = true;
        Ok(())
    }
}

// ── Source registry ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    ClaudeCode,
    Codex,
}

impl SourceKind {
    fn id(&self) -> &'static str {
        match self {
            SourceKind::ClaudeCode => "claude_code",
            SourceKind::Codex => "codex",
        }
    }
    fn runtime(&self) -> &'static str {
        // Reuse ATO's existing runtime taxonomy so the History panel
        // can render alongside ATO's own dispatches without a new
        // icon registry.
        match self {
            SourceKind::ClaudeCode => "claude",
            SourceKind::Codex => "codex",
        }
    }
    /// Default billing surface for this CLI's primary install path.
    /// Conservative honest default — we can't see the upstream
    /// process's env vars from a passive observer.
    fn default_billing_surface(&self) -> &'static str {
        match self {
            SourceKind::ClaudeCode => "claude_code_subscription",
            SourceKind::Codex => "codex_cli_subscription",
        }
    }
}

#[derive(Debug, Clone)]
struct Source {
    kind: SourceKind,
    root: PathBuf,
}

fn discover_sources(home: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    let claude_dir = home.join(".claude").join("projects");
    if claude_dir.exists() {
        out.push(Source { kind: SourceKind::ClaudeCode, root: claude_dir });
    }
    let codex_dir = home.join(".codex").join("sessions");
    if codex_dir.exists() {
        out.push(Source { kind: SourceKind::Codex, root: codex_dir });
    }
    out
}

fn is_session_file(kind: SourceKind, path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !name.ends_with(".jsonl") {
        return false;
    }
    match kind {
        SourceKind::ClaudeCode => true,
        SourceKind::Codex => name.starts_with("rollout-"),
    }
}

fn enumerate_existing(src: &Source) -> Vec<PathBuf> {
    let mut out = Vec::new();
    visit_jsonls(&src.root, src.kind, &mut out);
    out
}

fn visit_jsonls(dir: &Path, kind: SourceKind, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            visit_jsonls(&path, kind, out);
        } else if ft.is_file() && is_session_file(kind, &path) {
            out.push(path);
        }
    }
}

// ── Worker loop ───────────────────────────────────────────────────────

struct ScanRequest {
    kind: SourceKind,
    path: PathBuf,
}

fn on_fs_event(src: &Source, ev: Event, tx: &Sender<ScanRequest>) {
    if !matches!(ev.kind, EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)) {
        return;
    }
    for path in ev.paths {
        if !is_session_file(src.kind, &path) {
            continue;
        }
        let _ = tx.send(ScanRequest { kind: src.kind, path });
    }
}

fn worker_loop(
    db_path: PathBuf,
    rx: std::sync::mpsc::Receiver<ScanRequest>,
) {
    // Coalesce bursts: notify can emit dozens of events for a single
    // 4KB append. Drain pending events on a short window per file so
    // we don't open + seek + parse the same file 20 times in a row.
    let mut pending: HashMap<PathBuf, SourceKind> = HashMap::new();
    loop {
        let first = match rx.recv() {
            Ok(req) => req,
            Err(_) => return, // channel closed, exit cleanly
        };
        pending.insert(first.path, first.kind);
        // Drain anything that lands within 250 ms.
        let deadline = std::time::Instant::now() + Duration::from_millis(250);
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            match rx.recv_timeout(remaining) {
                Ok(req) => {
                    pending.insert(req.path, req.kind);
                }
                Err(_) => break,
            }
        }
        let batch: Vec<(SourceKind, PathBuf)> =
            pending.drain().map(|(p, k)| (k, p)).collect();
        for (kind, path) in batch {
            if let Err(e) = scan_file(&db_path, kind, &path) {
                // Stay quiet on individual file failures — the panel
                // is best-effort and one corrupted JSONL line should
                // never log-spam the console.
                eprintln!(
                    "passive_observer: scan {:?} failed: {}",
                    path, e
                );
            }
        }
    }
}

// ── Per-file scan + parser dispatch ────────────────────────────────────

fn scan_file(db_path: &Path, kind: SourceKind, path: &Path) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let path_str = path.to_string_lossy().to_string();
    let (mut offset, mut last_seq) = load_state(&conn, kind, &path_str);
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            // File may have been deleted between event and scan —
            // forget our offset row so a future same-name file
            // restarts cleanly.
            if e.kind() == std::io::ErrorKind::NotFound {
                let _ = conn.execute(
                    "DELETE FROM watcher_state WHERE source = ?1 AND file_path = ?2",
                    rusqlite::params![kind.id(), &path_str],
                );
                return Ok(());
            }
            return Err(e.to_string());
        }
    };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    let size = metadata.len();
    // A file is "fresh" if its OS mtime is within the recent window —
    // that's our proxy for "this session might still be in progress."
    // Stale files get history rows only; live_runs writes are skipped
    // so we don't synthesize a fake live session on a 6-hour-old log
    // during the initial sweep.
    let is_fresh = metadata
        .modified()
        .ok()
        .and_then(|m| m.elapsed().ok())
        .map(|e| e < Duration::from_secs(5 * 60))
        .unwrap_or(false);
    if size < offset {
        // File was truncated or replaced (e.g. CLI rotated logs).
        // Reset and re-ingest from the start; INSERT OR IGNORE on
        // the unique index drops duplicates from prior runs.
        offset = 0;
        last_seq = 0;
    }
    if size == offset {
        return Ok(()); // no new bytes
    }
    file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let mut buf = Vec::with_capacity((size - offset) as usize);
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;

    // Split into complete lines; trailing partial line stays buffered
    // (we just don't advance the offset past it).
    let mut consumed: usize = 0;
    let mut session_state = SessionStateMap::new();
    for line_end in line_iter(&buf) {
        let line = &buf[consumed..line_end];
        let trimmed = trim_newline(line);
        if !trimmed.is_empty() {
            if let Ok(value) = serde_json::from_slice::<Value>(trimmed) {
                process_line(
                    db_path,
                    kind,
                    &value,
                    &mut session_state,
                    &mut last_seq,
                    is_fresh,
                );
            }
            // Malformed line: skip silently. Notepad-edited or partial
            // writes shouldn't kill the watcher for the whole file.
        }
        consumed = line_end;
    }
    let new_offset = offset + consumed as u64;
    save_state(&conn, kind, &path_str, new_offset, last_seq);
    Ok(())
}

/// Iterator over end-of-line positions (exclusive) in a byte slice.
/// Stops short of any trailing partial line so callers can leave it
/// buffered until the next read.
fn line_iter(buf: &[u8]) -> impl Iterator<Item = usize> + '_ {
    let mut starts: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == b'\n' {
            starts.push(i + 1);
        }
        i += 1;
    }
    starts.into_iter()
}

fn trim_newline(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    if end > 0 && line[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && line[end - 1] == b'\r' {
        end -= 1;
    }
    &line[..end]
}

fn load_state(conn: &Connection, kind: SourceKind, path: &str) -> (u64, i64) {
    conn.query_row(
        "SELECT byte_offset, last_seq FROM watcher_state \
            WHERE source = ?1 AND file_path = ?2",
        rusqlite::params![kind.id(), path],
        |r| {
            let off: i64 = r.get(0)?;
            let seq: i64 = r.get(1)?;
            Ok((off.max(0) as u64, seq))
        },
    )
    .ok()
    .unwrap_or((0, 0))
}

fn save_state(conn: &Connection, kind: SourceKind, path: &str, offset: u64, last_seq: i64) {
    let now = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "INSERT INTO watcher_state (source, file_path, byte_offset, last_seq, updated_at) \
            VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT (source, file_path) DO UPDATE SET \
            byte_offset = excluded.byte_offset, \
            last_seq    = excluded.last_seq, \
            updated_at  = excluded.updated_at",
        rusqlite::params![kind.id(), path, offset as i64, last_seq, now],
    );
}

// ── Per-session parser state ──────────────────────────────────────────

/// Holds the in-progress pair-state for every session_id seen during
/// one `scan_file` pass. Lives only for the duration of the scan; the
/// `sequence_within_session` we emit comes from `last_seq` (per-file
/// monotonic counter persisted in watcher_state) so resumption is
/// idempotent without needing to thread session-level state across
/// scans.
struct SessionStateMap {
    sessions: HashMap<String, PendingPair>,
}

struct PendingPair {
    user_text: Option<String>,
    user_started_at: Option<String>,
    last_model: Option<String>,
    cwd: Option<String>,
    /// Codex token_count events arrive on a separate line from the
    /// assistant message. Latch the most recent counts so we can
    /// attribute them to the next assistant message we emit.
    pending_tokens_in: Option<i64>,
    pending_tokens_out: Option<i64>,
}

impl SessionStateMap {
    fn new() -> Self {
        Self { sessions: HashMap::new() }
    }
    fn get_or_init(&mut self, sid: &str) -> &mut PendingPair {
        self.sessions.entry(sid.to_string()).or_insert_with(|| PendingPair {
            user_text: None,
            user_started_at: None,
            last_model: None,
            cwd: None,
            pending_tokens_in: None,
            pending_tokens_out: None,
        })
    }
}

fn process_line(
    db_path: &Path,
    kind: SourceKind,
    line: &Value,
    state: &mut SessionStateMap,
    last_seq: &mut i64,
    file_is_fresh: bool,
) {
    match kind {
        SourceKind::ClaudeCode => {
            process_claude_line(db_path, line, state, last_seq, file_is_fresh)
        }
        SourceKind::Codex => {
            process_codex_line(db_path, line, state, last_seq, file_is_fresh)
        }
    }
}

// ── Claude Code parser ────────────────────────────────────────────────
//
// ~/.claude/projects/<workspace-slug>/<session-uuid>.jsonl. Each line
// is one event; relevant types are `user` and `assistant`. The
// `sessionId` field is stable per file. `isMeta: true` marks CLI-
// injected reminders (slash commands, local-command-caveat blocks) —
// we skip those because they're not the user's typed prompt.

fn process_claude_line(
    db_path: &Path,
    line: &Value,
    state: &mut SessionStateMap,
    last_seq: &mut i64,
    file_is_fresh: bool,
) {
    let ty = line.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let session_id = match line.get("sessionId").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return,
    };
    let cwd = line.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
    let timestamp = line
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match ty {
        "user" => {
            // Skip CLI-injected reminders (`isMeta: true`) — slash
            // commands, "local command caveat" blocks, etc.
            if line.get("isMeta").and_then(|v| v.as_bool()).unwrap_or(false) {
                return;
            }
            let Some(message) = line.get("message") else { return };
            let role = message.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role != "user" {
                return;
            }
            // message.content is either a plain string (typed prompt)
            // or an array (tool result reply, image, etc). For
            // execution_logs we only care about the typed-prompt case.
            let text = match message.get("content") {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Array(parts)) => {
                    // If any element is a tool_result, this turn is
                    // a tool reply, not a user prompt — skip.
                    let any_tool_result = parts.iter().any(|p| {
                        p.get("type").and_then(|v| v.as_str()) == Some("tool_result")
                    });
                    if any_tool_result {
                        None
                    } else {
                        let collected: Vec<&str> = parts
                            .iter()
                            .filter_map(|p| {
                                if p.get("type").and_then(|v| v.as_str()) == Some("text") {
                                    p.get("text").and_then(|v| v.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if collected.is_empty() {
                            None
                        } else {
                            Some(collected.join("\n"))
                        }
                    }
                }
                _ => None,
            };
            let Some(text) = text else { return };
            let pair = state.get_or_init(session_id);
            pair.user_text = Some(text);
            pair.user_started_at = timestamp.clone();
            if cwd.is_some() {
                pair.cwd = cwd.clone();
            }
            if file_is_fresh {
                mark_passive_in_progress(
                    db_path,
                    SourceKind::ClaudeCode,
                    session_id,
                    timestamp.as_deref(),
                    cwd.as_deref(),
                );
            }
        }
        "assistant" => {
            let Some(message) = line.get("message") else { return };
            // Pure tool_use turns (no human-visible text) are skipped —
            // they aren't an "answer" to attribute to a prompt.
            let content = message.get("content");
            let text_parts: Vec<String> = match content {
                Some(Value::Array(parts)) => parts
                    .iter()
                    .filter_map(|p| {
                        if p.get("type").and_then(|v| v.as_str()) == Some("text") {
                            p.get("text")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            };
            if text_parts.is_empty() {
                return;
            }
            let response = text_parts.join("\n");
            let model = message
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // Anthropic's usage block on each assistant message.
            // `input_tokens` reflects this turn only (cache stats
            // omitted from the panel intentionally — they live one
            // level deeper if a future analytics pass wants them).
            let tokens_in = message
                .get("usage")
                .and_then(|u| u.get("input_tokens"))
                .and_then(|v| v.as_i64());
            let tokens_out = message
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_i64());

            let pair = state.get_or_init(session_id);
            if let Some(m) = &model {
                pair.last_model = Some(m.clone());
            }
            let prompt = match pair.user_text.take() {
                Some(t) => t,
                None => {
                    // Assistant message with no preceding user prompt
                    // we captured — most often this is a continuation
                    // after a tool turn. Skip without erroring.
                    return;
                }
            };
            let started_at = pair.user_started_at.take().or(timestamp.clone());

            let seq = *last_seq + 1;
            emit_row(
                db_path,
                SourceKind::ClaudeCode,
                session_id,
                seq,
                &prompt,
                &response,
                tokens_in,
                tokens_out,
                model.as_deref().or(pair.last_model.as_deref()),
                started_at.as_deref(),
            );
            *last_seq = seq;
        }
        _ => {}
    }
}

// ── Codex CLI parser ──────────────────────────────────────────────────
//
// ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl. First line is
// `session_meta` (carries the session id + model_provider + cwd).
// Subsequent lines are `response_item` (messages) and `event_msg`
// (state updates incl. token_count). We pair user `input_text`
// messages with the following assistant `output_text` message.

fn process_codex_line(
    db_path: &Path,
    line: &Value,
    state: &mut SessionStateMap,
    last_seq: &mut i64,
    file_is_fresh: bool,
) {
    let ty = line.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let timestamp = line
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let Some(payload) = line.get("payload") else { return };

    match ty {
        "session_meta" => {
            let sid = match payload.get("id").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return,
            };
            let cwd = payload
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let pair = state.get_or_init(&sid);
            if pair.cwd.is_none() {
                pair.cwd = cwd;
            }
            // Codex doesn't expose the chosen model in session_meta
            // directly (only the provider). The model name lands on
            // response_item.payload.model in newer versions; if it
            // never lands we leave last_model = None and the row
            // falls back to the runtime default in pricing lookups.
        }
        "event_msg" => {
            // Token counters publish per-turn after the assistant
            // message lands. We latch the most-recent totals and
            // apply them to the next assistant turn we emit.
            let inner = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if inner != "token_count" {
                return;
            }
            let usage = payload.get("info").and_then(|v| v.get("last_token_usage"));
            let Some(u) = usage else { return };
            let t_in = u.get("input_tokens").and_then(|v| v.as_i64());
            let t_out = u.get("output_tokens").and_then(|v| v.as_i64());
            // Review Finding #3 — Codex doesn't repeat session id on
            // event_msg lines; there's only ever one session per
            // rollout file. Scope the latch to that single session
            // explicitly rather than iterating, so a hypothetical
            // future multi-session rollout format doesn't bleed
            // counts across pairs.
            //
            // Review Finding #4 — we assume token_count events arrive
            // BEFORE the next user message AND AFTER the assistant
            // response they apply to (Codex's task_started → message
            // → token_count ordering on real rollouts confirms this).
            // If a future Codex version reverses the order we'd
            // attribute counts to the previous turn; surfacing as
            // honest-but-imprecise per-row data, not a correctness
            // failure on aggregate spend.
            if let Some(pair) = state.sessions.values_mut().next() {
                pair.pending_tokens_in = t_in;
                pair.pending_tokens_out = t_out;
            }
        }
        "response_item" => {
            let inner = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if inner != "message" {
                return;
            }
            let role = payload.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let model = payload
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Codex doesn't include the session id on every line —
            // it's only on session_meta. We rely on there being a
            // single pending session in the map (always true for
            // rollout files which are 1-session-per-file).
            let Some(sid) = state.sessions.keys().next().cloned() else {
                // session_meta hadn't landed yet — happens if a
                // future Codex version reorders, or the file is
                // partial. Skip until we see session_meta.
                return;
            };

            let content_items = payload
                .get("content")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            // Concatenate every input_text / output_text element.
            let pick_text = |types: &[&str]| -> String {
                content_items
                    .iter()
                    .filter_map(|it| {
                        let t = it.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if types.contains(&t) {
                            it.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            match role {
                "user" => {
                    let text = pick_text(&["input_text"]);
                    if text.is_empty() {
                        return;
                    }
                    // Filter out the giant AGENTS.md / permissions
                    // injection lines that codex prepends — those
                    // aren't the user's typed prompt and would
                    // pollute the History panel. Heuristic markers
                    // observed on real rollouts.
                    if text.starts_with("<permissions instructions>")
                        || text.starts_with("# AGENTS.md")
                        || text.starts_with("You are continuing an ongoing conversation")
                    {
                        return;
                    }
                    let cwd_now = state.sessions.get(&sid).and_then(|s| s.cwd.clone());
                    let pair = state.get_or_init(&sid);
                    pair.user_text = Some(text);
                    pair.user_started_at = timestamp.clone();
                    if file_is_fresh {
                        mark_passive_in_progress(
                            db_path,
                            SourceKind::Codex,
                            &sid,
                            timestamp.as_deref(),
                            cwd_now.as_deref(),
                        );
                    }
                }
                "assistant" => {
                    let response = pick_text(&["output_text"]);
                    if response.is_empty() {
                        return;
                    }
                    let pair = state.get_or_init(&sid);
                    if let Some(m) = model.as_ref() {
                        pair.last_model = Some(m.clone());
                    }
                    let prompt = match pair.user_text.take() {
                        Some(t) => t,
                        None => return,
                    };
                    let started_at = pair.user_started_at.take().or(timestamp.clone());
                    let tokens_in = pair.pending_tokens_in.take();
                    let tokens_out = pair.pending_tokens_out.take();
                    let model_str = pair.last_model.clone();
                    let seq = *last_seq + 1;
                    emit_row(
                        db_path,
                        SourceKind::Codex,
                        &sid,
                        seq,
                        &prompt,
                        &response,
                        tokens_in,
                        tokens_out,
                        model_str.as_deref(),
                        started_at.as_deref(),
                    );
                    *last_seq = seq;
                }
                _ => {}
            }
        }
        _ => {}
    }
}

// ── Persist ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn emit_row(
    db_path: &Path,
    kind: SourceKind,
    session_id: &str,
    sequence: i64,
    prompt: &str,
    response: &str,
    tokens_in: Option<i64>,
    tokens_out: Option<i64>,
    model: Option<&str>,
    started_at: Option<&str>,
) {
    let tag = ObservationTag {
        dispatch_kind: "passive_observation",
        billing_surface: Some(kind.default_billing_surface()),
        provider_session_id: Some(session_id),
        sequence_within_session: Some(sequence),
        tokens_in,
        tokens_out,
        observed_at: started_at,
    };
    persist_execution_log(
        kind.runtime(),
        prompt,
        &Ok(response.to_string()),
        0, // duration unknown for observed sessions
        model,
        None, // agent_slug — we don't know which agent the user invoked
        Some(&tag),
    );
    // The user→assistant pair is complete; if we'd inserted a
    // "session in progress" row when the user message landed, clear
    // it now so the Live tab returns to idle for this session.
    clear_passive_live_row(db_path, kind, session_id);
}

/// Insert a transient live_runs row when we see a user message in a
/// session we haven't already marked in-progress. The companion
/// `clear_passive_live_row` removes it once the assistant response
/// lands (or on next boot via the existing `DELETE FROM live_runs`).
///
/// run_id key shape `passive:<source-id>:<session-uuid>` keeps the
/// row unique across watcher restarts within the same desktop
/// session (re-emitting on file event is idempotent due to INSERT OR
/// REPLACE) and visually identifiable in the CLI debug surface.
fn mark_passive_in_progress(
    db_path: &Path,
    kind: SourceKind,
    session_id: &str,
    started_at: Option<&str>,
    cwd: Option<&str>,
) {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let run_id = format!("passive:{}:{}", kind.id(), session_id);
    let started = started_at
        .map(|s| s.to_string())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    // INSERT OR IGNORE — a second user message before the assistant
    // lands (e.g. the user appended) shouldn't reset started_at to
    // the later time. We want "first user prompt that opened this
    // pending turn."
    let _ = conn.execute(
        "INSERT OR IGNORE INTO live_runs \
            (run_id, agent_slug, runtime, workspace, source, started_at, status, child_pid, dispatch_kind, billing_surface) \
         VALUES (?1, NULL, ?2, ?3, ?4, ?5, 'running', NULL, 'passive_observation', ?6)",
        rusqlite::params![
            run_id,
            kind.runtime(),
            cwd,
            format!("observed:{}", kind.id()),
            started,
            kind.default_billing_surface(),
        ],
    );
}

fn clear_passive_live_row(db_path: &Path, kind: SourceKind, session_id: &str) {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let run_id = format!("passive:{}:{}", kind.id(), session_id);
    let _ = conn.execute("DELETE FROM live_runs WHERE run_id = ?1", [&run_id]);
}

// ── Tauri state wrapper ───────────────────────────────────────────────

pub struct PassiveObserverState(pub Arc<Mutex<PassiveObserver>>);

impl PassiveObserverState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(PassiveObserver::new())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_session_file_filters_correctly() {
        let p = PathBuf::from("/home/me/.claude/projects/x/abcd.jsonl");
        assert!(is_session_file(SourceKind::ClaudeCode, &p));
        let p = PathBuf::from("/home/me/.claude/projects/x/sessions-index.json");
        assert!(!is_session_file(SourceKind::ClaudeCode, &p));
        let p = PathBuf::from("/home/me/.codex/sessions/2026/05/06/rollout-x.jsonl");
        assert!(is_session_file(SourceKind::Codex, &p));
        let p = PathBuf::from("/home/me/.codex/sessions/2026/05/06/notes.jsonl");
        assert!(!is_session_file(SourceKind::Codex, &p));
    }

    #[test]
    fn trim_newline_handles_crlf() {
        assert_eq!(trim_newline(b"hello\n"), b"hello");
        assert_eq!(trim_newline(b"hello\r\n"), b"hello");
        assert_eq!(trim_newline(b"hello"), b"hello");
        assert_eq!(trim_newline(b""), b"");
    }

    #[test]
    fn line_iter_yields_complete_lines_only() {
        let buf = b"a\nbb\nccc";
        let ends: Vec<usize> = line_iter(buf).collect();
        // "a\n" ends at 2, "bb\n" ends at 5; "ccc" is partial and skipped.
        assert_eq!(ends, vec![2, 5]);
    }
}
