// The execution-receipt contract — the "open box" made concrete.
//
// Two "LiveCodeBench 62%" numbers are NOT comparable unless you can see how
// each was produced. This module defines the receipt that travels with every
// scorecard so a re-run is verifiable:
//
//   • DatasetSnapshot  — which tasks, which pinned version/revision, run date.
//                        Pin by hash; never silently refresh.
//   • HarnessConfig    — the prompt wrapper, system prompt, stop tokens, code
//                        extraction regex, comparison mode, exec limits, and
//                        sampling (attempts / temperature). This is what makes
//                        two scores comparable — publish its hash.
//   • ExecEnv          — os/arch, sandbox backend + isolation, runtime version.
//   • TaskReceipt      — per-task: model + provider + revision, sampling,
//                        pass/fail, test counts, failure kind, stderr excerpt,
//                        sandbox report, contamination flag.
//
// Everything here is pure data + deterministic hashing. No model calls, no
// process execution (that lives in `sandbox` + `grader`).

use crate::grader::GraderConfig;
use crate::sandbox::SandboxReport;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Deterministic SHA-256 of any serializable value, hex-encoded.
///
/// The value is routed through `serde_json::Value` (whose object maps are
/// `BTreeMap`, i.e. key-sorted, since we do not enable the `preserve_order`
/// feature) so the byte stream is canonical regardless of struct field order.
pub fn stable_hash<T: Serialize>(value: &T) -> String {
    // Fail CLOSED. A serialization failure must NOT collapse to the hash of
    // empty bytes — that would look like a valid 64-hex hash and let distinct
    // configs silently collide. Our receipt structs are always serializable;
    // the only realistic trigger is a non-finite float in `Sampling` (a caller
    // bug: sampling params must be finite), so surface it loudly instead.
    let canonical = serde_json::to_value(value)
        .and_then(|v| serde_json::to_vec(&v))
        .expect("bench receipt structs must serialize (non-finite float in config?)");
    let mut hasher = Sha256::new();
    hasher.update(&canonical);
    to_hex(&hasher.finalize())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// The sampling parameters used for a model call. Captured on every receipt so
/// the run is reproducible-to-a-distribution. `seed` is `None` when the
/// provider does not honor one (most don't) — in which case even temp=0 is not
/// bit-reproducible and repeated attempts should report a distribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sampling {
    pub temperature: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// One attempt per task is the code-exec default; >1 is opt-in for
    /// nondeterminism studies.
    pub attempts: u32,
}

impl Default for Sampling {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            top_p: None,
            max_tokens: None,
            seed: None,
            attempts: 1,
        }
    }
}

/// The full harness configuration whose hash gates comparability. Composes the
/// grader knobs (`GraderConfig`) with the prompt-shaping fields that the
/// dispatch layer owns. Change any field → the hash changes → the scorecard is
/// explicitly a different measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessConfig {
    /// Optional system prompt sent with every task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Template wrapping each problem statement. `{{problem}}` is substituted.
    pub prompt_wrapper: String,
    /// Stop sequences passed to the model.
    #[serde(default)]
    pub stop_tokens: Vec<String>,
    /// Sampling parameters (temperature/attempts/etc).
    pub sampling: Sampling,
    /// Grader knobs: extraction regex, comparison mode, exec limits.
    pub grader: GraderConfig,
}

impl HarnessConfig {
    /// The hash published on every scorecard. Include a schema tag so the hash
    /// space is versioned if the receipt layout ever changes.
    pub fn hash(&self) -> String {
        stable_hash(&("ato-bench/harness/v1", self))
    }
}

/// The pinned dataset the scorecard was run against. Its hash is what makes a
/// re-run identifiable as "the same benchmark".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatasetSnapshot {
    /// e.g. "livecodebench" or "synthetic".
    pub source: String,
    /// Upstream version tag when applicable (e.g. "release_v6").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_tag: Option<String>,
    /// Exact upstream revision (e.g. a HuggingFace commit SHA). Pinning this is
    /// how we refuse to "silently refresh".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// The exact task IDs included, in order.
    pub task_ids: Vec<String>,
    /// RFC3339 date the run was executed (supplied by the caller — this crate
    /// takes no wall-clock dependency).
    pub run_date: String,
}

impl DatasetSnapshot {
    /// Hash of dataset IDENTITY only. `run_date` is deliberately EXCLUDED so
    /// re-running the same pinned dataset on a different day yields the same
    /// hash — that is the reproducibility contract. The date is run metadata,
    /// not dataset identity. `task_ids` order is part of identity, so the
    /// importer must emit them deterministically.
    pub fn hash(&self) -> String {
        stable_hash(&(
            "ato-bench/dataset/v1",
            &self.source,
            &self.version_tag,
            &self.revision,
            &self.task_ids,
        ))
    }
}

/// The environment the grader executed in. Part of the receipt because a
/// sandbox mode or runtime-version change can move scores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecEnv {
    pub os: String,
    pub arch: String,
    /// e.g. "docker", "seatbelt", "unconfined".
    pub sandbox_backend: String,
    /// e.g. "Python 3.14.2" or a docker image digest.
    pub runtime_version: String,
}

impl ExecEnv {
    pub fn hash(&self) -> String {
        stable_hash(&("ato-bench/execenv/v1", self))
    }
}

/// Why a task failed. `None` on the receipt means it passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// No code could be extracted from the model response.
    Extraction,
    /// Code raised a SyntaxError (failed to compile/parse).
    Compile,
    /// Code ran but exited non-zero / raised at runtime.
    Runtime,
    /// Code ran and produced output, but it did not match expected.
    WrongAnswer,
    /// Execution exceeded the wall-clock / CPU limit.
    Timeout,
    /// The sandbox itself failed to execute (infra error, not the code).
    Sandbox,
    /// The problem is malformed (no test cases / no oracle to verify against).
    /// A dataset or importer defect — NOT a model failure. Kept out of the
    /// model's pass-rate denominator by callers.
    InvalidProblem,
}

/// Contamination status of a single task relative to a model's training cutoff.
/// The whole open-box thesis is "only count post-cutoff tasks" — this makes the
/// judgment explicit and auditable per task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ContaminationFlag {
    /// Task released strictly after the model's cutoff — safe to count.
    Clean,
    /// Task predates (or equals) the model's cutoff — results may be optimistic.
    Predates {
        model_cutoff: String,
        task_release: String,
    },
    /// Missing a release date or a cutoff — cannot decide; treat with caution.
    Unknown,
}

/// A `(year, month, day)` date bound, comparable by tuple ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DateBound(i32, u32, u32);

/// Which end of a coarse date's period to snap to.
#[derive(Clone, Copy)]
enum Bound {
    /// Fill missing month/day with the earliest (01-01).
    Start,
    /// Fill missing month/day with the latest (12-31).
    End,
}

/// Parse a date string (`YYYY`, `YYYY-MM`, `YYYY-MM-DD`, or an RFC3339
/// timestamp whose date prefix is one of those) into a comparable bound.
/// Returns `None` for anything unparseable — so ambiguous input becomes
/// `Unknown` rather than a confident (and possibly optimistic) guess.
///
/// Granularity is snapped conservatively for contamination: a coarse cutoff
/// snaps to the END of its period and a coarse release to the START, so a task
/// released in the same month as a month-granular cutoff is NOT counted clean.
fn parse_date_bound(s: &str, bound: Bound) -> Option<DateBound> {
    // Take the date portion (before any 'T' time or whitespace).
    let date = s.trim().split(['T', ' ']).next().unwrap_or("").trim();
    if date.is_empty() {
        return None;
    }
    let mut parts = date.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    if !(1000..=9999).contains(&year) {
        return None;
    }
    let month_str = parts.next();
    let day_str = parts.next();
    if parts.next().is_some() {
        return None; // too many components
    }

    let (month, day) = match (month_str, day_str) {
        (None, None) => match bound {
            Bound::Start => (1, 1),
            Bound::End => (12, 31),
        },
        (Some(m), None) => {
            let m: u32 = m.parse().ok()?;
            if !(1..=12).contains(&m) {
                return None;
            }
            let d = match bound {
                Bound::Start => 1,
                Bound::End => 31, // upper bound; exact month length unneeded for ordering
            };
            (m, d)
        }
        (Some(m), Some(d)) => {
            let m: u32 = m.parse().ok()?;
            let d: u32 = d.parse().ok()?;
            if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
                return None;
            }
            (m, d)
        }
        (None, Some(_)) => return None,
    };
    Some(DateBound(year, month, day))
}

/// Is `s` a date the contamination classifier can interpret (`YYYY`, `YYYY-MM`,
/// `YYYY-MM-DD`, or an RFC3339 timestamp with such a prefix)? Callers accepting
/// user-supplied cutoffs validate with this so a typo'd date fails loudly at
/// the flag instead of silently classifying every task `Unknown`.
pub fn is_parseable_cutoff(s: &str) -> bool {
    parse_date_bound(s, Bound::Start).is_some()
}

/// Classify a task's contamination status.
///
/// A task counts as **Clean** only when its release date is strictly after the
/// model's training cutoff, evaluated conservatively across date granularities
/// (see [`parse_date_bound`]): the release is snapped to the start of its
/// period and the cutoff to the end, so overlap resolves to `Predates`. Any
/// unparseable or missing date yields `Unknown` — never a confident guess.
pub fn classify_contamination(
    task_release: Option<&str>,
    model_cutoff: Option<&str>,
) -> ContaminationFlag {
    let (release, cutoff) = match (task_release, model_cutoff) {
        (Some(r), Some(c)) => (r, c),
        _ => return ContaminationFlag::Unknown,
    };
    match (
        parse_date_bound(release, Bound::Start),
        parse_date_bound(cutoff, Bound::End),
    ) {
        (Some(r), Some(c)) => {
            if r > c {
                ContaminationFlag::Clean
            } else {
                ContaminationFlag::Predates {
                    model_cutoff: cutoff.to_string(),
                    task_release: release.to_string(),
                }
            }
        }
        // Unparseable on either side → we cannot make a safe claim.
        _ => ContaminationFlag::Unknown,
    }
}

/// The per-task result: everything needed to trust (or refute) one data point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskReceipt {
    pub task_id: String,
    pub model: String,
    pub provider: String,
    /// Provider-reported model revision/snapshot when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_revision: Option<String>,
    pub sampling: Sampling,
    pub pass: bool,
    pub tests_total: usize,
    pub tests_passed: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<FailureKind>,
    /// Truncated stderr from the first failing test (for debugging, not scoring).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_excerpt: Option<String>,
    /// Wall-clock spent in the grader for this task (all tests), milliseconds.
    pub grader_ms: u64,
    pub sandbox: SandboxReport,
    pub contamination: ContaminationFlag,
}

impl TaskReceipt {
    /// Whether this receipt counts toward a model's pass rate. Malformed
    /// problems (no oracle) are dataset defects, not model outcomes, so they
    /// are excluded from the denominator.
    pub fn is_scorable(&self) -> bool {
        self.failure_kind != Some(FailureKind::InvalidProblem)
    }
}

/// The context describing which model produced a response — supplied by the
/// dispatch layer, stamped onto each receipt by the grader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunContext {
    pub model: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_cutoff: Option<String>,
    pub sampling: Sampling,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_across_field_order_and_runs() {
        let env = ExecEnv {
            os: "macos".into(),
            arch: "aarch64".into(),
            sandbox_backend: "seatbelt".into(),
            runtime_version: "Python 3.14.2".into(),
        };
        assert_eq!(env.hash(), env.hash());
        assert_eq!(env.hash().len(), 64); // hex sha-256
    }

    #[test]
    fn hash_changes_when_any_field_changes() {
        let a = ExecEnv {
            os: "macos".into(),
            arch: "aarch64".into(),
            sandbox_backend: "seatbelt".into(),
            runtime_version: "Python 3.14.2".into(),
        };
        let mut b = a.clone();
        b.sandbox_backend = "docker".into();
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn dataset_hash_reflects_task_set_and_pin() {
        let base = DatasetSnapshot {
            source: "livecodebench".into(),
            version_tag: Some("release_v6".into()),
            revision: Some("abc123".into()),
            task_ids: vec!["t1".into(), "t2".into()],
            run_date: "2026-07-01".into(),
        };
        let mut changed_pin = base.clone();
        changed_pin.revision = Some("def456".into());
        let mut changed_tasks = base.clone();
        changed_tasks.task_ids.push("t3".into());

        assert_ne!(base.hash(), changed_pin.hash());
        assert_ne!(base.hash(), changed_tasks.hash());
    }

    #[test]
    fn dataset_hash_ignores_run_date() {
        // Re-running the SAME pinned dataset on a different day must not change
        // its identity hash — the reproducibility contract.
        let a = DatasetSnapshot {
            source: "livecodebench".into(),
            version_tag: Some("release_v6".into()),
            revision: Some("abc123".into()),
            task_ids: vec!["t1".into(), "t2".into()],
            run_date: "2026-07-01".into(),
        };
        let mut b = a.clone();
        b.run_date = "2026-09-15".into();
        assert_eq!(a.hash(), b.hash());
    }

    #[test]
    fn contamination_only_clean_when_strictly_after_cutoff() {
        assert_eq!(
            classify_contamination(Some("2025-03-01"), Some("2024-08-01")),
            ContaminationFlag::Clean
        );
        // Equal date is NOT clean — could be in the training window.
        assert!(matches!(
            classify_contamination(Some("2024-08-01"), Some("2024-08-01")),
            ContaminationFlag::Predates { .. }
        ));
        assert!(matches!(
            classify_contamination(Some("2023-01-01"), Some("2024-08-01")),
            ContaminationFlag::Predates { .. }
        ));
        assert_eq!(
            classify_contamination(None, Some("2024-08-01")),
            ContaminationFlag::Unknown
        );
        assert_eq!(
            classify_contamination(Some("2025-01-01"), None),
            ContaminationFlag::Unknown
        );
    }

    #[test]
    fn contamination_handles_granularity_and_bad_input() {
        // Coarse month cutoff: a task released mid-month is NOT clean (cutoff
        // snaps to end of Aug, release stays within Aug) — the bug the two
        // reviewers caught in the old lexicographic compare.
        assert!(matches!(
            classify_contamination(Some("2024-08-15"), Some("2024-08")),
            ContaminationFlag::Predates { .. }
        ));
        // A month after the coarse cutoff IS clean.
        assert_eq!(
            classify_contamination(Some("2024-09-01"), Some("2024-08")),
            ContaminationFlag::Clean
        );
        // Mixed RFC3339 timestamp vs date-only compares on the date prefix.
        assert_eq!(
            classify_contamination(Some("2025-03-01T00:00:00Z"), Some("2024-08-01")),
            ContaminationFlag::Clean
        );
        // Year-only cutoff snaps to end-of-year; a task that same year predates.
        assert!(matches!(
            classify_contamination(Some("2024-06-01"), Some("2024")),
            ContaminationFlag::Predates { .. }
        ));
        // Unparseable input is Unknown, never a confident guess.
        assert_eq!(
            classify_contamination(Some("banana"), Some("2024-08-01")),
            ContaminationFlag::Unknown
        );
        assert_eq!(
            classify_contamination(Some("2025-13-40"), Some("2024-08-01")),
            ContaminationFlag::Unknown
        );
    }

    #[test]
    fn stable_hash_is_pinned_against_serde_feature_drift() {
        // Guards the canonical-hash contract: if `serde_json`'s `preserve_order`
        // feature ever gets unified on in this workspace (Cargo unifies features
        // globally), object key order would change and this pinned value would
        // break loudly in CI instead of silently invalidating every receipt.
        let v = serde_json::json!({"b": 1, "a": [3, 2, 1], "c": {"z": true, "y": false}});
        assert_eq!(
            stable_hash(&v),
            "f48a38ddf4a0294d766d769993f9948a68e7f62d833c0a3c05d954d0fa93cdd9"
        );
    }

    #[test]
    fn sampling_default_is_one_attempt_temp_zero() {
        let s = Sampling::default();
        assert_eq!(s.attempts, 1);
        assert_eq!(s.temperature, 0.0);
        assert!(s.seed.is_none());
    }
}
