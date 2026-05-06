import { invoke } from "@tauri-apps/api/core";

// v1.4.0 Polish-T4 — Configuration export / import.
//
// JSON snapshot of the user's local config (agents, hooks, variables,
// groups, projects, env vars, model configs + metadata-only for secrets and
// LLM keys). Plain JSON — not a zip, by design: we wanted the simplest thing
// that can be diffed, eyeballed, version-controlled. The Rust side
// (commands.rs::export_configuration / import_configuration) is the
// source of truth.

export interface ConfigBackup {
  version: number;
  exportedAt: string;
  agents: unknown[];
  agentVariables: unknown[];
  agentHooks: unknown[];
  agentGroups: unknown[];
  agentGroupMembers: unknown[];
  projects: unknown[];
  envVars: unknown[];
  modelConfigs: unknown[];
  secretsMeta: unknown[];
  llmApiKeysMeta: unknown[];
  settings: unknown[];
}

export interface ImportSummary {
  agents: number;
  agentVariables: number;
  agentHooks: number;
  agentGroups: number;
  agentGroupMembers: number;
  projects: number;
  envVars: number;
  modelConfigs: number;
  secretsMeta: number;
  llmApiKeysMeta: number;
  settings: number;
}

export async function exportConfiguration(): Promise<ConfigBackup> {
  return invoke<ConfigBackup>("export_configuration");
}

export async function importConfiguration(backupJson: string): Promise<ImportSummary> {
  return invoke<ImportSummary>("import_configuration", { backupJson });
}
