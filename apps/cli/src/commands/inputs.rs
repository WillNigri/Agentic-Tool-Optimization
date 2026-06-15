use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use uuid::Uuid;

use crate::db;
use crate::output::{emit_human, emit_json, Opts};

const INPUTS_SELECT: &str =
    "SELECT id, slug, name, content, kind, tags, created_at, updated_at FROM inputs";
const KIND_MARKDOWN: &str = "markdown";
const KIND_JSON: &str = "json";
const KIND_TEXT: &str = "text";
const KIND_VALUES: &[&str] = &[KIND_MARKDOWN, KIND_JSON, KIND_TEXT];

#[derive(Args, Debug)]
pub struct InputsArgs {
    #[command(subcommand)]
    pub sub: InputsSub,
}

#[derive(Subcommand, Debug)]
pub enum InputsSub {
    /// Store a named input bundle. Reads stdin when --from-file is omitted
    /// or when it is explicitly set to `-`.
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        slug: Option<String>,
        #[arg(long = "from-file", value_name = "PATH")]
        from_file: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = InputKind::Markdown)]
        kind: InputKind,
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// List stored inputs, newest-updated first.
    List {
        #[arg(long = "tag")]
        tag: Option<String>,
        #[arg(long, value_enum)]
        kind: Option<InputKind>,
    },
    /// Get one input bundle by slug or id.
    Get {
        slug_or_id: String,
    },
    /// Alias for get.
    Show {
        slug_or_id: String,
    },
    /// Delete one input bundle. Requires --yes.
    Delete {
        slug_or_id: String,
        #[arg(long, default_value_t = false)]
        yes: bool,
    },
    /// Partially update an input bundle.
    Edit {
        slug_or_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "content-file", value_name = "PATH")]
        content_file: Option<PathBuf>,
        #[arg(long = "add-tag")]
        add_tag: Vec<String>,
        #[arg(long = "remove-tag")]
        remove_tag: Vec<String>,
        #[arg(long, value_enum)]
        kind: Option<InputKind>,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum InputKind {
    Markdown,
    Json,
    Text,
}

impl InputKind {
    fn as_str(self) -> &'static str {
        match self {
            InputKind::Markdown => KIND_MARKDOWN,
            InputKind::Json => KIND_JSON,
            InputKind::Text => KIND_TEXT,
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct InputRow {
    id: String,
    slug: String,
    name: String,
    content: String,
    kind: String,
    #[serde(default)]
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
}

pub fn run(args: InputsArgs, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    match args.sub {
        InputsSub::Add {
            name,
            slug,
            from_file,
            kind,
            tags,
        } => run_add(name, slug, from_file, kind, tags, db_path, opts),
        InputsSub::List { tag, kind } => run_list(tag, kind, db_path, opts),
        InputsSub::Get { slug_or_id } | InputsSub::Show { slug_or_id } => {
            run_get(slug_or_id, db_path, opts)
        }
        InputsSub::Delete { slug_or_id, yes } => run_delete(slug_or_id, yes, db_path, opts),
        InputsSub::Edit {
            slug_or_id,
            name,
            content_file,
            add_tag,
            remove_tag,
            kind,
        } => run_edit(
            slug_or_id,
            name,
            content_file,
            add_tag,
            remove_tag,
            kind,
            db_path,
            opts,
        ),
    }
}

fn run_add(
    name: String,
    slug_override: Option<String>,
    from_file: Option<PathBuf>,
    kind: InputKind,
    tags: Vec<String>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let row = create_input(&conn, name, slug_override, from_file, kind.as_str(), tags)?;
    if opts.human {
        emit_human(&render_human_row("Created input", &row, true));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

fn run_list(
    tag: Option<String>,
    kind: Option<InputKind>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let rows = list_inputs(&conn, tag.as_deref(), kind.map(InputKind::as_str))?;
    if opts.human {
        if rows.is_empty() {
            emit_human("No inputs found.");
        } else {
            for row in &rows {
                let tags = if row.tags.is_empty() {
                    "-".to_string()
                } else {
                    row.tags.join(",")
                };
                emit_human(&format!(
                    "{}  {}  kind={}  tags={}  updated={}",
                    row.slug, row.name, row.kind, tags, row.updated_at
                ));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

fn run_get(slug_or_id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let row = load_input(&conn, &slug_or_id)?;
    if opts.human {
        emit_human(&render_human_row("Input", &row, true));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

fn run_delete(slug_or_id: String, yes: bool, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    require_delete_yes(&slug_or_id, yes)?;
    let conn = db::open_readwrite(db_path)?;
    let row = load_input(&conn, &slug_or_id)?;
    let changed = conn.execute(
        &format!("DELETE FROM inputs WHERE {} = ?1", id_or_slug_column(&slug_or_id)),
        params![slug_or_id],
    )?;
    if changed == 0 {
        anyhow::bail!("input not found: {}", row.slug);
    }
    if opts.human {
        emit_human(&format!("Deleted input '{}' ({})", row.slug, row.id));
    } else {
        emit_json(&serde_json::json!({
            "deleted": true,
            "id": row.id,
            "slug": row.slug,
        }))?;
    }
    Ok(())
}

fn require_delete_yes(slug_or_id: &str, yes: bool) -> Result<()> {
    if yes {
        Ok(())
    } else {
        anyhow::bail!(
            "Refusing to delete input '{}': pass --yes to confirm.",
            slug_or_id
        )
    }
}

fn run_edit(
    slug_or_id: String,
    name: Option<String>,
    content_file: Option<PathBuf>,
    add_tag: Vec<String>,
    remove_tag: Vec<String>,
    kind: Option<InputKind>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let row = edit_input(
        &conn,
        &slug_or_id,
        name,
        content_file,
        add_tag,
        remove_tag,
        kind.map(InputKind::as_str),
    )?;
    if opts.human {
        emit_human(&render_human_row("Updated input", &row, true));
    } else {
        emit_json(&row)?;
    }
    Ok(())
}

fn create_input(
    conn: &Connection,
    name: String,
    slug_override: Option<String>,
    from_file: Option<PathBuf>,
    kind: &str,
    tags: Vec<String>,
) -> Result<InputRow> {
    validate_kind(kind)?;
    let name = validate_name(name)?;
    let tags = normalize_tags(tags);
    let content = read_content_arg(from_file)?;
    let base_slug = slug_override
        .map(|s| slugify(&s))
        .unwrap_or_else(|| slugify(&name));
    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let tags_json = tags_json_opt(&tags)?;
    // Codex 2026-06-13: insert-with-retry on UNIQUE(slug). The previous
    // SELECT-COUNT-then-INSERT shape had a race window where two
    // concurrent `ato inputs add` invocations could both pick the same
    // candidate and one would die on the constraint with a generic
    // error. The slug is the source of truth — let the DB allocate it.
    let slug = insert_with_unique_slug(conn, &id, &base_slug, &name, &content, kind, tags_json.as_deref(), &now)?;
    let _ = slug;
    load_input_by_id(conn, &id)
}

/// Insert one row, retrying with a numeric suffix when the DB rejects the
/// candidate slug. Returns the slug actually written. Caps at 1000 tries
/// to bail on pathological collision storms.
fn insert_with_unique_slug(
    conn: &Connection,
    id: &str,
    base_slug: &str,
    name: &str,
    content: &str,
    kind: &str,
    tags_json: Option<&str>,
    now: &str,
) -> Result<String> {
    // Seed the candidate optimistically; on UNIQUE collision walk the
    // suffix forward. Never reuse a suffix that's already on disk OR
    // that another concurrent writer just won.
    let mut candidate = unique_slug(conn, base_slug)?;
    let mut next_suffix: i64 = 2;
    for _ in 0..1000 {
        match conn.execute(
            "INSERT INTO inputs (id, slug, name, content, kind, tags, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, candidate, name, content, kind, tags_json, now, now],
        ) {
            Ok(_) => return Ok(candidate),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                candidate = format!("{}-{}", base_slug, next_suffix);
                next_suffix += 1;
                continue;
            }
            Err(e) => return Err(anyhow::Error::from(e).context("insert input")),
        }
    }
    anyhow::bail!("slug-exhaustion: 1000 candidates rejected by UNIQUE(slug)")
}

fn list_inputs(conn: &Connection, tag: Option<&str>, kind: Option<&str>) -> Result<Vec<InputRow>> {
    if let Some(kind) = kind {
        validate_kind(kind)?;
    }
    let rows = if let Some(kind) = kind {
        let sql = format!("{} WHERE kind = ?1 ORDER BY updated_at DESC", INPUTS_SELECT);
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![kind], row_to_input)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    } else {
        let sql = format!("{} ORDER BY updated_at DESC", INPUTS_SELECT);
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], row_to_input)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    let filtered = match tag {
        Some(tag) => rows
            .into_iter()
            .filter(|row| row.tags.iter().any(|t| t == tag))
            .collect(),
        None => rows,
    };
    Ok(filtered)
}

fn edit_input(
    conn: &Connection,
    slug_or_id: &str,
    name: Option<String>,
    content_file: Option<PathBuf>,
    add_tag: Vec<String>,
    remove_tag: Vec<String>,
    kind: Option<&str>,
) -> Result<InputRow> {
    if let Some(kind) = kind {
        validate_kind(kind)?;
    }

    let current = load_input(conn, slug_or_id)?;
    let mut next_name = current.name.clone();
    let mut next_content = current.content.clone();
    let mut next_kind = current.kind.clone();
    let mut next_tags = current.tags.clone();

    if let Some(name) = name {
        next_name = validate_name(name)?;
    }
    if let Some(path) = content_file {
        next_content = read_content_arg(Some(path))?;
    }
    if let Some(kind) = kind {
        next_kind = kind.to_string();
    }
    if !add_tag.is_empty() {
        for tag in normalize_tags(add_tag) {
            if !next_tags.contains(&tag) {
                next_tags.push(tag);
            }
        }
    }
    if !remove_tag.is_empty() {
        let remove = normalize_tags(remove_tag);
        next_tags.retain(|tag| !remove.contains(tag));
    }
    next_tags = normalize_tags(next_tags);

    let updated_at = now_rfc3339();
    let tags_json = tags_json_opt(&next_tags)?;
    let changed = conn.execute(
        "UPDATE inputs
            SET name = ?1,
                content = ?2,
                kind = ?3,
                tags = ?4,
                updated_at = ?5
          WHERE id = ?6",
        params![
            next_name,
            next_content,
            next_kind,
            tags_json,
            updated_at,
            current.id
        ],
    )?;
    if changed == 0 {
        anyhow::bail!("input not found: {}", slug_or_id);
    }
    load_input_by_id(conn, &current.id)
}

fn load_input(conn: &Connection, slug_or_id: &str) -> Result<InputRow> {
    let sql = format!("{} WHERE {} = ?1", INPUTS_SELECT, id_or_slug_column(slug_or_id));
    conn.query_row(&sql, params![slug_or_id], row_to_input)
        .with_context(|| format!("input not found: {}", slug_or_id))
}

fn load_input_by_id(conn: &Connection, id: &str) -> Result<InputRow> {
    conn.query_row(
        &format!("{} WHERE id = ?1", INPUTS_SELECT),
        params![id],
        row_to_input,
    )
    .with_context(|| format!("input not found by id: {}", id))
}

fn row_to_input(row: &rusqlite::Row<'_>) -> rusqlite::Result<InputRow> {
    let tags_raw: Option<String> = row.get(5)?;
    let tags = parse_tags(tags_raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
        )
    })?;
    Ok(InputRow {
        id: row.get(0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        content: row.get(3)?,
        kind: row.get(4)?,
        tags,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn read_content_arg(path: Option<PathBuf>) -> Result<String> {
    match path {
        Some(p) if p.as_os_str() != "-" => {
            fs::read_to_string(&p).with_context(|| format!("read content file {}", p.display()))
        }
        _ => {
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .context("read content from stdin")?;
            Ok(s)
        }
    }
}

fn id_or_slug_column(input: &str) -> &'static str {
    if Uuid::parse_str(input).is_ok() {
        "id"
    } else {
        "slug"
    }
}

fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_sep = true;
    for ch in name.chars().take(200) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('-');
            last_was_sep = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("input");
    }
    out.chars().take(64).collect()
}

fn unique_slug(conn: &Connection, base: &str) -> Result<String> {
    let mut candidate = base.to_string();
    let mut suffix = 2;
    loop {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM inputs WHERE slug = ?1",
                params![candidate],
                |r| r.get(0),
            )
            .context("query slug collision")?;
        if exists == 0 {
            return Ok(candidate);
        }
        candidate = format!("{}-{}", base, suffix);
        suffix += 1;
        if suffix > 1000 {
            anyhow::bail!("slug-exhaustion");
        }
    }
}

fn validate_kind(kind: &str) -> Result<()> {
    if KIND_VALUES.contains(&kind) {
        Ok(())
    } else {
        anyhow::bail!(
            "invalid --kind: '{}' (expected {})",
            kind,
            KIND_VALUES.join("|")
        )
    }
}

fn validate_name(name: String) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        anyhow::bail!("name must not be empty");
    }
    Ok(trimmed.to_string())
}

/// Decode the tags JSON column. Codex 2026-06-13: surface an error on
/// malformed/non-array payloads instead of silently turning them into
/// []. A bad row that survives means future writes silently lose tag
/// state — better to fail loudly so the operator can repair.
fn parse_tags(raw: Option<String>) -> Result<Vec<String>> {
    let Some(raw) = raw else { return Ok(Vec::new()) };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<String>>(trimmed)
        .with_context(|| format!("inputs.tags column is not a JSON array of strings: {}", raw))
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.iter().any(|existing| existing == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn tags_json_opt(tags: &[String]) -> Result<Option<String>> {
    if tags.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(tags)
            .map(Some)
            .context("serialize tags as JSON")
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn render_human_row(prefix: &str, row: &InputRow, include_content: bool) -> String {
    let tags = if row.tags.is_empty() {
        "-".to_string()
    } else {
        row.tags.join(", ")
    };
    let mut out = format!(
        "{} '{}' (slug: {})\n  id: {}\n  kind: {}\n  tags: {}\n  created: {}\n  updated: {}",
        prefix, row.name, row.slug, row.id, row.kind, tags, row.created_at, row.updated_at
    );
    if include_content {
        out.push_str(&format!("\n\n{}", row.content));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "
            CREATE TABLE inputs (
                id            TEXT PRIMARY KEY,
                slug          TEXT NOT NULL UNIQUE,
                name          TEXT NOT NULL,
                content       TEXT NOT NULL,
                kind          TEXT NOT NULL DEFAULT 'markdown',
                tags          TEXT,
                created_at    TEXT NOT NULL,
                updated_at    TEXT NOT NULL
            );
            CREATE INDEX idx_inputs_updated ON inputs(updated_at DESC);
            ",
        )
        .expect("create inputs schema");
        conn
    }

    #[test]
    fn slugify_handles_basic_input_shapes() {
        assert_eq!(slugify("Weekly Security Review"), "weekly-security-review");
        assert_eq!(slugify("  spaces  around  "), "spaces-around");
        assert_eq!(slugify("!!!only-punctuation!!!"), "only-punctuation");
        assert_eq!(slugify(""), "input");
    }

    #[test]
    fn unique_slug_appends_numeric_suffix_on_collision() {
        let conn = test_conn();
        let now = now_rfc3339();
        conn.execute(
            "INSERT INTO inputs (id, slug, name, content, kind, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params!["1", "context-bundle", "A", "body", KIND_MARKDOWN, now, now_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO inputs (id, slug, name, content, kind, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params!["2", "context-bundle-2", "B", "body", KIND_MARKDOWN, now_rfc3339(), now_rfc3339()],
        )
        .unwrap();

        assert_eq!(unique_slug(&conn, "fresh").unwrap(), "fresh");
        assert_eq!(unique_slug(&conn, "context-bundle").unwrap(), "context-bundle-3");
    }

    #[test]
    fn validate_kind_accepts_and_rejects_expected_values() {
        assert!(validate_kind(KIND_MARKDOWN).is_ok());
        assert!(validate_kind(KIND_JSON).is_ok());
        assert!(validate_kind(KIND_TEXT).is_ok());
        let err = validate_kind("yaml").expect_err("must reject unknown kind");
        assert!(
            err.to_string().contains("invalid --kind"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn crud_round_trip_in_memory_db() {
        let conn = test_conn();
        let id = Uuid::new_v4().to_string();
        let now = now_rfc3339();
        conn.execute(
            "INSERT INTO inputs (id, slug, name, content, kind, tags, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                "bundle-alpha",
                "Bundle Alpha",
                "# Hello",
                KIND_MARKDOWN,
                serde_json::to_string(&vec!["core", "docs"]).unwrap(),
                now,
                now_rfc3339()
            ],
        )
        .unwrap();

        let loaded = load_input(&conn, "bundle-alpha").unwrap();
        assert_eq!(loaded.name, "Bundle Alpha");
        assert_eq!(loaded.tags, vec!["core".to_string(), "docs".to_string()]);

        let edited = edit_input(
            &conn,
            "bundle-alpha",
            Some("Bundle Beta".into()),
            None,
            vec!["ops".into()],
            vec!["docs".into()],
            Some(KIND_TEXT),
        )
        .unwrap();
        assert_eq!(edited.name, "Bundle Beta");
        assert_eq!(edited.kind, KIND_TEXT);
        assert_eq!(edited.tags, vec!["core".to_string(), "ops".to_string()]);

        let deleted = conn
            .execute("DELETE FROM inputs WHERE slug = ?1", params!["bundle-alpha"])
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(load_input(&conn, "bundle-alpha").is_err());
    }

    #[test]
    fn delete_without_yes_refuses() {
        let err = require_delete_yes("bundle", false).expect_err("must refuse");
        assert!(err.to_string().contains("pass --yes"));
    }

    #[test]
    fn tag_add_remove_round_trip() {
        let conn = test_conn();
        let now = now_rfc3339();
        conn.execute(
            "INSERT INTO inputs (id, slug, name, content, kind, tags, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                "seed",
                "bundle-tags",
                "Bundle Tags",
                "body",
                KIND_MARKDOWN,
                serde_json::to_string(&vec!["alpha", "beta"]).unwrap(),
                now,
                now_rfc3339()
            ],
        )
        .unwrap();

        let row = edit_input(
            &conn,
            "bundle-tags",
            None,
            None,
            vec!["gamma".into(), "alpha".into()],
            vec!["beta".into()],
            None,
        )
        .unwrap();
        assert_eq!(
            row.tags,
            vec!["alpha".to_string(), "gamma".to_string()]
        );
    }
}
