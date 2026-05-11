// v2.3.7 Phase 4 — Ops recipes storage + types.
//
// Recipes are user-authored trigger→action workflows.
//
// SOURCE OF TRUTH: the `ops_recipes` SQLite table. Always.
// JSON SNAPSHOT (best-effort): `~/.ato/recipes/<slug>.json` is written
//   after each successful DB write so the user can `ls` to see what
//   exists. It is NOT a hand-editable surface in this phase — edits
//   to the JSON files are not reconciled back to SQLite. If the JSON
//   write fails, the DB write is NOT rolled back; the recipe still
//   works, the JSON is just stale. (Reconciliation + hand-editable
//   import lands with the execution engine.)
//
// Phase 4.1 (this commit) ships storage + CRUD. The execution engine
// (subscribes to events::bus, runs actions when triggers match) is a
// separate follow-up — keeping storage decoupled from execution means
// we can sanity-check the data model without entangling tokio tasks.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

type Result<T> = std::result::Result<T, String>;

fn to_string_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Validate a recipe slug. Required because the slug is used as a
/// filename in ~/.ato/recipes/<slug>.json — without sanitization
/// values like "../escape" or "/etc/passwd" would write outside the
/// recipes directory. Caught by codex-reviewer in v2.3.7 review.
///
/// Shape: lowercase alphanumerics + hyphens, 1-64 chars, must start
/// with alphanumeric. Mirrors how skills, agents, and runtimes name
/// their disk-backed records.
pub fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() || slug.len() > 64 {
        return Err("slug must be 1-64 characters".to_string());
    }
    let bytes = slug.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return Err("slug must start with a letter or digit".to_string());
    }
    for &b in bytes {
        let ok = b.is_ascii_lowercase()
            || b.is_ascii_digit()
            || b == b'-';
        if !ok {
            return Err(format!(
                "slug may only contain lowercase letters, digits, and hyphens; got '{}'",
                slug
            ));
        }
    }
    // Defensive: re-reject any path component shape, even though the
    // character class already rules them out. Belt + suspenders.
    if slug.contains("..") || slug.contains('/') || slug.contains('\\') {
        return Err("slug contains illegal path characters".to_string());
    }
    Ok(())
}

// ─── Types ────────────────────────────────────────────────────────────

/// Trigger types. Each variant matches one AtoEvent variant from
/// events.rs, plus optional filter config so a single trigger can be
/// scoped (e.g. "only severity=regression", "only target_runtime=codex").
///
/// The filter shape is intentionally loose JSON — recipes grow new
/// filters over time without schema churn. Unknown filter keys are
/// ignored at evaluation time (forward-compat).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RecipeTrigger {
    #[serde(rename = "on_regression_detected")]
    OnRegressionDetected {
        /// Optional: "regression" | "improvement". None = either.
        severity: Option<String>,
        /// Optional: only fire when this agent slug regressed.
        agent_slug: Option<String>,
    },
    #[serde(rename = "on_dispatch_failed")]
    OnDispatchFailed {
        runtime: Option<String>,
        agent_slug: Option<String>,
    },
    #[serde(rename = "on_replay_done")]
    OnReplayDone {
        /// "done" | "failed". None = either.
        status: Option<String>,
        /// Only fire when target_runtime matches.
        target_runtime: Option<String>,
    },
    #[serde(rename = "on_cost_threshold_exceeded")]
    OnCostThresholdExceeded {
        /// "1d" | "7d" | "30d". None = any window.
        window: Option<String>,
        agent_slug: Option<String>,
    },
    #[serde(rename = "on_schedule")]
    OnSchedule {
        /// Cron expression. None = matches any scheduled tick.
        cron: Option<String>,
        agent_slug: Option<String>,
    },
}

/// Action types — what to do when a trigger fires. Like triggers, each
/// variant carries the minimum config it needs to execute.
///
/// Destructive actions (run_script, kill_run) get a runtime guard in
/// the execution engine: a recipe can spend at most N tokens / kill at
/// most N runs per day per recipe. Per-recipe caps land with the
/// execution engine in a follow-up commit; the storage layer carries
/// the action shape only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RecipeAction {
    /// Draft a SKILL.md from a successful replay. Equivalent to
    /// `ato skills draft --from-replay <job-id>`. The replay job id
    /// comes from the trigger payload at execution time.
    #[serde(rename = "draft_skill_from_replay")]
    DraftSkillFromReplay {
        /// Optional output path template. Defaults to
        /// `~/.<runtime>/skills/<slug>/SKILL.md`.
        out: Option<String>,
    },

    /// Replay the trigger's source trace against an alternative runtime.
    /// Equivalent to `ato replay start <trace-id> --runtime X`.
    #[serde(rename = "replay_on_alt")]
    ReplayOnAlt {
        target_runtime: String,
        target_model: Option<String>,
    },

    /// Kill the run referenced by the trigger payload. Used by stuck-
    /// dispatch recovery recipes (e.g. "if a run hasn't finished in
    /// 5 minutes, kill it").
    #[serde(rename = "kill_run")]
    KillRun,

    /// Dispatch a new prompt to an agent. The prompt is a template
    /// string with `{{trigger_field}}` placeholders that get filled
    /// from the event payload at execution time.
    #[serde(rename = "dispatch_agent")]
    DispatchAgent {
        runtime: String,
        agent_slug: Option<String>,
        prompt_template: String,
    },

    /// POST the event payload (plus optional template-derived body) to
    /// a webhook URL. Useful for piping events out to Slack / Discord /
    /// custom dashboards.
    #[serde(rename = "post_webhook")]
    PostWebhook {
        url: String,
        /// Optional JSON template; placeholders are filled from the
        /// event payload. Defaults to the raw event as JSON.
        body_template: Option<String>,
    },

    /// Post a message to the activity feed (Phase 5). Until the feed
    /// lands, this action no-ops with a warning — so recipes that use
    /// it can be authored today without breaking.
    #[serde(rename = "notify_human")]
    NotifyHuman { text_template: String },

    /// Run a local shell script with the event payload as JSON on stdin.
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

// ─── Storage paths ────────────────────────────────────────────────────

/// Directory where recipe JSON files mirror the SQLite rows. Created
/// on first write. Files: ~/.ato/recipes/<slug>.json.
pub fn recipes_dir() -> PathBuf {
    let mut p = crate::home_dir();
    p.push(".ato");
    p.push("recipes");
    p
}

fn recipe_json_path(slug: &str) -> PathBuf {
    let mut p = recipes_dir();
    p.push(format!("{}.json", slug));
    p
}

// ─── CRUD ─────────────────────────────────────────────────────────────

pub fn create(conn: &Connection, input: CreateRecipeInput) -> Result<OpsRecipe> {
    validate_slug(&input.slug)?;
    // Reject duplicates at the slug boundary.
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM ops_recipes WHERE slug = ?1 LIMIT 1",
            [&input.slug],
            |r| r.get(0),
        )
        .ok();
    if existing.is_some() {
        return Err(format!("Recipe with slug '{}' already exists.", input.slug));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let trigger_type = trigger_type_name(&input.trigger);
    let action_type = action_type_name(&input.action);
    let trigger_config = serde_json::to_string(&input.trigger).map_err(to_string_err)?;
    let action_config = serde_json::to_string(&input.action).map_err(to_string_err)?;

    conn.execute(
        "INSERT INTO ops_recipes (id, slug, name, description, trigger_type, trigger_config, action_type, action_config, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            id,
            input.slug,
            input.name,
            input.description,
            trigger_type,
            trigger_config,
            action_type,
            action_config,
            input.enabled as i64,
            now,
            now,
        ],
    )
    .map_err(to_string_err)?;

    let recipe = OpsRecipe {
        id,
        slug: input.slug.clone(),
        name: input.name,
        description: input.description,
        trigger: input.trigger,
        action: input.action,
        enabled: input.enabled,
        created_at: now.clone(),
        updated_at: now,
    };
    // JSON mirror is best-effort. A failed mirror write does NOT
    // unwind the SQLite create — the recipe is real either way,
    // the snapshot will refresh on next mutation.
    if let Err(e) = write_json_mirror(&recipe) {
        eprintln!("warning: ops_recipes json mirror write failed for '{}': {}", recipe.slug, e);
    }
    Ok(recipe)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRecipeInput {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger: RecipeTrigger,
    pub action: RecipeAction,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

pub fn list(conn: &Connection) -> Result<Vec<OpsRecipe>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, slug, name, description, trigger_config, action_config, enabled, created_at, updated_at
               FROM ops_recipes
              ORDER BY created_at DESC",
        )
        .map_err(to_string_err)?;
    let iter = stmt
        .query_map([], |r| {
            let trigger_json: String = r.get(4)?;
            let action_json: String = r.get(5)?;
            let enabled_int: i64 = r.get(6)?;
            // Deserialization happens outside the rusqlite Result; we
            // can't fail-with-string-error here, so we propagate via
            // a synthetic error type.
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                trigger_json,
                action_json,
                enabled_int,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
            ))
        })
        .map_err(to_string_err)?;

    let mut out: Vec<OpsRecipe> = Vec::new();
    for row in iter {
        let (id, slug, name, description, trigger_json, action_json, enabled_int, created_at, updated_at) =
            row.map_err(to_string_err)?;
        let trigger: RecipeTrigger = serde_json::from_str(&trigger_json).map_err(to_string_err)?;
        let action: RecipeAction = serde_json::from_str(&action_json).map_err(to_string_err)?;
        out.push(OpsRecipe {
            id,
            slug,
            name,
            description,
            trigger,
            action,
            enabled: enabled_int != 0,
            created_at,
            updated_at,
        });
    }
    Ok(out)
}

pub fn get(conn: &Connection, slug: &str) -> Result<Option<OpsRecipe>> {
    let row: Option<(String, String, String, Option<String>, String, String, i64, String, String)> = conn
        .query_row(
            "SELECT id, slug, name, description, trigger_config, action_config, enabled, created_at, updated_at
               FROM ops_recipes WHERE slug = ?1",
            [slug],
            |r| Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            )),
        )
        .ok();
    let Some((id, slug, name, description, trigger_json, action_json, enabled_int, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let trigger: RecipeTrigger = serde_json::from_str(&trigger_json).map_err(to_string_err)?;
    let action: RecipeAction = serde_json::from_str(&action_json).map_err(to_string_err)?;
    Ok(Some(OpsRecipe {
        id,
        slug,
        name,
        description,
        trigger,
        action,
        enabled: enabled_int != 0,
        created_at,
        updated_at,
    }))
}

pub fn set_enabled(conn: &Connection, slug: &str, enabled: bool) -> Result<OpsRecipe> {
    let now = chrono::Utc::now().to_rfc3339();
    let updated = conn
        .execute(
            "UPDATE ops_recipes SET enabled = ?1, updated_at = ?2 WHERE slug = ?3",
            rusqlite::params![enabled as i64, now, slug],
        )
        .map_err(to_string_err)?;
    if updated == 0 {
        return Err(format!("No recipe with slug '{}'.", slug));
    }
    let recipe = get(conn, slug)?.ok_or_else(|| "recipe vanished after update".to_string())?;
    if let Err(e) = write_json_mirror(&recipe) {
        eprintln!("warning: ops_recipes json mirror write failed for '{}': {}", recipe.slug, e);
    }
    Ok(recipe)
}

pub fn delete(conn: &Connection, slug: &str) -> Result<bool> {
    let n = conn
        .execute("DELETE FROM ops_recipes WHERE slug = ?1", [slug])
        .map_err(to_string_err)?;
    if n > 0 {
        let _ = fs::remove_file(recipe_json_path(slug));
        Ok(true)
    } else {
        Ok(false)
    }
}

// ─── JSON mirror ──────────────────────────────────────────────────────

fn write_json_mirror(recipe: &OpsRecipe) -> Result<()> {
    let dir = recipes_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(to_string_err)?;
    }
    let path = recipe_json_path(&recipe.slug);
    let json = serde_json::to_string_pretty(recipe).map_err(to_string_err)?;
    fs::write(path, json).map_err(to_string_err)?;
    Ok(())
}

// ─── Type-name helpers ────────────────────────────────────────────────

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

// ─── Templates ────────────────────────────────────────────────────────

/// Pre-built recipe templates. Users `ato recipes install <slug>` to
/// drop one into their workspace. Templates are the canonical example
/// for each common workflow pattern — Skillify gets two so users see
/// how a chain of recipes composes.
#[derive(Debug, Clone, Serialize)]
pub struct RecipeTemplate {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub trigger: RecipeTrigger,
    pub action: RecipeAction,
}

pub fn builtin_templates() -> Vec<RecipeTemplate> {
    vec![
        // v2.3.8 — `auto-replay-regression-failures` was a v1 template
        // but is held back because RegressionDetected doesn't yet
        // carry the previous_runtime in its payload, so
        // {{previous_runtime}} resolves to empty and the action always
        // fails. Add it back after the RegressionDetected schema
        // gains old_value/new_value (Phase 4.3).
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

pub fn template_by_slug(slug: &str) -> Option<RecipeTemplate> {
    builtin_templates().into_iter().find(|t| t.slug == slug)
}
