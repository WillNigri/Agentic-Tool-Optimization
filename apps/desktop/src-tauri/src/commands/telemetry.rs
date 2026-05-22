// commands/telemetry.rs — Conversion-event write + funnel-read commands.
//
// Strategy PR-B (2026-05-21). Distinct from the older `crate::telemetry`
// module (opt-in analytics event queue) — this one is the local-only
// willingness-to-pay funnel for paid Pro re-introduction. Rows are
// aggregated in the renderer (one Map<feature, …> per session) and
// flushed here every 60s.
//
// Architecture war-room rulings baked in below:
//   - session_id is a renderer-minted UUID, NOT a FK to sessions.id
//   - tier_at_event + trial_cohort are snapshotted by the caller
//   - rows are local-only; no cloud-forward in this PR
//   - one row per (session, feature, flush) — never UPSERT-by-feature
//     because that would lose first_seen_at across flushes

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::DbState;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionEventInput {
    pub session_id: String,
    pub feature: String,
    pub tier_at_event: String,
    pub trial_cohort: Option<String>,
    pub count: i64,
    pub first_seen_at: String,
    pub last_seen_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionFunnelRow {
    pub feature: String,
    pub tier_at_event: String,
    pub trial_cohort: Option<String>,
    pub total_count: i64,
    pub session_count: i64,
    pub first_seen_at: String,
    pub last_seen_at: String,
}

/// Pure write helper — takes a `&mut Connection` so it can open a
/// transaction. Reused by the Tauri command + tests.
pub fn insert_conversion_events(
    conn: &mut Connection,
    events: &[ConversionEventInput],
    flushed_at: &str,
) -> Result<(), rusqlite::Error> {
    if events.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO conversion_events
             (session_id, feature, tier_at_event, trial_cohort,
              count, first_seen_at, last_seen_at, flushed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for ev in events {
            stmt.execute(params![
                ev.session_id,
                ev.feature,
                ev.tier_at_event,
                ev.trial_cohort,
                ev.count,
                ev.first_seen_at,
                ev.last_seen_at,
                flushed_at,
            ])?;
        }
    }
    tx.commit()
}

/// Pure read helper — same `&Connection` shape, easy to test.
pub fn query_conversion_funnel(
    conn: &Connection,
    since: Option<&str>,
) -> Result<Vec<ConversionFunnelRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT feature,
                tier_at_event,
                trial_cohort,
                SUM(count)                  AS total_count,
                COUNT(DISTINCT session_id)  AS session_count,
                MIN(first_seen_at)          AS first_seen_at,
                MAX(last_seen_at)           AS last_seen_at
           FROM conversion_events
          WHERE (?1 IS NULL OR flushed_at >= ?1)
          GROUP BY feature, tier_at_event, trial_cohort
          ORDER BY total_count DESC",
    )?;
    let rows = stmt.query_map(params![since], |row| {
        Ok(ConversionFunnelRow {
            feature: row.get(0)?,
            tier_at_event: row.get(1)?,
            trial_cohort: row.get(2)?,
            total_count: row.get(3)?,
            session_count: row.get(4)?,
            first_seen_at: row.get(5)?,
            last_seen_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

/// Write one flush batch. Each entry becomes one row in `conversion_events`.
/// The renderer batches per (session_id, feature) inside the 60s window;
/// no further aggregation happens here.
#[tauri::command]
pub fn record_conversion_events(
    db: State<'_, DbState>,
    events: Vec<ConversionEventInput>,
) -> Result<(), String> {
    let mut conn = db.0.lock().map_err(|e| e.to_string())?;
    let flushed_at = chrono::Utc::now().to_rfc3339();
    insert_conversion_events(&mut conn, &events, &flushed_at).map_err(|e| e.to_string())
}

/// Aggregate rollup for the admin `ConversionFunnel` page. Groups by
/// (feature, tier_at_event, trial_cohort). `total_count` answers "how
/// often", `session_count` answers "how many distinct user-sessions"
/// (one boot-session UUID per launch).
#[tauri::command]
pub fn get_conversion_funnel(
    db: State<'_, DbState>,
    since: Option<String>,
) -> Result<Vec<ConversionFunnelRow>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    query_conversion_funnel(&conn, since.as_deref()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::init_database;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_database(&conn);
        conn
    }

    fn ev(
        session: &str,
        feature: &str,
        tier: &str,
        cohort: Option<&str>,
        count: i64,
    ) -> ConversionEventInput {
        ConversionEventInput {
            session_id: session.into(),
            feature: feature.into(),
            tier_at_event: tier.into(),
            trial_cohort: cohort.map(String::from),
            count,
            first_seen_at: "2026-05-21T10:00:00Z".into(),
            last_seen_at: "2026-05-21T10:00:30Z".into(),
        }
    }

    #[test]
    fn empty_batch_is_noop() {
        let mut conn = fresh_db();
        insert_conversion_events(&mut conn, &[], "2026-05-21T10:01:00Z").unwrap();
        let funnel = query_conversion_funnel(&conn, None).unwrap();
        assert!(funnel.is_empty());
    }

    #[test]
    fn funnel_groups_by_feature_tier_cohort() {
        let mut conn = fresh_db();
        insert_conversion_events(
            &mut conn,
            &[
                ev("session-1", "cloud-traces", "pro", Some("A5"), 3),
                ev("session-1", "cloud-traces", "pro", Some("A5"), 5),
                ev("session-2", "cloud-traces", "pro", Some("A5"), 2),
                ev("session-2", "evaluators", "free", None, 1),
            ],
            "2026-05-21T10:01:00Z",
        )
        .unwrap();

        let funnel = query_conversion_funnel(&conn, None).unwrap();
        let cloud = funnel
            .iter()
            .find(|r| r.feature == "cloud-traces")
            .expect("cloud-traces row missing");
        assert_eq!(cloud.total_count, 10);
        assert_eq!(cloud.session_count, 2);
        assert_eq!(cloud.tier_at_event, "pro");
        assert_eq!(cloud.trial_cohort.as_deref(), Some("A5"));

        let evals = funnel
            .iter()
            .find(|r| r.feature == "evaluators")
            .expect("evaluators row missing");
        assert_eq!(evals.total_count, 1);
        assert_eq!(evals.tier_at_event, "free");
        assert_eq!(evals.trial_cohort, None);
    }

    #[test]
    fn since_filter_drops_older_flushes() {
        let mut conn = fresh_db();
        insert_conversion_events(
            &mut conn,
            &[ev("s", "cloud-traces", "pro", None, 4)],
            "2026-05-20T10:00:00Z",
        )
        .unwrap();
        insert_conversion_events(
            &mut conn,
            &[ev("s", "cloud-traces", "pro", None, 7)],
            "2026-05-21T10:00:00Z",
        )
        .unwrap();
        let all = query_conversion_funnel(&conn, None).unwrap();
        assert_eq!(all[0].total_count, 11);
        let recent = query_conversion_funnel(&conn, Some("2026-05-21T00:00:00Z")).unwrap();
        assert_eq!(recent[0].total_count, 7);
    }
}
