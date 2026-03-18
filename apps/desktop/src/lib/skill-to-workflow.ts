// ---------------------------------------------------------------------------
// Convert skills with multi-step workflows into visual automation flows
// ---------------------------------------------------------------------------

import type { Workflow, FlowNode, FlowEdge } from "@/components/automation/types";
import type { LocalSkill } from "@/lib/tauri-api";

interface ParsedStep {
  label: string;
  description: string;
  type: FlowNode["type"];
  service?: string;
}

/**
 * Known gstack-style workflow skills and their step breakdowns.
 * These map skill names to their multi-step automation flows.
 */
const KNOWN_WORKFLOWS: Record<string, ParsedStep[]> = {
  ship: [
    { label: "Detect Base Branch", description: "Find and merge the base branch", type: "process" },
    { label: "Run Tests", description: "Execute test suite", type: "action" },
    { label: "Review Diff", description: "Review all changes against base", type: "decision" },
    { label: "Bump VERSION", description: "Increment version number", type: "process" },
    { label: "Update CHANGELOG", description: "Update changelog with changes", type: "process" },
    { label: "Commit & Push", description: "Commit all changes and push to remote", type: "action" },
    { label: "Create PR", description: "Create a pull request", type: "service", service: "github" },
  ],
  qa: [
    { label: "Start QA Session", description: "Initialize browser and QA tier", type: "trigger" },
    { label: "Test User Flows", description: "Systematically test application pages", type: "action" },
    { label: "Find Bugs", description: "Identify visual, functional, and logic bugs", type: "process" },
    { label: "Severity Check", description: "Classify bugs by severity tier", type: "decision" },
    { label: "Fix Bug", description: "Fix bug in source code", type: "action" },
    { label: "Commit Fix", description: "Atomic commit for each fix", type: "process" },
    { label: "Re-verify", description: "Verify fix with before/after screenshots", type: "action" },
    { label: "Health Report", description: "Produce ship-readiness summary", type: "output" },
  ],
  review: [
    { label: "Fetch Diff", description: "Get changed files from PR", type: "trigger", service: "github" },
    { label: "SQL Safety", description: "Check for SQL injection risks", type: "decision" },
    { label: "Trust Boundaries", description: "Check LLM trust boundary violations", type: "decision" },
    { label: "Side Effects", description: "Check conditional side effects", type: "decision" },
    { label: "Structural Issues", description: "Check for structural code problems", type: "process" },
    { label: "Post Review", description: "Write review comment on PR", type: "output" },
  ],
  "design-review": [
    { label: "Browse Site", description: "Open and navigate the live site", type: "trigger" },
    { label: "Visual Audit", description: "Check spacing, hierarchy, consistency", type: "process" },
    { label: "AI Slop Detection", description: "Find generic AI-generated patterns", type: "decision" },
    { label: "Fix Issues", description: "Fix visual issues in source code", type: "action" },
    { label: "Before/After", description: "Take comparison screenshots", type: "process" },
    { label: "Commit Fix", description: "Atomic commit per visual fix", type: "action" },
  ],
  "plan-ceo-review": [
    { label: "Read Plan", description: "Read and understand the current plan", type: "trigger" },
    { label: "Challenge Premises", description: "Question assumptions and scope", type: "process" },
    { label: "Mode Selection", description: "Scope expansion / hold / reduction", type: "decision" },
    { label: "10-Star Product", description: "Propose the ideal product vision", type: "action" },
    { label: "Updated Plan", description: "Deliver revised plan", type: "output" },
  ],
  "plan-eng-review": [
    { label: "Read Plan", description: "Read the execution plan", type: "trigger" },
    { label: "Architecture Review", description: "Review data flow and architecture", type: "process" },
    { label: "Edge Cases", description: "Identify unhandled edge cases", type: "decision" },
    { label: "Test Coverage", description: "Review test strategy", type: "process" },
    { label: "Performance Check", description: "Check for performance issues", type: "decision" },
    { label: "Locked Plan", description: "Deliver locked-in execution plan", type: "output" },
  ],
  retro: [
    { label: "Analyze Commits", description: "Read commit history for the period", type: "trigger" },
    { label: "Work Patterns", description: "Analyze coding patterns and velocity", type: "process" },
    { label: "Per-Person Breakdown", description: "Break down contributions per team member", type: "process" },
    { label: "Quality Metrics", description: "Assess code quality trends", type: "decision" },
    { label: "Retro Report", description: "Generate retrospective document", type: "output" },
  ],
  "document-release": [
    { label: "Read Diff", description: "Read all changes since last release", type: "trigger" },
    { label: "Read Docs", description: "Read all project documentation", type: "process" },
    { label: "Update README", description: "Sync README with shipped changes", type: "action" },
    { label: "Update ARCHITECTURE", description: "Update architecture docs", type: "action" },
    { label: "Update CHANGELOG", description: "Polish changelog entries", type: "action" },
    { label: "Bump VERSION", description: "Optionally bump version number", type: "process" },
  ],
};

/**
 * Convert a skill to a visual workflow if it has known automation steps.
 */
export function skillToWorkflow(skill: LocalSkill): Workflow | null {
  const steps = KNOWN_WORKFLOWS[skill.name];
  if (!steps) return null;

  const nodes: FlowNode[] = steps.map((step, i) => ({
    id: `${skill.name}-step-${i}`,
    label: step.label,
    description: step.description,
    type: step.type,
    service: step.service,
    runtime: (skill.runtime as "claude" | "codex" | "openclaw" | "hermes") || "claude",
    x: 50 + i * 230,
    y: 180,
    stats: { executions: 0, errors: 0, avgTimeMs: 0 },
    status: "idle",
  }));

  const edges: FlowEdge[] = [];
  for (let i = 0; i < nodes.length - 1; i++) {
    edges.push({
      from: nodes[i].id,
      to: nodes[i + 1].id,
      animated: i === 0,
    });
  }

  return {
    id: `skill-${skill.name}`,
    name: `/${skill.name}`,
    description: skill.description,
    enabled: skill.enabled,
    runCount: 0,
    errorCount: 0,
    nodes,
    edges,
  };
}

/**
 * Generate workflows from all skills that have known automation patterns.
 */
export function generateWorkflowsFromSkills(skills: LocalSkill[]): Workflow[] {
  return skills
    .map(skillToWorkflow)
    .filter((w): w is Workflow => w !== null);
}
