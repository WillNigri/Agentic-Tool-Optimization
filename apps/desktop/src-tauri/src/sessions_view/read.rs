// sessions_view/read.rs — DB read paths for the Sessions surface.
//
// Pure read commands: hits SQLite, returns serializable rows for the
// Sessions tab list, detail views, search, and cost breakdown. No
// subprocess spawns, no writes — that lives in `sessions_view/write.rs`.
//
// 2026-05-19 elegance war-room split (was 1635-line sessions_view.rs;
// codex flagged the file as a hotspot before lazy row creation lands
// on top of it).
//
// Owned: list_sessions_full, list_sessions_inner, search_session_turns,
// billing_mode_fallback, get_session_cost_breakdown,
// get_war_room_constituents, get_single_run_detail, get_session_transcript.
// Local structs: SessionCostRow, SessionCostBreakdown, SingleRunDetail.

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::DbState;
use super::{SessionListRow, SessionTranscript, SessionTurn};

#[tauri::command]
pub fn list_sessions_full(
    db: State<'_, DbState>,
    limit: Option<i64>,
) -> Result<Vec<SessionListRow>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_sessions_inner(&conn, limit.unwrap_or(50)).map_err(|e| e.to_string())
}

fn list_sessions_inner(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<SessionListRow>> {
    // PR 5a (Sessions UX polish, 2026-05-17) — Sessions list is now a
    // unified feed of both real multi-turn sessions and "single_run"
    // single-shot dispatches (execution_logs with session_id IS NULL).
    // The History tab (Runs → History) shipped the same data twice; PR
    // 5 collapses it into one WhatsApp-style inbox where group chats
    // (sessions) and single chats (standalone dispatches) coexist.
    //
    // Two SELECTs, merged Rust-side rather than via SQL UNION — the
    // enrichment phase below (distinct runtimes, agent slugs, cost
    // sum, last-turn preview) is per-row work on session rows and
    // is irrelevant for single-run rows, so doing them separately
    // keeps each path readable and avoids a UNION that would have to
    // emit dummy-column padding for the session-only fields. The
    // final list is sorted by the unified timestamp (last_used_at for
    // sessions, created_at for single-runs — both are "when this thing
    // last had activity") and truncated to `limit`.
    //
    // SELECT the v2.6 lifecycle columns alongside the originals.
    // COALESCE wraps status because the v2.6 migration sets a default
    // of 'open' but pre-migration rows on a partially-upgraded install
    // could still surface NULL on read (defensive — the ALTER carries
    // the default forward, but the cost of being safe is zero).
    // PR 15 — LEFT JOIN against projects so the row carries both the
    // canonical id (project_id) AND the human-readable name
    // (project_name). Falls back to NULL on the name when the row
    // was tagged with an id whose project was later deleted; the
    // frontend then renders the short-form id.
    // v2.7.13 — also SELECT human_comment so the list card can render
    // the human's note without drilling in. Sessions don't have a
    // coordinator_runtime column persisted yet (close() emits the
    // value into the CLI response but never UPDATEs sessions with
    // it — historical oversight, queued as a v2.7.14 fix). The
    // session card uses s.runtime for the coordinator label today
    // anyway, so the omission is invisible. War-rooms + chats DO
    // have their own coordinator_runtime column (added v2.7.13).
    let mut stmt = conn.prepare(
        "SELECT s.id, s.runtime, s.agent_slug, s.title, s.created_at, s.last_used_at, s.turn_count,
                COALESCE(s.status, 'open'), s.closed_at, s.auto_title, s.summary, s.tags_json, s.project_id,
                s.category, s.team, p.name, s.human_comment
           FROM sessions s
           LEFT JOIN projects p ON p.id = s.project_id
          ORDER BY s.last_used_at DESC
          LIMIT ?1",
    )?;
    let rows: Vec<SessionListRow> = stmt
        .query_map([limit], |r| {
            let tags_json: Option<String> = r.get(11)?;
            let tags: Vec<String> = tags_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            Ok(SessionListRow {
                id: r.get(0)?,
                runtime: r.get(1)?,
                agent_slug: r.get(2)?,
                title: r.get(3)?,
                created_at: r.get(4)?,
                last_used_at: r.get(5)?,
                turn_count: r.get(6)?,
                runtimes_used: Vec::new(),
                agents_used: Vec::new(),
                total_cost_usd: None,
                last_assistant_preview: None,
                status: r.get(7)?,
                closed_at: r.get(8)?,
                auto_title: r.get(9)?,
                summary: r.get(10)?,
                tags,
                project_id: r.get(12)?,
                project_name: r.get(15)?,
                category: r.get(13)?,
                team: r.get(14)?,
                // See module comment above — sessions don't have a
                // persisted coordinator_runtime column. The card
                // falls back to s.runtime for the coordinator label.
                coordinator_runtime: None,
                human_comment: r.get(16)?,
                row_kind: "session".to_string(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Enrich each row with computed fields. Two cheap follow-up queries
    // per session — fine for the default limit of 50, and the indexes
    // on session_turns(session_id, turn_index ASC) make them O(log N).
    let mut enriched = Vec::with_capacity(rows.len());
    for mut row in rows {
        // Distinct runtimes in this session's turns. For Slice B
        // cross-runtime sessions this is what makes the multi-badge UI
        // render correctly.
        let mut rt_stmt = conn.prepare_cached(
            "SELECT DISTINCT runtime FROM session_turns WHERE session_id = ?1 ORDER BY turn_index ASC",
        )?;
        let runtimes: Vec<String> = rt_stmt
            .query_map([&row.id], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        // Fall back to the session's anchor runtime when session_turns
        // is empty (e.g. a freshly opened session before its first
        // dispatch lands a turn).
        row.runtimes_used = if runtimes.is_empty() {
            vec![row.runtime.clone()]
        } else {
            runtimes
        };

        // 2026-05-16 — distinct agent slugs on assistant turns. Order
        // by first appearance (MIN(turn_index)) so the badge cluster
        // matches the order seats spoke in. Generalist turns (NULL
        // agent_slug) are excluded — they show up via the runtime
        // badges alone.
        let mut ag_stmt = conn.prepare_cached(
            "SELECT agent_slug FROM session_turns
              WHERE session_id = ?1 AND role = 'assistant' AND agent_slug IS NOT NULL
              GROUP BY agent_slug
              ORDER BY MIN(turn_index) ASC",
        )?;
        let agents: Vec<String> = ag_stmt
            .query_map([&row.id], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        row.agents_used = agents;

        // 2026-05-16 — session-total cost from execution_logs. NULL out
        // (rather than 0.0) when there are no rows so the UI knows the
        // session pre-dates session-id-on-execution-logs and can hide
        // the pill instead of rendering a misleading "$0.00".
        let mut cost_stmt = conn.prepare_cached(
            "SELECT SUM(COALESCE(cost_usd_estimated, 0)), COUNT(*)
               FROM execution_logs
              WHERE session_id = ?1",
        )?;
        let (sum_cost, n): (Option<f64>, i64) = cost_stmt
            .query_row([&row.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or((None, 0));
        row.total_cost_usd = if n > 0 { sum_cost.or(Some(0.0)) } else { None };

        // Last assistant turn → preview. Order by turn_index DESC so we
        // get the chronologically last assistant message, not whichever
        // arrived first.
        let mut last_stmt = conn.prepare_cached(
            "SELECT text FROM session_turns
              WHERE session_id = ?1 AND role = 'assistant'
              ORDER BY turn_index DESC
              LIMIT 1",
        )?;
        let preview: Option<String> = last_stmt
            .query_row([&row.id], |r| r.get::<_, String>(0))
            .ok();
        row.last_assistant_preview = preview.map(|s| {
            // Trim to 160 chars max so list rows stay one-line on most
            // viewports. The full text is available in the transcript.
            if s.chars().count() > 160 {
                let truncated: String = s.chars().take(160).collect();
                format!("{}…", truncated)
            } else {
                s
            }
        });

        enriched.push(row);
    }

    // PR 5a — single-run rows: standalone dispatches (execution_logs
    // with session_id IS NULL). One row per dispatch. Synthesizes the
    // session-shaped fields so the frontend renders against one
    // contract; all "session-only" fields (status/summary/tags/etc.)
    // are NULL or sentinel values so the single-run card variant can
    // render without branching on each field.
    //
    // Why session_id IS NULL: anything WITH a session_id is already
    // counted as part of its session via the session_turns table — we
    // don't double-count those here. Standalone dispatches are the
    // ones the History tab was the only surface for, and they're what
    // we need to absorb to make the History tab redundant.
    //
    // The single-run count is also capped at `limit` so a user with
    // 10k standalone dispatches doesn't slow the feed render. The
    // final Rust-side merge orders by timestamp DESC and truncates
    // the combined list to `limit`.
    // PR 14b — single-runs now exclude rows that share a war_room_id;
    // those land below as one synthetic "war-room" row per distinct
    // war_room_id. A user dispatching `ato dispatch claude "..."`
    // without --war-room-id (the common case) still gets a single-run
    // card; only the explicit war-room workflow gets the group
    // treatment.
    // 2026-05-19 war-room synthesis: filter to dispatch_kind='active' so
    // passive-observation rows (Claude Code / Codex CLI jsonl tail rows
    // landing via v2.6 PR-A) don't synthesize as empty-prompt single_run
    // cards. They lack the (prompt, response, model, agent_slug)
    // invariants this card variant assumes.
    let mut eph_stmt = conn.prepare(
        "SELECT e.id, e.runtime, e.agent_slug, e.created_at, e.cost_usd_estimated,
                e.prompt, e.response, e.model, e.status
           FROM execution_logs e
          WHERE e.session_id IS NULL
            AND e.war_room_id IS NULL
            AND e.dispatch_kind = 'active'
          ORDER BY e.created_at DESC
          LIMIT ?1",
    )?;
    let single_runs: Vec<SessionListRow> = eph_stmt
        .query_map([limit], |r| {
            let id: String = r.get(0)?;
            let runtime: String = r.get(1)?;
            let agent_slug: Option<String> = r.get(2)?;
            let created_at: String = r.get(3)?;
            let cost: Option<f64> = r.get(4)?;
            let prompt: Option<String> = r.get(5)?;
            let response: Option<String> = r.get(6)?;
            let _model: Option<String> = r.get(7)?;
            let status_str: Option<String> = r.get(8)?;
            // Single-run title = first 80 chars of the prompt (so the
            // card is recognizable at a glance). The last_assistant_
            // preview slot carries the response truncated to 160. If
            // either is NULL the card still renders — the missing
            // field just collapses.
            let title = prompt.as_deref().map(|s| {
                if s.chars().count() > 80 {
                    let head: String = s.chars().take(80).collect();
                    format!("{}…", head)
                } else {
                    s.to_string()
                }
            });
            let preview = response.as_deref().map(|s| {
                if s.chars().count() > 160 {
                    let head: String = s.chars().take(160).collect();
                    format!("{}…", head)
                } else {
                    s.to_string()
                }
            });
            // A single-run dispatch's "status" mirrors the
            // execution_logs status (success/error/...) rather than
            // sessions' open/closed lifecycle. The frontend uses
            // row_kind to decide whether status semantics are
            // lifecycle ("open"/"closed") or outcome ("success"/etc.).
            let status = status_str.unwrap_or_else(|| "unknown".to_string());
            let agents_used: Vec<String> = match agent_slug.as_deref() {
                Some(slug) if !slug.is_empty() => vec![slug.to_string()],
                _ => Vec::new(),
            };
            Ok(SessionListRow {
                id,
                runtime: runtime.clone(),
                agent_slug,
                title,
                // Single-run rows reuse created_at for both timestamps —
                // a single-shot dispatch IS its own last_used_at.
                created_at: created_at.clone(),
                last_used_at: created_at,
                turn_count: 1,
                runtimes_used: vec![runtime],
                agents_used,
                total_cost_usd: cost,
                last_assistant_preview: preview,
                status,
                closed_at: None,
                auto_title: None,
                summary: None,
                tags: Vec::new(),
                project_id: None,
                project_name: None,
                category: None,
                team: None,
                // Single-runs have no close concept — they're one-shot
                // dispatches, not multi-turn conversations.
                coordinator_runtime: None,
                human_comment: None,
                row_kind: "single_run".to_string(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    // PR 14b — war-room synthetic rows. Group execution_logs by
    // war_room_id (where set), aggregate distinct runtimes + agents,
    // sum cost. One synthetic row per distinct war_room_id. The
    // frontend renders this with the row_kind="war_room" branch and
    // routes click-into to a filtered view showing the constituent
    // single-runs.
    //
    // Query strategy: SELECT distinct war_room_ids first (cheap with
    // the partial index), then per-id aggregate inline. This is N
    // small queries — fine for the typical war-room count (dozens
    // per user, not thousands) and keeps the row shape clean.
    let mut wr_ids_stmt = conn.prepare(
        "SELECT war_room_id, MIN(created_at) AS first_at, MAX(created_at) AS last_at
           FROM execution_logs
          WHERE war_room_id IS NOT NULL
          GROUP BY war_room_id
          ORDER BY last_at DESC
          LIMIT ?1",
    )?;
    let war_room_ids: Vec<(String, String, String)> = wr_ids_stmt
        .query_map([limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut war_rooms: Vec<SessionListRow> = Vec::with_capacity(war_room_ids.len());
    for (wr_id, first_at, last_at) in war_room_ids {
        // Distinct runtimes + agents, in first-spoken order. Same
        // ordering convention as the session-side enrichment, so the
        // frontend renders consistently across row_kinds.
        let mut rt_stmt = conn.prepare_cached(
            "SELECT runtime FROM execution_logs
              WHERE war_room_id = ?1
              GROUP BY runtime
              ORDER BY MIN(created_at) ASC",
        )?;
        let runtimes: Vec<String> = rt_stmt
            .query_map([&wr_id], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();

        let mut ag_stmt = conn.prepare_cached(
            "SELECT agent_slug FROM execution_logs
              WHERE war_room_id = ?1 AND agent_slug IS NOT NULL
              GROUP BY agent_slug
              ORDER BY MIN(created_at) ASC",
        )?;
        let agents: Vec<String> = ag_stmt
            .query_map([&wr_id], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();

        // Sum cost + count participants.
        let mut sum_stmt = conn.prepare_cached(
            "SELECT SUM(COALESCE(cost_usd_estimated, 0)), COUNT(*)
               FROM execution_logs
              WHERE war_room_id = ?1",
        )?;
        let (sum_cost, n_participants): (Option<f64>, i64) = sum_stmt
            .query_row([&wr_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or((None, 0));

        // First prompt of the war-room as a preview — the user's
        // question that kicked it off. Title falls back to a short
        // form of the war_room_id when no prompt is recorded.
        let mut first_stmt = conn.prepare_cached(
            "SELECT prompt FROM execution_logs
              WHERE war_room_id = ?1
              ORDER BY created_at ASC
              LIMIT 1",
        )?;
        let first_prompt: Option<String> = first_stmt
            .query_row([&wr_id], |r| r.get::<_, Option<String>>(0))
            .unwrap_or(None);
        let title = first_prompt.as_deref().map(|s| {
            if s.chars().count() > 80 {
                let head: String = s.chars().take(80).collect();
                format!("{}…", head)
            } else {
                s.to_string()
            }
        });

        // Anchor runtime = first runtime in the order list (i.e.,
        // whichever seat fired first). Keeps the per-card runtime
        // badge in a sensible slot; the full participant cluster is
        // in runtimes_used.
        let anchor_runtime = runtimes
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        // v2.7.13 — LEFT JOIN-style read of the war_rooms row so the
        // list card can render coordinator + CLOSED + summary + tags
        // for closed war rooms without drilling in (Will's dogfood
        // 2026-05-21: war-room cards used to show only the prompt +
        // "kind: parallel" intro text even after close). Legacy war
        // rooms (no war_rooms row yet) fall through to all-NULL +
        // status='open' so the card renders the live state.
        type WrLifecycle = (
            String,         // status
            Option<String>, // closed_at
            Option<String>, // auto_title
            Option<String>, // summary
            Option<String>, // tags_json
            Option<String>, // category
            Option<String>, // team
            Option<String>, // project_id
            Option<String>, // coordinator_runtime
            Option<String>, // human_comment
        );
        let lifecycle: WrLifecycle = conn
            .query_row(
                "SELECT status, closed_at, auto_title, summary, tags_json,
                        category, team, project_id, coordinator_runtime, human_comment
                   FROM war_rooms WHERE id = ?1",
                [&wr_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, Option<String>>(9)?,
                    ))
                },
            )
            .unwrap_or_else(|_| {
                (
                    "open".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            });
        let (
            wr_status,
            wr_closed_at,
            wr_auto_title,
            wr_summary,
            wr_tags_json,
            wr_category,
            wr_team,
            wr_project_id,
            wr_coordinator,
            wr_human_comment,
        ) = lifecycle;
        let wr_tags: Vec<String> = wr_tags_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        // Resolve project name when project_id is set (parity with
        // session-side LEFT JOIN). One small query per war room, fine
        // for the typical war-room count per user.
        let wr_project_name: Option<String> = wr_project_id.as_deref().and_then(|pid| {
            conn.query_row(
                "SELECT name FROM projects WHERE id = ?1",
                [pid],
                |r| r.get::<_, String>(0),
            )
            .ok()
        });

        war_rooms.push(SessionListRow {
            id: wr_id.clone(),
            runtime: anchor_runtime,
            agent_slug: None,
            title,
            created_at: first_at,
            last_used_at: last_at,
            turn_count: n_participants,
            runtimes_used: runtimes,
            agents_used: agents,
            total_cost_usd: if n_participants > 0 {
                sum_cost.or(Some(0.0))
            } else {
                None
            },
            last_assistant_preview: None,
            status: wr_status,
            closed_at: wr_closed_at,
            auto_title: wr_auto_title,
            summary: wr_summary,
            tags: wr_tags,
            project_id: wr_project_id,
            project_name: wr_project_name,
            category: wr_category,
            team: wr_team,
            coordinator_runtime: wr_coordinator,
            human_comment: wr_human_comment,
            row_kind: "war_room".to_string(),
        });
    }

    // 2026-05-18 — Path A: bottom-pane chat threads land in the Sessions
    // feed as a fourth row_kind. Per Will: "the chats we have in the
    // bottom of the app should also appear in the sessions." Chat
    // threads remain their own table (no migration); we just teach the
    // feed to UNION them so the user has ONE inbox for every
    // conversation kind. Path B (migrating chat_threads into the
    // sessions table) lives behind a multi-launcher refactor of the
    // bottom pane — not in scope here.
    //
    // Card shape: title comes from chat_threads.title (already
    // human-readable, no truncation needed). Runtime = the runtime of
    // the most recent assistant turn (chat_threads doesn't pin to one;
    // each message records which runtime answered). Preview = the
    // most recent assistant message's content, truncated to 160 chars
    // to match the session/war-room preview convention.
    // v2.7.13 — SELECT the lifecycle columns added by the v2.7.13
    // schema migration so the chat card can render the same closed-
    // state surface as sessions + war rooms (coordinator badge, auto
    // title, summary, tags, human comment).
    //
    // War-room review 76F7CEEB (claude FIX #1): defensive fallback.
    // The schema ALTERs are wrapped in `let _ = conn.execute(...)`
    // (schema.rs) — they silently swallow errors. On older SQLite
    // (pre-3.25) or a write-locked DB during init, ALTER can fail
    // and leave the column missing; then this prepare() errors,
    // list_sessions_inner returns Err, and the WHOLE Sessions feed
    // empties out — the exact regression shape we just patched on
    // the sessions side (0c5ef70). Try the wide SELECT first; on
    // prepare failure, fall back to the pre-v2.7.13 narrow SELECT
    // so chats still appear with lifecycle defaulted to 'open'.
    let chats_wide = conn.prepare(
        "SELECT id, title, created_at, COALESCE(last_message_at, created_at) AS last_at,
                message_count, project_id,
                COALESCE(status, 'open'), closed_at, auto_title, summary, tags_json,
                category, team, coordinator_runtime, human_comment
           FROM chat_threads
          WHERE archived = 0
          ORDER BY last_at DESC
          LIMIT ?1",
    );
    let mut ct_stmt = match chats_wide {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "list_sessions_inner: chat_threads wide SELECT failed ({}); falling back to narrow pre-v2.7.13 shape so the feed still renders.",
                e
            );
            return list_sessions_narrow_chat_fallback(conn, enriched, single_runs, war_rooms, limit);
        }
    };
    let chats: Vec<SessionListRow> = ct_stmt
        .query_map([limit], |r| {
            Ok((
                r.get::<_, String>(0)?,                 // id
                r.get::<_, String>(1)?,                 // title
                r.get::<_, String>(2)?,                 // created_at
                r.get::<_, String>(3)?,                 // last_at
                r.get::<_, i64>(4)?,                    // message_count
                r.get::<_, Option<String>>(5)?,         // project_id
                r.get::<_, String>(6)?,                 // status
                r.get::<_, Option<String>>(7)?,         // closed_at
                r.get::<_, Option<String>>(8)?,         // auto_title
                r.get::<_, Option<String>>(9)?,         // summary
                r.get::<_, Option<String>>(10)?,        // tags_json
                r.get::<_, Option<String>>(11)?,        // category
                r.get::<_, Option<String>>(12)?,        // team
                r.get::<_, Option<String>>(13)?,        // coordinator_runtime
                r.get::<_, Option<String>>(14)?,        // human_comment
            ))
        })?
        // v2.7.14 — log on row-drop so the next regression of the
        // "silent empty list" shape (0c5ef70 / b1a397c) leaves a trail.
        // The fallback below kicks in on prepare() failure; this
        // covers per-row decode failure (e.g. one bad column on one
        // row). MiniMax dogfood review 2026-05-21 #2.
        .filter_map(|r| match r {
            Ok(row) => Some(row),
            Err(e) => {
                eprintln!("list_sessions_inner: dropping chat row: {}", e);
                None
            }
        })
        .map(|(
            id,
            title,
            created_at,
            last_at,
            message_count,
            project_id,
            status,
            closed_at,
            auto_title,
            summary,
            tags_json,
            category,
            team,
            coordinator_runtime,
            human_comment,
        )| {
            let tags: Vec<String> = tags_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            // Last assistant message — runtime + content preview. Two
            // small queries per thread, fine for the default limit. Each
            // is O(log N) on the (thread_id, created_at) index.
            let mut last_rt_stmt = conn.prepare_cached(
                "SELECT runtime FROM chat_messages
                  WHERE thread_id = ?1 AND role = 'assistant' AND runtime IS NOT NULL
                  ORDER BY created_at DESC
                  LIMIT 1",
            ).ok();
            let runtime: String = last_rt_stmt
                .as_mut()
                .and_then(|s| s.query_row([&id], |r| r.get::<_, String>(0)).ok())
                .unwrap_or_else(|| "chat".to_string());

            let mut last_msg_stmt = conn.prepare_cached(
                "SELECT content FROM chat_messages
                  WHERE thread_id = ?1 AND role = 'assistant'
                  ORDER BY created_at DESC
                  LIMIT 1",
            ).ok();
            let preview: Option<String> = last_msg_stmt
                .as_mut()
                .and_then(|s| s.query_row([&id], |r| r.get::<_, String>(0)).ok())
                .map(|s| {
                    if s.chars().count() > 160 {
                        let head: String = s.chars().take(160).collect();
                        format!("{}…", head)
                    } else {
                        s
                    }
                });

            SessionListRow {
                id,
                runtime: runtime.clone(),
                agent_slug: None,
                title: Some(title),
                created_at,
                last_used_at: last_at,
                turn_count: message_count,
                runtimes_used: vec![runtime],
                agents_used: Vec::new(),
                total_cost_usd: None,
                last_assistant_preview: preview,
                // v2.7.13 — lifecycle now real: pulled from the
                // chat_threads row via the SELECT above. Defaults
                // were 'open' / NULL pre-close so legacy chats render
                // unchanged.
                status,
                closed_at,
                auto_title,
                summary,
                tags,
                project_id,
                project_name: None,
                category,
                team,
                coordinator_runtime,
                human_comment,
                row_kind: "chat".to_string(),
            }
        })
        .collect();

    // Merge: real sessions + single-runs + war-rooms + chat threads,
    // sorted by their unified timestamp (last_used_at, which equals
    // created_at for single-runs). Stable sort so two rows with the
    // same timestamp keep their intra-list order, which is good for
    // determinism in tests.
    enriched.extend(single_runs);
    enriched.extend(war_rooms);
    enriched.extend(chats);
    enriched.sort_by(|a, b| b.last_used_at.cmp(&a.last_used_at));
    enriched.truncate(limit as usize);
    Ok(enriched)
}

/// War-room review 76F7CEEB FIX #1 — fallback path for when the
/// v2.7.13 chat_threads lifecycle columns aren't available (older
/// SQLite, partial migration, write-locked init). Reads only the
/// pre-v2.7.13 columns; defaults status to 'open' + lifecycle
/// metadata to None so chats still appear in the feed while the
/// real columns are absent. The narrow SELECT mirrors the original
/// shape before the v2.7.13 wide SELECT was introduced — single
/// source of truth for "what columns can we definitely trust on
/// chat_threads."
fn list_sessions_narrow_chat_fallback(
    conn: &Connection,
    mut enriched: Vec<SessionListRow>,
    single_runs: Vec<SessionListRow>,
    war_rooms: Vec<SessionListRow>,
    limit: i64,
) -> rusqlite::Result<Vec<SessionListRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at, COALESCE(last_message_at, created_at) AS last_at,
                message_count, project_id
           FROM chat_threads
          WHERE archived = 0
          ORDER BY last_at DESC
          LIMIT ?1",
    )?;
    let chats: Vec<SessionListRow> = stmt
        .query_map([limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(|(id, title, created_at, last_at, message_count, project_id)| SessionListRow {
            id,
            runtime: "chat".to_string(),
            agent_slug: None,
            title: Some(title),
            created_at,
            last_used_at: last_at,
            turn_count: message_count,
            runtimes_used: vec!["chat".to_string()],
            agents_used: Vec::new(),
            total_cost_usd: None,
            last_assistant_preview: None,
            status: "open".to_string(),
            closed_at: None,
            auto_title: None,
            summary: None,
            tags: Vec::new(),
            project_id,
            project_name: None,
            category: None,
            team: None,
            coordinator_runtime: None,
            human_comment: None,
            row_kind: "chat".to_string(),
        })
        .collect();
    enriched.extend(single_runs);
    enriched.extend(war_rooms);
    enriched.extend(chats);
    enriched.sort_by(|a, b| b.last_used_at.cmp(&a.last_used_at));
    enriched.truncate(limit as usize);
    Ok(enriched)
}

// v2.6 Slice C — content search across turn text. The metadata search
// in the React component handles title/summary/tag/runtime matching
// client-side; this Tauri command extends it to "find sessions that
// contain these words anywhere in the conversation." Returns the set
// of session ids whose turns contain ALL the query tokens (each
// token can match any turn — they don't have to be in the same turn,
// since multi-turn conversations split topics across messages).
//
// Implementation is plain LIKE rather than FTS5 because (a) the
// turn-text table is bounded by a single user's local sessions —
// not a corpus — and (b) keeping it LIKE means no migration cost
// and no FTS5 index drift to worry about. If a user reports it
// being slow we can swap in FTS5 transparently.
#[tauri::command]
pub fn search_session_turns(
    db: State<'_, DbState>,
    query: String,
) -> Result<Vec<String>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    // Tokenize on whitespace and require every token to appear in
    // SOME turn of the session. Cap tokens to 8 to bound the query
    // size and reject empty strings post-trim.
    let tokens: Vec<String> = trimmed
        .split_whitespace()
        .take(8)
        .map(|s| s.to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    // For each token, find the set of sessions whose turns include it.
    // Intersect across tokens to get sessions containing all of them.
    let mut result_set: Option<std::collections::HashSet<String>> = None;
    for token in &tokens {
        let like_pattern = format!("%{}%", token);
        let mut stmt = conn
            .prepare_cached(
                "SELECT DISTINCT session_id FROM session_turns WHERE LOWER(text) LIKE ?1",
            )
            .map_err(|e| e.to_string())?;
        let ids: std::collections::HashSet<String> = stmt
            .query_map([&like_pattern], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        result_set = Some(match result_set {
            Some(existing) => existing.intersection(&ids).cloned().collect(),
            None => ids,
        });
        // Short-circuit once the intersection is empty.
        if result_set.as_ref().map(|s| s.is_empty()).unwrap_or(false) {
            return Ok(Vec::new());
        }
    }
    Ok(result_set.map(|s| s.into_iter().collect()).unwrap_or_default())
}

// 2026-05-16 — cost receipts panel.
//
// The Loom shot-list's most compelling moment is the cost-comparison
// table that shows "the cheapest model caught the bug." That data lives
// in execution_logs.cost_usd_estimated + tokens_in/out + duration_ms,
// joined to the session by session_id. This command exposes the per-
// (runtime, agent_slug) breakdown for a single session so the chat
// detail can render a receipts panel at the bottom.
//
// Rows include both successful AND error turns (errors still cost
// tokens at the provider) — `successful_turns` lets the UI distinguish.
// Generalist turns surface as agent_slug = None; the UI renders these
// as "<generalist>" so the row reads cleanly.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionCostRow {
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub total_turns: i64,
    pub successful_turns: i64,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub total_duration_ms: Option<i64>,
    /// 2026-05-16 — `cost_null_turns` counts rows where the dispatch
    /// computed a NULL cost (model missing from pricing table). The UI
    /// surfaces these as "$? (model not in pricing table)" so a stale
    /// pricing table doesn't masquerade as a free dispatch.
    pub cost_null_turns: i64,
    pub total_cost_usd: f64,
    /// "subscription" / "api_key" / "local" — read from
    /// `execution_logs.auth_mode` when populated (authoritative; per-
    /// row truth from the dispatch). Falls back to a static lookup on
    /// the runtime name for pre-auth-mode rows.
    pub billing_mode: String,
}

/// Fallback for older rows where `execution_logs.auth_mode` is NULL.
/// Delegates to the shared `ato_pricing::billing_mode` so the CLI and
/// desktop classify runtimes identically.
fn billing_mode_fallback(runtime: &str) -> &'static str {
    ato_pricing::billing_mode(runtime).as_str()
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionCostBreakdown {
    pub session_id: String,
    pub total_cost_usd: f64,
    pub total_turns: i64,
    pub total_tokens_in: i64,
    pub total_tokens_out: i64,
    pub total_duration_ms: i64,
    pub rows: Vec<SessionCostRow>,
}

#[tauri::command]
pub fn get_session_cost_breakdown(
    db: State<'_, DbState>,
    session_id: String,
) -> Result<SessionCostBreakdown, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT runtime,
                    agent_slug,
                    auth_mode,
                    COUNT(*) AS total_turns,
                    SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) AS successful_turns,
                    SUM(COALESCE(tokens_in, 0))  AS tokens_in,
                    SUM(COALESCE(tokens_out, 0)) AS tokens_out,
                    SUM(COALESCE(duration_ms, 0)) AS total_duration_ms,
                    SUM(CASE WHEN cost_usd_estimated IS NULL AND status = 'success' THEN 1 ELSE 0 END) AS cost_null_turns,
                    SUM(COALESCE(cost_usd_estimated, 0)) AS total_cost_usd
               FROM execution_logs
              WHERE session_id = ?1
              GROUP BY runtime, agent_slug, auth_mode
              ORDER BY total_cost_usd DESC, runtime ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows: Vec<SessionCostRow> = stmt
        .query_map([&session_id], |r| {
            let runtime: String = r.get(0)?;
            let auth_mode: Option<String> = r.get(2)?;
            let billing_mode = auth_mode
                .clone()
                .unwrap_or_else(|| billing_mode_fallback(&runtime).to_string());
            Ok(SessionCostRow {
                runtime,
                agent_slug: r.get(1)?,
                total_turns: r.get(3)?,
                successful_turns: r.get(4)?,
                tokens_in: r.get::<_, Option<i64>>(5)?,
                tokens_out: r.get::<_, Option<i64>>(6)?,
                total_duration_ms: r.get::<_, Option<i64>>(7)?,
                cost_null_turns: r.get(8)?,
                total_cost_usd: r.get(9)?,
                billing_mode,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let total_cost_usd: f64 = rows.iter().map(|r| r.total_cost_usd).sum();
    let total_turns: i64 = rows.iter().map(|r| r.total_turns).sum();
    let total_tokens_in: i64 = rows.iter().map(|r| r.tokens_in.unwrap_or(0)).sum();
    let total_tokens_out: i64 = rows.iter().map(|r| r.tokens_out.unwrap_or(0)).sum();
    let total_duration_ms: i64 = rows.iter().map(|r| r.total_duration_ms.unwrap_or(0)).sum();

    Ok(SessionCostBreakdown {
        session_id,
        total_cost_usd,
        total_turns,
        total_tokens_in,
        total_tokens_out,
        total_duration_ms,
        rows,
    })
}

/// PR 5c (Sessions UX polish, 2026-05-17) — full detail for a single
/// "single_run" dispatch (an `execution_logs` row with `session_id IS
/// NULL`). The Sessions tab's single-run cards (added in 5a/5b) route
/// here on click instead of the multi-turn `get_session_transcript`
/// path, because a single-run row has no session row to fetch — it
/// IS the entire conversation, one prompt + one response.
///
/// Why a separate command rather than overloading `get_session_
/// transcript`: codex-reviewer Round-1 #4 — "Define the single-run-
/// open click contract explicitly. If detail loaders assume
/// `session_id`, single-run open will misroute." Two commands keeps
/// the contracts honest: each one has a fixed expectation about its
/// id space (session uuid vs execution_log uuid) and a fixed return
/// shape, so the frontend's discriminator (`rowKind`) maps to a
/// real-routing fork rather than a runtime branch inside one bag.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SingleRunDetail {
    pub id: String,
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub model: Option<String>,
    pub status: String,
    pub prompt: Option<String>,
    pub response: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub duration_ms: Option<i64>,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub cost_usd_estimated: Option<f64>,
    pub auth_mode: Option<String>,
    /// PR 16 (2026-05-18) — war-room round (1-indexed) for rows that
    /// participate in a multi-turn war-room. NULL for single-run
    /// dispatches (which have no war_room_id) and for rows
    /// pre-PR-16 (backfilled to 1 by the migration). The frontend's
    /// WarRoomDetailView groups by this to render rounds as
    /// stacked sections.
    pub war_room_round: Option<i64>,
    /// F1 / S3 follow-up (v2.7.11) — raw JSON from
    /// `execution_logs.tool_calls_summary` ([{name, args_brief,
    /// is_error}, ...]) for the PermissionEventsPanel. NULL on
    /// pre-v2.7.8 rows (the column is recent) and on dispatches
    /// that didn't use any tools.
    pub tool_calls_summary: Option<String>,
}

/// First-Chat Wizard (2026-05-18) — fire a war-room from the
/// desktop in one Tauri call. The wizard hands us a prompt + the
/// list of enabled runtimes; this command mints a war_room_id and
/// spawns N parallel `ato dispatch <runtime> "<prompt>" --war-room-id
/// <uuid> --quiet` subprocesses, waits for all to return, and
/// reports the war_room_id back so the frontend can route to
/// WarRoomDetailView.
///
/// Why a dedicated command instead of N invoke("prompt_agent")
/// calls from the frontend: prompt_agent doesn't accept a
/// war_room_id (and threading it through every existing dispatch
/// path is a bigger refactor than this onboarding-flow shortcut
/// warrants). The CLI already accepts --war-room-id (PR 14a), so
/// shelling out to it inherits all the existing dispatch
/// behavior — quotas, byok, signed-binary handling, cost
/// recording — without re-implementing any of it.
///
/// Best-effort error handling: if a single seat fails (e.g., one
/// runtime hits a rate limit), the war-room still surfaces the
/// other replies. The CLI writes the failed seat's execution_log
/// row with status="error", so the war-room card + drill-in view
/// render the failure visibly rather than dropping the seat
#[tauri::command]
pub fn get_war_room_constituents(
    db: State<'_, DbState>,
    war_room_id: String,
) -> Result<Vec<SingleRunDetail>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, runtime, agent_slug, model, status, prompt, response, error_message,
                    created_at, duration_ms, tokens_in, tokens_out, cost_usd_estimated, auth_mode,
                    war_room_round, tool_calls_summary
               FROM execution_logs
              WHERE war_room_id = ?1
              ORDER BY war_room_round ASC, created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<SingleRunDetail> = stmt
        .query_map([&war_room_id], |r| {
            Ok(SingleRunDetail {
                id: r.get(0)?,
                runtime: r.get(1)?,
                agent_slug: r.get(2)?,
                model: r.get(3)?,
                status: r.get(4)?,
                prompt: r.get(5)?,
                response: r.get(6)?,
                error_message: r.get(7)?,
                created_at: r.get(8)?,
                duration_ms: r.get(9)?,
                tokens_in: r.get(10)?,
                tokens_out: r.get(11)?,
                cost_usd_estimated: r.get(12)?,
                auth_mode: r.get(13)?,
                war_room_round: r.get(14)?,
                tool_calls_summary: r.get(15)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

#[tauri::command]
pub fn get_single_run_detail(
    db: State<'_, DbState>,
    log_id: String,
) -> Result<SingleRunDetail, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // Codex Round-1 #1 — the WHERE clause MUST enforce `session_id IS
    // NULL`. Without it, any execution_logs row is fetchable, including
    // session-attached turns; the frontend's `rowKind` discriminator
    // would be advisory rather than load-bearing. Making the contract
    // real here means a misrouted click (or a stale id from an
    // outdated client) gets a `not found` error instead of silently
    // pulling session-turn data into the wrong detail view.
    conn.query_row(
        "SELECT id, runtime, agent_slug, model, status, prompt, response, error_message,
                created_at, duration_ms, tokens_in, tokens_out, cost_usd_estimated, auth_mode,
                war_room_round, tool_calls_summary
           FROM execution_logs
          WHERE id = ?1 AND session_id IS NULL",
        [&log_id],
        |r| {
            Ok(SingleRunDetail {
                id: r.get(0)?,
                runtime: r.get(1)?,
                agent_slug: r.get(2)?,
                model: r.get(3)?,
                status: r.get(4)?,
                prompt: r.get(5)?,
                response: r.get(6)?,
                error_message: r.get(7)?,
                created_at: r.get(8)?,
                duration_ms: r.get(9)?,
                tokens_in: r.get(10)?,
                tokens_out: r.get(11)?,
                cost_usd_estimated: r.get(12)?,
                auth_mode: r.get(13)?,
                war_room_round: r.get(14)?,
                tool_calls_summary: r.get(15)?,
            })
        },
    )
    .map_err(|e| {
        format!(
            "single-run dispatch id {} not found (either the id doesn't exist or it belongs to a session — session-attached turns are fetched via get_session_transcript, not this command): {}",
            log_id, e
        )
    })
}

#[tauri::command]
pub fn get_session_transcript(
    db: State<'_, DbState>,
    session_id: String,
) -> Result<SessionTranscript, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    type Header = (
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let (
        runtime,
        agent_slug,
        title,
        status,
        closed_at,
        auto_title,
        summary,
        tags_json,
        project_id,
        human_comment,
    ): Header = conn
        .query_row(
            "SELECT runtime, agent_slug, title,
                    COALESCE(status, 'open'), closed_at, auto_title, summary, tags_json, project_id,
                    human_comment
               FROM sessions WHERE id = ?1",
            [&session_id],
            |r| Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            )),
        )
        .map_err(|e| format!("session not found: {}", e))?;
    let tags: Vec<String> = tags_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let mut stmt = conn
        .prepare(
            "SELECT turn_index, role, text, runtime, created_at, agent_slug
               FROM session_turns
              WHERE session_id = ?1
              ORDER BY turn_index ASC",
        )
        .map_err(|e| e.to_string())?;

    let turns: Vec<SessionTurn> = stmt
        .query_map([&session_id], |r| {
            Ok(SessionTurn {
                turn_index: r.get(0)?,
                role: r.get(1)?,
                text: r.get(2)?,
                runtime: r.get(3)?,
                created_at: r.get(4)?,
                agent_slug: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(SessionTranscript {
        id: session_id,
        runtime,
        agent_slug,
        title,
        turns,
        status,
        closed_at,
        auto_title,
        summary,
        tags,
        project_id,
        human_comment,
    })
}

// ───────────────────────────────────────────────────────────────────────
// v2.3.43 — Tauri commands for the New / Continue / Bridge buttons.
//
// Each shells out to the `ato` CLI binary, which is the canonical
// implementation of sessions / dispatch / bridge. The desktop's own
// prompt_agent path doesn't yet support --session natively (a deeper
// change); going through the CLI keeps these slices independent and
// the behavior provably identical to what an agent invoking
// `ato dispatch ... --session` would do.
