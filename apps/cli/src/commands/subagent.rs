// `ato subagent log` — record Claude Code subagent (code-writer / cso /
// pr-reviewer / etc.) runs in `execution_logs` so they appear in the
// same Sessions feed as `ato dispatch` runs.
//
// Why: Claude Code's Agent tool dispatches a sub-conversation through
// Anthropic's API. Those runs don't go through `ato dispatch` and so
// don't land in execution_logs — meaning the bulk of agent-driven code
// production was invisible in the desktop app (issue raised 2026-06-14
// during the v2.17 session: "I don't see new sessions in the app").
//
// Cleanest fix: bracket every Agent tool invocation with this CLI. The
// pending row is written BEFORE the agent fires; the finish row writes
// the response + tokens + status after it returns. Multi-agent fan-outs
// share a `--war-room-id` and can be summarized via
// `ato war-rooms close <id>` exactly like the existing dispatch war-rooms.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use rusqlite::params;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

use crate::db;
use crate::output::{emit_human, emit_json, Opts};

/// Codex R1+R2 fix — match dispatch.rs::truncate's 64KB cap so
/// subagent rows respect the desktop's log-size assumptions and
/// don't bloat execution_logs.
const MAX_LOG_BYTES: usize = 64 * 1024;
fn truncate_for_log(s: &str) -> String {
    if s.len() <= MAX_LOG_BYTES {
        s.to_string()
    } else {
        format!("{}…[truncated]", &s[..MAX_LOG_BYTES])
    }
}

/// Codex R1+R2 fix — validate persona as a slug-shape (lowercase
/// alpha + dash + digits + colon for sub-personas like `agent:claude`).
/// Pre-fix shape wrote raw values like `Explore` or `code-writer`
/// directly to agent_slug, polluting agent-targeted analytics with
/// non-agent rows. Reject everything that isn't already slug-shaped.
fn validate_persona_slug(s: &str) -> Result<()> {
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == ':' || c == '_');
    if !ok {
        anyhow::bail!(
            "--persona must be lowercase slug-shape (a-z 0-9 - _ :), got: {:?}",
            s
        );
    }
    Ok(())
}

/// Codex R1 fix — desktop close/get/reopen paths hard-reject non-UUID
/// war_room_ids. Pre-fix shape accepted any string; this matches the
/// strictness `ato dispatch` enforces on the same flag.
fn validate_uuid(label: &str, v: &str) -> Result<()> {
    Uuid::parse_str(v).with_context(|| format!("--{} must be a UUID, got: {:?}", label, v))?;
    Ok(())
}

#[derive(Args, Debug)]
pub struct SubagentArgs {
    #[command(subcommand)]
    pub sub: SubagentSub,
}

#[derive(Subcommand, Debug)]
pub enum SubagentSub {
    /// Subagent run logging. Pair with Claude Code's Agent tool: call
    /// `create` to write a pending row; capture the printed id; call
    /// `finish <id>` once the subagent returns.
    Log {
        #[command(subcommand)]
        sub: LogSub,
    },
}

#[derive(Subcommand, Debug)]
pub enum LogSub {
    /// Create a pending execution_logs row for a subagent that's about
    /// to run. Prints the new log id to stdout for the caller to pass
    /// to `finish` afterwards.
    Create {
        /// Subagent persona (e.g. code-writer, cso, pr-reviewer, Explore).
        #[arg(long)]
        persona: String,
        /// Prompt the subagent is being given. Either inline or @<path>.
        #[arg(long)]
        prompt: String,
        /// Optional war_room_id to cluster a fan-out (mint with uuidgen
        /// and pass to every `create` in the fan-out).
        #[arg(long)]
        war_room_id: Option<String>,
        /// Optional round number within the war-room. Defaults to 1
        /// when war_room_id is set.
        #[arg(long)]
        war_room_round: Option<i64>,
        /// Optional model hint, surfaced in execution_logs.model so
        /// the receipt records which LLM the subagent is using.
        #[arg(long)]
        model: Option<String>,
    },
    /// Update a pending row with the subagent's response + status.
    Finish {
        /// log id from `create`.
        id: String,
        /// success | error
        #[arg(long, default_value = "success")]
        status: String,
        /// Response text. Inline or @<path>.
        #[arg(long)]
        response: Option<String>,
        /// Error message when status=error.
        #[arg(long)]
        error: Option<String>,
        /// Token counts (best-effort; pass when known).
        #[arg(long)]
        tokens_in: Option<i64>,
        #[arg(long)]
        tokens_out: Option<i64>,
        /// Cost in USD if known.
        #[arg(long)]
        cost_usd: Option<f64>,
        /// Override the duration. Default: now - created_at.
        #[arg(long)]
        duration_ms: Option<i64>,
    },
}

pub fn run(args: SubagentArgs, db_path: &PathBuf, opts: &Opts) -> Result<()> {
    match args.sub {
        SubagentSub::Log { sub } => match sub {
            LogSub::Create {
                persona,
                prompt,
                war_room_id,
                war_room_round,
                model,
            } => create(persona, prompt, war_room_id, war_room_round, model, db_path, opts),
            LogSub::Finish {
                id,
                status,
                response,
                error,
                tokens_in,
                tokens_out,
                cost_usd,
                duration_ms,
            } => finish(
                id,
                status,
                response,
                error,
                tokens_in,
                tokens_out,
                cost_usd,
                duration_ms,
                db_path,
                opts,
            ),
        },
    }
}

fn create(
    persona: String,
    prompt: String,
    war_room_id: Option<String>,
    war_room_round: Option<i64>,
    model: Option<String>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    // Codex R1+R2 fixes — slug-validate persona + UUID-validate war_room_id
    // before any DB write so an invalid value can't poison the agent-slug
    // analytics surface or land in war-room space that the desktop can't
    // operate on.
    validate_persona_slug(&persona)?;
    if let Some(ref wr) = war_room_id {
        validate_uuid("war-room-id", wr)?;
    }

    let prompt_text = truncate_for_log(&resolve_text_arg(&prompt).context("read prompt")?);
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // war_room_round defaults to 1 only when war_room_id is set.
    let round = match (&war_room_id, war_room_round) {
        (Some(_), Some(r)) => Some(r),
        (Some(_), None) => Some(1),
        (None, _) => None,
    };

    let conn = db::open_readwrite(db_path).context("open db")?;
    conn.execute(
        "INSERT INTO execution_logs
           (id, runtime, prompt, status, created_at,
            agent_slug, war_room_id, war_room_round, model,
            initiator_kind, client_surface, initiator_id)
         VALUES
           (?1, 'claude', ?2, 'pending', ?3,
            ?4, ?5, ?6, ?7,
            'agent:claude', 'subagent', 'claude-code')",
        params![id, prompt_text, now, persona, war_room_id, round, model],
    )
    .context("INSERT pending execution_log")?;

    if opts.human {
        emit_human(&format!("subagent log {} created (persona={})", id, persona));
    } else {
        emit_json(&json!({ "id": id, "status": "pending", "persona": persona }))?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finish(
    id: String,
    status: String,
    response: Option<String>,
    error: Option<String>,
    tokens_in: Option<i64>,
    tokens_out: Option<i64>,
    cost_usd: Option<f64>,
    duration_ms: Option<i64>,
    db_path: &PathBuf,
    opts: &Opts,
) -> Result<()> {
    if status != "success" && status != "error" {
        anyhow::bail!("--status must be 'success' or 'error', got: {}", status);
    }
    let response_text = response
        .map(|s| resolve_text_arg(&s))
        .transpose()
        .context("read response")?
        .map(|s| truncate_for_log(&s));
    let error_text = error.map(|s| truncate_for_log(&s));

    let conn = db::open_readwrite(db_path).context("open db")?;

    // If duration not supplied: now - created_at (clamped at 0).
    let computed_duration: Option<i64> = if duration_ms.is_none() {
        let created: Option<String> = conn
            .query_row(
                "SELECT created_at FROM execution_logs WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .ok();
        created
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|c| {
                let delta = chrono::Utc::now() - c.with_timezone(&chrono::Utc);
                delta.num_milliseconds().max(0)
            })
    } else {
        None
    };
    let final_duration = duration_ms.or(computed_duration);

    // Codex R1+R2 fix — idempotency guard. Pre-fix shape was
    // "UPDATE … WHERE id = ?" with COALESCE on every column. Two
    // racing/retried `finish` calls could write status='error' but
    // keep the prior `response` from the success call, leaving the
    // row internally inconsistent. AND a typo'd id could mutate a
    // non-subagent receipt — the WHERE didn't pin client_surface.
    //
    // New shape:
    //   WHERE id = ? AND client_surface = 'subagent' AND status = 'pending'
    // First finisher wins; second sees 0 rows updated and bails so
    // the caller knows about the race.
    let updated = conn.execute(
        "UPDATE execution_logs
            SET status = ?1,
                response = COALESCE(?2, response),
                error_message = COALESCE(?3, error_message),
                tokens_in = COALESCE(?4, tokens_in),
                tokens_out = COALESCE(?5, tokens_out),
                cost_usd_estimated = COALESCE(?6, cost_usd_estimated),
                duration_ms = COALESCE(?7, duration_ms)
          WHERE id = ?8
            AND client_surface = 'subagent'
            AND status = 'pending'",
        params![
            status,
            response_text,
            error_text,
            tokens_in,
            tokens_out,
            cost_usd,
            final_duration,
            id
        ],
    )
    .context("UPDATE execution_log")?;

    if updated == 0 {
        // Disambiguate: was it a missing id, a non-subagent row, or
        // an already-finished row? Helpful for the caller's retry
        // / cleanup logic.
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT COALESCE(client_surface, ''), status FROM execution_logs WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        match row {
            None => anyhow::bail!("no execution_logs row found for id={}", id),
            Some((surf, _)) if surf != "subagent" => {
                anyhow::bail!(
                    "execution_logs id={} is not a subagent row (client_surface={}); refusing to finish",
                    id, surf
                );
            }
            Some((_, existing)) => {
                anyhow::bail!(
                    "execution_logs id={} already finished (status={}); refusing to overwrite",
                    id, existing
                );
            }
        }
    }
    if opts.human {
        emit_human(&format!("subagent log {} -> {}", id, status));
    } else {
        emit_json(&json!({ "id": id, "status": status }))?;
    }
    Ok(())
}

/// Resolve `--prompt foo` (inline) or `--prompt @/path/to/file` (file
/// contents). Mirrors the convention in `team_shared::parse_json_arg`.
fn resolve_text_arg(input: &str) -> Result<String> {
    if let Some(rest) = input.strip_prefix('@') {
        let path = PathBuf::from(rest);
        let body = fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        Ok(body)
    } else {
        Ok(input.to_string())
    }
}
