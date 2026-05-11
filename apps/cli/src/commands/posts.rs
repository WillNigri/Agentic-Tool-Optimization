// `ato posts add <text>` / `ato posts list [--limit N] [--kind X]` — talk
// to the activity_posts SQLite table the desktop manages.
//
// Phase 5.1 v1: add + list. Streaming `posts tail` lands in 5.2 once
// we wire SQLite-poll JSONL like `ato events watch`. The desktop and
// CLI share the type defs via the `ato-posts` crate.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{anyhow, Context, Result};
use ato_posts::{validate_author_slug, validate_text, CreatePostInput, Post, PostAuthorKind, PostKind};
use rusqlite::{Connection, OptionalExtension};

fn parse_author_kind(s: &str) -> Result<PostAuthorKind> {
    match s {
        "human" => Ok(PostAuthorKind::Human),
        "agent" => Ok(PostAuthorKind::Agent),
        "system" => Ok(PostAuthorKind::System),
        other => Err(anyhow!(
            "unknown --as kind '{}' (expected human|agent|system)",
            other
        )),
    }
}

fn parse_kind(s: &str) -> Result<PostKind> {
    match s {
        "message" => Ok(PostKind::Message),
        "event_notice" => Ok(PostKind::EventNotice),
        "approval_request" => Ok(PostKind::ApprovalRequest),
        "approval_decision" => Ok(PostKind::ApprovalDecision),
        other => Err(anyhow!(
            "unknown --kind '{}' (expected message|event_notice|approval_request|approval_decision)",
            other
        )),
    }
}

fn author_kind_str(k: PostAuthorKind) -> &'static str {
    match k {
        PostAuthorKind::Human => "human",
        PostAuthorKind::Agent => "agent",
        PostAuthorKind::System => "system",
    }
}

fn kind_str(k: PostKind) -> &'static str {
    match k {
        PostKind::Message => "message",
        PostKind::EventNotice => "event_notice",
        PostKind::ApprovalRequest => "approval_request",
        PostKind::ApprovalDecision => "approval_decision",
    }
}

pub fn add(
    conn: &Connection,
    text: String,
    author_kind: String,
    author_slug: Option<String>,
    kind: String,
    related_event_seq: Option<i64>,
    opts: &Opts,
) -> Result<()> {
    validate_text(&text).map_err(|e| anyhow!(e))?;
    if let Some(s) = &author_slug {
        validate_author_slug(s).map_err(|e| anyhow!(e))?;
    }
    let author_kind_parsed = parse_author_kind(&author_kind)?;
    let kind_parsed = parse_kind(&kind)?;

    if !has_table(conn) {
        return Err(anyhow!(
            "activity_posts table not found. Launch the ATO desktop (v2.3.16+) once to apply the migration."
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    let trimmed = text.trim().to_string();
    conn.execute(
        "INSERT INTO activity_posts (id, created_at, author_kind, author_slug, kind, text, related_event_seq, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
        rusqlite::params![
            id,
            created_at,
            author_kind_str(author_kind_parsed),
            author_slug,
            kind_str(kind_parsed),
            trimmed,
            related_event_seq,
        ],
    )
    .context("insert activity_posts row")?;

    let post = Post {
        id: id.clone(),
        created_at,
        author_kind: author_kind_parsed,
        author_slug,
        kind: kind_parsed,
        text: trimmed,
        related_event_seq,
        payload: None,
    };
    if opts.human {
        emit_human(&format!("Posted #{} ({}).", post.id, kind_str(post.kind)));
    } else {
        emit_json(&post)?;
    }
    Ok(())
}

pub fn list(conn: &Connection, limit: usize, kind_filter: Option<String>, opts: &Opts) -> Result<()> {
    if !has_table(conn) {
        if opts.human {
            emit_human(
                "activity_posts table not found. Launch the ATO desktop (v2.3.16+) once to apply the migration.",
            );
        } else {
            emit_json(&Vec::<Post>::new())?;
        }
        return Ok(());
    }
    let kind_parsed: Option<PostKind> = match kind_filter {
        Some(s) => Some(parse_kind(&s)?),
        None => None,
    };
    let safe_limit = limit.min(10_000) as i64;
    let mut stmt = conn
        .prepare(
            "SELECT id, created_at, author_kind, author_slug, kind, text, related_event_seq, payload
               FROM activity_posts
              WHERE (?1 IS NULL OR kind = ?1)
              ORDER BY created_at DESC
              LIMIT ?2",
        )
        .context("prepare posts list query")?;
    let kind_str_opt: Option<&str> = kind_parsed.map(kind_str);
    let mut rows = stmt.query(rusqlite::params![kind_str_opt, safe_limit])?;
    let mut out: Vec<Post> = Vec::new();
    while let Some(r) = rows.next()? {
        let ak: String = r.get(2)?;
        let k: String = r.get(4)?;
        let payload_s: Option<String> = r.get(7)?;
        out.push(Post {
            id: r.get(0)?,
            created_at: r.get(1)?,
            author_kind: parse_author_kind(&ak).map_err(|e| anyhow!(e))?,
            author_slug: r.get(3)?,
            kind: parse_kind(&k).map_err(|e| anyhow!(e))?,
            text: r.get(5)?,
            related_event_seq: r.get(6)?,
            payload: parse_payload(payload_s.as_deref())?,
        });
    }
    if opts.human {
        if out.is_empty() {
            emit_human("No activity yet. Try `ato posts add 'hello world'` to post one.");
        } else {
            emit_human(&format!("{} posts:", out.len()));
            for p in &out {
                let author = match &p.author_slug {
                    Some(s) => format!("{} @{}", author_kind_str(p.author_kind), s),
                    None => author_kind_str(p.author_kind).to_string(),
                };
                emit_human(&format!(
                    "  [{}] {} — {}: {}",
                    kind_str(p.kind),
                    p.created_at,
                    author,
                    one_line(&p.text)
                ));
            }
        }
    } else {
        emit_json(&out)?;
    }
    Ok(())
}

/// Render a (validated) post body as a single safe line for `--human`
/// output. Codex round-1 5.1 caught that raw text with embedded \n
/// would collapse the list layout. validate_text now allows \n but
/// rejects other control characters; here we render \n as " ⏎ " and
/// truncate at 200 chars so long posts don't dominate the screen.
fn one_line(text: &str) -> String {
    let collapsed = text.replace('\n', " ⏎ ");
    if collapsed.chars().count() <= 200 {
        collapsed
    } else {
        let mut s: String = collapsed.chars().take(200).collect();
        s.push('…');
        s
    }
}

fn has_table(conn: &Connection) -> bool {
    let c: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='activity_posts'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    c > 0
}

pub fn get(conn: &Connection, id: &str, opts: &Opts) -> Result<()> {
    if !has_table(conn) {
        return Err(anyhow!(
            "activity_posts table not found. Launch the ATO desktop (v2.3.16+) once to apply the migration."
        ));
    }
    // Codex round-1 5.2: hard-fail on unknown enum values rather
    // than silently downgrading to System/Message. The other readers
    // (list, tail, desktop storage) already do this; making get
    // consistent prevents a corrupted-row reader from lying.
    let raw: Option<(String, String, String, Option<String>, String, String, Option<i64>, Option<String>)> = conn
        .query_row(
            "SELECT id, created_at, author_kind, author_slug, kind, text, related_event_seq, payload
               FROM activity_posts WHERE id = ?1",
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
                    r.get(7)?,
                ))
            },
        )
        .optional()
        .context("query post")?;
    let row: Option<Post> = match raw {
        Some((id, created_at, ak, slug, k, text, related, payload_s)) => Some(Post {
            id,
            created_at,
            author_kind: parse_author_kind(&ak).map_err(|e| anyhow!(e))?,
            author_slug: slug,
            kind: parse_kind(&k).map_err(|e| anyhow!(e))?,
            text,
            related_event_seq: related,
            payload: parse_payload(payload_s.as_deref())?,
        }),
        None => None,
    };
    match row {
        Some(p) => {
            if opts.human {
                let author = match &p.author_slug {
                    Some(s) => format!("{} @{}", author_kind_str(p.author_kind), s),
                    None => author_kind_str(p.author_kind).to_string(),
                };
                emit_human(&format!(
                    "Post {}\n  created_at: {}\n  author: {}\n  kind: {}\n  related_event_seq: {}\n  text: {}",
                    p.id,
                    p.created_at,
                    author,
                    kind_str(p.kind),
                    p.related_event_seq
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "—".to_string()),
                    p.text
                ));
            } else {
                emit_json(&p)?;
            }
            Ok(())
        }
        None => Err(anyhow!("No post with id '{}'.", id)),
    }
}

/// Tail new posts as JSONL on stdout. Mirrors `ato events watch` —
/// same retry-on-transient-error pattern, same de-duped stderr
/// logging, same JSONL-on-stdout output.
///
/// Cursor: composite (created_at, id). Codex round-1 5.2 caught that
/// a plain created_at watermark would PERMANENTLY drop rows sharing
/// the previous emit's timestamp. created_at comes from
/// `Utc::now().to_rfc3339()`, not a DB-generated monotonic value, so
/// ties depend on clock granularity + scheduling. With a composite
/// cursor and `WHERE created_at > ?1 OR (created_at = ?1 AND id > ?2)`
/// every row is reachable.
#[derive(Clone)]
struct Cursor {
    created_at: String,
    id: String,
}

pub fn tail(
    db_path: &std::path::PathBuf,
    kind_filter: Option<String>,
    since_id: Option<String>,
    max_rows: Option<usize>,
    poll_ms: u64,
    opts: &Opts,
) -> Result<()> {
    let poll_ms = poll_ms.clamp(100, 5_000);
    let interval = std::time::Duration::from_millis(poll_ms);
    let kind_parsed: Option<PostKind> = match kind_filter {
        Some(s) => Some(parse_kind(&s)?),
        None => None,
    };

    // Bootstrap cursor. --since-id -> use that post's (created_at,
    // id); otherwise default to the (created_at, id) of the row at
    // MAX(created_at), so new posts (later created_at or equal+later
    // id) start streaming from then on.
    let mut cursor: Cursor = loop {
        match crate::db::open_readonly(db_path) {
            Ok(conn) => {
                if !has_table(&conn) {
                    if opts.human {
                        emit_human(
                            "activity_posts table not found. Launch the ATO desktop (v2.3.16+) once to apply the migration.",
                        );
                    }
                    return Ok(());
                }
                if let Some(id) = since_id.as_ref() {
                    let row: Option<(String, String)> = conn
                        .query_row(
                            "SELECT created_at, id FROM activity_posts WHERE id = ?1",
                            [id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    match row {
                        Some((ts, id)) => break Cursor { created_at: ts, id },
                        None => {
                            return Err(anyhow!(
                                "No post with id '{}' to start streaming after.",
                                id
                            ))
                        }
                    }
                }
                // tail-f: latest existing row at boot, or epoch sentinel if empty.
                let row: Option<(String, String)> = conn
                    .query_row(
                        "SELECT created_at, id FROM activity_posts ORDER BY created_at DESC, id DESC LIMIT 1",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()
                    .unwrap_or(None);
                break match row {
                    Some((ts, id)) => Cursor { created_at: ts, id },
                    None => Cursor {
                        created_at: "1970-01-01T00:00:00+00:00".to_string(),
                        id: String::new(),
                    },
                };
            }
            Err(_) => {
                std::thread::sleep(interval);
                continue;
            }
        }
    };

    if opts.human {
        emit_human(&format!(
            "Tailing activity_posts after ({}, {}) (poll {}ms). Ctrl-C to stop.",
            cursor.created_at, cursor.id, poll_ms
        ));
    }

    let mut emitted: usize = 0;
    let mut last_error_msg: Option<String> = None;
    loop {
        let conn = match crate::db::open_readonly(db_path) {
            Ok(c) => c,
            Err(e) => {
                let msg = e.to_string();
                if last_error_msg.as_ref() != Some(&msg) {
                    eprintln!("ato posts tail: open error (will retry): {}", msg);
                    last_error_msg = Some(msg);
                }
                std::thread::sleep(interval);
                continue;
            }
        };
        let new_rows = match fetch_new(&conn, &cursor, kind_parsed) {
            Ok(rows) => {
                last_error_msg = None;
                rows
            }
            Err(e) => {
                let msg = e.to_string();
                if last_error_msg.as_ref() != Some(&msg) {
                    eprintln!("ato posts tail: fetch error (will retry): {}", msg);
                    last_error_msg = Some(msg);
                }
                drop(conn);
                std::thread::sleep(interval);
                continue;
            }
        };
        for row in new_rows {
            cursor = Cursor {
                created_at: row.created_at.clone(),
                id: row.id.clone(),
            };
            if opts.human {
                let author = match &row.author_slug {
                    Some(s) => format!("{} @{}", author_kind_str(row.author_kind), s),
                    None => author_kind_str(row.author_kind).to_string(),
                };
                emit_human(&format!(
                    "  [{}] {} — {}: {}",
                    kind_str(row.kind),
                    row.created_at,
                    author,
                    one_line(&row.text)
                ));
            } else {
                let line = serde_json::to_string(&row)
                    .unwrap_or_else(|_| String::from("{}"));
                println!("{}", line);
            }
            emitted += 1;
            if let Some(cap) = max_rows {
                if emitted >= cap {
                    return Ok(());
                }
            }
        }
        drop(conn);
        std::thread::sleep(interval);
    }
}

fn fetch_new(
    conn: &Connection,
    cursor: &Cursor,
    kind_filter: Option<PostKind>,
) -> Result<Vec<Post>> {
    let mut out: Vec<Post> = Vec::new();
    let kind_str_opt: Option<&str> = kind_filter.map(kind_str);
    let mut stmt = conn
        .prepare(
            "SELECT id, created_at, author_kind, author_slug, kind, text, related_event_seq, payload
               FROM activity_posts
              WHERE (created_at > ?1 OR (created_at = ?1 AND id > ?2))
                AND (?3 IS NULL OR kind = ?3)
              ORDER BY created_at ASC, id ASC
              LIMIT 500",
        )
        .context("prepare posts tail query")?;
    let mut rows = stmt.query(rusqlite::params![
        &cursor.created_at,
        &cursor.id,
        kind_str_opt
    ])?;
    while let Some(r) = rows.next()? {
        let ak: String = r.get(2)?;
        let k: String = r.get(4)?;
        let payload_s: Option<String> = r.get(7)?;
        out.push(Post {
            id: r.get(0)?,
            created_at: r.get(1)?,
            author_kind: parse_author_kind(&ak).map_err(|e| anyhow!(e))?,
            author_slug: r.get(3)?,
            kind: parse_kind(&k).map_err(|e| anyhow!(e))?,
            text: r.get(5)?,
            related_event_seq: r.get(6)?,
            payload: parse_payload(payload_s.as_deref())?,
        });
    }
    Ok(out)
}

/// Decode the optional `payload` column. Codex round-2 5.2: previously
/// `.and_then(serde_json::from_str(...).ok())` silently dropped
/// malformed payloads to None, while desktop storage hard-fails on
/// the same row. Now consistent: NULL → None, valid JSON → Some(v),
/// invalid JSON → Err.
fn parse_payload(s: Option<&str>) -> Result<Option<serde_json::Value>> {
    match s {
        Some(raw) => Ok(Some(
            serde_json::from_str::<serde_json::Value>(raw)
                .context("payload column is not valid JSON")?,
        )),
        None => Ok(None),
    }
}

// ─── Approval flow (Phase 5.3) ────────────────────────────────────────

/// Write an ApprovalDecision post linking to the given ApprovalRequest.
/// Validates: the target row exists, is kind=ApprovalRequest, and has
/// no prior ApprovalDecision in payload.request_post_id pointing at
/// it (one decision per request). Decision payload format:
///   { "request_post_id": "<id>", "decision": "approved"|"denied",
///     "notes": "<text>" }
pub fn decide(
    conn: &Connection,
    request_id: &str,
    approved: bool,
    notes: Option<String>,
    opts: &Opts,
) -> Result<()> {
    if !has_table(conn) {
        return Err(anyhow!(
            "activity_posts table not found. Launch the ATO desktop (v2.3.16+) once to apply the migration."
        ));
    }
    // Codex round-2 5.3: the unique index is only created in the
    // desktop's init_database. If the user runs `ato posts approve`
    // on a DB that hasn't been migrated yet (CLI upgraded but desktop
    // not relaunched after upgrade), the storage-layer race
    // protection is missing. Ensure it idempotently here on every
    // decide call. Surface failures (duplicates from before this
    // protection landed) instead of silently downgrading.
    ensure_decision_unique_index(conn).context(
        "approval decisions rely on a partial UNIQUE index — its creation failed (existing duplicates?)",
    )?;
    // Validate target.
    let target_kind: Option<String> = conn
        .query_row(
            "SELECT kind FROM activity_posts WHERE id = ?1",
            [request_id],
            |r| r.get(0),
        )
        .optional()
        .context("look up target post")?;
    let target_kind = target_kind.ok_or_else(|| {
        anyhow!("No post with id '{}' to approve / deny.", request_id)
    })?;
    if target_kind != "approval_request" {
        return Err(anyhow!(
            "Post '{}' is kind={}, not approval_request — cannot decide.",
            request_id,
            target_kind
        ));
    }
    if has_existing_decision(conn, request_id)? {
        return Err(anyhow!(
            "Post '{}' already has an ApprovalDecision — refusing to write a second."
        , request_id));
    }

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
    validate_text(&body).map_err(|e| anyhow!(e))?;

    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    let payload_str = serde_json::to_string(&payload).context("serialize decision payload")?;
    // The partial unique index on (kind='approval_decision',
    // json_extract(payload, '$.request_post_id')) catches the
    // race between has_existing_decision and the INSERT (codex 5.3
    // round-1 medium). On collision we surface the same logical
    // error the pre-flight check would have.
    if let Err(e) = conn.execute(
        "INSERT INTO activity_posts (id, created_at, author_kind, author_slug, kind, text, related_event_seq, payload)
         VALUES (?1, ?2, 'human', NULL, 'approval_decision', ?3, NULL, ?4)",
        rusqlite::params![id, created_at, body, payload_str],
    ) {
        let msg = e.to_string();
        if msg.contains("UNIQUE constraint") {
            return Err(anyhow!(
                "Post '{}' already has an ApprovalDecision (race detected by storage).",
                request_id
            ));
        }
        return Err(e).context("insert ApprovalDecision row");
    }

    let post = Post {
        id: id.clone(),
        created_at,
        author_kind: PostAuthorKind::Human,
        author_slug: None,
        kind: PostKind::ApprovalDecision,
        text: body,
        related_event_seq: None,
        payload: Some(payload),
    };
    if opts.human {
        emit_human(&format!(
            "Wrote ApprovalDecision #{} ({}) for request '{}'.",
            post.id, decision_str, request_id
        ));
    } else {
        emit_json(&post)?;
    }
    Ok(())
}

/// Create the partial UNIQUE index that enforces one
/// ApprovalDecision per ApprovalRequest. Idempotent under
/// `IF NOT EXISTS`. Returns Err if creation fails (typically
/// pre-existing duplicates that block the constraint).
fn ensure_decision_unique_index(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_activity_posts_decision_request
            ON activity_posts(json_extract(payload, '$.request_post_id'))
          WHERE kind = 'approval_decision'",
        [],
    )
    .map(|_| ())
    .context("create idx_activity_posts_decision_request")
}

fn has_existing_decision(conn: &Connection, request_id: &str) -> Result<bool> {
    // SQLite's json_extract works on the payload TEXT. The string is
    // a JSON object so this is the cleanest check; no need to load
    // all decisions into memory and parse them client-side.
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM activity_posts
              WHERE kind = 'approval_decision'
                AND json_extract(payload, '$.request_post_id') = ?1",
            [request_id],
            |r| r.get(0),
        )
        .context("check for existing decision")?;
    Ok(count > 0)
}

/// List ApprovalRequest posts that don't yet have a matching
/// ApprovalDecision. Newest pending first.
pub fn pending(conn: &Connection, limit: usize, opts: &Opts) -> Result<()> {
    if !has_table(conn) {
        if opts.human {
            emit_human("activity_posts table not found. Launch the ATO desktop (v2.3.16+) once.");
        } else {
            emit_json(&Vec::<Post>::new())?;
        }
        return Ok(());
    }
    let safe_limit = limit.min(10_000) as i64;
    // Codex round-1 5.3: NOT IN with json_extract is brittle —
    // a single NULL from a malformed payload (or a decision row
    // shaped differently in the future) propagates NULL semantics
    // and makes the predicate evaluate to unknown for every
    // candidate row. NOT EXISTS is the safer form because it
    // checks for the existence of a row matching the inner
    // condition explicitly, with no NULL-collapsing arithmetic.
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
        .context("prepare pending approvals query")?;
    let mut rows = stmt.query(rusqlite::params![safe_limit])?;
    let mut out: Vec<Post> = Vec::new();
    while let Some(r) = rows.next()? {
        let ak: String = r.get(2)?;
        let k: String = r.get(4)?;
        let payload_s: Option<String> = r.get(7)?;
        out.push(Post {
            id: r.get(0)?,
            created_at: r.get(1)?,
            author_kind: parse_author_kind(&ak).map_err(|e| anyhow!(e))?,
            author_slug: r.get(3)?,
            kind: parse_kind(&k).map_err(|e| anyhow!(e))?,
            text: r.get(5)?,
            related_event_seq: r.get(6)?,
            payload: parse_payload(payload_s.as_deref())?,
        });
    }
    if opts.human {
        if out.is_empty() {
            emit_human("No pending approvals.");
        } else {
            emit_human(&format!("{} pending approvals:", out.len()));
            for p in &out {
                let author = match &p.author_slug {
                    Some(s) => format!("{} @{}", author_kind_str(p.author_kind), s),
                    None => author_kind_str(p.author_kind).to_string(),
                };
                emit_human(&format!(
                    "  {} — {}: {}",
                    p.id,
                    author,
                    one_line(&p.text)
                ));
            }
        }
    } else {
        emit_json(&out)?;
    }
    Ok(())
}
