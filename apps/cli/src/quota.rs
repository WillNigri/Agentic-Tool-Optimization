// v2.3.27 Phase 6.x — Runtime quota visibility.
//
// Rate-limit info is only visible at the moment a dispatch fails.
// ATO already sees those errors (they flow through ato dispatch's
// stderr / error_message column) but didn't persist or surface them
// proactively until this commit. Triggered when codex hit its quota
// mid-review of v2.3.19 — the error message contained a reset time,
// but ATO discarded it and the user had to remember it manually.
//
// What this module does:
//   - parse_reset_time: extract a future timestamp from common
//     rate-limit error patterns. v1 covers codex's "try again at
//     May 13th, 2026 12:10 PM" shape. Other runtimes get added as
//     we see real examples.
//   - upsert: persist the parsed reset_at into runtime_quotas.
//   - lookup: return any stored reset_at that's still in the future.
//   - format_human: render a reset_at for `ato runtimes status`.

use anyhow::Result;
use chrono::{DateTime, Datelike, Utc};
use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct QuotaRow {
    pub runtime: String,
    pub resets_at: String, // RFC3339
    pub source: String,
    pub captured_at: String,
}

/// Try to extract a reset timestamp from an error message. Returns
/// (resets_at_rfc3339, source_label) on success. Patterns covered:
///   - codex: "try again at May 13th, 2026 12:10 PM"
///   - openai-style: "Please try again in 12.5s" (relative — added
///     to now)
///   - claude CLI: "Rate limit reached. Try again at Jul 10, 2026 12:00 AM PT"
///   - gemini CLI: "Quota exceeded. Quota will reset at Jul 10 12:00 UTC"
///   - anthropic API: "rate_limit_error" + body.error.message has reset time
///   - minimax: status_code 1027 body — caller passes message text; we
///     pick up the `available_after` or `reset_at` field if present
///
/// Conservative: only matches strict patterns to avoid false
/// positives. If we don't recognize the shape, we return None and
/// the caller treats it as a non-quota error.
///
/// v2.15.2 (war_room 78617E68 codex finding): extended from codex+openai
/// to all 6 patterns we've seen in real error logs. Per codex's verdict,
/// classifier patterns stay in quota.rs (co-located with the cache write)
/// instead of moving to ato-retry-policy — subscription exhaustion is
/// durable state, distinct from transient retry's seconds-window scope.
pub fn parse_reset_time(error_message: &str) -> Option<(String, &'static str)> {
    // Codex: "try again at May 13th, 2026 12:10 PM"
    // Anchor on "try again at" (case-insensitive) to avoid false
    // positives on prose that happens to contain a date. Claude CLI
    // uses "Try again at" (capital T), so we lowercase for matching
    // then slice the original to preserve the date span's casing.
    let lower_message = error_message.to_ascii_lowercase();
    if let Some(idx) = lower_message.find("try again at ") {
        let after = &error_message[idx + "try again at ".len()..];
        // Match up to the first period or newline (the error message
        // usually ends with "." or wraps).
        let end = after
            .find(['.', '\n', '\r'])
            .unwrap_or(after.len())
            .min(after.len());
        let span = after[..end].trim();
        if let Some(ts) = parse_codex_date(span) {
            return Some((ts.to_rfc3339(), "codex_error_text"));
        }
        // Claude CLI shape: "Try again at Jul 10, 2026 12:00 AM PT"
        // (slightly different spacing — no ordinal, may have TZ suffix).
        if let Some(ts) = parse_claude_date(span) {
            return Some((ts.to_rfc3339(), "claude_error_text"));
        }
    }

    // Gemini CLI: "Quota exceeded. Quota will reset at <date>" — the
    // anchor differs from codex/claude.
    if let Some(idx) = error_message.find("Quota will reset at ") {
        let after = &error_message[idx + "Quota will reset at ".len()..];
        let end = after
            .find(['.', '\n', '\r'])
            .unwrap_or(after.len())
            .min(after.len());
        let span = after[..end].trim();
        if let Some(ts) = parse_gemini_date(span) {
            return Some((ts.to_rfc3339(), "gemini_error_text"));
        }
    }

    // MiniMax body: status_code 1027 typically includes "available_after"
    // or "next_available_at" as a UTC seconds-since-epoch number. Look
    // for the labels.
    if error_message.contains("1027") || error_message.contains("quota") {
        for needle in [
            "available_after\":",
            "next_available_at\":",
            "reset_at\":",
            "available_after=",
        ] {
            if let Some(idx) = error_message.find(needle) {
                let after = &error_message[idx + needle.len()..];
                let trimmed = after.trim_start_matches([' ', '"']);
                let end = trimmed
                    .find(['"', ',', '}', '\n'])
                    .unwrap_or(trimmed.len());
                let span = trimmed[..end].trim();
                // Two possible shapes: epoch seconds OR RFC3339.
                if let Ok(epoch) = span.parse::<i64>() {
                    if let Some(when) = DateTime::<Utc>::from_timestamp(epoch, 0) {
                        if when > Utc::now() && when < Utc::now() + chrono::Duration::days(180) {
                            return Some((when.to_rfc3339(), "minimax_body_field"));
                        }
                    }
                }
                if let Ok(when) = DateTime::parse_from_rfc3339(span) {
                    let utc = when.with_timezone(&Utc);
                    if utc > Utc::now() {
                        return Some((utc.to_rfc3339(), "minimax_body_field"));
                    }
                }
            }
        }
    }

    // Anthropic API: HTTP 429 with retry-after header → caller has
    // already passed the header value through; we accept either
    // RFC3339 or relative seconds in the message.
    if error_message.contains("anthropic-ratelimit-requests-reset")
        || error_message.contains("retry-after")
    {
        // Look for RFC3339 (anthropic emits this in the
        // anthropic-ratelimit-*-reset headers).
        for token in error_message.split_whitespace() {
            let candidate = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != ':' && c != '-' && c != '+' && c != '.');
            if let Ok(when) = DateTime::parse_from_rfc3339(candidate) {
                let utc = when.with_timezone(&Utc);
                if utc > Utc::now() && utc < Utc::now() + chrono::Duration::days(180) {
                    return Some((utc.to_rfc3339(), "anthropic_header"));
                }
            }
        }
    }

    // OpenAI-style: "Please try again in 12.5s" or "retry after 60s"
    if let Some(secs) = parse_relative_seconds(error_message) {
        let when = Utc::now() + chrono::Duration::seconds(secs as i64);
        return Some((when.to_rfc3339(), "relative_seconds"));
    }
    None
}

/// Claude CLI: "Try again at Jul 10, 2026 12:00 AM PT" — no ordinal
/// suffix, may have a trailing TZ abbreviation. We strip the TZ
/// abbreviation (PT, ET, UTC) before parsing; UTC is the assumed
/// default, same conservative posture as parse_codex_date.
fn parse_claude_date(span: &str) -> Option<DateTime<Utc>> {
    let cleaned: String = strip_tz_suffix(span);
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%b %d, %Y %I:%M %p") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%B %d, %Y %I:%M %p") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    None
}

/// Gemini CLI: "Quota will reset at Jul 10 12:00 UTC" — may or may not
/// have a year; assume the current year if missing and the parsed
/// date hasn't passed in that year.
fn parse_gemini_date(span: &str) -> Option<DateTime<Utc>> {
    let cleaned: String = strip_tz_suffix(span);
    // Try with year first: "Jul 10, 2026 12:00"
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%b %d, %Y %H:%M") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&cleaned, "%b %d, %Y %I:%M %p") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    // Try without year — assume current year.
    let now = Utc::now();
    let with_year = format!("{} {}", cleaned, now.year());
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&with_year, "%b %d %H:%M %Y") {
        let candidate = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
        if candidate > now {
            return Some(candidate);
        }
        // Already past in current year — assume next year.
        let with_next_year = format!("{} {}", cleaned, now.year() + 1);
        if let Ok(naive_next) =
            chrono::NaiveDateTime::parse_from_str(&with_next_year, "%b %d %H:%M %Y")
        {
            return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive_next, Utc));
        }
    }
    None
}

/// Strip trailing TZ abbreviations from a date span. We don't try to
/// honor the TZ; per the codex_date conservative posture, the
/// worst-case error is unblocking-too-early (next dispatch hits the
/// limit again, no real damage).
fn strip_tz_suffix(span: &str) -> String {
    let mut s = span.to_string();
    for suffix in [" UTC", " GMT", " PT", " ET", " PST", " EST", " PDT", " EDT"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.to_string();
            break;
        }
    }
    s
}

/// Parse "May 13th, 2026 12:10 PM" into a UTC DateTime.
/// chrono can't natively eat the ordinal suffix ("13th"), so strip
/// it before handing to the parser.
///
/// MiniMax round-1 6.x flagged that the input string has no timezone
/// info — assuming UTC could be 7-8 hours off if codex emits PT or
/// some other zone. We're keeping UTC as the default because (a)
/// codex's API server appears to use UTC for backend timestamps,
/// (b) the worst-case error is unblocking-too-early (subsequent
/// dispatch would just hit the limit again, no real damage), and
/// (c) we have no robust way to detect the timezone from the
/// surrounding text. Worth revisiting if a user reports the
/// pre-flight short-circuit firing past the actual reset.
fn parse_codex_date(span: &str) -> Option<DateTime<Utc>> {
    // Strip ordinal suffixes: 1st 2nd 3rd Nth
    let cleaned = strip_ordinal_suffixes(span);
    // Try "%B %d, %Y %I:%M %p" → "May 13, 2026 12:10 PM"
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(
        &cleaned,
        "%B %d, %Y %I:%M %p",
    ) {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    None
}

fn strip_ordinal_suffixes(s: &str) -> String {
    // Replace "Nst|Nnd|Nrd|Nth" with "N" where N is one or two
    // digits. Cheap state machine; no regex dep.
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        out.push(b as char);
        i += 1;
        // After a digit, peek for the 2-char suffix.
        if b.is_ascii_digit() && i + 1 < bytes.len() {
            let suffix = &bytes[i..i + 2];
            if matches!(
                suffix,
                b"st" | b"nd" | b"rd" | b"th" | b"ST" | b"ND" | b"RD" | b"TH"
            ) {
                i += 2;
            }
        }
    }
    out
}

fn parse_relative_seconds(error_message: &str) -> Option<u64> {
    // Look for "in X seconds" / "after X seconds" / "in X.Ys".
    let lower = error_message.to_ascii_lowercase();
    for needle in ["try again in ", "retry after ", "retry in "] {
        if let Some(idx) = lower.find(needle) {
            let after = &lower[idx + needle.len()..];
            // Take up to the first space or non-numeric character
            // beyond the number itself.
            let mut end = 0;
            let mut saw_digit = false;
            for (i, c) in after.char_indices() {
                if c.is_ascii_digit() || c == '.' {
                    saw_digit = true;
                    end = i + c.len_utf8();
                } else if saw_digit {
                    break;
                } else if !c.is_ascii_whitespace() {
                    break;
                }
            }
            if end == 0 {
                continue;
            }
            let span = &after[..end];
            if let Ok(secs_f) = span.parse::<f64>() {
                if secs_f > 0.0 && secs_f < 86_400.0 * 7.0 {
                    return Some(secs_f.ceil() as u64);
                }
            }
        }
    }
    None
}

/// Upsert a runtime-quota row. Idempotent on the runtime PK.
pub fn upsert(
    db_path: &Path,
    runtime: &str,
    resets_at: &str,
    source: &str,
) -> Result<()> {
    let conn = Connection::open(db_path)?;
    let captured_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO runtime_quotas (runtime, resets_at, source, captured_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(runtime) DO UPDATE SET
             resets_at = excluded.resets_at,
             source = excluded.source,
             captured_at = excluded.captured_at",
        rusqlite::params![runtime, resets_at, source, captured_at],
    )?;
    Ok(())
}

/// Lookup a runtime's quota. Returns Some(resets_at_rfc3339) iff
/// the stored timestamp is still in the future. Past timestamps are
/// silently ignored (and on a successful dispatch we'd clear them).
pub fn lookup_future(db_path: &Path, runtime: &str) -> Result<Option<String>> {
    let conn = Connection::open(db_path)?;
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='runtime_quotas'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(None);
    }
    let row: Option<String> = conn
        .query_row(
            "SELECT resets_at FROM runtime_quotas WHERE runtime = ?1",
            [runtime],
            |r| r.get(0),
        )
        .ok();
    if let Some(ts) = row {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(&ts) {
            if parsed > Utc::now() {
                return Ok(Some(ts));
            }
        }
    }
    Ok(None)
}

/// v2.15.2 — user preference for what to do when a runtime hits its
/// subscription/quota limit during a dispatch. Per war_room 78617E68
/// codex finding: the "ask user via modal" path is desktop-only and
/// optional; loops/CLI must always degrade to a persisted setting,
/// never blocking on UI. New users start with `AskOrDefault`, which
/// behaves as `StopAndNotify` for non-interactive contexts and emits
/// the `dispatch_exhausted` event for the desktop to react to with a
/// chooser modal (writing the user's choice back so future runs use
/// that policy directly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExhaustionPolicy {
    /// First-event default. CLI/loops degrade to StopAndNotify;
    /// desktop pops the chooser modal on first dispatch_exhausted
    /// event AFTER which the user's choice is persisted.
    AskOrDefault,
    /// Fail the dispatch with the existing pre-flight bail!() shape.
    StopAndNotify,
    /// Walk user's preferred fallback order; if a non-exhausted
    /// runtime is available, retarget the dispatch to it.
    FallbackChain,
    /// Schedule a pause-and-wake at reset_at. v2.15.3 will ship the
    /// actual scheduler; v2.15.2 surfaces this option as deferred
    /// and degrades to StopAndNotify with an explanatory message.
    PauseAndWake,
}

impl ExhaustionPolicy {
    fn as_str(&self) -> &'static str {
        match self {
            ExhaustionPolicy::AskOrDefault => "ask",
            ExhaustionPolicy::StopAndNotify => "stop-and-notify",
            ExhaustionPolicy::FallbackChain => "fallback-chain",
            ExhaustionPolicy::PauseAndWake => "pause-and-wake",
        }
    }
    fn parse(s: &str) -> ExhaustionPolicy {
        match s {
            "stop-and-notify" => ExhaustionPolicy::StopAndNotify,
            "fallback-chain" => ExhaustionPolicy::FallbackChain,
            "pause-and-wake" => ExhaustionPolicy::PauseAndWake,
            _ => ExhaustionPolicy::AskOrDefault,
        }
    }
}

/// Read the current exhaustion policy from `settings`. Returns
/// `AskOrDefault` if unset.
pub fn read_exhaustion_policy(conn: &Connection) -> Result<ExhaustionPolicy> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'exhaustion_policy'",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(v.map(|s| ExhaustionPolicy::parse(&s))
        .unwrap_or(ExhaustionPolicy::AskOrDefault))
}

/// Persist the user's exhaustion-policy choice.
pub fn write_exhaustion_policy(conn: &Connection, p: ExhaustionPolicy) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('exhaustion_policy', ?1)",
        [p.as_str()],
    )?;
    Ok(())
}

/// Read the user's preferred fallback-chain order. Returns an empty
/// vec if unset.
pub fn read_fallback_order(conn: &Connection) -> Result<Vec<String>> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'exhaustion_fallback_order'",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(v.map(|s| {
        s.split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect()
    })
    .unwrap_or_default())
}

/// Persist the user's preferred fallback-chain order (comma-separated).
pub fn write_fallback_order(conn: &Connection, slugs: &[&str]) -> Result<()> {
    let joined = slugs.join(",");
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('exhaustion_fallback_order', ?1)",
        [joined],
    )?;
    Ok(())
}

/// v2.15.2 fallback-chain algorithm per war_room 78617E68:
/// > "walk user order, skip candidates with a future runtime_quotas
/// >  row, pre-flight-check the retargeted runtime too, then dispatch;
/// >  if that runtime exhausts immediately, capture its reset and
/// >  continue the chain until exhausted."
///
/// Returns Some(runtime_slug) if a non-exhausted candidate exists, or
/// None if every entry in the fallback order is currently rate-limited.
/// Skips `target_runtime` itself even if it's in the order list — the
/// caller targeted that runtime first and we already know it's
/// exhausted (otherwise the pre-flight gate wouldn't have fired).
pub fn select_fallback_runtime(
    conn: &Connection,
    target_runtime: &str,
) -> Result<Option<String>> {
    let order = read_fallback_order(conn)?;
    for slug in order {
        if slug == target_runtime {
            continue;
        }
        // Use the same future-only lookup the pre-flight gate uses.
        let row: Option<String> = conn
            .query_row(
                "SELECT resets_at FROM runtime_quotas WHERE runtime = ?1",
                [&slug],
                |r| r.get(0),
            )
            .ok();
        let exhausted = if let Some(ts) = row {
            if let Ok(parsed) = DateTime::parse_from_rfc3339(&ts) {
                parsed > Utc::now()
            } else {
                false
            }
        } else {
            false
        };
        if !exhausted {
            return Ok(Some(slug));
        }
    }
    Ok(None)
}

/// Clear a quota row — call after a successful dispatch, since
/// the previous limit obviously isn't blocking anymore.
pub fn clear(db_path: &Path, runtime: &str) -> Result<()> {
    let conn = Connection::open(db_path)?;
    conn.execute("DELETE FROM runtime_quotas WHERE runtime = ?1", [runtime])?;
    Ok(())
}

/// List all currently-recorded quotas (past + future).
pub fn list_all(db_path: &Path) -> Result<Vec<QuotaRow>> {
    let conn = Connection::open(db_path)?;
    let table_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='runtime_quotas'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT runtime, resets_at, source, captured_at FROM runtime_quotas
          ORDER BY resets_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(QuotaRow {
            runtime: r.get(0)?,
            resets_at: r.get(1)?,
            source: r.get(2)?,
            captured_at: r.get(3)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_pattern() {
        let err = "ERROR: You've hit your usage limit. Upgrade to Plus to continue using Codex, or try again at May 13th, 2026 12:10 PM.";
        let result = parse_reset_time(err);
        assert!(result.is_some());
        let (ts, source) = result.unwrap();
        assert_eq!(source, "codex_error_text");
        assert!(ts.starts_with("2026-05-13T"));
    }

    #[test]
    fn parses_relative_seconds() {
        let err = "Rate limit exceeded. Please try again in 60 seconds.";
        let result = parse_reset_time(err);
        assert!(result.is_some());
    }

    #[test]
    fn rejects_unrelated_text() {
        let err = "Connection refused";
        assert!(parse_reset_time(err).is_none());
    }

    #[test]
    fn settings_round_trip_for_exhaustion_policy() {
        let conn = open_in_mem();
        // Default returns when unset.
        let p = read_exhaustion_policy(&conn).unwrap();
        assert_eq!(p, ExhaustionPolicy::AskOrDefault);
        // Set + read back.
        write_exhaustion_policy(&conn, ExhaustionPolicy::FallbackChain).unwrap();
        assert_eq!(read_exhaustion_policy(&conn).unwrap(), ExhaustionPolicy::FallbackChain);
        // Update.
        write_exhaustion_policy(&conn, ExhaustionPolicy::StopAndNotify).unwrap();
        assert_eq!(read_exhaustion_policy(&conn).unwrap(), ExhaustionPolicy::StopAndNotify);
    }

    #[test]
    fn fallback_order_round_trip() {
        let conn = open_in_mem();
        assert!(read_fallback_order(&conn).unwrap().is_empty());
        write_fallback_order(&conn, &["codex", "openai", "claude"]).unwrap();
        let read = read_fallback_order(&conn).unwrap();
        assert_eq!(read, vec!["codex".to_string(), "openai".to_string(), "claude".to_string()]);
    }

    #[test]
    fn select_fallback_skips_exhausted_runtimes() {
        let conn = open_in_mem();
        // Order: codex → claude → gemini.
        write_fallback_order(&conn, &["codex", "claude", "gemini"]).unwrap();
        // Mark codex as exhausted until 1h from now.
        let future = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        conn.execute(
            "INSERT INTO runtime_quotas (runtime, resets_at, source, captured_at)
             VALUES (?1, ?2, 'test', ?3)",
            rusqlite::params!["codex", future, Utc::now().to_rfc3339()],
        )
        .unwrap();
        // Originally targeted runtime was codex; chosen should skip
        // codex (the target itself) AND any other exhausted runtime
        // in the order list.
        let chosen = select_fallback_runtime(&conn, "codex").unwrap();
        assert_eq!(chosen, Some("claude".to_string()));
    }

    #[test]
    fn select_fallback_returns_none_when_all_exhausted() {
        let conn = open_in_mem();
        write_fallback_order(&conn, &["codex", "claude"]).unwrap();
        let future = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        for r in ["codex", "claude"] {
            conn.execute(
                "INSERT INTO runtime_quotas (runtime, resets_at, source, captured_at)
                 VALUES (?1, ?2, 'test', ?3)",
                rusqlite::params![r, &future, Utc::now().to_rfc3339()],
            )
            .unwrap();
        }
        assert!(select_fallback_runtime(&conn, "codex").unwrap().is_none());
    }

    fn open_in_mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE runtime_quotas (runtime TEXT PRIMARY KEY, resets_at TEXT NOT NULL, source TEXT NOT NULL, captured_at TEXT NOT NULL);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn strips_ordinals() {
        assert_eq!(strip_ordinal_suffixes("May 13th, 2026"), "May 13, 2026");
        assert_eq!(strip_ordinal_suffixes("Jan 1st"), "Jan 1");
        assert_eq!(strip_ordinal_suffixes("3rd quarter"), "3 quarter");
    }

    // v2.15.2 — new runtime patterns per war_room 78617E68.

    #[test]
    fn parses_claude_pattern_with_pt_tz() {
        // claude CLI rate-limit message shape.
        let err = "Rate limit reached. Try again at Jul 10, 2026 12:00 AM PT";
        let result = parse_reset_time(err);
        assert!(result.is_some(), "claude pattern must parse");
        let (ts, source) = result.unwrap();
        assert_eq!(source, "claude_error_text");
        assert!(ts.starts_with("2026-07-10T"));
    }

    #[test]
    fn parses_gemini_pattern_with_year_and_utc() {
        let err = "Quota exceeded. Quota will reset at Jul 10, 2026 12:00 UTC.";
        let result = parse_reset_time(err);
        assert!(result.is_some(), "gemini pattern must parse");
        let (_, source) = result.unwrap();
        assert_eq!(source, "gemini_error_text");
    }

    #[test]
    fn parses_minimax_status_code_1027_epoch() {
        // MiniMax body shape with epoch-seconds available_after.
        let future_epoch = (Utc::now() + chrono::Duration::hours(2)).timestamp();
        let err = format!(
            r#"{{"base_resp":{{"status_code":1027,"status_msg":"quota exceeded"}},"available_after":{}}}"#,
            future_epoch
        );
        let result = parse_reset_time(&err);
        assert!(result.is_some(), "minimax 1027 epoch must parse");
        let (_, source) = result.unwrap();
        assert_eq!(source, "minimax_body_field");
    }

    #[test]
    fn parses_minimax_status_code_1027_rfc3339() {
        let future = (Utc::now() + chrono::Duration::hours(3))
            .to_rfc3339();
        let err = format!(
            r#"{{"base_resp":{{"status_code":1027}},"next_available_at":"{}"}}"#,
            future
        );
        let result = parse_reset_time(&err);
        assert!(result.is_some(), "minimax 1027 RFC3339 must parse");
    }

    #[test]
    fn parses_anthropic_header_rfc3339() {
        let future = (Utc::now() + chrono::Duration::minutes(30))
            .to_rfc3339();
        let err = format!(
            "HTTP 429 anthropic-ratelimit-requests-reset: {}",
            future
        );
        let result = parse_reset_time(&err);
        assert!(result.is_some(), "anthropic header pattern must parse");
        let (_, source) = result.unwrap();
        assert_eq!(source, "anthropic_header");
    }

    #[test]
    fn rejects_minimax_1027_with_past_timestamp() {
        // Stale timestamps shouldn't trigger — runtime is actually fine.
        let past_epoch = (Utc::now() - chrono::Duration::hours(1)).timestamp();
        let err = format!(
            r#"{{"base_resp":{{"status_code":1027}},"available_after":{}}}"#,
            past_epoch
        );
        assert!(parse_reset_time(&err).is_none());
    }

    #[test]
    fn rejects_minimax_1027_with_absurd_future() {
        // Sanity bound — 1 year out is suspicious; we cap at 180 days.
        let absurd_epoch = (Utc::now() + chrono::Duration::days(365)).timestamp();
        let err = format!(
            r#"{{"base_resp":{{"status_code":1027}},"available_after":{}}}"#,
            absurd_epoch
        );
        assert!(parse_reset_time(&err).is_none());
    }

    #[test]
    fn precedence_codex_beats_relative_seconds() {
        // Make sure the codex anchor fires before the relative-seconds
        // fallback for messages that mention both.
        let err = "Usage limit. try again at May 13th, 2026 12:10 PM. Or please try again in 60 seconds for related limits.";
        let (ts, source) = parse_reset_time(err).unwrap();
        assert_eq!(source, "codex_error_text");
        assert!(ts.starts_with("2026-05-13T"));
    }
}
