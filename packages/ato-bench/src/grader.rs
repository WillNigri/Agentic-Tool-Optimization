// The verifiable code-execution grader.
//
// This is the headline metric: pass/fail by RUNNING the code, not by asking a
// judge model whether it looks right. Given a model response, the grader
// extracts the code, runs it in the sandbox once per test case (input on stdin),
// and compares stdout to the expected output. A problem passes only if every
// test passes — the standard LiveCodeBench semantics — and we short-circuit on
// the first failure both for speed and to record what broke first.
//
// llm-judge grading (for suites with no executable oracle) lives in the
// methodology runner and its scorecards are labelled "judged, not verified".
// This grader is the "verified" path.

use crate::problem::{ComparisonMode, Problem};
use crate::receipt::{classify_contamination, FailureKind, RunContext, TaskReceipt};
use crate::sandbox::{ExecLimits, Sandbox};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Default fenced-code extraction: capture the body of a ```python / ``` block.
/// This exact string is part of the harness-config hash — changing it changes
/// the measurement.
pub const DEFAULT_EXTRACTION_REGEX: &str = r"(?s)```(?:python|py)?\s*?\n(.*?)```";

const STDERR_EXCERPT_CAP: usize = 2_000;

/// The grader knobs that affect the score. Embedded in `HarnessConfig` and
/// hashed, so two scorecards are only comparable when these match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraderConfig {
    /// Regex whose first capture group is the code to run.
    pub extraction_regex: String,
    pub comparison: ComparisonMode,
    pub limits: ExecLimits,
}

impl Default for GraderConfig {
    fn default() -> Self {
        Self {
            extraction_regex: DEFAULT_EXTRACTION_REGEX.to_string(),
            comparison: ComparisonMode::default(),
            limits: ExecLimits::default(),
        }
    }
}

/// Extract code from a model response. Uses the LAST fenced block matched by the
/// regex (models often explain, then give the final program). Falls back to the
/// whole trimmed response when no fence is present — many models return raw code
/// — so only a genuinely empty response counts as an extraction failure.
pub fn extract_code(response: &str, extraction_regex: &str) -> String {
    if let Ok(re) = regex::Regex::new(extraction_regex) {
        if let Some(code) = re
            .captures_iter(response)
            .last()
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
        {
            if !code.trim().is_empty() {
                return code;
            }
        }
    }
    response.trim().to_string()
}

/// Truncate to at most `max` bytes without splitting a UTF-8 char. stderr from
/// arbitrary model code can contain multibyte text, so naive `&s[..max]` would
/// panic.
fn safe_trunc(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn excerpt(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else if t.len() <= STDERR_EXCERPT_CAP {
        Some(t.to_string())
    } else {
        Some(format!("{}…", safe_trunc(t, STDERR_EXCERPT_CAP)))
    }
}

/// Grade one problem against one model response, producing an immutable receipt.
/// Does NOT call the model — the response is supplied by the dispatch layer, so
/// this stays a pure verifiable grader.
pub fn grade_problem(
    problem: &Problem,
    response: &str,
    ctx: &RunContext,
    cfg: &GraderConfig,
    sandbox: &dyn Sandbox,
) -> TaskReceipt {
    let start = Instant::now();
    let contamination =
        classify_contamination(problem.release_date.as_deref(), ctx.model_cutoff.as_deref());
    let sandbox_report = sandbox.report();
    let total = problem.tests.len();

    let receipt = |pass: bool,
                   passed: usize,
                   failure_kind: Option<FailureKind>,
                   stderr_excerpt: Option<String>|
     -> TaskReceipt {
        TaskReceipt {
            task_id: problem.id.clone(),
            model: ctx.model.clone(),
            provider: ctx.provider.clone(),
            model_revision: ctx.model_revision.clone(),
            sampling: ctx.sampling.clone(),
            pass,
            tests_total: total,
            tests_passed: passed,
            failure_kind,
            stderr_excerpt,
            grader_ms: start.elapsed().as_millis() as u64,
            sandbox: sandbox_report.clone(),
            contamination: contamination.clone(),
        }
    };

    let code = extract_code(response, &cfg.extraction_regex);
    if code.trim().is_empty() {
        return receipt(false, 0, Some(FailureKind::Extraction), None);
    }
    if total == 0 {
        // No oracle to verify against — a dataset/importer defect, not a model
        // failure. Labelled distinctly so it doesn't blame the model or pollute
        // the pass-rate denominator.
        return receipt(
            false,
            0,
            Some(FailureKind::InvalidProblem),
            Some("problem has no test cases".into()),
        );
    }

    let mut passed = 0usize;
    for test in &problem.tests {
        match sandbox.run(&code, &test.input, problem.language, &cfg.limits) {
            Ok(out) => {
                if out.timed_out {
                    return receipt(
                        false,
                        passed,
                        Some(FailureKind::Timeout),
                        excerpt(&out.stderr),
                    );
                }
                if out.exit_code != Some(0) {
                    let kind = if out.stderr.contains("SyntaxError")
                        || out.stderr.contains("IndentationError")
                    {
                        FailureKind::Compile
                    } else {
                        FailureKind::Runtime
                    };
                    return receipt(false, passed, Some(kind), excerpt(&out.stderr));
                }
                if out.stdout_truncated {
                    // Output exceeded the capture cap; we can't verify a
                    // truncated stream against full expected output. Surface it
                    // rather than laundering a truncated-mismatch into a plain
                    // WrongAnswer with no signal that size, not logic, was why.
                    return receipt(
                        false,
                        passed,
                        Some(FailureKind::WrongAnswer),
                        Some("stdout exceeded capture cap; output too large to verify".into()),
                    );
                }
                if !cfg.comparison.matches(&out.stdout, &test.expected) {
                    let detail = format!(
                        "expected {:?}, got {:?}",
                        truncate(&test.expected, 200),
                        truncate(&out.stdout, 200)
                    );
                    return receipt(false, passed, Some(FailureKind::WrongAnswer), Some(detail));
                }
                passed += 1;
            }
            Err(e) => {
                return receipt(
                    false,
                    passed,
                    Some(FailureKind::Sandbox),
                    Some(e.to_string()),
                );
            }
        }
    }

    receipt(true, passed, None, None)
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", safe_trunc(s, n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_last_fenced_python_block() {
        let resp = "Here's a first idea:\n```python\nprint(1)\n```\nActually, use:\n```python\nprint(2)\n```";
        assert_eq!(
            extract_code(resp, DEFAULT_EXTRACTION_REGEX).trim(),
            "print(2)"
        );
    }

    #[test]
    fn falls_back_to_whole_response_without_fence() {
        let resp = "print(42)";
        assert_eq!(extract_code(resp, DEFAULT_EXTRACTION_REGEX), "print(42)");
    }

    #[test]
    fn empty_response_extracts_empty() {
        assert_eq!(extract_code("   \n  ", DEFAULT_EXTRACTION_REGEX), "");
    }

    #[test]
    fn safe_trunc_never_splits_a_multibyte_char() {
        // "é" is 2 bytes; truncating at an odd byte must floor to a boundary,
        // not panic.
        let s = "aééééé";
        for max in 0..s.len() {
            let t = safe_trunc(s, max);
            assert!(s.starts_with(t));
            assert!(t.len() <= max);
        }
        // Emoji (4 bytes) at a cap that lands mid-char.
        assert_eq!(safe_trunc("🎯🎯", 3), "");
        assert_eq!(safe_trunc("🎯🎯", 5), "🎯");
    }
}
