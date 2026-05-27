// v2.13 — Tauri commands feeding the Observability → PassiveFeed UI.
//
// The auto-started watcher in `passive_observer.rs` writes rows into
// `execution_logs` with `dispatch_kind='passive_observation'`. This
// module exposes two pull-mode commands the React panel polls:
//
//   * `list_passive_observations` — recent observed pairs.
//   * `get_observer_status` — whether the watcher is alive and which
//     CLI source directories it's tracking.
//
// We intentionally avoid Tauri events / streaming. The desktop's
// other live surfaces (LiveRuns at 2s, Insights at 5s) all poll —
// keeping the pattern consistent avoids parallel state-sync
// machinery for one panel.

use rusqlite::Connection;
use serde::Serialize;

use crate::get_db_path;
use crate::passive_observer::PassiveObserverState;

#[derive(Debug, Serialize)]
pub struct PassiveObservation {
    pub id: String,
    pub runtime: String,
    pub model: Option<String>,
    pub prompt: Option<String>,
    pub response: Option<String>,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub cost_usd_estimated: Option<f64>,
    pub billing_surface: Option<String>,
    pub provider_session_id: Option<String>,
    pub sequence_within_session: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ObserverStatus {
    pub running: bool,
    /// Identifiers of the CLI source roots the running watcher is
    /// tracking. When the CLI started the daemon, this comes from the
    /// pidfile's recorded `--runtime` set (the actual filter applied
    /// at start). When the desktop's auto-start owns the watcher, it
    /// falls back to the install-set on disk. Per coordinator
    /// MEDIUM-2: never report a runtime that isn't actually being
    /// watched.
    pub sources: Vec<String>,
}

#[tauri::command]
pub fn list_passive_observations(
    limit: Option<i64>,
    runtime: Option<String>,
) -> Result<Vec<PassiveObservation>, String> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let safe_limit = limit.unwrap_or(100).clamp(1, 5_000);
    let runtime_filter = runtime.as_deref();

    let mut where_parts: Vec<String> =
        vec!["dispatch_kind = 'passive_observation'".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(r) = runtime_filter {
        where_parts.push("runtime = ?".to_string());
        params.push(Box::new(r.to_string()));
    }

    let sql = format!(
        "SELECT id, runtime, model, prompt, response, tokens_in, tokens_out, \
                cost_usd_estimated, billing_surface, provider_session_id, \
                sequence_within_session, created_at \
           FROM execution_logs \
          WHERE {} \
          ORDER BY created_at DESC \
          LIMIT ?",
        where_parts.join(" AND ")
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    params.push(Box::new(safe_limit));
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(param_refs.iter()), |r| {
            Ok(PassiveObservation {
                id: r.get(0)?,
                runtime: r.get(1)?,
                model: r.get(2)?,
                prompt: r.get(3)?,
                response: r.get(4)?,
                tokens_in: r.get(5)?,
                tokens_out: r.get(6)?,
                cost_usd_estimated: r.get(7)?,
                billing_surface: r.get(8)?,
                provider_session_id: r.get(9)?,
                sequence_within_session: r.get(10)?,
                created_at: r.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out: Vec<PassiveObservation> = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn get_observer_status(
    state: tauri::State<'_, PassiveObserverState>,
) -> Result<ObserverStatus, String> {
    let observer = state.0.lock().map_err(|_| "observer mutex poisoned".to_string())?;

    // Coordinator MEDIUM-2: report the watcher's ACTUAL source set,
    // not "installed CLIs now" — those drift when the user installs
    // a new CLI after the daemon started. Prefer the pidfile (CLI
    // path); fall back to "what the desktop watcher would have
    // discovered at boot" by intersecting the install-set with
    // what exists on disk now (the desktop's `start` is also
    // install-set-bounded today).
    if let Some(rec) = read_observe_pidfile() {
        if !rec.runtimes.is_empty() {
            return Ok(ObserverStatus {
                running: observer.is_started(),
                sources: rec.runtimes,
            });
        }
    }

    let mut sources: Vec<String> = Vec::new();
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Ok(ObserverStatus { running: false, sources }),
    };
    if home.join(".claude").join("projects").exists() {
        sources.push("claude_code".to_string());
    }
    if home.join(".codex").join("sessions").exists() {
        sources.push("codex".to_string());
    }
    if home.join(".gemini").exists() {
        sources.push("gemini".to_string());
    }
    Ok(ObserverStatus {
        running: observer.is_started(),
        sources,
    })
}

#[derive(serde::Deserialize)]
struct PidfileRecord {
    #[serde(default)]
    runtimes: Vec<String>,
}

/// Read the CLI watcher's pidfile (if present). The CLI surface
/// (apps/cli/src/commands/observe.rs) writes JSON pidfiles with the
/// runtime filter. Desktop-only auto-started watchers don't write
/// one, so this returns None in the common GUI-only case.
fn read_observe_pidfile() -> Option<PidfileRecord> {
    let home = dirs::home_dir()?;
    let path = home.join(".ato").join("observe.pid");
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<PidfileRecord>(raw.trim()).ok()
}
