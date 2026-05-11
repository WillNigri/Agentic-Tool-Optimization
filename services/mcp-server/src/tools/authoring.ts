// v2.3.4 Phase 3 — Authoring MCP tools.
//
// Writes that produce real artifacts: SKILL.md drafts, agent records,
// agent updates. Each one logs to the configuration ledger so the
// local regression detector sees the change.

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { runAtoCli } from "../ato-cli.js";

function toolText(data: unknown) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(data, null, 2) }],
  };
}

export function registerAuthoringTools(server: McpServer) {
  server.tool(
    "draft_skill_from_replay",
    "Generate a SKILL.md draft from a successful replay. The replay tells us a prompt that originally failed on runtime A succeeded on runtime B; the skill encodes that routing decision so future prompts get sent to the runtime that works. Writes to the target runtime's standard skills dir (e.g. ~/.claude/skills/<slug>/SKILL.md) by default; pass `out` to override. Returns the skill name + the path written + a preview of the draft.",
    {
      from_replay_job_id: z.string().describe("Replay job ID (status must be 'done')"),
      out: z.string().optional().describe("Optional absolute path to write the SKILL.md to"),
    },
    async ({ from_replay_job_id, out }) => {
      const args = ["skills", "draft", "--from-replay", from_replay_job_id];
      if (out) args.push("--out", out);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "create_agent",
    "Create a new agent record. INSERTs into the local agents table AND writes the runtime's native config file (~/.claude/agents/<slug>.md for Claude, ~/.codex/agents/<slug>/AGENTS.md for Codex). Logs the create to the configuration ledger.",
    {
      slug: z.string().describe("Unique slug (per-runtime)"),
      runtime: z.string().describe("claude, codex, gemini, openclaw, or hermes"),
      display_name: z.string().optional(),
      description: z.string().optional(),
      model: z.string().optional().describe("Pinned model (optional)"),
      system_prompt: z.string().optional(),
      project_id: z.string().optional(),
    },
    async ({ slug, runtime, display_name, description, model, system_prompt, project_id }) => {
      const args = ["agents", "create", "--slug", slug, "--runtime", runtime];
      if (display_name) args.push("--display-name", display_name);
      if (description) args.push("--description", description);
      if (model) args.push("--model", model);
      if (system_prompt) args.push("--system-prompt", system_prompt);
      if (project_id) args.push("--project-id", project_id);
      return toolText(await runAtoCli(args));
    },
  );

  server.tool(
    "update_agent",
    "Update an existing agent's editable fields. Each diff is logged to the agent_config_changes ledger so the regression detector sees the edit the same way it sees GUI edits. Skills can be replaced wholesale (skills='a,b,c') or mutated incrementally (add_skill / remove_skill). Pass at most one of skills, add_skill, remove_skill.",
    {
      slug: z.string().describe("Slug of the agent to update"),
      runtime: z.string().optional().describe("Disambiguate when the slug exists on multiple runtimes"),
      model: z.string().optional(),
      system_prompt: z.string().optional(),
      display_name: z.string().optional(),
      description: z.string().optional(),
      skills: z.array(z.string()).optional().describe("Replace the entire skills list with this set"),
      add_skill: z.string().optional().describe("Append one skill (idempotent)"),
      remove_skill: z.string().optional().describe("Remove one skill (no-op if absent)"),
    },
    async ({ slug, runtime, model, system_prompt, display_name, description, skills, add_skill, remove_skill }) => {
      const args = ["agents", "update", slug];
      if (runtime) args.push("--runtime", runtime);
      if (model) args.push("--model", model);
      if (system_prompt) args.push("--system-prompt", system_prompt);
      if (display_name) args.push("--display-name", display_name);
      if (description) args.push("--description", description);
      if (skills && skills.length > 0) args.push("--skills", skills.join(","));
      if (add_skill) args.push("--add-skill", add_skill);
      if (remove_skill) args.push("--remove-skill", remove_skill);
      return toolText(await runAtoCli(args));
    },
  );
}
