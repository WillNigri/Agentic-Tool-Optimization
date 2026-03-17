import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as os from "node:os";

interface McpServerConfig {
  command?: string;
  args?: string[];
  url?: string;
  type?: string;
  env?: Record<string, string>;
  [key: string]: unknown;
}

interface McpServerInfo {
  name: string;
  transport: string;
  command?: string;
  args?: string[];
  url?: string;
  source: string;
}

interface McpStatus {
  servers: McpServerInfo[];
  config_files_checked: string[];
  total_servers: number;
}

async function readJsonFile(
  filePath: string,
): Promise<Record<string, unknown> | null> {
  try {
    const content = await fs.readFile(filePath, "utf-8");
    return JSON.parse(content) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function extractServers(
  config: Record<string, unknown>,
  source: string,
): McpServerInfo[] {
  const servers: McpServerInfo[] = [];

  // Try common keys where MCP servers might be configured
  const serverMaps = [
    config.mcpServers,
    config.mcp_servers,
    (config.mcp as Record<string, unknown> | undefined)?.servers,
  ];

  for (const serverMap of serverMaps) {
    if (serverMap && typeof serverMap === "object" && !Array.isArray(serverMap)) {
      const entries = serverMap as Record<string, McpServerConfig>;
      for (const [name, serverConfig] of Object.entries(entries)) {
        if (!serverConfig || typeof serverConfig !== "object") continue;

        let transport = "unknown";
        if (serverConfig.command) {
          transport = "stdio";
        } else if (serverConfig.url) {
          transport = serverConfig.url.startsWith("ws") ? "websocket" : "sse";
        } else if (serverConfig.type) {
          transport = serverConfig.type;
        }

        servers.push({
          name,
          transport,
          command: serverConfig.command,
          args: serverConfig.args,
          url: serverConfig.url,
          source,
        });
      }
    }
  }

  return servers;
}

export function registerMcpTools(server: McpServer): void {
  server.tool(
    "get_mcp_status",
    "Reads MCP server configuration from ~/.claude.json and .claude/settings.json and lists configured servers with their transport types",
    {},
    async () => {
      try {
        const homeDir = os.homedir();
        const configPaths = [
          { path: path.join(homeDir, ".claude.json"), label: "~/.claude.json" },
          {
            path: path.join(process.cwd(), ".claude", "settings.json"),
            label: ".claude/settings.json",
          },
          {
            path: path.join(homeDir, ".claude", "settings.json"),
            label: "~/.claude/settings.json",
          },
        ];

        const allServers: McpServerInfo[] = [];
        const checkedFiles: string[] = [];

        for (const { path: configPath, label } of configPaths) {
          checkedFiles.push(label);
          const config = await readJsonFile(configPath);
          if (config) {
            const servers = extractServers(config, label);
            allServers.push(...servers);
          }
        }

        const status: McpStatus = {
          servers: allServers,
          config_files_checked: checkedFiles,
          total_servers: allServers.length,
        };

        return {
          content: [
            { type: "text", text: JSON.stringify(status, null, 2) },
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
