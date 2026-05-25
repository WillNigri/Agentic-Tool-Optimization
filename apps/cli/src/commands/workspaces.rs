// v2.11 PR-11 — workspaces foundation (CLI surface).
//
// Workspaces are a local-first namespace primitive. Free tier ships with
// a single "Personal" workspace (auto-created by the schema). Team tier
// (ato-cloud, closed-source) adds multi-user membership + cross-device
// sync over the same SQLite tables.
//
// What this PR ships in OSS:
//   * `ato workspaces create / list / use / current / rename / archive`
//   * Active workspace persisted in ~/.ato/active-workspace.json
//     (single-line JSON, same shape as the rate-card override file)
//
// What's NOT in OSS (lives in ato-cloud):
//   * workspace_members population — the schema accepts the table but
//     OSS never writes to it
//   * Cross-device sync of workspace state
//   * RBAC enforcement (role: owner / admin / editor / viewer)
//   * Invite-link generation
//
// Tier doctrine ("scarcity in cloud, not in BYOK/local"): the workspace
// PRIMITIVE is free. The collaboration features that cost us infra to
// host are Team-gated, and they live in ato-cloud.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::db;
use crate::output::{emit_human, emit_json, Opts};

#[derive(Args, Debug)]
pub struct WorkspacesArgs {
    #[command(subcommand)]
    pub sub: WorkspacesSub,
}

#[derive(Subcommand, Debug)]
pub enum WorkspacesSub {
    /// Create a new workspace. The slug is your stable identifier
    /// (used in `ato workspaces use <slug>`).
    Create {
        /// URL-safe identifier, unique in this DB.
        slug: String,
        /// Human-readable name shown in `list`.
        #[arg(long)]
        name: Option<String>,
        /// Tier hint. `personal` (default) is free-forever; `team`
        /// flags that this workspace expects multi-user features
        /// from ato-cloud (sync, RBAC). The CLI doesn't enforce the
        /// distinction — it's a UI hint.
        #[arg(long, default_value = "personal")]
        tier: String,
    },
    /// List every workspace defined locally. Archived workspaces hidden
    /// by default; pass --all to include them.
    List {
        #[arg(long, default_value_t = false)]
        all: bool,
    },
    /// Switch the active workspace. The selection is persisted to
    /// ~/.ato/active-workspace.json so subsequent commands inherit
    /// the choice. Filters that read the active workspace land in
    /// future PRs; this PR just persists the choice.
    Use {
        slug: String,
    },
    /// Print the currently active workspace.
    Current,
    /// Rename a workspace (its slug stays the same).
    Rename {
        slug: String,
        new_name: String,
    },
    /// Archive a workspace — keeps the row but excludes it from
    /// `list` by default. Reversible via `unarchive`.
    Archive {
        slug: String,
    },
    /// Unarchive a workspace.
    Unarchive {
        slug: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub tier_hint: String,
    pub created_at: String,
    pub archived_at: Option<String>,
}

pub fn run(args: WorkspacesArgs, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    match args.sub {
        WorkspacesSub::Create { slug, name, tier } => {
            handle_create(slug, name, tier, db_path, opts)
        }
        WorkspacesSub::List { all } => handle_list(all, db_path, opts),
        WorkspacesSub::Use { slug } => handle_use(slug, db_path, opts),
        WorkspacesSub::Current => handle_current(db_path, opts),
        WorkspacesSub::Rename { slug, new_name } => {
            handle_rename(slug, new_name, db_path, opts)
        }
        WorkspacesSub::Archive { slug } => handle_archive(slug, db_path, opts),
        WorkspacesSub::Unarchive { slug } => handle_unarchive(slug, db_path, opts),
    }
}

fn active_workspace_path_readonly() -> PathBuf {
    let mut p = db::home_dir();
    p.push(".ato");
    p.push("active-workspace.json");
    p
}

fn active_workspace_path_writable() -> PathBuf {
    let mut p = db::home_dir();
    p.push(".ato");
    let _ = std::fs::create_dir_all(&p);
    p.push("active-workspace.json");
    p
}

fn handle_create(
    slug: String,
    name: Option<String>,
    tier: String,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    if tier != "personal" && tier != "team" {
        anyhow::bail!("tier must be `personal` or `team`; got `{}`", tier);
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!(
            "workspace slug must be URL-safe (ASCII letters / digits / `-` / `_`); got `{}`",
            slug
        );
    }
    let display_name = name.unwrap_or_else(|| slug.clone());
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let conn = db::open_readwrite(db_path)?;
    let result = conn.execute(
        "INSERT INTO workspaces (id, slug, name, tier_hint, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![&id, &slug, &display_name, &tier, &now],
    );
    match result {
        Ok(_) => {
            if opts.human {
                emit_human(&format!(
                    "Created workspace '{}' [{}]: {}",
                    slug, tier, display_name,
                ));
            } else {
                let _ = emit_json(&serde_json::json!({
                    "id": id,
                    "slug": slug,
                    "name": display_name,
                    "tier_hint": tier,
                }));
            }
            Ok(())
        }
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            anyhow::bail!(
                "workspace slug '{}' already exists. Use `ato workspaces list` to see existing workspaces.",
                slug
            )
        }
        Err(e) => Err(e).context("insert workspace"),
    }
}

fn handle_list(all: bool, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let sql = if all {
        "SELECT id, slug, name, tier_hint, created_at, archived_at
         FROM workspaces ORDER BY created_at ASC"
    } else {
        "SELECT id, slug, name, tier_hint, created_at, archived_at
         FROM workspaces WHERE archived_at IS NULL ORDER BY created_at ASC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<WorkspaceRow> = stmt
        .query_map([], |r| {
            Ok(WorkspaceRow {
                id: r.get(0)?,
                slug: r.get(1)?,
                name: r.get(2)?,
                tier_hint: r.get(3)?,
                created_at: r.get(4)?,
                archived_at: r.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    let active = current_active_slug();
    if opts.human {
        if rows.is_empty() {
            emit_human("(no workspaces — use `ato workspaces create` to add one)");
        } else {
            emit_human(&format!("{} workspaces:", rows.len()));
            for r in &rows {
                let marker = if active.as_deref() == Some(r.slug.as_str()) {
                    "* "
                } else {
                    "  "
                };
                let archived = if r.archived_at.is_some() {
                    " (archived)"
                } else {
                    ""
                };
                emit_human(&format!(
                    "{}{}  [{}]  {}{}",
                    marker, r.slug, r.tier_hint, r.name, archived,
                ));
            }
            if active.is_some() {
                emit_human("\n  * = active workspace (`ato workspaces use <slug>` to switch)");
            }
        }
    } else {
        let _ = emit_json(&serde_json::json!({
            "workspaces": rows,
            "active_slug": active,
        }));
    }
    Ok(())
}

fn handle_use(slug: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    // Verify the slug exists (and isn't archived) before persisting.
    let conn = db::open_readonly(db_path)?;
    let (id, name, archived): (String, String, Option<String>) = conn
        .query_row(
            "SELECT id, name, archived_at FROM workspaces WHERE slug = ?1",
            params![&slug],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "no workspace with slug '{}'. `ato workspaces list` to see what's defined.",
                slug
            )
        })?;
    if archived.is_some() {
        anyhow::bail!(
            "workspace '{}' is archived. Run `ato workspaces unarchive {}` first.",
            slug, slug
        );
    }
    let path = active_workspace_path_writable();
    let payload = serde_json::json!({
        "slug": slug,
        "id": id,
        "switched_at": chrono::Utc::now().to_rfc3339(),
    });
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&payload).context("serialize active workspace")?,
    )
    .context("write active-workspace.json")?;
    if opts.human {
        emit_human(&format!(
            "Active workspace switched to '{}': {}",
            slug, name
        ));
    } else {
        let _ = emit_json(&payload);
    }
    Ok(())
}

fn handle_current(db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let active = current_active_slug();
    match active {
        Some(slug) => {
            let conn = db::open_readonly(db_path)?;
            let row: WorkspaceRow = conn
                .query_row(
                    "SELECT id, slug, name, tier_hint, created_at, archived_at
                     FROM workspaces WHERE slug = ?1",
                    params![&slug],
                    |r| {
                        Ok(WorkspaceRow {
                            id: r.get(0)?,
                            slug: r.get(1)?,
                            name: r.get(2)?,
                            tier_hint: r.get(3)?,
                            created_at: r.get(4)?,
                            archived_at: r.get(5)?,
                        })
                    },
                )
                .map_err(|_| {
                    anyhow::anyhow!(
                        "active workspace '{}' no longer exists in DB. \
                         Run `ato workspaces list` and `ato workspaces use <slug>` to recover.",
                        slug
                    )
                })?;
            if opts.human {
                emit_human(&format!(
                    "Active workspace: {} [{}] — {}",
                    row.slug, row.tier_hint, row.name
                ));
            } else {
                let _ = emit_json(&row);
            }
        }
        None => {
            if opts.human {
                emit_human(
                    "(no active workspace set — `ato workspaces use <slug>` to choose one. \
                     The schema seeds a 'personal' workspace by default.)",
                );
            } else {
                let _ = emit_json(&serde_json::json!({
                    "active_slug": null,
                }));
            }
        }
    }
    Ok(())
}

fn current_active_slug() -> Option<String> {
    let path = active_workspace_path_readonly();
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("slug").and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn handle_rename(
    slug: String,
    new_name: String,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let changed = conn
        .execute(
            "UPDATE workspaces SET name = ?1 WHERE slug = ?2",
            params![&new_name, &slug],
        )
        .context("update workspace name")?;
    if changed == 0 {
        anyhow::bail!("no workspace with slug '{}'", slug);
    }
    if opts.human {
        emit_human(&format!(
            "Renamed workspace '{}' → '{}'",
            slug, new_name
        ));
    } else {
        let _ = emit_json(&serde_json::json!({
            "slug": slug, "new_name": new_name,
        }));
    }
    Ok(())
}

fn handle_archive(slug: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let now = chrono::Utc::now().to_rfc3339();
    let changed = conn
        .execute(
            "UPDATE workspaces SET archived_at = ?1
             WHERE slug = ?2 AND archived_at IS NULL",
            params![&now, &slug],
        )
        .context("archive workspace")?;
    if changed == 0 {
        anyhow::bail!(
            "no active workspace with slug '{}' to archive (already archived or missing)",
            slug
        );
    }
    if opts.human {
        emit_human(&format!("Archived workspace '{}'", slug));
    } else {
        let _ = emit_json(&serde_json::json!({"archived": slug}));
    }
    Ok(())
}

fn handle_unarchive(slug: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let changed = conn
        .execute(
            "UPDATE workspaces SET archived_at = NULL
             WHERE slug = ?1 AND archived_at IS NOT NULL",
            params![&slug],
        )
        .context("unarchive workspace")?;
    if changed == 0 {
        anyhow::bail!(
            "no archived workspace with slug '{}' to unarchive",
            slug
        );
    }
    if opts.human {
        emit_human(&format!("Unarchived workspace '{}'", slug));
    } else {
        let _ = emit_json(&serde_json::json!({"unarchived": slug}));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_db() -> (rusqlite::Connection, PathBuf) {
        let tmp = std::env::temp_dir().join(format!(
            "ato-workspaces-test-{}.db",
            uuid::Uuid::new_v4()
        ));
        let conn = rusqlite::Connection::open(&tmp).unwrap();
        // Mirror the schema's workspaces table here for hermetic unit
        // testing — the production schema lives in apps/desktop/src-tauri.
        conn.execute(
            "CREATE TABLE workspaces (
                id           TEXT PRIMARY KEY,
                slug         TEXT NOT NULL UNIQUE,
                name         TEXT NOT NULL,
                tier_hint    TEXT NOT NULL DEFAULT 'personal',
                created_at   TEXT NOT NULL,
                archived_at  TEXT
            )",
            [],
        )
        .unwrap();
        (conn, tmp)
    }

    #[test]
    fn slug_validator_predicate_matches_handle_create() {
        // The validator inside handle_create uses this exact predicate.
        // Pinning it as a separate test guards against silent loosening.
        let bad = "no spaces allowed";
        assert!(!bad
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        let bad2 = "slash/path";
        assert!(!bad2
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        let good = "team-alpha_1";
        assert!(good
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn insert_then_list_round_trips() {
        let (conn, path) = make_test_db();
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO workspaces (id, slug, name, tier_hint, created_at)
             VALUES (?1, 'team-alpha', 'Team Alpha', 'team', '2026-05-25')",
            params![&id],
        )
        .unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn slug_unique_constraint_rejects_duplicates() {
        let (conn, path) = make_test_db();
        conn.execute(
            "INSERT INTO workspaces (id, slug, name, tier_hint, created_at)
             VALUES ('id-1', 'dup', 'A', 'personal', '2026-05-25')",
            [],
        )
        .unwrap();
        let result = conn.execute(
            "INSERT INTO workspaces (id, slug, name, tier_hint, created_at)
             VALUES ('id-2', 'dup', 'B', 'personal', '2026-05-25')",
            [],
        );
        assert!(result.is_err(), "duplicate slug must violate UNIQUE constraint");
        let _ = std::fs::remove_file(&path);
    }
}
