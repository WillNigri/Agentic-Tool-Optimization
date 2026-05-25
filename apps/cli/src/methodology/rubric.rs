// v2.10.0 PR-4 — Rubric library: how a dispatch gets a score.
//
// Three foundational rubric kinds + one composite that combines them.
// The runner calls `Rubric::score(prompt, response, db_path)` after each
// dispatch (or each adopt) and stores the resulting `score` on
// methodology_run_dispatches. Composition (PR-3) now operates over
// scores rather than just receipt-native metrics.
//
//   ┌─ Regex      — pattern match against response. score = 1.0 if match, 0.0 else.
//   ├─ Structural — N independent assertions (must_contain / must_not_contain /
//   │              min/max length). score = (passed assertions) / (total assertions).
//   ├─ LlmJudge   — fan out to a judge model that returns {"score": 0..1, "reason"}.
//   │              Cost lands in `provider_judge_cost_usd`.
//   └─ Composite  — combine sub-rubric scores via mean / min / weighted.
//
// JSON schema (what a customer writes in their methodology config):
//
//   {"kind": "regex", "pattern": "(?i)vulnerability", "label": "mentions-vuln"}
//   {"kind": "structural",
//    "must_contain": ["SQL injection", "vulnerability"],
//    "must_not_contain": ["I cannot help"],
//    "min_length": 200}
//   {"kind": "llm_judge",
//    "judge_model": "claude-sonnet-4-6",
//    "judge_runtime": "claude",
//    "prompt_template": "Did the response answer the question? {{response}}"}
//   {"kind": "composite",
//    "combiner": "mean",
//    "rubrics": [ <any of the above>, ... ]}
//
// All variants are JSON-tagged for forward-compat: unknown variants
// surface as `RubricParseError` rather than crashing.

use anyhow::{Context, Result};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// Rubric definition. Tagged enum so the JSON shape stays customer-readable
/// and serde unknown variants land as parse errors not silent defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Rubric {
    Regex {
        pattern: String,
        #[serde(default)]
        case_insensitive: bool,
        #[serde(default)]
        label: Option<String>,
    },
    Structural {
        #[serde(default)]
        must_contain: Vec<String>,
        #[serde(default)]
        must_not_contain: Vec<String>,
        #[serde(default)]
        min_length: Option<usize>,
        #[serde(default)]
        max_length: Option<usize>,
        #[serde(default)]
        label: Option<String>,
    },
    LlmJudge {
        judge_model: String,
        /// Runtime to dispatch the judge through. Defaults to the same
        /// auto-derivation rules the runner uses (`claude-*` → anthropic).
        /// Set to `"claude"` to use the CLI subscription instead of API.
        #[serde(default)]
        judge_runtime: Option<String>,
        /// Prompt template. `{{prompt}}` + `{{response}}` are substituted
        /// at call time. If omitted, a default template is used.
        #[serde(default)]
        prompt_template: Option<String>,
        #[serde(default)]
        label: Option<String>,
    },
    Composite {
        rubrics: Vec<Rubric>,
        #[serde(default)]
        combiner: Combiner,
        #[serde(default)]
        label: Option<String>,
    },
    /// Placeholder used by PR-3 methodologies that hadn't picked a rubric
    /// yet. Scoring returns 0.0 with a "not scored" reason. Lets
    /// adopted/legacy runs deserialize cleanly.
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Combiner {
    /// Arithmetic mean across sub-rubric scores. Default.
    Mean,
    /// Minimum sub-rubric score — any failure drags the composite down.
    /// Use when ALL sub-rubrics must pass for the dispatch to count.
    Min,
    /// Weighted mean. Weights MUST be the same length as `rubrics`; if
    /// not, falls back to plain mean and notes it in `reason`.
    Weighted { weights: Vec<f64> },
}

impl Default for Combiner {
    fn default() -> Self {
        Self::Mean
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubricScore {
    pub score: f64,
    pub reason: String,
    #[serde(default)]
    pub sub_scores: Vec<f64>,
    /// USD spent on LLM-judge calls for this dispatch. Zero for non-judge
    /// rubrics. Sum across all dispatches lands on
    /// methodology_runs.provider_judge_cost_usd.
    #[serde(default)]
    pub judge_cost_usd: f64,
}

impl RubricScore {
    pub fn pass() -> Self {
        Self {
            score: 1.0,
            reason: "passed".to_string(),
            sub_scores: Vec::new(),
            judge_cost_usd: 0.0,
        }
    }
    pub fn fail(reason: impl Into<String>) -> Self {
        Self {
            score: 0.0,
            reason: reason.into(),
            sub_scores: Vec::new(),
            judge_cost_usd: 0.0,
        }
    }
    pub fn partial(score: f64, reason: impl Into<String>) -> Self {
        Self {
            score: score.clamp(0.0, 1.0),
            reason: reason.into(),
            sub_scores: Vec::new(),
            judge_cost_usd: 0.0,
        }
    }
}

impl Rubric {
    pub fn parse(value: &serde_json::Value) -> Result<Self> {
        serde_json::from_value(value.clone()).context(
            "parse rubric JSON — expected {\"kind\": \"regex|structural|llm_judge|composite|pending\", ...}",
        )
    }

    pub fn score(
        &self,
        prompt: &str,
        response: &str,
        db_path: &Path,
    ) -> Result<RubricScore> {
        match self {
            Rubric::Regex {
                pattern,
                case_insensitive,
                ..
            } => Ok(score_regex(pattern, *case_insensitive, response)),
            Rubric::Structural {
                must_contain,
                must_not_contain,
                min_length,
                max_length,
                ..
            } => Ok(score_structural(
                must_contain,
                must_not_contain,
                *min_length,
                *max_length,
                response,
            )),
            Rubric::LlmJudge {
                judge_model,
                judge_runtime,
                prompt_template,
                ..
            } => score_llm_judge(
                judge_model,
                judge_runtime.as_deref(),
                prompt_template.as_deref(),
                prompt,
                response,
                db_path,
            ),
            Rubric::Composite {
                rubrics, combiner, ..
            } => score_composite(rubrics, combiner, prompt, response, db_path),
            Rubric::Pending => Ok(RubricScore {
                score: 0.0,
                reason: "pending — no rubric defined yet (PR-4 placeholder)".to_string(),
                sub_scores: Vec::new(),
                judge_cost_usd: 0.0,
            }),
        }
    }
}

fn score_regex(pattern: &str, case_insensitive: bool, response: &str) -> RubricScore {
    match RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
    {
        Ok(re) => {
            if re.is_match(response) {
                RubricScore {
                    score: 1.0,
                    reason: format!("matched `{}`", pattern),
                    sub_scores: Vec::new(),
                    judge_cost_usd: 0.0,
                }
            } else {
                RubricScore {
                    score: 0.0,
                    reason: format!("no match for `{}`", pattern),
                    sub_scores: Vec::new(),
                    judge_cost_usd: 0.0,
                }
            }
        }
        Err(e) => RubricScore::fail(format!("invalid regex `{}`: {}", pattern, e)),
    }
}

fn score_structural(
    must_contain: &[String],
    must_not_contain: &[String],
    min_length: Option<usize>,
    max_length: Option<usize>,
    response: &str,
) -> RubricScore {
    let mut total = 0;
    let mut passed = 0;
    let mut reasons: Vec<String> = Vec::new();

    for needle in must_contain {
        total += 1;
        if response.contains(needle.as_str()) {
            passed += 1;
        } else {
            reasons.push(format!("missing `{}`", short(needle, 40)));
        }
    }
    for needle in must_not_contain {
        total += 1;
        if !response.contains(needle.as_str()) {
            passed += 1;
        } else {
            reasons.push(format!("forbidden phrase present: `{}`", short(needle, 40)));
        }
    }
    if let Some(min) = min_length {
        total += 1;
        if response.len() >= min {
            passed += 1;
        } else {
            reasons.push(format!("too short ({} < min {})", response.len(), min));
        }
    }
    if let Some(max) = max_length {
        total += 1;
        if response.len() <= max {
            passed += 1;
        } else {
            reasons.push(format!("too long ({} > max {})", response.len(), max));
        }
    }
    if total == 0 {
        return RubricScore::partial(0.0, "structural rubric has no assertions");
    }
    let score = passed as f64 / total as f64;
    let reason = if reasons.is_empty() {
        format!("passed all {} structural assertions", total)
    } else {
        format!("{}/{} passed; failures: {}", passed, total, reasons.join("; "))
    };
    RubricScore {
        score,
        reason,
        sub_scores: Vec::new(),
        judge_cost_usd: 0.0,
    }
}

fn short(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.min(s.len())])
    }
}

fn score_llm_judge(
    judge_model: &str,
    judge_runtime: Option<&str>,
    prompt_template: Option<&str>,
    prompt: &str,
    response: &str,
    db_path: &Path,
) -> Result<RubricScore> {
    if response.is_empty() {
        return Ok(RubricScore::fail("response is empty — no signal to judge"));
    }
    let runtime = match judge_runtime {
        Some(r) => r.to_string(),
        None => match ato_pricing::provider_for_model(judge_model) {
            Some(p) => p.to_string(),
            None => "claude".to_string(),
        },
    };
    let template = prompt_template.unwrap_or(DEFAULT_JUDGE_TEMPLATE);
    let judge_prompt = template
        .replace("{{prompt}}", prompt)
        .replace("{{response}}", response);

    // Capture the new execution_logs row via the same rowid-before idiom
    // the runner uses for fan-out cells.
    let conn = crate::db::open_readonly(db_path)?;
    let before_max: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(rowid), 0) FROM execution_logs",
            [],
            |r| r.get(0),
        )
        .context("read execution_logs MAX(rowid) before judge dispatch")?;
    drop(conn);

    let exe = std::env::current_exe().context("locate current ato binary for judge")?;
    let output = Command::new(&exe)
        .arg("dispatch")
        .arg(&runtime)
        .arg(&judge_prompt)
        .arg("--model")
        .arg(judge_model)
        .arg("--quiet")
        .arg("--db")
        .arg(db_path)
        .output()
        .with_context(|| format!("spawn judge dispatch `ato dispatch {}`", runtime))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(RubricScore::fail(format!(
            "judge {} exited {:?}: {}",
            runtime,
            output.status.code(),
            stderr.trim()
        )));
    }

    let conn = crate::db::open_readonly(db_path)?;
    let row: (Option<String>, Option<f64>, Option<String>) = conn
        .query_row(
            "SELECT response, cost_usd_estimated, status
             FROM execution_logs
             WHERE rowid > ?1
             ORDER BY rowid ASC
             LIMIT 1",
            rusqlite::params![before_max],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .context("read judge dispatch's execution_logs row")?;

    let judge_response = row.0.unwrap_or_default();
    let judge_cost = row.1.unwrap_or(0.0);
    let status = row.2.unwrap_or_else(|| "unknown".to_string());

    if status != "success" {
        return Ok(RubricScore {
            score: 0.0,
            reason: format!("judge dispatch status: {}", status),
            sub_scores: Vec::new(),
            judge_cost_usd: judge_cost,
        });
    }

    match parse_judge_response(&judge_response) {
        Some((score, reason)) => Ok(RubricScore {
            score: score.clamp(0.0, 1.0),
            reason,
            sub_scores: Vec::new(),
            judge_cost_usd: judge_cost,
        }),
        None => Ok(RubricScore {
            score: 0.0,
            reason: format!(
                "judge response did not contain a parseable score (first 200 chars: {})",
                short(&judge_response, 200)
            ),
            sub_scores: Vec::new(),
            judge_cost_usd: judge_cost,
        }),
    }
}

const DEFAULT_JUDGE_TEMPLATE: &str = r#"You are an evaluator. Score the response below against the original prompt on a 0..1 scale (0 = completely wrong/unhelpful, 1 = excellent answer).

Reply ONLY with a JSON object on a single line: {"score": <float 0..1>, "reason": "<one short sentence>"}.

Original prompt:
{{prompt}}

Response to evaluate:
{{response}}
"#;

/// Extract `{"score": ..., "reason": ...}` from the judge's response.
/// Tolerant of pre/post text — finds the first JSON object that has both
/// a `score` number and a `reason` string. Returns None if neither
/// pattern matches.
/// v2.11 PR-12.1 — brace-balanced JSON snippet extractor.
///
/// Public so the diagnose pipeline can reuse the same tolerance for
/// LLMs that wrap their JSON in preamble text. Scans `text` for the
/// first `{...}` block (tracking nesting + strings + escapes) where
/// EVERY key in `required_keys` is present as a top-level field.
/// Returns the slice (still as a `String` so the caller can re-parse
/// into whatever target type they want). O(N·K) worst case where N is
/// `text.len()` and K is the average distance to a closing brace —
/// LLM responses are bounded, so this is fine in practice.
pub fn parse_brace_balanced_json(text: &str, required_keys: &[&str]) -> Option<String> {
    let bytes = text.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] != b'{' {
            continue;
        }
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escape = false;
        for j in i..bytes.len() {
            let c = bytes[j];
            if escape {
                escape = false;
                continue;
            }
            if in_string {
                match c {
                    b'\\' => escape = true,
                    b'"' => in_string = false,
                    _ => {}
                }
                continue;
            }
            match c {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let snippet = &text[i..=j];
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(snippet) {
                            if required_keys.iter().all(|k| v.get(*k).is_some()) {
                                return Some(snippet.to_string());
                            }
                        }
                        break; // try next `{`
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn parse_judge_response(text: &str) -> Option<(f64, String)> {
    // Try whole-text JSON first (cleanest path).
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(s) = v.get("score").and_then(|x| x.as_f64()) {
            let reason = v
                .get("reason")
                .and_then(|x| x.as_str())
                .unwrap_or("(no reason)")
                .to_string();
            return Some((s, reason));
        }
    }
    // Fall back: brace-balanced scan. For each `{` find its matching
    // `}` (tracking nesting + strings + escapes) and try to parse the
    // resulting slice. First slice with both a `score` number and any
    // `reason` field wins. O(N²) worst case but responses are bounded.
    let bytes = text.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] != b'{' {
            continue;
        }
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escape = false;
        for j in i..bytes.len() {
            let c = bytes[j];
            if escape {
                escape = false;
                continue;
            }
            if in_string {
                match c {
                    b'\\' => escape = true,
                    b'"' => in_string = false,
                    _ => {}
                }
                continue;
            }
            match c {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        // Slice through index j inclusive.
                        if let Ok(v) =
                            serde_json::from_str::<serde_json::Value>(&text[i..=j])
                        {
                            if let Some(s) = v.get("score").and_then(|x| x.as_f64()) {
                                let reason = v
                                    .get("reason")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("(no reason)")
                                    .to_string();
                                return Some((s, reason));
                            }
                        }
                        break; // try next `{` start
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn score_composite(
    rubrics: &[Rubric],
    combiner: &Combiner,
    prompt: &str,
    response: &str,
    db_path: &Path,
) -> Result<RubricScore> {
    if rubrics.is_empty() {
        return Ok(RubricScore::partial(0.0, "composite rubric has no sub-rubrics"));
    }
    let mut sub_scores: Vec<f64> = Vec::with_capacity(rubrics.len());
    let mut total_judge_cost: f64 = 0.0;
    let mut reasons: Vec<String> = Vec::with_capacity(rubrics.len());
    for r in rubrics {
        let s = r.score(prompt, response, db_path)?;
        sub_scores.push(s.score);
        total_judge_cost += s.judge_cost_usd;
        reasons.push(format!("{:.2}: {}", s.score, short(&s.reason, 80)));
    }
    let combined = match combiner {
        Combiner::Mean => sub_scores.iter().sum::<f64>() / sub_scores.len() as f64,
        Combiner::Min => sub_scores
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min),
        Combiner::Weighted { weights } => {
            if weights.len() != sub_scores.len() {
                // Falls back to plain mean — annotated below.
                sub_scores.iter().sum::<f64>() / sub_scores.len() as f64
            } else {
                let sum_w: f64 = weights.iter().sum();
                if sum_w == 0.0 {
                    sub_scores.iter().sum::<f64>() / sub_scores.len() as f64
                } else {
                    sub_scores
                        .iter()
                        .zip(weights.iter())
                        .map(|(s, w)| s * w)
                        .sum::<f64>()
                        / sum_w
                }
            }
        }
    };
    let reason = match combiner {
        Combiner::Mean => format!("mean({}) — {}", sub_scores.len(), reasons.join(" | ")),
        Combiner::Min => format!("min({}) — {}", sub_scores.len(), reasons.join(" | ")),
        Combiner::Weighted { .. } => {
            format!("weighted({}) — {}", sub_scores.len(), reasons.join(" | "))
        }
    };
    Ok(RubricScore {
        score: combined.clamp(0.0, 1.0),
        reason,
        sub_scores,
        judge_cost_usd: total_judge_cost,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fake_db() -> PathBuf {
        // Tests that don't dispatch (regex, structural, composite-of-
        // non-judge) never read this path. The few that do should be
        // marked #[ignore] and run only against a real DB.
        PathBuf::from("/tmp/methodology-rubric-tests-fake.db")
    }

    #[test]
    fn regex_matches_pass() {
        let r = Rubric::Regex {
            pattern: "vulnerab".to_string(),
            case_insensitive: false,
            label: None,
        };
        let s = r.score("prompt", "Found a vulnerability in src/auth.ts", &fake_db()).unwrap();
        assert_eq!(s.score, 1.0);
    }

    #[test]
    fn regex_case_insensitive_flag_works() {
        let r = Rubric::Regex {
            pattern: "VULNERABILITY".to_string(),
            case_insensitive: true,
            label: None,
        };
        let s = r.score("prompt", "Found a vulnerability", &fake_db()).unwrap();
        assert_eq!(s.score, 1.0);
    }

    #[test]
    fn regex_no_match_scores_zero() {
        let r = Rubric::Regex {
            pattern: "vulnerab".to_string(),
            case_insensitive: false,
            label: None,
        };
        let s = r.score("prompt", "The code is fine", &fake_db()).unwrap();
        assert_eq!(s.score, 0.0);
    }

    #[test]
    fn regex_invalid_pattern_scores_zero_with_reason() {
        let r = Rubric::Regex {
            pattern: "[unclosed".to_string(),
            case_insensitive: false,
            label: None,
        };
        let s = r.score("prompt", "anything", &fake_db()).unwrap();
        assert_eq!(s.score, 0.0);
        assert!(s.reason.contains("invalid regex"));
    }

    #[test]
    fn structural_all_assertions_pass() {
        let r = Rubric::Structural {
            must_contain: vec!["SQL".to_string(), "injection".to_string()],
            must_not_contain: vec!["I cannot".to_string()],
            min_length: Some(20),
            max_length: Some(500),
            label: None,
        };
        let s = r
            .score(
                "prompt",
                "Found a SQL injection vulnerability in the login query.",
                &fake_db(),
            )
            .unwrap();
        // 2 must_contain + 1 must_not_contain + 1 min_length + 1 max_length = 5
        assert_eq!(s.score, 1.0, "got {} ({})", s.score, s.reason);
        assert!(
            s.reason.contains("passed all 5"),
            "expected 'passed all 5' assertions in reason; got: {}",
            s.reason
        );
    }

    #[test]
    fn structural_partial_credit_proportional() {
        let r = Rubric::Structural {
            must_contain: vec!["SQL".to_string(), "injection".to_string()],
            must_not_contain: vec![],
            min_length: None,
            max_length: None,
            label: None,
        };
        let s = r.score("prompt", "SQL but no other word", &fake_db()).unwrap();
        // 1/2 pass → 0.5
        assert!((s.score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn structural_min_length_fails_with_reason() {
        let r = Rubric::Structural {
            must_contain: vec![],
            must_not_contain: vec![],
            min_length: Some(100),
            max_length: None,
            label: None,
        };
        let s = r.score("prompt", "short", &fake_db()).unwrap();
        assert!(s.score < 1.0);
        assert!(s.reason.contains("too short"));
    }

    #[test]
    fn composite_mean_combines_sub_scores() {
        let r = Rubric::Composite {
            rubrics: vec![
                Rubric::Regex {
                    pattern: "yes".to_string(),
                    case_insensitive: false,
                    label: None,
                },
                Rubric::Regex {
                    pattern: "no".to_string(),
                    case_insensitive: false,
                    label: None,
                },
            ],
            combiner: Combiner::Mean,
            label: None,
        };
        let s = r.score("prompt", "yes only", &fake_db()).unwrap();
        // First passes (1.0), second fails (0.0) → mean 0.5
        assert!((s.score - 0.5).abs() < 1e-9);
        assert_eq!(s.sub_scores, vec![1.0, 0.0]);
    }

    #[test]
    fn composite_min_drops_to_lowest() {
        let r = Rubric::Composite {
            rubrics: vec![
                Rubric::Regex {
                    pattern: "yes".to_string(),
                    case_insensitive: false,
                    label: None,
                },
                Rubric::Regex {
                    pattern: "no".to_string(),
                    case_insensitive: false,
                    label: None,
                },
            ],
            combiner: Combiner::Min,
            label: None,
        };
        let s = r.score("prompt", "yes only", &fake_db()).unwrap();
        assert_eq!(s.score, 0.0);
    }

    #[test]
    fn composite_weighted_combines_with_weights() {
        let r = Rubric::Composite {
            rubrics: vec![
                Rubric::Regex {
                    pattern: "yes".to_string(),
                    case_insensitive: false,
                    label: None,
                },
                Rubric::Regex {
                    pattern: "no".to_string(),
                    case_insensitive: false,
                    label: None,
                },
            ],
            combiner: Combiner::Weighted {
                weights: vec![3.0, 1.0],
            },
            label: None,
        };
        let s = r.score("prompt", "yes only", &fake_db()).unwrap();
        // (1.0*3 + 0.0*1) / (3+1) = 0.75
        assert!((s.score - 0.75).abs() < 1e-9);
    }

    #[test]
    fn rubric_json_round_trip_for_each_variant() {
        let variants: Vec<serde_json::Value> = vec![
            serde_json::json!({"kind": "regex", "pattern": "x"}),
            serde_json::json!({"kind": "structural", "must_contain": ["a"]}),
            serde_json::json!({"kind": "llm_judge", "judge_model": "claude-sonnet-4-6"}),
            serde_json::json!({"kind": "composite", "combiner": "mean", "rubrics": []}),
            serde_json::json!({"kind": "pending"}),
        ];
        for v in &variants {
            let r = Rubric::parse(v).expect("parse rubric");
            let round = serde_json::to_value(&r).expect("re-serialize");
            assert_eq!(round.get("kind"), v.get("kind"));
        }
    }

    #[test]
    fn parse_judge_response_extracts_from_pure_json() {
        let r = parse_judge_response(r#"{"score": 0.87, "reason": "thorough"}"#);
        assert_eq!(r, Some((0.87, "thorough".to_string())));
    }

    #[test]
    fn parse_judge_response_extracts_from_text_with_preamble() {
        let r = parse_judge_response(
            r#"Sure! Here's my evaluation: {"score": 0.4, "reason": "weak"}"#,
        );
        assert_eq!(r, Some((0.4, "weak".to_string())));
    }

    #[test]
    fn parse_judge_response_returns_none_for_unparseable() {
        assert!(parse_judge_response("I dont know").is_none());
    }

    #[test]
    fn pending_rubric_scores_zero_without_dispatch() {
        let s = Rubric::Pending.score("p", "r", &fake_db()).unwrap();
        assert_eq!(s.score, 0.0);
        assert!(s.reason.contains("pending"));
    }
}
