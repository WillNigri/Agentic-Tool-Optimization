// `ato config-changes list --agent <slug> [--since 7d]`
//
// Reads agent_config_changes table — the configuration ledger that
// auto-logs every model swap, prompt edit, role-models change, etc.
// Schema lives in the migration that shipped with v2.1.0.

use crate::db::parse_since;
use crate::output::{emit_human, emit_json, Opts};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ConfigChange {
    pub change_id: String,
    pub agent_slug: String,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub actor: Option<String>,
    pub changed_at: String,
}

pub fn list(conn: &Connection, agent: &str, since: &str, opts: &Opts) -> Result<()> {
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='agent_config_changes'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if table_exists == 0 {
        if opts.human {
            emit_human(
                "No agent_config_changes table found. This table is created on first \
                 desktop launch after v2.1.0. If you've upgraded recently, run the \
                 desktop GUI once to apply the migration.",
            );
        } else {
            emit_json(&Vec::<ConfigChange>::new())?;
        }
        return Ok(());
    }

    let since_modifier = parse_since(since)?;

    let mut stmt = conn.prepare(
        "SELECT id, agent_slug, field, old_value, new_value, actor, changed_at
           FROM agent_config_changes
          WHERE agent_slug = ?1
            AND changed_at > datetime('now', ?2)
          ORDER BY changed_at DESC",
    ).context("Failed to prepare config-changes query")?;

    let rows = stmt
        .query_map([agent, since_modifier.as_str()], |r| {
            Ok(ConfigChange {
                change_id: r.get(0)?,
                agent_slug: r.get(1)?,
                field: r.get(2)?,
                old_value: r.get(3)?,
                new_value: r.get(4)?,
                actor: r.get(5)?,
                changed_at: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if opts.human {
        if rows.is_empty() {
            emit_human(&format!("No config changes for @{} in the last {}.", agent, since));
        } else {
            let mut s = format!("{} config changes for @{} (since {}):\n\n", rows.len(), agent, since);
            for r in &rows {
                let old = r.old_value.as_deref().unwrap_or("(unset)");
                let new = r.new_value.as_deref().unwrap_or("(unset)");
                s.push_str(&format!(
                    "  {} | {} : {} → {}\n",
                    r.changed_at, r.field, old, new
                ));
            }
            emit_human(&s);
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}
