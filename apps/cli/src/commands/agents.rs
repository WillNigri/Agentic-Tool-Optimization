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
use std::path::{Path, PathBuf};

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
    /// v2.3.0 — when set, the per-runtime config file we wrote OR
    /// recognized so the runtime's own @-mention can find this agent.
    /// None when the runtime's file format isn't yet supported by the
    /// CLI (file write is then left to the GUI).
    pub file_path: Option<PathBuf>,
    /// Honest signal for the co-piloting story: did we write the file
    /// the runtime needs? When false, the agent record exists in
    /// SQLite but the runtime won't find it via its own native APIs.
    pub runtime_visible: bool,
    /// "wrote" when we authored the runtime file; "registered" when the
    /// user provided the file (--from-file) and we left it untouched;
    /// "none" when no file was involved.
    pub file_action: String,
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
    create_inner(
        conn,
        slug,
        runtime,
        display_name,
        description,
        model,
        system_prompt,
        project_id,
        /* write_runtime_file_too = */ true,
        opts,
    )
}

/// Inner helper shared between `create()` (writes the runtime file as
/// part of registration) and `create_from_file()` (skips the rewrite so
/// the user's authored frontmatter doesn't get rewritten lossily —
/// rich fields like `roster:` / `source_skill:` / `filter_framework:`
/// aren't in our SQLite schema, and we shouldn't strip them when the
/// user gave us the file as the source of truth).
fn create_inner(
    conn: &Connection,
    slug: &str,
    runtime: &str,
    display_name: Option<String>,
    description: Option<String>,
    model: Option<String>,
    system_prompt: Option<String>,
    project_id: Option<String>,
    write_runtime_file_too: bool,
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

    // v2.3.0 — write the per-runtime config file so the runtime's native
    // @-mention / agent-discovery finds this agent. Skipped when the
    // caller is `create_from_file`: the user already provided the file
    // and rewriting it would strip frontmatter fields outside our schema.
    let mut file_path_written: Option<PathBuf> = None;
    let mut file_action = "none".to_string();
    let runtime_visible = if write_runtime_file_too {
        match write_runtime_file(runtime, slug, &display, description.as_deref(), model.as_deref(), system_prompt.as_deref()) {
            Ok(Some(path)) => {
                file_path_written = Some(path);
                file_action = "wrote".to_string();
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
        }
    } else {
        // create_from_file path — the user owns the file. We record the
        // canonical runtime-file path on the row if it exists on disk
        // (so the GUI can locate it), but we never write it.
        if let Some(path) = agent_file_path(runtime, slug) {
            if path.exists() {
                file_path_written = Some(path);
                file_action = "registered".to_string();
                true
            } else {
                false
            }
        } else {
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
        file_action,
    };

    if opts.human {
        emit_human(&format!(
            "Created agent @{} ({}) on runtime {} — runtime_visible={}",
            result.slug, result.id, result.runtime, result.runtime_visible
        ));
        if let Some(p) = &result.file_path {
            let verb = match result.file_action.as_str() {
                "wrote" => "wrote",
                "registered" => "registered (file untouched at)",
                _ => "tracked",
            };
            emit_human(&format!("  {} {}", verb, p.display()));
        }
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

/// v2.4.6 — resolved agent record for downstream consumers like
/// `ato review --reviewer @<slug>`. We don't return the full row
/// (skills/mcps/permissions/etc. aren't needed for the persona-prepend
/// flow); only the fields the dispatch path actually consumes.
#[derive(Debug, Clone)]
pub struct AgentRef {
    pub slug: String,
    pub display_name: String,
    pub runtime: String,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
}

/// Look up an agent by slug. The agents table has a UNIQUE(runtime,
/// slug) constraint so the same slug can exist on multiple runtimes;
/// when that happens we prefer the most-recently-used one (typical
/// human intent when they just type `@reviewer-bot` without
/// disambiguating). Returns None when the slug doesn't match.
///
/// Callers that need the disambiguated variant can pass an explicit
/// `runtime` to scope the lookup. None means "any runtime, prefer
/// last_used_at."
pub fn lookup_by_slug(
    conn: &Connection,
    slug: &str,
    runtime: Option<&str>,
) -> Result<Option<AgentRef>> {
    let row: Option<(String, String, String, Option<String>, Option<String>)> = match runtime {
        Some(rt) => conn
            .query_row(
                "SELECT slug, display_name, runtime, model, system_prompt
                   FROM agents
                  WHERE runtime = ?1 AND slug = ?2
                  LIMIT 1",
                [rt, slug],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?,
        None => conn
            .query_row(
                "SELECT slug, display_name, runtime, model, system_prompt
                   FROM agents
                  WHERE slug = ?1
                  ORDER BY COALESCE(last_used_at, created_at) DESC
                  LIMIT 1",
                [slug],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?,
    };
    Ok(row.map(|(slug, display_name, runtime, model, system_prompt)| {
        AgentRef {
            slug,
            display_name,
            runtime,
            model,
            system_prompt,
        }
    }))
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
        file_action: "none".to_string(),
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

// ──────────────────────────────────────────────────────────────────────
// v2.6 — `ato agents create --from-file <path>`
//
// Bridges a Claude-Code-style agent file (markdown with YAML frontmatter,
// `~/.claude/agents/<slug>.md`) into the SQLite `agents` row that
// `ato dispatch <runtime> --agent <slug>` loads via `lookup_by_slug`.
// Without this, users must duplicate every persona file into a long
// inline `--system-prompt` arg, which is hostile to the war-room workflow
// (where personas are file-authored and version-controlled).
//
// Scope is intentionally narrow: top-level `name`/`display_name`/
// `description`/`model` strings + the body-after-frontmatter as the
// system_prompt. Multi-line YAML (`description: |` block scalars),
// nested keys (`roster:`), and other frontmatter fields are ignored;
// the user can override any parsed value with the existing CLI flags.
//
// Rejects fancy frontmatter gracefully — if a value our parser can't
// confidently extract is required (e.g. slug), the user gets a clear
// error pointing at the file + the override flag they can pass.
// ──────────────────────────────────────────────────────────────────────

/// Parsed values from an agent file's YAML frontmatter + body. All
/// fields are optional at parse time; the caller decides which are
/// required for the operation at hand.
#[derive(Debug, Default, Clone)]
pub struct ParsedAgentFile {
    pub slug: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
}

/// Read a Claude-Code-style agent file and extract the fields we know
/// how to map onto a SQLite `agents` row. Returns ParsedAgentFile with
/// the extracted values; caller merges with CLI overrides.
///
/// Format expected:
///
///   ---
///   name: <slug>
///   display_name: <human-readable>
///   description: <one line>
///   model: <runtime hint>
///   ---
///
///   <body — becomes system_prompt>
///
/// Unknown frontmatter fields are silently ignored. Files without
/// frontmatter are treated as body-only (slug then MUST come from
/// CLI or filename).
pub fn parse_agent_file(path: &Path) -> Result<ParsedAgentFile> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read agent file at {}", path.display()))?;

    // Detect leading `---\n` (allow trailing whitespace on the marker line).
    let (frontmatter_block, body) = split_frontmatter(&raw);

    let mut parsed = ParsedAgentFile::default();

    if let Some(fm) = frontmatter_block {
        parsed.slug = top_level_scalar(fm, "name");
        parsed.display_name = top_level_scalar(fm, "display_name");
        parsed.description = top_level_scalar(fm, "description");
        parsed.model = top_level_scalar(fm, "model");
    }

    // Fall back to the filename stem when frontmatter has no `name:`.
    if parsed.slug.is_none() {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            parsed.slug = Some(stem.to_string());
        }
    }

    // Body becomes the system prompt. Empty body is fine; callers may
    // intend to override with `--system-prompt` even when loading from
    // file (e.g. tweaking a persona for a single dispatch).
    let body_trimmed = body.trim();
    if !body_trimmed.is_empty() {
        parsed.system_prompt = Some(body_trimmed.to_string());
    }

    Ok(parsed)
}

/// Split a `---\n...\n---\n<body>` document into (frontmatter, body).
/// Returns (None, full_text) when no frontmatter is present so the
/// caller can still treat the whole file as a persona body.
fn split_frontmatter(raw: &str) -> (Option<&str>, &str) {
    // Strip a leading BOM if present.
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);

    // Require the file to start with `---` on its own line.
    if !raw.starts_with("---\n") && !raw.starts_with("---\r\n") {
        return (None, raw);
    }

    // Skip past the opening fence.
    let after_open = match raw.find('\n') {
        Some(i) => &raw[i + 1..],
        None => return (None, raw),
    };

    // Find the closing `---` on its own line.
    let mut closing_idx: Option<usize> = None;
    let mut cursor = 0;
    for line in after_open.split_inclusive('\n') {
        let line_no_newline = line.trim_end_matches(['\r', '\n']);
        if line_no_newline.trim() == "---" {
            closing_idx = Some(cursor);
            break;
        }
        cursor += line.len();
    }

    match closing_idx {
        Some(end) => {
            let fm = &after_open[..end];
            // Body starts AFTER the closing fence line.
            let after_close = &after_open[end..];
            let body = match after_close.find('\n') {
                Some(i) => &after_close[i + 1..],
                None => "",
            };
            (Some(fm), body)
        }
        None => (None, raw),
    }
}

/// Extract a top-level scalar like `key: value` from a YAML frontmatter
/// block. Limitations (deliberate, to keep this dep-free):
///
///   - Only matches lines with no leading whitespace (top-level keys).
///   - Stops at the next top-level key (so nested children of `roster:`
///     etc. don't accidentally leak into the next key's value).
///   - Treats `key: |` block scalars as None (caller should pass via
///     `--description` flag instead if they have multi-line copy).
///   - Strips surrounding double-quotes from the value.
fn top_level_scalar(block: &str, key: &str) -> Option<String> {
    let prefix = format!("{}:", key);
    for line in block.lines() {
        if !line.starts_with(&prefix) {
            continue;
        }
        let rest = line[prefix.len()..].trim();
        // Block-scalar indicators ("|", ">") aren't handled — surface as None.
        if rest == "|" || rest == ">" || rest.starts_with("|") || rest.starts_with(">") {
            return None;
        }
        // Empty value (just "key:") → None so caller can fall back.
        if rest.is_empty() {
            return None;
        }
        // Strip optional surrounding double quotes.
        let stripped = rest.trim_matches('"');
        return Some(stripped.to_string());
    }
    None
}

/// `ato agents create --from-file <path>` entry point. Parses the file,
/// merges with CLI overrides, then calls into the existing `create()`.
/// CLI overrides win on every field where the user supplied one.
pub fn create_from_file(
    conn: &Connection,
    path: &Path,
    runtime: &str,
    slug_override: Option<String>,
    display_name_override: Option<String>,
    description_override: Option<String>,
    model_override: Option<String>,
    system_prompt_override: Option<String>,
    project_id: Option<String>,
    opts: &Opts,
) -> Result<()> {
    let parsed = parse_agent_file(path)?;

    let slug = slug_override
        .or(parsed.slug)
        .ok_or_else(|| anyhow!(
            "Could not determine slug. The file at {} has no `name:` in its frontmatter and no recognizable filename stem; pass `--slug <value>` explicitly.",
            path.display()
        ))?;

    let display_name = display_name_override.or(parsed.display_name);
    let description = description_override.or(parsed.description);
    let model = model_override.or(parsed.model);

    let system_prompt = system_prompt_override.or(parsed.system_prompt);
    if system_prompt.is_none() {
        return Err(anyhow!(
            "No system prompt available. The file at {} has an empty body after the frontmatter; pass `--system-prompt <value>` or fill in the file.",
            path.display()
        ));
    }

    create_inner(
        conn,
        &slug,
        runtime,
        display_name,
        description,
        model,
        system_prompt,
        project_id,
        /* write_runtime_file_too = */ false,
        opts,
    )
}

// ──────────────────────────────────────────────────────────────────────
// v2.6 — `ato agents list [--runtime X] [--project-id Y]`
// ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AgentListRow {
    pub slug: String,
    pub runtime: String,
    pub display_name: String,
    pub model: Option<String>,
    pub description: Option<String>,
    pub project_id: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

pub fn list(
    conn: &Connection,
    runtime: Option<String>,
    project_id: Option<String>,
    opts: &Opts,
) -> Result<()> {
    // Build the query incrementally so we can pass optional filters
    // through rusqlite's parameter binding without string-concat.
    let mut sql = String::from(
        "SELECT slug, runtime, display_name, model, description, project_id, created_at, last_used_at
           FROM agents
          WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(rt) = runtime.as_ref() {
        sql.push_str(" AND runtime = ?");
        params.push(Box::new(rt.clone()));
    }
    if let Some(pid) = project_id.as_ref() {
        sql.push_str(" AND project_id = ?");
        params.push(Box::new(pid.clone()));
    }
    sql.push_str(" ORDER BY COALESCE(last_used_at, created_at) DESC");

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(AgentListRow {
                slug: r.get(0)?,
                runtime: r.get(1)?,
                display_name: r.get(2)?,
                model: r.get(3)?,
                description: r.get(4)?,
                project_id: r.get(5)?,
                created_at: r.get(6)?,
                last_used_at: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if opts.human {
        if rows.is_empty() {
            emit_human("No agents registered.");
        } else {
            emit_human(&format!("{} agent(s):", rows.len()));
            for r in &rows {
                let model = r.model.as_deref().unwrap_or("—");
                let last = r.last_used_at.as_deref().unwrap_or("never");
                emit_human(&format!(
                    "  @{:<20} runtime={:<10} model={:<28} last_used={}",
                    r.slug, r.runtime, model, last
                ));
            }
        }
    } else {
        emit_json(&rows)?;
    }
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────
// v2.6 — `ato agents delete --slug X [--runtime Y] [--also-remove-file]`
// ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AgentDeleteResult {
    pub slug: String,
    pub runtime: String,
    pub file_removed: Option<PathBuf>,
    pub action: String, // "deleted"
}

pub fn delete(
    conn: &Connection,
    slug: &str,
    runtime: Option<String>,
    also_remove_file: bool,
    opts: &Opts,
) -> Result<()> {
    // Resolve the (slug, runtime) target before deleting so we can
    // report cleanly. Disambiguation matches lookup_by_slug's policy:
    // explicit runtime wins; otherwise the most-recently-used.
    let target: Option<(String, String)> = match runtime.as_ref() {
        Some(rt) => conn
            .query_row(
                "SELECT slug, runtime FROM agents WHERE runtime = ?1 AND slug = ?2 LIMIT 1",
                [rt, slug],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?,
        None => conn
            .query_row(
                "SELECT slug, runtime FROM agents WHERE slug = ?1 ORDER BY COALESCE(last_used_at, created_at) DESC LIMIT 1",
                [slug],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?,
    };
    let (resolved_slug, resolved_runtime) = match target {
        Some(t) => t,
        None => {
            let scope = match runtime.as_ref() {
                Some(rt) => format!(" on runtime '{}'", rt),
                None => String::new(),
            };
            return Err(anyhow!(
                "No agent with slug '{}'{} found.",
                slug, scope
            ));
        }
    };

    let deleted = conn.execute(
        "DELETE FROM agents WHERE runtime = ?1 AND slug = ?2",
        [&resolved_runtime, &resolved_slug],
    )?;
    if deleted == 0 {
        return Err(anyhow!(
            "Agent '{}' on runtime '{}' was resolved but DELETE affected 0 rows — concurrent removal?",
            resolved_slug, resolved_runtime
        ));
    }

    // Log the delete as a config-change so the ledger / regression
    // detector still see it (matches the pattern in create / update).
    log_config_change(
        conn,
        &resolved_slug,
        "delete",
        Some(&resolved_runtime),
        None,
        "ato-cli",
    )?;

    // Optional: remove the runtime config file. Off by default —
    // files are often checked into git or shared across machines, so
    // removing them silently from a CLI flag is hostile.
    let mut file_removed: Option<PathBuf> = None;
    if also_remove_file {
        if let Some(path) = agent_file_path(&resolved_runtime, &resolved_slug) {
            if path.exists() {
                // Backup before removing, same discipline as the
                // write path. Backup goes to ".md.bak" so the human
                // can recover if they regret it.
                let backup = path.with_extension("md.bak");
                let _ = fs::copy(&path, &backup);
                fs::remove_file(&path)
                    .with_context(|| format!("Failed to remove {}", path.display()))?;
                file_removed = Some(path);
            }
        }
    }

    let result = AgentDeleteResult {
        slug: resolved_slug,
        runtime: resolved_runtime,
        file_removed,
        action: "deleted".to_string(),
    };

    if opts.human {
        emit_human(&format!(
            "Deleted agent @{} from runtime {}.",
            result.slug, result.runtime
        ));
        if let Some(p) = &result.file_removed {
            emit_human(&format!("  removed file {} (backup at .md.bak)", p.display()));
        }
    } else {
        emit_json(&result)?;
    }
    Ok(())
}
