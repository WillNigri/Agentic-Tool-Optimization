import { useTranslation } from "react-i18next";
import { Play, Loader2, Terminal, Cpu, Server, Globe, Link2 } from "lucide-react";
import { cn } from "@/lib/utils";
import type { CronJob, CronExecution, AgentRuntime } from "./types";
import ExecutionTimeline from "./ExecutionTimeline";
import { cronToHuman, formatRelativeTime } from "@/lib/cron-utils";

const RUNTIME_ICON: Record<AgentRuntime, typeof Terminal> = {
  claude: Terminal,
  codex: Cpu,
  openclaw: Server,
  hermes: Globe,
};

const RUNTIME_COLOR: Record<AgentRuntime, string> = {
  claude: "#f97316",
  codex: "#22c55e",
  openclaw: "#06b6d4",
  hermes: "#a855f7",
};

const STATUS_COLORS: Record<string, { dot: string; text: string; label: string }> = {
  healthy: { dot: "bg-green-400", text: "text-green-400", label: "Healthy" },
  failed: { dot: "bg-red-500", text: "text-red-400", label: "Failed" },
  "silent-failure": { dot: "bg-red-500 animate-pulse", text: "text-red-400", label: "Silent Failure" },
  warning: { dot: "bg-yellow-500", text: "text-yellow-400", label: "Warning" },
  paused: { dot: "bg-gray-500", text: "text-gray-400", label: "Paused" },
};

interface CronJobCardProps {
  job: CronJob;
  executions: CronExecution[];
  isSelected: boolean;
  isRunning: boolean;
  onClick: () => void;
  onTrigger: () => void;
}

export default function CronJobCard({
  job,
  executions,
  isSelected,
  isRunning,
  onClick,
  onTrigger,
}: CronJobCardProps) {
  const { t } = useTranslation();
  const RuntimeIcon = RUNTIME_ICON[job.runtime];
  const runtimeColor = RUNTIME_COLOR[job.runtime];
  const statusConfig = STATUS_COLORS[job.status] || STATUS_COLORS.healthy;

  return (
    <div
      onClick={onClick}
      className={cn(
        "card cursor-pointer transition-colors",
        isSelected
          ? "border-cs-accent/50 bg-cs-accent/5"
          : "hover:border-cs-border/80"
      )}
    >
      <div className="flex items-start justify-between gap-3">
        {/* Left content */}
        <div className="min-w-0 flex-1">
          {/* Title row */}
          <div className="flex items-center gap-2 mb-1 flex-wrap">
            <span className={cn("w-2 h-2 rounded-full shrink-0", statusConfig.dot)} />
            <p className="text-sm font-medium truncate">{job.name}</p>

            {/* Runtime badge */}
            <span
              className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider rounded-full border"
              style={{
                borderColor: `${runtimeColor}66`,
                background: `${runtimeColor}18`,
                color: runtimeColor,
              }}
            >
              <RuntimeIcon size={10} />
              {job.runtime}
            </span>

            {/* Status */}
            <span className={cn("text-[10px] font-medium", statusConfig.text)}>
              {t(`cron.status.${job.status}`)}
            </span>
          </div>

          {/* Schedule */}
          <p className="text-xs text-cs-muted mb-1.5">
            {cronToHuman(job.schedule)}
          </p>

          {/* Last/Next run */}
          <div className="flex items-center gap-4 text-[11px] text-cs-muted mb-2">
            {job.lastRunAt && (
              <span>
                {t("cron.job.lastRun")}: {formatRelativeTime(job.lastRunAt)}
              </span>
            )}
            {job.nextRunAt && job.enabled && (
              <span>
                {t("cron.job.nextRun")}: {formatRelativeTime(job.nextRunAt)}
              </span>
            )}
          </div>

          {/* Linked workflow */}
          {job.linkedWorkflowId && (
            <div className="flex items-center gap-1.5 text-[11px] text-cs-accent mb-2">
              <Link2 size={10} />
              <span>{t("cron.job.linkedWorkflow", { name: job.linkedWorkflowId })}</span>
            </div>
          )}

          {/* Timeline */}
          <ExecutionTimeline executions={executions} />
        </div>

        {/* Right: Run Now button */}
        <div className="shrink-0 pt-1">
          <button
            onClick={(e) => {
              e.stopPropagation();
              onTrigger();
            }}
            disabled={isRunning || !job.enabled}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium border transition-colors",
              isRunning
                ? "border-yellow-500/40 text-yellow-400 bg-yellow-500/10"
                : job.enabled
                  ? "border-cs-accent/40 text-cs-accent bg-cs-accent/10 hover:bg-cs-accent/20"
                  : "border-cs-border text-cs-muted opacity-50 cursor-not-allowed"
            )}
          >
            {isRunning ? (
              <>
                <Loader2 size={12} className="animate-spin" />
                {t("cron.job.running")}
              </>
            ) : (
              <>
                <Play size={12} />
                {t("cron.job.runNow")}
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
