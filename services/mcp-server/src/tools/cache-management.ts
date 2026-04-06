import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { cache, CACHE_KEYS } from "../cache.js";

export function registerCacheTools(server: McpServer): void {
  server.tool(
    "get_cache_stats",
    "Returns statistics about the MCP server's in-memory cache, including cached keys and entry count.",
    {},
    async () => {
      try {
        const stats = cache.stats();
        return {
          content: [
            {
              type: "text",
              text: JSON.stringify(
                {
                  ...stats,
                  available_cache_keys: CACHE_KEYS,
                },
                null,
                2
              ),
            },
          ],
        };
      } catch (error) {
        return {
          content: [
            { type: "text", text: JSON.stringify({ error: String(error) }) },
          ],
          isError: true,
        };
      }
    }
  );

  server.tool(
    "clear_cache",
    "Clears the MCP server's in-memory cache. Optionally clear only a specific key or keys matching a prefix.",
    {
      key: z.string().optional().describe("Specific cache key to clear"),
      prefix: z
        .string()
        .optional()
        .describe("Clear all keys starting with this prefix"),
    },
    async ({ key, prefix }) => {
      try {
        let cleared: number | boolean;
        let message: string;

        if (key) {
          cleared = cache.invalidate(key);
          message = cleared
            ? `Cleared cache key: ${key}`
            : `Key not found: ${key}`;
        } else if (prefix) {
          cleared = cache.invalidateByPrefix(prefix);
          message = `Cleared ${cleared} cache entries with prefix: ${prefix}`;
        } else {
          const stats = cache.stats();
          const count = stats.size;
          cache.clear();
          cleared = count;
          message = `Cleared all ${count} cache entries`;
        }

        return {
          content: [
            {
              type: "text",
              text: JSON.stringify({ success: true, message, cleared }),
            },
          ],
        };
      } catch (error) {
        return {
          content: [
            { type: "text", text: JSON.stringify({ error: String(error) }) },
          ],
          isError: true,
        };
      }
    }
  );
}
