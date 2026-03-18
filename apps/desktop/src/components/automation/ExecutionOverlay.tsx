import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { X, Clock, CheckCircle, AlertTriangle, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAutomationStore } from "@/stores/useAutomationStore";
import type { ExecutionNodeStatus } from "./types";

const STATUS_ICONS: Record<ExecutionNodeStatus, React.ElementType> = {
  pending: Clock,
  running: Loader2,
  completed: CheckCircle,
  failed: AlertTriangle,
};

const STATUS_COLORS: Record<ExecutionNodeStatus, string> = {
  pending: "#8888a0",
  running: "#FFB800",
  completed: "#00FFB2",
  failed: "#FF4466",
};

export default function ExecutionOverlay() {
  const { t } = useTranslation();
  const execution = useAutomationStore((s) => s.execution);
  const workflow = useAutomationStore((s) => s.getActiveWorkflow());
  const finishExecution = useAutomationStore((s) => s.finishExecution);
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    if (!execution.running || !execution.startedAt) return;
    const interval = setInterval(() => {
      setElapsed(Math.floor((Date.now() - execution.startedAt!) / 1000));
    }, 1000);
    return () => clearInterval(interval);
  }, [execution.running, execution.startedAt]);

  if (!execution.running && !execution.output) return null;

  return (
    <div
      className="border-t border-[#2a2a3a]"
      style={{ background: "#12121a", maxHeight: 280, zIndex: 40 }}
    >
      {/* Header */}
      <div className="flex items-center gap-3 px-4 py-2 border-b border-[#2a2a3a]">
        <div className="flex items-center gap-2">
          {execution.running ? (
            <Loader2 size={14} className="text-[#FFB800] animate-spin" />
          ) : execution.error ? (
            <AlertTriangle size={14} className="text-[#FF4466]" />
          ) : (
            <CheckCircle size={14} className="text-[#00FFB2]" />
          )}
          <span className="text-xs font-semibold text-[#e8e8f0]">
            {execution.running
              ? t("automation.builder.executing", "Executing...")
              : execution.error
                ? t("automation.builder.failed", "Failed")
                : t("automation.builder.completed", "Completed")}
          </span>
        </div>

        {/* Elapsed time */}
        <span className="text-[10px] text-[#8888a0] font-mono">
          {elapsed}s
        </span>

        {/* Node status pills */}
        <div className="flex items-center gap-1 flex-1">
          {workflow.nodes.slice(0, 8).map((node) => {
            const status = execution.nodeStatuses[node.id] || "pending";
            const Icon = STATUS_ICONS[status];
            const color = STATUS_COLORS[status];
            return (
              <div
                key={node.id}
                className="flex items-center gap-1 rounded px-1.5 py-0.5"
                style={{ background: `${color}15` }}
                title={node.label}
              >
                <Icon size={9} style={{ color }} className={cn(status === "running" && "animate-spin")} />
                <span className="text-[8px] font-medium truncate max-w-[60px]" style={{ color }}>
                  {node.label}
                </span>
              </div>
            );
          })}
        </div>

        {/* Close */}
        {!execution.running && (
          <button
            onClick={() => finishExecution()}
            className="flex items-center justify-center w-6 h-6 rounded-md border border-[#2a2a3a] hover:border-[#FF4466] transition-colors"
          >
            <X size={12} className="text-[#8888a0]" />
          </button>
        )}
      </div>

      {/* Output */}
      <div className="px-4 py-2 overflow-y-auto" style={{ maxHeight: 220 }}>
        <pre className="text-[11px] text-[#e8e8f0] font-mono whitespace-pre-wrap leading-relaxed">
          {execution.output || t("automation.builder.waitingOutput", "Waiting for output...")}
        </pre>
        {execution.error && (
          <div className="mt-2 rounded-md border border-[#FF4466]/30 bg-[#FF446610] px-3 py-2">
            <p className="text-xs text-[#FF4466]">{execution.error}</p>
          </div>
        )}
      </div>
    </div>
  );
}
