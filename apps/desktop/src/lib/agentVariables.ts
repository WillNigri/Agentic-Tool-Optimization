import { invoke, Channel } from "@tauri-apps/api/core";
import { uploadAgentTrace } from "@/lib/agentTraceUpload";

// v2.1.0 — pre/post mtime snapshots so traces carry "files touched"
// attribution. Gated on activeProjectPath; cheap (<200ms typical) and
// silent on failure so the dispatch path never breaks.
async function snapshotBefore(activeProjectPath?: string): Promise<Record<string, number> | null> {
  if (!activeProjectPath) return null;
  try {
    return await invoke<Record<string, number>>("snapshot_project_files", { root: activeProjectPath });
  } catch {
    return null;
  }
}
async function diffAfter(
  before: Record<string, number> | null,
  activeProjectPath?: string,
): Promise<string[] | undefined> {
  if (!before || !activeProjectPath) return undefined;
  try {
    return await invoke<string[]>("diff_project_files", { root: activeProjectPath, prior: before });
  } catch {
    return undefined;
  }
}

// v1.4.0 F1 — Frontend wrappers for agent variables (dynamic prompt resolvers).
//
// The Rust source of truth is `apps/desktop/src-tauri/src/commands.rs`:
// `agent_variables` table, plus `resolve_agent_variables()` /
// `prompt_agent_with_context()` for runtime resolution.

export type VariableKind =
  | "static"           // Free
  | "env"              // Free
  | "project-path"     // Free
  | "file"             // Free
  | "db-query"         // Pro (stub today, real in Wave 2.2)
  | "mcp-call"         // Pro (stub today)
  | "computed";        // Pro (stub today)

export const FREE_VARIABLE_KINDS: VariableKind[] = ["static", "env", "project-path", "file"];
export const PRO_VARIABLE_KINDS: VariableKind[] = ["db-query", "mcp-call", "computed"];

export interface AgentVariable {
  id: string;
  agentId: string;
  name: string;
  kind: VariableKind;
  /** JSON-encoded resolver config. Shape depends on `kind`. */
  configJson: string;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

/** Typed config for each kind — what the user fills in. */
export type VariableConfig =
  | { kind: "static"; value: string }
  | { kind: "env"; var: string }
  | { kind: "project-path" }
  | { kind: "file"; path: string; maxBytes?: number }
  | { kind: "db-query"; path: string; sql: string; maxRows?: number }
  | { kind: "mcp-call"; server: string; tool: string; args: Record<string, unknown> }
  | { kind: "computed"; expr: string };

export function parseConfig(v: AgentVariable): VariableConfig {
  try {
    const obj = JSON.parse(v.configJson) as Record<string, unknown>;
    return { kind: v.kind, ...obj } as VariableConfig;
  } catch {
    return { kind: v.kind } as VariableConfig;
  }
}

export function configToJson(cfg: VariableConfig): string {
  // Strip the discriminator before serializing — the kind lives on the row.
  const { kind: _kind, ...rest } = cfg as VariableConfig & { kind: string };
  return JSON.stringify(rest);
}

export async function listAgentVariables(agentId: string): Promise<AgentVariable[]> {
  return invoke<AgentVariable[]>("list_agent_variables", { agentId });
}

export async function saveAgentVariable(input: {
  id?: string;
  agentId: string;
  name: string;
  kind: VariableKind;
  configJson: string;
  enabled?: boolean;
}): Promise<AgentVariable> {
  return invoke<AgentVariable>("save_agent_variable", {
    id: input.id ?? null,
    agentId: input.agentId,
    name: input.name,
    kind: input.kind,
    configJson: input.configJson,
    enabled: input.enabled ?? true,
  });
}

export async function deleteAgentVariable(id: string): Promise<void> {
  return invoke("delete_agent_variable", { id });
}

/** Single-shot dispatch with variable resolution. Used by Quick Test. */
export async function promptAgentWithContext(input: {
  agentId: string;
  /** Slug for trace attribution. Optional — falls back to agentId if not provided. */
  agentSlug?: string;
  runtime: string;
  prompt: string;
  config?: string;
  activeProjectPath?: string;
  /** Used for trace metadata only. e.g. "desktop:quick_test" | "desktop:run_button" */
  source?: string;
}): Promise<string> {
  const startedAt = new Date().toISOString();
  const t0 = Date.now();
  const before = await snapshotBefore(input.activeProjectPath);
  try {
    const result = await invoke<string>("prompt_agent_with_context", {
      agentId: input.agentId,
      runtime: input.runtime,
      prompt: input.prompt,
      config: input.config ?? null,
      activeProjectPath: input.activeProjectPath ?? null,
    });
    const filesTouched = await diffAfter(before, input.activeProjectPath);
    void uploadAgentTrace({
      agentSlug: input.agentSlug ?? input.agentId,
      runtime: input.runtime,
      startedAt,
      durationMs: Date.now() - t0,
      ok: true,
      source: input.source ?? "desktop:quick_test",
      filesTouched,
    });
    return result;
  } catch (err) {
    void uploadAgentTrace({
      agentSlug: input.agentSlug ?? input.agentId,
      runtime: input.runtime,
      startedAt,
      durationMs: Date.now() - t0,
      ok: false,
      error: err instanceof Error ? err.message : String(err),
      source: input.source ?? "desktop:quick_test",
    });
    throw err;
  }
}

// v1.4.0 F3 — Multi-turn dispatch with conversation summarization.
// When `history.length > memoryPolicy.summarizeAfter`, the older messages
// are summarized via the agent's summarizer model before dispatch.
export interface AgentMessage {
  /** "user" | "assistant" | "system" | "summary" */
  role: string;
  content: string;
}

// v1.5.0 — Streaming events emitted by prompt_agent_*_stream commands.
export type StreamEvent =
  | { kind: "chunk"; text: string }
  | { kind: "done"; full: string }
  | { kind: "error"; message: string };

/** Streaming counterpart of promptAgentWithHistory. Calls onChunk for every
 *  text fragment the runtime emits; resolves with the full final response
 *  when the runtime exits successfully. Rejects on error. */
export async function promptAgentWithHistoryStream(input: {
  agentId: string;
  agentSlug?: string;
  runtime: string;
  history: AgentMessage[];
  newPrompt: string;
  config?: string;
  activeProjectPath?: string;
  source?: string;
  onChunk: (text: string) => void;
}): Promise<string> {
  const startedAt = new Date().toISOString();
  const t0 = Date.now();
  // Snapshot eagerly — we kick off async snapshot in parallel with the
  // stream invocation so attribution doesn't add wall-clock latency.
  const beforePromise = snapshotBefore(input.activeProjectPath);
  return new Promise<string>((resolve, reject) => {
    const channel = new Channel<StreamEvent>();
    let settled = false;
    channel.onmessage = (msg) => {
      if (settled) return;
      if (msg.kind === "chunk") {
        input.onChunk(msg.text);
      } else if (msg.kind === "done") {
        settled = true;
        void (async () => {
          const filesTouched = await diffAfter(await beforePromise, input.activeProjectPath);
          void uploadAgentTrace({
            agentSlug: input.agentSlug ?? input.agentId,
            runtime: input.runtime,
            startedAt,
            durationMs: Date.now() - t0,
            ok: true,
            source: input.source ?? "desktop:promptbar:stream",
            metadata: { historyLength: input.history.length, streamed: true },
            filesTouched,
          });
        })();
        resolve(msg.full);
      } else if (msg.kind === "error") {
        settled = true;
        void uploadAgentTrace({
          agentSlug: input.agentSlug ?? input.agentId,
          runtime: input.runtime,
          startedAt,
          durationMs: Date.now() - t0,
          ok: false,
          error: msg.message,
          source: input.source ?? "desktop:promptbar:stream",
        });
        reject(new Error(msg.message));
      }
    };
    invoke("prompt_agent_with_history_stream", {
      agentId: input.agentId,
      runtime: input.runtime,
      history: input.history,
      newPrompt: input.newPrompt,
      config: input.config ?? null,
      activeProjectPath: input.activeProjectPath ?? null,
      onEvent: channel,
    }).catch((err) => {
      if (settled) return;
      settled = true;
      reject(err instanceof Error ? err : new Error(String(err)));
    });
  });
}

/** Streaming counterpart of promptAgent. No agent context — just runtime. */
export async function promptAgentStream(input: {
  runtime: string;
  prompt: string;
  config?: string;
  onChunk: (text: string) => void;
}): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const channel = new Channel<StreamEvent>();
    let settled = false;
    channel.onmessage = (msg) => {
      if (settled) return;
      if (msg.kind === "chunk") {
        input.onChunk(msg.text);
      } else if (msg.kind === "done") {
        settled = true;
        resolve(msg.full);
      } else if (msg.kind === "error") {
        settled = true;
        reject(new Error(msg.message));
      }
    };
    invoke("prompt_agent_stream", {
      runtime: input.runtime,
      prompt: input.prompt,
      config: input.config ?? null,
      onEvent: channel,
    }).catch((err) => {
      if (settled) return;
      settled = true;
      reject(err instanceof Error ? err : new Error(String(err)));
    });
  });
}

export async function promptAgentWithHistory(input: {
  agentId: string;
  agentSlug?: string;
  runtime: string;
  history: AgentMessage[];
  newPrompt: string;
  config?: string;
  activeProjectPath?: string;
  source?: string;
}): Promise<string> {
  const startedAt = new Date().toISOString();
  const t0 = Date.now();
  try {
    const result = await invoke<string>("prompt_agent_with_history", {
      agentId: input.agentId,
      runtime: input.runtime,
      history: input.history,
      newPrompt: input.newPrompt,
      config: input.config ?? null,
      activeProjectPath: input.activeProjectPath ?? null,
    });
    void uploadAgentTrace({
      agentSlug: input.agentSlug ?? input.agentId,
      runtime: input.runtime,
      startedAt,
      durationMs: Date.now() - t0,
      ok: true,
      source: input.source ?? "desktop:multi_turn",
      metadata: { historyLength: input.history.length },
    });
    return result;
  } catch (err) {
    void uploadAgentTrace({
      agentSlug: input.agentSlug ?? input.agentId,
      runtime: input.runtime,
      startedAt,
      durationMs: Date.now() - t0,
      ok: false,
      error: err instanceof Error ? err.message : String(err),
      source: input.source ?? "desktop:multi_turn",
    });
    throw err;
  }
}

/** Find {var} tokens in a string. Used by the prompt editor to highlight
 *  unresolved tokens / show which variables are referenced. */
export function findReferencedVariables(template: string): string[] {
  const matches = template.matchAll(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g);
  return Array.from(new Set(Array.from(matches, (m) => m[1])));
}
