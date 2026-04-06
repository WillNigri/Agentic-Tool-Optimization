import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as os from "node:os";
import { cache, CACHE_KEYS, CACHE_TTL } from "../cache.js";
import { skillIndex } from "../skill-index.js";

interface ContextBreakdown {
  system_prompts: { estimated_tokens: number };
  skills: { files: string[]; total_tokens: number; enabled_only: boolean };
  mcp_schemas: { estimated_tokens: number };
  claude_md: { file: string | null; tokens: number };
  total_estimated_tokens: number;
}

function estimateTokens(content: string): number {
  return Math.ceil(content.length / 4);
}

async function readFileIfExists(filePath: string): Promise<string | null> {
  try {
    return await fs.readFile(filePath, "utf-8");
  } catch {
    return null;
  }
}

async function computeContextUsage(): Promise<ContextBreakdown> {
  // Get skills from the index (uses incremental scanning)
  const allSkills = await skillIndex.getSkills();

  // Only count enabled skills for context estimation
  const enabledSkills = allSkills.filter((s) => s.enabled);
  const skillsTokens = enabledSkills.reduce(
    (sum, s) => sum + s.token_count,
    0
  );

  // Read CLAUDE.md
  const claudeMdPaths = [
    path.join(process.cwd(), "CLAUDE.md"),
    path.join(os.homedir(), "CLAUDE.md"),
  ];

  let claudeMdFile: string | null = null;
  let claudeMdTokens = 0;

  for (const mdPath of claudeMdPaths) {
    const content = await readFileIfExists(mdPath);
    if (content !== null) {
      claudeMdFile = mdPath;
      claudeMdTokens = estimateTokens(content);
      break;
    }
  }

  // Estimate MCP schema tokens from configured servers
  let mcpSchemaTokens = 0;
  const mcpConfigPaths = [
    path.join(os.homedir(), ".claude.json"),
    path.join(process.cwd(), ".claude", "settings.json"),
  ];

  for (const configPath of mcpConfigPaths) {
    const content = await readFileIfExists(configPath);
    if (content !== null) {
      try {
        const config = JSON.parse(content);
        const servers = config.mcpServers || config.mcp_servers || {};
        // Rough estimate: ~500 tokens per MCP server for schemas
        mcpSchemaTokens += Object.keys(servers).length * 500;
      } catch {
        // Invalid JSON, skip
      }
    }
  }

  const systemPromptEstimate = 30000;

  return {
    system_prompts: { estimated_tokens: systemPromptEstimate },
    skills: {
      files: enabledSkills.map((s) => s.file_path),
      total_tokens: skillsTokens,
      enabled_only: true,
    },
    mcp_schemas: { estimated_tokens: mcpSchemaTokens },
    claude_md: { file: claudeMdFile, tokens: claudeMdTokens },
    total_estimated_tokens:
      systemPromptEstimate + skillsTokens + mcpSchemaTokens + claudeMdTokens,
  };
}

export function registerContextTools(server: McpServer): void {
  server.tool(
    "get_context_usage",
    "Estimates Claude Code context window usage by analyzing skills, CLAUDE.md, and MCP schemas. Uses incremental skill scanning for optimal performance. Results are cached for 30 seconds.",
    {},
    async () => {
      try {
        const breakdown = await cache.getOrSet(
          CACHE_KEYS.CONTEXT_USAGE,
          CACHE_TTL.CONTEXT_USAGE,
          computeContextUsage
        );

        return {
          content: [
            { type: "text", text: JSON.stringify(breakdown, null, 2) },
          ],
        };
      } catch (error) {
        const message =
          error instanceof Error ? error.message : String(error);
        return {
          content: [
            {
              type: "text",
              text: JSON.stringify({ error: message }),
            },
          ],
          isError: true,
        };
      }
    },
  );
}
