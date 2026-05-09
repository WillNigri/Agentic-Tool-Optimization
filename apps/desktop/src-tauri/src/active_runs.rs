// v2.1.0 Phase 4 — Active runs registry.
//
// Tracks every in-flight agent dispatch in a process-wide map so the
// UI can answer "which runtime is in which workspace right now" and
// "kill this run without reading the terminal buffer." Twitter ask
// (Timur Yessenov, 2026-05-08): "the missing ops layer."
//
// Why in-memory:
//   - Active runs are an inherently runtime-only concept; persisting
//     them across desktop restarts would just leak stale entries.
//   - The dispatch path already touches this state on every start/end,
//     so a Mutex<HashMap> is faster + simpler than re-reading SQLite.
//   - When the desktop restarts, the dispatch processes die with it,
//     so the registry being lost matches reality.
//
// Concurrency: a single Mutex around the map. Insert + remove + scan
// happen at single-dispatch frequency (handful per minute peak), so
// lock contention is irrelevant. If we ever need finer granularity,
// switch to DashMap.
//
// Cancellation: keeping the spawned process handles inside the
// registry is what makes kill possible. The `child` field is
// optional because not every dispatch path has a kill-able process
// today (subagent group dispatches, MCP-routed runs, etc), and we'd
// rather track "started but unkillable" than not track at all.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ActiveRun {
    pub run_id: String,
    pub agent_slug: Option<String>,
    pub runtime: String,
    pub workspace: Option<String>,
    pub started_at_unix: u64,
    /// "running" | "killing" | "done" — done rows are tombstones the
    /// UI can show briefly before they age out (5s).
    pub status: String,
    /// Optional human-readable summary. e.g. "PromptBar / Quick test"
    pub source: Option<String>,
}

/// Generic kill closure. Different dispatch paths supply different
/// kill mechanisms — std::process::Child::kill for blocking commands,
/// a tokio::spawn that awaits Child::kill for async tokio Child,
/// portable_pty::Child::kill for PTYs. The registry doesn't care; it
/// just calls the closure when the user clicks Kill.
type KillFn = Box<dyn FnOnce() + Send + 'static>;

struct Slot {
    info: ActiveRun,
    kill_fn: Option<KillFn>,
}

struct Registry {
    inner: Mutex<HashMap<String, Slot>>,
}

impl Registry {
    fn new() -> Self {
        Self { inner: Mutex::new(HashMap::new()) }
    }
}

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Register a new in-flight dispatch. Returns the run_id so the
/// caller can pass it to `finish_run` on completion. Run IDs are
/// random UUIDs — caller never has to allocate them.
pub fn begin_run(
    runtime: &str,
    agent_slug: Option<&str>,
    workspace: Option<&str>,
    source: Option<&str>,
) -> String {
    let run_id = uuid::Uuid::new_v4().to_string();
    let info = ActiveRun {
        run_id: run_id.clone(),
        agent_slug: agent_slug.map(|s| s.to_string()),
        runtime: runtime.to_string(),
        workspace: workspace.map(|s| s.to_string()),
        started_at_unix: now_unix(),
        status: "running".to_string(),
        source: source.map(|s| s.to_string()),
    };
    if let Ok(mut map) = registry().inner.lock() {
        map.insert(run_id.clone(), Slot { info, kill_fn: None });
    }
    run_id
}

/// Attach a kill closure so this run becomes terminable. The closure
/// consumes ownership of whatever the dispatch path needs to kill its
/// process (typically an `Arc<Mutex<Option<Child>>>` that's `take()`d
/// inside the closure). Optional — runs without an attached handler
/// stay visible but show "kill not supported" in the UI.
pub fn attach_kill_handler(run_id: &str, f: impl FnOnce() + Send + 'static) {
    if let Ok(mut map) = registry().inner.lock() {
        if let Some(slot) = map.get_mut(run_id) {
            slot.kill_fn = Some(Box::new(f));
        }
    }
}

/// Mark a run finished. Removes from the map after a short grace so
/// the UI can briefly show the tombstone (~immediate enough that
/// users see consistency, slow enough not to flicker).
pub fn finish_run(run_id: &str) {
    if let Ok(mut map) = registry().inner.lock() {
        map.remove(run_id);
    }
}

/// Snapshot of every active run. Cheap — clones the small
/// `ActiveRun` records, never the child handles.
pub fn list_runs() -> Vec<ActiveRun> {
    let mut runs: Vec<ActiveRun> = registry()
        .inner
        .lock()
        .ok()
        .map(|map| map.values().map(|s| s.info.clone()).collect())
        .unwrap_or_default();
    runs.sort_by(|a, b| b.started_at_unix.cmp(&a.started_at_unix));
    runs
}

/// Invoke the kill handler attached to this run, if any. Returns
/// false when the run is unknown OR has no kill handler attached
/// (UI shows the run as "kill not supported" in that case). Always
/// marks status='killing' on a known run so the user's intent is
/// reflected even when we can't actually terminate.
pub fn kill_run(run_id: &str) -> bool {
    let kill_fn = {
        let mut map = match registry().inner.lock() {
            Ok(m) => m,
            Err(_) => return false,
        };
        let slot = match map.get_mut(run_id) {
            Some(s) => s,
            None => return false,
        };
        slot.info.status = "killing".to_string();
        slot.kill_fn.take()
    };
    match kill_fn {
        Some(f) => {
            f();
            true
        }
        None => false,
    }
}

// ─── Tauri commands ─────────────────────────────────────────────────

#[tauri::command]
pub fn list_active_runs() -> Vec<ActiveRun> {
    list_runs()
}

#[tauri::command]
pub fn kill_active_run(run_id: String) -> Result<bool, String> {
    Ok(kill_run(&run_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_then_finish_clears_registry() {
        let id = begin_run("claude", Some("reviewer"), Some("/tmp/x"), Some("test"));
        let runs = list_runs();
        assert!(runs.iter().any(|r| r.run_id == id && r.status == "running"));
        finish_run(&id);
        let runs = list_runs();
        assert!(!runs.iter().any(|r| r.run_id == id));
    }

    #[test]
    fn kill_unknown_returns_false() {
        assert!(!kill_run("nonexistent-id"));
    }

    #[test]
    fn kill_without_handler_returns_false_but_marks_killing() {
        let id = begin_run("codex", None, None, None);
        assert!(!kill_run(&id));
        // status updates even when there's no handler so the UI can
        // reflect the user's intent visually.
        let runs = list_runs();
        let found = runs.iter().find(|r| r.run_id == id).unwrap();
        assert_eq!(found.status, "killing");
        finish_run(&id);
    }

    #[test]
    fn kill_with_handler_invokes_closure() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();
        let id = begin_run("claude", None, None, None);
        attach_kill_handler(&id, move || {
            called_clone.store(true, Ordering::SeqCst);
        });
        assert!(kill_run(&id));
        assert!(called.load(Ordering::SeqCst));
        finish_run(&id);
    }
}
