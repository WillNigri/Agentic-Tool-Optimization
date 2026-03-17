#!/usr/bin/env node

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { registerContextTools } from "./tools/context.js";
import { registerSkillsTools } from "./tools/skills.js";
import { registerUsageTools } from "./tools/usage.js";
import { registerMcpTools } from "./tools/mcp.js";

const server = new McpServer({
  name: "ato",
  version: "0.1.0",
});

registerContextTools(server);
registerSkillsTools(server);
registerUsageTools(server);
registerMcpTools(server);

const transport = new StdioServerTransport();
await server.connect(transport);
