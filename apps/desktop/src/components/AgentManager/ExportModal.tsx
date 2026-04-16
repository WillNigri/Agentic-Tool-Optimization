import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { X, Download, Copy, Check, ArrowRight, FileText } from "lucide-react";
import { cn } from "@/lib/utils";
import { readAgentConfigFile, type AgentConfigRuntime } from "@/lib/api";

interface Props {
  sourcePath: string;
  sourceRuntime: AgentConfigRuntime;
  onClose: () => void;
}

const TARGET_RUNTIMES: { value: AgentConfigRuntime; label: string }[] = [
  { value: "claude", label: "Claude" },
  { value: "codex", label: "Codex" },
  { value: "openclaw", label: "OpenClaw" },
  { value: "hermes", label: "Hermes" },
];

export default function ExportModal({ sourcePath, sourceRuntime, onClose }: Props) {
  const { t } = useTranslation();
  const [targetRuntime, setTargetRuntime] = useState<AgentConfigRuntime>(
    sourceRuntime === "claude" ? "codex" : "claude"
  );
  const [sourceContent, setSourceContent] = useState<string>("");
  const [convertedContent, setConvertedContent] = useState<string>("");
  const [copied, setCopied] = useState(false);

  // Load source content
  useEffect(() => {
    readAgentConfigFile(sourcePath)
      .then((parsed) => {
        setSourceContent(parsed.raw);
        // Convert on load
        convertContent(parsed.raw, sourceRuntime, targetRuntime);
      })
      .catch(console.error);
  }, [sourcePath, sourceRuntime]);

  // Re-convert when target changes
  useEffect(() => {
    if (sourceContent) {
      convertContent(sourceContent, sourceRuntime, targetRuntime);
    }
  }, [sourceContent, sourceRuntime, targetRuntime]);

  const convertContent = (
    content: string,
    from: AgentConfigRuntime,
    to: AgentConfigRuntime
  ) => {
    // Parse frontmatter if present
    const { frontmatter, body } = parseFrontmatter(content);

    let converted = "";

    if (to === "codex") {
      // Convert to Codex TOML format
      converted = convertToCodex(frontmatter, body);
    } else if (to === "hermes") {
      // Convert to Hermes YAML format
      converted = convertToHermes(frontmatter, body);
    } else if (to === "openclaw") {
      // Convert to OpenClaw format
      converted = convertToOpenClaw(frontmatter, body);
    } else if (to === "claude") {
      // Convert to Claude SKILL.md format
      converted = convertToClaude(frontmatter, body);
    } else {
      converted = content;
    }

    setConvertedContent(converted);
  };

  const parseFrontmatter = (content: string): { frontmatter: Record<string, unknown>; body: string } => {
    const trimmed = content.trim();
    if (!trimmed.startsWith("---")) {
      return { frontmatter: {}, body: content };
    }

    const secondDash = trimmed.indexOf("---", 3);
    if (secondDash === -1) {
      return { frontmatter: {}, body: content };
    }

    const fmStr = trimmed.slice(3, secondDash).trim();
    const body = trimmed.slice(secondDash + 3).trim();

    // Simple YAML parsing
    const frontmatter: Record<string, unknown> = {};
    for (const line of fmStr.split("\n")) {
      const colonIdx = line.indexOf(":");
      if (colonIdx > 0) {
        const key = line.slice(0, colonIdx).trim();
        let value: unknown = line.slice(colonIdx + 1).trim();
        // Handle arrays
        if (value === "") {
          // Multi-line array follows
          continue;
        }
        // Handle quoted strings
        if (typeof value === "string" && value.startsWith('"') && value.endsWith('"')) {
          value = value.slice(1, -1);
        }
        frontmatter[key] = value;
      } else if (line.trim().startsWith("- ")) {
        // Array item - find last key
        const keys = Object.keys(frontmatter);
        const lastKey = keys[keys.length - 1];
        if (lastKey) {
          const existing = frontmatter[lastKey];
          const item = line.trim().slice(2);
          if (Array.isArray(existing)) {
            existing.push(item);
          } else {
            frontmatter[lastKey] = [item];
          }
        }
      }
    }

    return { frontmatter, body };
  };

  const convertToCodex = (fm: Record<string, unknown>, body: string): string => {
    const name = fm.name || "skill";
    const description = fm.description || "";

    return `# ${name}

[agent]
name = "${name}"
description = "${description}"

[permissions]
bash = true
read = true
write = true
edit = true

# Instructions
${body}
`;
  };

  const convertToHermes = (fm: Record<string, unknown>, body: string): string => {
    const name = fm.name || "skill";
    const description = fm.description || "";

    return `# ${name}

agent:
  name: "${name}"
  description: "${description}"

permissions:
  - bash
  - read
  - write
  - edit

---

${body}
`;
  };

  const convertToOpenClaw = (fm: Record<string, unknown>, body: string): string => {
    const name = fm.name || "skill";
    const description = fm.description || "";

    return `# ${name}

## Description
${description}

## Tools
- Bash
- Read
- Write
- Edit

## Instructions

${body}
`;
  };

  const convertToClaude = (fm: Record<string, unknown>, body: string): string => {
    const name = fm.name || "skill";
    const description = fm.description || "";

    return `---
name: ${name}
description: ${description}
allowed-tools:
  - Bash
  - Read
  - Write
  - Edit
---

# ${name}

${body}
`;
  };

  const handleCopy = () => {
    navigator.clipboard.writeText(convertedContent);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleDownload = () => {
    const ext = targetRuntime === "codex" ? "toml" : targetRuntime === "hermes" ? "yaml" : "md";
    const blob = new Blob([convertedContent], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `skill.${ext}`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const availableTargets = TARGET_RUNTIMES.filter((r) => r.value !== sourceRuntime);

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-cs-card border border-cs-border rounded-lg w-full max-w-4xl mx-4 max-h-[80vh] flex flex-col overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border shrink-0">
          <div className="flex items-center gap-2">
            <Download size={18} className="text-cs-accent" />
            <h2 className="font-semibold">
              {t("agentManager.export.title", "Export Config")}
            </h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-cs-border transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-hidden flex flex-col p-4">
          {/* Runtime selector */}
          <div className="flex items-center justify-center gap-4 mb-4">
            <div className="text-sm font-medium px-3 py-1.5 bg-cs-border rounded-md">
              {sourceRuntime}
            </div>
            <ArrowRight size={20} className="text-cs-muted" />
            <div className="flex gap-2">
              {availableTargets.map((r) => (
                <button
                  key={r.value}
                  onClick={() => setTargetRuntime(r.value)}
                  className={cn(
                    "px-3 py-1.5 rounded-md text-sm border transition-colors",
                    targetRuntime === r.value
                      ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                      : "border-cs-border hover:border-cs-muted"
                  )}
                >
                  {r.label}
                </button>
              ))}
            </div>
          </div>

          {/* Side-by-side comparison */}
          <div className="flex-1 grid grid-cols-2 gap-4 min-h-0">
            {/* Source */}
            <div className="flex flex-col border border-cs-border rounded-lg overflow-hidden">
              <div className="px-3 py-2 bg-cs-border/50 text-sm font-medium flex items-center gap-2">
                <FileText size={14} />
                {t("agentManager.export.source", "Source")} ({sourceRuntime})
              </div>
              <pre className="flex-1 p-3 text-sm font-mono overflow-auto bg-cs-card/50">
                {sourceContent}
              </pre>
            </div>

            {/* Target */}
            <div className="flex flex-col border border-cs-border rounded-lg overflow-hidden">
              <div className="px-3 py-2 bg-cs-border/50 text-sm font-medium flex items-center gap-2">
                <FileText size={14} />
                {t("agentManager.export.converted", "Converted")} ({targetRuntime})
              </div>
              <pre className="flex-1 p-3 text-sm font-mono overflow-auto bg-cs-card/50">
                {convertedContent}
              </pre>
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-cs-border shrink-0">
          <button
            onClick={handleCopy}
            className="flex items-center gap-2 px-4 py-2 rounded-md text-sm border border-cs-border hover:bg-cs-border/50 transition-colors"
          >
            {copied ? <Check size={14} /> : <Copy size={14} />}
            {copied
              ? t("agentManager.export.copied", "Copied!")
              : t("agentManager.export.copy", "Copy")}
          </button>
          <button
            onClick={handleDownload}
            className="flex items-center gap-2 px-4 py-2 rounded-md text-sm bg-cs-accent text-black font-medium hover:bg-cs-accent/90 transition-colors"
          >
            <Download size={14} />
            {t("agentManager.export.download", "Download")}
          </button>
        </div>
      </div>
    </div>
  );
}
