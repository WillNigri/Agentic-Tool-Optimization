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
///
/// Codex R2 fix — slice on a UTF-8 char boundary, not a raw byte
/// offset. Naive `&s[..MAX]` panics if MAX lands inside a multi-byte
/// codepoint (any non-ASCII near the 64KB mark — emoji in a prompt
/// is the realistic trigger). Walk char_indices and stop at the
/// last boundary ≤ MAX.
const MAX_LOG_BYTES: usize = 64 * 1024;
fn truncate_for_log(s: &str) -> String {
    if s.len() <= MAX_LOG_BYTES {
        return s.to_string();
    }
    let mut last_ok = 0usize;
    for (i, _) in s.char_indices() {
        if i > MAX_LOG_BYTES {
            break;
        }
        last_ok = i;
    }
    format!("{}…[truncated]", &s[..last_ok])
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

/// Codex R2 fix — canonical auth_mode vocabulary mirrors what
/// dispatch.rs writes and what the cost-split UI buckets on.
const AUTH_MODE_VOCAB: &[&str] = &["subscription", "api_key", "local"];

/// Codex R2 fix — canonical billing_surface vocabulary mirrors
/// schema.rs:529 + active_runs.rs:224-228 + sources.rs:41. Anything
/// outside this set silently shows up as "Other" in the cost-split
/// UI; reject at the CLI boundary so analytics stays honest.
const BILLING_SURFACE_VOCAB: &[&str] = &[
    "claude_code_subscription",
    "anthropic_api",
    "codex_cli_subscription",
    "openai_api",
    "gemini_cli_subscription",
    "gemini_api",
    "ollama_local",
];

fn validate_one_of(label: &str, value: &str, vocab: &[&str]) -> Result<()> {
    if !vocab.contains(&value) {
        anyhow::bail!(
            "--{} must be one of [{}], got: {:?}",
            label,
            vocab.join(", "),
            value
        );
    }
    Ok(())
}

// #71 follow-up — git HEAD provenance.
//
// Codex R1 fix — the original implementation here spawned `git
// rev-parse HEAD` synchronously and blocked the CLI. A wedged git
// (NFS hang, fsck-in-flight, hung filesystem) would hang `ato
// subagent log create` indefinitely.
//
// dispatch.rs already has the right shape: a worker thread + a 2s
// recv_timeout. Reuse it directly so the two surfaces share the same
// bound. `apps/cli/src/commands/dispatch.rs` exposes
// `capture_git_head` as `pub(crate)` precisely so other commands can
// call it.

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
        /// #71 follow-up — how the subagent dispatch authenticated.
        /// Defaults to "subscription" matching dispatch.rs's vocab.
        /// Accepted: subscription | api_key | local.
        ///
        /// Codex R2 fix — pre-fix shape was free-form; downstream
        /// readers assume canonical enums for the auth-mode bucket
        /// in cost-split analytics. Validated against AUTH_MODE_VOCAB
        /// before INSERT.
        #[arg(long, default_value = "subscription")]
        auth_mode: String,
        /// #71 follow-up — billing-side classification for cost-split
        /// surfaces. Defaults to "claude_code_subscription". Accepted
        /// per schema.rs:529 vocabulary: claude_code_subscription |
        /// anthropic_api | codex_cli_subscription | openai_api |
        /// gemini_cli_subscription | gemini_api | ollama_local.
        ///
        /// Codex R2 fix — pre-fix shape was free-form and the help
        /// text documented "ollama" (canonical is "ollama_local").
        /// Validated against BILLING_SURFACE_VOCAB before INSERT.
        #[arg(long, default_value = "claude_code_subscription")]
        billing_surface: String,
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
                auth_mode,
                billing_surface,
            } => create(
                persona, prompt, war_room_id, war_room_round, model,
                auth_mode, billing_surface, db_path, opts,
            ),
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

#[allow(clippy::too_many_arguments)]
fn create(
    persona: String,
    prompt: String,
    war_room_id: Option<String>,
    war_room_round: Option<i64>,
    model: Option<String>,
    auth_mode: String,
    billing_surface: String,
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
    // Codex R2 — vocab guard so analytics buckets stay honest.
    validate_one_of("auth-mode", &auth_mode, AUTH_MODE_VOCAB)?;
    validate_one_of("billing-surface", &billing_surface, BILLING_SURFACE_VOCAB)?;

    let prompt_text = truncate_for_log(&resolve_text_arg(&prompt).context("read prompt")?);
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // war_room_round defaults to 1 only when war_room_id is set.
    let round = match (&war_room_id, war_room_round) {
        (Some(_), Some(r)) => Some(r),
        (Some(_), None) => Some(1),
        (None, _) => None,
    };

    // #71 follow-up — bounded git_commit_sha provenance via the
    // shared dispatch::capture_git_head helper. 2s timeout; failures
    // (not a repo, git missing, wedged) leave the column NULL.
    let git_commit_sha = crate::commands::dispatch::capture_git_head(None);

    let conn = db::open_readwrite(db_path).context("open db")?;
    let machine_id_val = db::machine_id(&conn);
    conn.execute(
        "INSERT INTO execution_logs
           (id, runtime, prompt, status, created_at,
            agent_slug, war_room_id, war_room_round, model,
            initiator_kind, client_surface, initiator_id,
            auth_mode, billing_surface, git_commit_sha,
            member_id, machine_id)
         VALUES
           (?1, 'claude', ?2, 'pending', ?3,
            ?4, ?5, ?6, ?7,
            'agent:claude', 'subagent', 'claude-code',
            ?8, ?9, ?10,
            ?11, ?12)",
        params![
            id, prompt_text, now, persona, war_room_id, round, model,
            auth_mode, billing_surface, git_commit_sha,
            crate::attribution::detect_member_id(), machine_id_val
        ],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string_unchanged() {
        let s = "hello world";
        assert_eq!(truncate_for_log(s), s);
    }

    #[test]
    fn truncate_long_ascii_string() {
        let s = "x".repeat(MAX_LOG_BYTES + 100);
        let out = truncate_for_log(&s);
        assert!(out.ends_with("…[truncated]"));
        assert!(out.len() < s.len());
    }

    /// Codex R2 fix — regression guard. Pre-fix shape used
    /// `&s[..MAX]` which panics on a multibyte UTF-8 boundary. Build
    /// a string of all-ASCII filler followed by a 4-byte emoji
    /// (`💥`, U+1F4A5) straddling the byte cap; truncation must
    /// return a clean string without panicking.
    #[test]
    fn truncate_multibyte_boundary_no_panic() {
        // (MAX_LOG_BYTES - 2) chars of ASCII puts the emoji's start
        // at byte (MAX - 2); its 4-byte UTF-8 encoding extends past
        // MAX. A raw &s[..MAX] would slice mid-codepoint and panic.
        let prefix = "a".repeat(MAX_LOG_BYTES - 2);
        let s = format!("{}💥💥💥", prefix);
        let out = truncate_for_log(&s);
        assert!(out.ends_with("…[truncated]"));
        // Truncated body must remain valid UTF-8 (the assertion is
        // implicit — String requires it; the panic would have fired
        // inside truncate_for_log if the boundary was wrong).
    }
}
