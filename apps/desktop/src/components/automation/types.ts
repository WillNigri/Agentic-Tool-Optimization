// ---------------------------------------------------------------------------
// Automation Builder Types
// ---------------------------------------------------------------------------

import type { AgentRuntime } from "@/components/cron/types";

export type { AgentRuntime } from "@/components/cron/types";

// Extended node types for v0.8.0 Advanced Automation
export type FlowNodeType =
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

export type WorkflowSource = "skill" | "cron" | "manual";

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
  category: "triggers" | "services" | "actions" | "flow-control" | "variables";
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
