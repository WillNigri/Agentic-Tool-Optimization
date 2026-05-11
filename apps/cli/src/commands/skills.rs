// `ato skills draft --from-replay <job-id> [--out path]`
//
// Generate a SKILL.md draft from a successful replay. The replay tells
// us: a prompt that originally failed on runtime A, succeeded on runtime
// B. The skill draft encodes that routing decision so future prompts
// like it get sent to the right runtime.
//
// Phase 1 ships the SKILL.md draft *only* — the human reviews + the
// resolver entry / smoke test / unit tests follow in later phases when
// we have the ops-recipe infrastructure. Honest about scope.

use crate::output::{emit_human, emit_json, Opts};
use anyhow::{anyhow, Context, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct SkillDraftResult {
    pub source_replay_job_id: String,
    pub skill_name: String,
    pub out_path: PathBuf,
    pub written: bool,
    pub draft_preview: String,
}

pub fn draft_from_replay(
    conn: &Connection,
    job_id: &str,
    out: Option<PathBuf>,
    opts: &Opts,
) -> Result<()> {
    let row: (String, String, String, Option<String>, String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT id, source_runtime, target_runtime, target_model, status,
                    (SELECT prompt FROM execution_logs WHERE id = rj.source_execution_log_id) AS source_prompt,
                    response
               FROM replay_jobs rj
              WHERE id = ?1",
            [job_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .context("Replay job not found. Find one with `ato replays for-trace <trace-id>`.")?;

    let (_replay_id, source_runtime, target_runtime, target_model, status, source_prompt, _replay_response) = row;

    if status != "done" {
        return Err(anyhow!(
            "Replay job {} has status '{}', not 'done'. Skill drafting needs a successful replay.",
            job_id,
            status
        ));
    }

    let source_prompt = source_prompt
        .ok_or_else(|| anyhow!("Source prompt is missing from the linked execution_logs row."))?;

    // Skill slug: a short, kebab-case identifier derived from the runtime swap.
    let skill_name = format!("route-{}-to-{}", source_runtime, target_runtime);
    let prompt_summary = summarize(&source_prompt, 80);

    let draft = format_skill_md(
        &skill_name,
        &source_runtime,
        &target_runtime,
        target_model.as_deref(),
        &prompt_summary,
        job_id,
    );

    // Default output: ~/.claude/skills/<skill-name>/SKILL.md when the
    // *target* is claude, else ~/.codex/skills/... etc. If the user
    // passed --out, honor it.
    let out_path = out.unwrap_or_else(|| default_skill_path(&target_runtime, &skill_name));

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).context("Failed to create skills directory")?;
    }
    fs::write(&out_path, &draft).context("Failed to write SKILL.md")?;

    let result = SkillDraftResult {
        source_replay_job_id: job_id.to_string(),
        skill_name,
        out_path,
        written: true,
        draft_preview: draft.lines().take(8).collect::<Vec<_>>().join("\n"),
    };

    if opts.human {
        emit_human(&format!(
            "Skill drafted: {}\n  written to: {}\n\nPreview:\n{}\n…",
            result.skill_name,
            result.out_path.display(),
            result.draft_preview
        ));
    } else {
        emit_json(&result)?;
    }
    Ok(())
}

fn default_skill_path(target_runtime: &str, skill_name: &str) -> PathBuf {
    let mut home = crate::db::home_dir();
    match target_runtime {
        "claude" => home.push(".claude/skills"),
        "codex" => home.push(".codex/skills"),
        "gemini" => home.push(".gemini/skills"),
        "openclaw" => home.push(".openclaw/skills"),
        "hermes" => home.push(".hermes/skills"),
        _ => home.push(".ato/skills"),
    }
    home.push(skill_name);
    home.push("SKILL.md");
    home
}

fn format_skill_md(
    name: &str,
    source_runtime: &str,
    target_runtime: &str,
    target_model: Option<&str>,
    prompt_summary: &str,
    source_replay_job_id: &str,
) -> String {
    let model_line = target_model
        .map(|m| format!("\n# Pinned model: {}\n", m))
        .unwrap_or_default();

    format!(
        r#"---
name: {name}
description: "Route prompts like '{prompt_summary}' to {target_runtime} — earlier replay showed {source_runtime} was failing on this shape."
allowed-tools: []
---
{model_line}
# Why this skill exists

A replay of a real failing dispatch on `{source_runtime}` showed `{target_runtime}` handled the same prompt cleanly. This skill encodes that routing decision so future prompts matching the same shape get sent to the runtime that works.

Source replay job: `{source_replay_job_id}`

# When to fire

When the user's request resembles the source prompt:

> {prompt_summary}

Specifically, route the prompt to **`{target_runtime}`** instead of `{source_runtime}`.

# How to fire

```
ato dispatch {target_runtime} "<prompt>"
```

If you (the agent) are inside an MCP-enabled harness, call the `start_dispatch` tool with `runtime: "{target_runtime}"`.

# Notes for the human

This skill was auto-drafted by `ato skills draft --from-replay {source_replay_job_id}`. Review the routing decision, refine the trigger description above so it captures the *shape* of the prompt rather than the exact text, then commit.

Follow-ups not yet automated (Phase 4+ in the v2.3.0 roadmap):
- Unit tests against fixture prompts
- LLM-as-judge eval on (source, replay) pairs
- Resolver-routing test
- DRY audit against existing skills
"#,
        name = name,
        prompt_summary = prompt_summary,
        source_runtime = source_runtime,
        target_runtime = target_runtime,
        model_line = model_line,
        source_replay_job_id = source_replay_job_id,
    )
}

fn summarize(text: &str, max_chars: usize) -> String {
    // Take the first non-empty line, truncate to max_chars. Strip
    // common assistant-framing prefixes that show up in stitched
    // pipeline prompts so the summary reads as the actual user ask.
    let first_line = text
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .replace("[user]:", "")
        .trim()
        .to_string();
    if first_line.chars().count() <= max_chars {
        first_line
    } else {
        let truncated: String = first_line.chars().take(max_chars).collect();
        format!("{}…", truncated)
    }
}

#[allow(dead_code)]
pub fn resolve_default_path(target_runtime: &str, skill_name: &str) -> PathBuf {
    default_skill_path(target_runtime, skill_name)
}

#[allow(dead_code)]
pub fn writable_check(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}
