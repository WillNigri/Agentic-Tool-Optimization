// n=30 real-data validation for the Claude Code parser.
//
// Hypothesis (locked BEFORE running the test, per development-
// discipline rule #5):
//
//   Running our parser against 30 real Claude Code session JSONL
//   files from ~/.claude/projects/ will extract ≥95% of the
//   (user→assistant) prompt-response pairs that a strict jq-shaped
//   ground-truth pass identifies, with ≤5% false positives (e.g.
//   tool-result turns mistakenly counted as user prompts).
//
// Method:
//   1. Enumerate Claude Code JSONL files under ~/.claude/projects/.
//   2. Pick the first 30 with size > 1 KB (skip the empties).
//   3. For each file, compute ground truth: count `assistant` lines
//      whose `message.content` contains at least one `text` part AND
//      which are preceded by a non-meta `user` line that itself has
//      typed-text content (no tool_result-only turn).
//   4. Run our `scan_file` against a fresh temp SQLite and count the
//      rows inserted with `dispatch_kind='passive_observation'` and
//      `runtime='claude'`.
//   5. Report per-file detection rate, total recall, and any false
//      positives.
//
// Pass criteria (matches the hypothesis):
//   - aggregate recall ≥ 0.95
//   - false-positive ratio ≤ 0.05
//
// The test is `#[ignore]` by default so unrelated CI runs don't
// require ~/.claude/projects/ to exist. Trigger with:
//   `cargo test --package ato-passive-observer --test n30_claude_real -- --ignored --nocapture`

use std::path::PathBuf;

use ato_passive_observer::sources::SourceKind;
use ato_passive_observer::worker::{scan_file, SessionStateMap};
use rusqlite::Connection;
use serde_json::Value;

const TARGET_N: usize = 30;

fn home() -> PathBuf {
    dirs::home_dir().expect("home dir")
}

fn enumerate_claude_jsonls() -> Vec<PathBuf> {
    let root = home().join(".claude").join("projects");
    let mut out = Vec::new();
    if !root.exists() {
        return out;
    }
    visit(&root, &mut out);
    // Smallest-first so the test is fast on a real machine. Still
    // gives us 30 distinct conversations.
    out.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
    out.into_iter()
        .filter(|p| {
            std::fs::metadata(p)
                .map(|m| m.len() > 1024 && m.len() < 5_000_000)
                .unwrap_or(false)
        })
        .take(TARGET_N)
        .collect()
}

fn visit(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(ft) = entry.file_type() {
            if ft.is_dir() {
                visit(&path, out);
            } else if ft.is_file()
                && path.extension().and_then(|s| s.to_str()) == Some("jsonl")
            {
                out.push(path);
            }
        }
    }
}

/// Ground-truth count: number of (user→assistant) pairs where:
///   - user is non-meta and has typed text (string content OR a text
///     part array without any tool_result)
///   - the following assistant turn has at least one text part
fn ground_truth_pairs(path: &std::path::Path) -> usize {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let mut pending_user = false;
    let mut pairs = 0usize;
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        match ty {
            "user" => {
                if v.get("isMeta").and_then(|x| x.as_bool()).unwrap_or(false) {
                    continue;
                }
                let message = match v.get("message") {
                    Some(m) => m,
                    None => continue,
                };
                if message.get("role").and_then(|r| r.as_str()) != Some("user") {
                    continue;
                }
                let has_typed_text = match message.get("content") {
                    Some(Value::String(s)) => !s.is_empty(),
                    Some(Value::Array(parts)) => {
                        let has_tool_result = parts.iter().any(|p| {
                            p.get("type").and_then(|t| t.as_str()) == Some("tool_result")
                        });
                        if has_tool_result {
                            false
                        } else {
                            parts.iter().any(|p| {
                                p.get("type").and_then(|t| t.as_str()) == Some("text")
                                    && p.get("text")
                                        .and_then(|t| t.as_str())
                                        .map(|s| !s.is_empty())
                                        .unwrap_or(false)
                            })
                        }
                    }
                    _ => false,
                };
                if has_typed_text {
                    pending_user = true;
                }
            }
            "assistant" => {
                if !pending_user {
                    continue;
                }
                let message = match v.get("message") {
                    Some(m) => m,
                    None => continue,
                };
                let has_text = match message.get("content") {
                    Some(Value::Array(parts)) => parts.iter().any(|p| {
                        p.get("type").and_then(|t| t.as_str()) == Some("text")
                            && p.get("text")
                                .and_then(|t| t.as_str())
                                .map(|s| !s.is_empty())
                                .unwrap_or(false)
                    }),
                    _ => false,
                };
                if has_text {
                    pairs += 1;
                    pending_user = false;
                }
            }
            _ => {}
        }
    }
    pairs
}

fn init_test_schema(conn: &Connection) {
    // Slimmed-down version of the desktop's `init_database` covering
    // only the tables/columns the passive observer writes.
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS execution_logs (
            id TEXT PRIMARY KEY,
            runtime TEXT NOT NULL,
            prompt TEXT,
            response TEXT,
            tokens_in INTEGER,
            tokens_out INTEGER,
            duration_ms INTEGER,
            status TEXT,
            error_message TEXT,
            skill_name TEXT,
            cloud_trace_id TEXT,
            created_at TEXT,
            cost_usd_estimated REAL,
            agent_slug TEXT,
            model TEXT,
            auth_mode TEXT,
            dispatch_kind TEXT NOT NULL DEFAULT 'active',
            billing_surface TEXT,
            provider_session_id TEXT,
            sequence_within_session INTEGER
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_logs_session_seq
            ON execution_logs(provider_session_id, sequence_within_session)
            WHERE provider_session_id IS NOT NULL;
        CREATE TABLE IF NOT EXISTS watcher_state (
            source TEXT NOT NULL,
            file_path TEXT NOT NULL,
            byte_offset INTEGER NOT NULL DEFAULT 0,
            last_seq INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (source, file_path)
        );
        CREATE TABLE IF NOT EXISTS live_runs (
            run_id TEXT PRIMARY KEY,
            agent_slug TEXT,
            runtime TEXT,
            workspace TEXT,
            source TEXT,
            started_at TEXT,
            status TEXT,
            child_pid INTEGER,
            dispatch_kind TEXT NOT NULL DEFAULT 'active',
            billing_surface TEXT
        );
        "#,
    )
    .expect("init schema");
}

#[test]
#[ignore]
fn n30_real_claude_corpus() {
    let files = enumerate_claude_jsonls();
    if files.is_empty() {
        eprintln!("SKIP: no Claude Code JSONLs under ~/.claude/projects/");
        return;
    }
    eprintln!(
        "n=30 real-data validation against {} Claude Code session files",
        files.len()
    );

    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    let db_path = tmp.path().to_path_buf();
    {
        let conn = Connection::open(&db_path).expect("open temp db");
        init_test_schema(&conn);
    }

    let mut total_ground_truth = 0usize;
    let mut per_file: Vec<(PathBuf, usize, usize)> = Vec::new();

    for file in &files {
        let gt = ground_truth_pairs(file);
        total_ground_truth += gt;

        // Each file ingests under its own provider_session_id (the
        // sessionId on the JSONL lines), so the UNIQUE INDEX prevents
        // cross-file contamination naturally. We use a fresh DB per
        // run because state across files isn't what we're testing.
        let mut state = SessionStateMap::new();
        scan_file(&db_path, SourceKind::ClaudeCode, file, &mut state)
            .expect("scan_file");

        let conn = Connection::open(&db_path).expect("open db");
        // Count rows for THIS session (provider_session_id derived
        // from the file's first sessionId field).
        let session_id = extract_first_session_id(file).unwrap_or_default();
        let observed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM execution_logs \
                 WHERE dispatch_kind = 'passive_observation' \
                   AND provider_session_id = ?1",
                rusqlite::params![session_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        per_file.push((file.clone(), gt, observed as usize));
    }

    let total_observed: usize = per_file.iter().map(|(_, _, o)| *o).sum();
    let recall = if total_ground_truth == 0 {
        1.0
    } else {
        total_observed as f64 / total_ground_truth as f64
    };
    let false_positives: usize = per_file
        .iter()
        .map(|(_, gt, o)| o.saturating_sub(*gt))
        .sum();
    let fp_ratio = if total_observed == 0 {
        0.0
    } else {
        false_positives as f64 / total_observed as f64
    };

    eprintln!("\n=== n=30 results ===");
    for (path, gt, o) in &per_file {
        let mark = if o >= gt && (o - gt) <= (gt / 20 + 1) {
            "OK"
        } else {
            "MISS"
        };
        eprintln!(
            "  [{}] gt={:4}  observed={:4}  file={}",
            mark,
            gt,
            o,
            path.file_name().and_then(|n| n.to_str()).unwrap_or("?")
        );
    }
    eprintln!(
        "\nTOTAL  ground_truth={}  observed={}  recall={:.3}  fp_ratio={:.3}",
        total_ground_truth, total_observed, recall, fp_ratio
    );
    eprintln!(
        "Hypothesis pass criteria: recall >= 0.95 AND fp_ratio <= 0.05"
    );

    assert!(
        recall >= 0.95,
        "recall {:.3} below the 0.95 hypothesis threshold",
        recall
    );
    assert!(
        fp_ratio <= 0.05,
        "false-positive ratio {:.3} above the 0.05 hypothesis threshold",
        fp_ratio
    );
}

/// Multi-turn stress test. Picks the 30 LARGEST sessions in the
/// corpus (instead of smallest-first) so each file has many user→
/// assistant turns. This catches per-turn dedup + sequence drift
/// that single-turn files would never exercise.
#[test]
#[ignore]
fn n30_real_claude_multi_turn() {
    let root = home().join(".claude").join("projects");
    if !root.exists() {
        eprintln!("SKIP: no Claude Code JSONLs under ~/.claude/projects/");
        return;
    }
    let mut all = Vec::new();
    visit(&root, &mut all);
    all.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)));
    let files: Vec<PathBuf> = all
        .into_iter()
        .filter(|p| {
            std::fs::metadata(p)
                .map(|m| m.len() > 50_000 && m.len() < 5_000_000)
                .unwrap_or(false)
        })
        .take(TARGET_N)
        .collect();
    if files.is_empty() {
        eprintln!("SKIP: no multi-turn Claude Code sessions found");
        return;
    }
    eprintln!(
        "n=30 multi-turn validation against {} large Claude Code session files",
        files.len()
    );

    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    let db_path = tmp.path().to_path_buf();
    {
        let conn = Connection::open(&db_path).expect("open temp db");
        init_test_schema(&conn);
    }

    let mut total_ground_truth = 0usize;
    let mut per_file: Vec<(PathBuf, usize, usize)> = Vec::new();
    for file in &files {
        let gt = ground_truth_pairs(file);
        total_ground_truth += gt;
        let mut state = SessionStateMap::new();
        scan_file(&db_path, SourceKind::ClaudeCode, file, &mut state).expect("scan_file");
        let conn = Connection::open(&db_path).expect("open db");
        let session_id = extract_first_session_id(file).unwrap_or_default();
        let observed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM execution_logs \
                 WHERE dispatch_kind = 'passive_observation' \
                   AND provider_session_id = ?1",
                rusqlite::params![session_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        per_file.push((file.clone(), gt, observed as usize));
    }

    let total_observed: usize = per_file.iter().map(|(_, _, o)| *o).sum();
    let recall = if total_ground_truth == 0 {
        1.0
    } else {
        total_observed as f64 / total_ground_truth as f64
    };
    let false_positives: usize = per_file
        .iter()
        .map(|(_, gt, o)| o.saturating_sub(*gt))
        .sum();
    let fp_ratio = if total_observed == 0 {
        0.0
    } else {
        false_positives as f64 / total_observed as f64
    };
    eprintln!("\n=== n=30 multi-turn results ===");
    for (path, gt, o) in &per_file {
        let drift = (*o as i64) - (*gt as i64);
        eprintln!(
            "  gt={:4}  observed={:4}  drift={:+4}  file={}",
            gt,
            o,
            drift,
            path.file_name().and_then(|n| n.to_str()).unwrap_or("?")
        );
    }
    eprintln!(
        "\nTOTAL  ground_truth={}  observed={}  recall={:.3}  fp_ratio={:.3}",
        total_ground_truth, total_observed, recall, fp_ratio
    );
    assert!(recall >= 0.95, "recall {:.3} below 0.95", recall);
    assert!(fp_ratio <= 0.05, "fp_ratio {:.3} above 0.05", fp_ratio);
}

fn extract_first_session_id(path: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(s) = v.get("sessionId").and_then(|x| x.as_str()) {
                return Some(s.to_string());
            }
        }
    }
    None
}
