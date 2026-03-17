import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as os from "node:os";
import { glob } from "glob";

interface LogEntry {
  timestamp?: string;
  type?: string;
  model?: string;
  input_tokens?: number;
  output_tokens?: number;
  cache_read_tokens?: number;
  cache_write_tokens?: number;
  cost_usd?: number;
  session_id?: string;
}

interface DailySummary {
  date: string;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cache_read_tokens: number;
  total_cache_write_tokens: number;
  total_cost_usd: number;
  request_count: number;
  sessions: string[];
}

interface UsageStats {
  daily: DailySummary[];
  total_input_tokens: number;
  total_output_tokens: number;
  total_cost_usd: number;
  total_requests: number;
  log_files_found: number;
}

// Approximate cost per token (Claude Sonnet pricing as default estimate)
const COST_PER_INPUT_TOKEN = 3.0 / 1_000_000;
const COST_PER_OUTPUT_TOKEN = 15.0 / 1_000_000;
const COST_PER_CACHE_READ_TOKEN = 0.3 / 1_000_000;
const COST_PER_CACHE_WRITE_TOKEN = 3.75 / 1_000_000;

function estimateCost(entry: LogEntry): number {
  if (entry.cost_usd !== undefined) return entry.cost_usd;

  let cost = 0;
  if (entry.input_tokens) cost += entry.input_tokens * COST_PER_INPUT_TOKEN;
  if (entry.output_tokens) cost += entry.output_tokens * COST_PER_OUTPUT_TOKEN;
  if (entry.cache_read_tokens)
    cost += entry.cache_read_tokens * COST_PER_CACHE_READ_TOKEN;
  if (entry.cache_write_tokens)
    cost += entry.cache_write_tokens * COST_PER_CACHE_WRITE_TOKEN;
  return cost;
}

async function parseLogFile(filePath: string): Promise<LogEntry[]> {
  const entries: LogEntry[] = [];
  try {
    const content = await fs.readFile(filePath, "utf-8");
    const lines = content.split("\n").filter((line) => line.trim());

    for (const line of lines) {
      try {
        const entry = JSON.parse(line) as LogEntry;
        entries.push(entry);
      } catch {
        // Skip malformed lines
      }
    }
  } catch {
    // File unreadable, skip
  }
  return entries;
}

export function registerUsageTools(server: McpServer): void {
  server.tool(
    "get_usage_stats",
    "Reads Claude Code JSONL logs from ~/.claude/logs/ and returns daily/session usage summaries with token counts and cost estimates",
    {},
    async () => {
      try {
        const logsDir = path.join(os.homedir(), ".claude", "logs");

        let logFiles: string[];
        try {
          logFiles = await glob("**/*.jsonl", {
            cwd: logsDir,
            absolute: true,
          });
        } catch {
          return {
            content: [
              {
                type: "text",
                text: JSON.stringify({
                  message: "No logs directory found at ~/.claude/logs/",
                  daily: [],
                  total_input_tokens: 0,
                  total_output_tokens: 0,
                  total_cost_usd: 0,
                  total_requests: 0,
                  log_files_found: 0,
                }),
              },
            ],
          };
        }

        if (logFiles.length === 0) {
          return {
            content: [
              {
                type: "text",
                text: JSON.stringify({
                  message: "No .jsonl log files found in ~/.claude/logs/",
                  daily: [],
                  total_input_tokens: 0,
                  total_output_tokens: 0,
                  total_cost_usd: 0,
                  total_requests: 0,
                  log_files_found: 0,
                }),
              },
            ],
          };
        }

        // Parse all log files
        const allEntries: LogEntry[] = [];
        for (const file of logFiles) {
          const entries = await parseLogFile(file);
          allEntries.push(...entries);
        }

        // Group by date
        const dailyMap = new Map<string, DailySummary>();

        for (const entry of allEntries) {
          // Only process entries that have token info
          if (!entry.input_tokens && !entry.output_tokens) continue;

          const date = entry.timestamp
            ? entry.timestamp.substring(0, 10)
            : "unknown";

          let daily = dailyMap.get(date);
          if (!daily) {
            daily = {
              date,
              total_input_tokens: 0,
              total_output_tokens: 0,
              total_cache_read_tokens: 0,
              total_cache_write_tokens: 0,
              total_cost_usd: 0,
              request_count: 0,
              sessions: [],
            };
            dailyMap.set(date, daily);
          }

          daily.total_input_tokens += entry.input_tokens || 0;
          daily.total_output_tokens += entry.output_tokens || 0;
          daily.total_cache_read_tokens += entry.cache_read_tokens || 0;
          daily.total_cache_write_tokens += entry.cache_write_tokens || 0;
          daily.total_cost_usd += estimateCost(entry);
          daily.request_count += 1;

          if (entry.session_id && !daily.sessions.includes(entry.session_id)) {
            daily.sessions.push(entry.session_id);
          }
        }

        const dailySummaries = Array.from(dailyMap.values()).sort(
          (a, b) => a.date.localeCompare(b.date),
        );

        const stats: UsageStats = {
          daily: dailySummaries,
          total_input_tokens: dailySummaries.reduce(
            (s, d) => s + d.total_input_tokens,
            0,
          ),
          total_output_tokens: dailySummaries.reduce(
            (s, d) => s + d.total_output_tokens,
            0,
          ),
          total_cost_usd: dailySummaries.reduce(
            (s, d) => s + d.total_cost_usd,
            0,
          ),
          total_requests: dailySummaries.reduce(
            (s, d) => s + d.request_count,
            0,
          ),
          log_files_found: logFiles.length,
        };

        // Round cost values
        stats.total_cost_usd =
          Math.round(stats.total_cost_usd * 10000) / 10000;
        for (const day of stats.daily) {
          day.total_cost_usd =
            Math.round(day.total_cost_usd * 10000) / 10000;
        }

        return {
          content: [
            { type: "text", text: JSON.stringify(stats, null, 2) },
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
