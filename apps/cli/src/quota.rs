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
use chrono::{DateTime, Utc};
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
///
/// Conservative: only matches strict patterns to avoid false
/// positives. If we don't recognize the shape, we return None and
/// the caller treats it as a non-quota error.
pub fn parse_reset_time(error_message: &str) -> Option<(String, &'static str)> {
    // Codex: "try again at May 13th, 2026 12:10 PM"
    // Anchor on "try again at" to avoid false positives on prose
    // that happens to contain a date.
    if let Some(idx) = error_message.find("try again at ") {
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
    }
    // OpenAI-style: "Please try again in 12.5s" or "retry after 60s"
    if let Some(secs) = parse_relative_seconds(error_message) {
        let when = Utc::now() + chrono::Duration::seconds(secs as i64);
        return Some((when.to_rfc3339(), "relative_seconds"));
    }
    None
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
    fn strips_ordinals() {
        assert_eq!(strip_ordinal_suffixes("May 13th, 2026"), "May 13, 2026");
        assert_eq!(strip_ordinal_suffixes("Jan 1st"), "Jan 1");
        assert_eq!(strip_ordinal_suffixes("3rd quarter"), "3 quarter");
    }
}
