#!/usr/bin/env node

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { registerContextTools } from "./tools/context.js";
import { registerSkillsTools } from "./tools/skills.js";
import { registerUsageTools } from "./tools/usage.js";
import { registerMcpTools } from "./tools/mcp.js";
import { registerRuntimeTools } from "./tools/runtimes.js";
import { registerCacheTools } from "./tools/cache-management.js";
import { registerAgentTools } from "./tools/agents.js";
import { registerObservationTools } from "./tools/observation.js";
import { registerOperationsTools } from "./tools/operations.js";
import { registerAuthoringTools } from "./tools/authoring.js";

const server = new McpServer({
  name: "ato",
  version: "0.2.0",
});

registerContextTools(server);
registerSkillsTools(server);
registerUsageTools(server);
registerMcpTools(server);
registerRuntimeTools(server);
registerCacheTools(server);
registerAgentTools(server); // v1.3.0+ — agents-as-MCPs (cross-runtime dispatch)
// v2.3.4 Phase 3 — agent-driveable platform surface. Each new tool
// shells out to the `ato` CLI rather than re-implementing SQLite
// queries in TypeScript. The CLI is the canonical implementation;
// this server is a thin protocol adapter.
registerObservationTools(server);
registerOperationsTools(server);
registerAuthoringTools(server);

const transport = new StdioServerTransport();
await server.connect(transport);
