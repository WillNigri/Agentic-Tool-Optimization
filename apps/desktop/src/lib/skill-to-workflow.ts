// ---------------------------------------------------------------------------
// Auto-detect automation flows from skill content by parsing Step/Phase headers
// ---------------------------------------------------------------------------

import type { Workflow, FlowNode, FlowEdge } from "@/components/automation/types";
import type { SkillDetail } from "@/lib/tauri-api";

interface ParsedStep {
  label: string;
  description: string;
  original: string; // raw header text
}

/**
 * Parse a SKILL.md's content for sequential steps or phases.
 * Detects patterns like:
 *   ## Step 0: Detect base branch
 *   ## Phase 1: Root Cause Investigation
 *   ## Step 3.5: Pre-Landing Review
 */
function parseStepsFromContent(content: string): ParsedStep[] {
  const steps: ParsedStep[] = [];
  const lines = content.split("\n");

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();

    // Match "## Step N: Title" or "## Phase N: Title" (with optional sub-numbers like 3.5)
    const stepMatch = line.match(/^#{1,3}\s+(?:Step|Phase)\s+[\d.]+(?:\s*[:—–-]\s*(.+))?$/i);
    if (stepMatch) {
      const title = stepMatch[1]?.trim() || line.replace(/^#{1,3}\s+/, "");

      // Skip boilerplate headers
      if (title.toLowerCase() === "steps to reproduce") continue;
      if (title.toLowerCase().startsWith("steps to")) continue;

      // Get the first non-empty line after the header as description
      let desc = "";
      for (let j = i + 1; j < Math.min(i + 5, lines.length); j++) {
        const nextLine = lines[j].trim();
        if (nextLine && !nextLine.startsWith("#") && !nextLine.startsWith("```")) {
          // Clean markdown formatting
          desc = nextLine
            .replace(/^\d+\.\s*/, "")
            .replace(/\*\*/g, "")
            .replace(/^[-*]\s*/, "")
            .slice(0, 100);
          break;
        }
      }

      steps.push({
        label: title,
        description: desc,
        original: line,
      });
    }
  }

  return steps;
}

/**
 * Infer node type from step label/description.
 */
function inferNodeType(label: string, _desc: string, index: number, total: number): FlowNode["type"] {
  const lower = label.toLowerCase();

  if (index === 0) return "trigger";
  if (index === total - 1) {
    if (lower.includes("report") || lower.includes("output") || lower.includes("commit") || lower.includes("push")) return "output";
  }

  if (lower.includes("check") || lower.includes("review") || lower.includes("verify") || lower.includes("audit") || lower.includes("triage") || lower.includes("gate")) return "decision";
  if (lower.includes("fix") || lower.includes("run") || lower.includes("test") || lower.includes("execute") || lower.includes("implement")) return "action";
  if (lower.includes("create pr") || lower.includes("github") || lower.includes("push")) return "service";

  return "process";
}

/**
 * Infer service from step label.
 */
function inferService(label: string): string | undefined {
  const lower = label.toLowerCase();
  if (lower.includes("pr") || lower.includes("github") || lower.includes("branch") || lower.includes("push") || lower.includes("diff")) return "github";
  if (lower.includes("slack") || lower.includes("notify")) return "slack";
  return undefined;
}

/**
 * Convert a skill with its full content to a visual workflow.
 * Returns null if the skill has no detectable steps/phases.
 */
export function skillToWorkflow(skill: SkillDetail): Workflow | null {
  const steps = parseStepsFromContent(skill.content);

  // Need at least 2 steps to form a flow
  if (steps.length < 2) return null;

  // Cap at 12 steps for visual clarity (skip sub-steps like 3.25, 3.5)
  const mainSteps = steps.length > 12
    ? steps.filter((_, i) => i % Math.ceil(steps.length / 12) === 0 || i === steps.length - 1).slice(0, 12)
    : steps;

  const nodes: FlowNode[] = mainSteps.map((step, i) => {
    const nodeType = inferNodeType(step.label, step.description, i, mainSteps.length);
    return {
      id: `${skill.id}-step-${i}`,
      label: step.label,
      description: step.description,
      type: nodeType,
      service: inferService(step.label),
      runtime: (skill.runtime as "claude" | "codex" | "openclaw" | "hermes") || "claude",
      x: 50 + i * 230,
      y: 180,
      stats: { executions: 0, errors: 0, avgTimeMs: 0 },
      status: "idle",
    };
  });

  const edges: FlowEdge[] = [];
  for (let i = 0; i < nodes.length - 1; i++) {
    edges.push({
      from: nodes[i].id,
      to: nodes[i + 1].id,
      animated: i === 0,
    });
  }

  return {
    id: `skill-${skill.id}`,
    name: `/${skill.name}`,
    description: skill.description.split("\n")[0].trim(),
    enabled: skill.enabled,
    runCount: 0,
    errorCount: 0,
    nodes,
    edges,
  };
}

/**
 * Generate workflows from all skills that have detectable automation steps.
 * Requires full SkillDetail (with content) for each skill.
 */
export function generateWorkflowsFromSkills(skills: SkillDetail[]): Workflow[] {
  return skills
    .map(skillToWorkflow)
    .filter((w): w is Workflow => w !== null);
}
