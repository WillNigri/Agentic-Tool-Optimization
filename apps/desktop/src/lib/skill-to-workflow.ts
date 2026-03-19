// ---------------------------------------------------------------------------
// Auto-detect automation flows from skill content by parsing Step/Phase headers.
// Generic parser — works for any skill formatted with standard Step/Phase headers,
// YAML frontmatter tools, conditional branches, and tool references.
// ---------------------------------------------------------------------------

import type { Workflow, FlowNode, FlowEdge } from "@/components/automation/types";
import { NODE_W } from "@/components/automation/constants";
import type { SkillDetail } from "@/lib/tauri-api";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface ParsedStep {
  label: string;
  description: string;
  original: string;       // raw header text
  body: string;           // full text between this header and the next
  condition: string | null; // e.g. "only if user said yes"
  tools: string[];        // tools detected in the step body
  hasHumanInput: boolean; // AskUserQuestion / approval gate detected
}

interface SkillFrontmatter {
  name?: string;
  tools: string[];  // allowed-tools from YAML frontmatter
}

// ---------------------------------------------------------------------------
// Known tool / service catalog — used for detection in step content
// ---------------------------------------------------------------------------

const TOOL_CATALOG: Record<string, { label: string; service?: string }> = {
  websearch:        { label: "WebSearch",        service: "websearch" },
  webfetch:         { label: "WebFetch",         service: "webfetch" },
  browse:           { label: "Browse",           service: "browse" },
  bash:             { label: "Bash",             service: "bash" },
  github:           { label: "GitHub",           service: "github" },
  "gh ":            { label: "GitHub CLI",       service: "github" },
  "git ":           { label: "Git",              service: "github" },
  "create pr":      { label: "GitHub PR",        service: "github" },
  slack:            { label: "Slack",            service: "slack" },
  discord:          { label: "Discord",          service: "discord" },
  telegram:         { label: "Telegram",         service: "telegram" },
  email:            { label: "Email",            service: "email" },
  resend:           { label: "Resend",           service: "resend" },
  notion:           { label: "Notion",           service: "notion" },
  linear:           { label: "Linear",           service: "linear" },
  postgres:         { label: "PostgreSQL",       service: "postgres" },
  redis:            { label: "Redis",            service: "redis" },
  askuserquestion:  { label: "User Input",       service: undefined },
};

// Patterns indicating human involvement in a step
const HUMAN_PATTERNS = [
  /askuserquestion/i,
  /ask\s+(?:the\s+)?user/i,
  /user\s+approv/i,
  /human\s+(?:review|approval|input)/i,
  /wait\s+for\s+(?:user|human|confirmation)/i,
  /confirm\s+with\s+(?:the\s+)?user/i,
  /get\s+(?:user\s+)?approval/i,
];

// Patterns indicating a conditional/branching step (in header or body)
const CONDITION_PATTERNS = [
  /\(only if (.+?)\)/i,
  /\(if (.+?)\)/i,
  /\(when (.+?)\)/i,
  /\(optional[:\s]*(.+?)\)/i,
  /\(conditional[:\s]*(.+?)\)/i,
];

// Runtime → human-readable label
const RUNTIME_LABELS: Record<string, string> = {
  claude: "Claude Code", codex: "Codex", openclaw: "OpenClaw", hermes: "Hermes",
};

// ---------------------------------------------------------------------------
// Frontmatter parser
// ---------------------------------------------------------------------------

function parseFrontmatter(content: string): SkillFrontmatter {
  const fm: SkillFrontmatter = { tools: [] };
  if (!content.startsWith("---")) return fm;

  const endIdx = content.indexOf("\n---", 3);
  if (endIdx < 0) return fm;

  const block = content.slice(4, endIdx);

  // Extract name
  const nameMatch = block.match(/^name:\s*(.+)/m);
  if (nameMatch) fm.name = nameMatch[1].trim();

  // Extract allowed-tools (list format: "  - ToolName" or inline: "tools: A, B, C")
  const toolLines: string[] = [];
  const lines = block.split("\n");
  let inToolSection = false;
  for (const line of lines) {
    if (/^(?:allowed-tools|tools)\s*:/i.test(line)) {
      // Check inline value
      const inline = line.replace(/^(?:allowed-tools|tools)\s*:\s*/i, "").trim();
      if (inline && !inline.startsWith("|")) {
        toolLines.push(...inline.split(",").map((t) => t.trim()).filter(Boolean));
      }
      inToolSection = true;
      continue;
    }
    if (inToolSection) {
      if (/^\s+-\s+/.test(line)) {
        toolLines.push(line.replace(/^\s+-\s+/, "").trim());
      } else if (/^\S/.test(line)) {
        inToolSection = false;
      }
    }
  }
  fm.tools = toolLines;
  return fm;
}

// ---------------------------------------------------------------------------
// Step parser — extracts steps with full body, conditions, tools, human flags
// ---------------------------------------------------------------------------

/**
 * Parse steps/phases from skill content. Returns enriched ParsedStep objects
 * with body text, detected tools, conditions, and human involvement flags.
 */
function parseStepsFromContent(content: string, frontmatterTools: string[]): ParsedStep[] {
  // Try header-based steps first (## Step N: / ## Phase N:)
  const headerSteps = parseHeaderStepsEnriched(content, frontmatterTools);
  if (headerSteps.length >= 2) return headerSteps;

  // Try numbered list items
  const listSteps = parseNumberedListStepsEnriched(content, frontmatterTools);
  if (listSteps.length >= 2) return listSteps;

  return [];
}

function parseHeaderStepsEnriched(content: string, fmTools: string[]): ParsedStep[] {
  const steps: ParsedStep[] = [];
  const lines = content.split("\n");

  // Find all step/phase header positions
  const headerPositions: { idx: number; line: string }[] = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (/^#{1,3}\s+(?:Step|Phase)\s+[\d.]+(?:\s*[:—–-]\s*(.+))?$/i.test(line)) {
      if (line.toLowerCase().includes("steps to reproduce")) continue;
      if (line.toLowerCase().includes("steps to ")) continue;
      headerPositions.push({ idx: i, line });
    }
  }

  for (let h = 0; h < headerPositions.length; h++) {
    const { idx, line } = headerPositions[h];
    const nextIdx = h + 1 < headerPositions.length ? headerPositions[h + 1].idx : lines.length;

    // Extract title from header
    const titleMatch = line.match(/^#{1,3}\s+(?:Step|Phase)\s+[\d.]+\s*[:—–-]\s*(.+)$/i);
    let title = titleMatch?.[1]?.trim() || line.replace(/^#{1,3}\s+/, "");

    // Extract condition from title parenthetical
    let condition: string | null = null;
    for (const pat of CONDITION_PATTERNS) {
      const cm = title.match(pat);
      if (cm) {
        condition = cm[1].trim();
        title = title.replace(pat, "").trim();
        break;
      }
    }

    // Get body text between this header and the next
    const bodyLines = lines.slice(idx + 1, nextIdx);
    const body = bodyLines.join("\n");

    // First non-empty, non-code, non-heading line as description
    let desc = "";
    for (const bl of bodyLines) {
      const trimmed = bl.trim();
      if (trimmed && !trimmed.startsWith("#") && !trimmed.startsWith("```")) {
        desc = trimmed
          .replace(/^\d+\.\s*/, "")
          .replace(/\*\*/g, "")
          .replace(/^[-*]\s*/, "")
          .slice(0, 100);
        break;
      }
    }

    // Detect tools used in this step's body
    const tools = detectToolsInText(body, fmTools);

    // Detect human involvement
    const hasHumanInput = HUMAN_PATTERNS.some((p) => p.test(body)) ||
      HUMAN_PATTERNS.some((p) => p.test(title));

    // Also check for conditions in body (e.g. "If the user wants competitive research:")
    if (!condition) {
      const bodyCondition = body.match(/^(?:if|only if|when)\s+(?:the\s+)?user\s+(.{5,60}?)(?:[:.]\s)/im);
      if (bodyCondition) {
        condition = bodyCondition[0].replace(/[:.]\s*$/, "").trim();
      }
    }

    steps.push({ label: title, description: desc, original: line, body, condition, tools, hasHumanInput });
  }

  return steps;
}

function parseNumberedListStepsEnriched(content: string, fmTools: string[]): ParsedStep[] {
  const steps: ParsedStep[] = [];
  const lines = content.split("\n");

  // Find section header or start after frontmatter
  let startIdx = 0;
  for (let i = 0; i < lines.length; i++) {
    if (/^#{1,3}\s+(?:Steps|Workflow|Process|Procedure|Instructions|How.?to)/i.test(lines[i].trim())) {
      startIdx = i + 1;
      break;
    }
  }
  if (startIdx === 0) {
    let pastFrontmatter = false, pastTitle = false;
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i].trim();
      if (i === 0 && line === "---") { continue; }
      if (!pastFrontmatter && line === "---") { pastFrontmatter = true; continue; }
      if (pastFrontmatter && !pastTitle && line.startsWith("#")) { pastTitle = true; continue; }
      if (pastFrontmatter && pastTitle) { startIdx = i; break; }
    }
  }

  for (let i = startIdx; i < lines.length; i++) {
    const line = lines[i].trim();
    if (line.startsWith("#") && steps.length > 0) break;

    const numMatch = line.match(/^(\d+)\.\s+(.+)/);
    if (numMatch) {
      let rawText = numMatch[2].trim()
        .replace(/`[^`]+`/g, (m) => m.slice(1, -1))
        .replace(/\*\*/g, "");

      // Check for condition
      let condition: string | null = null;
      for (const pat of CONDITION_PATTERNS) {
        const cm = rawText.match(pat);
        if (cm) {
          condition = cm[1].trim();
          rawText = rawText.replace(pat, "").trim();
          break;
        }
      }

      // Continuation lines as body
      const bodyLines: string[] = [];
      for (let j = i + 1; j < Math.min(i + 10, lines.length); j++) {
        const next = lines[j].trim();
        if (!next || next.startsWith("#") || /^\d+\.\s/.test(next)) break;
        if (next.startsWith("```")) break;
        bodyLines.push(next);
      }
      const body = bodyLines.join("\n");
      const desc = bodyLines[0]
        ?.replace(/\*\*/g, "").replace(/^[-*]\s*/, "")
        .replace(/`[^`]+`/g, (m) => m.slice(1, -1)).slice(0, 100) || "";

      const tools = detectToolsInText(rawText + "\n" + body, fmTools);
      const hasHumanInput = HUMAN_PATTERNS.some((p) => p.test(rawText + " " + body));

      steps.push({ label: rawText.slice(0, 80), description: desc, original: line, body, condition, tools, hasHumanInput });
    }
  }

  return steps;
}

// ---------------------------------------------------------------------------
// Tool detection — scans text for known tool names and frontmatter tool refs
// ---------------------------------------------------------------------------

function detectToolsInText(text: string, frontmatterTools: string[]): string[] {
  const lower = text.toLowerCase();
  const found = new Map<string, string>(); // service → label

  // Match against known tool catalog
  for (const [key, info] of Object.entries(TOOL_CATALOG)) {
    if (lower.includes(key)) {
      const svc = info.service || key;
      if (!found.has(svc)) found.set(svc, info.label);
    }
  }

  // Also check frontmatter tools mentioned in the step body
  for (const t of frontmatterTools) {
    const tLower = t.toLowerCase();
    if (lower.includes(tLower) && !found.has(tLower)) {
      const catalogMatch = TOOL_CATALOG[tLower];
      if (catalogMatch) {
        found.set(catalogMatch.service || tLower, catalogMatch.label);
      }
    }
  }

  return [...found.values()];
}

// ---------------------------------------------------------------------------
// Node type inference
// ---------------------------------------------------------------------------

function inferNodeType(step: ParsedStep, index: number, total: number): FlowNode["type"] {
  // Conditional steps are decision nodes
  if (step.condition) return "decision";

  const lower = step.label.toLowerCase();

  if (index === 0) return "trigger";
  if (index === total - 1) {
    if (lower.match(/report|output|commit|push|write|confirm|deliver/)) return "output";
  }

  if (lower.match(/check|review|verify|audit|triage|gate|pre-?check/)) return "decision";
  if (lower.match(/fix|run|test|execute|implement|build|generate/)) return "action";
  if (lower.match(/create pr|github|push|deploy|notify|send/)) return "service";

  return "process";
}

function inferService(label: string): string | undefined {
  const lower = label.toLowerCase();
  if (lower.match(/\bpr\b|github|branch|push|diff/)) return "github";
  if (lower.match(/slack|notify/)) return "slack";
  if (lower.match(/telegram/)) return "telegram";
  if (lower.match(/email|resend/)) return "email";
  if (lower.match(/discord/)) return "discord";
  return undefined;
}

// ---------------------------------------------------------------------------
// Workflow builder — converts parsed steps into a visual graph
// ---------------------------------------------------------------------------

export function skillToWorkflow(skill: SkillDetail): Workflow | null {
  const fm = parseFrontmatter(skill.content);
  const steps = parseStepsFromContent(skill.content, fm.tools);

  if (steps.length < 2) return null;

  // Cap at 12 steps for visual clarity
  const mainSteps = steps.length > 12
    ? steps.filter((_, i) => i % Math.ceil(steps.length / 12) === 0 || i === steps.length - 1).slice(0, 12)
    : steps;

  const runtimeName = (skill.runtime as string) || "claude";
  const agentLabel = RUNTIME_LABELS[runtimeName] || runtimeName.charAt(0).toUpperCase() + runtimeName.slice(1);

  // Layout constants
  const ROW_HOW = 50;       // top row: tools/services (HOW)
  const ROW_WHAT = 180;     // middle row: action steps (WHAT)
  const ROW_WHO = 310;      // bottom row: agent (WHO)
  const COL_START = 50;
  const COL_GAP = 30;       // minimum gap between columns

  const nodes: FlowNode[] = [];
  const edges: FlowEdge[] = [];

  // Pre-compute per-step data so we can calculate column positions
  const stepData = mainSteps.map((step, i) => {
    const labelService = inferService(step.label);
    const stepTools = step.tools.length > 0
      ? step.tools
      : labelService
        ? [labelService.charAt(0).toUpperCase() + labelService.slice(1)]
        : [];
    const uniqueTools = [...new Set(stepTools)].slice(0, 3);
    // Width needed: max of action node width vs tool nodes side-by-side
    const toolsWidth = uniqueTools.length * NODE_W + Math.max(0, uniqueTools.length - 1) * COL_GAP;
    const slotWidth = Math.max(NODE_W, toolsWidth);
    return { step, labelService, uniqueTools, slotWidth, nodeType: inferNodeType(step, i, mainSteps.length) };
  });

  // Calculate column X positions — each step starts after the previous one's slot + gap
  const colPositions: number[] = [];
  let x = COL_START;
  for (const sd of stepData) {
    colPositions.push(x);
    x += sd.slotWidth + COL_GAP;
  }

  stepData.forEach((sd, i) => {
    const col = colPositions[i];
    const stepId = `${skill.id}-step-${i}`;

    // ── WHAT row: action step node — stretches to match tool row width ──
    const actionWidth = sd.slotWidth;
    nodes.push({
      id: stepId,
      label: sd.step.label.slice(0, 40),
      description: sd.step.description || sd.step.label.slice(0, 80),
      type: sd.nodeType,
      service: sd.labelService,
      runtime: (skill.runtime as "claude" | "codex" | "openclaw" | "hermes") || "claude",
      ...(actionWidth > NODE_W ? { width: actionWidth } : {}),
      x: col,
      y: ROW_WHAT,
      stats: { executions: 0, errors: 0, avgTimeMs: 0 },
      status: "idle",
    });

    // Horizontal edge from previous step
    if (i > 0) {
      const prevStepId = `${skill.id}-step-${i - 1}`;
      if (sd.step.condition) {
        edges.push({ from: prevStepId, to: stepId, label: sd.step.condition.slice(0, 30), animated: false });
      } else {
        edges.push({ from: prevStepId, to: stepId, animated: i === 1 });
      }
    }

    // ── HOW row: tool nodes (spread evenly across slot width) ──
    sd.uniqueTools.forEach((toolLabel, tIdx) => {
      const toolId = `${skill.id}-tool-${i}-${tIdx}`;
      const toolService = Object.entries(TOOL_CATALOG)
        .find(([, v]) => v.label === toolLabel)?.[1]?.service
        || sd.labelService
        || toolLabel.toLowerCase();

      const toolX = col + tIdx * (NODE_W + COL_GAP);
      nodes.push({
        id: toolId,
        label: toolLabel,
        description: `Via ${toolLabel}`,
        type: "service",
        service: toolService,
        runtime: (skill.runtime as "claude" | "codex" | "openclaw" | "hermes") || "claude",
        tool: toolLabel,
        x: toolX,
        y: ROW_HOW,
        stats: { executions: 0, errors: 0, avgTimeMs: 0 },
        status: "idle",
      });
      edges.push({ from: toolId, to: stepId });
    });

    // ── WHO row: human involvement nodes ──
    if (sd.step.hasHumanInput) {
      const humanId = `${skill.id}-human-${i}`;
      nodes.push({
        id: humanId,
        label: "Human Approval",
        description: "User input or approval required",
        type: "decision",
        runtime: (skill.runtime as "claude" | "codex" | "openclaw" | "hermes") || "claude",
        agentName: "Human",
        x: col,
        y: ROW_WHO,
        stats: { executions: 0, errors: 0, avgTimeMs: 0 },
        status: "idle",
      });
      edges.push({ from: humanId, to: stepId });
    }
  });

  // Bottom row: agent node (WHO) — connected to first step
  const agentNodeId = `${skill.id}-agent`;
  nodes.push({
    id: agentNodeId,
    label: agentLabel,
    description: `Runtime: ${runtimeName}`,
    type: "process",
    runtime: (skill.runtime as "claude" | "codex" | "openclaw" | "hermes") || "claude",
    agentName: agentLabel,
    x: colPositions[0],
    y: ROW_WHO + (mainSteps[0]?.hasHumanInput ? 130 : 0),
    stats: { executions: 0, errors: 0, avgTimeMs: 0 },
    status: "active",
  });
  edges.push({ from: agentNodeId, to: `${skill.id}-step-0` });

  return {
    id: `skill-${skill.id}`,
    name: `/${skill.name}`,
    description: skill.description.split("\n")[0].trim(),
    enabled: skill.enabled,
    runCount: 0,
    errorCount: 0,
    nodes,
    edges,
    source: "skill" as const,
  };
}

/**
 * Generate workflows from all skills that have detectable automation steps.
 */
export function generateWorkflowsFromSkills(skills: SkillDetail[]): Workflow[] {
  return skills
    .map(skillToWorkflow)
    .filter((w): w is Workflow => w !== null);
}
