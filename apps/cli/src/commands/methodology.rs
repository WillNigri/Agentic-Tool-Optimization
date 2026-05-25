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
