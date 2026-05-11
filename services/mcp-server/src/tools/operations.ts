// v2.3.4 Phase 3 — Operations MCP tools.
//
// Operations that change state: dispatch a prompt, start a replay,
// poll a replay, kill a running dispatch, compare two traces.
//
// Note on side effects:
//   - start_dispatch and start_replay spend real LLM tokens.
//   - kill_run cancels in-flight work.
// The AGENTS.md safety table documents which require explicit human
// approval before the agent fires them. We don't enforce that here —
// the agent's harness is the right place to gate destructive actions.
// But we do mark them clearly in the tool description.

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { runAtoCli } from "../ato-cli.js";

function toolText(data: unknown) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(data, null, 2) }],
  };
}

export function registerOperationsTools(server: McpServer) {
  server.tool(
    "start_dispatch",
    "Fire a single-shot dispatch against a runtime. SPENDS REAL TOKENS — confirm with the human first if cost matters. Captures stdout/stderr, persists to execution_logs with token + cost estimates, returns the run's id and response.",
    {
      runtime: z.string().describe("claude, codex, gemini, openclaw, or hermes"),
      prompt: z.string().describe("The prompt text"),
      model: z.string().optional().describe("Override the runtime's default model"),
      agent_slug: z.string().optional().describe("Optional agent slug for attribution"),
    },
    async ({ runtime, prompt, model, agent_slug }) => {
      const args = ["dispatch", runtime, prompt];
      if (model) args.push("--model", model);
      if (agent_slug) args.push("--agent", agent_slug);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "start_replay",
    "Replay an existing dispatch against a different runtime / model. SPENDS REAL TOKENS — confirm with the human first if cost matters. Looks up the source prompt from execution_logs, dispatches synchronously, returns the replay_job record with response, duration, cost.",
    {
      source_id: z.string().describe("Cloud trace ID or execution_logs ID of the source dispatch"),
      runtime: z.string().describe("Target runtime to replay against"),
      model: z.string().optional().describe("Override the target model"),
    },
    async ({ source_id, runtime, model }) => {
      const args = ["replay", "start", source_id, "--runtime", runtime];
      if (model) args.push("--model", model);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "get_replay_job",
    "Fetch a replay job's current state by id. Read-only; safe to poll.",
    {
      job_id: z.string().describe("Replay job ID"),
      wait: z.boolean().optional().describe("Block until terminal status (max 5 minutes). Default false."),
    },
    async ({ job_id, wait }) => {
      const args = ["replay", "get", job_id];
      if (wait) args.push("--wait");
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "kill_run",
    "Terminate a running dispatch by run_id. DESTRUCTIVE — ask the human first unless the run is clearly stuck. Sends SIGTERM to the child process the desktop dispatch path spawned.",
    {
      run_id: z.string().describe("Run ID from get_active_runs"),
    },
    async ({ run_id }) => {
      return toolText(await runAtoCli(["kill", run_id]));
    },
  );

  server.tool(
    "compare_traces",
    "Compare two runs side-by-side. Returns both run records plus a diff with duration delta, cost delta, and same-status flag. Both IDs accept either execution_logs.id or cloud_trace_id.",
    {
      a: z.string().describe("First run ID"),
      b: z.string().describe("Second run ID"),
    },
    async ({ a, b }) => {
      return toolText(await runAtoCli(["compare", a, b]));
    },
  );
}
