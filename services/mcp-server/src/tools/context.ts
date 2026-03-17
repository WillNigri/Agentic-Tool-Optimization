import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as os from "node:os";
import { glob } from "glob";

interface ContextBreakdown {
  system_prompts: { estimated_tokens: number };
  skills: { files: string[]; total_tokens: number };
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

async function findSkillFiles(): Promise<{ path: string; content: string }[]> {
  const homeDir = os.homedir();
  const searchDirs = [
    path.join(homeDir, ".claude", "skills"),
    path.join(process.cwd(), ".claude", "skills"),
  ];

  const results: { path: string; content: string }[] = [];

  for (const dir of searchDirs) {
    try {
      const files = await glob("**/*.md", { cwd: dir, absolute: true });
      for (const file of files) {
        const content = await readFileIfExists(file);
        if (content !== null) {
          results.push({ path: file, content });
        }
      }
    } catch {
      // Directory may not exist, skip
    }
  }

  return results;
}

export function registerContextTools(server: McpServer): void {
  server.tool(
    "get_context_usage",
    "Estimates Claude Code context window usage by analyzing skills, CLAUDE.md, and MCP schemas",
    {},
    async () => {
      try {
        // Read skills files
        const skillFiles = await findSkillFiles();
        const skillsTokens = skillFiles.reduce(
          (sum, f) => sum + estimateTokens(f.content),
          0,
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
              const servers =
                config.mcpServers || config.mcp_servers || {};
              // Rough estimate: ~500 tokens per MCP server for schemas
              mcpSchemaTokens +=
                Object.keys(servers).length * 500;
            } catch {
              // Invalid JSON, skip
            }
          }
        }

        const systemPromptEstimate = 30000;

        const breakdown: ContextBreakdown = {
          system_prompts: { estimated_tokens: systemPromptEstimate },
          skills: {
            files: skillFiles.map((f) => f.path),
            total_tokens: skillsTokens,
          },
          mcp_schemas: { estimated_tokens: mcpSchemaTokens },
          claude_md: { file: claudeMdFile, tokens: claudeMdTokens },
          total_estimated_tokens:
            systemPromptEstimate +
            skillsTokens +
            mcpSchemaTokens +
            claudeMdTokens,
        };

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
