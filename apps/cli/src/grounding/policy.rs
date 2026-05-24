// v2.9.0 PR-1 — GroundingPolicy: the compiled per-dispatch ruleset.
//
// Two key principles, both validated by the multi-LLM debate captured in
// /Users/beatriznigri/.claude/plans/witty-crafting-harp.md:
//
//   1. Deny rules and mandatory rules have OPPOSITE shapes and need
//      separate columns. Denies are negative ("agent CANNOT do X");
//      mandatories are positive ("agent MUST do X before emitting a final
//      reply"). Overloading `permissions` to carry both is the classic
//      "ACL got too clever" mistake — every consumer would have to switch
//      on rule kind anyway. We keep `agents.permissions` (already shipped
//      in v2.7.8) for denies and add `agents.mandatory_rules` for
//      obligations.
//
//   2. Dispatch tightens, never relaxes — with ONE explicit escape hatch.
//      Deny rules: dispatch cannot ever relax, only add new denies. Mode:
//      dispatch can tighten (off → soft → strict) but cannot go laxer than
//      the agent's `allowed_mode_floor`. Mandatory rules: dispatch CAN
//      skip ONE by passing `skip_mandatory: <rule_id>` with a required
//      written reason — recorded verbatim in `grounding_overrides` and
//      counted against the agent's compliance metric. No silent bypasses.
//
// PR-1 only computes the policy and serializes it onto the receipt; PR-2
// turns this struct into actual interceptor + enforcement behavior.

use serde::{Deserialize, Serialize};

/// The three enforcement levels for an agent. `off` is fully backward
/// compatible — agents created before v2.9 keep this default forever
/// until their author flips them. `soft` records what *would* have
/// happened in `strict` without blocking anything (PR-1's observability-
/// only mode). `strict` enforces (lands in PR-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroundingMode {
    /// No grounding behavior. Pre-v2.9 dispatches see exactly today's
    /// behavior. New agents created through the wizard land here until
    /// the soft-mode opt-out lands in PR-2.
    Off,
    /// Observe + record, don't block. Tool calls are audited against
    /// `mandatory_rules`; the receipt records `grounding_verdict:
    /// advisory` and lists what would have failed under strict. The
    /// pull-toward-strict comes from showing users the data on their
    /// own dispatches.
    Soft,
    /// Enforce. Denied tool calls return a structured error mid-stream;
    /// missing mandatory rules block the final reply with one retry
    /// chance. Receipt verdict is `compliant` or `violation`. Strict
    /// behavior fully wired in PR-2.
    Strict,
}

impl GroundingMode {
    /// String token persisted in the SQLite column. Must match the
    /// `ALTER TABLE agents ADD COLUMN grounding_mode TEXT NOT NULL
    /// DEFAULT 'off'` migration in schema.rs.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Soft => "soft",
            Self::Strict => "strict",
        }
    }

    /// Parse from the SQLite column value. Unrecognized strings fall back
    /// to `Off` (back-compat: a row with a future enum variant we don't
    /// understand should not break dispatch).
    pub fn parse(s: &str) -> Self {
        match s {
            "soft" => Self::Soft,
            "strict" => Self::Strict,
            _ => Self::Off,
        }
    }

    /// Ordering for tightening checks. Dispatch can override toward a
    /// higher level (off < soft < strict) but never lower than the
    /// agent's `allowed_mode_floor`. Implemented as a u8 ranking rather
    /// than `Ord` because the enum has a logical-not-alphabetical order.
    pub fn rank(&self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Soft => 1,
            Self::Strict => 2,
        }
    }

    /// Reject a dispatch-time override that would relax the policy below
    /// the agent's floor. Returns the effective mode if accepted, or an
    /// error string describing why the override was refused (string
    /// rather than a typed error to keep the CLI surface simple — the
    /// caller renders it via stderr).
    pub fn apply_override(self, override_to: Self, floor: Self) -> Result<Self, String> {
        // The agent's allowed_mode_floor sets the minimum. The override
        // proposes a new mode. The effective mode is the override only
        // if it ranks ≥ floor; otherwise the override is rejected.
        if override_to.rank() < floor.rank() {
            return Err(format!(
                "override to '{}' refused: agent's allowed_mode_floor is '{}'; \
                 dispatch can only tighten, never relax below the floor",
                override_to.as_str(),
                floor.as_str(),
            ));
        }
        // Also: the override is meaningful only if it differs from the
        // current mode AND is at least as strict. (Going laxer than the
        // current agent mode is also refused, even if the floor would
        // technically allow it — the principle "dispatch tightens only"
        // applies relative to the agent record, not just the floor.)
        if override_to.rank() < self.rank() {
            return Err(format!(
                "override to '{}' refused: agent's current grounding_mode is '{}'; \
                 dispatch tightens only — use the agent's record to relax",
                override_to.as_str(),
                self.as_str(),
            ));
        }
        Ok(override_to)
    }
}

/// One obligation: an agent MUST do this before the dispatch is marked
/// compliant. Stored as a JSON array on `agents.mandatory_rules`; the
/// receipt's `grounding_overrides` records any per-dispatch additions
/// (tightening) or skips (with reason).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MandatoryRule {
    /// Unique id within the agent's rules array — used by `skip_mandatory`
    /// to point at exactly which rule the caller wants to skip. Author
    /// provides this; the wizard auto-fills if missing (e.g. "rule-1").
    pub id: String,

    pub kind: MandatoryRuleKind,

    /// What the rule targets — meaning depends on `kind`:
    ///   - MustUseTool        → tool name ("read_file")
    ///   - MustReadPathGlob   → glob pattern ("src/auth/**")
    ///   - MustEmitMarker     → literal text the response must contain
    pub target: String,

    /// Minimum count required (e.g. `must_use_tool read_file min_count:2`
    /// requires at least 2 invocations). Default 1 if absent.
    #[serde(default = "default_min_count")]
    pub min_count: u32,

    /// Optional author-supplied note explaining why the rule exists —
    /// surfaced in the receipt when the rule is missed so the user (or
    /// AI) understands the obligation rather than just being told to
    /// re-do it.
    #[serde(default)]
    pub rationale: Option<String>,
}

fn default_min_count() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MandatoryRuleKind {
    /// Agent must call this tool at least `min_count` times.
    MustUseTool,
    /// Agent must call a read-style tool against a path matching this
    /// glob at least `min_count` times. PR-2 wires the actual matching;
    /// PR-1 only persists the rule.
    MustReadPathGlob,
    /// Agent's response text must contain this literal marker (e.g.
    /// "<CITATION_LIST>"). Useful for "always cite sources" obligations.
    MustEmitMarker,
}

/// One per-dispatch override the caller passed. Recorded verbatim on the
/// receipt in `execution_logs.grounding_overrides` so a later audit can
/// reconstruct exactly which rules applied to this specific dispatch.
/// **No silent override surface** — every override appears here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverrideAudit {
    /// Caller asked for a different mode than the agent's default.
    /// Recorded whether it was accepted (effective != original) or
    /// rejected (refused string captured).
    ModeOverride {
        from: GroundingMode,
        to: GroundingMode,
        floor: GroundingMode,
        effective: GroundingMode,
        refused: Option<String>,
    },
    /// Caller added per-dispatch denies on top of the agent's. Tightens
    /// only — always accepted.
    AdditionalDenies { added: Vec<String> },
    /// Caller added per-dispatch mandatory rules on top of the agent's.
    /// Tightens only — always accepted.
    AdditionalMandatories { added: Vec<MandatoryRule> },
    /// Caller chose to skip ONE mandatory rule with a written reason.
    /// Counts against the agent's compliance metric.
    SkipMandatory {
        rule_id: String,
        reason: String,
    },
    /// Caller asked for a dry-run — the response was generated through
    /// the policy compilation but the runtime was NOT invoked. Useful
    /// for "preview what tools the agent will be allowed to call".
    DryRun,
}

/// The compiled policy for one dispatch. Built by combining the agent's
/// record fields with any caller-supplied overrides, then handed to the
/// per-runtime dispatch path. In PR-1 this struct is consumed only by
/// the soft-mode prompt prepend + receipt write; PR-2 plumbs it into the
/// per-runtime interceptor (`--allowedTools` for Claude, function-call
/// loop for API providers, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingPolicy {
    pub mode: GroundingMode,
    /// Effective deny rules after the agent's denies + any caller-added
    /// `additional_denies`. Format mirrors `agents.permissions` JSON
    /// strings ("deny:Bash(rm:*)") for v2.7.8 compatibility.
    pub denies: Vec<String>,
    /// Effective mandatory rules after the agent's mandatories + any
    /// caller-added `additional_mandatories` MINUS any
    /// `skip_mandatory` entries (those still appear in
    /// `overrides_audit` so the receipt records the skip).
    pub mandatories: Vec<MandatoryRule>,
    /// The audit trail of caller-supplied overrides — recorded verbatim
    /// on the receipt regardless of whether they were accepted.
    pub overrides_audit: Vec<OverrideAudit>,
}

impl GroundingPolicy {
    /// Compose a policy from an agent's record + per-dispatch overrides.
    ///
    /// `record_mode` / `record_floor` / `record_denies` / `record_mandatories`
    /// come from the agents table. The `override_*` params come from the
    /// dispatch CLI flags (or the MCP `run_agent` params). Returns the
    /// compiled policy or an error if any override violates the
    /// tighten-only invariant.
    pub fn compose(
        record_mode: GroundingMode,
        record_floor: GroundingMode,
        record_denies: Vec<String>,
        record_mandatories: Vec<MandatoryRule>,
        override_mode: Option<GroundingMode>,
        override_denies: Vec<String>,
        override_mandatories: Vec<MandatoryRule>,
        skip_mandatory: Option<(String, String)>, // (rule_id, reason)
        dry_run: bool,
    ) -> Result<Self, String> {
        let mut audit: Vec<OverrideAudit> = Vec::new();

        // Resolve effective mode (tighten-only check).
        let effective_mode = match override_mode {
            Some(req) => match record_mode.apply_override(req, record_floor) {
                Ok(m) => {
                    audit.push(OverrideAudit::ModeOverride {
                        from: record_mode,
                        to: req,
                        floor: record_floor,
                        effective: m,
                        refused: None,
                    });
                    m
                }
                Err(reason) => {
                    audit.push(OverrideAudit::ModeOverride {
                        from: record_mode,
                        to: req,
                        floor: record_floor,
                        effective: record_mode,
                        refused: Some(reason.clone()),
                    });
                    // Refused overrides do NOT block the dispatch — the
                    // record's mode applies and the audit records the
                    // refusal so the caller sees why their override
                    // didn't stick.
                    record_mode
                }
            },
            None => record_mode,
        };

        // Compose denies — record + additional (tightening always
        // accepted). Dedupe so the same rule appearing in both lists
        // doesn't show twice.
        let mut denies: Vec<String> = record_denies.clone();
        if !override_denies.is_empty() {
            let added: Vec<String> = override_denies
                .iter()
                .filter(|d| !record_denies.contains(d))
                .cloned()
                .collect();
            if !added.is_empty() {
                audit.push(OverrideAudit::AdditionalDenies {
                    added: added.clone(),
                });
                denies.extend(added);
            }
        }

        // Compose mandatories — record + additional, then subtract
        // skip_mandatory (which is recorded as audit, NOT removed from
        // record audit).
        let mut mandatories: Vec<MandatoryRule> = record_mandatories.clone();
        if !override_mandatories.is_empty() {
            let added: Vec<MandatoryRule> = override_mandatories
                .iter()
                .filter(|m| !record_mandatories.iter().any(|r| r.id == m.id))
                .cloned()
                .collect();
            if !added.is_empty() {
                audit.push(OverrideAudit::AdditionalMandatories {
                    added: added.clone(),
                });
                mandatories.extend(added);
            }
        }

        if let Some((rule_id, reason)) = skip_mandatory {
            audit.push(OverrideAudit::SkipMandatory {
                rule_id: rule_id.clone(),
                reason,
            });
            mandatories.retain(|m| m.id != rule_id);
        }

        if dry_run {
            audit.push(OverrideAudit::DryRun);
        }

        Ok(Self {
            mode: effective_mode,
            denies,
            mandatories,
            overrides_audit: audit,
        })
    }

    /// Serialize the override audit for `execution_logs.grounding_overrides`.
    /// Returns None if there were no overrides (so the column stays NULL
    /// rather than carrying an empty array, keeping receipt queries lean).
    pub fn overrides_json(&self) -> Option<String> {
        if self.overrides_audit.is_empty() {
            None
        } else {
            serde_json::to_string(&self.overrides_audit).ok()
        }
    }

    /// The system-prompt note prepended by soft mode. Lists the
    /// mandatory rules as expected behavior; the agent isn't blocked if
    /// it skips them, but the receipt records the omission. Empty
    /// string when mode is Off (no prepend).
    pub fn soft_mode_prompt_prepend(&self) -> String {
        if self.mode == GroundingMode::Off {
            return String::new();
        }

        let mut out = String::new();
        out.push_str("## Grounding policy (ATO)\n\n");
        out.push_str(match self.mode {
            GroundingMode::Off => "Mode: off — no enforcement.\n",
            GroundingMode::Soft => {
                "Mode: soft — your tool calls and obligations are audited \
                 and surfaced on the receipt. Nothing will block, but the \
                 verdict on this dispatch reflects whether you followed \
                 the rules below.\n"
            }
            GroundingMode::Strict => {
                "Mode: strict — denied tools return a structured error \
                 mid-stream; failing to satisfy a mandatory rule blocks \
                 the final reply with one retry chance.\n"
            }
        });

        if !self.denies.is_empty() {
            out.push_str("\nDeny rules:\n");
            for d in &self.denies {
                out.push_str(&format!("  - {}\n", d));
            }
        }

        if !self.mandatories.is_empty() {
            out.push_str("\nMandatory obligations (must satisfy before emitting final reply):\n");
            for m in &self.mandatories {
                let target = &m.target;
                let kind = match m.kind {
                    MandatoryRuleKind::MustUseTool => "must call tool",
                    MandatoryRuleKind::MustReadPathGlob => "must read path matching",
                    MandatoryRuleKind::MustEmitMarker => "response must contain marker",
                };
                let rationale = m
                    .rationale
                    .as_deref()
                    .map(|r| format!(" — {}", r))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "  - [{}] {} `{}` (min_count: {}){}\n",
                    m.id, kind, target, m.min_count, rationale
                ));
            }
        }

        out.push('\n');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rule(id: &str, kind: MandatoryRuleKind, target: &str) -> MandatoryRule {
        MandatoryRule {
            id: id.to_string(),
            kind,
            target: target.to_string(),
            min_count: 1,
            rationale: None,
        }
    }

    #[test]
    fn mode_override_tighten_accepted() {
        let result = GroundingMode::Off.apply_override(GroundingMode::Strict, GroundingMode::Off);
        assert_eq!(result, Ok(GroundingMode::Strict));
    }

    #[test]
    fn mode_override_relax_below_floor_refused() {
        let result = GroundingMode::Strict.apply_override(GroundingMode::Off, GroundingMode::Soft);
        assert!(result.is_err(), "off < soft floor — must refuse");
        let err = result.unwrap_err();
        assert!(
            err.contains("allowed_mode_floor"),
            "refusal must mention the floor: {}",
            err
        );
    }

    #[test]
    fn mode_override_relax_below_record_refused_even_if_floor_allows() {
        // Floor is Off, record is Strict. Override to Soft would be
        // allowed by floor alone, but tighten-only relative to the
        // RECORD's current mode says no.
        let result = GroundingMode::Strict.apply_override(GroundingMode::Soft, GroundingMode::Off);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("tightens only"),
            "refusal must cite the tighten-only principle: {}",
            err
        );
    }

    #[test]
    fn compose_records_dry_run_audit() {
        let policy = GroundingPolicy::compose(
            GroundingMode::Soft,
            GroundingMode::Off,
            vec![],
            vec![],
            None,
            vec![],
            vec![],
            None,
            true, // dry_run
        )
        .expect("compose");
        assert_eq!(policy.mode, GroundingMode::Soft);
        assert_eq!(policy.overrides_audit.len(), 1);
        assert!(matches!(
            policy.overrides_audit[0],
            OverrideAudit::DryRun
        ));
    }

    #[test]
    fn compose_skip_mandatory_removes_rule_but_keeps_audit() {
        let policy = GroundingPolicy::compose(
            GroundingMode::Strict,
            GroundingMode::Soft,
            vec![],
            vec![
                make_rule("r1", MandatoryRuleKind::MustUseTool, "read_file"),
                make_rule("r2", MandatoryRuleKind::MustUseTool, "grep"),
            ],
            None,
            vec![],
            vec![],
            Some(("r1".to_string(), "single-file diff, no read needed".to_string())),
            false,
        )
        .expect("compose");
        // The skipped rule is gone from the effective list...
        assert_eq!(policy.mandatories.len(), 1);
        assert_eq!(policy.mandatories[0].id, "r2");
        // ...but the audit records it with the reason verbatim.
        let skip_audits: Vec<&OverrideAudit> = policy
            .overrides_audit
            .iter()
            .filter(|a| matches!(a, OverrideAudit::SkipMandatory { .. }))
            .collect();
        assert_eq!(skip_audits.len(), 1);
        if let OverrideAudit::SkipMandatory { rule_id, reason } = skip_audits[0] {
            assert_eq!(rule_id, "r1");
            assert!(reason.contains("single-file"));
        } else {
            unreachable!()
        }
    }

    #[test]
    fn compose_refused_mode_override_appears_in_audit_with_reason() {
        // Try to relax from Strict→Off; the floor is Soft. Refused.
        // Policy still composes (effective stays Strict) and the audit
        // captures the refusal.
        let policy = GroundingPolicy::compose(
            GroundingMode::Strict,
            GroundingMode::Soft,
            vec![],
            vec![],
            Some(GroundingMode::Off),
            vec![],
            vec![],
            None,
            false,
        )
        .expect("compose");
        assert_eq!(
            policy.mode,
            GroundingMode::Strict,
            "record's mode applies when override refused"
        );
        let refused: Vec<_> = policy
            .overrides_audit
            .iter()
            .filter_map(|a| match a {
                OverrideAudit::ModeOverride { refused, .. } => refused.as_ref(),
                _ => None,
            })
            .collect();
        assert_eq!(refused.len(), 1);
        assert!(refused[0].contains("refused"));
    }

    #[test]
    fn soft_mode_prompt_prepend_lists_mandatories() {
        let policy = GroundingPolicy::compose(
            GroundingMode::Soft,
            GroundingMode::Off,
            vec!["deny:Write".to_string()],
            vec![
                MandatoryRule {
                    id: "r1".to_string(),
                    kind: MandatoryRuleKind::MustUseTool,
                    target: "read_file".to_string(),
                    min_count: 2,
                    rationale: Some("walk the live repo before flagging".to_string()),
                },
            ],
            None,
            vec![],
            vec![],
            None,
            false,
        )
        .expect("compose");
        let prepend = policy.soft_mode_prompt_prepend();
        assert!(prepend.contains("Grounding policy"));
        assert!(prepend.contains("soft"));
        assert!(prepend.contains("read_file"));
        assert!(prepend.contains("min_count: 2"));
        assert!(prepend.contains("walk the live repo"));
        assert!(prepend.contains("deny:Write"));
    }

    #[test]
    fn off_mode_prompt_prepend_is_empty() {
        let policy = GroundingPolicy::compose(
            GroundingMode::Off,
            GroundingMode::Off,
            vec![],
            vec![],
            None,
            vec![],
            vec![],
            None,
            false,
        )
        .expect("compose");
        assert_eq!(policy.soft_mode_prompt_prepend(), "");
    }

    #[test]
    fn parse_unknown_mode_falls_back_to_off() {
        // Forward-compat: a future enum variant from a newer schema
        // version should not break dispatch — fall back to Off.
        assert_eq!(GroundingMode::parse("future-variant"), GroundingMode::Off);
        assert_eq!(GroundingMode::parse(""), GroundingMode::Off);
        assert_eq!(GroundingMode::parse("strict"), GroundingMode::Strict);
        assert_eq!(GroundingMode::parse("soft"), GroundingMode::Soft);
    }

    #[test]
    fn overrides_json_returns_none_when_no_overrides() {
        let policy = GroundingPolicy::compose(
            GroundingMode::Soft,
            GroundingMode::Off,
            vec![],
            vec![],
            None,
            vec![],
            vec![],
            None,
            false,
        )
        .expect("compose");
        assert_eq!(policy.overrides_json(), None);
    }
}
