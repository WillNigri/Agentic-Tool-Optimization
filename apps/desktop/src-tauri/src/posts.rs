// v2.3.16 Phase 5.1 — Activity feed storage.
//
// SOURCE OF TRUTH: the activity_posts SQLite table.
// Types live in the ato-posts shared crate (re-exported here so other
// modules can use `crate::posts::Post` etc. without an extra import).
//
// What's here: create + list + get. No JSON mirror (posts are a
// stream, not a config), no GUI bindings yet (Phase 5.2). The
// recipes engine's NotifyHuman executor calls `create` directly.

use ato_posts::{validate_author_slug};
pub use ato_posts::{validate_text, CreatePostInput, Post, PostAuthorKind, PostKind};
use rusqlite::Connection;

type Result<T> = std::result::Result<T, String>;

fn map_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn author_kind_str(k: PostAuthorKind) -> &'static str {
    match k {
        PostAuthorKind::Human => "human",
        PostAuthorKind::Agent => "agent",
        PostAuthorKind::System => "system",
    }
}

fn parse_author_kind(s: &str) -> Result<PostAuthorKind> {
    match s {
        "human" => Ok(PostAuthorKind::Human),
        "agent" => Ok(PostAuthorKind::Agent),
        "system" => Ok(PostAuthorKind::System),
        other => Err(format!("unknown author_kind '{}' in activity_posts row", other)),
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

fn parse_kind(s: &str) -> Result<PostKind> {
    match s {
        "message" => Ok(PostKind::Message),
        "event_notice" => Ok(PostKind::EventNotice),
        "approval_request" => Ok(PostKind::ApprovalRequest),
        "approval_decision" => Ok(PostKind::ApprovalDecision),
        other => Err(format!("unknown kind '{}' in activity_posts row", other)),
    }
}

pub fn create(conn: &Connection, input: CreatePostInput) -> Result<Post> {
    validate_text(&input.text)?;
    if let Some(slug) = &input.author_slug {
        validate_author_slug(slug)?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    let payload_str = match &input.payload {
        Some(v) => Some(serde_json::to_string(v).map_err(map_err)?),
        None => None,
    };
    conn.execute(
        "INSERT INTO activity_posts (id, created_at, author_kind, author_slug, kind, text, related_event_seq, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            id,
            created_at,
            author_kind_str(input.author_kind),
            input.author_slug,
            kind_str(input.kind),
            input.text.trim(),
            input.related_event_seq,
            payload_str,
        ],
    )
    .map_err(map_err)?;
    Ok(Post {
        id,
        created_at,
        author_kind: input.author_kind,
        author_slug: input.author_slug,
        kind: input.kind,
        text: input.text.trim().to_string(),
        related_event_seq: input.related_event_seq,
        payload: input.payload,
    })
}

pub fn list(conn: &Connection, limit: usize, kind_filter: Option<PostKind>) -> Result<Vec<Post>> {
    let safe_limit = limit.min(10_000) as i64;
    let mut stmt = conn
        .prepare(
            "SELECT id, created_at, author_kind, author_slug, kind, text, related_event_seq, payload
               FROM activity_posts
              WHERE (?1 IS NULL OR kind = ?1)
              ORDER BY created_at DESC
              LIMIT ?2",
        )
        .map_err(map_err)?;
    let kind_str_opt: Option<&str> = kind_filter.map(kind_str);
    let mut rows = stmt
        .query(rusqlite::params![kind_str_opt, safe_limit])
        .map_err(map_err)?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().map_err(map_err)? {
        let author_kind_s: String = r.get(2).map_err(map_err)?;
        let kind_s: String = r.get(4).map_err(map_err)?;
        let payload_s: Option<String> = r.get(7).map_err(map_err)?;
        let payload = match payload_s {
            Some(s) => Some(serde_json::from_str(&s).map_err(map_err)?),
            None => None,
        };
        out.push(Post {
            id: r.get(0).map_err(map_err)?,
            created_at: r.get(1).map_err(map_err)?,
            author_kind: parse_author_kind(&author_kind_s)?,
            author_slug: r.get(3).map_err(map_err)?,
            kind: parse_kind(&kind_s)?,
            text: r.get(5).map_err(map_err)?,
            related_event_seq: r.get(6).map_err(map_err)?,
            payload,
        });
    }
    Ok(out)
}
