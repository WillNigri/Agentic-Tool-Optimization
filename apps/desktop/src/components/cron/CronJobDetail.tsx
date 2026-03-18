import { useTranslation } from "react-i18next";
import {
  X,
  Clock,
  CheckCircle,
  AlertTriangle,
  Loader2,
  RotateCcw,
  Trash2,
  Terminal,
  Cpu,
  Server,
  Globe,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { CronJob, CronExecution, AgentRuntime } from "./types";
import { cronToHuman, formatRelativeTime } from "@/lib/cron-utils";
import { useCronStore } from "@/stores/useCronStore";
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  CartesianGrid,
} from "recharts";

const RUNTIME_ICON: Record<AgentRuntime, typeof Terminal> = {
  claude: Terminal,
  codex: Cpu,
  openclaw: Server,
  hermes: Globe,
};

interface CronJobDetailProps {
  job: CronJob;
  onClose: () => void;
}

export default function CronJobDetail({ job, onClose }: CronJobDetailProps) {
  const { t } = useTranslation();
  const executions = useCronStore((s) => s.getJobExecutions(job.id));
  const retryExecution = useCronStore((s) => s.retryExecution);
  const deleteJob = useCronStore((s) => s.deleteJob);
  const toggleJob = useCronStore((s) => s.toggleJob);
  const RuntimeIcon = RUNTIME_ICON[job.runtime];

  const completedExecs = executions.filter((e) => e.status === "success" || e.status === "failed");
  const successCount = completedExecs.filter((e) => e.status === "success").length;
  const successRate = completedExecs.length > 0
    ? Math.round((successCount / completedExecs.length) * 100)
    : 0;
  const avgDuration = completedExecs.length > 0
    ? Math.round(
        completedExecs.reduce((sum, e) => sum + (e.durationMs || 0), 0) / completedExecs.length
      )
    : 0;

  // Chart data: last 7 executions
  const chartData = executions
    .filter((e) => e.durationMs != null)
    .slice(0, 10)
    .reverse()
    .map((e, i) => ({
      name: `#${i + 1}`,
      duration: Math.round((e.durationMs || 0) / 1000 * 10) / 10,
      status: e.status,
    }));

  function handleDelete() {
    if (confirm(t("cron.job.confirmDelete"))) {
      deleteJob(job.id);
      onClose();
    }
  }

  return (
    <>
      {/* Backdrop */}
      <div className="fixed inset-0 bg-black/30 z-40 lg:hidden" onClick={onClose} />

      {/* Panel */}
      <div className="fixed right-0 top-0 bottom-0 w-full lg:w-1/2 max-w-2xl bg-cs-card border-l border-cs-border z-50 flex flex-col shadow-2xl animate-slide-in-right">
        {/* Header */}
        <div className="flex items-start justify-between p-4 border-b border-cs-border">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2 flex-wrap">
              <Clock size={18} className="text-cs-accent shrink-0" />
              <h3 className="text-lg font-semibold truncate">{job.name}</h3>
              <span
                className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider rounded-full border border-cs-border"
              >
                <RuntimeIcon size={10} />
                {job.runtime}
              </span>
            </div>
            <p className="text-xs text-cs-muted mt-1">{job.description}</p>
            <p className="text-xs text-cs-muted mt-0.5 font-mono">{job.schedule} — {cronToHuman(job.schedule)}</p>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
          >
            <X size={16} />
          </button>
        </div>

        <div className="p-4 space-y-5 overflow-y-auto flex-1">
          {/* Stats cards */}
          <div className="grid grid-cols-3 gap-3">
            <div className="rounded-lg border border-cs-border p-3 text-center">
              <p className="text-2xl font-bold text-cs-text">{successRate}%</p>
              <p className="text-[10px] text-cs-muted uppercase tracking-wider">{t("cron.detail.successRate")}</p>
            </div>
            <div className="rounded-lg border border-cs-border p-3 text-center">
              <p className="text-2xl font-bold text-cs-text">
                {avgDuration > 1000 ? `${(avgDuration / 1000).toFixed(1)}s` : `${avgDuration}ms`}
              </p>
              <p className="text-[10px] text-cs-muted uppercase tracking-wider">{t("cron.detail.avgDuration")}</p>
            </div>
            <div className="rounded-lg border border-cs-border p-3 text-center">
              <p className="text-2xl font-bold text-cs-text">{completedExecs.length}</p>
              <p className="text-[10px] text-cs-muted uppercase tracking-wider">{t("cron.detail.totalRuns")}</p>
            </div>
          </div>

          {/* Duration chart */}
          {chartData.length > 0 && (
            <div>
              <h4 className="text-xs font-semibold text-cs-muted uppercase tracking-wider mb-2">
                {t("cron.detail.duration")}
              </h4>
              <div className="h-32 rounded-lg border border-cs-border p-2" style={{ background: "#0e0e16" }}>
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={chartData}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#2a2a3a" />
                    <XAxis dataKey="name" tick={{ fontSize: 9, fill: "#8888a0" }} />
                    <YAxis tick={{ fontSize: 9, fill: "#8888a0" }} unit="s" />
                    <Tooltip
                      contentStyle={{
                        background: "#16161e",
                        border: "1px solid #2a2a3a",
                        borderRadius: 8,
                        fontSize: 11,
                      }}
                    />
                    <Bar
                      dataKey="duration"
                      fill="#00FFB2"
                      radius={[3, 3, 0, 0]}
                    />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </div>
          )}

          {/* Execution history */}
          <div>
            <h4 className="text-xs font-semibold text-cs-muted uppercase tracking-wider mb-2">
              {t("cron.detail.executionHistory")}
            </h4>
            {executions.length === 0 ? (
              <p className="text-sm text-cs-muted text-center py-4">{t("cron.detail.noExecutions")}</p>
            ) : (
              <div className="space-y-1.5 max-h-64 overflow-y-auto">
                {executions.slice(0, 20).map((exec) => (
                  <div
                    key={exec.id}
                    className="flex items-center gap-3 px-3 py-2 rounded-lg border border-cs-border"
                  >
                    {/* Status icon */}
                    {exec.status === "success" && <CheckCircle size={14} className="text-green-400 shrink-0" />}
                    {exec.status === "failed" && <AlertTriangle size={14} className="text-red-400 shrink-0" />}
                    {exec.status === "running" && <Loader2 size={14} className="text-yellow-400 animate-spin shrink-0" />}
                    {exec.status === "skipped" && <Clock size={14} className="text-gray-400 shrink-0" />}

                    {/* Time */}
                    <div className="flex-1 min-w-0">
                      <p className="text-xs text-cs-text truncate">
                        {formatRelativeTime(exec.startedAt)}
                        {exec.durationMs != null && (
                          <span className="text-cs-muted ml-2">
                            {exec.durationMs > 1000
                              ? `${(exec.durationMs / 1000).toFixed(1)}s`
                              : `${exec.durationMs}ms`}
                          </span>
                        )}
                      </p>
                      {exec.error && (
                        <p className="text-[11px] text-red-400 truncate">{exec.error}</p>
                      )}
                      {exec.output && !exec.error && (
                        <p className="text-[11px] text-cs-muted truncate">{exec.output}</p>
                      )}
                      {exec.retryOf && (
                        <p className="text-[10px] text-yellow-400">Retry</p>
                      )}
                    </div>

                    {/* Retry button for failed executions */}
                    {exec.status === "failed" && (
                      <button
                        onClick={() => retryExecution(exec.id)}
                        className="flex items-center gap-1 px-2 py-1 text-[10px] rounded border border-yellow-500/30 text-yellow-400 hover:bg-yellow-500/10 transition-colors shrink-0"
                      >
                        <RotateCcw size={10} />
                        {t("cron.job.retry")}
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* Footer actions */}
        <div className="flex gap-2 p-4 border-t border-cs-border">
          <button
            onClick={() => toggleJob(job.id)}
            className={cn(
              "px-4 py-2 text-sm rounded-lg border font-medium transition-colors",
              job.enabled
                ? "border-yellow-500/30 text-yellow-400 hover:bg-yellow-500/10"
                : "border-cs-accent/30 text-cs-accent hover:bg-cs-accent/10"
            )}
          >
            {job.enabled ? "Pause" : "Resume"}
          </button>
          <div className="flex-1" />
          <button
            onClick={handleDelete}
            className="flex items-center gap-1.5 px-4 py-2 text-sm rounded-lg border border-red-500/30 text-red-400 hover:bg-red-500/10 transition-colors"
          >
            <Trash2 size={14} />
            {t("common.delete")}
          </button>
        </div>
      </div>
    </>
  );
}
