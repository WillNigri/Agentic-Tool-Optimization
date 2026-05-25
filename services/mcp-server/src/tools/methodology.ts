// v2.10 — Methodology runner MCP tools.
//
// Exposes the `ato evaluations methodology …` CLI surface (PR-2/3/3.1/4/5)
// to MCP-driven AI agents. Closes the Agentic Usage Interface loop the
// docs/grounded-mode plan called out: anything a human can do via CLI,
// an AI can do via MCP — including running a full methodology + scoring
// the results.
//
// All tools shell out to `ato evaluations methodology <subcommand>`. The
// CLI is the source of truth; this file is just MCP glue.
//
// Wedge use cases for AI agents calling these:
//
//   1. Cold reader: "list_methodologies" → "show_run" → digest the
//      composition without re-running anything.
//   2. Adopt + score: AI agent decides to evaluate the last week's
//      dispatches under a new rubric. Calls "adopt" then "score".
//   3. Fan-out: AI agent realizes a real model-ladder is overdue.
//      Calls "cost_estimate" first (no surprise spend!), then "run".
//   4. Margin / pricing check: any AI building on ATO can call
//      "margin_report" to see the open-source unit economics for
//      the user's tier.

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { runAtoCli } from "../ato-cli.js";

function toolText(data: unknown) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(data, null, 2) }],
  };
}

export function registerMethodologyTools(server: McpServer) {
  server.tool(
    "list_methodologies",
    "List all methodology recipes defined in the local DB. Newest first. Each row includes slug, archetype, and dispatch-count-per-run. Use this before `show_methodology` or `run_methodology` to discover what's available.",
    {
      archetype: z
        .string()
        .optional()
        .describe(
          "Optional filter by archetype slug (model-ladder | tools-vs-no-tools | reviewer-order-effects | regression-watch | custom)",
        ),
    },
    async ({ archetype }) => {
      const args = ["evaluations", "methodology", "list"];
      if (archetype) args.push("--archetype", archetype);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "show_methodology",
    "Print one methodology's full record — variant matrix (prompts × models × conditions × reps) + rubric (regex / structural / llm_judge / composite). Use before `cost_estimate_methodology` or `run_methodology`.",
    {
      slug: z.string().describe("Methodology slug"),
    },
    async ({ slug }) => {
      return toolText(
        await runAtoCli(["evaluations", "methodology", "get", slug]),
      );
    },
  );

  server.tool(
    "methodology_archetypes",
    "List the built-in archetype catalog: model-ladder, tools-vs-no-tools, reviewer-order-effects, regression-watch, custom. Each archetype has a default reps_per_cell tuned to industry baselines. No DB read — pure registry.",
    {},
    async () => {
      return toolText(
        await runAtoCli(["evaluations", "methodology", "archetypes"]),
      );
    },
  );

  server.tool(
    "cost_estimate_methodology",
    "Compute the pre-run cost estimate for a methodology BEFORE fan-out. ALWAYS call this before `run_methodology` — the runner spec mandates no-surprise-spend. Returns customer-side LLM cost + our-side cost (storage + bandwidth + compute + judge) per the open-source rate card.",
    {
      slug: z.string().describe("Methodology slug"),
      billing: z
        .enum(["byok", "pool"])
        .optional()
        .describe(
          "byok (default) = customer's API keys pay; pool = our shared key pays (Team+ tier).",
        ),
      judge_calls: z
        .number()
        .optional()
        .describe(
          "LLM-judge calls per dispatch. 0 (default) for rule-based rubrics; 1 for simple LLM-judge; higher for composite rubrics.",
        ),
    },
    async ({ slug, billing, judge_calls }) => {
      const args = ["evaluations", "methodology", "cost-estimate", slug];
      if (billing) args.push("--billing", billing);
      if (judge_calls !== undefined)
        args.push("--judge-calls", String(judge_calls));
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "run_methodology",
    "Fan out a methodology: expand variant matrix → sequential `ato dispatch` calls → write methodology_runs + methodology_run_dispatches with dual cost accounting + score each dispatch through the methodology's rubric. THIS BURNS LLM TOKENS — always call `cost_estimate_methodology` first.",
    {
      slug: z.string().describe("Methodology slug"),
      billing: z
        .enum(["byok", "pool"])
        .optional()
        .describe("byok (default) or pool"),
      max_dispatches: z
        .number()
        .optional()
        .describe("Cap the run at N dispatches (smoke-testing)"),
      stop_on_error: z
        .boolean()
        .optional()
        .describe(
          "Abort on first failed dispatch (default false — continue and record the failure)",
        ),
    },
    async ({ slug, billing, max_dispatches, stop_on_error }) => {
      const args = ["evaluations", "methodology", "run", slug];
      if (billing) args.push("--billing", billing);
      if (max_dispatches !== undefined)
        args.push("--max-dispatches", String(max_dispatches));
      if (stop_on_error) args.push("--stop-on-error");
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "adopt_existing_dispatches",
    "Compose a methodology over EXISTING execution_logs rows WITHOUT re-dispatching. The Pro angle: every dispatch the user has fired in the last week is suddenly evaluable, retrospectively. Costs $0 incremental LLM spend. Then call `score_methodology_run` to evaluate the adopted dispatches through the rubric.",
    {
      slug: z.string().describe("Methodology slug"),
      since: z
        .string()
        .optional()
        .describe("ISO-8601 or YYYY-MM-DD lower bound on created_at"),
      until: z.string().optional().describe("Upper bound"),
      runtime: z
        .string()
        .optional()
        .describe("Filter by runtime (e.g. 'claude', 'anthropic', 'google')"),
      model: z
        .string()
        .optional()
        .describe("Filter by model (e.g. 'claude-sonnet-4-6')"),
      status: z
        .string()
        .optional()
        .describe("Filter by status ('success' default; 'all' for everything)"),
      agent: z.string().optional().describe("Filter by agent slug"),
      limit: z
        .number()
        .optional()
        .describe("Cap on adopted rows (default 500)"),
    },
    async ({ slug, since, until, runtime, model, status, agent, limit }) => {
      const args = ["evaluations", "methodology", "adopt", slug];
      if (since) args.push("--since", since);
      if (until) args.push("--until", until);
      if (runtime) args.push("--runtime", runtime);
      if (model) args.push("--model", model);
      if (status) args.push("--status", status);
      if (agent) args.push("--agent", agent);
      if (limit !== undefined) args.push("--limit", String(limit));
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "score_methodology_run",
    "Score every dispatch in a methodology run using the methodology's rubric. Idempotent — re-run with `force=true` to re-score. LLM-judge rubric kinds burn tokens (cost lands in provider_judge_cost_usd); regex / structural rubric kinds are free.",
    {
      run_id: z
        .string()
        .describe(
          "Methodology run id (returned by `run_methodology` or `adopt_existing_dispatches`)",
        ),
      force: z
        .boolean()
        .optional()
        .describe("Re-score even dispatches that already have a score"),
    },
    async ({ run_id, force }) => {
      const args = ["evaluations", "methodology", "score", run_id];
      if (force) args.push("--force");
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "list_methodology_runs",
    "List recent methodology runs, newest first. Each row includes status, planned vs completed counts, and customer+provider cost. Use before `show_methodology_run` to find the id of a run you want to inspect.",
    {
      methodology: z
        .string()
        .optional()
        .describe("Filter to runs of one methodology by slug"),
      limit: z.number().optional().describe("Max rows (default 50)"),
    },
    async ({ methodology, limit }) => {
      const args = ["evaluations", "methodology", "runs", "list"];
      if (methodology) args.push("--methodology", methodology);
      if (limit !== undefined) args.push("--limit", String(limit));
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "show_methodology_run",
    "Print one run's full composition: per-cell statistics (n, mean cost, mean tokens, mean duration, 95% confidence intervals), rubric score summary, grounding-verdict mix per cell, and pairwise Welch t-statistics over cost for any (prompt, condition) cell with ≥2 models. This is what the customer reads to make a decision.",
    {
      run_id: z.string().describe("Methodology run id"),
    },
    async ({ run_id }) => {
      return toolText(
        await runAtoCli(["evaluations", "methodology", "runs", "show", run_id]),
      );
    },
  );

  server.tool(
    "methodology_margin_report",
    "Aggregate dual-cost-ledger margin report across methodology_runs. Shows customer-side LLM spend vs our-side delivery cost (storage + bandwidth + compute + judge), with the rate card constants printed verbatim. Useful for: 'what was my eval budget last month' (customer) and 'what's our unit economics' (admin).",
    {
      since: z
        .string()
        .optional()
        .describe("ISO-8601 or YYYY-MM-DD lower bound"),
      until: z.string().optional().describe("Upper bound"),
      methodology: z
        .string()
        .optional()
        .describe("Filter to one methodology by slug"),
    },
    async ({ since, until, methodology }) => {
      const args = ["evaluations", "methodology", "margin"];
      if (since) args.push("--since", since);
      if (until) args.push("--until", until);
      if (methodology) args.push("--methodology", methodology);
      return toolText(await runAtoCli(args));
    },
  );
}
