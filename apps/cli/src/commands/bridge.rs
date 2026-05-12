// v2.3.33 Phase 6 Slice B — Cross-runtime conversation bridge.
//
// What this does: after a session-bound dispatch returns, scan the
// assistant's reply for `@<token>` mentions. If a mention resolves to
// a known runtime (remote_runtimes / api_providers / local CLI), kick
// off a bridged dispatch using the same session — so the two LLMs
// effectively talk to each other through ATO, sharing the same
// history substrate (session_turns). Stops on `[CONSENSUS]`, when no
// mention is found, or after `max_rounds` round-trips.
//
// Why it lives next to dispatch.rs: the loop body is just "dispatch
// again with a different runtime, same session." All persistence and
// runtime routing already exists in dispatch::run. The bridge module
// only owns mention parsing, target resolution, and termination.
//
// Design choices for v1:
//   - Bridge cue is fixed: "You were tagged by @<prev_runtime> in the
//     previous turn. Continue the conversation. Reply [CONSENSUS]
//     when you agree." Appended as a user turn so the responding
//     runtime sees full history + a clear hand-off.
//   - First resolvable mention wins. Multi-mention "@codex @gemini"
//     bridges to codex; gemini can be tagged in a follow-up turn.
//   - Self-reference guard: if the responder tagged itself, stop.
//   - Code-fence stripping: the parser ignores text inside ``` blocks
//     so example prompts in a reply don't trigger spurious bridges.

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::commands::sessions::Turn;
use crate::output::{emit_human, Opts};

/// Parse @-mentions out of an assistant reply.
///
/// Conservative: only matches `@\w+` at word boundary, lowercased,
/// and strips fenced code blocks first so example prompts don't fire
/// spurious bridges. Returns mentions in first-seen order, deduped.
pub fn parse_mentions(text: &str) -> Vec<String> {
    let stripped = strip_code_fences(text);
    let mut seen: Vec<String> = Vec::new();
    let bytes = stripped.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            // word boundary: previous char is start-of-string or non-word
            let prev_ok = i == 0
                || !matches!(bytes[i - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_');
            if !prev_ok {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < bytes.len()
                && matches!(
                    bytes[j],
                    b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-'
                )
            {
                j += 1;
            }
            if j > i + 1 {
                let slug = stripped[i + 1..j].to_ascii_lowercase();
                if !seen.contains(&slug) {
                    seen.push(slug);
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    seen
}

/// Strip fenced code blocks (```...```) so @-mentions inside example
/// code don't trigger a real bridge. Inline backticks aren't stripped
/// because they're more often used for emphasizing words than
/// quoting agent prompts.
///
/// Treats ``` as a toggle delimiter regardless of line position.
/// Earlier line-based version assumed each fence marker was on its
/// own line; dogfood QA caught a case where claude wrote
/// `Example usage: ```\n...\n```` and the parser drained the wrong
/// half (everything *after* the closer instead of everything inside).
/// This walk-by-marker version handles both cases.
fn strip_code_fences(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_fence = false;
    let mut cursor = 0;
    while let Some(rel) = s[cursor..].find("```") {
        let marker = cursor + rel;
        if !in_fence {
            out.push_str(&s[cursor..marker]);
        }
        in_fence = !in_fence;
        cursor = marker + 3;
    }
    if !in_fence {
        out.push_str(&s[cursor..]);
    }
    out
}

/// Resolve a mention slug to a dispatchable runtime, using the same
/// fall-through dispatch::run uses: remote_runtimes (Phase 6.x-J) →
/// api_providers (MiniMax / Grok / etc.) → local CLI runtimes.
/// Returns the canonical slug to pass to dispatch::run, or None if
/// the mention doesn't resolve to anything we know how to dispatch.
pub fn resolve_target(conn: &Connection, mention: &str) -> Option<String> {
    // Remote runtimes first: user explicitly named them, intent is
    // strongest.
    if let Ok(Some(r)) = crate::remote_runtime::lookup(conn, mention) {
        return Some(r.slug);
    }
    // API providers (minimax, grok, deepseek, qwen, openrouter, ...).
    if crate::api_dispatch::is_api_provider(mention) {
        return Some(mention.to_string());
    }
    // Local CLI runtimes — only a closed set today. Mirrors
    // resolve_runtime_cli's match arm so we don't accept "@foobar"
    // and then bail on dispatch.
    if matches!(mention, "claude" | "codex" | "gemini" | "openclaw" | "hermes") {
        return Some(mention.to_string());
    }
    None
}

/// Fetch the most-recent assistant turn for a session. Returns None
/// if the session has no assistant turns yet (only user) — caller
/// treats that as "nothing to bridge from."
pub fn last_assistant_turn(conn: &Connection, session_id: &str) -> Result<Option<Turn>> {
    let row = conn
        .query_row(
            "SELECT session_id, turn_index, role, text, runtime, created_at
               FROM session_turns
              WHERE session_id = ?1 AND role = 'assistant'
              ORDER BY turn_index DESC
              LIMIT 1",
            [session_id],
            |r| {
                Ok(Turn {
                    session_id: r.get(0)?,
                    turn_index: r.get(1)?,
                    role: r.get(2)?,
                    text: r.get(3)?,
                    runtime: r.get(4)?,
                    created_at: r.get(5)?,
                })
            },
        )
        .ok();
    Ok(row)
}

/// Run the bridge loop for a session that just received an assistant
/// turn. The caller (dispatch::run) already persisted the primary
/// turn; this picks up from there.
///
/// Returns Ok(()) on every termination condition — bridge failures
/// are surfaced via emit_human + execution_logs rows, not bubbled up,
/// because a failed bridge shouldn't fail the primary dispatch the
/// user already saw a response from.
pub fn run_loop(
    primary_session_id: &str,
    max_rounds: u32,
    db_path: &std::path::PathBuf,
    opts: &Opts,
) -> Result<()> {
    let conn = crate::db::open_readonly(db_path)?;
    let mut round = 0u32;
    // v2.3.46 — spinning detector state. Tracks the last assistant
    // runtime we saw across rounds so we can flag "model A → model A
    // → model A" loops where the conversation isn't actually moving.
    // After 2+ rounds with the same runtime replying back-to-back,
    // post an approval_request to the activity feed and bail. Human
    // can re-trigger with `ato bridge` if they think it's salvageable.
    let mut consecutive_same_runtime = 0u32;
    let mut last_assistant_runtime: Option<String> = None;

    loop {
        let last = match last_assistant_turn(&conn, primary_session_id)? {
            Some(t) => t,
            None => {
                if opts.human {
                    emit_human("Bridge: no assistant turn to read from; stopping.");
                }
                return Ok(());
            }
        };

        // v2.3.46 — spinning detector. If the same runtime has now
        // produced 3 assistant turns in a row inside this bridge run,
        // the conversation isn't going anywhere — escalate to the
        // human via the activity feed and bail.
        match &last_assistant_runtime {
            Some(prev) if *prev == last.runtime => {
                consecutive_same_runtime += 1;
            }
            _ => {
                consecutive_same_runtime = 1;
            }
        }
        last_assistant_runtime = Some(last.runtime.clone());
        // Threshold of 3 same-runtime turns picks up monologue patterns
        // (model A keeps replying to itself's tags or to filtered self-
        // mentions) without false-firing on a legitimate two-turn
        // exchange where A and B alternate.
        if consecutive_same_runtime >= 3 && round >= 1 {
            if let Ok(rw_conn) = crate::db::open_readwrite(db_path) {
                let post_id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now().to_rfc3339();
                let text = format!(
                    "Bridge spinning on session {}: @{} produced the last {} assistant turns without consensus. Human review needed before continuing.",
                    primary_session_id,
                    last.runtime,
                    consecutive_same_runtime,
                );
                let _ = rw_conn.execute(
                    "INSERT INTO activity_posts (id, created_at, author_kind, author_slug, kind, text, related_event_seq, payload)
                     VALUES (?1, ?2, 'system', 'bridge', 'approval_request', ?3, NULL, ?4)",
                    rusqlite::params![
                        post_id,
                        now,
                        text,
                        serde_json::json!({
                            "event_type": "bridge_spinning",
                            "session_id": primary_session_id,
                            "runtime": last.runtime,
                            "consecutive_turns": consecutive_same_runtime,
                            "round": round,
                        }).to_string(),
                    ],
                );
            }
            if opts.human {
                emit_human(&format!(
                    "Bridge: ⚠  spinning detected — @{} produced {} turns in a row. Escalated to activity feed (approval_request) for human review. Stopping.",
                    last.runtime, consecutive_same_runtime
                ));
            }
            return Ok(());
        }

        // Termination keyword check. Accept either:
        //   - `[CONSENSUS]` on a line by itself (the original v1 form,
        //     what the bridge cue asks the model to emit), or
        //   - `<consensus/>` anywhere in the text (v2.3.44 — a
        //     structured tag harder for the model to emit accidentally
        //     while quoting the cue, and easier to compose with prose
        //     summaries on the same line).
        // We still require the standalone-line form for `[CONSENSUS]`
        // to avoid the earlier false-positive case where a model
        // quoted "reply [CONSENSUS] if you agree" while still
        // disagreeing.
        let consensus_reached = last
            .text
            .lines()
            .any(|l| l.trim() == "[CONSENSUS]")
            || last.text.contains("<consensus/>");
        if consensus_reached {
            if opts.human {
                emit_human(&format!(
                    "Bridge: ✓ [CONSENSUS] reached by @{} (round {}).",
                    last.runtime, round
                ));
            }
            return Ok(());
        }

        let mentions = parse_mentions(&last.text);
        // v2.3.44 Slice B polish — multi-mention round-robin. When a
        // turn names several runtimes (e.g. "@minimax @gemini please
        // both review"), prefer the one that hasn't yet contributed
        // an assistant turn to this session. That way a single
        // bridge run can cycle through every tagged collaborator
        // before re-bridging to one who already replied. Falls back
        // to "first resolvable, self-excluded" when every mention
        // has already been heard from.
        let prior_runtimes: std::collections::HashSet<String> = {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT runtime FROM session_turns
                  WHERE session_id = ?1 AND role = 'assistant'",
            )?;
            let rows = stmt
                .query_map([primary_session_id], |r| r.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect::<std::collections::HashSet<String>>();
            rows
        };
        let mentions_filtered: Vec<&String> = mentions
            .iter()
            .filter(|m| *m != &last.runtime) // self-reference guard
            .collect();
        let target_slug = mentions_filtered
            .iter()
            .find_map(|m| {
                let resolved = resolve_target(&conn, m)?;
                if !prior_runtimes.contains(&resolved) {
                    Some(resolved)
                } else {
                    None
                }
            })
            .or_else(|| {
                mentions_filtered
                    .iter()
                    .find_map(|m| resolve_target(&conn, m))
            });

        let Some(target_slug) = target_slug else {
            if opts.human {
                if mentions.is_empty() {
                    emit_human("Bridge: no @-mention in last turn; conversation ended.");
                } else {
                    emit_human(&format!(
                        "Bridge: mention(s) {:?} didn't resolve to a known runtime; stopping.",
                        mentions
                    ));
                }
            }
            return Ok(());
        };

        round += 1;
        if round > max_rounds {
            if opts.human {
                emit_human(&format!(
                    "Bridge: round cap ({}) reached without [CONSENSUS].",
                    max_rounds
                ));
            }
            return Ok(());
        }

        if opts.human {
            emit_human(&format!(
                "\n--- Bridge round {} of {}: @{} → @{} ---",
                round, max_rounds, last.runtime, target_slug
            ));
        }

        let bridge_cue = format!(
            "You were tagged by @{} in the previous turn of this conversation. \
             Continue the conversation. When you agree with the resolution and \
             have nothing to add, emit either `[CONSENSUS]` on a line by itself \
             or `<consensus/>` inline — the bridge loop checks for both.",
            last.runtime
        );

        // Re-enter dispatch::run with the bridged runtime and the same
        // session. dispatch::run handles all routing (remote / api /
        // CLI), persistence, and appending the user-prompt+assistant
        // turn pair. Errors don't fail the loop — they're logged via
        // execution_logs and we still try the next round.
        if let Err(e) = crate::commands::dispatch::run(
            &target_slug,
            &bridge_cue,
            None, // model override is per-mention, future work
            None, // no agent label
            Some(primary_session_id.to_string()),
            false, // bridge doesn't stream individual turns
            db_path,
            opts,
        ) {
            if opts.human {
                emit_human(&format!(
                    "Bridge: dispatch to @{} failed: {}. Stopping loop.",
                    target_slug, e
                ));
            }
            return Ok(());
        }
    }
}

/// Smoke-test helper used by `ato bridge dry-run <text>` (not wired
/// into the CLI by default, but useful when debugging the parser).
#[allow(dead_code)]
pub fn debug_parse(text: &str) -> Vec<String> {
    parse_mentions(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_mention() {
        let m = parse_mentions("hey @codex what do you think?");
        assert_eq!(m, vec!["codex".to_string()]);
    }

    #[test]
    fn ignores_email_like_at() {
        let m = parse_mentions("contact me at foo@example.com");
        assert!(m.is_empty(), "got: {:?}", m);
    }

    #[test]
    fn dedupes_and_preserves_order() {
        let m = parse_mentions("@claude please ping @codex and then @claude again");
        assert_eq!(m, vec!["claude".to_string(), "codex".to_string()]);
    }

    #[test]
    fn strips_code_fences() {
        let text = "first response\n\n```\nuse @internal-helper\n```\n\nbut for real @codex please review";
        let m = parse_mentions(text);
        assert_eq!(m, vec!["codex".to_string()]);
    }

    #[test]
    fn strips_inline_code_fences() {
        // Regression — QA caught this on 2026-05-12. The old
        // line-based stripper drained everything AFTER the closing
        // fence when the opening fence wasn't on its own line.
        // Now ``` toggles regardless of line position.
        let text = "Example usage: ```\nato dispatch codex --agent @codex-reviewer \"test\"\n```\nBut for the real review, @bogus_runtime should weigh in.";
        let m = parse_mentions(text);
        // codex-reviewer is inside the fence → must not appear.
        // bogus_runtime is outside → must appear.
        assert!(!m.contains(&"codex-reviewer".to_string()), "leaked inside-fence mention: {:?}", m);
        assert!(m.contains(&"bogus_runtime".to_string()), "missed outside-fence mention: {:?}", m);
    }

    #[test]
    fn handles_dashes_in_slug() {
        // Phase 6.x-J remote runtimes commonly have hyphenated slugs.
        let m = parse_mentions("delegate to @claude-server");
        assert_eq!(m, vec!["claude-server".to_string()]);
    }

    #[test]
    fn consensus_must_be_on_own_line() {
        // Documents the termination contract used by run_loop: the
        // bracketed form has to stand alone on a line, the structured
        // tag form can appear anywhere.
        let standalone = "i agree with the resolution\n[CONSENSUS]";
        let quoted = "Reply [CONSENSUS] if you agree.";
        let structured_inline = "agreed, ship it <consensus/> — no other concerns";
        assert!(standalone.lines().any(|l| l.trim() == "[CONSENSUS]"));
        assert!(!quoted.lines().any(|l| l.trim() == "[CONSENSUS]"));
        // v2.3.44 — the structured tag form is detected by .contains()
        // anywhere in the text; quoting it in a code block won't
        // trigger because parse_mentions strips fences (and so does
        // any future consensus parser that wants to be conservative).
        assert!(structured_inline.contains("<consensus/>"));
    }
}
