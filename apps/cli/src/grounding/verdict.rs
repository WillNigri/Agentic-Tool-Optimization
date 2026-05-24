// v2.9.0 PR-1 — verdict computation: did this dispatch follow the rules?
//
// Given a compiled `GroundingPolicy` and the list of tool calls actually
// observed during the dispatch, compute the verdict that goes onto the
// receipt's `grounding_verdict` column. Outcome ladder:
//
//   not_enforced  — agent's mode is Off (or grounding wasn't compiled
//                    for this dispatch, e.g. cold legacy callers).
//   advisory      — mode is Soft. Verdict records what would have
//                    failed; nothing blocks. Default state for v2.9
//                    new agents until PR-2 wires strict enforcement.
//   compliant     — mode is Strict and every mandatory rule was
//                    satisfied (no denies were attempted either,
//                    though deny-attempts surface separately in PR-2's
//                    structured-error path).
//   violation     — mode is Strict and at least one mandatory rule was
//                    NOT satisfied OR at least one denied tool was
//                    attempted. Receipt rendering shows the specific
//                    rules that failed.
//
// PR-1 only computes verdicts for the SOFT path (advisory). PR-2 will
// extend this same function for strict (compliant / violation) once the
// per-runtime interceptor lands. Keeping the function shape stable now
// means downstream callers (`ato dispatches show`, the GUI Insights
// panel) can read the column with one code path that works for all
// modes today and forward-compatible with PR-2's stricter outcomes.

use serde::{Deserialize, Serialize};

use super::policy::{GroundingMode, GroundingPolicy, MandatoryRuleKind};

/// A single tool call observed during the dispatch. Compatible with
/// the existing `tool_calls_summary` JSON shipped in v2.4.5 — the
/// dispatch path already records these; PR-1 just adds the verdict
/// computation on top.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallObservation {
    pub name: String,
    /// Brief stringified args ("read_file('src/auth.ts')") for the
    /// receipt UI. Not used for verdict math.
    #[serde(default)]
    pub args_brief: Option<String>,
    /// If the runtime indicates the tool errored. Counted as an
    /// attempt for "did the agent use the tool" purposes — PR-2 will
    /// refine this for the strict path.
    #[serde(default)]
    pub is_error: bool,
}

/// The compact verdict written to `execution_logs.grounding_verdict`.
/// String tokens match the column values; deserializing future tokens
/// we don't recognize falls back to NotEnforced (forward-compat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroundingVerdict {
    NotEnforced,
    Advisory,
    Compliant,
    Violation,
}

impl GroundingVerdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotEnforced => "not_enforced",
            Self::Advisory => "advisory",
            Self::Compliant => "compliant",
            Self::Violation => "violation",
        }
    }
}

/// Detail rendered next to the verdict on the receipt — surfaces which
/// specific rules failed (for advisory + violation verdicts) so the
/// caller knows what to fix.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VerdictDetail {
    pub verdict: Option<GroundingVerdict>,
    /// Rule ids that were not satisfied. Empty when verdict is
    /// NotEnforced / Compliant.
    pub unmet_rules: Vec<UnmetRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmetRule {
    pub rule_id: String,
    pub kind: MandatoryRuleKind,
    pub target: String,
    pub observed_count: u32,
    pub required_count: u32,
}

/// Run the verdict computation. `tool_calls` is the parsed
/// `tool_calls_summary` for this dispatch (may be empty). `response_text`
/// is the assistant's reply text (used only by `MustEmitMarker` rules).
///
/// Returns (verdict, detail). The caller writes verdict.as_str() to
/// `execution_logs.grounding_verdict`; detail is rendered on the receipt
/// when the verdict is Advisory or Violation.
pub fn compile_verdict(
    policy: &GroundingPolicy,
    tool_calls: &[ToolCallObservation],
    response_text: &str,
) -> (GroundingVerdict, VerdictDetail) {
    if policy.mode == GroundingMode::Off {
        return (
            GroundingVerdict::NotEnforced,
            VerdictDetail::default(),
        );
    }

    // Tally tool calls by name once so the rule loop is O(rules) not
    // O(rules × calls).
    use std::collections::HashMap;
    let mut tool_counts: HashMap<&str, u32> = HashMap::new();
    for call in tool_calls {
        *tool_counts.entry(call.name.as_str()).or_insert(0) += 1;
    }

    let mut unmet: Vec<UnmetRule> = Vec::new();

    for rule in &policy.mandatories {
        let observed_count = match rule.kind {
            MandatoryRuleKind::MustUseTool => {
                *tool_counts.get(rule.target.as_str()).unwrap_or(&0)
            }
            MandatoryRuleKind::MustReadPathGlob => {
                // PR-1: only check whether ANY read-style call argument
                // contains the glob's literal stem. Real glob matching
                // lands in PR-2 alongside the interceptor that has
                // structured tool-call args. For now: substring match
                // against args_brief is the conservative observability
                // pass — it underreports (a strict-match miss won't
                // false-positive) but never over-claims compliance.
                let stem = rule.target.trim_end_matches("/**").trim_end_matches("/*");
                tool_calls
                    .iter()
                    .filter(|c| {
                        matches!(c.name.as_str(), "read_file" | "read" | "cat" | "view")
                            && c.args_brief
                                .as_deref()
                                .map(|a| a.contains(stem))
                                .unwrap_or(false)
                    })
                    .count() as u32
            }
            MandatoryRuleKind::MustEmitMarker => {
                if response_text.contains(rule.target.as_str()) {
                    rule.min_count
                } else {
                    0
                }
            }
        };

        if observed_count < rule.min_count {
            unmet.push(UnmetRule {
                rule_id: rule.id.clone(),
                kind: rule.kind,
                target: rule.target.clone(),
                observed_count,
                required_count: rule.min_count,
            });
        }
    }

    let verdict = match (policy.mode, unmet.is_empty()) {
        (GroundingMode::Soft, true) => GroundingVerdict::Advisory, // Observed, all met — still advisory in soft.
        (GroundingMode::Soft, false) => GroundingVerdict::Advisory, // Observed, some unmet — still advisory.
        (GroundingMode::Strict, true) => GroundingVerdict::Compliant,
        (GroundingMode::Strict, false) => GroundingVerdict::Violation,
        (GroundingMode::Off, _) => GroundingVerdict::NotEnforced, // unreachable due to early return
    };

    (
        verdict,
        VerdictDetail {
            verdict: Some(verdict),
            unmet_rules: unmet,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::policy::{GroundingPolicy, MandatoryRule};

    fn tool(name: &str, args: Option<&str>) -> ToolCallObservation {
        ToolCallObservation {
            name: name.to_string(),
            args_brief: args.map(|s| s.to_string()),
            is_error: false,
        }
    }

    fn mandatory_rule(id: &str, kind: MandatoryRuleKind, target: &str, min: u32) -> MandatoryRule {
        MandatoryRule {
            id: id.to_string(),
            kind,
            target: target.to_string(),
            min_count: min,
            rationale: None,
        }
    }

    fn build_policy(mode: GroundingMode, mandatories: Vec<MandatoryRule>) -> GroundingPolicy {
        GroundingPolicy::compose(
            mode,
            GroundingMode::Off,
            vec![],
            mandatories,
            None,
            vec![],
            vec![],
            None,
            false,
        )
        .expect("compose")
    }

    #[test]
    fn off_mode_always_not_enforced_regardless_of_calls() {
        let policy = build_policy(GroundingMode::Off, vec![]);
        let (v, d) = compile_verdict(&policy, &[tool("read_file", None)], "any reply");
        assert_eq!(v, GroundingVerdict::NotEnforced);
        assert!(d.unmet_rules.is_empty());
    }

    #[test]
    fn soft_mode_with_all_rules_met_is_advisory_not_compliant() {
        // PR-1 design: soft mode never says "compliant" because nothing
        // was actually enforced. Compliant requires strict + clean.
        let policy = build_policy(
            GroundingMode::Soft,
            vec![mandatory_rule(
                "r1",
                MandatoryRuleKind::MustUseTool,
                "read_file",
                1,
            )],
        );
        let (v, d) = compile_verdict(&policy, &[tool("read_file", Some("src/auth.ts"))], "");
        assert_eq!(v, GroundingVerdict::Advisory);
        assert!(
            d.unmet_rules.is_empty(),
            "rule was met — no unmet entries"
        );
    }

    #[test]
    fn soft_mode_with_missing_rule_is_advisory_with_unmet_detail() {
        let policy = build_policy(
            GroundingMode::Soft,
            vec![mandatory_rule(
                "r1",
                MandatoryRuleKind::MustUseTool,
                "read_file",
                2,
            )],
        );
        let (v, d) = compile_verdict(
            &policy,
            &[tool("read_file", Some("only-one.ts"))],
            "",
        );
        assert_eq!(v, GroundingVerdict::Advisory);
        assert_eq!(d.unmet_rules.len(), 1);
        assert_eq!(d.unmet_rules[0].rule_id, "r1");
        assert_eq!(d.unmet_rules[0].observed_count, 1);
        assert_eq!(d.unmet_rules[0].required_count, 2);
    }

    #[test]
    fn strict_mode_all_rules_met_is_compliant() {
        let policy = build_policy(
            GroundingMode::Strict,
            vec![
                mandatory_rule("r1", MandatoryRuleKind::MustUseTool, "read_file", 1),
                mandatory_rule("r2", MandatoryRuleKind::MustUseTool, "grep", 1),
            ],
        );
        let (v, _) = compile_verdict(
            &policy,
            &[tool("read_file", None), tool("grep", None)],
            "",
        );
        assert_eq!(v, GroundingVerdict::Compliant);
    }

    #[test]
    fn strict_mode_missing_rule_is_violation() {
        let policy = build_policy(
            GroundingMode::Strict,
            vec![
                mandatory_rule("r1", MandatoryRuleKind::MustUseTool, "read_file", 1),
                mandatory_rule("r2", MandatoryRuleKind::MustUseTool, "grep", 1),
            ],
        );
        let (v, d) = compile_verdict(&policy, &[tool("read_file", None)], "");
        assert_eq!(v, GroundingVerdict::Violation);
        assert_eq!(d.unmet_rules.len(), 1);
        assert_eq!(d.unmet_rules[0].rule_id, "r2");
    }

    #[test]
    fn must_emit_marker_checks_response_text() {
        let policy = build_policy(
            GroundingMode::Soft,
            vec![mandatory_rule(
                "r1",
                MandatoryRuleKind::MustEmitMarker,
                "<CITATION_LIST>",
                1,
            )],
        );
        let (_, d_missing) = compile_verdict(&policy, &[], "answer without citations");
        assert_eq!(d_missing.unmet_rules.len(), 1);
        let (_, d_present) = compile_verdict(
            &policy,
            &[],
            "answer with sources <CITATION_LIST>[doc.md]</CITATION_LIST>",
        );
        assert_eq!(d_present.unmet_rules.len(), 0);
    }

    #[test]
    fn must_read_path_glob_substring_match() {
        let policy = build_policy(
            GroundingMode::Soft,
            vec![mandatory_rule(
                "r1",
                MandatoryRuleKind::MustReadPathGlob,
                "src/auth/**",
                1,
            )],
        );
        // PR-1: conservative substring match against args_brief stem.
        let (_, d_hit) = compile_verdict(
            &policy,
            &[tool("read_file", Some("read_file('src/auth/session.ts')"))],
            "",
        );
        assert_eq!(d_hit.unmet_rules.len(), 0);
        let (_, d_miss) = compile_verdict(
            &policy,
            &[tool("read_file", Some("read_file('src/db/queries.ts')"))],
            "",
        );
        assert_eq!(d_miss.unmet_rules.len(), 1);
    }
}
