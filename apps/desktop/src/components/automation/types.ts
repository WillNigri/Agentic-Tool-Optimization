// ---------------------------------------------------------------------------
// Automation Builder Types
// ---------------------------------------------------------------------------

import type { AgentRuntime } from "@/components/cron/types";

export type { AgentRuntime } from "@/components/cron/types";

export interface FlowNode {
  id: string;
  label: string;
  description: string;
  type: "trigger" | "process" | "decision" | "action" | "output" | "service";
  service?: string;
  runtime?: AgentRuntime;
  x: number;
  y: number;
  stats: {
    executions: number;
    errors: number;
    avgTimeMs: number;
  };
  status: "active" | "idle" | "error";
  config?: NodeConfig;
}

export interface NodeConfig {
  params: Record<string, string>;
  condition?: string; // for decision nodes
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
}

export interface NodeTemplate {
  type: FlowNode["type"];
  service?: string;
  action?: string;
  label: string;
  description: string;
  category: "triggers" | "services" | "actions";
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

export type ExecutionNodeStatus = "pending" | "running" | "completed" | "failed";

export interface ExecutionState {
  running: boolean;
  nodeStatuses: Record<string, ExecutionNodeStatus>;
  output: string;
  startedAt?: number;
  error?: string;
}
