// v2.3.35 Phase 6 — Sessions + cross-runtime bridge MCP tools.
//
// Exposes Phase 6 Slice A (sticky multi-turn sessions) and Slice B
// (cross-runtime @-mention bridge) to MCP-only agent harnesses. The
// shell-mode equivalent is `ato sessions ...` / `ato bridge ...`; in
// MCP-only mode an agent calls these tools instead.

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { runAtoCli } from "../ato-cli.js";

function toolText(data: unknown) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(data, null, 2) }],
  };
}

export function registerSessionTools(server: McpServer) {
  server.tool(
    "sessions_new",
    "Open a new sticky multi-turn session. Returns the session id. Pass it as `session_id` to start_dispatch (via the --session CLI flag) to continue the conversation across turns. Slice A.2: works with any runtime, including api providers and Phase 6.x-J SSH remotes.",
    {
      runtime: z.string().describe("Anchor runtime — drives whether native --resume applies (claude) or history-replay does (everyone else)"),
      agent_slug: z.string().optional().describe("Optional agent slug to associate with the session"),
      title: z.string().optional().describe("Human-readable title for `ato sessions list`"),
    },
    async ({ runtime, agent_slug, title }) => {
      const args = ["sessions", "new", "--runtime", runtime];
      if (agent_slug) args.push("--as", agent_slug);
      if (title) args.push("--title", title);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "sessions_list",
    "List recent sessions newest-first. Useful for finding the id of a session you started earlier in this conversation.",
    {
      limit: z.number().optional().describe("Max rows (default 20)"),
    },
    async ({ limit }) => {
      const args = ["sessions", "list"];
      if (limit) args.push("--limit", String(limit));
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "sessions_get",
    "Fetch one session by id, including turn count and last-used time.",
    {
      id: z.string().describe("Session id"),
    },
    async ({ id }) => toolText(await runAtoCli(["sessions", "get", id])),
  );

  server.tool(
    "sessions_delete",
    "Delete a session record. Does NOT clean up the underlying runtime's history (claude --resume etc. still works on its side). Mostly cosmetic / for tidying `sessions_list` output.",
    {
      id: z.string().describe("Session id"),
    },
    async ({ id }) => toolText(await runAtoCli(["sessions", "delete", id])),
  );

  server.tool(
    "bridge_run",
    "Slice B cross-runtime bridge: read the latest assistant turn of a session, parse `@<runtime>` mentions, dispatch to the mentioned runtime, repeat until `[CONSENSUS]` on a line by itself / no mention / round cap. SPENDS REAL TOKENS — each round-trip is an LLM call. Useful for getting a second runtime's opinion on the last response without orchestrating manually.",
    {
      session: z.string().describe("Session id to bridge in"),
      max_rounds: z.number().optional().describe("Max bridge round-trips before bailing (default 3)"),
    },
    async ({ session, max_rounds }) => {
      const args = ["bridge", "--session", session];
      if (max_rounds) args.push("--max-rounds", String(max_rounds));
      return toolText(await runAtoCli(args));
    },
  );
}
