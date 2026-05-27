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

/// How often to walk `per_file_state` and drop entries whose
/// backing file has been deleted/rotated. Per coordinator MEDIUM-3.
/// Long-running daemons (days of dogfood) would otherwise accumulate
/// a state map proportional to every session file ever observed.
const GC_INTERVAL: Duration = Duration::from_secs(10 * 60);

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
    let mut last_gc_at = std::time::Instant::now();
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
        if last_gc_at.elapsed() >= GC_INTERVAL {
            gc_stale_state(&mut per_file_state);
            last_gc_at = std::time::Instant::now();
        }
    }
}

/// Drop in-memory `SessionStateMap` entries whose backing file no
/// longer exists on disk (deleted, rotated, moved). The watcher's
/// scan_file already DELETEs the matching `watcher_state` row on a
/// NotFound; this matches that cleanup in memory so a week-long
/// daemon doesn't keep state for thousands of vanished sessions.
///
/// Performance note (coordinator re-review gemini-#3): `path.exists()`
/// is a `stat(2)` syscall per entry. On a local FS at the scale we
/// care about (1k-10k entries) the GC pass is sub-millisecond. If
/// the daemon is run against a slow NFS / SMB mount and the entry
/// count grows into the tens of thousands, swap this for an
/// ino/dev-tracking scheme that prunes inline when scan_file hits
/// ENOENT. The 10-minute GC_INTERVAL means even a 1s pass would
/// only stall the worker once every 10 minutes.
///
/// Exposed (pub(crate)) so the regression test can drive it directly
/// without waiting GC_INTERVAL.
pub(crate) fn gc_stale_state(
    per_file_state: &mut HashMap<(SourceKind, PathBuf), SessionStateMap>,
) {
    per_file_state.retain(|(_, path), _| path.exists());
}

/// Maximum bytes read per scan invocation. Caps memory growth on a
/// single-shot append from a runaway tool dump (per review MEDIUM-8).
/// When a file grew by more than this between scans, we read this
/// much, persist the offset, and let the next FS event pick up the
/// rest — bounded latency, bounded memory.
///
/// Per coordinator re-review gemini-#4: 4MB is a pragmatic middle
/// ground. Typical Claude/Codex session deltas are a few KB per
/// event; embedded systems with <1GB RAM can absorb a single 4MB
/// buf allocation comfortably; abundant-RAM systems lose little by
/// breaking a 50MB append into thirteen 4MB scans, since notify
/// fires continuously on the same file. Not exposed as
/// configuration — a real performance constraint hitting this cap
/// is a sign of an upstream-CLI behavioural change worth tracing,
/// not a tuning opportunity.
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

    // Detect file shape. Per coordinator review MEDIUM-1, Gemini
    // CLI's older `logs.json` is a top-level JSON array (one big
    // `[ {...}, {...} ]` document). That format defeats line-
    // delimited incremental scanning: appending an element shifts
    // the closing `]` and emits no complete-line in between. Peek
    // the first non-whitespace byte from byte 0 (NOT the offset) —
    // we need to see the leading `[` regardless of how far we've
    // already scanned.
    //
    // Coordinator re-review LOW-6: gate on Gemini specifically.
    // Claude and Codex don't have a documented array layout; a
    // file in their tree starting with `[` is corruption or a
    // stray fixture, and routing it into scan_json_array_file
    // would leave byte_offset stuck at 0 and re-scan it forever.
    if kind == SourceKind::Gemini && is_json_array_file(&mut file).unwrap_or(false) {
        return scan_json_array_file(db_path, kind, &mut file, &path_str, &conn, session_state);
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

/// Sniff the first non-whitespace byte. `true` if the file is shaped
/// like a top-level JSON array (`[...]`). Resets the seek position to
/// the start before returning so the caller can re-read.
fn is_json_array_file(file: &mut std::fs::File) -> Result<bool, String> {
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut peek = [0u8; 64];
    let n = file.read(&mut peek).map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    Ok(peek[..n]
        .iter()
        .find(|b| !b.is_ascii_whitespace())
        .map(|b| *b == b'[')
        .unwrap_or(false))
}

/// Per-array-file cap. Per coordinator re-review MEDIUM-3:
/// Gemini's older `logs.json` is bounded by a single session, so
/// the 4MB scan cap that protects the JSONL paths is unnecessarily
/// restrictive here — a heavy Gemini chat session can easily clear
/// 4MB and would otherwise become permanently invisible. Raise to
/// 32MB; a session that exceeds this is genuinely pathological and
/// worth surfacing (still log-once-per-file rather than thrashing
/// stderr on every FS event).
///
/// Re-review-2 (gemini) HIGH-1: this is a documented hard skip.
/// If a real customer hits it, the daemon eprintln's once per
/// daemon-restart-per-file and the file's events are silently
/// dropped above the cap. We don't write a persistent sentinel
/// (re-review-2 LOW-5) — instead the in-memory `oversized_warned`
/// flag in SessionStateMap dedups the log; restart re-logs once.
/// A future move to incremental array parsing would lift this cap.
const MAX_ARRAY_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// Re-read the whole file as one JSON document and process every
/// element. Used for Gemini's older `logs.json` shape. Dedup is
/// preserved by the UNIQUE INDEX on (provider_session_id,
/// sequence_within_session) — re-processed events with the same
/// (session, array-index)-derived sequence are silently dropped at
/// INSERT.
///
/// Per coordinator re-review MEDIUM-4: we use `watcher_state.
/// byte_offset` as "size of the file at last successful scan." If
/// the current file size matches, the array hasn't grown and we
/// skip all the re-read + re-parse + repeated-INSERT work — but
/// that short-circuit lives at `scan_file`'s outer `size == offset`
/// check (re-review-2 MEDIUM-1: deleting the inner duplicate that
/// was unreachable). The same load_state/save_state column powers
/// both, so they can't disagree.
fn scan_json_array_file(
    db_path: &Path,
    kind: SourceKind,
    file: &mut std::fs::File,
    path_str: &str,
    conn: &Connection,
    session_state: &mut SessionStateMap,
) -> Result<(), String> {
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    let size = metadata.len();

    // Re-review-2 (gemini) HIGH-1 + (claude) LOW-5: log-once on
    // oversize files using in-memory state, no persisted sentinel.
    // The previous i64::MAX sentinel overloaded byte_offset's
    // semantics; tracking the warning in SessionStateMap keeps the
    // column meaning honest. The cost is one re-log per daemon
    // restart per file — acceptable trade for unambiguous storage.
    if size > MAX_ARRAY_FILE_BYTES {
        if !session_state.oversized_warned {
            eprintln!(
                "passive_observer: skipping JSON-array file {} ({} bytes > {} cap). \
                 Subsequent FS events on this file silently no-op until restart.",
                path_str, size, MAX_ARRAY_FILE_BYTES
            );
            session_state.oversized_warned = true;
        }
        return Ok(());
    }

    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut buf = Vec::with_capacity(size as usize);
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let value: Value = match serde_json::from_slice(&buf) {
        Ok(v) => v,
        Err(_) => {
            // Possibly still being written — leave for the next FS
            // event. Don't update byte_offset; we want to retry.
            return Ok(());
        }
    };
    let is_fresh = metadata
        .modified()
        .ok()
        .and_then(|m| m.elapsed().ok())
        .map(|e| e < Duration::from_secs(5 * 60))
        .unwrap_or(false);
    // Reset last_seq for the scan; parser increments from 0, so the
    // first event emits sequence=1 deterministically. Re-scans of an
    // appended array re-process from the start with the same
    // sequence assignments — INSERT OR IGNORE handles dedup.
    let mut last_seq: i64 = 0;
    process_line(db_path, kind, &value, session_state, &mut last_seq, is_fresh);
    // Persist the file size so the outer `size == offset` check at
    // scan_file:162 short-circuits unchanged-size re-scans.
    // Re-review-2 (claude) MEDIUM-1: the previous in-function copy
    // of this short-circuit was dead code; the outer check fires
    // first because load_state reads this same column.
    save_state(conn, kind, path_str, size, last_seq);
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

/// Per-session pair-state held across scans (per coordinator review
/// HIGH-1) so a user line in scan N and an assistant line in scan
/// N+1 still produce one row. Per-file map lives in the worker_loop
/// (see `per_file_state`); this struct is the per-file value.
///
/// `oversized_warned` (re-review-2 HIGH-1 / LOW-5): for Gemini
/// array files whose size exceeds MAX_ARRAY_FILE_BYTES, we log
/// once per file and silently no-op every subsequent FS event
/// (an `i64::MAX` sentinel in `watcher_state.byte_offset` was the
/// previous design, dropped because it overloaded the column).
pub struct SessionStateMap {
    pub sessions: HashMap<String, PendingPair>,
    pub oversized_warned: bool,
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
        Self { sessions: HashMap::new(), oversized_warned: false }
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

    /// Regression for coordinator MEDIUM-3: per_file_state must drop
    /// entries whose backing file has been deleted. Simulates a
    /// rotated session file and asserts the GC pass cleans the map.
    #[test]
    fn gc_drops_state_for_deleted_files() {
        let alive = tempfile::NamedTempFile::new().unwrap();
        let to_delete = tempfile::NamedTempFile::new().unwrap();
        let alive_path = alive.path().to_path_buf();
        let deleted_path = to_delete.path().to_path_buf();
        // Drop the second file so its path resolves to nothing.
        drop(to_delete);
        assert!(alive_path.exists());
        assert!(!deleted_path.exists());

        let mut map: HashMap<(SourceKind, PathBuf), SessionStateMap> = HashMap::new();
        map.insert(
            (SourceKind::ClaudeCode, alive_path.clone()),
            SessionStateMap::new(),
        );
        map.insert(
            (SourceKind::ClaudeCode, deleted_path.clone()),
            SessionStateMap::new(),
        );
        assert_eq!(map.len(), 2);

        gc_stale_state(&mut map);

        assert_eq!(map.len(), 1, "stale entry dropped");
        assert!(map.contains_key(&(SourceKind::ClaudeCode, alive_path)));
        assert!(!map.contains_key(&(SourceKind::ClaudeCode, deleted_path)));
    }

    /// Regression for coordinator MEDIUM-1: Gemini `logs.json`
    /// (top-level JSON array, multi-line pretty-printed) must be
    /// detected by shape sniffing and processed as a whole-file
    /// re-read instead of being eaten by line-delimited scanning.
    #[test]
    fn gemini_top_level_array_logs_json() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        crate::schema::ensure_schema(tmp_db.path()).unwrap();

        let tmp_json = tempfile::NamedTempFile::new().unwrap();
        let events = serde_json::json!([
            {
                "type": "user",
                "sessionId": "gem-array-1",
                "message": "what's the weather?",
                "timestamp": "2026-05-26T12:00:00Z"
            },
            {
                "type": "model",
                "sessionId": "gem-array-1",
                "message": "I don't have realtime data, but…",
                "model": "gemini-2.5-flash",
                "timestamp": "2026-05-26T12:00:02Z",
                "usage": { "promptTokenCount": 6, "candidatesTokenCount": 11 }
            }
        ]);
        // Pretty-print on purpose — that's the case incremental
        // line-scanning fails to handle, so we want the test asset
        // to look exactly like real Gemini logs.json on disk.
        let serialized = serde_json::to_string_pretty(&events).unwrap();
        std::fs::write(tmp_json.path(), serialized).unwrap();

        let mut state = SessionStateMap::new();
        scan_file(tmp_db.path(), SourceKind::Gemini, tmp_json.path(), &mut state).unwrap();

        let conn = Connection::open(tmp_db.path()).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM execution_logs \
                  WHERE dispatch_kind='passive_observation' \
                    AND runtime='gemini'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "exactly one pair from a two-element user→model array");

        let (prompt, response, model, ti, to): (String, String, String, i64, i64) = conn
            .query_row(
                "SELECT prompt, response, model, tokens_in, tokens_out \
                   FROM execution_logs WHERE runtime='gemini'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(prompt, "what's the weather?");
        assert_eq!(response, "I don't have realtime data, but…");
        assert_eq!(model, "gemini-2.5-flash");
        assert_eq!(ti, 6);
        assert_eq!(to, 11);

        // Re-review-2 (claude) INFO-8: drive the re-scan through
        // the parser path (NOT the outer size==offset short-circuit
        // that fires when watcher_state.byte_offset still equals
        // file size). Force a size change by rewriting the file
        // with the SAME logical events but slightly different
        // serialization (extra whitespace). The UNIQUE INDEX on
        // (provider_session_id, sequence_within_session) is now
        // the active dedup signal, which is what we claim to test.
        let reserialized = serde_json::to_string(&events).unwrap()
            + "                "; // trailing whitespace shifts file size
        std::fs::write(tmp_json.path(), reserialized).unwrap();
        scan_file(tmp_db.path(), SourceKind::Gemini, tmp_json.path(), &mut state).unwrap();
        let n2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM execution_logs \
                  WHERE dispatch_kind='passive_observation' \
                    AND runtime='gemini'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n2, 1, "re-scan does not duplicate — UNIQUE INDEX exercised");
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
