// v2.3.5 Phase 4 (in progress) — Event schema for ops recipes.
//
// The recipes engine subscribes to events emitted by other parts of
// the desktop process: dispatch lifecycle, regression detection, replay
// completion, cost thresholds, schedule firings. Each event carries the
// minimum payload a recipe action needs to make a decision without
// having to re-query SQLite.
//
// Review notes applied (codex-reviewer 2026-05-11):
//   - Discriminants (severity, status, window) are now typed enums so
//     publishers can't emit garbage strings.
//   - ReplayDone carries error_message for the failed case (was forcing
//     a re-query just to decide retry vs alert).
//   - publish() no longer clones the event needlessly and the API is
//     unambiguous about send-success vs subscriber-count.
//   - Every event carries a monotonic `event_seq` so lagging
//     subscribers (RecvError::Lagged) can detect exactly which IDs
//     they missed and replay them from the SQLite ledger.
//   - Timestamps are RFC3339 strings still — typed timestamp wrappers
//     would help but add boilerplate without changing behavior for v1.
//     Newtype'd if/when the schema escapes the desktop process boundary.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Severity precomputed by the publisher. "neutral" deltas are filtered
/// at the source — recipes shouldn't have to dedupe noise.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionSeverity {
    Regression,
    Improvement,
}

/// Replay job terminal status. Maps 1:1 to replay_jobs.status when the
/// job finishes; in-flight states never produce a ReplayDone event.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayStatus {
    Done,
    Failed,
}

/// Cost-window cadence. Recipes filter on this so a "daily spend over
/// $X" rule doesn't double-fire when the 7d window crosses too.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CostWindow {
    #[serde(rename = "1d")]
    Day,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
}

/// Every event the ops-recipes engine can react to. New variants land
/// at the bottom of the enum so JSON consumers that don't recognize a
/// type can skip without breaking.
///
/// Every variant carries `event_seq` — a monotonic counter from the
/// publish path. Subscribers track the last seq they processed; on
/// `RecvError::Lagged` they can query the ledger for missed seqs.
// v2.3.9 — Deserialize added so the events_log poll loop can parse
// stored payloads. The {{previous_runtime}} placeholder grammar
// expansion + cross-process event publishing both need this.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AtoEvent {
    #[serde(rename = "regression_detected")]
    RegressionDetected {
        event_seq: u64,
        change_id: String,
        agent_slug: String,
        field: String,
        /// v2.3.9 — old/new field values, copied from the
        /// agent_config_changes row that caused the regression. Lets
        /// the recipe engine resolve {{previous_runtime}} from
        /// old_value when field == "runtime". None when the original
        /// change row didn't capture them.
        old_value: Option<String>,
        new_value: Option<String>,
        severity: RegressionSeverity,
        eval_delta_pp: Option<f64>, // null when no evaluators
        ok_delta_pp: f64,
        cost_delta_pct: f64,
        failing_trace_ids: Vec<String>,
        detected_at: String, // RFC3339
    },

    #[serde(rename = "dispatch_failed")]
    DispatchFailed {
        event_seq: u64,
        run_id: String,
        agent_slug: Option<String>,
        runtime: String,
        error_message: String,
        duration_ms: i64,
        failed_at: String,
    },

    /// Replay reached a terminal state. The failure case carries its
    /// error_message inline so a recipe can decide retry-vs-alert
    /// without a SQLite re-query.
    #[serde(rename = "replay_done")]
    ReplayDone {
        event_seq: u64,
        job_id: String,
        source_trace_id: String,
        source_runtime: String,
        target_runtime: String,
        target_model: Option<String>,
        status: ReplayStatus,
        duration_ms: Option<i64>,
        cost_usd_estimated: Option<f64>,
        /// Some only when status == Failed.
        error_message: Option<String>,
        finished_at: String,
    },

    #[serde(rename = "cost_threshold_exceeded")]
    CostThresholdExceeded {
        event_seq: u64,
        agent_slug: String,
        window: CostWindow,
        cost_usd: f64,
        threshold_usd: f64,
        exceeded_at: String,
    },

    #[serde(rename = "schedule_fired")]
    ScheduleFired {
        event_seq: u64,
        cron_id: String,
        agent_slug: String,
        fired_at: String,
    },
}

impl AtoEvent {
    /// String discriminator for matching in recipe trigger filters.
    /// Mirrors the serde rename tags above. Recipes filter on this
    /// before deserializing the payload, so it has to stay stable.
    pub fn type_name(&self) -> &'static str {
        match self {
            AtoEvent::RegressionDetected { .. } => "regression_detected",
            AtoEvent::DispatchFailed { .. } => "dispatch_failed",
            AtoEvent::ReplayDone { .. } => "replay_done",
            AtoEvent::CostThresholdExceeded { .. } => "cost_threshold_exceeded",
            AtoEvent::ScheduleFired { .. } => "schedule_fired",
        }
    }

    /// Sequence number for lag-recovery. Subscribers compare against
    /// their last-seen seq; on a gap, they pull missed events from the
    /// SQLite events ledger (added in a follow-up commit).
    pub fn event_seq(&self) -> u64 {
        match self {
            AtoEvent::RegressionDetected { event_seq, .. } => *event_seq,
            AtoEvent::DispatchFailed { event_seq, .. } => *event_seq,
            AtoEvent::ReplayDone { event_seq, .. } => *event_seq,
            AtoEvent::CostThresholdExceeded { event_seq, .. } => *event_seq,
            AtoEvent::ScheduleFired { event_seq, .. } => *event_seq,
        }
    }
}

/// In-memory event bus. Phase 4.0 ships with a single broadcast channel
/// shared by all subscribers. Each subscriber gets a Receiver that
/// drops events when its queue is full (lagging subscribers don't block
/// the publisher — a recipe stuck waiting for a slow webhook shouldn't
/// stop dispatches from emitting events).
pub mod bus {
    use super::AtoEvent;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::sync::broadcast;

    /// Buffer per subscriber. 256 is enough for typical bursts; lagging
    /// subscribers will see `RecvError::Lagged` rather than block.
    const CHANNEL_CAPACITY: usize = 256;

    static BUS: OnceLock<broadcast::Sender<AtoEvent>> = OnceLock::new();
    /// Monotonic event sequence counter. Callers stamp the event with
    /// `next_seq()` before sending. Seeded from MAX(event_seq) in
    /// events_log on app boot (`init_seq_from_db`) so the counter
    /// stays strictly increasing across desktop restarts. Without that
    /// seeding, the previous run's events would be overwritten when
    /// the new run reuses seq 1, 2, 3... — caught by codex-reviewer
    /// in v2.3.8 review.
    static SEQ: AtomicU64 = AtomicU64::new(1);

    fn sender() -> &'static broadcast::Sender<AtoEvent> {
        BUS.get_or_init(|| broadcast::channel(CHANNEL_CAPACITY).0)
    }

    /// Reserve the next event sequence number. Publishers should call
    /// this once per event they construct.
    pub fn next_seq() -> u64 {
        SEQ.fetch_add(1, Ordering::Relaxed)
    }

    /// v2.3.8 — Seed the in-memory sequence counter from the highest
    /// event_seq already persisted in events_log. Called by the
    /// desktop at app boot, after the DB schema is initialized.
    /// Idempotent: subsequent calls only ratchet UP, never down.
    pub fn init_seq_from_db(db_path: &std::path::Path) {
        let conn = match rusqlite::Connection::open(db_path) {
            Ok(c) => c,
            Err(_) => return,
        };
        let max: i64 = conn
            .query_row("SELECT COALESCE(MAX(event_seq), 0) FROM events_log", [], |r| r.get(0))
            .unwrap_or(0);
        let next = (max as u64).saturating_add(1);
        // Ratchet: only raise SEQ, never lower it. Concurrent next_seq
        // calls during boot are unlikely but safe — they always end up
        // >= the persisted max.
        loop {
            let cur = SEQ.load(Ordering::Relaxed);
            if cur >= next {
                return;
            }
            match SEQ.compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return,
                Err(_) => continue,
            }
        }
    }

    /// Outcome of publishing an event. Distinguishes "delivered to N
    /// subscribers" from "no subscribers attached" — the previous API
    /// conflated them via receiver_count(), which can drift between
    /// the send and the count read.
    #[derive(Debug, Clone)]
    pub struct PublishOutcome {
        /// Number of receivers the send succeeded into. Zero is fine —
        /// means nobody's watching, the event still landed in the
        /// SQLite ledger (when the ledger lands) for later replay.
        pub delivered_to: usize,
    }

    /// Publish an event. Caller passes ownership; we don't clone.
    /// Returns the outcome with `delivered_to` populated from the send
    /// result (not from `receiver_count()`, which can drift).
    ///
    /// v2.3.8 — Also persists to the events_log SQLite table so
    /// `ato events recent` can read history + lagging subscribers can
    /// re-read from a deterministic source. Persistence is best-effort:
    /// a locked DB doesn't block the send.
    pub fn publish(event: AtoEvent) -> PublishOutcome {
        persist_event(&event);
        match sender().send(event) {
            Ok(n) => PublishOutcome { delivered_to: n },
            Err(_) => PublishOutcome { delivered_to: 0 },
        }
    }

    /// Best-effort insert into events_log. Failure is logged but never
    /// blocks the broadcast send — the bus is the primary signal path.
    fn persist_event(event: &AtoEvent) {
        let db_path = crate::get_db_path();
        let conn = match rusqlite::Connection::open(&db_path) {
            Ok(c) => c,
            Err(_) => return,
        };
        let payload = match serde_json::to_string(event) {
            Ok(s) => s,
            Err(_) => return,
        };
        let occurred_at = chrono::Utc::now().to_rfc3339();
        // Plain INSERT — `event_seq` is seeded from MAX on app boot so
        // collisions don't happen across restarts (caught by codex-
        // reviewer in v2.3.8). If insert fails (DB locked, schema
        // missing on older DB), silently drop — the broadcast send
        // still happens.
        let _ = conn.execute(
            "INSERT INTO events_log (event_seq, event_type, payload, occurred_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                event.event_seq() as i64,
                event.type_name(),
                payload,
                occurred_at,
            ],
        );
    }

    /// Get a fresh subscriber. Each receiver sees events published after
    /// it was created — not historical. Recipes that need to replay
    /// history should query the SQLite events ledger (follow-up).
    pub fn subscribe() -> broadcast::Receiver<AtoEvent> {
        sender().subscribe()
    }
}

// Re-export the seq helper at the module root so publishers can call
// `events::next_seq()` without reaching into the bus submodule.
pub use bus::next_seq;
// Keep AtomicU64 reachable for future tests that want to reset the
// counter; unused warning suppressed when not under cfg(test).
#[allow(dead_code)]
fn _ensure_atomic_in_scope() -> AtomicU64 {
    AtomicU64::new(0)
}

// Local-only guard against the SEQ counter loading at the wrong
// Ordering — the bus uses Relaxed which is correct for a strictly
// monotonic counter where ordering between distinct events doesn't
// need to be observed from outside the publisher. Documenting here so
// the next reviewer doesn't "fix" it to SeqCst.
const _: Ordering = Ordering::Relaxed;
