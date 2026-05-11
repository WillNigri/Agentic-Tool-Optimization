// `ato posts add <text>` / `ato posts list [--limit N] [--kind X]` — talk
// to the activity_posts SQLite table the desktop manages.
//
// Phase 5.1 v1: add + list. Streaming `posts tail` lands in 5.2 once
// we wire SQLite-poll JSONL like `ato events watch`. The desktop and
// CLI share the type defs via the `ato-posts` crate.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{anyhow, Context, Result};
use ato_posts::{validate_author_slug, validate_text, CreatePostInput, Post, PostAuthorKind, PostKind};
use rusqlite::Connection;

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
        let payload = match payload_s {
            Some(s) => Some(serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            None => None,
        };
        out.push(Post {
            id: r.get(0)?,
            created_at: r.get(1)?,
            author_kind: parse_author_kind(&ak).map_err(|e| anyhow!(e))?,
            author_slug: r.get(3)?,
            kind: parse_kind(&k).map_err(|e| anyhow!(e))?,
            text: r.get(5)?,
            related_event_seq: r.get(6)?,
            payload,
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
