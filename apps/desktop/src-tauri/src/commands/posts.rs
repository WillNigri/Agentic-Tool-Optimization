// commands/posts.rs — activity-feed posts (v2.3.20 Phase 5.5).
//
// PR 5 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md). Thin
// Tauri wrappers around crate::posts::*; heavy validation lives in
// the ato-posts shared crate. These commands just thread DB
// connections, translate errors, and emit a `activity_posts:new`
// event so the feed pane can refresh sub-100ms without waiting for
// the 1s poll.
//
// Scope:
//   - `posts_list`           — paged feed list, optional kind filter
//   - `posts_create`         — human/agent/system post (validated)
//   - `posts_pending`        — approval_requests without a decision
//   - `posts_decide`         — approve/deny an approval_request
//   - `posts_pending_count`  — count for the sidebar badge
//
// Out of scope (stays in commands/mod.rs):
//   - `prompt_api_provider`  — sessions_dispatch domain, merged into
//     `agents` per the codex Round 1 revision; lands in PR 28.

use tauri::Emitter as _;

use crate::posts::{Post, PostAuthorKind, PostKind};

#[tauri::command]
pub fn posts_list(
    limit: usize,
    kind: Option<String>,
) -> Result<Vec<Post>, String> {
    let conn = rusqlite::Connection::open(crate::get_db_path()).map_err(|e| e.to_string())?;
    let kind_parsed = match kind.as_deref() {
        Some("message") => Some(PostKind::Message),
        Some("event_notice") => Some(PostKind::EventNotice),
        Some("approval_request") => Some(PostKind::ApprovalRequest),
        Some("approval_decision") => Some(PostKind::ApprovalDecision),
        Some(other) => return Err(format!("unknown kind '{}'", other)),
        None => None,
    };
    crate::posts::list(&conn, limit, kind_parsed)
}

#[tauri::command]
pub fn posts_create(
    app: tauri::AppHandle,
    text: String,
    author_kind: String,
    author_slug: Option<String>,
    kind: Option<String>,
) -> Result<Post, String> {
    let conn = rusqlite::Connection::open(crate::get_db_path()).map_err(|e| e.to_string())?;
    let ak = match author_kind.as_str() {
        "human" => PostAuthorKind::Human,
        "agent" => PostAuthorKind::Agent,
        "system" => PostAuthorKind::System,
        other => return Err(format!("unknown author_kind '{}'", other)),
    };
    let k = match kind.as_deref().unwrap_or("message") {
        "message" => PostKind::Message,
        "event_notice" => PostKind::EventNotice,
        "approval_request" => PostKind::ApprovalRequest,
        "approval_decision" => PostKind::ApprovalDecision,
        other => return Err(format!("unknown kind '{}'", other)),
    };
    let input = crate::posts::CreatePostInput {
        author_kind: ak,
        author_slug,
        kind: k,
        text,
        related_event_seq: None,
        payload: None,
    };
    let post = crate::posts::create(&conn, input)?;
    // v2.3.24 Phase 5.6 — emit a Tauri event so the feed pane can
    // refresh sub-100ms instead of waiting for the 1s poll. Background
    // post creation (NotifyHuman / RequestApproval / approval-resume
    // watcher) still relies on the poll for now; the GUI-driven path
    // gets the snappy refresh.
    let _ = app.emit("activity_posts:new", &post);
    Ok(post)
}

#[tauri::command]
pub fn posts_pending(limit: usize) -> Result<Vec<Post>, String> {
    let conn = rusqlite::Connection::open(crate::get_db_path()).map_err(|e| e.to_string())?;
    let safe_limit = limit.min(10_000) as i64;
    // Mirrors the CLI's posts::pending — NOT EXISTS form, immune to
    // NULL semantics. Kept inline rather than refactored because the
    // CLI's `commands::posts::pending` is opts/output-coupled.
    let mut stmt = conn
        .prepare(
            "SELECT id, created_at, author_kind, author_slug, kind, text, related_event_seq, payload
               FROM activity_posts AS req
              WHERE req.kind = 'approval_request'
                AND NOT EXISTS (
                  SELECT 1 FROM activity_posts AS d
                   WHERE d.kind = 'approval_decision'
                     AND json_extract(d.payload, '$.request_post_id') = req.id
                )
              ORDER BY req.created_at DESC
              LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(rusqlite::params![safe_limit])
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().map_err(|e| e.to_string())? {
        let ak: String = r.get(2).map_err(|e| e.to_string())?;
        let k: String = r.get(4).map_err(|e| e.to_string())?;
        let payload_s: Option<String> = r.get(7).map_err(|e| e.to_string())?;
        let payload = match payload_s {
            Some(s) => Some(serde_json::from_str(&s).map_err(|e| format!("payload not valid JSON: {}", e))?),
            None => None,
        };
        let author_kind = match ak.as_str() {
            "human" => PostAuthorKind::Human,
            "agent" => PostAuthorKind::Agent,
            "system" => PostAuthorKind::System,
            other => return Err(format!("unknown author_kind '{}' in row", other)),
        };
        let post_kind = match k.as_str() {
            "message" => PostKind::Message,
            "event_notice" => PostKind::EventNotice,
            "approval_request" => PostKind::ApprovalRequest,
            "approval_decision" => PostKind::ApprovalDecision,
            other => return Err(format!("unknown kind '{}' in row", other)),
        };
        out.push(Post {
            id: r.get(0).map_err(|e| e.to_string())?,
            created_at: r.get(1).map_err(|e| e.to_string())?,
            author_kind,
            author_slug: r.get(3).map_err(|e| e.to_string())?,
            kind: post_kind,
            text: r.get(5).map_err(|e| e.to_string())?,
            related_event_seq: r.get(6).map_err(|e| e.to_string())?,
            payload,
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn posts_decide(
    app: tauri::AppHandle,
    request_id: String,
    approved: bool,
    notes: Option<String>,
) -> Result<Post, String> {
    let conn = rusqlite::Connection::open(crate::get_db_path()).map_err(|e| e.to_string())?;
    // Validate target.
    let target_kind: Option<String> = conn
        .query_row(
            "SELECT kind FROM activity_posts WHERE id = ?1",
            [&request_id],
            |r| r.get(0),
        )
        .ok();
    let target_kind = target_kind.ok_or_else(|| {
        format!("No post with id '{}' to approve / deny.", request_id)
    })?;
    if target_kind != "approval_request" {
        return Err(format!(
            "Post '{}' is kind={}, not approval_request — cannot decide.",
            request_id, target_kind
        ));
    }
    // Ensure the storage-layer race protection is present (the same
    // partial UNIQUE index the CLI installs in ensure_decision_unique_index).
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_activity_posts_decision_request
            ON activity_posts(json_extract(payload, '$.request_post_id'))
          WHERE kind = 'approval_decision'",
        [],
    )
    .map_err(|e| format!("ensure decision unique index: {}", e))?;

    let decision_str = if approved { "approved" } else { "denied" };
    let payload = serde_json::json!({
        "request_post_id": request_id,
        "decision": decision_str,
        "notes": notes,
    });
    let body = match &notes {
        Some(n) if !n.trim().is_empty() => format!("{} (notes: {})", decision_str, n.trim()),
        _ => decision_str.to_string(),
    };
    crate::posts::validate_text(&body).map_err(|e| e.to_string())?;

    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    let payload_str = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    if let Err(e) = conn.execute(
        "INSERT INTO activity_posts (id, created_at, author_kind, author_slug, kind, text, related_event_seq, payload)
         VALUES (?1, ?2, 'human', NULL, 'approval_decision', ?3, NULL, ?4)",
        rusqlite::params![id, created_at, body, payload_str],
    ) {
        let msg = e.to_string();
        if msg.contains("UNIQUE constraint") {
            return Err(format!(
                "Post '{}' already has an ApprovalDecision.",
                request_id
            ));
        }
        return Err(format!("insert ApprovalDecision: {}", msg));
    }
    let post = Post {
        id,
        created_at,
        author_kind: PostAuthorKind::Human,
        author_slug: None,
        kind: PostKind::ApprovalDecision,
        text: body,
        related_event_seq: None,
        payload: Some(payload),
    };
    // v2.3.24 Phase 5.6 — emit so the GUI can flip the parked
    // request out of the pending list immediately, before the next
    // poll tick. Resume watcher (5s) still handles the recipe_run
    // state transition separately.
    let _ = app.emit("activity_posts:new", &post);
    Ok(post)
}

// New Tauri command — used by the badge component on the sidebar.
// Counts only; faster than posts_pending which returns full rows.
#[tauri::command]
pub fn posts_pending_count() -> Result<i64, String> {
    let conn = rusqlite::Connection::open(crate::get_db_path()).map_err(|e| e.to_string())?;
    // Mirror posts_pending's NOT EXISTS shape so the count matches
    // the list exactly. The single-row query is cheap (indexed scan
    // over approval_requests + a NOT EXISTS probe per row).
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM activity_posts AS req
              WHERE req.kind = 'approval_request'
                AND NOT EXISTS (
                  SELECT 1 FROM activity_posts AS d
                   WHERE d.kind = 'approval_decision'
                     AND json_extract(d.payload, '$.request_post_id') = req.id
                )",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok(n)
}
