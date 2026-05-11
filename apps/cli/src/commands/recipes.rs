// `ato recipes ...` — manage ops recipes.
//
// Talks to the same `ops_recipes` SQLite table the desktop manages.
// Recipe type definitions are kept in sync between this file and
// apps/desktop/src-tauri/src/recipes.rs. (Future refactor: extract
// to a shared ato-core crate. For Phase 4 v1, duplication has a smaller
// blast radius than the workspace restructure.)

use crate::db;
use crate::output::{emit_human, emit_json, Opts};
use anyhow::{anyhow, Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ─── Types (mirror of recipes.rs in the desktop crate) ────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RecipeTrigger {
    #[serde(rename = "on_regression_detected")]
    OnRegressionDetected {
        severity: Option<String>,
        agent_slug: Option<String>,
    },
    #[serde(rename = "on_dispatch_failed")]
    OnDispatchFailed {
        runtime: Option<String>,
        agent_slug: Option<String>,
    },
    #[serde(rename = "on_replay_done")]
    OnReplayDone {
        status: Option<String>,
        target_runtime: Option<String>,
    },
    #[serde(rename = "on_cost_threshold_exceeded")]
    OnCostThresholdExceeded {
        window: Option<String>,
        agent_slug: Option<String>,
    },
    #[serde(rename = "on_schedule")]
    OnSchedule {
        cron: Option<String>,
        agent_slug: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RecipeAction {
    #[serde(rename = "draft_skill_from_replay")]
    DraftSkillFromReplay { out: Option<String> },
    #[serde(rename = "replay_on_alt")]
    ReplayOnAlt {
        target_runtime: String,
        target_model: Option<String>,
    },
    #[serde(rename = "kill_run")]
    KillRun,
    #[serde(rename = "dispatch_agent")]
    DispatchAgent {
        runtime: String,
        agent_slug: Option<String>,
        prompt_template: String,
    },
    #[serde(rename = "post_webhook")]
    PostWebhook {
        url: String,
        body_template: Option<String>,
    },
    #[serde(rename = "notify_human")]
    NotifyHuman { text_template: String },
    #[serde(rename = "run_script")]
    RunScript {
        path: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsRecipe {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger: RecipeTrigger,
    pub action: RecipeAction,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

// ─── Templates ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct RecipeTemplate {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub trigger: RecipeTrigger,
    pub action: RecipeAction,
}

// TODO(v2.3.8): extract recipe types + templates to a shared crate
// (e.g. crates/ato-recipes-core) so the CLI and desktop don't drift.
// codex-reviewer caught wording drift in this initial commit. Until
// the shared crate lands, keep these strings BYTE-IDENTICAL with
// apps/desktop/src-tauri/src/recipes.rs::builtin_templates.
fn builtin_templates() -> Vec<RecipeTemplate> {
    vec![
        // v2.3.9 — reinstated; RegressionDetected now carries
        // old_value/new_value. Kept BYTE-IDENTICAL with the desktop
        // crate's version. TODO(v2.3.8): extract to shared crate.
        RecipeTemplate {
            slug: "auto-replay-regression-failures".to_string(),
            name: "Auto-replay regression failing examples".to_string(),
            description:
                "When a regression fires, replay each failing example on the previous runtime. \
                The replay's own `replay_done` event can chain into the skillify-replays template \
                below to draft skills automatically."
                    .to_string(),
            trigger: RecipeTrigger::OnRegressionDetected {
                severity: Some("regression".to_string()),
                agent_slug: None,
            },
            action: RecipeAction::ReplayOnAlt {
                target_runtime: "{{previous_runtime}}".to_string(),
                target_model: None,
            },
        },
        RecipeTemplate {
            slug: "skillify-successful-replays".to_string(),
            name: "Skillify successful cross-runtime replays".to_string(),
            description:
                "When a replay succeeds on a different runtime than the original, draft a SKILL.md \
                routing future similar prompts to the working runtime. Reviews are still up to the \
                human — this only creates the draft."
                    .to_string(),
            trigger: RecipeTrigger::OnReplayDone {
                status: Some("done".to_string()),
                target_runtime: None,
            },
            action: RecipeAction::DraftSkillFromReplay { out: None },
        },
    ]
}

// ─── Slug validation (mirrors desktop recipes::validate_slug) ─────────
//
// Required because slug → filename in ~/.ato/recipes/<slug>.json.
// Without sanitization, `--as ../escape` would write outside the
// recipes dir. Caught by codex-reviewer in the v2.3.7 review.

fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() || slug.len() > 64 {
        return Err(anyhow!("slug must be 1-64 characters"));
    }
    let bytes = slug.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return Err(anyhow!("slug must start with a letter or digit"));
    }
    for &b in bytes {
        let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
        if !ok {
            return Err(anyhow!(
                "slug may only contain lowercase letters, digits, and hyphens; got '{}'",
                slug
            ));
        }
    }
    if slug.contains("..") || slug.contains('/') || slug.contains('\\') {
        return Err(anyhow!("slug contains illegal path characters"));
    }
    Ok(())
}

// ─── Schema check + paths ─────────────────────────────────────────────

fn has_ops_recipes_table(conn: &Connection) -> bool {
    let c: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='ops_recipes'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    c > 0
}

fn recipes_dir() -> PathBuf {
    let mut p = db::home_dir();
    p.push(".ato");
    p.push("recipes");
    p
}

fn recipe_json_path(slug: &str) -> PathBuf {
    recipes_dir().join(format!("{}.json", slug))
}

/// Best-effort snapshot. SQLite is the source of truth; the JSON
/// snapshot is for `ls ~/.ato/recipes/` discoverability only. Caller
/// should log + ignore failures, not unwind the SQLite write.
fn write_json_mirror(recipe: &OpsRecipe) -> Result<()> {
    let dir = recipes_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    let path = recipe_json_path(&recipe.slug);
    let json = serde_json::to_string_pretty(recipe)?;
    fs::write(path, json)?;
    Ok(())
}

// ─── Subcommand impls ─────────────────────────────────────────────────

pub fn list(conn: &Connection, opts: &Opts) -> Result<()> {
    if !has_ops_recipes_table(conn) {
        if opts.human {
            emit_human(
                "ops_recipes table not found. Launch the ATO desktop (v2.3.7+) once to apply the migration.",
            );
        } else {
            emit_json(&Vec::<OpsRecipe>::new())?;
        }
        return Ok(());
    }
    let recipes = list_inner(conn)?;
    if opts.human {
        if recipes.is_empty() {
            emit_human(
                "No recipes installed. See `ato recipes templates` for built-in starters.",
            );
        } else {
            emit_human(&format!("{} recipes:", recipes.len()));
            for r in &recipes {
                let badge = if r.enabled { "enabled " } else { "disabled" };
                emit_human(&format!("  [{}] {} ({})", badge, r.slug, r.name));
            }
        }
    } else {
        emit_json(&recipes)?;
    }
    Ok(())
}

fn list_inner(conn: &Connection) -> Result<Vec<OpsRecipe>> {
    let mut stmt = conn.prepare(
        "SELECT id, slug, name, description, trigger_config, action_config, enabled, created_at, updated_at
           FROM ops_recipes
          ORDER BY created_at DESC",
    )?;
    let rows: Vec<(String, String, String, Option<String>, String, String, i64, String, String)> = stmt
        .query_map([], |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?,
            r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?,
        )))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (id, slug, name, description, trigger_json, action_json, enabled_int, created_at, updated_at) in rows {
        let trigger: RecipeTrigger = serde_json::from_str(&trigger_json)
            .with_context(|| format!("Failed to parse trigger for recipe {}", slug))?;
        let action: RecipeAction = serde_json::from_str(&action_json)
            .with_context(|| format!("Failed to parse action for recipe {}", slug))?;
        out.push(OpsRecipe {
            id, slug, name, description, trigger, action,
            enabled: enabled_int != 0,
            created_at, updated_at,
        });
    }
    Ok(out)
}

pub fn get(conn: &Connection, slug: &str, opts: &Opts) -> Result<()> {
    if !has_ops_recipes_table(conn) {
        return Err(anyhow!("ops_recipes table not found. Launch the ATO desktop (v2.3.7+) once."));
    }
    let recipe = get_inner(conn, slug)?
        .ok_or_else(|| anyhow!("No recipe with slug '{}'.", slug))?;
    if opts.human {
        emit_human(&format!(
            "@{} ({}) — {}\n  {}",
            recipe.slug,
            if recipe.enabled { "enabled" } else { "disabled" },
            recipe.name,
            recipe.description.as_deref().unwrap_or("(no description)")
        ));
    } else {
        emit_json(&recipe)?;
    }
    Ok(())
}

fn get_inner(conn: &Connection, slug: &str) -> Result<Option<OpsRecipe>> {
    let row: Option<(String, String, String, Option<String>, String, String, i64, String, String)> = conn
        .query_row(
            "SELECT id, slug, name, description, trigger_config, action_config, enabled, created_at, updated_at
               FROM ops_recipes WHERE slug = ?1",
            [slug],
            |r| Ok((
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?,
                r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?,
            )),
        )
        .ok();
    let Some((id, slug, name, description, tj, aj, enabled_int, created_at, updated_at)) = row else {
        return Ok(None);
    };
    Ok(Some(OpsRecipe {
        id,
        slug: slug.clone(),
        name,
        description,
        trigger: serde_json::from_str(&tj)?,
        action: serde_json::from_str(&aj)?,
        enabled: enabled_int != 0,
        created_at,
        updated_at,
    }))
}

pub fn templates(opts: &Opts) -> Result<()> {
    let tmpls = builtin_templates();
    if opts.human {
        emit_human(&format!("{} built-in templates:", tmpls.len()));
        for t in &tmpls {
            emit_human(&format!("  {} — {}", t.slug, t.name));
            emit_human(&format!("    {}", t.description));
        }
    } else {
        emit_json(&tmpls)?;
    }
    Ok(())
}

pub fn install_template(
    conn: &Connection,
    template_slug: &str,
    rename_to: Option<String>,
    opts: &Opts,
) -> Result<()> {
    if !has_ops_recipes_table(conn) {
        return Err(anyhow!("ops_recipes table not found. Launch the ATO desktop (v2.3.7+) once."));
    }
    let t = builtin_templates()
        .into_iter()
        .find(|t| t.slug == template_slug)
        .ok_or_else(|| anyhow!("No template with slug '{}'. Try `ato recipes templates`.", template_slug))?;
    let target_slug = rename_to.unwrap_or_else(|| t.slug.clone());
    validate_slug(&target_slug)?;
    // Reject duplicate slug at the unique constraint boundary.
    if get_inner(conn, &target_slug)?.is_some() {
        return Err(anyhow!(
            "A recipe with slug '{}' already exists. Pass --as <new-slug> to rename.",
            target_slug
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let trigger_type = trigger_type_name(&t.trigger);
    let action_type = action_type_name(&t.action);
    let trigger_config = serde_json::to_string(&t.trigger)?;
    let action_config = serde_json::to_string(&t.action)?;
    conn.execute(
        "INSERT INTO ops_recipes (id, slug, name, description, trigger_type, trigger_config, action_type, action_config, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?9)",
        rusqlite::params![
            id, target_slug, t.name, Some(t.description.clone()),
            trigger_type, trigger_config, action_type, action_config, now,
        ],
    )?;
    let recipe = OpsRecipe {
        id, slug: target_slug.clone(), name: t.name, description: Some(t.description),
        trigger: t.trigger, action: t.action,
        enabled: true, created_at: now.clone(), updated_at: now,
    };
    if let Err(e) = write_json_mirror(&recipe) {
        eprintln!("warning: json mirror write failed for '{}': {}", recipe.slug, e);
    }
    if opts.human {
        emit_human(&format!(
            "Installed template '{}' as @{}",
            template_slug, target_slug
        ));
    } else {
        emit_json(&recipe)?;
    }
    Ok(())
}

pub fn set_enabled(conn: &Connection, slug: &str, enabled: bool, opts: &Opts) -> Result<()> {
    if !has_ops_recipes_table(conn) {
        return Err(anyhow!("ops_recipes table not found. Launch the ATO desktop (v2.3.7+) once."));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let n = conn.execute(
        "UPDATE ops_recipes SET enabled = ?1, updated_at = ?2 WHERE slug = ?3",
        rusqlite::params![enabled as i64, now, slug],
    )?;
    if n == 0 {
        return Err(anyhow!("No recipe with slug '{}'.", slug));
    }
    let recipe = get_inner(conn, slug)?.ok_or_else(|| anyhow!("recipe vanished after update"))?;
    if let Err(e) = write_json_mirror(&recipe) {
        eprintln!("warning: json mirror write failed for '{}': {}", recipe.slug, e);
    }
    if opts.human {
        emit_human(&format!(
            "Recipe @{} is now {}",
            slug,
            if enabled { "enabled" } else { "disabled" }
        ));
    } else {
        emit_json(&recipe)?;
    }
    Ok(())
}

/// `ato recipes runs <slug>` — tail the ops_recipe_runs audit table
/// for a recipe. Shows what the engine has done on the user's behalf:
/// when it fired, what status, what result/error.
pub fn runs(conn: &Connection, slug: &str, limit: usize, opts: &Opts) -> Result<()> {
    let exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='ops_recipe_runs'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        if opts.human {
            emit_human(
                "ops_recipe_runs table not found. Launch the ATO desktop (v2.3.8+) once.",
            );
        } else {
            emit_json(&serde_json::json!({"runs": []}))?;
        }
        return Ok(());
    }
    let safe_limit = limit.min(10_000) as i64;
    let mut stmt = conn.prepare(
        "SELECT id, event_seq, event_type, action_type, status, result, error_message, started_at, finished_at
           FROM ops_recipe_runs
          WHERE recipe_slug = ?1
          ORDER BY started_at DESC
          LIMIT ?2",
    )?;
    #[derive(serde::Serialize)]
    struct RunRow {
        id: String,
        event_seq: i64,
        event_type: String,
        action_type: String,
        status: String,
        result: Option<String>,
        error_message: Option<String>,
        started_at: String,
        finished_at: Option<String>,
    }
    let rows: Vec<RunRow> = stmt
        .query_map(rusqlite::params![slug, safe_limit], |r| {
            Ok(RunRow {
                id: r.get(0)?,
                event_seq: r.get(1)?,
                event_type: r.get(2)?,
                action_type: r.get(3)?,
                status: r.get(4)?,
                result: r.get(5)?,
                error_message: r.get(6)?,
                started_at: r.get(7)?,
                finished_at: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if opts.human {
        if rows.is_empty() {
            emit_human(&format!("No runs for recipe @{} yet.", slug));
        } else {
            emit_human(&format!("{} runs for recipe @{}:", rows.len(), slug));
            for r in &rows {
                emit_human(&format!(
                    "  [{}] {} -> {} (#{} {})",
                    r.status,
                    r.event_type,
                    r.action_type,
                    r.event_seq,
                    r.started_at
                ));
                if let Some(err) = &r.error_message {
                    emit_human(&format!("    error: {}", err));
                }
                if let Some(res) = &r.result {
                    emit_human(&format!("    result: {}", res));
                }
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

pub fn delete(conn: &Connection, slug: &str, opts: &Opts) -> Result<()> {
    if !has_ops_recipes_table(conn) {
        return Err(anyhow!("ops_recipes table not found. Launch the ATO desktop (v2.3.7+) once."));
    }
    let n = conn.execute("DELETE FROM ops_recipes WHERE slug = ?1", [slug])?;
    let _ = fs::remove_file(recipe_json_path(slug));
    let deleted = n > 0;
    if opts.human {
        if deleted {
            emit_human(&format!("Deleted recipe @{}", slug));
        } else {
            emit_human(&format!("No recipe with slug '{}' to delete.", slug));
        }
    } else {
        emit_json(&serde_json::json!({ "slug": slug, "deleted": deleted }))?;
    }
    Ok(())
}

// ─── Type-name helpers (duplicated from desktop) ──────────────────────

fn trigger_type_name(t: &RecipeTrigger) -> &'static str {
    match t {
        RecipeTrigger::OnRegressionDetected { .. } => "on_regression_detected",
        RecipeTrigger::OnDispatchFailed { .. } => "on_dispatch_failed",
        RecipeTrigger::OnReplayDone { .. } => "on_replay_done",
        RecipeTrigger::OnCostThresholdExceeded { .. } => "on_cost_threshold_exceeded",
        RecipeTrigger::OnSchedule { .. } => "on_schedule",
    }
}

fn action_type_name(a: &RecipeAction) -> &'static str {
    match a {
        RecipeAction::DraftSkillFromReplay { .. } => "draft_skill_from_replay",
        RecipeAction::ReplayOnAlt { .. } => "replay_on_alt",
        RecipeAction::KillRun => "kill_run",
        RecipeAction::DispatchAgent { .. } => "dispatch_agent",
        RecipeAction::PostWebhook { .. } => "post_webhook",
        RecipeAction::NotifyHuman { .. } => "notify_human",
        RecipeAction::RunScript { .. } => "run_script",
    }
}
