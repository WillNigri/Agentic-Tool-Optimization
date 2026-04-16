import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  X,
  CheckCircle,
  Circle,
  ExternalLink,
  FilePlus,
  Terminal,
  ChevronRight,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  getOnboardingStatus,
  createAgentSkill,
  type AgentConfigRuntime,
  type OnboardingItem,
} from "@/lib/api";

interface Props {
  onClose: () => void;
}

const RUNTIMES: { value: AgentConfigRuntime; label: string; color: string }[] = [
  { value: "claude", label: "Claude Code", color: "text-orange-400" },
  { value: "codex", label: "Codex", color: "text-green-400" },
  { value: "hermes", label: "Hermes", color: "text-purple-400" },
  { value: "openclaw", label: "OpenClaw", color: "text-cyan-400" },
];

export default function OnboardingModal({ onClose }: Props) {
  const { t } = useTranslation();
  const [selectedRuntime, setSelectedRuntime] = useState<AgentConfigRuntime>("claude");

  const { data: status, isLoading, refetch } = useQuery({
    queryKey: ["onboarding-status", selectedRuntime],
    queryFn: () => getOnboardingStatus(selectedRuntime),
  });

  const handleAction = async (item: OnboardingItem) => {
    if (!item.action) return;

    switch (item.action.actionType) {
      case "external_link":
        window.open(item.action.target, "_blank");
        break;
      case "run_command":
        // Could integrate with terminal, for now show instructions
        alert(`Run this command in your terminal:\n\n${item.action.target}`);
        break;
      case "create_file":
        // Could create the file via Tauri command
        alert(`Create file at:\n\n${item.action.target}`);
        break;
    }

    // Refresh status after action
    setTimeout(() => refetch(), 1000);
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-cs-card border border-cs-border rounded-lg w-full max-w-2xl mx-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border">
          <h2 className="font-semibold">
            {t("agentManager.onboarding.title", "Setup Guide")}
          </h2>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-cs-border transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Runtime tabs */}
        <div className="flex border-b border-cs-border">
          {RUNTIMES.map((rt) => (
            <button
              key={rt.value}
              onClick={() => setSelectedRuntime(rt.value)}
              className={cn(
                "flex-1 px-4 py-3 text-sm font-medium transition-colors border-b-2 -mb-px",
                selectedRuntime === rt.value
                  ? `border-cs-accent ${rt.color}`
                  : "border-transparent text-cs-muted hover:text-cs-text"
              )}
            >
              {rt.label}
            </button>
          ))}
        </div>

        {/* Content */}
        <div className="p-4">
          {isLoading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 size={24} className="animate-spin text-cs-muted" />
            </div>
          ) : status ? (
            <>
              {/* Progress bar */}
              <div className="mb-6">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm text-cs-muted">Setup Progress</span>
                  <span className="text-sm font-medium">{status.completionPercent}%</span>
                </div>
                <div className="h-2 bg-cs-border rounded-full overflow-hidden">
                  <div
                    className="h-full bg-cs-accent transition-all duration-300"
                    style={{ width: `${status.completionPercent}%` }}
                  />
                </div>
              </div>

              {/* Checklist */}
              <div className="space-y-2">
                {status.items.map((item) => (
                  <div
                    key={item.id}
                    className={cn(
                      "flex items-center justify-between p-3 rounded-lg border transition-colors",
                      item.completed
                        ? "border-green-500/20 bg-green-500/5"
                        : "border-cs-border hover:border-cs-muted"
                    )}
                  >
                    <div className="flex items-center gap-3">
                      {item.completed ? (
                        <CheckCircle size={18} className="text-green-400" />
                      ) : (
                        <Circle size={18} className="text-cs-muted" />
                      )}
                      <span className={cn(
                        "text-sm",
                        item.completed ? "text-cs-muted" : "text-cs-text"
                      )}>
                        {item.label}
                      </span>
                    </div>

                    {!item.completed && item.action && (
                      <button
                        onClick={() => handleAction(item)}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-cs-accent/10 text-cs-accent text-xs font-medium hover:bg-cs-accent/20 transition-colors"
                      >
                        {item.action.actionType === "external_link" && (
                          <>
                            <ExternalLink size={12} />
                            Open Docs
                          </>
                        )}
                        {item.action.actionType === "create_file" && (
                          <>
                            <FilePlus size={12} />
                            Create
                          </>
                        )}
                        {item.action.actionType === "run_command" && (
                          <>
                            <Terminal size={12} />
                            Run
                          </>
                        )}
                      </button>
                    )}
                  </div>
                ))}
              </div>

              {status.completionPercent === 100 && (
                <div className="mt-6 p-4 rounded-lg bg-green-500/10 border border-green-500/20 text-center">
                  <CheckCircle size={24} className="mx-auto mb-2 text-green-400" />
                  <p className="text-sm text-green-400 font-medium">
                    {t("agentManager.onboarding.complete", "Setup complete! You're ready to use this runtime.")}
                  </p>
                </div>
              )}
            </>
          ) : null}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end px-4 py-3 border-t border-cs-border">
          <button
            onClick={onClose}
            className="px-4 py-2 rounded-md text-sm bg-cs-accent text-black font-medium hover:bg-cs-accent/90 transition-colors"
          >
            {t("common.done", "Done")}
          </button>
        </div>
      </div>
    </div>
  );
}
