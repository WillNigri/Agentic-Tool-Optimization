// v2.1.0 — File attribution for agent dispatches.
//
// Snapshots file mtimes within a project root before/after a dispatch
// so the result of `prompt_agent` can be tagged with the list of
// files the agent touched. The whole point: in a multi-agent run
// (sequential pipeline like writer → reviewer, or routed group),
// answer "which agent wrote which files" without git-blame detective
// work.
//
// Why mtime-based (vs parsing the runtime's tool-use stream):
//   - Works for EVERY runtime (Claude Code, Codex, Gemini CLI,
//     OpenClaw SSH, Hermes) with one impl. Stream parsing would need
//     a per-runtime adapter.
//   - Catches anything the agent did, not just things the runtime
//     happens to surface as Tool events (shell-out edits, child
//     process writes, etc).
//   - Cheap. No subprocess interception.
//
// Tradeoffs:
//   - Concurrent dispatches in the same project root attribute their
//     touches by time-window overlap, which is approximate. We accept
//     this; multi-runtime concurrent dispatch is the v2.2 fix.
//   - Background tools (file watchers, dev servers) that happen to
//     write during the dispatch window get attributed too. Mitigated
//     by `IGNORED_DIRS`.
//   - Doesn't catch deletes (mtime requires existence). Future work:
//     also track the set of paths that EXISTED before vs after.

use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

/// Directories never scanned. Build artifacts, VCS metadata, deps —
/// these change frequently and aren't what users mean by "the agent
/// touched a file."
const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".vite",
    ".turbo",
    "vendor",
    "__pycache__",
    ".pytest_cache",
    ".venv",
    "venv",
    ".mypy_cache",
    ".cargo",
    ".rustup",
    ".idea",
    ".vscode",
    "coverage",
    ".cache",
];

/// Cap on files scanned. Beyond this, attribution is best-effort
/// (we still scan, just stop tracking). 50k covers typical
/// monorepos; the cap exists to keep snapshot under ~500ms.
const MAX_FILES: usize = 50_000;

/// (relative_path, mtime_unix_secs)
pub type FileSnapshot = HashMap<String, u64>;

/// Walk `root` and capture every file's (relative_path, mtime).
/// Returns an empty map for non-existent / inaccessible roots so the
/// caller's diff is a clean no-op rather than an error.
pub fn snapshot_files(root: &Path) -> FileSnapshot {
    let mut snap = FileSnapshot::new();
    if !root.exists() || !root.is_dir() {
        return snap;
    }
    walk(root, root, &mut snap);
    snap
}

/// Returns paths whose mtime advanced (or that didn't exist before).
/// Sorted alphabetically for deterministic output.
pub fn diff_snapshots(before: &FileSnapshot, after: &FileSnapshot) -> Vec<String> {
    let mut touched: Vec<String> = after
        .iter()
        .filter_map(|(path, mtime_after)| {
            match before.get(path) {
                Some(mtime_before) if mtime_after > mtime_before => Some(path.clone()),
                None => Some(path.clone()),
                _ => None,
            }
        })
        .collect();
    touched.sort();
    touched
}

fn walk(root: &Path, dir: &Path, snap: &mut FileSnapshot) {
    if snap.len() >= MAX_FILES {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if snap.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        // Hidden files at top of an ignored list are dropped, but
        // we DO follow regular dot-dirs (e.g. .github, .claude) so
        // edits to skill / agent / workflow files are attributed.
        if IGNORED_DIRS.iter().any(|d| *d == name) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.is_dir() {
            walk(root, &path, snap);
        } else if metadata.is_file() {
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if let Ok(rel) = path.strip_prefix(root) {
                if let Some(rel_str) = rel.to_str() {
                    snap.insert(rel_str.to_string(), mtime);
                }
            }
        }
    }
}

// Tauri command wrappers — exposed so the desktop frontend can take
// snapshots around an arbitrary dispatch path it controls (Quick
// Test, MCP run_agent, scheduled cron, etc).

#[tauri::command]
pub fn snapshot_project_files(root: String) -> Result<FileSnapshot, String> {
    let path = Path::new(&root);
    Ok(snapshot_files(path))
}

#[tauri::command]
pub fn diff_project_files(
    root: String,
    prior: FileSnapshot,
) -> Result<Vec<String>, String> {
    let path = Path::new(&root);
    let after = snapshot_files(path);
    Ok(diff_snapshots(&prior, &after))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn diff_detects_new_files() {
        let dir = tempfile::tempdir().unwrap();
        let before = snapshot_files(dir.path());
        fs::write(dir.path().join("new.txt"), "hi").unwrap();
        let after = snapshot_files(dir.path());
        assert_eq!(diff_snapshots(&before, &after), vec!["new.txt"]);
    }

    #[test]
    fn diff_detects_modified_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "v1").unwrap();
        let before = snapshot_files(dir.path());
        // mtime resolution can be 1s on some filesystems; sleep so
        // the modification registers as a newer mtime.
        thread::sleep(Duration::from_millis(1100));
        fs::write(dir.path().join("a.txt"), "v2").unwrap();
        let after = snapshot_files(dir.path());
        assert_eq!(diff_snapshots(&before, &after), vec!["a.txt"]);
    }

    #[test]
    fn ignored_dirs_skipped() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        fs::write(dir.path().join("node_modules/x.js"), "x").unwrap();
        fs::write(dir.path().join("real.ts"), "real").unwrap();
        let snap = snapshot_files(dir.path());
        assert!(snap.contains_key("real.ts"));
        assert!(!snap.contains_key("node_modules/x.js"));
    }

    #[test]
    fn empty_for_missing_root() {
        let snap = snapshot_files(Path::new("/nonexistent/path/should/not/exist"));
        assert!(snap.is_empty());
    }
}
