// ---------------------------------------------------------------------------
// Automation Builder Types
// ---------------------------------------------------------------------------

import type { AgentRuntime } from "@/components/cron/types";

export type { AgentRuntime } from "@/components/cron/types";

// Extended node types for v0.8.0 Advanced Automation
// Legacy IFTTT-style node types — kept for backwards compat with
// workflows migrated from the v2.13 Automations tab (per task #15).
// New designs should reach for `LoopStepKind` below instead.
export type LegacyFlowNodeType =
  | "trigger"      // Start node (webhook, cron, manual, etc.)
  | "process"      // Generic processing step
  | "decision"     // Conditional branching
  | "action"       // Agent action
  | "output"       // Terminal node
  | "service"      // External service call
  | "parallel"     // Parallel execution container
  | "try-catch"    // Error handling wrapper
  | "retry"        // Retry wrapper with backoff
  | "variable"     // Set/transform variables
  | "template";    // Reusable template reference

// v2.14 — LLM-aware first-class kinds. Each one wraps a specific ATO
// CLI primitive that the loop executor (#14) dispatches in-process.
// Naming mirrors the CLI subcommand so the loop editor and the docs
// stay grep-compatible: a user reading "diagnose" in the canvas can
// reasonably guess `ato evaluations methodology diagnose` is what
// runs under it.
export type LoopStepKind =
  | "dispatch"          // wraps `ato dispatch` — single LLM call
  | "methodology_run"   // wraps `ato evaluations methodology run`
  | "diagnose"          // wraps `ato evaluations methodology diagnose`
  | "apply"             // wraps `... methodology diagnose --apply`
  | "review"            // wraps `ato review` — multi-LLM diff review
  | "war_room"          // wraps `ato war-rooms` — multi-seat debate
  | "score"             // rubric application against a target output
  | "input";            // markdown context bundle (new in v2.14)

// Union — the palette UI lets the user drop EITHER an LLM-aware kind
// or a legacy IFTTT-style node (e.g. for an external service call).
// The runtime can tell which is which because `LoopStepKind` strings
// don't overlap with `LegacyFlowNodeType` strings.
export type FlowNodeType = LegacyFlowNodeType | LoopStepKind;

export interface FlowNode {
  id: string;
  label: string;
  description: string;
  type: FlowNodeType;
  service?: string;
  runtime?: AgentRuntime;
  agentId?: string;    // WHO: which agent runs this step
  agentName?: string;  // WHO: human-readable name
  skillId?: string;    // WHAT: which skill is invoked
  tool?: string;       // HOW: external tool/MCP used
  width?: number;      // override default NODE_W (for wider nodes)
  x: number;
  y: number;
  stats: {
    executions: number;
    errors: number;
    avgTimeMs: number;
  };
  status: "active" | "idle" | "error";
  config?: NodeConfig;
  // v0.8.0: Parallel execution - child node IDs that run in parallel
  parallelChildren?: string[];
  // v0.8.0: Error handling - nodes to run on error
  catchNodeId?: string;
  finallyNodeId?: string;
  // v0.8.0: Retry configuration
  retryConfig?: RetryConfig;
  // v0.8.0: Variable operations
  variableOps?: VariableOperation[];
  // v0.8.0: Template reference
  templateId?: string;
}

// v1.6.0 — extended to include agent groups (routed + sequential) so the
// Automations canvas becomes the canonical visualization for everything
// that runs without a human in the loop, not just skill-derived flows.
export type WorkflowSource = "skill" | "cron" | "manual" | "group-routed" | "group-sequential" | "hook";

export interface NodeConfig {
  params: Record<string, string>;
  condition?: string; // for decision nodes
  // v0.8.0: Webhook configuration
  webhook?: WebhookConfig;
}

// v0.8.0: Webhook trigger configuration
export interface WebhookConfig {
  path: string;           // URL path: /webhook/{path}
  method: "GET" | "POST" | "PUT" | "DELETE";
  secret?: string;        // Optional HMAC secret for validation
  headers?: Record<string, string>; // Required headers
  bodySchema?: string;    // JSON schema for validation (optional)
}

// v0.8.0: Retry configuration for retry nodes
export interface RetryConfig {
  maxAttempts: number;    // Max retry attempts (1-10)
  backoffType: "fixed" | "exponential" | "linear";
  initialDelayMs: number; // Initial delay in milliseconds
  maxDelayMs: number;     // Max delay cap
  retryOn?: string[];     // Error types to retry on (empty = all)
}

// v0.8.0: Variable operations
export interface VariableOperation {
  op: "set" | "get" | "transform" | "delete";
  name: string;           // Variable name
  value?: string;         // Value or expression
  transform?: "json" | "string" | "number" | "boolean" | "jq"; // Transform type
  expression?: string;    // jq or template expression
}

// v0.8.0: Workflow variables context
export interface WorkflowVariables {
  // Built-in variables
  $trigger: Record<string, unknown>;  // Trigger payload (webhook body, etc.)
  $env: Record<string, string>;       // Environment variables
  $workflow: {
    id: string;
    name: string;
    runId: string;
    startedAt: string;
  };
  // User-defined variables
  [key: string]: unknown;
}

// v0.8.0: Workflow template
export interface WorkflowTemplate {
  id: string;
  name: string;
  description: string;
  category: string;       // e.g., "CI/CD", "Notifications", "Data Processing"
  tags: string[];
  author?: string;
  version: string;
  nodes: FlowNode[];
  edges: FlowEdge[];
  variables?: {           // Template variables that users can customize
    name: string;
    description: string;
    defaultValue?: string;
    required?: boolean;
  }[];
  icon?: string;
  isBuiltIn?: boolean;    // System-provided templates
}

export interface FlowEdge {
  from: string;
  to: string;
  label?: string;
  animated?: boolean;
}

export interface Workflow {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  lastRun?: string;
  runCount: number;
  errorCount: number;
  nodes: FlowNode[];
  edges: FlowEdge[];
  source?: WorkflowSource;
  // v0.8.0: Webhook trigger endpoint
  webhookId?: string;     // Unique webhook identifier
  webhookPath?: string;   // Generated webhook path
  // v0.8.0: Default variables
  defaultVariables?: Record<string, unknown>;
  // v0.8.0: Template this workflow was created from
  fromTemplateId?: string;
}

export interface NodeTemplate {
  type: FlowNode["type"];
  service?: string;
  action?: string;
  label: string;
  description: string;
  /**
   * Palette grouping. v2.14 added "llm" (the new LLM-aware first-class
   * kinds) and "data" (input/output bundles + control flow that doesn't
   * fit "flow-control" naming). Existing categories stay for migrated
   * workflows: "triggers" / "services" / "actions" / "flow-control" /
   * "variables".
   */
  category:
    | "triggers"
    | "services"
    | "actions"
    | "flow-control"
    | "variables"
    | "llm"    // v2.14 — Dispatch / MethodologyRun / Diagnose / Apply / Review / WarRoom / Score
    | "data"; // v2.14 — Input / Output bundles
  // v0.8.0: Default config for new nodes
  defaultConfig?: Partial<NodeConfig>;
  defaultRetryConfig?: RetryConfig;
}

export interface ServiceAction {
  id: string;
  label: string;
  description: string;
  params: ParamSchema[];
}

export interface ParamSchema {
  key: string;
  label: string;
  type: "text" | "textarea" | "select";
  placeholder?: string;
  required?: boolean;
  options?: string[];
}

export type BuilderMode = "view" | "edit";

export interface ConnectingState {
  fromNodeId: string;
  fromPort: "output";
}

export type ExecutionNodeStatus = "pending" | "running" | "completed" | "failed" | "skipped" | "retrying";

export interface ExecutionState {
  running: boolean;
  nodeStatuses: Record<string, ExecutionNodeStatus>;
  output: string;
  startedAt?: number;
  error?: string;
  // v0.8.0: Enhanced execution tracking
  runId?: string;                     // Unique execution ID
  variables?: WorkflowVariables;      // Current variable state
  nodeOutputs?: Record<string, unknown>; // Output from each node
  nodeRetries?: Record<string, number>;  // Retry count per node
  parallelGroups?: Record<string, {      // Parallel execution groups
    nodeIds: string[];
    completedIds: string[];
    failedIds: string[];
  }>;
  triggeredBy?: "manual" | "webhook" | "cron" | "api";
  triggerPayload?: unknown;           // Incoming webhook/trigger data
}
