// The scorecard — the composed "open box" for one model over one pinned dataset.
//
// The receipt module defines the ingredients (dataset snapshot, harness config,
// exec env, per-task receipts, Wilson stats); this binds them into the single
// artifact `ato bench run` emits and re-runs compare against. Every input that
// affects the score is hashed and travels with it, so a re-run is verifiable:
// matching dataset + harness hashes and an overlapping pass-rate CI == reproduced.
//
// Crucially, the headline number is the CONTAMINATION-CLEAN pass rate — the
// thesis is "only count tasks released after the model's cutoff", so the flag
// each grader stamps onto a receipt actually gates the denominator here.

use crate::receipt::{ContaminationFlag, DatasetSnapshot, ExecEnv, HarnessConfig, TaskReceipt};
use crate::stats::{wilson_interval, WilsonInterval};
use serde::{Deserialize, Serialize};

/// One model's results over one pinned dataset, with the full reproducibility
/// receipt attached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scorecard {
    pub model: String,
    pub provider: String,
    pub dataset: DatasetSnapshot,
    pub harness: HarnessConfig,
    pub env: ExecEnv,
    /// The training cutoff used to classify contamination for this run, with
    /// its provenance — the headline (contamination-clean) number is only
    /// auditable if the reader can see which cutoff gated the denominator and
    /// where that date came from. `None` = no cutoff known → every task
    /// classifies `Unknown` (and the headline honestly reads n/a).
    ///
    /// Deliberately OUTSIDE the dataset/harness/env hashes: the cutoff changes
    /// which tasks *count*, not how any task was measured. Two runs with equal
    /// hashes and different cutoffs share raw receipts but headline different
    /// subsets — the field makes that visible instead of hash-breaking it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_cutoff: Option<ModelCutoffInfo>,
    pub receipts: Vec<TaskReceipt>,
}

/// Where a model-cutoff date came from. `Registry` = ato-bench's cited,
/// vendor-stated table; `User` = supplied on the command line (trusted as an
/// explicit operator claim and labelled as such on the scorecard).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutoffOrigin {
    Registry,
    User,
}

/// The cutoff actually applied to a run, carried on the scorecard for audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCutoffInfo {
    /// The date, vendor granularity kept (e.g. "2026-01" or "2024-09-30").
    pub cutoff: String,
    /// "training_data" or "knowledge" — as the vendor states it. Empty-string
    /// only for user-supplied dates with no stated kind.
    pub kind: String,
    /// Vendor URL that states the date (registry) or "" (user-supplied).
    pub source: String,
    pub origin: CutoffOrigin,
}

/// How the task set breaks down by contamination status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContaminationSummary {
    pub clean: usize,
    pub predates: usize,
    pub unknown: usize,
}

impl ContaminationSummary {
    /// True if any counted task predates the cutoff — the scorecard should warn
    /// loudly when this holds ("N tasks predate the model's cutoff").
    pub fn has_overlap(&self) -> bool {
        self.predates > 0
    }
}

impl Scorecard {
    pub fn dataset_hash(&self) -> String {
        self.dataset.hash()
    }
    pub fn harness_hash(&self) -> String {
        self.harness.hash()
    }
    pub fn env_hash(&self) -> String {
        self.env.hash()
    }

    /// Pass rate over all SCORABLE tasks (excludes malformed/no-oracle
    /// problems, which are dataset defects rather than model outcomes).
    pub fn pass_rate(&self, z: f64) -> WilsonInterval {
        let scorable = self.receipts.iter().filter(|r| r.is_scorable());
        let n = scorable.clone().count() as u64;
        let passes = scorable.filter(|r| r.pass).count() as u64;
        wilson_interval(passes, n, z)
    }

    /// Pass rate over CONTAMINATION-CLEAN, scorable tasks only — the headline,
    /// trustworthy metric. This is what the open-box thesis stands behind.
    pub fn clean_pass_rate(&self, z: f64) -> WilsonInterval {
        let clean = self
            .receipts
            .iter()
            .filter(|r| r.is_scorable() && matches!(r.contamination, ContaminationFlag::Clean));
        let n = clean.clone().count() as u64;
        let passes = clean.filter(|r| r.pass).count() as u64;
        wilson_interval(passes, n, z)
    }

    pub fn contamination_summary(&self) -> ContaminationSummary {
        let mut s = ContaminationSummary {
            clean: 0,
            predates: 0,
            unknown: 0,
        };
        for r in &self.receipts {
            match r.contamination {
                ContaminationFlag::Clean => s.clean += 1,
                ContaminationFlag::Predates { .. } => s.predates += 1,
                ContaminationFlag::Unknown => s.unknown += 1,
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::{FailureKind, Sampling};
    use crate::sandbox::SandboxReport;

    fn sandbox_report() -> SandboxReport {
        SandboxReport {
            backend: "seatbelt".into(),
            network_isolated: true,
            filesystem_isolated: true,
            resource_limited: true,
            note: None,
        }
    }

    fn receipt(id: &str, pass: bool, contamination: ContaminationFlag) -> TaskReceipt {
        TaskReceipt {
            task_id: id.into(),
            model: "m".into(),
            provider: "p".into(),
            model_revision: None,
            sampling: Sampling::default(),
            pass,
            tests_total: 2,
            tests_passed: if pass { 2 } else { 1 },
            failure_kind: if pass {
                None
            } else {
                Some(FailureKind::WrongAnswer)
            },
            stderr_excerpt: None,
            grader_ms: 5,
            sandbox: sandbox_report(),
            contamination,
        }
    }

    fn invalid(id: &str) -> TaskReceipt {
        let mut r = receipt(id, false, ContaminationFlag::Clean);
        r.failure_kind = Some(FailureKind::InvalidProblem);
        r.tests_total = 0;
        r
    }

    fn scorecard(receipts: Vec<TaskReceipt>) -> Scorecard {
        Scorecard {
            model: "m".into(),
            provider: "p".into(),
            dataset: DatasetSnapshot {
                source: "synthetic".into(),
                version_tag: None,
                revision: None,
                task_ids: receipts.iter().map(|r| r.task_id.clone()).collect(),
                run_date: "2026-07-01".into(),
            },
            harness: HarnessConfig {
                system_prompt: None,
                prompt_wrapper: "{{problem}}".into(),
                stop_tokens: vec![],
                sampling: Sampling::default(),
                grader: crate::grader::GraderConfig::default(),
            },
            env: ExecEnv {
                os: "macos".into(),
                arch: "aarch64".into(),
                sandbox_backend: "seatbelt".into(),
                runtime_version: "Python 3.14.2".into(),
            },
            model_cutoff: None,
            receipts,
        }
    }

    #[test]
    fn clean_pass_rate_counts_only_clean_scorable_tasks() {
        let sc = scorecard(vec![
            receipt("a", true, ContaminationFlag::Clean),
            receipt("b", false, ContaminationFlag::Clean),
            // predates: excluded from the clean denominator
            receipt(
                "c",
                true,
                ContaminationFlag::Predates {
                    model_cutoff: "2024-08".into(),
                    task_release: "2024-01".into(),
                },
            ),
            // unknown: excluded from clean denominator
            receipt("d", true, ContaminationFlag::Unknown),
            // invalid problem: excluded from BOTH denominators
            invalid("e"),
        ]);

        // Clean denominator = a,b only → 1/2.
        let clean = sc.clean_pass_rate(crate::stats::Z_95);
        assert_eq!((clean.passes, clean.n), (1, 2));

        // Overall scorable denominator = a,b,c,d (not e) → 3/4.
        let all = sc.pass_rate(crate::stats::Z_95);
        assert_eq!((all.passes, all.n), (3, 4));

        let cs = sc.contamination_summary();
        assert_eq!((cs.clean, cs.predates, cs.unknown), (3, 1, 1));
        assert!(cs.has_overlap());
    }

    #[test]
    fn hashes_are_exposed_and_stable() {
        let sc = scorecard(vec![receipt("a", true, ContaminationFlag::Clean)]);
        assert_eq!(sc.dataset_hash(), sc.dataset.hash());
        assert_eq!(sc.harness_hash().len(), 64);
    }

    #[test]
    fn scorecard_without_cutoff_field_still_deserializes() {
        // Receipts written by pre-cutoff-registry binaries lack `model_cutoff`;
        // they must keep loading (as None) — receipts are durable artifacts.
        let sc = scorecard(vec![receipt("a", true, ContaminationFlag::Clean)]);
        let mut json = serde_json::to_value(&sc).unwrap();
        json.as_object_mut().unwrap().remove("model_cutoff");
        let back: Scorecard = serde_json::from_value(json).unwrap();
        assert!(back.model_cutoff.is_none());

        let with = Scorecard {
            model_cutoff: Some(ModelCutoffInfo {
                cutoff: "2025-01".into(),
                kind: "knowledge".into(),
                source: "https://example.test/models".into(),
                origin: CutoffOrigin::Registry,
            }),
            ..sc
        };
        let round: Scorecard =
            serde_json::from_str(&serde_json::to_string(&with).unwrap()).unwrap();
        assert_eq!(round.model_cutoff, with.model_cutoff);
    }
}
