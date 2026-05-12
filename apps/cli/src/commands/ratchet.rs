// v2.3.39 Phase 6.x-K — Eval-score ratchet.
//
// Inspired by Garry Tan's "AI Agent Complexity Ratchet" post (May 2026):
// once you lock in a quality floor, no new dispatch can drop below it
// without the CI step failing loudly. For ATO this is a natural
// extension of Phase 5b (regression detection on config changes) —
// regressions catches "your model swap dropped quality"; the ratchet
// catches "any drift below the floor for any reason."
//
// Metric for v1 is `success_rate`: COUNT(status='success') / COUNT(*)
// over a time window. Coarse but universally available locally — we
// don't need cloud eval scores to ship a useful CI gate. When cloud
// eval_score lands locally (or when the user opts into eval scoring
// via a local evaluator), we add `--metric eval_score` to the same
// table — the schema's `metric` column is the discriminator.
//
// Target kinds:
//   - agent:<slug>   — only execution_logs rows with agent_slug = ?
//   - runtime:<name> — only rows with runtime = ?
//   - global         — every row
// Pick the granularity that matches how you actually drive
// dispatches. If you flag every run with --agent, ratchet by agent.
// If you mostly do `ato dispatch claude "..."` ad hoc, ratchet by
// runtime instead.

use anyhow::{anyhow, Context, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;

use crate::db;
use crate::output::{emit_human, emit_json, Opts};

#[derive(Debug, Serialize, Clone)]
pub struct RatchetRecord {
    pub target_kind: String,
    pub target_value: String,
    pub metric: String,
    pub baseline_value: f64,
    pub baseline_window_days: i64,
    pub threshold: f64,
    pub locked_at: String,
    pub locked_by: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct RatchetCheckRow {
    pub target_kind: String,
    pub target_value: String,
    pub metric: String,
    pub baseline_value: f64,
    pub threshold: f64,
    pub current_value: Option<f64>,
    pub current_sample_count: i64,
    pub current_window_days: i64,
    /// Floor minus threshold — the value the current must stay above.
    pub floor_with_tolerance: f64,
    /// `pass` / `fail` / `insufficient_data`.
    pub verdict: String,
    pub note: Option<String>,
}

/// Parse a `--target` flag value of the form `agent:<slug>` /
/// `runtime:<name>` / `global`. Returns (kind, value) with `value=""`
/// for global. Bails on unknown kinds rather than guessing, so a
/// typo (`agnet:foo`) doesn't silently fall through to global.
pub fn parse_target(s: &str) -> Result<(String, String)> {
    if s == "global" {
        return Ok(("global".into(), "".into()));
    }
    let (kind, value) = s
        .split_once(':')
        .ok_or_else(|| anyhow!("--target must be `agent:<slug>`, `runtime:<name>`, or `global` (got `{}`)", s))?;
    match kind {
        "agent" | "runtime" => {
            if value.is_empty() {
                anyhow::bail!("--target `{}:` needs a non-empty value", kind);
            }
            Ok((kind.to_string(), value.to_string()))
        }
        other => anyhow::bail!(
            "--target kind must be `agent`, `runtime`, or `global` (got `{}`)",
            other
        ),
    }
}

/// Compute the success-rate window for a target. Returns (rate,
/// sample_count). Sample_count of zero means there's nothing in the
/// window — callers treat that as "insufficient data," not a failure.
pub fn compute_success_rate(
    conn: &Connection,
    target_kind: &str,
    target_value: &str,
    days: i64,
) -> Result<(Option<f64>, i64)> {
    let cutoff = format!("-{} days", days);
    let (sql, args): (&str, Vec<String>) = match target_kind {
        "agent" => (
            "SELECT
                COUNT(*) AS total,
                SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) AS ok
             FROM execution_logs
             WHERE created_at >= datetime('now', ?1)
               AND agent_slug = ?2",
            vec![cutoff, target_value.to_string()],
        ),
        "runtime" => (
            "SELECT
                COUNT(*) AS total,
                SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) AS ok
             FROM execution_logs
             WHERE created_at >= datetime('now', ?1)
               AND runtime = ?2",
            vec![cutoff, target_value.to_string()],
        ),
        "global" => (
            "SELECT
                COUNT(*) AS total,
                SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) AS ok
             FROM execution_logs
             WHERE created_at >= datetime('now', ?1)",
            vec![cutoff],
        ),
        other => anyhow::bail!("unknown target_kind '{}'", other),
    };
    let params_dyn: Vec<&dyn rusqlite::ToSql> = args.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let (total, ok): (i64, Option<i64>) = conn.query_row(sql, params_dyn.as_slice(), |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    if total == 0 {
        Ok((None, 0))
    } else {
        let ok = ok.unwrap_or(0);
        Ok((Some(ok as f64 / total as f64), total))
    }
}

pub fn lock(
    db_path: &PathBuf,
    target_kind: &str,
    target_value: &str,
    window_days: i64,
    threshold: f64,
    notes: Option<&str>,
    opts: &Opts,
) -> Result<()> {
    if threshold < 0.0 || threshold > 1.0 {
        anyhow::bail!("--threshold must be between 0.0 and 1.0 (got {})", threshold);
    }
    if window_days <= 0 {
        anyhow::bail!("--days must be positive (got {})", window_days);
    }
    let conn = db::open_readwrite(db_path)?;
    let (rate, samples) = compute_success_rate(&conn, target_kind, target_value, window_days)?;
    let baseline = rate.ok_or_else(|| {
        anyhow!(
            "Can't lock {}:{} — no dispatches in the last {} days. Run a few dispatches first, then retry.",
            target_kind, target_value, window_days,
        )
    })?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO eval_ratchets
             (target_kind, target_value, metric, baseline_value,
              baseline_window_days, threshold, locked_at, locked_by, notes)
         VALUES (?1, ?2, 'success_rate', ?3, ?4, ?5, ?6, 'manual', ?7)
         ON CONFLICT(target_kind, target_value, metric) DO UPDATE SET
             baseline_value = excluded.baseline_value,
             baseline_window_days = excluded.baseline_window_days,
             threshold = excluded.threshold,
             locked_at = excluded.locked_at,
             notes = excluded.notes",
        rusqlite::params![target_kind, target_value, baseline, window_days, threshold, now, notes],
    )
    .context("INSERT eval_ratchets")?;
    if opts.human {
        emit_human(&format!(
            "Locked {}:{} floor = {:.1}% success ({} samples over {}d), threshold = {:.0}pp.",
            target_kind,
            target_value,
            baseline * 100.0,
            samples,
            window_days,
            threshold * 100.0,
        ));
        emit_human(&format!(
            "  Run `ato ratchet check` to verify; current rate must stay ≥ {:.1}%.",
            (baseline - threshold).max(0.0) * 100.0,
        ));
    } else {
        emit_json(&serde_json::json!({
            "target_kind": target_kind,
            "target_value": target_value,
            "baseline_value": baseline,
            "baseline_window_days": window_days,
            "threshold": threshold,
            "samples": samples,
            "locked": true,
        }))?;
    }
    Ok(())
}

pub fn unlock(
    db_path: &PathBuf,
    target_kind: &str,
    target_value: &str,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    let n = conn.execute(
        "DELETE FROM eval_ratchets WHERE target_kind = ?1 AND target_value = ?2",
        rusqlite::params![target_kind, target_value],
    )?;
    if opts.human {
        if n == 0 {
            emit_human(&format!("No ratchet locked for {}:{}.", target_kind, target_value));
        } else {
            emit_human(&format!("Unlocked {}:{}.", target_kind, target_value));
        }
    } else {
        emit_json(&serde_json::json!({"deleted": n}))?;
    }
    Ok(())
}

pub fn list(db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let rows = list_inner(&conn)?;
    if opts.human {
        if rows.is_empty() {
            emit_human("No ratchets locked. `ato ratchet lock --target ...` to create one.");
        } else {
            emit_human(&format!("{} ratchet(s) locked:", rows.len()));
            for r in &rows {
                let target_disp = if r.target_kind == "global" {
                    "global".to_string()
                } else {
                    format!("{}:{}", r.target_kind, r.target_value)
                };
                emit_human(&format!(
                    "  {:30}  floor={:.1}%  threshold={:.0}pp  window={}d  locked={}",
                    target_disp,
                    r.baseline_value * 100.0,
                    r.threshold * 100.0,
                    r.baseline_window_days,
                    r.locked_at.split('T').next().unwrap_or(&r.locked_at),
                ));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

fn list_inner(conn: &Connection) -> Result<Vec<RatchetRecord>> {
    let mut stmt = conn.prepare(
        "SELECT target_kind, target_value, metric, baseline_value,
                baseline_window_days, threshold, locked_at, locked_by, notes
           FROM eval_ratchets
          ORDER BY target_kind, target_value",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(RatchetRecord {
                target_kind: r.get(0)?,
                target_value: r.get(1)?,
                metric: r.get(2)?,
                baseline_value: r.get(3)?,
                baseline_window_days: r.get(4)?,
                threshold: r.get(5)?,
                locked_at: r.get(6)?,
                locked_by: r.get(7)?,
                notes: r.get(8)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Window the check looks back over. Shorter than the baseline
/// window because the check should detect recent drift, not slow-
/// moving averages. Configurable on the CLI; this is the default.
const DEFAULT_CHECK_WINDOW_DAYS: i64 = 7;

/// `ato ratchet check`. Returns Ok(true) when every ratchet passes,
/// Ok(false) when at least one fails. The CLI handler exits with the
/// inverted code so this lands cleanly in CI as a gate.
pub fn check(
    db_path: &PathBuf,
    target_filter: Option<(String, String)>,
    check_window_days: i64,
    opts: &Opts,
) -> Result<bool> {
    let conn = db::open_readonly(db_path)?;
    let all = list_inner(&conn)?;
    let targets: Vec<RatchetRecord> = match target_filter {
        Some((k, v)) => all
            .into_iter()
            .filter(|r| r.target_kind == k && r.target_value == v)
            .collect(),
        None => all,
    };
    if targets.is_empty() {
        if opts.human {
            emit_human("No ratchets to check.");
        } else {
            emit_json(&Vec::<RatchetCheckRow>::new())?;
        }
        return Ok(true);
    }

    let mut rows: Vec<RatchetCheckRow> = Vec::new();
    let mut all_pass = true;
    for r in &targets {
        let (current, samples) =
            compute_success_rate(&conn, &r.target_kind, &r.target_value, check_window_days)?;
        let floor_tol = (r.baseline_value - r.threshold).max(0.0);
        let (verdict, note) = match current {
            None => (
                "insufficient_data".to_string(),
                Some(format!("no dispatches in last {}d", check_window_days)),
            ),
            Some(c) if c >= floor_tol => ("pass".to_string(), None),
            Some(c) => {
                all_pass = false;
                (
                    "fail".to_string(),
                    Some(format!(
                        "current {:.1}% < floor-tol {:.1}%",
                        c * 100.0,
                        floor_tol * 100.0,
                    )),
                )
            }
        };
        rows.push(RatchetCheckRow {
            target_kind: r.target_kind.clone(),
            target_value: r.target_value.clone(),
            metric: r.metric.clone(),
            baseline_value: r.baseline_value,
            threshold: r.threshold,
            current_value: current,
            current_sample_count: samples,
            current_window_days: check_window_days,
            floor_with_tolerance: floor_tol,
            verdict,
            note,
        });
    }

    if opts.human {
        emit_human(&format!(
            "Ratchet check ({} target(s), {}d window):",
            rows.len(),
            check_window_days
        ));
        for r in &rows {
            let tag = match r.verdict.as_str() {
                "pass" => "✓ pass",
                "fail" => "✗ FAIL",
                _ => "?  insufficient",
            };
            let target_disp = if r.target_kind == "global" {
                "global".to_string()
            } else {
                format!("{}:{}", r.target_kind, r.target_value)
            };
            emit_human(&format!(
                "  {:30}  {}  current={}  floor-tol={:.1}%  samples={}",
                target_disp,
                tag,
                r.current_value
                    .map(|v| format!("{:.1}%", v * 100.0))
                    .unwrap_or_else(|| "—".into()),
                r.floor_with_tolerance * 100.0,
                r.current_sample_count,
            ));
            if let Some(n) = &r.note {
                emit_human(&format!("                                    note: {}", n));
            }
        }
        if !all_pass {
            emit_human("\nRatchet check FAILED. Exit code 1.");
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(all_pass)
}

pub fn status(
    db_path: &PathBuf,
    target_filter: Option<(String, String)>,
    opts: &Opts,
) -> Result<()> {
    // Status is just a check with verdict-suppression — surfaces
    // current numbers without failing the CLI. Used for "where do we
    // stand?" queries outside CI.
    let _ = check(db_path, target_filter, DEFAULT_CHECK_WINDOW_DAYS, opts)?;
    Ok(())
}

pub const CHECK_WINDOW_DEFAULT: i64 = DEFAULT_CHECK_WINDOW_DAYS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_handles_all_kinds() {
        assert_eq!(parse_target("global").unwrap(), ("global".into(), "".into()));
        assert_eq!(parse_target("agent:triage").unwrap(), ("agent".into(), "triage".into()));
        assert_eq!(
            parse_target("runtime:claude").unwrap(),
            ("runtime".into(), "claude".into())
        );
    }

    #[test]
    fn parse_target_rejects_typos() {
        assert!(parse_target("agnet:foo").is_err());
        assert!(parse_target("foo").is_err());
        assert!(parse_target("agent:").is_err());
    }
}
