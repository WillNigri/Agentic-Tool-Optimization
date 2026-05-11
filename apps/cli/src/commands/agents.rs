// `ato agents create | update <slug>`
//
// Writes to the agents table that the desktop already maintains.
// Phase 1.x scope:
//   - create: INSERT a minimal agent row, log to agent_config_changes
//   - update: UPDATE a field (model, system_prompt, description), log
//     the change to agent_config_changes so the regression detector sees it
//
// What we DON'T do here (deferred):
//   - Writing the per-runtime config file (~/.claude/agents/<slug>.md, etc.).
//     The GUI's createAgent flow does that — it's a wizard-driven multi-step
//     process. The CLI's minimal create is for "I want an agent record I
//     can dispatch against"; the human can edit the corresponding runtime
//     config file separately if they need the file-on-disk surface too.
//   - Variables, hooks, summarizers, role models, evaluators. Those are
//     v1.4.0 production-grade authoring features and remain GUI-driven.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AgentResult {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub runtime: String,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub description: Option<String>,
    pub project_id: Option<String>,
    pub created_at: String,
    pub action: String, // "created" or "updated"
}

pub fn create(
    conn: &Connection,
    slug: &str,
    runtime: &str,
    display_name: Option<String>,
    description: Option<String>,
    model: Option<String>,
    system_prompt: Option<String>,
    project_id: Option<String>,
    opts: &Opts,
) -> Result<()> {
    // Reject duplicates at the (runtime, slug) UNIQUE boundary the GUI
    // enforces. Better to fail clean here than let SQLite return a
    // cryptic constraint violation.
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM agents WHERE runtime = ?1 AND slug = ?2 LIMIT 1",
            [runtime, slug],
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_some() {
        return Err(anyhow!(
            "An agent with slug '{}' on runtime '{}' already exists. Use `ato agents update {}` instead.",
            slug,
            runtime,
            slug
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let display = display_name.unwrap_or_else(|| slug.to_string());
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO agents (id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, NULL, NULL, ?9, NULL)",
        rusqlite::params![
            id,
            slug,
            display,
            description,
            runtime,
            model,
            project_id,
            system_prompt,
            now,
        ],
    )
    .context("Failed to insert agent row")?;

    // Log the create as a config-change so the regression detector and
    // the GUI's History tab both see it.
    log_config_change(conn, slug, "create", None, Some(&runtime.to_string()), "ato-cli")?;

    let result = AgentResult {
        id,
        slug: slug.to_string(),
        display_name: display,
        runtime: runtime.to_string(),
        model,
        system_prompt,
        description,
        project_id,
        created_at: now,
        action: "created".to_string(),
    };

    if opts.human {
        emit_human(&format!(
            "Created agent @{} ({}) on runtime {}",
            result.slug, result.id, result.runtime,
        ));
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

pub fn update(
    conn: &Connection,
    slug: &str,
    runtime: Option<String>,
    model: Option<String>,
    system_prompt: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    opts: &Opts,
) -> Result<()> {
    // If --runtime is given, scope by both (runtime, slug); otherwise
    // require slug to be unique across runtimes (rare but possible
    // because the agents table's UNIQUE constraint is (runtime, slug)).
    let candidates: Vec<(String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>)> = {
        let sql = if runtime.is_some() {
            "SELECT id, slug, runtime, model, system_prompt, display_name, description FROM agents WHERE slug = ?1 AND runtime = ?2"
        } else {
            "SELECT id, slug, runtime, model, system_prompt, display_name, description FROM agents WHERE slug = ?1"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows: Vec<_> = if let Some(rt) = &runtime {
            stmt.query_map([slug, rt.as_str()], |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            )))?
            .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([slug], |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            )))?
            .collect::<Result<Vec<_>, _>>()?
        };
        rows
    };

    if candidates.is_empty() {
        return Err(anyhow!("No agent found with slug '{}'.", slug));
    }
    if candidates.len() > 1 {
        return Err(anyhow!(
            "Multiple agents share slug '{}'. Disambiguate with --runtime <name>.",
            slug
        ));
    }
    let (id, _slug, current_runtime, current_model, current_prompt, current_name, current_desc) =
        candidates.into_iter().next().unwrap();

    // Build the UPDATE incrementally. Each Some() override generates a
    // SET fragment + logs a config-change row so the regression detector
    // sees the edit.
    let mut sets: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let mut changes_to_log: Vec<(&str, Option<String>, Option<String>)> = Vec::new();

    if let Some(m) = &model {
        if current_model.as_deref() != Some(m.as_str()) {
            sets.push("model = ?".to_string());
            params.push(Box::new(m.clone()));
            changes_to_log.push(("model", current_model.clone(), Some(m.clone())));
        }
    }
    if let Some(p) = &system_prompt {
        if current_prompt.as_deref() != Some(p.as_str()) {
            sets.push("system_prompt = ?".to_string());
            params.push(Box::new(p.clone()));
            changes_to_log.push((
                "system_prompt",
                current_prompt.clone(),
                Some(p.clone()),
            ));
        }
    }
    if let Some(n) = &display_name {
        if current_name.as_deref() != Some(n.as_str()) {
            sets.push("display_name = ?".to_string());
            params.push(Box::new(n.clone()));
            changes_to_log.push(("display_name", current_name.clone(), Some(n.clone())));
        }
    }
    if let Some(d) = &description {
        if current_desc.as_deref() != Some(d.as_str()) {
            sets.push("description = ?".to_string());
            params.push(Box::new(d.clone()));
            changes_to_log.push(("description", current_desc.clone(), Some(d.clone())));
        }
    }

    if sets.is_empty() {
        if opts.human {
            emit_human(&format!("Nothing to update for @{} — all fields match.", slug));
        } else {
            emit_json(&serde_json::json!({
                "slug": slug,
                "action": "noop",
                "note": "No fields differed from current values."
            }))?;
        }
        return Ok(());
    }

    let sql = format!("UPDATE agents SET {} WHERE id = ?", sets.join(", "));
    params.push(Box::new(id.clone()));
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    conn.execute(&sql, rusqlite::params_from_iter(refs.iter()))
        .context("Failed to UPDATE agent")?;

    for (field, old, new) in &changes_to_log {
        log_config_change(conn, slug, field, old.as_deref(), new.as_deref(), "ato-cli")?;
    }

    let now = chrono::Utc::now().to_rfc3339();
    let result = AgentResult {
        id,
        slug: slug.to_string(),
        display_name: display_name.or(current_name).unwrap_or_else(|| slug.to_string()),
        runtime: current_runtime.unwrap_or_default(),
        model: model.or(current_model),
        system_prompt: system_prompt.or(current_prompt),
        description: description.or(current_desc),
        project_id: None,
        created_at: now,
        action: "updated".to_string(),
    };

    if opts.human {
        emit_human(&format!(
            "Updated agent @{} — {} field(s) changed",
            slug,
            changes_to_log.len()
        ));
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

/// Append a row to agent_config_changes so the regression detector picks
/// up CLI-driven edits the same way it does GUI-driven ones. Best-effort
/// — if the table doesn't exist (old DB), skip silently rather than
/// blocking the actual write.
fn log_config_change(
    conn: &Connection,
    agent_slug: &str,
    field: &str,
    old_value: Option<&str>,
    new_value: Option<&str>,
    actor: &str,
) -> Result<()> {
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='agent_config_changes'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "INSERT INTO agent_config_changes (id, agent_slug, field, old_value, new_value, actor, changed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, agent_slug, field, old_value, new_value, actor, now],
    );
    Ok(())
}
