// v2.9.0 PR-1 — derive `badges` array for AI-facing listings.
//
// Empirically validated by the 3-format A/B/C test in
// /Users/beatriznigri/.claude/plans/witty-crafting-harp.md (and receipts
// at /tmp/grounded-mode-receipts/): LLMs picked the same agents across
// all three encodings, so the format choice is low-stakes for
// discrimination. We ship `badges` because it's slug-stable, derived
// from structured fields (one source of truth, no drift between
// description text and policy fields), and queryable (`WHERE 'strict'
// IN badges` is a real SQL question the GUI Insights panel asks).
//
// Vocabulary is frozen at v2.9 so MCP clients can match against it:
//
//   off / soft / strict      - grounding_mode
//   mode-locked              - allowed_mode_floor ≥ grounding_mode
//   tools-required           - mandatory_rules has a MustUseTool
//   read-only                - permissions denies all write tools
//   write-enabled            - permissions allows ≥1 write tool
//   hitl-gated               - any rule with on_miss: hitl_approval
//   served                   - this agent record is wrapped by
//                              `ato agents serve` (PR-4)
//
// PR-1 only exposes the badges that derive from already-shipped fields
// (grounding_mode, allowed_mode_floor) plus the new mandatory_rules.
// `read-only` / `write-enabled` derive from agents.permissions JSON.
// `hitl-gated` and `served` ship empty in PR-1 — they appear after
// PR-2 wires the on_miss surface and PR-4 ships agents serve.

use super::policy::{GroundingMode, MandatoryRule, MandatoryRuleKind};

/// Compute the badges array from an agent's structured fields. The
/// input is whatever the caller has loaded — typically from one SELECT
/// against `agents`. Output is the deduped, ordered list ready to be
/// returned in `list_agents` / rendered in the GUI / printed by CLI.
pub fn derive_badges(
    grounding_mode: GroundingMode,
    allowed_mode_floor: GroundingMode,
    mandatory_rules: &[MandatoryRule],
    permissions: &[String],
) -> Vec<String> {
    let mut badges: Vec<String> = Vec::new();

    // Mode badge is always first — it's the load-bearing signal an AI
    // scans to decide whether the agent will enforce its rules.
    badges.push(grounding_mode.as_str().to_string());

    // Mode-locked: the dispatch can't override below the floor. Surfaces
    // when an agent is "permanently" at its mode regardless of caller.
    // Most relevant for `ato agents serve`-deployed agents where end
    // users can't override.
    if allowed_mode_floor.rank() >= grounding_mode.rank()
        && grounding_mode != GroundingMode::Off
    {
        badges.push("mode-locked".to_string());
    }

    // Tools-required: agent has at least one MustUseTool rule. The
    // empirical test (see plan) showed this is what LLMs latch onto
    // when deciding whether to walk the code vs. answer from priors.
    if mandatory_rules
        .iter()
        .any(|r| matches!(r.kind, MandatoryRuleKind::MustUseTool))
    {
        badges.push("tools-required".to_string());
    }

    // Read-only vs write-enabled: derived from agents.permissions. The
    // permission strings follow the v2.7.8 convention
    // ("allow:Bash(ato:*)", "deny:Write", "deny:Bash(rm:*)"). A
    // conservative classifier:
    //   - read-only: at least one deny on a write-capable tool, and no
    //                explicit allow on writes.
    //   - write-enabled: explicit allow on Write / Edit / Bash that
    //                    isn't a narrow read-style command.
    // Anything ambiguous (no permissions, mixed signals) gets neither
    // badge — the AI has to look at the structured permissions list.
    let is_read_only = permissions
        .iter()
        .any(|p| {
            p.contains("deny:Write") || p.contains("deny:Edit") || p.contains("deny:Bash")
        })
        && !permissions
            .iter()
            .any(|p| p.starts_with("allow:Write") || p.starts_with("allow:Edit"));
    let is_write_enabled = permissions
        .iter()
        .any(|p| p.starts_with("allow:Write") || p.starts_with("allow:Edit"));
    if is_read_only {
        badges.push("read-only".to_string());
    } else if is_write_enabled {
        badges.push("write-enabled".to_string());
    }

    badges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::policy::MandatoryRuleKind;

    fn mr(kind: MandatoryRuleKind, target: &str) -> MandatoryRule {
        MandatoryRule {
            id: "r1".to_string(),
            kind,
            target: target.to_string(),
            min_count: 1,
            rationale: None,
        }
    }

    #[test]
    fn off_mode_agent_has_only_off_badge() {
        let b = derive_badges(GroundingMode::Off, GroundingMode::Off, &[], &[]);
        assert_eq!(b, vec!["off"]);
    }

    #[test]
    fn strict_agent_with_floor_strict_gets_mode_locked() {
        let b = derive_badges(GroundingMode::Strict, GroundingMode::Strict, &[], &[]);
        assert!(b.contains(&"strict".to_string()));
        assert!(b.contains(&"mode-locked".to_string()));
    }

    #[test]
    fn strict_agent_with_floor_soft_is_not_mode_locked() {
        let b = derive_badges(GroundingMode::Strict, GroundingMode::Soft, &[], &[]);
        assert!(b.contains(&"strict".to_string()));
        assert!(!b.contains(&"mode-locked".to_string()));
    }

    #[test]
    fn agent_with_must_use_tool_rule_gets_tools_required_badge() {
        let rules = vec![mr(MandatoryRuleKind::MustUseTool, "read_file")];
        let b = derive_badges(GroundingMode::Soft, GroundingMode::Off, &rules, &[]);
        assert!(b.contains(&"tools-required".to_string()));
    }

    #[test]
    fn agent_with_only_marker_rule_does_not_get_tools_required() {
        let rules = vec![mr(MandatoryRuleKind::MustEmitMarker, "<CITATION_LIST>")];
        let b = derive_badges(GroundingMode::Soft, GroundingMode::Off, &rules, &[]);
        assert!(!b.contains(&"tools-required".to_string()));
    }

    #[test]
    fn read_only_classification_from_deny_strings() {
        let perms = vec![
            "deny:Write".to_string(),
            "deny:Edit".to_string(),
            "allow:Read".to_string(),
        ];
        let b = derive_badges(GroundingMode::Off, GroundingMode::Off, &[], &perms);
        assert!(b.contains(&"read-only".to_string()));
        assert!(!b.contains(&"write-enabled".to_string()));
    }

    #[test]
    fn write_enabled_classification_from_allow_strings() {
        let perms = vec!["allow:Write".to_string()];
        let b = derive_badges(GroundingMode::Off, GroundingMode::Off, &[], &perms);
        assert!(b.contains(&"write-enabled".to_string()));
        assert!(!b.contains(&"read-only".to_string()));
    }

    #[test]
    fn ambiguous_permissions_get_neither_classification_badge() {
        // No deny rules, no allow:Write either. Caller would look at
        // the structured permissions list to determine more.
        let perms = vec!["allow:Read".to_string(), "allow:Bash(ls:*)".to_string()];
        let b = derive_badges(GroundingMode::Off, GroundingMode::Off, &[], &perms);
        assert!(!b.contains(&"read-only".to_string()));
        assert!(!b.contains(&"write-enabled".to_string()));
    }
}
