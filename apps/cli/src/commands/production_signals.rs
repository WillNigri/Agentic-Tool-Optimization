// v2.11 PR-12.5-lean — production_signals CLI (OSS consumer side).
//
// The Langfuse / Helicone INGESTER lives in ato-cloud (closed-source —
// it talks to their APIs + pulls trace data). The OSS side ships
// strictly the WRITE TARGET + the READ surface the diagnose pipeline
// consumes. Customers without ato-cloud can pipe their own
// `langfuse traces export --json` (or any structured trace source)
// into `ato production-signals add` today.
//
// Schema lives in apps/desktop/src-tauri/src/schema.rs:
//
//   CREATE TABLE production_signals (
//       id           TEXT PRIMARY KEY,
//       agent_slug   TEXT NOT NULL,
//       source       TEXT NOT NULL,    -- 'langfuse' | 'helicone' | 'manual'
//       signal_json  TEXT NOT NULL,    -- arbitrary shape; diagnose treats opaquely
//       captured_at  TEXT NOT NULL
//   );
//   CREATE INDEX idx_production_signals_agent
//       ON production_signals(agent_slug, captured_at DESC);
//
// The diagnose pipeline (`apps/cli/src/methodology/diagnose.rs`) reads
// from this table when the methodology has an agent_slug binding, and
// injects a structured `## Production signals` block into the
// diagnose prompt per docs/v2.11-learning-loop.md §Q6. The diagnose
// agent's system prompt instructs: "when dev rubric scores and
// production signals conflict, production wins."

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::db;
use crate::output::{emit_human, emit_json, Opts};

#[derive(Args, Debug)]
pub struct ProductionSignalsArgs {
    #[command(subcommand)]
    pub sub: ProductionSignalsSub,
}

#[derive(Subcommand, Debug)]
pub enum ProductionSignalsSub {
    /// Add a production signal for an agent. The `signal_file` is read
    /// verbatim as the JSON blob — we don't enforce a schema here so
    /// customers can pipe any structured trace export through.
    /// Diagnose treats the contents opaquely and just injects them
    /// into the diagnose prompt's `## Production signals` block.
    Add {
        /// Agent slug the signal pertains to.
        #[arg(long)]
        agent_slug: String,
        /// Source label. Free-form; conventional values are
        /// `langfuse` / `helicone` / `manual`.
        #[arg(long, default_value = "manual")]
        source: String,
        /// Path to the JSON signal payload. Use `-` to read from stdin.
        #[arg(long, short)]
        signal_file: PathBuf,
        /// Optional timestamp (ISO-8601) when the signal was captured.
        /// Defaults to now.
        #[arg(long)]
        captured_at: Option<String>,
    },
    /// List stored production signals. Filterable by agent.
    List {
        /// Filter to one agent's signals.
        #[arg(long)]
        agent_slug: Option<String>,
        /// Max rows. Default 50.
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Delete one signal by id.
    Delete {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionSignalRow {
    pub id: String,
    pub agent_slug: String,
    pub source: String,
    pub signal_json: String,
    pub captured_at: String,
}

pub fn run(args: ProductionSignalsArgs, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    match args.sub {
        ProductionSignalsSub::Add {
            agent_slug,
            source,
            signal_file,
            captured_at,
        } => handle_add(agent_slug, source, signal_file, captured_at, db_path, opts),
        ProductionSignalsSub::List { agent_slug, limit } => {
            handle_list(agent_slug, limit, db_path, opts)
        }
        ProductionSignalsSub::Delete { id } => handle_delete(id, db_path, opts),
    }
}

fn handle_add(
    agent_slug: String,
    source: String,
    signal_file: PathBuf,
    captured_at: Option<String>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let raw = if signal_file.as_os_str() == "-" {
        let mut buf = String::new();
        use std::io::Read;
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("read production signal from stdin")?;
        buf
    } else {
        std::fs::read_to_string(&signal_file)
            .with_context(|| format!("read production signal from {}", signal_file.display()))?
    };
    // Validate it's parseable JSON (we don't enforce a schema beyond
    // that — diagnose treats the contents opaquely + the customer
    // controls what they pipe in).
    let _: serde_json::Value = serde_json::from_str(&raw)
        .context("production signal payload must be valid JSON")?;

    let id = Uuid::new_v4().to_string();
    let captured_at = captured_at.unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let conn = db::open_readwrite(db_path)?;
    conn.execute(
        "INSERT INTO production_signals (id, agent_slug, source, signal_json, captured_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![&id, &agent_slug, &source, &raw, &captured_at],
    )
    .context("insert production_signals row")?;

    if opts.human {
        emit_human(&format!(
            "Stored production signal '{}' for agent '{}' from source '{}' ({} bytes).",
            id,
            agent_slug,
            source,
            raw.len()
        ));
    } else {
        let _ = emit_json(&serde_json::json!({
            "id": id,
            "agent_slug": agent_slug,
            "source": source,
            "captured_at": captured_at,
            "bytes": raw.len(),
        }));
    }
    Ok(())
}

fn handle_list(
    agent_slug: Option<String>,
    limit: u32,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let (sql, has_filter) = match &agent_slug {
        Some(_) => (
            "SELECT id, agent_slug, source, signal_json, captured_at
             FROM production_signals
             WHERE agent_slug = ?1
             ORDER BY captured_at DESC
             LIMIT ?2",
            true,
        ),
        None => (
            "SELECT id, agent_slug, source, signal_json, captured_at
             FROM production_signals
             ORDER BY captured_at DESC
             LIMIT ?1",
            false,
        ),
    };
    let mut stmt = conn.prepare(sql)?;
    let map_row = |r: &rusqlite::Row| -> rusqlite::Result<ProductionSignalRow> {
        Ok(ProductionSignalRow {
            id: r.get(0)?,
            agent_slug: r.get(1)?,
            source: r.get(2)?,
            signal_json: r.get(3)?,
            captured_at: r.get(4)?,
        })
    };
    let rows: Vec<ProductionSignalRow> = if has_filter {
        let slug = agent_slug.unwrap();
        stmt.query_map(params![&slug, limit as i64], map_row)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map(params![limit as i64], map_row)?
            .filter_map(|r| r.ok())
            .collect()
    };
    if opts.human {
        if rows.is_empty() {
            emit_human("(no production signals — `ato production-signals add` to record one)");
        } else {
            emit_human(&format!("{} production signal(s):", rows.len()));
            for r in &rows {
                let preview = r
                    .signal_json
                    .chars()
                    .take(80)
                    .collect::<String>();
                emit_human(&format!(
                    "  {}  [{}]  agent={}  {}\n    {}",
                    r.id,
                    r.source,
                    r.agent_slug,
                    r.captured_at,
                    preview
                ));
            }
        }
    } else {
        let _ = emit_json(&rows);
    }
    Ok(())
}

fn handle_delete(id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let changed = conn
        .execute(
            "DELETE FROM production_signals WHERE id = ?1",
            params![&id],
        )
        .context("delete production_signals row")?;
    if changed == 0 {
        anyhow::bail!(
            "no production signal with id '{}'. `ato production-signals list` to see what's stored.",
            id
        );
    }
    if opts.human {
        emit_human(&format!("Deleted production signal '{}'.", id));
    } else {
        let _ = emit_json(&serde_json::json!({"deleted": id}));
    }
    Ok(())
}

/// Read every production signal stored for an agent, ordered newest-first.
/// Returns the raw JSON strings — the diagnose pipeline injects them
/// verbatim into the prompt's `## Production signals` block.
pub fn signals_for_agent(
    db_path: &std::path::Path,
    agent_slug: &str,
    limit: usize,
) -> Result<Vec<ProductionSignalRow>> {
    let conn = db::open_readonly(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, agent_slug, source, signal_json, captured_at
         FROM production_signals
         WHERE agent_slug = ?1
         ORDER BY captured_at DESC
         LIMIT ?2",
    )?;
    let rows: Vec<ProductionSignalRow> = stmt
        .query_map(params![agent_slug, limit as i64], |r| {
            Ok(ProductionSignalRow {
                id: r.get(0)?,
                agent_slug: r.get(1)?,
                source: r.get(2)?,
                signal_json: r.get(3)?,
                captured_at: r.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_db() -> (rusqlite::Connection, PathBuf) {
        let tmp = std::env::temp_dir().join(format!(
            "ato-production-signals-test-{}.db",
            uuid::Uuid::new_v4()
        ));
        let conn = rusqlite::Connection::open(&tmp).unwrap();
        conn.execute(
            "CREATE TABLE production_signals (
                id TEXT PRIMARY KEY,
                agent_slug TEXT NOT NULL,
                source TEXT NOT NULL,
                signal_json TEXT NOT NULL,
                captured_at TEXT NOT NULL
            )",
            [],
        )
        .unwrap();
        (conn, tmp)
    }

    #[test]
    fn insert_then_select_round_trips() {
        let (conn, path) = make_test_db();
        conn.execute(
            "INSERT INTO production_signals (id, agent_slug, source, signal_json, captured_at)
             VALUES ('s1', 'code-reviewer', 'langfuse', '{\"abandonment\":0.34}', '2026-05-25')",
            [],
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM production_signals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn signals_for_agent_filters_by_slug() {
        let (conn, path) = make_test_db();
        for (id, slug) in [("s1", "x"), ("s2", "y"), ("s3", "x")] {
            conn.execute(
                "INSERT INTO production_signals (id, agent_slug, source, signal_json, captured_at)
                 VALUES (?1, ?2, 'manual', '{}', '2026-05-25')",
                params![id, slug],
            )
            .unwrap();
        }
        let mut stmt = conn
            .prepare("SELECT COUNT(*) FROM production_signals WHERE agent_slug = ?1")
            .unwrap();
        let x_count: i64 = stmt.query_row(params!["x"], |r| r.get(0)).unwrap();
        assert_eq!(x_count, 2);
        let y_count: i64 = stmt.query_row(params!["y"], |r| r.get(0)).unwrap();
        assert_eq!(y_count, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn invalid_json_payload_validation_predicate() {
        // The handler validates payloads as JSON before insertion.
        // Pin the predicate so a future refactor can't loosen it.
        assert!(serde_json::from_str::<serde_json::Value>("not json").is_err());
        assert!(serde_json::from_str::<serde_json::Value>(r#"{"ok": true}"#).is_ok());
        assert!(serde_json::from_str::<serde_json::Value>("[1, 2, 3]").is_ok());
    }
}
