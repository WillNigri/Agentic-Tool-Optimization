// ato-recipes — shared ops-recipe types for the ATO desktop + CLI.
//
// Why this crate exists: the desktop's recipes.rs and the CLI's
// commands/recipes.rs were maintaining byte-identical type defs and
// drifting at every new trigger/action variant (caught in v2.3.13 by
// having to mirror OnDispatchLongRunning into both). Centralizing the
// types here eliminates the drift risk and the "TODO: extract to
// shared crate" notes from v2.3.7.
//
// Scope: pure type definitions, slug validation, built-in template
// list. No I/O, no DB, no tokio. The crate is intentionally tiny so
// both downstream Rust crates can depend on it without inheriting
// transitive deps they don't want.

use serde::{Deserialize, Serialize};

// ─── Trigger ──────────────────────────────────────────────────────────

/// Trigger types. Each variant matches one AtoEvent variant from the
/// desktop's events module, plus optional filter config so a single
/// trigger can be scoped (e.g. "only severity=regression", "only
/// target_runtime=codex").
///
/// The filter shape is intentionally loose — recipes grow new filters
/// over time without schema churn. Unknown filter keys are ignored at
/// evaluation time (forward-compat).
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
    /// v2.3.13 Phase 4.7 — fires when an active dispatch has been
    /// running for at least `threshold_secs`. The engine's watcher
    /// task scans active_runs every 30s and emits one event per
    /// (run_id, threshold) crossing.
    #[serde(rename = "on_dispatch_long_running")]
    OnDispatchLongRunning {
        runtime: Option<String>,
        agent_slug: Option<String>,
        threshold_secs: u32,
    },
}

// ─── Action ───────────────────────────────────────────────────────────

/// Action types — what to do when a trigger fires. Like triggers, each
/// variant carries the minimum config it needs to execute.
///
/// Destructive actions (run_script, kill_run) get a runtime guard in
/// the execution engine: a recipe can spend at most N runs per minute
/// per recipe. That's enforced by the desktop's rate_limit_locks, not
/// here in the schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RecipeAction {
    /// Draft a SKILL.md from a successful replay. Equivalent to
    /// `ato skills draft --from-replay <job-id>`.
    #[serde(rename = "draft_skill_from_replay")]
    DraftSkillFromReplay {
        /// Optional output path template. Defaults to
        /// `~/.<runtime>/skills/<slug>/SKILL.md`.
        out: Option<String>,
    },

    /// Replay the trigger's source trace against an alternative runtime.
    #[serde(rename = "replay_on_alt")]
    ReplayOnAlt {
        target_runtime: String,
        target_model: Option<String>,
    },

    /// Kill the run referenced by the trigger payload. v2.3.13: only
    /// acts on DispatchLongRunning events whose run_id is the live
    /// active_runs registry key.
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
    /// a webhook URL.
    #[serde(rename = "post_webhook")]
    PostWebhook {
        url: String,
        body_template: Option<String>,
    },

    /// Post a message to the activity feed. text_template runs
    /// through the engine's standard placeholder substitution
    /// ({{source_runtime}}, {{target_runtime}}, {{agent_slug}},
    /// {{previous_runtime}}). The post is attributed as
    /// `system` author with the recipe's slug.
    #[serde(rename = "notify_human")]
    NotifyHuman { text_template: String },

    /// v2.3.19 Phase 5.4 — request human approval before continuing.
    /// Writes an ApprovalRequest post and marks the recipe_run as
    /// `awaiting_approval`. A separate watcher task in the engine
    /// scans awaiting_approval runs every few seconds and resumes
    /// them when an ApprovalDecision post lands. Same placeholder
    /// substitution as NotifyHuman.
    #[serde(rename = "request_approval")]
    RequestApproval { text_template: String },

    /// Run a local shell script with the event payload as JSON on stdin.
    #[serde(rename = "run_script")]
    RunScript {
        path: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

// ─── Records ──────────────────────────────────────────────────────────

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

// ─── Validation ───────────────────────────────────────────────────────

/// Validate a recipe slug. Caught by codex-reviewer in v2.3.7: the
/// slug is used as a filename in `~/.ato/recipes/<slug>.json`, so
/// values like "../escape" or "/etc/passwd" would write outside the
/// recipes directory. Mirror lives in the CLI for early-fail before
/// any IPC, and the desktop re-validates before any DB write.
///
/// Shape: lowercase alphanumerics + hyphens, 1-64 chars, must start
/// with alphanumeric.
pub fn validate_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() || slug.len() > 64 {
        return Err("slug must be 1-64 characters".to_string());
    }
    let bytes = slug.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return Err("slug must start with a letter or digit".to_string());
    }
    for &b in bytes {
        let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
        if !ok {
            return Err(format!(
                "slug may only contain lowercase letters, digits, and hyphens; got '{}'",
                slug
            ));
        }
    }
    if slug.contains("..") || slug.contains('/') || slug.contains('\\') {
        return Err("slug contains illegal path characters".to_string());
    }
    Ok(())
}

// ─── Type-name helpers ────────────────────────────────────────────────

pub fn trigger_type_name(t: &RecipeTrigger) -> &'static str {
    match t {
        RecipeTrigger::OnRegressionDetected { .. } => "on_regression_detected",
        RecipeTrigger::OnDispatchFailed { .. } => "on_dispatch_failed",
        RecipeTrigger::OnReplayDone { .. } => "on_replay_done",
        RecipeTrigger::OnCostThresholdExceeded { .. } => "on_cost_threshold_exceeded",
        RecipeTrigger::OnSchedule { .. } => "on_schedule",
        RecipeTrigger::OnDispatchLongRunning { .. } => "on_dispatch_long_running",
    }
}

pub fn action_type_name(a: &RecipeAction) -> &'static str {
    match a {
        RecipeAction::DraftSkillFromReplay { .. } => "draft_skill_from_replay",
        RecipeAction::ReplayOnAlt { .. } => "replay_on_alt",
        RecipeAction::KillRun => "kill_run",
        RecipeAction::DispatchAgent { .. } => "dispatch_agent",
        RecipeAction::PostWebhook { .. } => "post_webhook",
        RecipeAction::NotifyHuman { .. } => "notify_human",
        RecipeAction::RunScript { .. } => "run_script",
        RecipeAction::RequestApproval { .. } => "request_approval",
    }
}

// ─── Built-in templates ───────────────────────────────────────────────

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

pub fn template_by_slug(slug: &str) -> Option<RecipeTemplate> {
    builtin_templates().into_iter().find(|t| t.slug == slug)
}
