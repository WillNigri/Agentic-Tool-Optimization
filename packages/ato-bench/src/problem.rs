// The code-generation problem schema.
//
// A `Problem` is language-agnostic at the statement level: a natural-language
// prompt plus a set of stdin→stdout test cases. The model generates a complete
// program; the grader runs it once per test case (program on disk, test input
// piped to stdin) and compares stdout to `expected`. A problem passes only when
// ALL of its tests pass — the standard LiveCodeBench semantics.
//
// This is the shape the LiveCodeBench importer (a later slice) populates from
// the pinned upstream dataset, and the shape the synthetic MIT-clean fixtures
// use. `release_date` drives contamination filtering; it is `None` for
// synthetic problems.

use serde::{Deserialize, Serialize};

/// Languages the grader can execute. Python only for now; the enum keeps the
/// interpreter invocation in one place so more can be added without touching
/// the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Python,
}

impl Language {
    /// The argv used to run a program file under this language. Flags favor
    /// isolation and determinism: `-I` isolated mode (ignore env/user site),
    /// `-B` no bytecode writes.
    pub fn run_argv(&self, program_path: &str) -> Vec<String> {
        match self {
            Language::Python => vec![
                "python3".into(),
                "-I".into(),
                "-B".into(),
                program_path.into(),
            ],
        }
    }

    /// File extension for the program artifact.
    pub fn ext(&self) -> &'static str {
        match self {
            Language::Python => "py",
        }
    }
}

/// A single stdin→stdout test case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestCase {
    /// Piped to the program's stdin verbatim.
    pub input: String,
    /// Expected stdout, compared under the grader's `ComparisonMode`.
    pub expected: String,
}

/// A benchmark problem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    /// Stable task ID, e.g. "synthetic/sum-two" or an upstream LCB id.
    pub id: String,
    /// Provenance, e.g. "synthetic" or "livecodebench".
    pub source: String,
    pub language: Language,
    /// The natural-language problem statement handed to the model.
    pub prompt: String,
    #[serde(default)]
    pub tests: Vec<TestCase>,
    /// ISO date the underlying problem was published, for contamination
    /// filtering. `None` for synthetic problems (no real-world release).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
}

/// How generated stdout is compared to the expected output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonMode {
    /// Byte-for-byte equality.
    Exact,
    /// Equal after trimming leading/trailing whitespace on the whole output.
    Trimmed,
    /// Line-by-line: right-strip each line, drop trailing blank lines, compare.
    /// The most robust default for code output (tolerates trailing spaces and a
    /// missing/extra final newline without accepting genuinely wrong answers).
    #[default]
    TrimmedLines,
}

impl ComparisonMode {
    /// Does `actual` stdout match `expected` under this mode?
    pub fn matches(&self, actual: &str, expected: &str) -> bool {
        match self {
            ComparisonMode::Exact => actual == expected,
            ComparisonMode::Trimmed => actual.trim() == expected.trim(),
            ComparisonMode::TrimmedLines => normalize_lines(actual) == normalize_lines(expected),
        }
    }
}

/// Right-strip each line and drop trailing empty lines.
fn normalize_lines(s: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = s.lines().map(|l| l.trim_end()).collect();
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimmed_lines_tolerates_trailing_whitespace_and_final_newline() {
        let m = ComparisonMode::TrimmedLines;
        assert!(m.matches("5\n", "5"));
        assert!(m.matches("5", "5\n"));
        assert!(m.matches("1 \n2  \n", "1\n2"));
        assert!(m.matches("hello\n\n", "hello"));
    }

    #[test]
    fn trimmed_lines_rejects_genuinely_different_output() {
        let m = ComparisonMode::TrimmedLines;
        assert!(!m.matches("5", "6"));
        assert!(!m.matches("1\n2", "1\n3"));
        // Interior whitespace is significant.
        assert!(!m.matches("a  b", "a b"));
    }

    #[test]
    fn exact_mode_is_strict() {
        let m = ComparisonMode::Exact;
        assert!(m.matches("5\n", "5\n"));
        assert!(!m.matches("5\n", "5"));
    }

    #[test]
    fn problem_round_trips_through_json() {
        let p = Problem {
            id: "synthetic/sum-two".into(),
            source: "synthetic".into(),
            language: Language::Python,
            prompt: "Read two ints, print their sum.".into(),
            tests: vec![TestCase {
                input: "2 3\n".into(),
                expected: "5".into(),
            }],
            release_date: None,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Problem = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}
