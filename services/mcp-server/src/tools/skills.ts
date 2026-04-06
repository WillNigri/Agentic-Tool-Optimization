import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import * as fs from "node:fs/promises";
import { z } from "zod";
import { cache, CACHE_KEYS, CACHE_TTL } from "../cache.js";
import { skillIndex, type SkillMetadata } from "../skill-index.js";

// Map SkillMetadata to the response format (exclude internal fields)
interface SkillInfo {
  name: string;
  description: string;
  file_path: string;
  token_count: number;
  enabled: boolean;
}

function toSkillInfo(skill: SkillMetadata): SkillInfo {
  return {
    name: skill.name,
    description: skill.description,
    file_path: skill.file_path,
    token_count: skill.token_count,
    enabled: skill.enabled,
  };
}

export function registerSkillsTools(server: McpServer): void {
  server.tool(
    "list_skills",
    "Lists all Claude Code skills from ~/.claude/skills/ and .claude/skills/, with frontmatter metadata and token counts. Uses incremental scanning with file watching for optimal performance.",
    {},
    async () => {
      try {
        // Use skill index for incremental scanning
        // Cache wraps the index access for additional TTL-based caching
        const skills = await cache.getOrSet(
          CACHE_KEYS.SKILLS_LIST,
          CACHE_TTL.SKILLS_LIST,
          async () => {
            const indexed = await skillIndex.getSkills();
            return indexed.map(toSkillInfo);
          }
        );
        return {
          content: [
            { type: "text", text: JSON.stringify(skills, null, 2) },
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

  server.tool(
    "toggle_skill",
    "Toggles a skill on/off by renaming the file (adds or removes .disabled extension). Updates the skill index incrementally.",
    { file_path: z.string().describe("Absolute path to the skill file") },
    async ({ file_path: filePath }) => {
      try {
        // Verify file exists
        await fs.access(filePath);

        let newPath: string;
        let newEnabled: boolean;

        if (filePath.endsWith(".disabled")) {
          // Enable: remove .disabled extension
          newPath = filePath.replace(/\.disabled$/, "");
          newEnabled = true;
        } else {
          // Disable: add .disabled extension
          newPath = filePath + ".disabled";
          newEnabled = false;
        }

        await fs.rename(filePath, newPath);

        // Update skill index (handles cache invalidation internally)
        await skillIndex.handleToggle(filePath, newPath);

        return {
          content: [
            {
              type: "text",
              text: JSON.stringify({
                previous_path: filePath,
                new_path: newPath,
                enabled: newEnabled,
                index_updated: true,
              }),
            },
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

  server.tool(
    "get_skill_index_stats",
    "Returns statistics about the skill index, including file watcher status and skill counts.",
    {},
    async () => {
      try {
        // Ensure index is initialized
        await skillIndex.getSkills();
        const stats = skillIndex.stats();
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

  server.tool(
    "rescan_skills",
    "Forces a full rescan of all skill directories. Use this if files were modified outside the watched directories or if the index seems out of sync.",
    {},
    async () => {
      try {
        await skillIndex.rescan();
        const stats = skillIndex.stats();
        return {
          content: [
            {
              type: "text",
              text: JSON.stringify({
                success: true,
                message: "Skill index rescanned",
                ...stats,
              }, null, 2),
            },
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
