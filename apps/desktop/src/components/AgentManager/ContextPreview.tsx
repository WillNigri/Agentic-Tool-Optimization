import { useTranslation } from "react-i18next";
import { Info, FileText } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAgentConfigStore } from "@/stores/useAgentConfigStore";

const SECTION_COLORS: Record<string, string> = {
  "System Prompt": "#00FFB2",
  "Project Config": "#FFB800",
  Skills: "#FF6B6B",
  "MCP Schemas": "#9B59B6",
  Conversation: "#3498DB",
};

export default function ContextPreview() {
  const { t } = useTranslation();
  const { contextPreview, activeRuntime } = useAgentConfigStore();

  if (!contextPreview) {
    return (
      <div className="h-full flex items-center justify-center text-cs-muted">
        <div className="text-center">
          <Info size={48} className="mx-auto mb-3 opacity-50" />
          <p>{t("agentManager.preview.loading", "Loading context preview...")}</p>
        </div>
      </div>
    );
  }

  const usagePercent = Math.round((contextPreview.totalTokens / contextPreview.limit) * 100);
  const isHigh = usagePercent > 70;
  const isCritical = usagePercent > 90;

  return (
    <div className="h-full overflow-auto">
      <div className="max-w-3xl mx-auto">
        {/* Header stats */}
        <div className="grid grid-cols-3 gap-4 mb-6">
          <div className="bg-cs-card border border-cs-border rounded-lg p-4">
            <div className="text-sm text-cs-muted mb-1">
              {t("agentManager.preview.totalUsed", "Total Used")}
            </div>
            <div className="text-2xl font-bold">
              {contextPreview.totalTokens.toLocaleString()}
            </div>
            <div className="text-sm text-cs-muted">
              {t("agentManager.preview.tokens", "tokens")}
            </div>
          </div>

          <div className="bg-cs-card border border-cs-border rounded-lg p-4">
            <div className="text-sm text-cs-muted mb-1">
              {t("agentManager.preview.limit", "Context Limit")}
            </div>
            <div className="text-2xl font-bold">
              {contextPreview.limit.toLocaleString()}
            </div>
            <div className="text-sm text-cs-muted">
              {activeRuntime === "all" ? "claude" : activeRuntime}
            </div>
          </div>

          <div className="bg-cs-card border border-cs-border rounded-lg p-4">
            <div className="text-sm text-cs-muted mb-1">
              {t("agentManager.preview.usage", "Usage")}
            </div>
            <div
              className={cn(
                "text-2xl font-bold",
                isCritical
                  ? "text-red-400"
                  : isHigh
                  ? "text-yellow-400"
                  : "text-cs-accent"
              )}
            >
              {usagePercent}%
            </div>
            <div className="text-sm text-cs-muted">
              {t("agentManager.preview.ofLimit", "of limit")}
            </div>
          </div>
        </div>

        {/* Progress bar */}
        <div className="mb-6">
          <div className="h-4 bg-cs-border rounded-full overflow-hidden">
            <div
              className={cn(
                "h-full transition-all duration-500",
                isCritical
                  ? "bg-red-400"
                  : isHigh
                  ? "bg-yellow-400"
                  : "bg-cs-accent"
              )}
              style={{ width: `${Math.min(usagePercent, 100)}%` }}
            />
          </div>
        </div>

        {/* Section breakdown */}
        <div className="space-y-3">
          <h3 className="text-sm font-medium text-cs-muted mb-3">
            {t("agentManager.preview.breakdown", "Context Breakdown")}
          </h3>

          {contextPreview.sections.map((section) => {
            const sectionPercent = Math.round(
              (section.tokens / contextPreview.totalTokens) * 100
            );
            const color = SECTION_COLORS[section.name] || "#00FFB2";

            return (
              <div
                key={section.name}
                className="bg-cs-card border border-cs-border rounded-lg p-4"
              >
                <div className="flex items-center justify-between mb-2">
                  <div className="flex items-center gap-2">
                    <div
                      className="w-3 h-3 rounded-full"
                      style={{ backgroundColor: color }}
                    />
                    <span className="font-medium">{section.name}</span>
                  </div>
                  <div className="text-sm">
                    <span className="font-mono">
                      {section.tokens.toLocaleString()}
                    </span>
                    <span className="text-cs-muted ml-1">tokens</span>
                    <span className="text-cs-muted ml-2">({sectionPercent}%)</span>
                  </div>
                </div>

                {/* Mini progress bar */}
                <div className="h-1.5 bg-cs-border rounded-full overflow-hidden mb-3">
                  <div
                    className="h-full transition-all duration-300"
                    style={{
                      width: `${sectionPercent}%`,
                      backgroundColor: color,
                    }}
                  />
                </div>

                {/* Source files */}
                {section.files.length > 0 && (
                  <div className="text-sm text-cs-muted">
                    {section.files.map((file, idx) => (
                      <div key={idx} className="flex items-center gap-2 py-0.5">
                        <FileText size={12} />
                        <span className="truncate">{file}</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {/* Note about skills */}
        <div className="mt-6 p-4 bg-cs-card border border-cs-border rounded-lg">
          <div className="flex items-start gap-3">
            <Info size={16} className="text-cs-accent mt-0.5" />
            <div>
              <p className="text-sm font-medium">
                {t("agentManager.preview.skillsNote", "Skills are on-demand")}
              </p>
              <p className="text-sm text-cs-muted mt-1">
                {t(
                  "agentManager.preview.skillsNoteDetail",
                  "Skills are not counted in the context total. They are only loaded when triggered by your prompts."
                )}
              </p>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
