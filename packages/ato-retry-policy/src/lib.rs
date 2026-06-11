// ato-retry-policy — shared retry classification + attempt accounting.
//
// Why this exists (v2.15.1, war_room 08F8629A):
//   Will hit gemini HTTP 503 four times across war-rooms today. The
//   model isn't broken — Google's per-model capacity allocation shifts
//   hour-to-hour. Same key, same code path, transient. For loops and
//   automation, failing on first 503 is catastrophic: one transient
//   failure kills the whole goal.
//
// Design (codex alternative-design verdict, war_room 08F8629A):
//   "Create a shared crate that does classification and accounting
//    only, not HTTP execution. Each surface keeps its own blocking/
//    async send call, but both pass the result into a common
//    classify_attempt(provider, http_status, headers, body,
//    transport_error) -> RetryDisposition and accumulate a shared
//    AttemptRecord. That avoids the reqwest 0.12 blocking vs 0.11
//    async mismatch while still eliminating policy drift."
//
// Scope (codex's "narrow first slice"):
//   - Retriable: 503, 502, 504, 429, transport-level timeout/connect
//   - max_attempts = 3, backoff 1s/4s/16s with jitter
//   - Honor Retry-After header on 429
//   - NO model fallback (silent model swaps break reproducibility)
//   - NO caller override knobs (env vars or flags)
//   - MiniMax body-status classification (provider already special-
//     cased in dispatch; retry must follow)

use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;

/// What can be retried. Wrapped in a config struct so policy can be
/// tweaked without code changes (e.g. tests can use shorter backoffs).
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    /// Wait BEFORE attempt N+1. `base_schedule[i]` is the wait before
    /// the i-th retry (0-indexed). Must have at least `max_attempts-1`
    /// entries. Last entry is reused if max_attempts grows beyond it.
    pub base_schedule: Vec<Duration>,
    pub retriable_http_statuses: Vec<u16>,
    pub honor_retry_after: bool,
}

impl RetryPolicy {
    /// v2.15.1 default policy. 3 attempts, 1s/4s backoff between them,
    /// retriable on the common transient HTTP codes + transport.
    /// Used by all dispatch paths unless explicitly overridden.
    pub fn default_v1() -> Self {
        Self {
            max_attempts: 3,
            base_schedule: vec![Duration::from_secs(1), Duration::from_secs(4)],
            // 429 = rate-limited (codex nuance: some 429s are short-lived
            // capacity windows, some are hard-quota exhaustion that won't
            // clear in 21s — we still retry but the user sees `retry_count`
            // in the receipt so persistent 429 is visible as failure
            // rather than as silent first-try error).
            retriable_http_statuses: vec![429, 502, 503, 504],
            honor_retry_after: true,
        }
    }

    /// Test policy — same shape, but with zero-duration backoffs so
    /// tests don't sleep.
    pub fn for_test() -> Self {
        Self {
            max_attempts: 3,
            base_schedule: vec![Duration::ZERO, Duration::ZERO],
            retriable_http_statuses: vec![429, 502, 503, 504],
            honor_retry_after: false,
        }
    }
}

/// What classifying a single attempt told us. Cases ordered from
/// terminal to retriable so a `match` reads naturally.
#[derive(Debug, Clone, PartialEq)]
pub enum AttemptOutcome {
    /// HTTP 2xx + provider body indicates success.
    Success,
    /// HTTP 4xx (auth, bad request) or 5xx not in the retriable set,
    /// OR provider-semantic permanent failure (e.g. MiniMax
    /// status_code 1004 with a quota-exhausted message).
    PermanentError {
        reason: String,
    },
    /// Retriable per the policy. `wait_hint` is set when the server
    /// gave us a Retry-After header (only used if policy honors it).
    RetriableError {
        reason: String,
        wait_hint: Option<Duration>,
    },
    /// reqwest itself errored before any HTTP status (DNS, connect,
    /// TLS, timeout). Treated as retriable per codex finding.
    TransportFailure {
        reason: String,
    },
}

/// What the policy says to do next given the latest attempt outcome
/// and the history so far.
#[derive(Debug, Clone, PartialEq)]
pub enum RetryDisposition {
    /// Latest attempt succeeded — caller should use the body.
    GiveUpSuccess,
    /// Latest attempt is terminal — caller should propagate the error.
    GiveUpPermanent {
        reason: String,
    },
    /// Retriable but max_attempts hit — caller propagates the last
    /// transient error (with retry_count in the audit row).
    GiveUpExhausted {
        last_reason: String,
        attempts_made: u32,
    },
    /// Sleep `wait`, then attempt again.
    RetryAfter {
        wait: Duration,
        next_attempt_index: u32,
    },
}

/// Recorded per attempt — both for the in-memory accounting that
/// drives `next_disposition` AND for the JSON column in execution_logs
/// (codex audit verdict: "one execution_logs row per dispatch, plus
/// retry_count and a compact JSON attempt summary column").
#[derive(Debug, Clone, Serialize)]
pub struct AttemptRecord {
    pub attempt_index: u32, // 0 = first try
    pub started_at_ms: i64,
    pub duration_ms: i64,
    pub status_code: Option<u16>,
    /// "success" | "retriable_5xx" | "rate_limited" | "transport" |
    /// "permanent" | "minimax_body_retriable" | "minimax_body_permanent"
    pub outcome_class: String,
    pub error_brief: Option<String>,
}

impl AttemptRecord {
    pub fn outcome_class_for(o: &AttemptOutcome, http_status: Option<u16>) -> String {
        match o {
            AttemptOutcome::Success => "success".to_string(),
            AttemptOutcome::PermanentError { .. } => "permanent".to_string(),
            AttemptOutcome::TransportFailure { .. } => "transport".to_string(),
            AttemptOutcome::RetriableError { .. } => match http_status {
                Some(429) => "rate_limited".to_string(),
                Some(502) | Some(503) | Some(504) => "retriable_5xx".to_string(),
                Some(_) => "retriable_other".to_string(),
                None => "retriable_provider_body".to_string(),
            },
        }
    }
}

/// Classify a single attempt. Provider-specific quirks (MiniMax's
/// status_code embedded in JSON body, etc.) live here, not in each
/// dispatcher.
pub fn classify_attempt(
    provider_flavor: &str,
    http_status: Option<u16>,
    response_headers: &HashMap<String, String>,
    response_body: Option<&str>,
    transport_error: Option<&str>,
) -> AttemptOutcome {
    // Transport errors first — they happen before any HTTP cycle.
    if let Some(t) = transport_error {
        return AttemptOutcome::TransportFailure {
            reason: t.to_string(),
        };
    }
    let status = match http_status {
        Some(s) => s,
        None => {
            return AttemptOutcome::PermanentError {
                reason: "neither HTTP status nor transport error supplied to classifier"
                    .to_string(),
            };
        }
    };

    // MiniMax: HTTP 200 with `base_resp.status_code != 0` is a
    // semantic error. The dispatch code currently special-cases this
    // (apps/cli/src/api_dispatch.rs:670, desktop mirror); the retry
    // classifier must follow.
    if provider_flavor == "minimax" && status == 200 {
        if let Some(body) = response_body {
            if let Some(class) = classify_minimax_body(body) {
                return class;
            }
        }
    }

    if (200..300).contains(&status) {
        return AttemptOutcome::Success;
    }

    let retriable = matches!(status, 429 | 502 | 503 | 504);
    let body_brief = response_body
        .map(|b| truncate(b, 240))
        .unwrap_or_else(|| "(no response body)".to_string());

    if !retriable {
        return AttemptOutcome::PermanentError {
            reason: format!("HTTP {} {}", status, body_brief),
        };
    }

    // Retriable. Pull Retry-After if present.
    let wait_hint = if status == 429 {
        parse_retry_after_header(response_headers).map(Duration::from_secs)
    } else {
        None
    };
    AttemptOutcome::RetriableError {
        reason: format!("HTTP {} {}", status, body_brief),
        wait_hint,
    }
}

fn classify_minimax_body(body: &str) -> Option<AttemptOutcome> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let code = v["base_resp"]["status_code"].as_i64()?;
    let msg = v["base_resp"]["status_msg"]
        .as_str()
        .unwrap_or("(no status_msg)");
    if code == 0 {
        return None; // Not an error body; fall back to HTTP path.
    }
    // MiniMax error codes are documented; 1004 = rate limited (treat
    // as retriable), 1002 = service-busy (retriable). Auth/quota
    // errors like 1000, 1008, 1013 are permanent.
    let retriable = matches!(code, 1002 | 1004 | 1027 | 2013);
    if retriable {
        Some(AttemptOutcome::RetriableError {
            reason: format!("MiniMax base_resp.status_code={} {}", code, msg),
            wait_hint: None,
        })
    } else {
        Some(AttemptOutcome::PermanentError {
            reason: format!("MiniMax base_resp.status_code={} {}", code, msg),
        })
    }
}

fn parse_retry_after_header(headers: &HashMap<String, String>) -> Option<u64> {
    // Look up case-insensitively per HTTP spec.
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("retry-after") {
            // RFC says either delta-seconds OR HTTP-date. Most APIs send
            // delta-seconds; we punt on HTTP-date parsing in v2.15.1.
            return v.trim().parse::<u64>().ok();
        }
    }
    None
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

/// Decide what to do next given the latest classification + history.
pub fn next_disposition(
    policy: &RetryPolicy,
    history: &[AttemptRecord],
    latest: AttemptOutcome,
) -> RetryDisposition {
    let attempts_made = history.len() as u32;
    match latest {
        AttemptOutcome::Success => RetryDisposition::GiveUpSuccess,
        AttemptOutcome::PermanentError { reason } => RetryDisposition::GiveUpPermanent { reason },
        AttemptOutcome::RetriableError { reason, wait_hint } => {
            if attempts_made >= policy.max_attempts {
                RetryDisposition::GiveUpExhausted {
                    last_reason: reason,
                    attempts_made,
                }
            } else {
                let wait = if policy.honor_retry_after {
                    wait_hint.unwrap_or_else(|| backoff_for_attempt(policy, attempts_made))
                } else {
                    backoff_for_attempt(policy, attempts_made)
                };
                RetryDisposition::RetryAfter {
                    wait,
                    next_attempt_index: attempts_made,
                }
            }
        }
        AttemptOutcome::TransportFailure { reason } => {
            if attempts_made >= policy.max_attempts {
                RetryDisposition::GiveUpExhausted {
                    last_reason: reason,
                    attempts_made,
                }
            } else {
                RetryDisposition::RetryAfter {
                    wait: backoff_for_attempt(policy, attempts_made),
                    next_attempt_index: attempts_made,
                }
            }
        }
    }
}

fn backoff_for_attempt(policy: &RetryPolicy, attempts_made: u32) -> Duration {
    let idx = attempts_made.saturating_sub(1) as usize;
    let base = policy
        .base_schedule
        .get(idx)
        .cloned()
        .or_else(|| policy.base_schedule.last().cloned())
        .unwrap_or(Duration::from_secs(1));
    // Add small jitter (±25%) so simultaneous loops don't thunder. We
    // use a tiny LCG (no rand dep in this crate) seeded by
    // attempt-index — deterministic in tests when policy uses Duration::ZERO.
    let jitter_pct = ((attempts_made as u64).wrapping_mul(2654435761) % 50) as i64 - 25;
    let base_ms = base.as_millis() as i64;
    let jitter_ms = base_ms * jitter_pct / 100;
    let total_ms = (base_ms + jitter_ms).max(0) as u64;
    Duration::from_millis(total_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_headers() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn success_2xx_classifies_as_success() {
        let o = classify_attempt("gemini", Some(200), &empty_headers(), Some(r#"{"x":1}"#), None);
        assert_eq!(o, AttemptOutcome::Success);
    }

    #[test]
    fn http_503_classifies_as_retriable() {
        let o = classify_attempt("gemini", Some(503), &empty_headers(), Some(r#"{"error":{"code":503}}"#), None);
        assert!(matches!(o, AttemptOutcome::RetriableError { .. }));
    }

    #[test]
    fn http_401_classifies_as_permanent() {
        let o = classify_attempt("gemini", Some(401), &empty_headers(), Some("unauthorized"), None);
        assert!(matches!(o, AttemptOutcome::PermanentError { .. }));
    }

    #[test]
    fn http_429_honors_retry_after_header() {
        let mut headers = HashMap::new();
        headers.insert("Retry-After".to_string(), "7".to_string());
        let o = classify_attempt("openai", Some(429), &headers, Some(""), None);
        match o {
            AttemptOutcome::RetriableError { wait_hint, .. } => {
                assert_eq!(wait_hint, Some(Duration::from_secs(7)));
            }
            _ => panic!("expected RetriableError"),
        }
    }

    #[test]
    fn transport_failure_classifies_as_transport() {
        let o = classify_attempt("gemini", None, &empty_headers(), None, Some("connect timed out"));
        assert!(matches!(o, AttemptOutcome::TransportFailure { .. }));
    }

    #[test]
    fn minimax_status_code_1004_classifies_as_retriable() {
        let body = r#"{"base_resp":{"status_code":1004,"status_msg":"rate limited"}}"#;
        let o = classify_attempt("minimax", Some(200), &empty_headers(), Some(body), None);
        assert!(matches!(o, AttemptOutcome::RetriableError { .. }));
    }

    #[test]
    fn minimax_status_code_1000_classifies_as_permanent() {
        let body = r#"{"base_resp":{"status_code":1000,"status_msg":"auth failed"}}"#;
        let o = classify_attempt("minimax", Some(200), &empty_headers(), Some(body), None);
        assert!(matches!(o, AttemptOutcome::PermanentError { .. }));
    }

    #[test]
    fn minimax_status_code_0_falls_through_to_success() {
        // MiniMax with status_code=0 means OK at the semantic layer;
        // we treat as Success.
        let body = r#"{"base_resp":{"status_code":0},"choices":[{"message":{"content":"hi"}}]}"#;
        let o = classify_attempt("minimax", Some(200), &empty_headers(), Some(body), None);
        assert_eq!(o, AttemptOutcome::Success);
    }

    #[test]
    fn first_retriable_returns_retry_after_short_wait() {
        let p = RetryPolicy::for_test();
        let history: Vec<AttemptRecord> = vec![mk_attempt(0, 200, "transport")];
        let latest = AttemptOutcome::RetriableError {
            reason: "503".to_string(),
            wait_hint: None,
        };
        let disp = next_disposition(&p, &history, latest);
        match disp {
            RetryDisposition::RetryAfter {
                next_attempt_index,
                wait,
            } => {
                assert_eq!(next_attempt_index, 1);
                assert!(wait.as_millis() < 1_000); // for_test uses ZERO
            }
            _ => panic!("expected RetryAfter"),
        }
    }

    #[test]
    fn exhaustion_returns_GiveUpExhausted() {
        let p = RetryPolicy::for_test();
        let history: Vec<AttemptRecord> = (0..3).map(|i| mk_attempt(i, 200, "transport")).collect();
        let latest = AttemptOutcome::RetriableError {
            reason: "503 again".to_string(),
            wait_hint: None,
        };
        let disp = next_disposition(&p, &history, latest);
        match disp {
            RetryDisposition::GiveUpExhausted {
                attempts_made,
                last_reason,
            } => {
                assert_eq!(attempts_made, 3);
                assert!(last_reason.contains("503"));
            }
            _ => panic!("expected GiveUpExhausted"),
        }
    }

    #[test]
    fn permanent_error_does_not_retry_even_on_first_attempt() {
        let p = RetryPolicy::for_test();
        let history: Vec<AttemptRecord> = vec![];
        let latest = AttemptOutcome::PermanentError {
            reason: "401 unauthorized".to_string(),
        };
        let disp = next_disposition(&p, &history, latest);
        assert!(matches!(disp, RetryDisposition::GiveUpPermanent { .. }));
    }

    fn mk_attempt(idx: u32, dur_ms: i64, class: &str) -> AttemptRecord {
        AttemptRecord {
            attempt_index: idx,
            started_at_ms: 0,
            duration_ms: dur_ms,
            status_code: Some(503),
            outcome_class: class.to_string(),
            error_brief: Some("503".to_string()),
        }
    }
}
