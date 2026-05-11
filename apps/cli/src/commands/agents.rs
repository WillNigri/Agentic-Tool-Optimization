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

use crate::db;
use crate::output::{emit_human, emit_json, Opts};
use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

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
    /// v2.3.0 — when set, the per-runtime config file we wrote (or
    /// updated) so the runtime's own @-mention can find this agent.
    /// None when the runtime's file format isn't yet supported by the
    /// CLI (file write is then left to the GUI).
    pub file_path: Option<PathBuf>,
    /// Honest signal for the co-piloting story: did we write the file
    /// the runtime needs? When false, the agent record exists in
    /// SQLite but the runtime won't find it via its own native APIs.
    pub runtime_visible: bool,
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

    // v2.3.0 — write the per-runtime config file too, so the runtime's
    // native @-mention / agent-discovery finds this agent. Without this,
    // the agent record is ATO-dispatchable but invisible to plain
    // `claude` / `codex` / etc. invocations — breaking the co-piloting
    // contract where the human and the agent should see the same agents.
    let mut file_path_written: Option<PathBuf> = None;
    let runtime_visible = match write_runtime_file(runtime, slug, &display, description.as_deref(), model.as_deref(), system_prompt.as_deref()) {
        Ok(Some(path)) => {
            file_path_written = Some(path);
            true
        }
        Ok(None) => false, // unsupported runtime file format yet
        Err(e) => {
            // Don't fail the whole create just because the file write
            // didn't go through. The SQLite row + ledger entry are
            // useful on their own; the human can rerun later or write
            // the file themselves.
            eprintln!("Warning: agent record created in SQLite but the per-runtime config file could not be written: {}", e);
            false
        }
    };

    conn.execute(
        "INSERT INTO agents (id, slug, display_name, description, runtime, model, project_id, system_prompt, permissions, skills, mcps, goal, file_path, created_at, last_used_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, NULL, ?9, ?10, NULL)",
        rusqlite::params![
            id,
            slug,
            display,
            description,
            runtime,
            model,
            project_id,
            system_prompt,
            file_path_written.as_ref().map(|p| p.display().to_string()),
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
        file_path: file_path_written,
        runtime_visible,
    };

    if opts.human {
        emit_human(&format!(
            "Created agent @{} ({}) on runtime {} — runtime_visible={}",
            result.slug, result.id, result.runtime, result.runtime_visible
        ));
        if let Some(p) = &result.file_path {
            emit_human(&format!("  wrote {}", p.display()));
        }
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

/// v2.3.0 — write the per-runtime agent config file so the runtime's
/// own discovery (`claude /agents`, codex @-mention, etc.) can see the
/// agent. Mirrors the format the desktop's `render_<runtime>_agent`
/// helpers produce. Returns Some(path) when written, None when the
/// runtime's file format isn't yet wired here.
fn write_runtime_file(
    runtime: &str,
    slug: &str,
    display_name: &str,
    description: Option<&str>,
    model: Option<&str>,
    system_prompt: Option<&str>,
) -> Result<Option<PathBuf>> {
    let path = match agent_file_path(runtime, slug) {
        Some(p) => p,
        None => return Ok(None),
    };
    let content = render_runtime_file(runtime, slug, display_name, description, model, system_prompt);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Could not create agents directory")?;
    }
    // Backup if the file already exists, then write.
    if path.exists() {
        let backup = path.with_extension("md.bak");
        let _ = fs::copy(&path, &backup);
    }
    fs::write(&path, content).context("Failed to write agent file")?;
    Ok(Some(path))
}

/// Per-runtime path resolution. None for runtimes the CLI doesn't yet
/// emit a config file for (gemini, openclaw, hermes are GUI-only as of
/// v2.3.0; their formats are richer / require project context).
fn agent_file_path(runtime: &str, slug: &str) -> Option<PathBuf> {
    let mut home = db::home_dir();
    match runtime {
        "claude" => {
            home.push(".claude");
            home.push("agents");
            home.push(format!("{}.md", slug));
            Some(home)
        }
        "codex" => {
            home.push(".codex");
            home.push("agents");
            home.push(slug);
            home.push("AGENTS.md");
            Some(home)
        }
        _ => None,
    }
}

/// Per-runtime file rendering. Mirrors the formats in
/// apps/desktop/src-tauri/src/commands.rs (render_claude_agent +
/// render_codex_agent). Keep in sync when the desktop's format evolves.
fn render_runtime_file(
    runtime: &str,
    slug: &str,
    display_name: &str,
    description: Option<&str>,
    model: Option<&str>,
    system_prompt: Option<&str>,
) -> String {
    match runtime {
        "claude" => render_claude(slug, display_name, description, model, system_prompt),
        "codex" => render_codex(display_name, description, model, system_prompt),
        _ => String::new(),
    }
}

fn render_claude(
    slug: &str,
    display_name: &str,
    description: Option<&str>,
    model: Option<&str>,
    system_prompt: Option<&str>,
) -> String {
    // Claude Code agent format: YAML frontmatter + body. Docs:
    // https://docs.claude.com/en/docs/claude-code/sub-agents
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", slug));
    if let Some(d) = description {
        out.push_str(&format!("description: {}\n", d));
    }
    if let Some(m) = model {
        out.push_str(&format!("model: {}\n", m));
    }
    out.push_str("---\n\n");
    out.push_str(&format!("# {}\n\n", display_name));
    if let Some(p) = system_prompt {
        if !p.trim().is_empty() {
            out.push_str(p);
            out.push('\n');
        }
    }
    out
}

fn render_codex(
    display_name: &str,
    description: Option<&str>,
    model: Option<&str>,
    system_prompt: Option<&str>,
) -> String {
    // Codex / OpenAI Agents SDK uses AGENTS.md.
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", display_name));
    if let Some(d) = description {
        out.push_str(&format!("> {}\n\n", d));
    }
    if let Some(m) = model {
        out.push_str(&format!("**Model:** `{}`\n\n", m));
    }
    if let Some(p) = system_prompt {
        out.push_str("## Instructions\n\n");
        out.push_str(p);
        out.push('\n');
    }
    out
}

/// Skill-list mutation modes for `ato agents update --skills`.
pub enum SkillsMutation {
    /// `--skills "a,b,c"` — replace the whole list.
    Replace(Vec<String>),
    /// `--add-skill X` — append X if not already present.
    Add(String),
    /// `--remove-skill X` — remove X if present.
    Remove(String),
}

pub fn update(
    conn: &Connection,
    slug: &str,
    runtime: Option<String>,
    model: Option<String>,
    system_prompt: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    skills_mutation: Option<SkillsMutation>,
    opts: &Opts,
) -> Result<()> {
    // If --runtime is given, scope by both (runtime, slug); otherwise
    // require slug to be unique across runtimes (rare but possible
    // because the agents table's UNIQUE constraint is (runtime, slug)).
    let candidates: Vec<(String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>)> = {
        let sql = if runtime.is_some() {
            "SELECT id, slug, runtime, model, system_prompt, display_name, description, skills FROM agents WHERE slug = ?1 AND runtime = ?2"
        } else {
            "SELECT id, slug, runtime, model, system_prompt, display_name, description, skills FROM agents WHERE slug = ?1"
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
                r.get::<_, Option<String>>(7)?,
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
                r.get::<_, Option<String>>(7)?,
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
    let (id, _slug, current_runtime, current_model, current_prompt, current_name, current_desc, current_skills_json) =
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

    // v2.3.0 — skills mutation. agents.skills is a JSON TEXT column.
    // Parse the existing list, apply Replace / Add / Remove, serialize,
    // and write back. The regression detector treats "skills" as a
    // first-class field so it'll see CLI-driven mutations the same way
    // it sees GUI-driven ones.
    if let Some(mutation) = &skills_mutation {
        let current_list: Vec<String> = current_skills_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let new_list = apply_skills_mutation(&current_list, mutation);
        if new_list != current_list {
            let new_json = serde_json::to_string(&new_list)?;
            let old_json = current_skills_json.clone();
            sets.push("skills = ?".to_string());
            params.push(Box::new(new_json.clone()));
            changes_to_log.push(("skills", old_json, Some(new_json)));
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
        file_path: None,
        runtime_visible: false,
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

/// Apply a skill-list mutation to the current list. Pure function — no
/// I/O — so unit tests can cover the edge cases (dedup on add, no-op on
/// remove-missing, full replace).
fn apply_skills_mutation(current: &[String], mutation: &SkillsMutation) -> Vec<String> {
    match mutation {
        SkillsMutation::Replace(new) => {
            // Dedup while preserving order.
            let mut seen = std::collections::HashSet::new();
            new.iter()
                .filter(|s| !s.trim().is_empty())
                .filter(|s| seen.insert(s.as_str().to_string()))
                .cloned()
                .collect()
        }
        SkillsMutation::Add(s) => {
            if current.contains(s) {
                current.to_vec()
            } else {
                let mut next = current.to_vec();
                next.push(s.clone());
                next
            }
        }
        SkillsMutation::Remove(s) => current
            .iter()
            .filter(|x| x.as_str() != s.as_str())
            .cloned()
            .collect(),
    }
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
