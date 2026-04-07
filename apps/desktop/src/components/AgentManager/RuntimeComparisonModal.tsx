import { useState } from "react";
import { useTranslation } from "react-i18next";
import { X, Check, Minus, HelpCircle, Zap } from "lucide-react";
import { cn } from "@/lib/utils";

interface Props {
  onClose: () => void;
}

interface RuntimeCapability {
  name: string;
  claude: boolean | string;
  codex: boolean | string;
  hermes: boolean | string;
  openclaw: boolean | string;
}

const CAPABILITIES: RuntimeCapability[] = [
  { name: "Context Window", claude: "200k", codex: "128k", hermes: "128k", openclaw: "128k" },
  { name: "Streaming", claude: true, codex: true, hermes: true, openclaw: true },
  { name: "Tool Use", claude: true, codex: true, hermes: true, openclaw: true },
  { name: "Image Input", claude: true, codex: true, hermes: false, openclaw: false },
  { name: "Code Execution", claude: true, codex: true, hermes: true, openclaw: true },
  { name: "MCP Support", claude: true, codex: false, hermes: false, openclaw: true },
  { name: "Local/Offline", claude: false, codex: false, hermes: true, openclaw: false },
  { name: "Multi-file Edit", claude: true, codex: true, hermes: true, openclaw: true },
  { name: "Web Search", claude: true, codex: true, hermes: false, openclaw: true },
  { name: "Git Integration", claude: true, codex: true, hermes: true, openclaw: true },
];

const CONFIG_INFO = [
  { name: "Skill Format", claude: "SKILL.md", codex: "SKILL.md", hermes: "SKILL.md", openclaw: "SKILL.md" },
  { name: "Project Config", claude: "CLAUDE.md", codex: "AGENTS.md", hermes: "SOUL.md", openclaw: "SOUL.md" },
  { name: "Settings Format", claude: "JSON", codex: "TOML", hermes: "YAML", openclaw: "JSON" },
  { name: "Skills Path", claude: "~/.claude/skills/", codex: "~/.agents/skills/", hermes: "~/.hermes/skills/", openclaw: "~/.openclaw/skills/" },
];

const BEST_FOR = {
  claude: ["General coding", "Complex refactoring", "Code review", "Documentation"],
  codex: ["Quick edits", "Completions", "Simple tasks", "OpenAI ecosystem"],
  hermes: ["Privacy-first", "Offline work", "Local development", "Custom models"],
  openclaw: ["Remote teams", "SSH workflows", "Cloud development", "Collaboration"],
};

const RUNTIME_COLORS = {
  claude: "text-orange-400 border-orange-400/30 bg-orange-400/5",
  codex: "text-green-400 border-green-400/30 bg-green-400/5",
  hermes: "text-purple-400 border-purple-400/30 bg-purple-400/5",
  openclaw: "text-cyan-400 border-cyan-400/30 bg-cyan-400/5",
};

export default function RuntimeComparisonModal({ onClose }: Props) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<"features" | "config" | "recommend">("features");

  const renderValue = (value: boolean | string) => {
    if (typeof value === "string") {
      return <span className="text-sm">{value}</span>;
    }
    return value ? (
      <Check size={16} className="text-green-400" />
    ) : (
      <Minus size={16} className="text-cs-muted" />
    );
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-cs-card border border-cs-border rounded-lg w-full max-w-4xl mx-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border">
          <div className="flex items-center gap-2">
            <Zap size={18} className="text-cs-accent" />
            <h2 className="font-semibold">
              {t("agentManager.compare.title", "Runtime Comparison")}
            </h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-cs-border transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Tabs */}
        <div className="flex border-b border-cs-border">
          {[
            { id: "features", label: "Features" },
            { id: "config", label: "Configuration" },
            { id: "recommend", label: "Best For" },
          ].map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id as typeof activeTab)}
              className={cn(
                "px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors",
                activeTab === tab.id
                  ? "border-cs-accent text-cs-accent"
                  : "border-transparent text-cs-muted hover:text-cs-text"
              )}
            >
              {tab.label}
            </button>
          ))}
        </div>

        {/* Content */}
        <div className="p-4 max-h-[500px] overflow-y-auto">
          {activeTab === "features" && (
            <table className="w-full">
              <thead>
                <tr className="border-b border-cs-border">
                  <th className="text-left py-2 px-3 text-sm font-medium text-cs-muted">Feature</th>
                  <th className="text-center py-2 px-3 text-sm font-medium text-orange-400">Claude</th>
                  <th className="text-center py-2 px-3 text-sm font-medium text-green-400">Codex</th>
                  <th className="text-center py-2 px-3 text-sm font-medium text-purple-400">Hermes</th>
                  <th className="text-center py-2 px-3 text-sm font-medium text-cyan-400">OpenClaw</th>
                </tr>
              </thead>
              <tbody>
                {CAPABILITIES.map((cap) => (
                  <tr key={cap.name} className="border-b border-cs-border/50">
                    <td className="py-2.5 px-3 text-sm">{cap.name}</td>
                    <td className="py-2.5 px-3 text-center">{renderValue(cap.claude)}</td>
                    <td className="py-2.5 px-3 text-center">{renderValue(cap.codex)}</td>
                    <td className="py-2.5 px-3 text-center">{renderValue(cap.hermes)}</td>
                    <td className="py-2.5 px-3 text-center">{renderValue(cap.openclaw)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}

          {activeTab === "config" && (
            <table className="w-full">
              <thead>
                <tr className="border-b border-cs-border">
                  <th className="text-left py-2 px-3 text-sm font-medium text-cs-muted">Setting</th>
                  <th className="text-center py-2 px-3 text-sm font-medium text-orange-400">Claude</th>
                  <th className="text-center py-2 px-3 text-sm font-medium text-green-400">Codex</th>
                  <th className="text-center py-2 px-3 text-sm font-medium text-purple-400">Hermes</th>
                  <th className="text-center py-2 px-3 text-sm font-medium text-cyan-400">OpenClaw</th>
                </tr>
              </thead>
              <tbody>
                {CONFIG_INFO.map((info) => (
                  <tr key={info.name} className="border-b border-cs-border/50">
                    <td className="py-2.5 px-3 text-sm">{info.name}</td>
                    <td className="py-2.5 px-3 text-center text-sm font-mono text-cs-muted">{info.claude}</td>
                    <td className="py-2.5 px-3 text-center text-sm font-mono text-cs-muted">{info.codex}</td>
                    <td className="py-2.5 px-3 text-center text-sm font-mono text-cs-muted">{info.hermes}</td>
                    <td className="py-2.5 px-3 text-center text-sm font-mono text-cs-muted">{info.openclaw}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}

          {activeTab === "recommend" && (
            <div className="grid grid-cols-2 gap-4">
              {(Object.keys(BEST_FOR) as Array<keyof typeof BEST_FOR>).map((runtime) => (
                <div
                  key={runtime}
                  className={cn(
                    "rounded-lg border p-4",
                    RUNTIME_COLORS[runtime]
                  )}
                >
                  <h3 className="font-semibold capitalize mb-3">{runtime}</h3>
                  <ul className="space-y-1.5">
                    {BEST_FOR[runtime].map((item) => (
                      <li key={item} className="flex items-center gap-2 text-sm text-cs-text">
                        <Check size={14} className="shrink-0" />
                        {item}
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-cs-border">
          <p className="text-xs text-cs-muted flex items-center gap-1">
            <HelpCircle size={12} />
            Capabilities may vary by version and configuration
          </p>
          <button
            onClick={onClose}
            className="px-4 py-2 rounded-md text-sm bg-cs-accent text-black font-medium hover:bg-cs-accent/90 transition-colors"
          >
            {t("common.close", "Close")}
          </button>
        </div>
      </div>
    </div>
  );
}
