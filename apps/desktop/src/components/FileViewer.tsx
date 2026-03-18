import { useTranslation } from "react-i18next";
import { X, FileText, Copy, Check } from "lucide-react";
import { cn } from "@/lib/utils";
import { useState } from "react";

// Mock file contents — in production these would come from Tauri fs read
const MOCK_FILE_CONTENTS: Record<string, string> = {
  "CLAUDE.md": `# ATO (Open Source)

Desktop dashboard and MCP server for AI coding tool visibility. MIT licensed.

## Structure

\`\`\`
apps/desktop/        # Tauri 2.x desktop app (Rust + React)
packages/core/       # Shared types, token utils, config paths (no I/O)
packages/db/         # Database abstraction (SQLite for desktop)
services/mcp-server/ # Standalone MCP server for Claude Code (stdio)
\`\`\`

## Commands

- \`npm run dev:desktop\` — Start Tauri desktop app in dev mode
- \`npm run dev:mcp\` — Start MCP server in dev mode
- \`npm run build:desktop\` — Build desktop app for distribution
- \`npm run build\` — Build all packages

## Desktop App

Tauri 2.x with:
- **Rust backend**: SQLite (rusqlite), file watcher (notify crate)
- **React frontend**: Vite + TailwindCSS + Recharts + Zustand
- **i18n**: English, Portuguese, Spanish (react-i18next)
- **Theme**: Dark (#0a0a0f) + cyan/mint (#00FFB2) accent
- **Offline-first**: Works without internet, all data in local SQLite

## Security

- Desktop is local-first. No network calls unless sync explicitly enabled.
- Use parameterized SQL queries only.
- Validate all inputs with zod schemas.`,

  "~/.claude/settings.json": `{
  "permissions": {
    "allow": [
      "Read",
      "Grep",
      "Glob",
      "Bash(npm run *)",
      "Bash(git *)",
      "Write(src/**)",
      "Edit(src/**)"
    ],
    "deny": [
      "Bash(rm -rf *)",
      "WebFetch"
    ]
  },
  "model": "claude-sonnet-4-5",
  "theme": "dark"
}`,

  "~/.claude/skills/code-review.md": `---
name: code-review
description: Perform thorough code reviews checking correctness, security, performance, readability, and test coverage.
allowed-tools: Read, Grep, Glob
---

# Code Review

When reviewing code, check for:

1. **Correctness** — Does the code do what it claims?
2. **Security** — Any injection vectors, exposed secrets?
3. **Performance** — Unnecessary allocations, N+1 queries?
4. **Readability** — Clear naming, good structure?
5. **Tests** — Adequate coverage for the change?`,

  ".claude/skills/conventions.md": `---
name: project-conventions
description: Project coding standards for ATO desktop app.
allowed-tools: Read, Write, Bash, Glob, Grep
model: claude-sonnet-4-5
---

# Project Conventions

## File Structure
- Components in \`src/components/\`
- Utilities in \`src/lib/\`
- Pages in \`src/pages/\`

## Styling
- Use Tailwind CSS utility classes
- Dark theme with cyan accent (#00FFB2)
- Monospace font for code elements

## State Management
- Zustand for global state
- React Query for server state
- Local state for component-specific UI`,
};

interface FileViewerProps {
  filePath: string;
  onClose: () => void;
}

export default function FileViewer({ filePath, onClose }: FileViewerProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  // Try to find content by exact path or by matching end of path
  const content = MOCK_FILE_CONTENTS[filePath]
    || Object.entries(MOCK_FILE_CONTENTS).find(([k]) => filePath.endsWith(k))?.[1]
    || null;

  const lineCount = content ? content.split("\n").length : 0;

  function handleCopy() {
    if (content) {
      navigator.clipboard.writeText(content);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  }

  return (
    <>
      <div className="fixed inset-0 bg-black/30 z-40 lg:hidden" onClick={onClose} />
      <div className="fixed right-0 top-0 bottom-0 w-full lg:w-1/2 max-w-2xl bg-cs-card border-l border-cs-border z-50 flex flex-col shadow-2xl animate-slide-in-right">
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-cs-border">
          <div className="flex items-center gap-2 min-w-0">
            <FileText size={18} className="text-cs-accent shrink-0" />
            <div className="min-w-0">
              <h3 className="text-sm font-semibold truncate">{filePath.split("/").pop()}</h3>
              <p className="text-[10px] text-cs-muted font-mono truncate">{filePath}</p>
            </div>
          </div>
          <div className="flex items-center gap-2 shrink-0 ml-3">
            {content && (
              <>
                <span className="text-[10px] text-cs-muted font-mono">{lineCount} lines</span>
                <button
                  onClick={handleCopy}
                  className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
                  title="Copy"
                >
                  {copied ? <Check size={14} className="text-cs-accent" /> : <Copy size={14} />}
                </button>
              </>
            )}
            <button
              onClick={onClose}
              className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
            >
              <X size={16} />
            </button>
          </div>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4">
          {content ? (
            <pre className="text-sm font-mono text-cs-text whitespace-pre-wrap leading-relaxed">
              {content.split("\n").map((line, i) => (
                <div key={i} className="flex hover:bg-cs-bg/50 -mx-2 px-2 rounded">
                  <span className="text-cs-muted/40 select-none w-8 shrink-0 text-right mr-3 text-xs leading-relaxed">
                    {i + 1}
                  </span>
                  <span className="flex-1">{line || "\u00A0"}</span>
                </div>
              ))}
            </pre>
          ) : (
            <div className="text-center py-12">
              <FileText size={32} className="text-cs-muted/30 mx-auto mb-3" />
              <p className="text-sm text-cs-muted">{t("context.fileNotFound")}</p>
              <p className="text-xs text-cs-muted/60 mt-1 font-mono">{filePath}</p>
            </div>
          )}
        </div>
      </div>
    </>
  );
}
