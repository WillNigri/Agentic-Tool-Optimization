// Worker thread: consumes ScanRequest events from the watchers,
// coalesces bursts (notify can fire dozens of events for a single 4KB
// append) and delegates parsing to the per-runtime parser modules.
//
// SQLite work runs serially on this single thread so the watcher
// doesn't fight itself for the lock. All persistence goes through
// `persist::emit_row` which does an `INSERT OR IGNORE` against the
// partial UNIQUE index on (provider_session_id, sequence_within_session)
// — re-ingesting the same JSONL produces zero new rows.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use rusqlite::Connection;
use serde_json::Value;

use crate::parser_claude;
use crate::parser_codex;
use crate::parser_gemini;
use crate::persist;
use crate::sources::SourceKind;

pub struct ScanRequest {
    pub kind: SourceKind,
    pub path: PathBuf,
}

pub fn worker_loop(db_path: PathBuf, rx: &Receiver<ScanRequest>) {
    let mut pending: HashMap<PathBuf, SourceKind> = HashMap::new();
    // Per-file pair state lives across scans (per review HIGH-1).
    // A user-message line landing in scan N and the assistant
    // response landing in scan N+1 must still produce one row — the
    // SessionStateMap can't be recreated per scan or we'd lose every
    // live pair, since notify fires within ~250ms of the upstream
    // write and LLM responses commonly take 5-30s.
    let mut per_file_state: HashMap<(SourceKind, PathBuf), SessionStateMap> =
        HashMap::new();
    loop {
        let first = match rx.recv() {
            Ok(req) => req,
            Err(_) => return, // channel closed, exit cleanly
        };
        pending.insert(first.path, first.kind);
        // Coalesce notify bursts to one scan per file per quarter-
        // second.
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
            let key = (kind, path.clone());
            let state = per_file_state
                .entry(key)
                .or_insert_with(SessionStateMap::new);
            if let Err(e) = scan_file(&db_path, kind, &path, state) {
                eprintln!("passive_observer: scan {:?} failed: {}", path, e);
            }
        }
    }
}

/// Maximum bytes read per scan invocation. Caps memory growth on a
/// single-shot append from a runaway tool dump (per review MEDIUM-8).
/// When a file grew by more than this between scans, we read this
/// much, persist the offset, and let the next FS event pick up the
/// rest — bounded latency, bounded memory.
pub const MAX_READ_BYTES_PER_SCAN: u64 = 4 * 1024 * 1024;

pub fn scan_file(
    db_path: &Path,
    kind: SourceKind,
    path: &Path,
    session_state: &mut SessionStateMap,
) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let path_str = path.to_string_lossy().to_string();
    let (mut offset, mut last_seq) = load_state(&conn, kind, &path_str);
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            // File may have been deleted between event and scan —
            // drop our offset row so a future same-name file
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
    let is_fresh = metadata
        .modified()
        .ok()
        .and_then(|m| m.elapsed().ok())
        .map(|e| e < Duration::from_secs(5 * 60))
        .unwrap_or(false);
    if size < offset {
        // Truncate / rotation — reset and re-ingest from byte 0; the
        // UNIQUE INDEX drops rows we already emitted.
        offset = 0;
        last_seq = 0;
    }
    if size == offset {
        return Ok(());
    }
    file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let read_cap = (size - offset).min(MAX_READ_BYTES_PER_SCAN);
    let mut buf = vec![0u8; read_cap as usize];
    let n = file.read(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(n);

    let mut consumed: usize = 0;
    for line_end in line_iter(&buf) {
        let line = &buf[consumed..line_end];
        let trimmed = trim_newline(line);
        if !trimmed.is_empty() {
            if let Ok(value) = serde_json::from_slice::<Value>(trimmed) {
                process_line(
                    db_path,
                    kind,
                    &value,
                    session_state,
                    &mut last_seq,
                    is_fresh,
                );
            }
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
pub fn line_iter(buf: &[u8]) -> impl Iterator<Item = usize> + '_ {
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

pub fn trim_newline(line: &[u8]) -> &[u8] {
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

/// Per-session pair-state held only for the lifetime of one scan.
/// Persistence is keyed by (provider_session_id, sequence_within_session)
/// so resumption across scans doesn't need to thread session-level state.
pub struct SessionStateMap {
    pub sessions: HashMap<String, PendingPair>,
}

pub struct PendingPair {
    pub user_text: Option<String>,
    pub user_started_at: Option<String>,
    pub last_model: Option<String>,
    pub cwd: Option<String>,
    /// Codex token_count events arrive on a separate line from the
    /// assistant message — latch counts so we can attribute them to
    /// the matching assistant turn we emit next.
    pub pending_tokens_in: Option<i64>,
    pub pending_tokens_out: Option<i64>,
}

impl SessionStateMap {
    pub fn new() -> Self {
        Self { sessions: HashMap::new() }
    }
    pub fn get_or_init(&mut self, sid: &str) -> &mut PendingPair {
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
            parser_claude::process(db_path, line, state, last_seq, file_is_fresh)
        }
        SourceKind::Codex => {
            parser_codex::process(db_path, line, state, last_seq, file_is_fresh)
        }
        SourceKind::Gemini => {
            parser_gemini::process(db_path, line, state, last_seq, file_is_fresh)
        }
    }
}

/// Exposed for tests so parser modules can drive a single line through
/// the same path the worker uses.
pub fn process_line_for_tests(
    db_path: &Path,
    kind: SourceKind,
    line: &Value,
    state: &mut SessionStateMap,
    last_seq: &mut i64,
    file_is_fresh: bool,
) {
    process_line(db_path, kind, line, state, last_seq, file_is_fresh);
}

/// Bridge helper: parser modules call `persist::emit_row` with the
/// resolved fields. We re-export here so individual parsers don't
/// have to know about the SQL.
#[allow(clippy::too_many_arguments)]
pub fn emit(
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
    persist::emit_row(
        db_path,
        kind,
        session_id,
        sequence,
        prompt,
        response,
        tokens_in,
        tokens_out,
        model,
        started_at,
    );
}

pub fn mark_in_progress(
    db_path: &Path,
    kind: SourceKind,
    session_id: &str,
    started_at: Option<&str>,
    cwd: Option<&str>,
) {
    persist::mark_passive_in_progress(db_path, kind, session_id, started_at, cwd);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceKind;
    use std::io::Write;

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

    /// Regression for review HIGH-1: user line lands in scan N,
    /// assistant lands in scan N+1, the pair must still emit.
    /// Before the fix, SessionStateMap was scan-local — the user
    /// turn was lost between scans and no row was emitted.
    #[test]
    fn pair_survives_split_across_two_scans() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        crate::schema::ensure_schema(tmp_db.path()).unwrap();

        let tmp_jsonl = tempfile::NamedTempFile::new().unwrap();
        // Write only the user turn first.
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(tmp_jsonl.path())
                .unwrap();
            let user_line = serde_json::json!({
                "type": "user",
                "sessionId": "split-test-session",
                "timestamp": "2026-05-26T12:00:00Z",
                "message": { "role": "user", "content": "first prompt" }
            });
            writeln!(f, "{}", user_line).unwrap();
        }
        // Scan #1 — user turn alone. Nothing emitted yet.
        let mut state = SessionStateMap::new();
        scan_file(tmp_db.path(), SourceKind::ClaudeCode, tmp_jsonl.path(), &mut state)
            .unwrap();

        let conn = Connection::open(tmp_db.path()).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM execution_logs WHERE dispatch_kind='passive_observation'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "no row emitted yet (assistant turn still pending)");

        // Append the assistant turn.
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(tmp_jsonl.path())
                .unwrap();
            let assistant_line = serde_json::json!({
                "type": "assistant",
                "sessionId": "split-test-session",
                "timestamp": "2026-05-26T12:00:05Z",
                "message": {
                    "model": "claude-sonnet-4-6",
                    "content": [{ "type": "text", "text": "first answer" }],
                    "usage": { "input_tokens": 8, "output_tokens": 5 }
                }
            });
            writeln!(f, "{}", assistant_line).unwrap();
        }
        // Scan #2 — reuses `state`, so the pending user turn pairs
        // with the assistant turn and emits one row.
        scan_file(tmp_db.path(), SourceKind::ClaudeCode, tmp_jsonl.path(), &mut state)
            .unwrap();

        let n2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM execution_logs WHERE dispatch_kind='passive_observation'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n2, 1, "pair emits exactly one row after second scan");

        // Spot-check the row carries both prompt and response.
        let (prompt, response, tokens_in, tokens_out): (String, String, i64, i64) = conn
            .query_row(
                "SELECT prompt, response, tokens_in, tokens_out \
                   FROM execution_logs WHERE dispatch_kind='passive_observation'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(prompt, "first prompt");
        assert_eq!(response, "first answer");
        assert_eq!(tokens_in, 8);
        assert_eq!(tokens_out, 5);
    }
}
