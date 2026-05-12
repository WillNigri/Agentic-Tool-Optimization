// v2.3.4 Phase 3 — Observation MCP tools.
//
// Read-only surface for an agent inspecting the developer's local
// ATO state: recent dispatches, active runs, config-change ledger,
// regressions, cost recommendations, file attribution, replay history.
//
// Each tool shells out to the equivalent `ato` CLI subcommand and
// returns the JSON. Single algorithm, two surfaces.

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { runAtoCli } from "../ato-cli.js";

function toolText(data: unknown) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(data, null, 2) }],
  };
}

export function registerObservationTools(server: McpServer) {
  server.tool(
    "get_recent_dispatches",
    "List the developer's recent dispatches (executions of an agent / runtime). Returns prompt summary, response, duration, cost estimate, and status.",
    {
      limit: z.number().int().min(1).max(500).optional().describe("How many to return (default 20)."),
      runtime: z.string().optional().describe("Filter by runtime (claude, codex, gemini, ...)."),
      status: z.string().optional().describe("Filter by status ('success' or 'error')."),
    },
    async ({ limit, runtime, status }) => {
      const args = ["dispatches", "recent"];
      if (limit !== undefined) args.push("--limit", String(limit));
      if (runtime) args.push("--runtime", runtime);
      if (status) args.push("--status", status);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "get_active_runs",
    "List currently in-flight dispatches. Includes run_id, agent_slug, runtime, workspace, source, started_at, status. Empty if nothing is running or the desktop's live_runs SQLite mirror hasn't populated yet.",
    {},
    async () => {
      return toolText(await runAtoCli(["runs", "live"]));
    },
  );

  server.tool(
    "get_run",
    "Fetch a single run's full record by id. Accepts either execution_logs.id or cloud_trace_id.",
    {
      id: z.string().describe("Run / execution_logs ID, or cloud trace ID."),
    },
    async ({ id }) => {
      return toolText(await runAtoCli(["runs", "get", id]));
    },
  );

  server.tool(
    "get_config_changes",
    "List configuration changes for a specific agent (model swaps, prompt edits, role-models changes). The regression detector joins these against trace stats.",
    {
      agent_slug: z.string().describe("Slug of the agent to inspect."),
      since: z.string().optional().describe("Window: '7d', '24h', '30m', etc. Defaults to 7d."),
    },
    async ({ agent_slug, since }) => {
      const args = ["config-changes", "list", "--agent", agent_slug];
      if (since) args.push("--since", since);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "get_regressions",
    "Run local-mode regression detection. Joins agent_config_changes against trace stats and surfaces meaningful deltas per change. Source field on the response tells you whether the data is from local SQLite ('local') or whether the schema isn't migrated ('local-no-schema').",
    {
      days: z.number().int().min(1).max(365).optional().describe("How far back to look (default 30)."),
      window_hours: z.number().int().min(1).max(720).optional().describe("Window on each side of a change (default 168 = 7 days)."),
      min_samples: z.number().int().min(5).optional().describe("Min runs per side to surface a change (default 20)."),
    },
    async ({ days, window_hours, min_samples }) => {
      const args = ["regressions", "list"];
      if (days !== undefined) args.push("--days", String(days));
      if (window_hours !== undefined) args.push("--window-hours", String(window_hours));
      if (min_samples !== undefined) args.push("--min-samples", String(min_samples));
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "get_failing_examples",
    "Get the failing trace IDs from a specific regression. Use this after get_regressions surfaces a regression you want to drill into. Returns up to 10 trace IDs; fetch each with get_run for full prompt/response.",
    {
      change_id: z.string().describe("Change ID from a regression row's `change_id` field."),
    },
    async ({ change_id }) => {
      // The CLI's regressions list already includes failing_trace_ids
      // per row. We fetch the full set and filter to the requested change.
      const result = await runAtoCli<{ regressions: Array<{ change_id: string; failing_trace_ids: string[] }> }>([
        "regressions",
        "list",
        "--days",
        "365", // Cast a wide net so we catch the requested change_id.
      ]);
      const row = result?.regressions?.find((r) => r.change_id === change_id);
      return toolText({
        change_id,
        failing_trace_ids: row?.failing_trace_ids ?? [],
        found: !!row,
      });
    },
  );

  server.tool(
    "get_cost_recommendations",
    "Surface model-swap recommendations when the developer's historical multi-runtime data justifies them. A recommendation must satisfy: alt at least 30% cheaper, ok-rate within 10pp.",
    {
      days: z.number().int().min(1).max(365).optional().describe("Window of history to consider (default 30)."),
      min_runs: z.number().int().min(5).optional().describe("Min runs per (agent, runtime) combo (default 10)."),
    },
    async ({ days, min_runs }) => {
      const args = ["cost", "recommendations"];
      if (days !== undefined) args.push("--days", String(days));
      if (min_runs !== undefined) args.push("--min-runs", String(min_runs));
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "get_file_attribution",
    "Get the list of files a specific dispatch touched (mtime-snapshot diff). Local mirroring is on the v2.3.x roadmap — today this returns honest 'cloud-fetch-required' status when the data is cloud-only, so the agent knows to route differently.",
    {
      run_id: z.string().describe("Run ID or cloud trace ID."),
    },
    async ({ run_id }) => {
      return toolText(await runAtoCli(["files-touched", run_id]));
    },
  );

  server.tool(
    "get_replay_history",
    "List replay jobs that have been run against a given cloud trace. Useful when the agent wants to know 'have we already tried replaying this on Codex?'",
    {
      trace_id: z.string().describe("Cloud trace ID (the source of the replays)."),
    },
    async ({ trace_id }) => {
      return toolText(await runAtoCli(["replays", "for-trace", trace_id]));
    },
  );

  server.tool(
    "ratchet_check",
    "Phase 6.x-K — eval-score ratchet. Compares current 7-day success rate against the locked floor for every ratchet (or one if `target` is set). Returns a per-target verdict (pass / fail / insufficient_data). Same semantics as the CLI's `ato ratchet check`; the CI exit code isn't propagated through MCP — agents should inspect `verdict` field.",
    {
      target: z.string().optional().describe("Optional filter: `agent:<slug>`, `runtime:<name>`, or `global`."),
      window_days: z.number().optional().describe("Days to look back for the current value (default 7)."),
    },
    async ({ target, window_days }) => {
      const args = ["ratchet", "check"];
      if (target) args.push("--target", target);
      if (window_days) args.push("--window-days", String(window_days));
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "ratchet_list",
    "List locked ratchets. Read-only.",
    {},
    async () => toolText(await runAtoCli(["ratchet", "list"])),
  );

  server.tool(
    "list_recent_events",
    "List recent events from the events_log bus (dispatch_failed, regression_detected, replay_done, etc.). Read-only. Use to spot patterns over the last N events without needing a live --watch tail.",
    {
      limit: z.number().optional().describe("Max rows (default 20)"),
      event_type: z.string().optional().describe("Filter by event type (e.g. 'regression_detected', 'dispatch_failed')"),
    },
    async ({ limit, event_type }) => {
      const args = ["events", "recent"];
      if (limit) args.push("--limit", String(limit));
      if (event_type) args.push("--type", event_type);
      return toolText(await runAtoCli(args));
    },
  );
}
