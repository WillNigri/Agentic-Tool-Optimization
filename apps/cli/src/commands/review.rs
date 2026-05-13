// `ato review [--against <ref>]` — multi-LLM code review with
// rich context, not just the diff.
//
// Today's `ato dispatch <reviewer> "<diff>"` pattern feeds reviewers
// only the diff hunks. That misses what a real human reviewer does:
//   - reads the FULL current file to understand what surrounds the
//     change
//   - looks at recent git history to see what's been churning
//   - runs the test suite to see if the change is even green
//
// Without those, LLM reviewers hallucinate findings. They flag "this
// function probably doesn't exist" when it does, sitting two files
// over. They miss "this caller now passes the wrong shape" because
// they never see the caller.
//
// This command bundles all the context a Tier-1 LLM-without-tools
// review needs (diff + touched-file content + git log + test
// output) into one structured prompt, then orchestrates dispatch to
// N reviewers in a shared session so the second reviewer can see
// what the first one said via history replay — same pattern the
// ato-review skill uses manually, automated.
//
// Tier 2 (function-calling so the reviewer can iterate: "grep for
// X", "open file Y at line N") is the follow-up. Tier 1 captures
// 80% of the human-reviewer experience at a fraction of the
// integration cost.

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

use crate::commands::sessions;
use crate::db;
use crate::output::{emit_human, emit_json, Opts};

/// Max bytes of file content we include per touched file. Keeps the
/// overall prompt under ~100KB even when several large files are
/// touched; reviewers get the most relevant chunks via the diff
/// either way.
const PER_FILE_CONTENT_CAP: usize = 8 * 1024;

/// Max overall prompt size. Roughly aligns with the 32k-token
/// context window that's safe across all providers we ship.
const PROMPT_CAP_BYTES: usize = 96 * 1024;

#[derive(Debug, Serialize)]
pub struct TouchedFile {
    pub path: String,
    pub content_after: Option<String>,
    pub recent_log: String,
}

#[derive(Debug, Serialize)]
pub struct BuildStatus {
    pub command: String,
    pub exit_code: Option<i32>,
    pub tail: String,
}

#[derive(Debug, Serialize)]
pub struct ReviewContext {
    pub base_ref: String,
    pub head_ref: String,
    pub diff: String,
    pub touched_files: Vec<TouchedFile>,
    pub build: Option<BuildStatus>,
    pub tests: Option<BuildStatus>,
}

/// CLI entry point.
#[allow(clippy::too_many_arguments)]
pub fn run(
    base: Option<&str>,
    reviewers: Vec<String>,
    out_path: Option<&str>,
    skip_build: bool,
    skip_tests: bool,
    consensus: bool,
    lean: bool,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    let ctx = gather_context(base, skip_build, skip_tests)?;
    if ctx.diff.trim().is_empty() {
        emit_human("Nothing to review — no diff against the base ref.");
        return Ok(());
    }

    if opts.human {
        emit_human(&format!(
            "Review context: {} touched file(s), diff {} bytes, prompt cap {} bytes.",
            ctx.touched_files.len(),
            ctx.diff.len(),
            PROMPT_CAP_BYTES,
        ));
        if let Some(b) = &ctx.build {
            emit_human(&format!(
                "  build: exit {:?}",
                b.exit_code.unwrap_or(-1)
            ));
        }
        if let Some(t) = &ctx.tests {
            emit_human(&format!(
                "  tests: exit {:?}",
                t.exit_code.unwrap_or(-1)
            ));
        }
    }

    let configured = resolve_reviewers(reviewers, db_path)?;
    if configured.is_empty() {
        anyhow::bail!(
            "No reviewers configured. Pass --reviewer <slug>, or set up an API key for minimax / google / grok / deepseek / qwen / openrouter."
        );
    }
    if opts.human {
        emit_human(&format!(
            "Reviewers: {}",
            configured.join(" → ")
        ));
    }

    let mut transcript = run_review(&ctx, &configured, lean, db_path, opts)?;

    // Consensus pass — only meaningful with 2+ reviewers. Asks each
    // reviewer (in the same session) to push back on the prior
    // reviewers' findings: "of all findings 1..N, which do you
    // disagree with and why?". Closes the "polite-agree" gap where
    // reviewer #2 nods at everything reviewer #1 said.
    if consensus && configured.len() >= 2 {
        let consensus_turns =
            run_consensus_pass(&transcript, &configured, db_path, opts)?;
        transcript.consensus = Some(consensus_turns);
    }

    if let Some(path) = out_path {
        write_transcript_markdown(&transcript, path)
            .with_context(|| format!("write transcript to {}", path))?;
        if opts.human {
            emit_human(&format!("Transcript saved to {}", path));
        }
    } else if !opts.human {
        emit_json(&transcript)?;
    }
    Ok(())
}

/// Compute the base ref (default: merge base with origin/main, or
/// `main` if no origin) so the diff is "what this branch changed."
fn resolve_base_ref(explicit: Option<&str>) -> Result<String> {
    if let Some(r) = explicit {
        return Ok(r.to_string());
    }
    // Try origin/main → main → HEAD~1 in that order.
    for candidate in &["origin/main", "main", "HEAD~1"] {
        let out = Command::new("git")
            .args(["rev-parse", "--verify", candidate])
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                // Use merge-base so a long-running feature branch
                // doesn't appear to have changed everything that
                // landed on main in the meantime.
                let mb = Command::new("git")
                    .args(["merge-base", "HEAD", candidate])
                    .output()?;
                if mb.status.success() {
                    let sha = String::from_utf8_lossy(&mb.stdout).trim().to_string();
                    if !sha.is_empty() {
                        return Ok(sha);
                    }
                }
                return Ok(candidate.to_string());
            }
        }
    }
    anyhow::bail!("could not resolve a base ref; pass --against <ref> explicitly")
}

pub fn gather_context(
    base: Option<&str>,
    skip_build: bool,
    skip_tests: bool,
) -> Result<ReviewContext> {
    let base_ref = resolve_base_ref(base)?;
    let head_ref = git_str(&["rev-parse", "HEAD"])?;
    let diff = git_str(&[
        "diff",
        "--no-color",
        "--no-prefix",
        &format!("{}...HEAD", base_ref),
    ])?;
    let touched_paths = git_str(&[
        "diff",
        "--name-only",
        &format!("{}...HEAD", base_ref),
    ])?;
    let touched_files = touched_paths
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(load_touched_file)
        .collect();

    // Best-effort build + test. Skipped via flags so a reviewer who
    // already ran them externally isn't paying the rebuild cost
    // again, and so we don't burn 5+ minutes on a build for a
    // doc-only change.
    let build = if skip_build {
        None
    } else if any_rust_changed(&diff) {
        Some(run_build("cargo build --manifest-path apps/cli/Cargo.toml -p ato"))
    } else {
        None
    };
    let tests = if skip_tests {
        None
    } else if any_rust_changed(&diff) {
        Some(run_build("cargo test --manifest-path apps/cli/Cargo.toml -p ato"))
    } else {
        None
    };

    Ok(ReviewContext {
        base_ref,
        head_ref,
        diff,
        touched_files,
        build,
        tests,
    })
}

fn load_touched_file(path: &str) -> TouchedFile {
    let content_after = match std::fs::read_to_string(path) {
        Ok(s) if s.len() <= PER_FILE_CONTENT_CAP => Some(s),
        Ok(s) => Some(format!(
            "{}\n\n[... file truncated to first {} bytes ...]",
            &s[..PER_FILE_CONTENT_CAP],
            PER_FILE_CONTENT_CAP
        )),
        Err(_) => None,
    };
    // Recent commit messages touching this file — 5 entries is plenty
    // for the reviewer to see "this is hot churn" vs "this hasn't
    // moved in 18 months."
    let recent_log = Command::new("git")
        .args([
            "log",
            "-5",
            "--no-color",
            "--pretty=format:%h %ad %s",
            "--date=short",
            "--",
            path,
        ])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    TouchedFile {
        path: path.to_string(),
        content_after,
        recent_log,
    }
}

fn any_rust_changed(diff: &str) -> bool {
    diff.lines()
        .filter(|l| l.starts_with("diff --git"))
        .any(|l| l.contains(".rs"))
}

fn run_build(cmd: &str) -> BuildStatus {
    let pieces: Vec<&str> = cmd.split_whitespace().collect();
    let (head, rest) = pieces.split_first().map(|(h, r)| (*h, r)).unwrap_or(("", &[]));
    let out = Command::new(head).args(rest).output();
    match out {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let combined = format!("{}\n{}", stdout, stderr);
            let tail = if combined.len() > 4096 {
                format!("[... truncated ...]\n{}", &combined[combined.len() - 4096..])
            } else {
                combined
            };
            BuildStatus {
                command: cmd.to_string(),
                exit_code: o.status.code(),
                tail,
            }
        }
        Err(e) => BuildStatus {
            command: cmd.to_string(),
            exit_code: None,
            tail: format!("spawn error: {}", e),
        },
    }
}

fn git_str(args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("spawn git {:?}", args))?;
    if !out.status.success() {
        anyhow::bail!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Default reviewer order: the first two API providers that have a
/// key configured, prioritizing minimax + google when both exist
/// because they're the only ones we've smoke-tested. Explicit
/// --reviewer flags override.
fn resolve_reviewers(explicit: Vec<String>, db_path: &PathBuf) -> Result<Vec<String>> {
    if !explicit.is_empty() {
        return Ok(explicit);
    }
    let conn = db::open_readonly(db_path)?;
    let preferred = [
        "minimax",
        "google",
        "grok",
        "deepseek",
        "qwen",
        "openrouter",
    ];
    let mut out = Vec::new();
    for slug in preferred {
        let has_key = conn
            .query_row(
                "SELECT 1 FROM llm_api_keys WHERE LOWER(provider) = ?1 AND is_active = 1",
                [slug],
                |_| Ok(()),
            )
            .is_ok();
        if has_key && out.len() < 2 {
            out.push(slug.to_string());
        }
    }
    Ok(out)
}

#[derive(Debug, Serialize)]
pub struct ReviewerTurn {
    pub reviewer: String,
    pub status: String,
    pub response: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ReviewTranscript {
    pub base_ref: String,
    pub head_ref: String,
    pub session_id: String,
    pub reviewers: Vec<ReviewerTurn>,
    pub context_summary: ContextSummary,
    /// Round-2 turns when --consensus was passed. Each reviewer
    /// answers "of the findings above, which do you disagree with
    /// and why?" so the human-in-the-loop sees real conflicts, not
    /// polite agreement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consensus: Option<Vec<ReviewerTurn>>,
}

#[derive(Debug, Serialize)]
pub struct ContextSummary {
    pub touched_files: usize,
    pub diff_bytes: usize,
    pub build_exit: Option<i32>,
    pub tests_exit: Option<i32>,
}

fn run_review(
    ctx: &ReviewContext,
    reviewers: &[String],
    lean: bool,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<ReviewTranscript> {
    // Open a fresh session anchored to the first reviewer. The
    // anchor is just a label; history replay will carry context to
    // every other runtime in the chain regardless.
    let anchor_runtime = reviewers
        .first()
        .ok_or_else(|| anyhow!("no reviewers"))?
        .clone();
    let session = sessions::create_inner(
        &db::open_readwrite(db_path)?,
        &anchor_runtime,
        None,
        Some(&format!("review/{}", short_head(ctx))),
    )?;
    let session_id = session.id.clone();

    let mut turns: Vec<ReviewerTurn> = Vec::with_capacity(reviewers.len());
    for (idx, reviewer) in reviewers.iter().enumerate() {
        let prompt = build_prompt_for(reviewer, idx, ctx, reviewers, lean);
        if opts.human {
            emit_human(&format!(
                "\n--- Reviewer #{} ({}) — {} char prompt ---",
                idx + 1,
                reviewer,
                prompt.chars().count()
            ));
        }
        let started = std::time::Instant::now();
        let outcome = crate::commands::dispatch::run(
            reviewer,
            &prompt,
            None,
            None,
            Some(session_id.clone()),
            false, // no streaming for review (we want the full reply)
            false, // no JSONL
            true,  // Tier 2 — reviewers get function-calling tools when their flavor supports it
            db_path,
            opts,
        );
        let duration_ms = started.elapsed().as_millis() as i64;

        // Pull the assistant's reply back out of session_turns since
        // dispatch::run doesn't return the text directly.
        let conn = db::open_readonly(db_path)?;
        let response: Option<String> = conn
            .query_row(
                "SELECT text FROM session_turns
                  WHERE session_id = ?1 AND role = 'assistant'
                  ORDER BY turn_index DESC
                  LIMIT 1",
                [&session_id],
                |r| r.get(0),
            )
            .ok();
        match outcome {
            Ok(()) => turns.push(ReviewerTurn {
                reviewer: reviewer.clone(),
                status: "ok".into(),
                response,
                error: None,
                duration_ms: Some(duration_ms),
            }),
            Err(e) => turns.push(ReviewerTurn {
                reviewer: reviewer.clone(),
                status: "error".into(),
                response: None,
                error: Some(format!("{}", e)),
                duration_ms: Some(duration_ms),
            }),
        }
    }

    Ok(ReviewTranscript {
        base_ref: ctx.base_ref.clone(),
        head_ref: ctx.head_ref.clone(),
        session_id,
        reviewers: turns,
        context_summary: ContextSummary {
            touched_files: ctx.touched_files.len(),
            diff_bytes: ctx.diff.len(),
            build_exit: ctx.build.as_ref().and_then(|b| b.exit_code),
            tests_exit: ctx.tests.as_ref().and_then(|b| b.exit_code),
        },
        consensus: None,
    })
}

/// Round-2 cross-reviewer push-back. Each reviewer is dispatched
/// into the SAME session with a short prompt asking "of the
/// findings above, which do you disagree with, AND which from
/// reviewer-N do you want to push back on?". History replay carries
/// the full prior conversation so they see who said what.
fn run_consensus_pass(
    transcript: &ReviewTranscript,
    reviewers: &[String],
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<Vec<ReviewerTurn>> {
    let session_id = &transcript.session_id;
    let mut out: Vec<ReviewerTurn> = Vec::with_capacity(reviewers.len());
    for (idx, reviewer) in reviewers.iter().enumerate() {
        let prompt = format!(
            "@{} — consensus round. You can see all prior reviewer turns above in this session's history. \
             Reply briefly to TWO questions:\n\
             1. Which of YOUR OWN findings would you withdraw or down-grade after reading the other reviewer(s)?\n\
             2. Which findings from the OTHER reviewer(s) do you actively disagree with, and why? Cite the file/line.\n\n\
             Don't repeat the original findings — just the deltas. If you have nothing to push back on, say so in one line.",
            reviewer
        );
        if opts.human {
            emit_human(&format!(
                "\n--- Consensus round, reviewer #{} ({}) ---",
                idx + 1,
                reviewer
            ));
        }
        let started = std::time::Instant::now();
        let outcome = crate::commands::dispatch::run(
            reviewer,
            &prompt,
            None,
            None,
            Some(session_id.clone()),
            false,
            false,
            true, // tools enabled — consensus reviewers may want to re-check claims
            db_path,
            opts,
        );
        let duration_ms = started.elapsed().as_millis() as i64;
        let conn = db::open_readonly(db_path)?;
        let response: Option<String> = conn
            .query_row(
                "SELECT text FROM session_turns
                  WHERE session_id = ?1 AND role = 'assistant'
                  ORDER BY turn_index DESC
                  LIMIT 1",
                [session_id],
                |r| r.get(0),
            )
            .ok();
        out.push(match outcome {
            Ok(()) => ReviewerTurn {
                reviewer: reviewer.clone(),
                status: "ok".into(),
                response,
                error: None,
                duration_ms: Some(duration_ms),
            },
            Err(e) => ReviewerTurn {
                reviewer: reviewer.clone(),
                status: "error".into(),
                response: None,
                error: Some(format!("{}", e)),
                duration_ms: Some(duration_ms),
            },
        });
    }
    Ok(out)
}

fn short_head(ctx: &ReviewContext) -> String {
    ctx.head_ref.chars().take(8).collect::<String>()
}

fn build_prompt_for(
    reviewer: &str,
    idx: usize,
    ctx: &ReviewContext,
    all_reviewers: &[String],
    lean: bool,
) -> String {
    let role = if idx == 0 {
        format!(
            "You are reviewer #1 of {}. Reviewers after you: {}.",
            all_reviewers.len(),
            all_reviewers[idx + 1..].join(", ")
        )
    } else {
        format!(
            "You are reviewer #{} of {}. You can see prior reviewers' findings via the session history above. Do NOT duplicate their findings; confirm/push-back briefly, then add what they missed.",
            idx + 1,
            all_reviewers.len()
        )
    };

    let mut body = String::with_capacity(PROMPT_CAP_BYTES);
    body.push_str(&format!(
        "# Code review request for `@{}`\n\n{}\n\n",
        reviewer, role
    ));
    body.push_str(&format!(
        "**Base**: `{}`\n**Head**: `{}`\n\n",
        ctx.base_ref, ctx.head_ref
    ));

    // Tools section moved to the top so the reviewer reads the
    // contract before deciding what to do. In lean mode we make
    // tool use mandatory; in default mode it's strongly encouraged.
    body.push_str("## Tools available — use these to verify\n\n");
    body.push_str("You have function-calling access to these tools. Real human reviewers go to the code; do the same:\n\n");
    body.push_str("- `read_file(path, start_line?, end_line?)` — read any file in the repo.\n");
    body.push_str("- `grep(pattern, glob?)` — search tracked files for a symbol or pattern.\n");
    body.push_str("- `git_log(path, n?)` — recent commits touching a file.\n\n");
    if lean {
        body.push_str("**Lean mode**: the bundle below contains the DIFF and a list of touched files — but NOT their full content. To examine a function, the surrounding context, or related callers, you MUST call `read_file` / `grep`. Plan two passes: (1) explore — read each touched file's relevant region; (2) verify — grep for callers of any symbol you flag. Don't write findings from the diff alone.\n\n");
    } else {
        body.push_str("**Required**: Before writing your final findings, call at least one tool to verify something in the live repo. The bundle below is a starting point, not the whole truth.\n\n");
    }

    if let Some(b) = &ctx.build {
        body.push_str(&format!(
            "## Build status: exit {}\n\n```\n{}\n```\n\n",
            b.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "?".into()),
            tail_for_prompt(&b.tail, 1500)
        ));
    }
    if let Some(t) = &ctx.tests {
        body.push_str(&format!(
            "## Test status: exit {}\n\n```\n{}\n```\n\n",
            t.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "?".into()),
            tail_for_prompt(&t.tail, 1500)
        ));
    }

    body.push_str("## Diff\n\n```diff\n");
    let diff_budget = PROMPT_CAP_BYTES.saturating_sub(body.len() + 2048);
    if ctx.diff.len() <= diff_budget {
        body.push_str(&ctx.diff);
    } else {
        body.push_str(&ctx.diff[..diff_budget]);
        body.push_str("\n[... diff truncated to fit prompt cap ...]\n");
    }
    body.push_str("\n```\n\n");

    if lean {
        // Lean mode — paths + recent log only, no content. Reviewer
        // is expected to call read_file / grep to examine code.
        body.push_str("## Touched files (paths only — call `read_file` to inspect)\n\n");
        for f in &ctx.touched_files {
            let size_hint = f
                .content_after
                .as_ref()
                .map(|c| format!(" ({} bytes)", c.len()))
                .unwrap_or_default();
            body.push_str(&format!("- `{}`{}\n", f.path, size_hint));
            if !f.recent_log.is_empty() {
                let log_line = f.recent_log.lines().next().unwrap_or("");
                body.push_str(&format!("  - latest: `{}`\n", log_line));
            }
        }
        body.push_str("\nTo see what these files look like now: `read_file(path)`. To see history: `git_log(path)`.\n\n");
    } else {
        // Default mode — full file content bundled (legacy behavior).
        body.push_str("## Touched files (full current text after change)\n\n");
        for f in &ctx.touched_files {
            if body.len() > PROMPT_CAP_BYTES * 3 / 4 {
                body.push_str(&format!(
                    "- `{}` (content omitted: prompt cap — call `read_file` to inspect)\n",
                    f.path
                ));
                continue;
            }
            body.push_str(&format!("### `{}`\n\n", f.path));
            if !f.recent_log.is_empty() {
                body.push_str(&format!(
                    "Recent commits touching this file:\n```\n{}\n```\n\n",
                    f.recent_log
                ));
            }
            if let Some(content) = &f.content_after {
                let lang = match std::path::Path::new(&f.path)
                    .extension()
                    .and_then(|s| s.to_str())
                {
                    Some("rs") => "rust",
                    Some("ts" | "tsx") => "typescript",
                    Some("js" | "mjs") => "javascript",
                    Some("py") => "python",
                    Some("md") => "markdown",
                    Some("json") => "json",
                    _ => "",
                };
                body.push_str(&format!("```{}\n{}\n```\n\n", lang, content));
            }
        }
    }

    body.push_str("## Your review\n\n");
    body.push_str("Reply with a numbered list of findings, severity-tagged (HIGH / MEDIUM / LOW / INFO). For each:\n");
    body.push_str("- N. **SEVERITY — short title**\n");
    body.push_str("  - Description: what's wrong and why it matters\n");
    body.push_str("  - Location: file + line\n");
    body.push_str("  - Fix: a concrete change\n");
    body.push_str("  - Verified-via: which tool calls you used to confirm this (e.g. `read_file foo.rs:120-160`, `grep canonical_signing_bytes`). If you didn't verify in the repo, write `prompt-only` — but expect that finding to be weighted lower.\n\n");
    body.push_str("5–10 findings max. Skip the obvious. If a candidate finding turns out not to apply on closer look, say so explicitly. Don't pad.\n");

    body
}

fn tail_for_prompt(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("[...{}B head omitted...]\n{}", s.len() - max, &s[s.len() - max..])
    }
}

fn write_transcript_markdown(t: &ReviewTranscript, path: &str) -> Result<()> {
    use std::io::Write as _;
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "# Multi-LLM review — {} → {}", short(&t.base_ref), short(&t.head_ref))?;
    writeln!(f)?;
    writeln!(
        f,
        "Session: `{}`. Reviewers in order: {}.",
        t.session_id,
        t.reviewers
            .iter()
            .map(|r| format!("@{}", r.reviewer))
            .collect::<Vec<_>>()
            .join(" → ")
    )?;
    writeln!(f)?;
    writeln!(f, "## Context")?;
    writeln!(f)?;
    writeln!(
        f,
        "- Touched files: {}\n- Diff bytes: {}\n- Build exit: {:?}\n- Tests exit: {:?}",
        t.context_summary.touched_files,
        t.context_summary.diff_bytes,
        t.context_summary.build_exit,
        t.context_summary.tests_exit
    )?;
    writeln!(f)?;
    for (i, r) in t.reviewers.iter().enumerate() {
        writeln!(
            f,
            "## Reviewer {}: @{} ({}, {}ms)",
            i + 1,
            r.reviewer,
            r.status,
            r.duration_ms.unwrap_or(0)
        )?;
        writeln!(f)?;
        if let Some(resp) = &r.response {
            writeln!(f, "{}", resp)?;
        }
        if let Some(err) = &r.error {
            writeln!(f, "_dispatch error: {}_", err)?;
        }
        writeln!(f)?;
    }

    if let Some(consensus) = &t.consensus {
        writeln!(f, "## Consensus round")?;
        writeln!(f)?;
        writeln!(
            f,
            "Each reviewer re-saw the full conversation and answered: (1) which of their own findings do they withdraw, (2) which from the others do they push back on."
        )?;
        writeln!(f)?;
        for (i, r) in consensus.iter().enumerate() {
            writeln!(
                f,
                "### @{} ({}, {}ms)",
                r.reviewer,
                r.status,
                r.duration_ms.unwrap_or(0)
            )?;
            writeln!(f)?;
            if let Some(resp) = &r.response {
                writeln!(f, "{}", resp)?;
            }
            if let Some(err) = &r.error {
                writeln!(f, "_dispatch error: {}_", err)?;
            }
            writeln!(f)?;
            // separator between consensus turns
            if i + 1 < consensus.len() {
                writeln!(f, "---")?;
                writeln!(f)?;
            }
        }
    }

    // Audit-notes scaffold the human-in-the-loop fills in. PRs that
    // paste this transcript verbatim include the audit decisions
    // inline so the reviewer of the PR sees what was applied vs
    // deferred and why.
    writeln!(f, "## Audit notes (fill in before PR)")?;
    writeln!(f)?;
    writeln!(
        f,
        "For each finding above, record decision + one-line rationale. Format:\n\n- `Finding N (reviewer X)`: APPLIED / DEFERRED / FALSE-POSITIVE — rationale.\n"
    )?;
    Ok(())
}

fn short(s: &str) -> String {
    s.chars().take(8).collect()
}
