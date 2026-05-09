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

/// v2.1.0+ Concurrent attribution refinement — minimal evidence the
/// dispatch path can attach to a finished trace so the dashboard can
/// show "this run overlapped with another." Tells the truth instead
/// of pretending concurrent dispatches don't exist.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OverlapEvidence {
    /// Other runs in the same workspace that were active at any point
    /// during this run's window. Each has slug + start time so the UI
    /// can render `@reviewer (started 14:32:08)` next to ambiguous
    /// file attributions.
    pub overlapped_with: Vec<OverlapPeer>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OverlapPeer {
    pub run_id: String,
    pub agent_slug: Option<String>,
    pub runtime: String,
    pub started_at_unix: u64,
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
    /// v2.1.0+ Concurrent attribution — peers seen overlapping at any
    /// point. Populated by begin_run when a new run starts in the
    /// same workspace as an existing one (mutual write so neither
    /// has to walk the history later). Reset by finish_run.
    overlapped: Vec<OverlapPeer>,
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
        // v2.1.0+ concurrent attribution — when this run shares a
        // workspace with any other in-flight run, record the overlap
        // mutually. Each side's trace will end up tagged with the
        // other so the dashboard can flag attribution ambiguity.
        let mut my_overlaps: Vec<OverlapPeer> = Vec::new();
        if let Some(my_ws) = info.workspace.as_deref() {
            // Two-pass to satisfy the borrow checker: first collect
            // matching peer IDs, then mutate each in turn.
            let peer_ids: Vec<String> = map
                .iter()
                .filter(|(_, s)| s.info.workspace.as_deref() == Some(my_ws))
                .map(|(id, _)| id.clone())
                .collect();
            for peer_id in &peer_ids {
                if let Some(other) = map.get_mut(peer_id) {
                    other.overlapped.push(OverlapPeer {
                        run_id: run_id.clone(),
                        agent_slug: info.agent_slug.clone(),
                        runtime: info.runtime.clone(),
                        started_at_unix: info.started_at_unix,
                    });
                    my_overlaps.push(OverlapPeer {
                        run_id: peer_id.clone(),
                        agent_slug: other.info.agent_slug.clone(),
                        runtime: other.info.runtime.clone(),
                        started_at_unix: other.info.started_at_unix,
                    });
                }
            }
        }
        map.insert(
            run_id.clone(),
            Slot { info, kill_fn: None, overlapped: my_overlaps },
        );
    }
    run_id
}

/// v2.1.0+ — Drain the overlap evidence for a run before / instead
/// of `finish_run`. Returns who overlapped this run's window so the
/// trace upload can tag attribution-ambiguous files. Does NOT remove
/// the slot — caller must still call finish_run for cleanup.
pub fn take_overlap_evidence(run_id: &str) -> OverlapEvidence {
    let mut overlapped_with: Vec<OverlapPeer> = Vec::new();
    if let Ok(map) = registry().inner.lock() {
        if let Some(slot) = map.get(run_id) {
            overlapped_with = slot.overlapped.clone();
        }
    }
    OverlapEvidence { overlapped_with }
}

#[tauri::command]
pub fn get_overlap_evidence(run_id: String) -> OverlapEvidence {
    take_overlap_evidence(&run_id)
}

/// v2.1.0+ — Frontend finalizer for runs whose Rust-side dispatch
/// returns a run_id (e.g. prompt_agent_with_context). The frontend
/// calls this after fetching overlap evidence + uploading the trace,
/// to release the registry slot. Idempotent — calling on an unknown
/// run_id is a no-op.
#[tauri::command]
pub fn finish_active_run(run_id: String) {
    finish_run(&run_id);
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

// async (vs `pub fn`) so Tauri runs us inside a tokio runtime
// context. The streaming dispatch's kill closure captures a runtime
// handle at registration time and uses `handle.spawn` regardless,
// but this is belt-and-suspenders: any future kill closure that
// accidentally uses bare `tokio::spawn` won't panic the app from
// here.
#[tauri::command]
pub async fn kill_active_run(run_id: String) -> Result<bool, String> {
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
    fn overlap_recorded_mutually_for_same_workspace() {
        let a = begin_run("claude", Some("writer"), Some("/tmp/repo-x"), None);
        let b = begin_run("codex", Some("reviewer"), Some("/tmp/repo-x"), None);
        let a_evidence = take_overlap_evidence(&a);
        let b_evidence = take_overlap_evidence(&b);
        // A started first, B starts and sees A. B's overlap should
        // contain A; A's overlap should contain B (mutual write).
        assert!(b_evidence.overlapped_with.iter().any(|p| p.run_id == a));
        assert!(a_evidence.overlapped_with.iter().any(|p| p.run_id == b));
        finish_run(&a);
        finish_run(&b);
    }

    #[test]
    fn no_overlap_for_different_workspaces() {
        let a = begin_run("claude", Some("writer"), Some("/tmp/repo-a"), None);
        let b = begin_run("claude", Some("writer"), Some("/tmp/repo-b"), None);
        let a_evidence = take_overlap_evidence(&a);
        let b_evidence = take_overlap_evidence(&b);
        assert!(a_evidence.overlapped_with.is_empty());
        assert!(b_evidence.overlapped_with.is_empty());
        finish_run(&a);
        finish_run(&b);
    }

    #[test]
    fn no_overlap_when_workspace_unknown() {
        // Runs without a workspace shouldn't false-positive against
        // other workspace-less runs (different agents in different
        // ad-hoc dispatches).
        let a = begin_run("claude", None, None, None);
        let b = begin_run("codex", None, None, None);
        let a_evidence = take_overlap_evidence(&a);
        assert!(a_evidence.overlapped_with.is_empty());
        let _ = b;
        finish_run(&a);
        finish_run(&b);
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
