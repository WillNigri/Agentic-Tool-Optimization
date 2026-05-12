// v2.3.35 — Activity feed MCP tools.
//
// The activity feed is where humans and agents both post and react.
// Phase 5 surface: messages, event notices, approval requests,
// approval decisions. These tools let MCP-only agents participate in
// the same feed the desktop GUI shows.

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { runAtoCli } from "../ato-cli.js";

function toolText(data: unknown) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(data, null, 2) }],
  };
}

export function registerPostsTools(server: McpServer) {
  server.tool(
    "post_message",
    "Post a message to the activity feed. Visible immediately in the desktop GUI's Activity pane. Use for status updates, asking a human a question, or recording a decision the team should see.",
    {
      text: z.string().describe("Message body (markdown ok)"),
      author_kind: z.string().optional().describe("'agent' | 'human' | 'system' (default human). When an MCP-driven agent posts, set this to 'agent'."),
      author_slug: z.string().optional().describe("Author slug (e.g. agent slug). Omit for plain humans."),
      kind: z.string().optional().describe("Post kind: 'message' (default), 'event_notice', 'approval_request'. Use 'approval_request' to ask a human to confirm a destructive action."),
    },
    async ({ text, author_kind, author_slug, kind }) => {
      const args = ["posts", "add", text];
      if (author_kind) args.push("--as", author_kind);
      if (author_slug) args.push("--slug", author_slug);
      if (kind) args.push("--kind", kind);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "list_posts",
    "List recent activity-feed posts newest-first. Filter by kind to focus on messages / approval requests / event notices.",
    {
      limit: z.number().optional().describe("Max rows (default 20)"),
      kind: z.string().optional().describe("'message' | 'event_notice' | 'approval_request' | 'approval_decision'"),
    },
    async ({ limit, kind }) => {
      const args = ["posts", "list"];
      if (limit) args.push("--limit", String(limit));
      if (kind) args.push("--kind", kind);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "list_pending_approvals",
    "List ApprovalRequest posts that don't yet have a matching ApprovalDecision. Use before firing a destructive action to confirm there's no human approval the agent should wait on.",
    {},
    async () => toolText(await runAtoCli(["posts", "pending"])),
  );

  server.tool(
    "approve_request",
    "Approve a pending ApprovalRequest post. Writes an ApprovalDecision post; the partial UNIQUE index in the schema makes the approve/deny race-safe at the storage layer. Use only when authorized by the human.",
    {
      request_post_id: z.string().describe("The ApprovalRequest post id from list_pending_approvals"),
      note: z.string().optional().describe("Optional reasoning note attached to the decision"),
    },
    async ({ request_post_id, note }) => {
      const args = ["posts", "approve", request_post_id];
      if (note) args.push("--notes", note);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "deny_request",
    "Deny a pending ApprovalRequest post. Same race-safety as approve_request. Use when the agent has been told (or detects) the request should not proceed.",
    {
      request_post_id: z.string().describe("The ApprovalRequest post id from list_pending_approvals"),
      note: z.string().optional().describe("Optional reasoning note attached to the decision"),
    },
    async ({ request_post_id, note }) => {
      const args = ["posts", "deny", request_post_id];
      if (note) args.push("--notes", note);
      return toolText(await runAtoCli(args));
    },
  );
}
