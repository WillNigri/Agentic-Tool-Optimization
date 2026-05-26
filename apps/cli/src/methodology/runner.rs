// v2.10.0 PR-3 — methodology fan-out engine.
//
// Expand a methodology's variant matrix into prompts × models × conditions
// × reps cells, dispatch each cell sequentially through the same `ato
// dispatch` CLI surface customers use directly (dogfood: the runner is
// just an orchestrator), capture the resulting execution_logs row, and
// write methodology_runs + methodology_run_dispatches with running
// totals + final composition.
//
// **Why shell out to our own binary instead of calling dispatch::run
// in-process:** the runner is a thin orchestrator over the same primitive
// customers compose by hand. By going through `ato dispatch`, the runner
// inherits every override flag, every grounding policy effect, every
// keychain ACL behavior as the customer's hand-run dispatches. Zero
// drift between "what the runner sees" and "what the customer sees".
// Sub-process overhead is ~milliseconds per dispatch — negligible against
// the seconds of LLM latency that follows.
//
// PR-3 scope (this file): sequential fan-out, dual-cost-accounting
// writes, basic progress emission. PR-4 adds the rubric scoring loop
// over each dispatch. PR-5 calibrates the provider_* rate constants
// against a real Railway month.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use uuid::Uuid;

use crate::db;
use crate::methodology::cost::CostRateCard;
use crate::methodology::rubric::Rubric;
use crate::methodology::types::{BillingMode, VariantMatrix};

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub billing_mode: BillingMode,
    pub max_dispatches: Option<u32>,
    pub stop_on_error: bool,
    /// When set, emit one JSON line per completed dispatch to stdout so
    /// the caller can stream progress. Default off (only the final
    /// summary prints).
    pub progress_jsonl: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            billing_mode: BillingMode::Byok,
            max_dispatches: None,
            stop_on_error: false,
            progress_jsonl: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub methodology_id: String,
    pub methodology_slug: String,
    pub started_at: String,
    pub ended_at: String,
    pub status: String,
    pub planned: u32,
    pub completed: u32,
    pub failed: u32,
    pub customer_cost_usd: f64,
    pub customer_tokens_in: i64,
    pub customer_tokens_out: i64,
    pub provider_total_cost_usd: f64,
    pub margin_usd: f64,
    pub duration_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantCell {
    pub prompt_idx: usize,
    pub model: String,
    pub condition: String,
    pub rep: u32,
}

/// Look up a methodology by slug + run it end-to-end. Returns the
/// RunSummary even on partial failure (status = "failed" but with the
/// dispatches that did complete still counted).
pub fn run_by_slug(
    methodology_slug: &str,
    db_path: &PathBuf,
    run_opts: &RunOptions,
) -> Result<RunSummary> {
    let conn = db::open_readwrite(db_path)?;
    let (methodology_id, variant_matrix_json, rubric_json) = conn
        .query_row(
            "SELECT id, variant_matrix, rubric FROM methodologies WHERE slug = ?1",
            params![methodology_slug],
            |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            )),
        )
        .with_context(|| {
            format!(
                "no methodology with slug '{}' — run `ato evaluations methodology list` to see what's defined",
                methodology_slug
            )
        })?;

    let matrix: VariantMatrix = serde_json::from_str(&variant_matrix_json)
        .context("parse variant_matrix from DB — methodology may be corrupted")?;
    // Parse rubric tolerantly: a methodology that pre-dates PR-4 (or has
    // a non-rubric JSON blob like `{"kind":"pending-pr4"}`) falls back to
    // Rubric::Pending so the runner can still execute without scoring.
    let rubric_value: serde_json::Value =
        serde_json::from_str(&rubric_json).unwrap_or(serde_json::Value::Null);
    let rubric: Rubric = Rubric::parse(&rubric_value).unwrap_or(Rubric::Pending);

    run_with_matrix(
        &conn,
        &methodology_id,
        methodology_slug,
        &matrix,
        &rubric,
        db_path,
        run_opts,
    )
}

fn run_with_matrix(
    conn: &Connection,
    methodology_id: &str,
    methodology_slug: &str,
    matrix: &VariantMatrix,
    rubric: &Rubric,
    db_path: &Path,
    run_opts: &RunOptions,
) -> Result<RunSummary> {
    let planned = matrix.total_dispatches();
    let planned_capped = run_opts.max_dispatches.unwrap_or(planned).min(planned);

    let run_id = Uuid::new_v4().to_string();
    let started_at = chrono::Utc::now().to_rfc3339();
    let started_clock = Instant::now();

    conn.execute(
        "INSERT INTO methodology_runs
            (id, methodology_id, customer_user_id, started_at, status,
             total_dispatches_planned, total_dispatches_completed,
             customer_billing_mode)
         VALUES (?1, ?2, NULL, ?3, 'running', ?4, 0, ?5)",
        params![
            &run_id,
            methodology_id,
            &started_at,
            planned_capped as i64,
            run_opts.billing_mode.as_str(),
        ],
    )
    .context("insert methodology_runs row")?;

    let cells = expand_cells(matrix);
    let mut completed: u32 = 0;
    let mut failed: u32 = 0;
    let mut customer_cost_usd: f64 = 0.0;
    let mut customer_tokens_in: i64 = 0;
    let mut customer_tokens_out: i64 = 0;
    let mut compute_seconds: f64 = 0.0;
    let mut bandwidth_bytes: i64 = 0;
    let mut judge_cost_usd: f64 = 0.0;
    let rates = CostRateCard::load_with_override();

    for cell in cells.iter().take(planned_capped as usize) {
        let prompt = matrix
            .prompts
            .get(cell.prompt_idx)
            .cloned()
            .unwrap_or_default();
        let cell_started = Instant::now();
        let cell_outcome = dispatch_cell(db_path, &prompt, cell, matrix.runtime.as_deref());
        let cell_elapsed = cell_started.elapsed().as_secs_f64();
        compute_seconds += cell_elapsed;

        match cell_outcome {
            Ok(record) => {
                completed += 1;
                customer_cost_usd += record.cost_usd;
                customer_tokens_in += record.tokens_in;
                customer_tokens_out += record.tokens_out;
                bandwidth_bytes += record.response_bytes;

                // PR-4: score this dispatch through the methodology's rubric.
                // Failures here never abort the run — a score of 0 lands
                // alongside the reason; the customer sees both.
                let response_text = record.response_text.clone().unwrap_or_default();
                let rubric_score = match rubric.score(&prompt, &response_text, db_path) {
                    Ok(s) => s,
                    Err(e) => crate::methodology::rubric::RubricScore::fail(format!(
                        "rubric scoring error: {}",
                        e
                    )),
                };
                judge_cost_usd += rubric_score.judge_cost_usd;
                let variant_cell_json =
                    serde_json::to_string(cell).unwrap_or_else(|_| "{}".to_string());
                let _ = conn.execute(
                    "INSERT OR REPLACE INTO methodology_run_dispatches
                        (methodology_run_id, execution_log_id, variant_cell, score)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        &run_id,
                        &record.execution_log_id,
                        &variant_cell_json,
                        rubric_score.score,
                    ],
                );
                if run_opts.progress_jsonl {
                    let _ = serde_json::to_string(&serde_json::json!({
                        "event": "dispatch_completed",
                        "run_id": run_id,
                        "execution_log_id": record.execution_log_id,
                        "variant_cell": cell,
                        "cost_usd": record.cost_usd,
                        "tokens_in": record.tokens_in,
                        "tokens_out": record.tokens_out,
                        "duration_ms": record.duration_ms,
                        "grounding_verdict": record.grounding_verdict,
                        "status": record.status,
                        "rubric_score": rubric_score.score,
                        "rubric_reason": rubric_score.reason,
                        "judge_cost_usd": rubric_score.judge_cost_usd,
                        "completed_so_far": completed,
                        "planned": planned_capped,
                    }))
                    .map(|s| println!("{}", s));
                }
            }
            Err(e) => {
                failed += 1;
                if run_opts.progress_jsonl {
                    let _ = serde_json::to_string(&serde_json::json!({
                        "event": "dispatch_failed",
                        "run_id": run_id,
                        "variant_cell": cell,
                        "error": e.to_string(),
                        "completed_so_far": completed,
                        "failed_so_far": failed,
                        "planned": planned_capped,
                    }))
                    .map(|s| println!("{}", s));
                }
                if run_opts.stop_on_error {
                    break;
                }
            }
        }
    }

    // Provider-side cost accounting. PR-3 fills the columns the spec
    // requires NOT NULL. PR-5 will calibrate the rate constants against
    // a real Railway month; for now the published rate card values do
    // double duty as estimate AND ledger entries — same numbers the
    // customer saw at cost-estimate time, no surprises post-run.
    let provider_llm_cost_usd = match run_opts.billing_mode {
        BillingMode::Byok => 0.0,
        BillingMode::Pool => customer_cost_usd, // Pool mode = WE paid the LLM bill.
    };
    let provider_compute_cost = compute_seconds * rates.compute_per_second_usd;
    let storage_bytes_estimate = (customer_tokens_in + customer_tokens_out) * 4; // ~4 bytes/token JSON
    // 28-day default retention matches the published rate-card defaults
    // pricing.json calls out — keeps cost-estimate (PR-2) and run-time
    // ledger (this file) on the same retention assumption so post-run
    // numbers match the pre-run preview.
    let retention_months = 28.0 / 30.0;
    let storage_cost = (storage_bytes_estimate as f64)
        * rates.storage_per_byte_month_usd
        * retention_months;
    let bandwidth_cost = (bandwidth_bytes as f64) * rates.bandwidth_per_byte_usd;
    let provider_total =
        provider_llm_cost_usd + judge_cost_usd + provider_compute_cost + storage_cost + bandwidth_cost;

    // Margin = what the customer's tier slot brought in minus our cost.
    // Pro tier monthly is $29 split across an estimated 100 runs/mo for
    // a heavy user → ~$0.29 per run. The number is calibrated in PR-5;
    // PR-3 just persists the column so downstream readers don't trip
    // over NULL.
    let per_run_pro_allocation = 0.29;
    let margin_usd = per_run_pro_allocation - provider_total;

    let duration_seconds = started_clock.elapsed().as_secs_f64();
    let ended_at = chrono::Utc::now().to_rfc3339();
    let final_status = if failed == 0 {
        "complete"
    } else if completed == 0 {
        "failed"
    } else {
        // Partial success — still mark complete so the row participates
        // in standard "give me last week's runs" queries. The failed
        // count is exposed verbatim on the receipt for honest reporting.
        "complete"
    };

    conn.execute(
        "UPDATE methodology_runs SET
            ended_at = ?1,
            status = ?2,
            total_dispatches_completed = ?3,
            customer_cost_usd = ?4,
            customer_tokens_in = ?5,
            customer_tokens_out = ?6,
            customer_dispatches = ?7,
            provider_llm_cost_usd = ?8,
            provider_judge_cost_usd = ?9,
            provider_compute_seconds = ?10,
            provider_storage_bytes = ?11,
            provider_bandwidth_bytes = ?12,
            provider_total_cost_usd = ?13,
            margin_usd = ?14
         WHERE id = ?15",
        params![
            &ended_at,
            final_status,
            completed as i64,
            customer_cost_usd,
            customer_tokens_in,
            customer_tokens_out,
            completed as i64,
            provider_llm_cost_usd,
            judge_cost_usd,
            compute_seconds,
            storage_bytes_estimate,
            bandwidth_bytes,
            provider_total,
            margin_usd,
            &run_id,
        ],
    )
    .context("finalize methodology_runs row")?;

    Ok(RunSummary {
        run_id,
        methodology_id: methodology_id.to_string(),
        methodology_slug: methodology_slug.to_string(),
        started_at,
        ended_at,
        status: final_status.to_string(),
        planned: planned_capped,
        completed,
        failed,
        customer_cost_usd,
        customer_tokens_in,
        customer_tokens_out,
        provider_total_cost_usd: provider_total,
        margin_usd,
        duration_seconds,
    })
}

fn expand_cells(matrix: &VariantMatrix) -> Vec<VariantCell> {
    let mut cells = Vec::new();
    let conditions: Vec<String> = if matrix.conditions.is_empty() {
        vec!["default".to_string()]
    } else {
        matrix.conditions.clone()
    };
    for (prompt_idx, _) in matrix.prompts.iter().enumerate() {
        for model in &matrix.models {
            for condition in &conditions {
                for rep in 0..matrix.reps_per_cell {
                    cells.push(VariantCell {
                        prompt_idx,
                        model: model.clone(),
                        condition: condition.clone(),
                        rep,
                    });
                }
            }
        }
    }
    cells
}

#[derive(Debug)]
struct DispatchRecord {
    execution_log_id: String,
    cost_usd: f64,
    tokens_in: i64,
    tokens_out: i64,
    #[allow(dead_code)]
    duration_ms: i64,
    response_bytes: i64,
    response_text: Option<String>,
    grounding_verdict: Option<String>,
    status: String,
}

/// Shell out to `ato dispatch` for one cell. Captures the resulting
/// execution_logs row via a unique per-call `session_id` UUID passed
/// through `--session <id>` and queried back. Code-review finding #1
/// from PR-12.7 (concurrency bug): the previous "rowid > before"
/// pattern raced under any concurrent writer — observed during the
/// n=30 background run when a parallel `ato evaluations methodology
/// diagnose` dispatch got captured as one of the runner's cells.
/// The session_id capture is bulletproof: even with N concurrent
/// runners writing simultaneously, each `SELECT WHERE session_id = ?`
/// returns the unique row that bears the runner's chosen UUID.
///
/// `runtime_override`: when `Some("claude" | "codex" | "gemini")`, use
/// the CLI runtime instead of auto-deriving the API provider from the
/// model name. Lets a methodology target a subscription path rather
/// than burn BYOK API keys.
fn dispatch_cell(
    db_path: &Path,
    prompt: &str,
    cell: &VariantCell,
    runtime_override: Option<&str>,
) -> Result<DispatchRecord> {
    // Per-dispatch sentinel UUID, stamped onto execution_logs via
    // `--war-room-id`. We use `--war-room-id` rather than `--session`
    // because the latter requires a pre-existing sessions row;
    // `--war-room-id` accepts any UUID-shaped string and lands on the
    // indexed `execution_logs.war_room_id` column. The semantic
    // stretch is intentional and documented at the read site below.
    let dispatch_war_room_id = Uuid::new_v4().to_string();

    let runtime = match runtime_override {
        Some(r) => r.to_string(),
        None => runtime_for_model(&cell.model),
    };
    // v2.11 PR-12.6 — Use the shared CLI-path resolver. Lets a dev
    // binary delegate to the prod binary via $ATO_CLI_PATH so the
    // runner can hit API providers without re-encrypting the keychain.
    let exe = crate::cli_path::resolve_ato_binary()
        .context("locate ato binary for runner shell-out")?;
    let mut cmd = Command::new(&exe);
    cmd.arg("dispatch")
        .arg(&runtime)
        .arg(prompt)
        .arg("--model")
        .arg(&cell.model)
        .arg("--quiet")
        .arg("--war-room-id")
        .arg(&dispatch_war_room_id);
    if cell.condition == "soft" || cell.condition == "strict" {
        cmd.arg("--mode-override").arg(&cell.condition);
    }
    // Forward the same DB path the runner is using. dispatch resolves
    // ~/.ato/local.db by default; passing --db here keeps the runner
    // self-contained against custom test DBs.
    cmd.arg("--db").arg(db_path);

    let output = cmd
        .output()
        .with_context(|| format!("spawn `ato dispatch {}`", runtime))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "dispatch {} failed (exit {:?}): {}",
            runtime,
            output.status.code(),
            stderr.trim()
        );
    }

    // PR-12.7 fix: query by the unique --war-room-id sentinel we
    // stamped on the outbound dispatch. Bulletproof under concurrency
    // — N parallel runners / diagnoses / judges hitting the same DB
    // each get back exactly the row they wrote. The semantic stretch
    // (we're not really running a war room) is the price we pay for
    // a sentinel field that's BOTH accepted on the dispatch CLI AND
    // queryable on execution_logs without inventing a new column.
    let conn = db::open_readonly(db_path)?;
    let row = conn
        .query_row(
            "SELECT id, cost_usd_estimated, tokens_in, tokens_out, duration_ms, response, status, grounding_verdict
             FROM execution_logs
             WHERE war_room_id = ?1
             ORDER BY rowid DESC
             LIMIT 1",
            params![&dispatch_war_room_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<f64>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .with_context(|| format!("read execution_logs row for war_room_id={}", dispatch_war_room_id))?;

    let response_bytes = row.5.as_ref().map(|s| s.len() as i64).unwrap_or(0);
    Ok(DispatchRecord {
        execution_log_id: row.0,
        cost_usd: row.1.unwrap_or(0.0),
        tokens_in: row.2.unwrap_or(0),
        tokens_out: row.3.unwrap_or(0),
        duration_ms: row.4.unwrap_or(0),
        response_bytes,
        response_text: row.5,
        status: row.6.unwrap_or_else(|| "unknown".to_string()),
        grounding_verdict: row.7,
    })
}

/// Map a model identifier to the API-provider runtime name `ato dispatch`
/// accepts (anthropic / google / openai / etc.). Falls back to "claude"
/// when the provider can't be determined — matches the default-runtime
/// behavior of `ato dispatch` with no runtime arg.
fn runtime_for_model(model: &str) -> String {
    match ato_pricing::provider_for_model(model) {
        Some(p) => p.to_string(),
        None => "claude".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix() -> VariantMatrix {
        VariantMatrix {
            prompts: vec!["p1".to_string(), "p2".to_string()],
            models: vec!["claude-sonnet-4-6".to_string(), "claude-opus-4-7".to_string()],
            conditions: vec!["soft".to_string(), "strict".to_string()],
            reps_per_cell: 3,
            runtime: None,
            holdout_prompts: Vec::new(),
        }
    }

    #[test]
    fn expand_cells_produces_full_cartesian_product() {
        let m = matrix();
        let cells = expand_cells(&m);
        // 2 prompts × 2 models × 2 conditions × 3 reps = 24
        assert_eq!(cells.len(), 24);
    }

    #[test]
    fn expand_cells_respects_empty_conditions_as_one_default_cell() {
        let m = VariantMatrix {
            prompts: vec!["p1".to_string()],
            models: vec!["claude-sonnet-4-6".to_string()],
            conditions: vec![],
            reps_per_cell: 5,
            runtime: None,
            holdout_prompts: Vec::new(),
        };
        let cells = expand_cells(&m);
        assert_eq!(cells.len(), 5);
        assert_eq!(cells[0].condition, "default");
    }

    #[test]
    fn expand_cells_orders_reps_innermost() {
        let m = VariantMatrix {
            prompts: vec!["p".to_string()],
            models: vec!["claude-sonnet-4-6".to_string()],
            conditions: vec!["soft".to_string()],
            reps_per_cell: 3,
            runtime: None,
            holdout_prompts: Vec::new(),
        };
        let cells = expand_cells(&m);
        assert_eq!(cells[0].rep, 0);
        assert_eq!(cells[1].rep, 1);
        assert_eq!(cells[2].rep, 2);
    }

    #[test]
    fn runtime_for_model_maps_known_prefixes() {
        assert_eq!(runtime_for_model("claude-sonnet-4-6"), "anthropic");
        assert_eq!(runtime_for_model("gemini-2.5-pro"), "google");
        assert_eq!(runtime_for_model("gpt-5"), "openai");
    }

    #[test]
    fn runtime_for_model_falls_back_to_claude_for_unknown() {
        assert_eq!(runtime_for_model("some-future-model"), "claude");
    }

    #[test]
    fn run_options_default_to_byok_and_no_cap() {
        let opts = RunOptions::default();
        assert_eq!(opts.billing_mode, BillingMode::Byok);
        assert!(opts.max_dispatches.is_none());
        assert!(!opts.stop_on_error);
        assert!(!opts.progress_jsonl);
    }
}
