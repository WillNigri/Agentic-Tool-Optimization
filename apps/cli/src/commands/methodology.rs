// v2.10.0 PR-2 — `ato evaluations methodology …` CLI surface.
//
// Local-first CRUD over the methodology tables shipped in v2.10 PR-1
// (apps/desktop/src-tauri/src/schema.rs: methodologies, methodology_runs,
// methodology_run_dispatches). The fan-out engine + composer (PR-3) and
// the rubric library (PR-4) come later; this PR ships only the surface
// that lets a user (or AI agent via MCP, eventually) define / list /
// inspect methodologies + see what the run would cost before paying for
// it.
//
// Subcommands shipped here:
//
//   ato evaluations methodology create <slug> --config <file.json>
//     Load a methodology JSON config (variant matrix + rubric) and
//     INSERT into the `methodologies` table.
//
//   ato evaluations methodology list [--archetype which-model]
//     SELECT methodologies — print one row per methodology with slug,
//     archetype, created_at, and dispatch-count if it would run today.
//
//   ato evaluations methodology get <slug>
//     SELECT one methodology — print the full record (variant matrix +
//     rubric expanded as JSON so the user can audit or copy-edit).
//
//   ato evaluations methodology archetypes
//     Print the built-in archetype catalog from
//     apps/cli/src/methodology/archetypes.rs (no DB read — pure registry).
//
//   ato evaluations methodology cost-estimate <slug> [--billing pool|byok]
//     Read the methodology + use apps/cli/src/methodology/cost.rs to
//     compute the pre-run cost estimate (customer + provider). Required
//     before fan-out per the methodology-runner spec.
//
// All commands default to JSON output (machine-readable, MCP-friendly).
// `--human` switches to readable terminal formatting (mirrors the rest
// of the CLI surface).

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::db;
use crate::methodology::compose;
use crate::methodology::rubric::Rubric;
use crate::methodology::runner::{self, RunOptions};
use crate::methodology::{
    cost_estimate_for_matrix, Archetype, BillingMode, CostRateCard, VariantMatrix,
};
use crate::output::{emit_human, emit_json, Opts};

#[derive(Args, Debug)]
pub struct EvaluationsArgs {
    #[command(subcommand)]
    pub sub: EvaluationsSub,
}

#[derive(Subcommand, Debug)]
pub enum EvaluationsSub {
    /// Methodology definitions — reusable test recipes. Methodologies
    /// describe a variant matrix (prompts × models × conditions × reps)
    /// and a rubric (how each dispatch is scored). Running a methodology
    /// produces composed results with confidence intervals and dual
    /// cost accounting. The fan-out runner lands in v2.10 PR-3; this
    /// PR ships create / list / get / archetypes / cost-estimate.
    Methodology(MethodologyArgs),
}

#[derive(Args, Debug)]
pub struct MethodologyArgs {
    #[command(subcommand)]
    pub sub: MethodologySub,
}

#[derive(Subcommand, Debug)]
pub enum MethodologySub {
    /// Create a new methodology from a JSON config file or stdin.
    ///
    /// The config file shape is documented in
    /// docs/methodology-runner.md §schema. Minimal example:
    ///
    /// ```json
    /// {
    ///   "slug": "claude-vs-gemini-security",
    ///   "description": "Quarterly model-ladder for security reviews",
    ///   "archetype": "model-ladder",
    ///   "variant_matrix": {
    ///     "prompts": ["Review src/auth.ts", "Audit src/db.ts"],
    ///     "models": ["claude-sonnet-4-6", "gemini-2.5-pro"],
    ///     "conditions": ["soft"],
    ///     "reps_per_cell": 30
    ///   },
    ///   "rubric": {"kind": "regex", "pattern": "(?i)vulnerability"}
    /// }
    /// ```
    Create {
        /// Methodology slug (URL-safe identifier, must be unique in this
        /// database). Overrides the `slug` field in the config file when
        /// both are present.
        slug: Option<String>,
        /// Path to a JSON config file. Use `-` to read from stdin.
        #[arg(long, short)]
        config: PathBuf,
    },
    /// List all methodologies in the local DB.
    List {
        /// Filter by archetype (e.g. `--archetype model-ladder`).
        #[arg(long)]
        archetype: Option<String>,
    },
    /// Print one methodology's full record (variant matrix + rubric).
    Get {
        /// Methodology slug to look up.
        slug: String,
    },
    /// Print the built-in archetype catalog. No DB read — these are the
    /// pre-built templates shipped with v2.10. Use `cost-estimate` after
    /// `create` to see what an archetype-shaped run would cost.
    Archetypes,
    /// Compute the pre-run cost estimate for a methodology before
    /// fan-out. Required by the methodology-runner spec: every methodology
    /// run must surface customer-cost + our-cost before the customer
    /// commits. See docs/methodology-runner.md §transparency.
    CostEstimate {
        /// Methodology slug to estimate. Pull its variant matrix from
        /// the DB; combine with the published rate card to produce
        /// the estimate.
        slug: String,
        /// Billing mode for the estimate. `byok` (default) assumes the
        /// customer's own API keys pay the LLM bill. `pool` assumes
        /// our shared Pro pool key pays.
        #[arg(long, default_value = "byok")]
        billing: String,
        /// Number of LLM-judge calls per dispatch. Default 0 (rule-based
        /// rubric). Set to 1 for a single LLM-judge call per dispatch,
        /// higher for composite rubrics.
        #[arg(long, default_value_t = 0)]
        judge_calls: u32,
    },
    /// v2.10 PR-3 — run a methodology. Fans out the variant matrix
    /// sequentially via `ato dispatch`, captures the resulting
    /// execution_logs rows into methodology_run_dispatches, and updates
    /// methodology_runs with dual cost accounting. The rubric scoring
    /// loop lands in PR-4. Use `--max-dispatches N` for smoke tests
    /// before burning the full matrix.
    Run {
        /// Methodology slug to execute.
        slug: String,
        /// Billing mode for THIS run. `byok` (default): customer's API
        /// keys pay. `pool`: our shared Pro pool key pays — fills
        /// `provider_llm_cost_usd` with the burn.
        #[arg(long, default_value = "byok")]
        billing: String,
        /// Cap the run at the first N dispatches (smoke testing).
        /// Default: no cap (run the full matrix).
        #[arg(long)]
        max_dispatches: Option<u32>,
        /// Abort the run on the first failed dispatch.
        /// Default: continue and record the failure.
        #[arg(long, default_value_t = false)]
        stop_on_error: bool,
        /// Emit one JSON line per completed dispatch to stdout so callers
        /// can stream progress. Default off — only the final summary.
        #[arg(long, default_value_t = false)]
        progress_jsonl: bool,
    },
    /// v2.10 PR-3 — inspect methodology runs. Default lists recent runs;
    /// pass `--run-id` (or a positional run id) to drill into one run's
    /// composition (per-cell stats + pairwise Welch t).
    Runs(RunsArgs),
    /// v2.10 PR-4 — score every dispatch in an existing run using the
    /// methodology's rubric. Idempotent — re-running re-scores. Costs
    /// of LLM-judge calls land in `provider_judge_cost_usd`. Use this
    /// after `adopt` (which intentionally skips scoring to avoid
    /// surprise judge spend).
    Score {
        /// Run id (UUID).
        run_id: String,
        /// Re-score dispatches that already have a non-NULL score. Off
        /// by default — the typical loop is adopt → score once.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// v2.10 PR-3 (Pro angle) — adopt EXISTING execution_logs into a
    /// methodology_run without re-dispatching. Lets a customer compose
    /// + (PR-4) score receipts they already paid for. Variant cell is
    /// derived from the row: prompt_idx by distinct prompt, model
    /// straight from the row, condition from grounding_verdict (or
    /// "default" if pre-grounding).
    Adopt {
        /// Methodology slug to attach the adopted run to.
        slug: String,
        /// Lower bound on execution_logs.created_at (ISO-8601 or YYYY-MM-DD).
        #[arg(long)]
        since: Option<String>,
        /// Upper bound on execution_logs.created_at.
        #[arg(long)]
        until: Option<String>,
        /// Filter by runtime (e.g. `--runtime claude`).
        #[arg(long)]
        runtime: Option<String>,
        /// Filter by model (e.g. `--model claude-sonnet-4-6`).
        #[arg(long)]
        model: Option<String>,
        /// Filter by status (default `success`; pass `all` to include errors).
        #[arg(long, default_value = "success")]
        status: String,
        /// Filter by agent slug.
        #[arg(long)]
        agent: Option<String>,
        /// Hard cap on adopted rows. Default 500 — keeps adopt from
        /// silently swallowing a whole month of dispatches.
        #[arg(long, default_value_t = 500)]
        limit: u32,
        /// Billing mode tag for the adopted run.
        #[arg(long, default_value = "byok")]
        billing: String,
    },
}

#[derive(Args, Debug)]
pub struct RunsArgs {
    #[command(subcommand)]
    pub sub: RunsSub,
}

#[derive(Subcommand, Debug)]
pub enum RunsSub {
    /// List recent methodology runs. Newest first.
    List {
        /// Filter to runs of one methodology (by slug).
        #[arg(long)]
        methodology: Option<String>,
        /// Limit number of rows returned. Default 50.
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Print one run's full composition: per-cell stats and pairwise
    /// Welch t over the cost metric. Until PR-4 lands rubric scoring,
    /// composition operates over receipt-native fields (cost, tokens,
    /// duration) + grounding-verdict mix.
    Show {
        /// Run id (UUID returned by `ato evaluations methodology run`).
        run_id: String,
    },
}

/// Config file shape — what a customer writes when they run `create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodologyConfig {
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub archetype: String,
    pub variant_matrix: VariantMatrix,
    pub rubric: serde_json::Value,
}

pub fn run(args: EvaluationsArgs, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    match args.sub {
        EvaluationsSub::Methodology(margs) => match margs.sub {
            MethodologySub::Create { slug, config } => handle_create(slug, config, db_path, opts),
            MethodologySub::List { archetype } => handle_list(archetype, db_path, opts),
            MethodologySub::Get { slug } => handle_get(slug, db_path, opts),
            MethodologySub::Archetypes => handle_archetypes(opts),
            MethodologySub::CostEstimate {
                slug,
                billing,
                judge_calls,
            } => handle_cost_estimate(slug, billing, judge_calls, db_path, opts),
            MethodologySub::Run {
                slug,
                billing,
                max_dispatches,
                stop_on_error,
                progress_jsonl,
            } => handle_run(
                slug,
                billing,
                max_dispatches,
                stop_on_error,
                progress_jsonl,
                db_path,
                opts,
            ),
            MethodologySub::Runs(runs_args) => match runs_args.sub {
                RunsSub::List { methodology, limit } => {
                    handle_runs_list(methodology, limit, db_path, opts)
                }
                RunsSub::Show { run_id } => handle_runs_show(run_id, db_path, opts),
            },
            MethodologySub::Adopt {
                slug,
                since,
                until,
                runtime,
                model,
                status,
                agent,
                limit,
                billing,
            } => handle_adopt(
                slug, since, until, runtime, model, status, agent, limit, billing, db_path, opts,
            ),
            MethodologySub::Score { run_id, force } => handle_score(run_id, force, db_path, opts),
        },
    }
}

fn read_config(config: &PathBuf) -> Result<MethodologyConfig> {
    let raw = if config.as_os_str() == "-" {
        let mut buf = String::new();
        use std::io::Read;
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("read methodology config from stdin")?;
        buf
    } else {
        std::fs::read_to_string(config)
            .with_context(|| format!("read methodology config from {}", config.display()))?
    };
    parse_config_str(&raw)
}

/// Parse a methodology config from a string. Extracted from `read_config`
/// so tests can exercise the parser + validator without round-tripping
/// through tempfiles or stdin.
fn parse_config_str(raw: &str) -> Result<MethodologyConfig> {
    let cfg: MethodologyConfig = serde_json::from_str(raw)
        .context("parse methodology config JSON — see docs/methodology-runner.md for shape")?;
    // Reject unknown archetype slugs early — better to fail at create
    // time than have the runner reject later. `custom` is always valid.
    if Archetype::parse(&cfg.archetype).is_none() {
        anyhow::bail!(
            "unknown archetype '{}'. Run `ato evaluations methodology archetypes` to see valid values.",
            cfg.archetype
        );
    }
    Ok(cfg)
}

fn handle_create(
    slug_override: Option<String>,
    config: PathBuf,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let cfg = read_config(&config)?;
    let slug = slug_override
        .or(cfg.slug.clone())
        .ok_or_else(|| anyhow::anyhow!("methodology slug required (pass as positional arg or include in config file)"))?;

    // Re-serialize variant_matrix to canonical JSON for the DB. The
    // rubric stays whatever shape the user passed (PR-4 defines the
    // formal rubric schema; PR-2 stores it opaquely).
    let variant_matrix_json = serde_json::to_string(&cfg.variant_matrix)
        .context("serialize variant_matrix")?;
    let rubric_json = serde_json::to_string(&cfg.rubric).context("serialize rubric")?;

    let id = Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    let conn = db::open_readwrite(db_path)?;

    // INSERT — UNIQUE(slug) constraint catches duplicates with a clean
    // SQLite error; convert it to a user-facing message.
    let result = conn.execute(
        "INSERT INTO methodologies
            (id, slug, description, archetype, variant_matrix, rubric, created_at, created_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
        params![
            &id,
            &slug,
            cfg.description.as_deref(),
            &cfg.archetype,
            &variant_matrix_json,
            &rubric_json,
            &created_at,
        ],
    );

    match result {
        Ok(_) => {
            if opts.human {
                emit_human(&format!(
                    "Created methodology '{}' ({}). Variant matrix: {} prompts × {} models × {} conditions × {} reps = {} dispatches per run.",
                    slug,
                    id,
                    cfg.variant_matrix.prompts.len(),
                    cfg.variant_matrix.models.len(),
                    cfg.variant_matrix.conditions.len(),
                    cfg.variant_matrix.reps_per_cell,
                    cfg.variant_matrix.total_dispatches(),
                ));
            } else {
                let _ = emit_json(&serde_json::json!({
                    "id": id,
                    "slug": slug,
                    "archetype": cfg.archetype,
                    "total_dispatches_per_run": cfg.variant_matrix.total_dispatches(),
                    "created_at": created_at,
                }));
            }
            Ok(())
        }
        Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::ConstraintViolation => {
            anyhow::bail!(
                "methodology slug '{}' already exists in this database. \
                 Pick a different slug or `ato evaluations methodology get {}` to inspect it.",
                slug, slug
            )
        }
        Err(e) => Err(e).context("insert methodology"),
    }
}

fn handle_list(archetype_filter: Option<String>, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let mut stmt = if archetype_filter.is_some() {
        conn.prepare(
            "SELECT id, slug, description, archetype, variant_matrix, created_at
             FROM methodologies
             WHERE archetype = ?1
             ORDER BY created_at DESC",
        )?
    } else {
        conn.prepare(
            "SELECT id, slug, description, archetype, variant_matrix, created_at
             FROM methodologies
             ORDER BY created_at DESC",
        )?
    };

    let map_row = |r: &rusqlite::Row| -> rusqlite::Result<MethodologyListRow> {
        let variant_matrix_json: String = r.get(4)?;
        let variant_matrix: Option<VariantMatrix> = serde_json::from_str(&variant_matrix_json).ok();
        Ok(MethodologyListRow {
            id: r.get(0)?,
            slug: r.get(1)?,
            description: r.get(2)?,
            archetype: r.get(3)?,
            total_dispatches_per_run: variant_matrix
                .as_ref()
                .map(VariantMatrix::total_dispatches)
                .unwrap_or(0),
            created_at: r.get(5)?,
        })
    };

    let rows: Vec<MethodologyListRow> = if let Some(arch) = archetype_filter {
        stmt.query_map([&arch], map_row)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map([], map_row)?
            .filter_map(|r| r.ok())
            .collect()
    };

    if opts.human {
        if rows.is_empty() {
            emit_human("(no methodologies defined yet — `ato evaluations methodology create` to add one)");
        } else {
            emit_human(&format!("{} methodologies:", rows.len()));
            for row in &rows {
                emit_human(&format!(
                    "  {}  [{}]  {} dispatches/run  ·  {}",
                    row.slug,
                    row.archetype,
                    row.total_dispatches_per_run,
                    row.description.as_deref().unwrap_or("(no description)"),
                ));
            }
        }
    } else {
        let _ = emit_json(&rows);
    }
    Ok(())
}

#[derive(Serialize)]
struct MethodologyListRow {
    id: String,
    slug: String,
    description: Option<String>,
    archetype: String,
    total_dispatches_per_run: u32,
    created_at: String,
}

fn handle_get(slug: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let result = conn.query_row(
        "SELECT id, slug, description, archetype, variant_matrix, rubric, created_at, created_by
         FROM methodologies
         WHERE slug = ?1",
        params![&slug],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        },
    );

    match result {
        Ok((id, slug, description, archetype, vm_json, rubric_json, created_at, created_by)) => {
            let variant_matrix: serde_json::Value =
                serde_json::from_str(&vm_json).unwrap_or(serde_json::Value::Null);
            let rubric: serde_json::Value =
                serde_json::from_str(&rubric_json).unwrap_or(serde_json::Value::Null);
            let dispatches = serde_json::from_str::<VariantMatrix>(&vm_json)
                .map(|vm| vm.total_dispatches())
                .unwrap_or(0);
            if opts.human {
                emit_human(&format!("Methodology: {}\n  id:            {}\n  archetype:     {}\n  description:   {}\n  created:       {}{}\n  dispatches/run: {}\n\nvariant_matrix:\n{}\n\nrubric:\n{}",
                    slug,
                    id,
                    archetype,
                    description.as_deref().unwrap_or("(none)"),
                    created_at,
                    created_by.as_ref().map(|b| format!("\n  created_by:    {}", b)).unwrap_or_default(),
                    dispatches,
                    serde_json::to_string_pretty(&variant_matrix).unwrap_or_default(),
                    serde_json::to_string_pretty(&rubric).unwrap_or_default(),
                ));
            } else {
                let _ = emit_json(&serde_json::json!({
                    "id": id,
                    "slug": slug,
                    "description": description,
                    "archetype": archetype,
                    "variant_matrix": variant_matrix,
                    "rubric": rubric,
                    "total_dispatches_per_run": dispatches,
                    "created_at": created_at,
                    "created_by": created_by,
                }));
            }
            Ok(())
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            anyhow::bail!(
                "no methodology with slug '{}'. Run `ato evaluations methodology list` to see what's defined.",
                slug
            )
        }
        Err(e) => Err(e).context("query methodology"),
    }
}

fn handle_archetypes(opts: &Opts) -> Result<()> {
    let archetypes = [
        Archetype::ModelLadder,
        Archetype::ToolsVsNoTools,
        Archetype::ReviewerOrderEffects,
        Archetype::RegressionWatch,
        Archetype::Custom,
    ];
    if opts.human {
        emit_human("Built-in methodology archetypes (v2.10):\n");
        for a in archetypes.iter() {
            emit_human(&format!(
                "  {:<25}  default_reps_per_cell: {}\n    {}\n    {}\n",
                a.as_str(),
                a.default_reps_per_cell(),
                a.label(),
                a.description(),
            ));
        }
    } else {
        let rows: Vec<_> = archetypes
            .iter()
            .map(|a| {
                serde_json::json!({
                    "slug": a.as_str(),
                    "label": a.label(),
                    "description": a.description(),
                    "default_reps_per_cell": a.default_reps_per_cell(),
                })
            })
            .collect();
        let _ = emit_json(&rows);
    }
    Ok(())
}

fn handle_cost_estimate(
    slug: String,
    billing: String,
    judge_calls: u32,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let billing_mode = BillingMode::parse(&billing).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown billing mode '{}'. Valid values: byok | pool",
            billing
        )
    })?;
    let conn = db::open_readonly(db_path)?;
    let variant_matrix_json: String = conn
        .query_row(
            "SELECT variant_matrix FROM methodologies WHERE slug = ?1",
            params![&slug],
            |r| r.get(0),
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "no methodology with slug '{}'. Run `ato evaluations methodology list` to see what's defined.",
                slug
            )
        })?;
    let matrix: VariantMatrix = serde_json::from_str(&variant_matrix_json)
        .context("parse variant_matrix from DB — methodology may be corrupted")?;

    let rates = CostRateCard::defaults_v1();
    let estimate = cost_estimate_for_matrix(&matrix, &rates, billing_mode, judge_calls);

    if opts.human {
        emit_human(&format!(
            "Cost estimate for methodology '{}':\n\
             \n\
             Variant matrix:    {} prompts × {} models × {} conditions × {} reps = {} dispatches\n\
             \n\
             YOUR estimated LLM spend (billing={}):",
            slug,
            matrix.prompts.len(),
            matrix.models.len(),
            matrix.conditions.len(),
            matrix.reps_per_cell,
            estimate.total_dispatches,
            billing_mode.as_str(),
        ));
        for share in &estimate.customer_by_model {
            emit_human(&format!(
                "  {:<25}  {} dispatches  ~{} tok in + {} tok out  ${:.4}",
                share.model,
                share.dispatches,
                share.tokens_in_estimate,
                share.tokens_out_estimate,
                share.customer_cost_usd,
            ));
        }
        emit_human(&format!(
            "  ─────────────────────────────────────────\n  YOUR total:    ${:.4}\n\n\
             OUR cost to deliver (your Pro tier covers this):\n  \
               LLM (provider pool):  ${:.4}\n  \
               LLM-judge calls:      ${:.4} ({} judge calls per dispatch × {})\n  \
               Orchestrator compute: ${:.4}\n  \
               Storage (28d):        ${:.4}\n  \
               Bandwidth:            ${:.4}\n  \
             ─────────────────────────────────────────\n  OUR total:     ${:.4}\n\n\
             Tier fit: {:?}",
            estimate.customer_cost_usd,
            estimate.provider.llm_cost_usd,
            estimate.provider.judge_cost_usd,
            judge_calls,
            estimate.total_dispatches,
            estimate.provider.compute_cost_usd,
            estimate.provider.storage_cost_usd,
            estimate.provider.bandwidth_cost_usd,
            estimate.provider.total_usd,
            estimate.fits_in_tier,
        ));
    } else {
        let _ = emit_json(&estimate);
    }
    Ok(())
}

fn handle_run(
    slug: String,
    billing: String,
    max_dispatches: Option<u32>,
    stop_on_error: bool,
    progress_jsonl: bool,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let billing_mode = BillingMode::parse(&billing).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown billing mode '{}'. Valid values: byok | pool",
            billing
        )
    })?;
    let run_opts = RunOptions {
        billing_mode,
        max_dispatches,
        stop_on_error,
        progress_jsonl,
    };
    if opts.human {
        emit_human(&format!(
            "Starting methodology run for '{}' (billing={}{}){}",
            slug,
            billing_mode.as_str(),
            max_dispatches
                .map(|n| format!(", cap={}", n))
                .unwrap_or_default(),
            if progress_jsonl {
                " — progress streaming on"
            } else {
                ""
            },
        ));
    }
    let summary = runner::run_by_slug(&slug, db_path, &run_opts)?;
    if opts.human {
        emit_human(&format!(
            "\nRun {} {}\n  planned:        {}\n  completed:      {}\n  failed:         {}\n  duration:       {:.1}s\n  YOUR cost:      ${:.4}\n  OUR cost:       ${:.4}\n  margin (est):   ${:.4}",
            summary.run_id,
            summary.status,
            summary.planned,
            summary.completed,
            summary.failed,
            summary.duration_seconds,
            summary.customer_cost_usd,
            summary.provider_total_cost_usd,
            summary.margin_usd,
        ));
        emit_human(&format!(
            "\nNext: `ato evaluations methodology runs show {}` for the per-cell composition.",
            summary.run_id
        ));
    } else {
        let _ = emit_json(&summary);
    }
    Ok(())
}

fn handle_runs_list(
    methodology_filter: Option<String>,
    limit: u32,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    // Join in methodology slug so the list is human-readable without
    // a second query per row.
    let (sql, has_filter) = if methodology_filter.is_some() {
        (
            "SELECT r.id, m.slug, r.started_at, r.ended_at, r.status,
                    r.total_dispatches_planned, r.total_dispatches_completed,
                    r.customer_cost_usd, r.provider_total_cost_usd, r.margin_usd
             FROM methodology_runs r
             JOIN methodologies m ON m.id = r.methodology_id
             WHERE m.slug = ?1
             ORDER BY r.started_at DESC
             LIMIT ?2",
            true,
        )
    } else {
        (
            "SELECT r.id, m.slug, r.started_at, r.ended_at, r.status,
                    r.total_dispatches_planned, r.total_dispatches_completed,
                    r.customer_cost_usd, r.provider_total_cost_usd, r.margin_usd
             FROM methodology_runs r
             JOIN methodologies m ON m.id = r.methodology_id
             ORDER BY r.started_at DESC
             LIMIT ?1",
            false,
        )
    };
    let mut stmt = conn.prepare(sql)?;
    let map_row = |r: &rusqlite::Row| -> rusqlite::Result<serde_json::Value> {
        Ok(serde_json::json!({
            "run_id": r.get::<_, String>(0)?,
            "methodology_slug": r.get::<_, String>(1)?,
            "started_at": r.get::<_, String>(2)?,
            "ended_at": r.get::<_, Option<String>>(3)?,
            "status": r.get::<_, String>(4)?,
            "planned": r.get::<_, i64>(5)?,
            "completed": r.get::<_, i64>(6)?,
            "customer_cost_usd": r.get::<_, f64>(7)?,
            "provider_total_cost_usd": r.get::<_, f64>(8)?,
            "margin_usd": r.get::<_, f64>(9)?,
        }))
    };
    let rows: Vec<serde_json::Value> = if has_filter {
        let m = methodology_filter.unwrap();
        stmt.query_map(params![&m, limit as i64], map_row)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map(params![limit as i64], map_row)?
            .filter_map(|r| r.ok())
            .collect()
    };
    if opts.human {
        if rows.is_empty() {
            emit_human("(no methodology runs yet — `ato evaluations methodology run <slug>` to start one)");
        } else {
            emit_human(&format!("{} runs:", rows.len()));
            for r in &rows {
                emit_human(&format!(
                    "  {}  [{}]  {}  {}/{} dispatches  ${:.4} customer / ${:.4} ours",
                    r["run_id"].as_str().unwrap_or(""),
                    r["methodology_slug"].as_str().unwrap_or(""),
                    r["status"].as_str().unwrap_or(""),
                    r["completed"].as_i64().unwrap_or(0),
                    r["planned"].as_i64().unwrap_or(0),
                    r["customer_cost_usd"].as_f64().unwrap_or(0.0),
                    r["provider_total_cost_usd"].as_f64().unwrap_or(0.0),
                ));
            }
        }
    } else {
        let _ = emit_json(&rows);
    }
    Ok(())
}

fn handle_runs_show(run_id: String, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readonly(db_path)?;
    let (methodology_slug, started_at, ended_at, status, planned, completed,
         customer_cost_usd, customer_tokens_in, customer_tokens_out,
         provider_total_cost_usd, margin_usd, billing_mode): (
        String, String, Option<String>, String, i64, i64, f64, i64, i64, f64, f64, String,
    ) = conn
        .query_row(
            "SELECT m.slug, r.started_at, r.ended_at, r.status,
                    r.total_dispatches_planned, r.total_dispatches_completed,
                    r.customer_cost_usd, r.customer_tokens_in, r.customer_tokens_out,
                    r.provider_total_cost_usd, r.margin_usd, r.customer_billing_mode
             FROM methodology_runs r
             JOIN methodologies m ON m.id = r.methodology_id
             WHERE r.id = ?1",
            params![&run_id],
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
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                ))
            },
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "no methodology run with id '{}'. `ato evaluations methodology runs list` to see what's there.",
                run_id
            )
        })?;

    // Pull every dispatch this run composed + the execution_logs metrics
    // we need for composition. One join keeps the round-trip count to 1.
    let mut stmt = conn.prepare(
        "SELECT mrd.variant_cell, mrd.score,
                e.cost_usd_estimated, e.tokens_in, e.tokens_out,
                e.duration_ms, e.status, e.grounding_verdict
         FROM methodology_run_dispatches mrd
         JOIN execution_logs e ON e.id = mrd.execution_log_id
         WHERE mrd.methodology_run_id = ?1",
    )?;
    let observations: Vec<compose::CellObservation> = stmt
        .query_map(params![&run_id], |r| {
            let vc_json: String = r.get(0)?;
            let score_opt: Option<f64> = r.get(1)?;
            let cost: Option<f64> = r.get(2)?;
            let tokens_in: Option<i64> = r.get(3)?;
            let tokens_out: Option<i64> = r.get(4)?;
            let duration_ms: Option<i64> = r.get(5)?;
            let status: Option<String> = r.get(6)?;
            let verdict: Option<String> = r.get(7)?;
            let cell: serde_json::Value =
                serde_json::from_str(&vc_json).unwrap_or(serde_json::Value::Null);
            let prompt_idx = cell.get("prompt_idx").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let model = cell
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown)")
                .to_string();
            let condition = cell
                .get("condition")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();
            let _ = tokens_in;
            Ok(compose::CellObservation {
                prompt_idx,
                model,
                condition,
                cost_usd: cost.unwrap_or(0.0),
                tokens_out: tokens_out.unwrap_or(0) as f64,
                duration_ms: duration_ms.unwrap_or(0) as f64,
                grounding_verdict: verdict,
                status: status.unwrap_or_else(|| "unknown".to_string()),
                score: score_opt,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    let composition = compose::compose(&observations);

    if opts.human {
        emit_human(&format!(
            "Methodology run: {}\n  methodology:    {}\n  billing:        {}\n  status:         {}\n  started:        {}\n  ended:          {}\n  planned:        {}\n  completed:      {}\n  YOUR cost:      ${:.4}\n  YOUR tokens:    {} in / {} out\n  OUR cost:       ${:.4}\n  margin (est):   ${:.4}",
            run_id,
            methodology_slug,
            billing_mode,
            status,
            started_at,
            ended_at.as_deref().unwrap_or("(not finished)"),
            planned,
            completed,
            customer_cost_usd,
            customer_tokens_in,
            customer_tokens_out,
            provider_total_cost_usd,
            margin_usd,
        ));
        if composition.cells.is_empty() {
            emit_human("\n(no dispatches composed yet — run did not complete any cells)");
        } else {
            emit_human(&format!("\nPer-cell composition ({} cells):", composition.cells.len()));
            for c in &composition.cells {
                emit_human(&format!(
                    "  prompt[{}] · {} · {}  n={} (success={}, error={})\n    cost:     mean ${:.4}  sd ${:.4}  95% CI [${:.4}, ${:.4}]\n    tok_out:  mean {:.0}  sd {:.0}  95% CI [{:.0}, {:.0}]\n    duration: mean {:.0}ms  sd {:.0}ms  95% CI [{:.0}, {:.0}]ms",
                    c.prompt_idx, c.condition, c.model, c.n, c.success_n, c.error_n,
                    c.cost_usd.mean, c.cost_usd.sd, c.cost_usd.ci_lo, c.cost_usd.ci_hi,
                    c.tokens_out.mean, c.tokens_out.sd, c.tokens_out.ci_lo, c.tokens_out.ci_hi,
                    c.duration_ms.mean, c.duration_ms.sd, c.duration_ms.ci_lo, c.duration_ms.ci_hi,
                ));
                if let (Some(score), Some(passed)) = (&c.score, c.passed_at_0_5) {
                    emit_human(&format!(
                        "    score:    mean {:.3}  sd {:.3}  95% CI [{:.3}, {:.3}]  passed ≥0.5: {}/{}",
                        score.mean, score.sd, score.ci_lo, score.ci_hi, passed, score.n
                    ));
                }
                if !c.grounding_verdicts.is_empty() {
                    let vs: Vec<String> = c
                        .grounding_verdicts
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    emit_human(&format!("    grounding: {}", vs.join(" · ")));
                }
            }
            if !composition.model_pairs_cost_t.is_empty() {
                emit_human(&format!(
                    "\nPairwise cost comparisons (Welch t):\n  CI-disjoint pairs (heuristic 'real difference'): {}",
                    composition.model_pairs_cost_t.iter().filter(|p| p.ci_disjoint).count(),
                ));
                for p in &composition.model_pairs_cost_t {
                    emit_human(&format!(
                        "  prompt[{}] · {}: {} vs {}  t={:.2} df={:.1}  CI {}",
                        p.prompt_idx, p.condition, p.model_a, p.model_b,
                        p.t_statistic, p.welch_df,
                        if p.ci_disjoint { "disjoint" } else { "overlapping" },
                    ));
                }
            }
        }
    } else {
        let _ = emit_json(&serde_json::json!({
            "run_id": run_id,
            "methodology_slug": methodology_slug,
            "billing_mode": billing_mode,
            "status": status,
            "started_at": started_at,
            "ended_at": ended_at,
            "planned": planned,
            "completed": completed,
            "customer_cost_usd": customer_cost_usd,
            "customer_tokens_in": customer_tokens_in,
            "customer_tokens_out": customer_tokens_out,
            "provider_total_cost_usd": provider_total_cost_usd,
            "margin_usd": margin_usd,
            "composition": composition,
        }));
    }
    Ok(())
}

fn handle_adopt(
    slug: String,
    since: Option<String>,
    until: Option<String>,
    runtime: Option<String>,
    model: Option<String>,
    status: String,
    agent: Option<String>,
    limit: u32,
    billing: String,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let billing_mode = BillingMode::parse(&billing).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown billing mode '{}'. Valid values: byok | pool",
            billing
        )
    })?;
    let conn = db::open_readwrite(db_path)?;
    let methodology_id: String = conn
        .query_row(
            "SELECT id FROM methodologies WHERE slug = ?1",
            params![&slug],
            |r| r.get(0),
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "no methodology with slug '{}'. `ato evaluations methodology create` first.",
                slug
            )
        })?;

    // Build the WHERE clause dynamically — keep params() positional + safe.
    // Status special-cases: `all` skips the filter entirely.
    let mut where_clauses: Vec<String> = Vec::new();
    let mut bind: Vec<String> = Vec::new();
    if let Some(s) = &since {
        where_clauses.push(format!("created_at >= ?{}", bind.len() + 1));
        bind.push(s.clone());
    }
    if let Some(u) = &until {
        where_clauses.push(format!("created_at <= ?{}", bind.len() + 1));
        bind.push(u.clone());
    }
    if let Some(r) = &runtime {
        where_clauses.push(format!("runtime = ?{}", bind.len() + 1));
        bind.push(r.clone());
    }
    if let Some(m) = &model {
        where_clauses.push(format!("model = ?{}", bind.len() + 1));
        bind.push(m.clone());
    }
    if status != "all" {
        where_clauses.push(format!("status = ?{}", bind.len() + 1));
        bind.push(status.clone());
    }
    if let Some(a) = &agent {
        where_clauses.push(format!("agent_slug = ?{}", bind.len() + 1));
        bind.push(a.clone());
    }
    let where_sql = if where_clauses.is_empty() {
        "1=1".to_string()
    } else {
        where_clauses.join(" AND ")
    };

    let sql = format!(
        "SELECT id, prompt, model, runtime, cost_usd_estimated, tokens_in, tokens_out,
                duration_ms, status, grounding_verdict, response
         FROM execution_logs
         WHERE {}
         ORDER BY created_at ASC
         LIMIT {}",
        where_sql, limit
    );
    let mut stmt = conn.prepare(&sql)?;
    let params_iter: Vec<&dyn rusqlite::ToSql> =
        bind.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

    type AdoptRow = (
        String,
        String,
        Option<String>,
        String,
        Option<f64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<AdoptRow> = stmt
        .query_map(params_iter.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<i64>>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<String>>(9)?,
                r.get::<_, Option<String>>(10)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        anyhow::bail!(
            "no execution_logs rows matched the filter. Widen --since / --status or check `ato evaluations methodology list`."
        );
    }

    // Distinct prompts → prompt_idx, in first-seen order so the index
    // matches what a human would expect when re-reading the corpus.
    let mut prompt_index: Vec<String> = Vec::new();
    let prompt_idx_of = |p: &str, idx: &mut Vec<String>| -> usize {
        if let Some(i) = idx.iter().position(|x| x == p) {
            i
        } else {
            idx.push(p.to_string());
            idx.len() - 1
        }
    };

    let run_id = Uuid::new_v4().to_string();
    let started_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO methodology_runs
            (id, methodology_id, customer_user_id, started_at, status,
             total_dispatches_planned, total_dispatches_completed,
             customer_billing_mode)
         VALUES (?1, ?2, NULL, ?3, 'running', ?4, 0, ?5)",
        params![
            &run_id,
            &methodology_id,
            &started_at,
            rows.len() as i64,
            billing_mode.as_str(),
        ],
    )
    .context("insert methodology_runs row for adopt")?;

    let mut customer_cost_usd: f64 = 0.0;
    let mut customer_tokens_in: i64 = 0;
    let mut customer_tokens_out: i64 = 0;
    let mut response_bytes: i64 = 0;
    let mut adopted: u32 = 0;
    for row in &rows {
        let (id, prompt, model_opt, _runtime_v, cost, tok_in, tok_out, dur_ms, status_v, verdict, response) =
            (&row.0, &row.1, &row.2, &row.3, &row.4, &row.5, &row.6, &row.7, &row.8, &row.9, &row.10);
        let pidx = prompt_idx_of(prompt, &mut prompt_index);
        let cell_model = model_opt
            .clone()
            .unwrap_or_else(|| "(unknown)".to_string());
        let cell_condition = verdict
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let cell = serde_json::json!({
            "prompt_idx": pidx,
            "model": cell_model,
            "condition": cell_condition,
            "rep": 0_u32,
            "adopted_from": id,
            "status": status_v,
        });
        let _ = conn.execute(
            "INSERT OR REPLACE INTO methodology_run_dispatches
                (methodology_run_id, execution_log_id, variant_cell, score)
             VALUES (?1, ?2, ?3, NULL)",
            params![&run_id, id, &cell.to_string()],
        );
        customer_cost_usd += cost.unwrap_or(0.0);
        customer_tokens_in += tok_in.unwrap_or(0);
        customer_tokens_out += tok_out.unwrap_or(0);
        response_bytes += response.as_ref().map(|s| s.len() as i64).unwrap_or(0);
        adopted += 1;
    }

    let rates = CostRateCard::defaults_v1();
    let storage_bytes_estimate = (customer_tokens_in + customer_tokens_out) * 4;
    let retention_months = 28.0 / 30.0;
    let storage_cost = (storage_bytes_estimate as f64)
        * rates.storage_per_byte_month_usd
        * retention_months;
    let bandwidth_cost = (response_bytes as f64) * rates.bandwidth_per_byte_usd;
    let provider_llm_cost_usd = match billing_mode {
        BillingMode::Byok => 0.0,
        BillingMode::Pool => customer_cost_usd,
    };
    // Adopt has zero orchestrator compute cost (we re-read the receipts;
    // we don't re-dispatch). Storage + bandwidth still apply because
    // composition + show queries hit those bytes.
    let provider_total = provider_llm_cost_usd + storage_cost + bandwidth_cost;
    let per_run_pro_allocation = 0.29;
    let margin_usd = per_run_pro_allocation - provider_total;
    let ended_at = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "UPDATE methodology_runs SET
            ended_at = ?1,
            status = 'complete',
            total_dispatches_completed = ?2,
            customer_cost_usd = ?3,
            customer_tokens_in = ?4,
            customer_tokens_out = ?5,
            customer_dispatches = ?6,
            provider_llm_cost_usd = ?7,
            provider_storage_bytes = ?8,
            provider_bandwidth_bytes = ?9,
            provider_total_cost_usd = ?10,
            margin_usd = ?11
         WHERE id = ?12",
        params![
            &ended_at,
            adopted as i64,
            customer_cost_usd,
            customer_tokens_in,
            customer_tokens_out,
            adopted as i64,
            provider_llm_cost_usd,
            storage_bytes_estimate,
            response_bytes,
            provider_total,
            margin_usd,
            &run_id,
        ],
    )
    .context("finalize adopted methodology_runs row")?;

    if opts.human {
        emit_human(&format!(
            "Adopted {} execution_logs rows into methodology run {}.\n  methodology:     {}\n  distinct prompts: {}\n  YOUR cost:        ${:.4}\n  YOUR tokens:      {} in / {} out\n  OUR cost:         ${:.4}\n  margin (est):     ${:.4}\n\nNext: `ato evaluations methodology runs show {}` for the composition.",
            adopted, run_id, slug, prompt_index.len(),
            customer_cost_usd, customer_tokens_in, customer_tokens_out,
            provider_total, margin_usd, run_id
        ));
    } else {
        let _ = emit_json(&serde_json::json!({
            "run_id": run_id,
            "methodology_slug": slug,
            "adopted": adopted,
            "distinct_prompts": prompt_index.len(),
            "customer_cost_usd": customer_cost_usd,
            "customer_tokens_in": customer_tokens_in,
            "customer_tokens_out": customer_tokens_out,
            "provider_total_cost_usd": provider_total,
            "margin_usd": margin_usd,
        }));
    }
    Ok(())
}

fn handle_score(run_id: String, force: bool, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    let conn = db::open_readwrite(db_path)?;
    // Pull the methodology's rubric from the run's methodology row.
    let rubric_json: String = conn
        .query_row(
            "SELECT m.rubric FROM methodologies m
             JOIN methodology_runs r ON r.methodology_id = m.id
             WHERE r.id = ?1",
            params![&run_id],
            |r| r.get(0),
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "no methodology run with id '{}'. `ato evaluations methodology runs list` to see what's there.",
                run_id
            )
        })?;
    let rubric_value: serde_json::Value =
        serde_json::from_str(&rubric_json).unwrap_or(serde_json::Value::Null);
    let rubric: Rubric = Rubric::parse(&rubric_value).unwrap_or(Rubric::Pending);
    if matches!(rubric, Rubric::Pending) {
        anyhow::bail!(
            "methodology's rubric is `pending` — define a real rubric on the methodology before scoring. See `docs/methodology-runner.md` rubric section.",
        );
    }

    // Pull all (or only un-scored) dispatches + the prompt + response from execution_logs.
    let sql = if force {
        "SELECT mrd.execution_log_id, e.prompt, e.response
         FROM methodology_run_dispatches mrd
         JOIN execution_logs e ON e.id = mrd.execution_log_id
         WHERE mrd.methodology_run_id = ?1"
    } else {
        "SELECT mrd.execution_log_id, e.prompt, e.response
         FROM methodology_run_dispatches mrd
         JOIN execution_logs e ON e.id = mrd.execution_log_id
         WHERE mrd.methodology_run_id = ?1 AND mrd.score IS NULL"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<(String, String, Option<String>)> = stmt
        .query_map(params![&run_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        if opts.human {
            emit_human(&format!(
                "Nothing to score for run {}. {}",
                run_id,
                if force { "Run is empty." } else { "All dispatches already scored — pass --force to re-score." }
            ));
        } else {
            let _ = emit_json(&serde_json::json!({
                "run_id": run_id,
                "scored": 0_u32,
                "force": force,
            }));
        }
        return Ok(());
    }

    let mut scored: u32 = 0;
    let mut total_score: f64 = 0.0;
    let mut total_judge_cost: f64 = 0.0;
    let mut sum_passed: u32 = 0;
    for (eid, prompt, response_opt) in &rows {
        let response = response_opt.clone().unwrap_or_default();
        let result = rubric.score(prompt, &response, db_path);
        let s = match result {
            Ok(s) => s,
            Err(e) => crate::methodology::rubric::RubricScore::fail(format!(
                "rubric error: {}",
                e
            )),
        };
        let _ = conn.execute(
            "UPDATE methodology_run_dispatches SET score = ?1
             WHERE methodology_run_id = ?2 AND execution_log_id = ?3",
            params![s.score, &run_id, eid],
        );
        scored += 1;
        total_score += s.score;
        total_judge_cost += s.judge_cost_usd;
        if s.score >= 0.5 {
            sum_passed += 1;
        }
    }
    // Bump provider_judge_cost_usd + provider_total_cost_usd if any
    // judge calls landed here. Margin recomputes from the same per-run
    // allocation as the runner.
    let _ = conn.execute(
        "UPDATE methodology_runs SET
            provider_judge_cost_usd = provider_judge_cost_usd + ?1,
            provider_total_cost_usd = provider_total_cost_usd + ?1,
            margin_usd = margin_usd - ?1
         WHERE id = ?2",
        params![total_judge_cost, &run_id],
    );

    let mean = total_score / (scored as f64);
    if opts.human {
        emit_human(&format!(
            "Scored {} dispatches in run {}.\n  mean score:    {:.3}\n  passed (≥0.5): {}/{}\n  judge cost:    ${:.4}\n\nRun `runs show {}` for the per-cell breakdown.",
            scored, run_id, mean, sum_passed, scored, total_judge_cost, run_id
        ));
    } else {
        let _ = emit_json(&serde_json::json!({
            "run_id": run_id,
            "scored": scored,
            "mean_score": mean,
            "passed_at_threshold_0_5": sum_passed,
            "judge_cost_usd": total_judge_cost,
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parses_with_slug_in_file() {
        let cfg = parse_config_str(
            r#"{
                "slug": "ladder-test",
                "archetype": "model-ladder",
                "variant_matrix": {
                    "prompts": ["p1", "p2"],
                    "models": ["claude-sonnet-4-6", "claude-opus-4-7"],
                    "conditions": ["soft"],
                    "reps_per_cell": 30
                },
                "rubric": {"kind": "regex", "pattern": "OK"}
            }"#,
        )
        .expect("config parses");
        assert_eq!(cfg.slug.as_deref(), Some("ladder-test"));
        assert_eq!(cfg.archetype, "model-ladder");
        assert_eq!(cfg.variant_matrix.reps_per_cell, 30);
        // 2 prompts × 2 models × 1 condition × 30 reps = 120
        assert_eq!(cfg.variant_matrix.total_dispatches(), 120);
    }

    #[test]
    fn config_rejects_unknown_archetype() {
        let err = parse_config_str(
            r#"{
                "archetype": "model-glider",
                "variant_matrix": {"prompts": [], "models": [], "conditions": [], "reps_per_cell": 10},
                "rubric": {}
            }"#,
        )
        .err()
        .unwrap();
        assert!(
            err.to_string().contains("unknown archetype"),
            "error message should mention unknown archetype: {}",
            err
        );
        assert!(err.to_string().contains("archetypes"));
    }

    #[test]
    fn config_invalid_json_errors_cleanly() {
        let err = parse_config_str("{not valid json").err().unwrap();
        assert!(
            err.to_string().contains("parse methodology config"),
            "error should mention parse failure: {}",
            err
        );
    }

    #[test]
    fn config_accepts_all_known_archetypes() {
        for archetype in [
            "model-ladder",
            "tools-vs-no-tools",
            "reviewer-order-effects",
            "regression-watch",
            "custom",
        ] {
            let json = format!(
                r#"{{
                    "archetype": "{}",
                    "variant_matrix": {{"prompts": [], "models": [], "conditions": [], "reps_per_cell": 10}},
                    "rubric": {{}}
                }}"#,
                archetype
            );
            assert!(
                parse_config_str(&json).is_ok(),
                "archetype '{}' should be valid",
                archetype
            );
        }
    }

    #[test]
    fn methodology_list_row_serializes_with_dispatch_count() {
        let row = MethodologyListRow {
            id: "id-1".to_string(),
            slug: "test".to_string(),
            description: Some("test methodology".to_string()),
            archetype: "model-ladder".to_string(),
            total_dispatches_per_run: 180,
            created_at: "2026-05-25T10:00:00Z".to_string(),
        };
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(json["total_dispatches_per_run"], 180);
        assert_eq!(json["archetype"], "model-ladder");
    }

    #[test]
    fn handle_archetypes_no_db_required() {
        // The archetypes subcommand is registry-only — no DB read.
        // This test only verifies the function returns Ok; the actual
        // output shape is exercised at the CLI integration layer.
        let opts = Opts {
            human: false,
            quiet: true,
        };
        assert!(handle_archetypes(&opts).is_ok());
    }
}
