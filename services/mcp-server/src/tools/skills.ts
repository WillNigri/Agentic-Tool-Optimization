import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as os from "node:os";
import { glob } from "glob";
import matter from "gray-matter";
import { z } from "zod";

interface SkillInfo {
  name: string;
  description: string;
  file_path: string;
  token_count: number;
  enabled: boolean;
}

function estimateTokens(content: string): number {
  return Math.ceil(content.length / 4);
}

async function scanSkills(): Promise<SkillInfo[]> {
  const homeDir = os.homedir();
  const searchDirs = [
    path.join(homeDir, ".claude", "skills"),
    path.join(process.cwd(), ".claude", "skills"),
  ];

  const skills: SkillInfo[] = [];

  for (const dir of searchDirs) {
    try {
      // Include both .md and .md.disabled files
      const files = await glob("**/*.md{,.disabled}", {
        cwd: dir,
        absolute: true,
      });

      for (const filePath of files) {
        try {
          const content = await fs.readFile(filePath, "utf-8");
          const parsed = matter(content);
          const frontmatter = parsed.data as Record<string, unknown>;

          const baseName = path.basename(filePath);
          const enabled = !baseName.endsWith(".disabled");
          const name =
            (frontmatter.name as string) ||
            baseName
              .replace(/\.disabled$/, "")
              .replace(/\.md$/, "");
          const description =
            (frontmatter.description as string) || "";

          skills.push({
            name,
            description,
            file_path: filePath,
            token_count: estimateTokens(content),
            enabled,
          });
        } catch {
          // Skip unreadable files
        }
      }
    } catch {
      // Directory may not exist, skip
    }
  }

  return skills;
}

export function registerSkillsTools(server: McpServer): void {
  server.tool(
    "list_skills",
    "Lists all Claude Code skills from ~/.claude/skills/ and .claude/skills/, with frontmatter metadata and token counts",
    {},
    async () => {
      try {
        const skills = await scanSkills();
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
    "Toggles a skill on/off by renaming the file (adds or removes .disabled extension)",
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

        return {
          content: [
            {
              type: "text",
              text: JSON.stringify({
                previous_path: filePath,
                new_path: newPath,
                enabled: newEnabled,
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
}
