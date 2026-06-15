// v2.7.13 — war rooms become first-class closeable conversations.
//
// Pre-existing shape: a war room is N execution_logs rows sharing a
// `war_room_id` UUID. Each row is one LLM seat's reply for one round.
// Multi-round war rooms accumulate into the same group via
// `war_room_round`. Until this module landed, war rooms were stateless
// — no summary, no lifecycle, no human framing.
//
// What this module adds:
//   - `WarRoom` struct + Closeable impl: fetches the rounds from
//     execution_logs, persists close fields to the new `war_rooms`
//     table (see schema.rs).
//   - `lookup` / `close` / `reopen` thin functions the CLI calls.
//
// The shared close orchestration lives in `conversation_close`. This
// module only owns: how to query a war room's rounds, and how to
// UPSERT the war_rooms row. Prompt construction + summarizer dispatch
// + JSON parse + validation all delegate to the trait + helper.

use anyhow::{anyhow, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::commands::conversation_close::{
    close_conversation, reopen_conversation, CloseFields, Closeable, ConversationTurn,
};
use crate::output::{emit_human, emit_json, Opts};

/// In-memory snapshot of a war room's lifecycle metadata. Built
/// lazily by `lookup` from the existing execution_logs grouping +
/// the (possibly absent) war_rooms row.
///
/// v2.7.14 — `rename_all = "camelCase"` so JSON keys on the wire
/// match SessionListRow / SessionTranscript / every other shape
/// the desktop frontend reads. Pre-fix `get_war_room` returned
/// snake_case (`coordinator_runtime`, `seat_count`, `closed_at`)
/// while every other Tauri command returned camelCase — the TS
/// `WarRoomSnapshot` interface compensated by using snake_case,
/// which works but is inconsistent and confuses future readers.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarRoom {
    pub id: String,
    pub status: String,
    pub closed_at: Option<String>,
    pub auto_title: Option<String>,
    pub summary: Option<String>,
    pub coordinator_runtime: Option<String>,
    /// v2.7.13 fix — the persisted human note. Surfaced in the
    /// closed-war-room summary card alongside the coordinator's
    /// output. NULL when no comment was attached.
    pub human_comment: Option<String>,
    /// v2.7.13 fix — persisted tags from the coordinator. Render as
    /// chips under the summary so the card matches sessions parity.
    pub tags: Vec<String>,
    /// Count of execution_logs rows that participate in this war room.
    /// Surfaced as "turns" in the close-time metadata block.
    pub seat_count: i64,
}

impl Closeable for WarRoom {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_label(&self) -> &'static str {
        "war room"
    }
    fn status(&self) -> &str {
        &self.status
    }
    fn stored_agent_slug(&self) -> Option<&str> {
        // War rooms are multi-agent by design; no single slug applies.
        // The resolve_summarizer chain falls through to the registry
        // default unless the caller passes --coordinator / --as.
        None
    }
    fn anchor_runtime(&self) -> Option<&str> {
        // Same reasoning: each seat has its own runtime. We don't pick
        // an "anchor" here; the summarizer is whichever LLM the user
        // selects (or the first registered API provider with a key).
        None
    }
    fn existing_title(&self) -> Option<&str> {
        self.auto_title.as_deref()
    }
    fn fetch_turns(&self, conn: &Connection) -> Result<Vec<ConversationTurn>> {
        // Order by round first, then created_at within round so the
        // coordinator sees the rounds in the order they fired.
        //
        // v2.7.14 — explicit LIMIT 1000 (MiniMax dogfood review
        // 2026-05-21 caught this): a war room with N=10k execution
        // logs would feed all of them into the coordinator prompt,
        // blowing token budgets and bill. 1000 is conservative — far
        // larger than any real war-room (typical: 3-15 seats × 1-5
        // rounds = ~75 rows) but bounds the worst case for a runaway
        // automation.
        //
        // Inner subquery takes the LATEST 1000 (DESC + LIMIT), outer
        // re-sorts ASC so the transcript reads chronologically. We
        // prefer recent context over ancient bootstrap when the cap
        // bites.
        let mut stmt = conn.prepare(
            "SELECT runtime, prompt, response FROM (
                SELECT runtime, prompt, response, war_room_round, created_at
                  FROM execution_logs
                 WHERE war_room_id = ?1
                 ORDER BY COALESCE(war_room_round, 1) DESC, created_at DESC
                 LIMIT 1000
             )
             ORDER BY COALESCE(war_room_round, 1) ASC, created_at ASC",
        )?;
        let rows = stmt.query_map([&self.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut turns = Vec::new();
        for row in rows {
            let (runtime, prompt, response) = row?;
            // The original prompt for the war-room round only needs to
            // appear once (it's identical across seats). We surface it
            // as a `user` turn before each seat's reply so the
            // coordinator sees the back-and-forth even though every
            // seat saw the same prompt — keeps the transcript readable
            // even when seats answered very differently.
            if let Some(p) = prompt {
                if !p.trim().is_empty() {
                    turns.push(ConversationTurn {
                        role: "user".to_string(),
                        text: p,
                        runtime: runtime.clone(),
                    });
                }
            }
            if let Some(r) = response {
                if !r.trim().is_empty() {
                    turns.push(ConversationTurn {
                        role: "assistant".to_string(),
                        text: r,
                        runtime,
                    });
                }
            }
        }
        Ok(turns)
    }
    fn persist_close(&self, conn: &Connection, fields: &CloseFields) -> Result<usize> {
        // UPSERT: war_rooms rows don't pre-exist for legacy war rooms,
        // so INSERT ... ON CONFLICT lets a first-time close write the
        // row while a re-close after reopen updates it. COALESCE on
        // category/team/human_comment matches the sessions-side
        // stickiness — a NULL on a subsequent close preserves the
        // prior value instead of clobbering it.
        let now = chrono::Utc::now().to_rfc3339();
        let tags_json = serde_json::to_string(&fields.tags).unwrap_or_else(|_| "[]".to_string());
        // v2.16 attribution — close is the only write path for the row,
        // so stamp the initiator of the close on both insert and re-close.
        let attribution = crate::attribution::Attribution::detect();
        let changed = conn.execute(
            "INSERT INTO war_rooms
                (id, status, closed_at, auto_title, summary, tags_json,
                 category, team, project_id, coordinator_runtime, coordinator_model,
                 human_comment, duration_ms, created_at, updated_at,
                 initiator_kind, client_surface, initiator_id)
             VALUES (?1, 'closed', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, ?14, ?15, ?16)
             ON CONFLICT(id) DO UPDATE SET
                status = 'closed',
                closed_at = excluded.closed_at,
                auto_title = excluded.auto_title,
                summary = excluded.summary,
                tags_json = excluded.tags_json,
                category = COALESCE(excluded.category, war_rooms.category),
                team = COALESCE(excluded.team, war_rooms.team),
                project_id = COALESCE(excluded.project_id, war_rooms.project_id),
                coordinator_runtime = excluded.coordinator_runtime,
                coordinator_model = excluded.coordinator_model,
                human_comment = COALESCE(excluded.human_comment, war_rooms.human_comment),
                duration_ms = excluded.duration_ms,
                updated_at = excluded.updated_at,
                initiator_kind = excluded.initiator_kind,
                client_surface = excluded.client_surface,
                initiator_id = COALESCE(excluded.initiator_id, war_rooms.initiator_id)
              WHERE war_rooms.status = 'open'",
            rusqlite::params![
                self.id,
                fields.closed_at,
                fields.auto_title,
                fields.summary,
                tags_json,
                fields.category,
                fields.team,
                fields.project_id,
                fields.coordinator_runtime,
                fields.coordinator_model,
                fields.human_comment,
                fields.duration_ms,
                now,
                attribution.kind,
                attribution.surface,
                attribution.id,
            ],
        )?;
        Ok(changed)
    }
    fn persist_reopen(&self, conn: &Connection) -> Result<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let changed = conn.execute(
            "UPDATE war_rooms
                SET status = 'open',
                    closed_at = NULL,
                    updated_at = ?1
              WHERE id = ?2 AND status = 'closed'",
            rusqlite::params![now, self.id],
        )?;
        Ok(changed)
    }
}

/// Load a war room snapshot by id. Pre-condition: the war_room_id
/// exists in execution_logs (otherwise the "war room" doesn't exist
/// for anyone). Status defaults to 'open' when no war_rooms row has
/// been written yet (legacy war rooms that pre-date this module).
pub fn lookup(conn: &Connection, id: &str) -> Result<WarRoom> {
    let seat_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM execution_logs WHERE war_room_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if seat_count == 0 {
        return Err(anyhow!(
            "No war room found with id '{}' (no execution_logs rows reference it).",
            id
        ));
    }
    let row: Option<(
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = conn
        .query_row(
            "SELECT status, closed_at, auto_title, summary, coordinator_runtime,
                    human_comment, tags_json
               FROM war_rooms WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()?;
    let (status, closed_at, auto_title, summary, coordinator_runtime, human_comment, tags_json) =
        row.unwrap_or_else(|| {
            // Legacy war room — no war_rooms row exists yet. Default
            // to 'open' so the lifecycle UI surfaces it correctly and
            // a first-time close UPSERTs the row.
            (
                "open".to_string(),
                None,
                None,
                None,
                None,
                None,
                None,
            )
        });
    let tags: Vec<String> = tags_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    Ok(WarRoom {
        id: id.to_string(),
        status,
        closed_at,
        auto_title,
        summary,
        coordinator_runtime,
        human_comment,
        tags,
        seat_count,
    })
}

/// Close a war room with the shared coordinator orchestration. Thin
/// wrapper around `close_conversation`.
#[allow(clippy::too_many_arguments)]
pub fn close(
    conn: &Connection,
    id: &str,
    agent_slug_override: Option<String>,
    model_override: Option<String>,
    coordinator_override: Option<String>,
    human_comment: Option<String>,
    force_close_without_context: bool,
    opts: &Opts,
) -> Result<()> {
    let (target, fields) = close_returning(
        conn,
        id,
        agent_slug_override,
        model_override,
        coordinator_override,
        human_comment,
        force_close_without_context,
        opts,
    )?;
    if opts.human {
        emit_human_close(&target, &fields);
    } else {
        emit_json_close(&target, &fields)?;
    }
    Ok(())
}

/// Close the war room and return the resolved (target, fields) instead
/// of emitting. #70 sweep() consumes this so it can accumulate per-WR
/// results into ONE stable JSON envelope at end-of-run rather than
/// emitting multiple stand-alone JSON documents to stdout.
fn close_returning(
    conn: &Connection,
    id: &str,
    agent_slug_override: Option<String>,
    model_override: Option<String>,
    coordinator_override: Option<String>,
    human_comment: Option<String>,
    force_close_without_context: bool,
    opts: &Opts,
) -> Result<(WarRoom, CloseFields)> {
    let target = lookup(conn, id)?;
    let fields = close_conversation(
        conn,
        &target,
        agent_slug_override.as_deref(),
        model_override.as_deref(),
        coordinator_override.as_deref(),
        human_comment.as_deref(),
        force_close_without_context,
        opts,
    )?;
    Ok((target, fields))
}

/// #70 — sweep idle open war-rooms and auto-close them.
///
/// Identifies war_room_ids that have at least one execution_logs row,
/// no NEW execution_logs rows in the last `idle_minutes`, and are not
/// already represented by a closed war_rooms row. Walks each one
/// through the same `close_conversation` orchestrator that
/// `ato war-rooms close <id>` uses, with the supplied coordinator.
///
/// Designed to be the body of a launchd / cron tick so one-shot R1
/// fan-outs (architecture review, security audit, etc.) self-close
/// once the seats land. Removes the manual `ato war-rooms close`
/// step that's been the #1 UX trap of using ATO for multi-LLM review.
///
/// Each close runs ONE coordinator dispatch (cheap with --coordinator
/// google, the default). The sweep is NOT atomic — closes happen
/// sequentially, and a failure on one WR does not roll back the
/// preceding closes. That is intentional for cron use: partial
/// progress is preferable to all-or-nothing on a transient seat error.
/// Failed WRs surface in the final JSON / human summary.
pub fn sweep(
    conn: &Connection,
    idle_minutes: i64,
    max_per_run: usize,
    coordinator: &str,
    dry_run: bool,
    opts: &Opts,
) -> Result<()> {
    if idle_minutes < 0 {
        anyhow::bail!("--idle-minutes must be >= 0");
    }
    if max_per_run == 0 {
        anyhow::bail!("--max-per-run must be > 0 (0 would silently no-op)");
    }
    if coordinator.trim().is_empty() {
        anyhow::bail!("--coordinator must be non-empty (e.g. google, claude, codex)");
    }
    let cutoff_seconds = idle_minutes * 60;

    // Eligible WRs: distinct war_room_ids with rows in execution_logs
    // whose MOST-RECENT row is older than the cutoff AND there's no
    // closed war_rooms row covering it yet. We sort by oldest-idle
    // first so a backlog drains FIFO instead of LIFO.
    let mut stmt = conn.prepare(
        "SELECT war_room_id, COUNT(*) AS seats, MAX(created_at) AS last_at
           FROM execution_logs
          WHERE war_room_id IS NOT NULL
            AND war_room_id NOT IN (
                SELECT id FROM war_rooms WHERE status = 'closed'
            )
          GROUP BY war_room_id
          HAVING (julianday('now') - julianday(MAX(created_at))) * 86400 > ?1
          ORDER BY last_at ASC
          LIMIT ?2",
    )?;
    let rows: Vec<(String, i64, String)> = stmt
        .query_map(rusqlite::params![cutoff_seconds, max_per_run as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);

    // ── DRY RUN ──────────────────────────────────────────────────────
    // Human mode: stream lines as we go (familiar CLI feel).
    // JSON mode: emit ONE final envelope with the candidate list, so
    // downstream tooling can `jq .candidates[]` without dealing with
    // multiple JSON documents on a single stdout.
    if dry_run {
        if opts.human {
            emit_human(&format!(
                "Found {} eligible war-room(s) (idle ≥ {}m).",
                rows.len(),
                idle_minutes
            ));
            for (id, seats, last_at) in &rows {
                emit_human(&format!(
                    "  [DRY-RUN] would close {} ({} seats; last activity {})",
                    id, seats, last_at
                ));
            }
        } else {
            let candidates: Vec<_> = rows
                .iter()
                .map(|(id, seats, last_at)| {
                    serde_json::json!({
                        "war_room_id": id,
                        "seats": seats,
                        "last_at": last_at,
                    })
                })
                .collect();
            emit_json(&serde_json::json!({
                "action": "sweep",
                "dry_run": true,
                "idle_minutes": idle_minutes,
                "coordinator": coordinator,
                "eligible": rows.len(),
                "candidates": candidates,
            }))?;
        }
        return Ok(());
    }

    // ── LIVE RUN ─────────────────────────────────────────────────────
    // Human mode prints a header + per-WR line + final summary.
    // JSON mode accumulates per-WR results into one final envelope.
    if opts.human {
        emit_human(&format!(
            "Found {} eligible war-room(s) (idle ≥ {}m).",
            rows.len(),
            idle_minutes
        ));
    }

    let mut closed_records: Vec<serde_json::Value> = Vec::new();
    let mut failed_records: Vec<serde_json::Value> = Vec::new();

    for (id, _seats, _last_at) in rows {
        match close_returning(
            conn,
            &id,
            None,
            None,
            Some(coordinator.to_string()),
            None,
            false,
            opts,
        ) {
            Ok((target, fields)) => {
                if opts.human {
                    emit_human(&format!(
                        "  ✓ closed {} ({} seats) — {}",
                        target.id,
                        target.seat_count,
                        fields.auto_title.as_deref().unwrap_or("(no title)")
                    ));
                } else {
                    closed_records.push(serde_json::json!({
                        "id": target.id,
                        "seat_count": target.seat_count,
                        "auto_title": fields.auto_title,
                        "summary": fields.summary,
                        "tags": fields.tags,
                        "coordinator_runtime": fields.coordinator_runtime,
                        "coordinator_model": fields.coordinator_model,
                        "duration_ms": fields.duration_ms,
                        "closed_at": fields.closed_at,
                    }));
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                if opts.human {
                    emit_human(&format!("  ✗ {} — {}", id, err_str));
                } else {
                    failed_records.push(serde_json::json!({
                        "war_room_id": id,
                        "error": err_str,
                    }));
                }
            }
        }
    }

    if opts.human {
        emit_human(&format!(
            "Sweep done. Closed {} war-room(s); {} failure(s).",
            closed_records.len(),
            failed_records.len()
        ));
    } else {
        emit_json(&serde_json::json!({
            "action": "sweep",
            "dry_run": false,
            "idle_minutes": idle_minutes,
            "coordinator": coordinator,
            "closed": closed_records.len(),
            "failed": failed_records.len(),
            "closed_records": closed_records,
            "failed_records": failed_records,
        }))?;
    }
    Ok(())
}

/// Reopen a closed war room.
pub fn reopen(conn: &Connection, id: &str, opts: &Opts) -> Result<()> {
    let target = lookup(conn, id)?;
    reopen_conversation(conn, &target)?;
    if opts.human {
        println!("Reopened war room {}.", id);
    } else {
        println!(
            "{}",
            serde_json::json!({
                "id": id,
                "action": "reopened",
                "status": "open",
            })
        );
    }
    let _ = opts;
    Ok(())
}

/// Print the war room snapshot (for `ato war-rooms get <id>`).
pub fn get(conn: &Connection, id: &str, opts: &Opts) -> Result<()> {
    let target = lookup(conn, id)?;
    if opts.human {
        println!(
            "War room {}\n  status: {}\n  seats: {}\n  title: {}\n  summary: {}",
            target.id,
            target.status,
            target.seat_count,
            target.auto_title.as_deref().unwrap_or("(none)"),
            target.summary.as_deref().unwrap_or("(none)"),
        );
    } else {
        println!("{}", serde_json::to_string(&target)?);
    }
    Ok(())
}

fn emit_human_close(target: &WarRoom, fields: &CloseFields) {
    println!(
        "Closed war room {} ({} seats).\n  title: {}\n  summary: {}\n  tags: {}\n  coordinator: {}{}",
        target.id,
        target.seat_count,
        fields.auto_title.as_deref().unwrap_or("(none)"),
        fields.summary.as_deref().unwrap_or("(none)"),
        if fields.tags.is_empty() {
            "(none)".to_string()
        } else {
            fields.tags.join(", ")
        },
        fields.coordinator_runtime,
        fields
            .human_comment
            .as_deref()
            .map(|c| format!("\n  human note: {}", c))
            .unwrap_or_default(),
    );
}

fn emit_json_close(target: &WarRoom, fields: &CloseFields) -> Result<()> {
    // v2.7.14 — camelCase keys to match the getter (`get_war_room`)
    // which got `rename_all = "camelCase"` in commit 737a3c6. Pre-fix
    // the getter and close-response had ASYMMETRIC shapes (getter
    // camelCase, close response snake_case) — claude + minimax X2
    // flagged the foot-gun for future contributors. Now consistent.
    let payload = serde_json::json!({
        "id": target.id,
        "kind": "war_room",
        "status": "closed",
        "seatCount": target.seat_count,
        "autoTitle": fields.auto_title,
        "summary": fields.summary,
        "tags": fields.tags,
        "category": fields.category,
        "team": fields.team,
        "projectId": fields.project_id,
        "coordinatorRuntime": fields.coordinator_runtime,
        "coordinatorModel": fields.coordinator_model,
        "humanComment": fields.human_comment,
        "durationMs": fields.duration_ms,
        "closedAt": fields.closed_at,
    });
    println!("{}", serde_json::to_string(&payload)?);
    Ok(())
}
