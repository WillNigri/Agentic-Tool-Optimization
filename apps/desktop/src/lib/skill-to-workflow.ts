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
 *   ## Steps\n 1. Check email\n 2. Fix config
 *   1. Run git config\n 2. If not set, run...\n 3. Proceed
 */
function parseStepsFromContent(content: string): ParsedStep[] {
  // Try header-based steps first (## Step N: / ## Phase N:)
  const headerSteps = parseHeaderSteps(content);
  if (headerSteps.length >= 2) return headerSteps;

  // Try numbered list items under a steps/workflow section or at top level
  const listSteps = parseNumberedListSteps(content);
  if (listSteps.length >= 2) return listSteps;

  return [];
}

/**
 * Parse "## Step N:" and "## Phase N:" headers.
 */
function parseHeaderSteps(content: string): ParsedStep[] {
  const steps: ParsedStep[] = [];
  const lines = content.split("\n");

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();

    const stepMatch = line.match(/^#{1,3}\s+(?:Step|Phase)\s+[\d.]+(?:\s*[:—–-]\s*(.+))?$/i);
    if (stepMatch) {
      const title = stepMatch[1]?.trim() || line.replace(/^#{1,3}\s+/, "");

      if (title.toLowerCase() === "steps to reproduce") continue;
      if (title.toLowerCase().startsWith("steps to")) continue;

      let desc = "";
      for (let j = i + 1; j < Math.min(i + 5, lines.length); j++) {
        const nextLine = lines[j].trim();
        if (nextLine && !nextLine.startsWith("#") && !nextLine.startsWith("```")) {
          desc = nextLine
            .replace(/^\d+\.\s*/, "")
            .replace(/\*\*/g, "")
            .replace(/^[-*]\s*/, "")
            .slice(0, 100);
          break;
        }
      }

      steps.push({ label: title, description: desc, original: line });
    }
  }

  return steps;
}

/**
 * Parse numbered list items (1. 2. 3.) as steps.
 * Looks for them under ## Steps / ## Workflow / ## Process headers,
 * or as top-level numbered items after the frontmatter/title section.
 */
function parseNumberedListSteps(content: string): ParsedStep[] {
  const steps: ParsedStep[] = [];
  const lines = content.split("\n");

  // Find a section header that signals steps, or scan the whole body
  let startIdx = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (/^#{1,3}\s+(?:Steps|Workflow|Process|Procedure|Instructions|How.?to)/i.test(line)) {
      startIdx = i + 1;
      break;
    }
  }

  // If no explicit section found, start after frontmatter and first heading
  if (startIdx === 0) {
    let pastFrontmatter = false;
    let pastTitle = false;
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i].trim();
      if (i === 0 && line === "---") { pastFrontmatter = false; continue; }
      if (!pastFrontmatter && line === "---") { pastFrontmatter = true; continue; }
      if (pastFrontmatter && !pastTitle && line.startsWith("#")) { pastTitle = true; continue; }
      if (pastFrontmatter && pastTitle) { startIdx = i; break; }
    }
  }

  for (let i = startIdx; i < lines.length; i++) {
    const line = lines[i].trim();

    // Stop at the next heading (a different section)
    if (line.startsWith("#") && steps.length > 0) break;

    // Match "N. Description" (numbered list items)
    const numMatch = line.match(/^(\d+)\.\s+(.+)/);
    if (numMatch) {
      const rawText = numMatch[2].trim();
      // Clean markdown: remove backticks, bold, inline code blocks
      const label = rawText
        .replace(/`[^`]+`/g, (m) => m.slice(1, -1)) // unwrap inline code
        .replace(/\*\*/g, "")
        .slice(0, 80);

      // Get continuation lines as description
      let desc = "";
      for (let j = i + 1; j < Math.min(i + 4, lines.length); j++) {
        const nextLine = lines[j].trim();
        if (!nextLine || nextLine.startsWith("#") || /^\d+\.\s/.test(nextLine)) break;
        if (nextLine.startsWith("```")) break;
        if (!desc) {
          desc = nextLine
            .replace(/\*\*/g, "")
            .replace(/^[-*]\s*/, "")
            .replace(/`[^`]+`/g, (m) => m.slice(1, -1))
            .slice(0, 100);
        }
      }

      steps.push({ label, description: desc, original: line });
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
