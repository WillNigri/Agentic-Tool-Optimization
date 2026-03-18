import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronLeft,
  ChevronRight,
  Clock,
  Terminal,
  Cpu,
  Server,
  Globe,
  CheckCircle,
  AlertTriangle,
  X,
  Lock,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { CronJob, CronExecution, AgentRuntime } from "./types";
import { parseCron, matchesCronDate } from "@/lib/cron-utils";
import { useCronStore } from "@/stores/useCronStore";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const WEEKDAY_LABELS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

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

interface CalendarDay {
  date: Date;
  dateStr: string; // YYYY-MM-DD
  isCurrentMonth: boolean;
  isToday: boolean;
  jobs: CronJob[];
  executions: CronExecution[];
}

function getCalendarDays(year: number, month: number, jobs: CronJob[], executions: CronExecution[]): CalendarDay[] {
  const firstDay = new Date(year, month, 1);
  const lastDay = new Date(year, month + 1, 0);
  const startOffset = firstDay.getDay(); // 0=Sun
  const today = new Date();
  const todayStr = today.toISOString().slice(0, 10);

  const days: CalendarDay[] = [];

  // Fill previous month's trailing days
  for (let i = startOffset - 1; i >= 0; i--) {
    const d = new Date(year, month, -i);
    const dateStr = d.toISOString().slice(0, 10);
    days.push({
      date: d,
      dateStr,
      isCurrentMonth: false,
      isToday: dateStr === todayStr,
      jobs: getJobsForDate(d, jobs),
      executions: getExecutionsForDate(dateStr, executions),
    });
  }

  // Current month days
  for (let day = 1; day <= lastDay.getDate(); day++) {
    const d = new Date(year, month, day);
    const dateStr = d.toISOString().slice(0, 10);
    days.push({
      date: d,
      dateStr,
      isCurrentMonth: true,
      isToday: dateStr === todayStr,
      jobs: getJobsForDate(d, jobs),
      executions: getExecutionsForDate(dateStr, executions),
    });
  }

  // Fill remaining cells to complete 6 rows (42 cells)
  const remaining = 42 - days.length;
  for (let i = 1; i <= remaining; i++) {
    const d = new Date(year, month + 1, i);
    const dateStr = d.toISOString().slice(0, 10);
    days.push({
      date: d,
      dateStr,
      isCurrentMonth: false,
      isToday: dateStr === todayStr,
      jobs: getJobsForDate(d, jobs),
      executions: getExecutionsForDate(dateStr, executions),
    });
  }

  return days;
}

function getJobsForDate(date: Date, jobs: CronJob[]): CronJob[] {
  return jobs.filter((job) => {
    if (!job.enabled) return false;
    return matchesCronDate(job.schedule, date);
  });
}

function getExecutionsForDate(dateStr: string, executions: CronExecution[]): CronExecution[] {
  return executions.filter((e) => e.startedAt.slice(0, 10) === dateStr);
}

/** Get the scheduled time from a cron expression (HH:MM) */
function getScheduledTime(schedule: string): string {
  const parsed = parseCron(schedule);
  if (!parsed) return "";
  const h = parsed.hour === "*" ? "--" : parsed.hour.padStart(2, "0");
  const m = parsed.minute === "*" ? "--" : parsed.minute.padStart(2, "0");
  return `${h}:${m}`;
}

// ---------------------------------------------------------------------------
// Execution status for a job on a specific day (paid feature preview)
// ---------------------------------------------------------------------------

type DayJobStatus = "success" | "failed" | "scheduled" | "not-run";

function getJobDayStatus(
  job: CronJob,
  dayExecs: CronExecution[],
  dayDate: Date
): DayJobStatus {
  const jobExecs = dayExecs.filter((e) => e.jobId === job.id);
  if (jobExecs.length === 0) {
    // No execution — if the day is in the past, it was missed
    const now = new Date();
    now.setHours(23, 59, 59, 999);
    if (dayDate < now) return "not-run";
    return "scheduled";
  }
  const hasFailed = jobExecs.some((e) => e.status === "failed");
  if (hasFailed) return "failed";
  return "success";
}

// ---------------------------------------------------------------------------
// Day Cell Component
// ---------------------------------------------------------------------------

interface DayCellProps {
  day: CalendarDay;
  onSelectJob: (jobId: string) => void;
  onSelectExecution: (exec: CronExecution) => void;
}

function DayCell({ day, onSelectJob, onSelectExecution }: DayCellProps) {
  const { t } = useTranslation();

  return (
    <div
      className={cn(
        "min-h-[100px] border-r border-b border-[#2a2a3a] p-1 transition-colors",
        day.isCurrentMonth ? "bg-cs-card" : "bg-[#0a0a0f]/50",
        day.isToday && "ring-1 ring-inset ring-cs-accent/40"
      )}
    >
      {/* Day number */}
      <div className="flex items-center justify-between mb-0.5 px-0.5">
        <span
          className={cn(
            "text-[11px] font-medium",
            day.isToday
              ? "text-cs-bg bg-cs-accent rounded-full w-5 h-5 flex items-center justify-center text-[10px] font-bold"
              : day.isCurrentMonth
                ? "text-cs-text"
                : "text-cs-muted/40"
          )}
        >
          {day.date.getDate()}
        </span>
        {day.jobs.length > 0 && (
          <span className="text-[9px] text-cs-muted">
            {day.jobs.length}
          </span>
        )}
      </div>

      {/* Job entries */}
      <div className="space-y-0.5">
        {day.jobs.slice(0, 4).map((job) => {
          const status = getJobDayStatus(job, day.executions, day.date);
          const RuntimeIcon = RUNTIME_ICON[job.runtime];
          const color = RUNTIME_COLOR[job.runtime];
          const time = getScheduledTime(job.schedule);

          // Find matching execution for click-to-inspect
          const jobExec = day.executions.find((e) => e.jobId === job.id);

          return (
            <button
              key={job.id}
              onClick={() => {
                if (jobExec && (status === "success" || status === "failed")) {
                  onSelectExecution(jobExec);
                } else {
                  onSelectJob(job.id);
                }
              }}
              className={cn(
                "w-full flex items-center gap-1 px-1 py-0.5 rounded text-left transition-colors group",
                status === "success" && "bg-green-500/10 hover:bg-green-500/20",
                status === "failed" && "bg-red-500/10 hover:bg-red-500/20",
                status === "scheduled" && "bg-[#2a2a3a]/50 hover:bg-[#2a2a3a]",
                status === "not-run" && "bg-yellow-500/5 hover:bg-yellow-500/10 opacity-60"
              )}
            >
              {/* Status indicator */}
              {status === "success" && (
                <CheckCircle size={8} className="text-green-400 shrink-0" />
              )}
              {status === "failed" && (
                <AlertTriangle size={8} className="text-red-400 shrink-0" />
              )}
              {status === "scheduled" && (
                <Clock size={8} className="text-cs-muted/60 shrink-0" />
              )}
              {status === "not-run" && (
                <X size={8} className="text-yellow-500/60 shrink-0" />
              )}

              {/* Runtime icon */}
              <RuntimeIcon
                size={8}
                style={{ color }}
                className="shrink-0"
              />

              {/* Time + name */}
              <span className="text-[9px] text-cs-muted font-mono shrink-0">
                {time}
              </span>
              <span
                className={cn(
                  "text-[9px] truncate",
                  status === "failed" ? "text-red-300" :
                  status === "success" ? "text-green-300" :
                  "text-cs-muted"
                )}
              >
                {job.name}
              </span>
            </button>
          );
        })}
        {day.jobs.length > 4 && (
          <span className="text-[8px] text-cs-muted/60 px-1">
            +{day.jobs.length - 4} {t("cron.calendar.more")}
          </span>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Execution Detail Popover (paid feature — shows status + error)
// ---------------------------------------------------------------------------

function ExecutionPopover({
  execution,
  job,
  onClose,
}: {
  execution: CronExecution;
  job: CronJob | undefined;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const isFailed = execution.status === "failed";
  const isSuccess = execution.status === "success";

  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onClose} />
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <div
          className={cn(
            "bg-cs-card border rounded-xl w-full max-w-sm shadow-2xl",
            isFailed ? "border-red-500/40" : "border-green-500/40"
          )}
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className={cn(
            "flex items-center justify-between px-4 py-3 border-b",
            isFailed ? "border-red-500/20 bg-red-500/5" : "border-green-500/20 bg-green-500/5"
          )}>
            <div className="flex items-center gap-2">
              {isFailed ? (
                <AlertTriangle size={16} className="text-red-400" />
              ) : (
                <CheckCircle size={16} className="text-green-400" />
              )}
              <span className={cn("text-sm font-semibold", isFailed ? "text-red-400" : "text-green-400")}>
                {isFailed ? t("cron.calendar.executionFailed") : t("cron.calendar.executionSuccess")}
              </span>
            </div>
            <button
              onClick={onClose}
              className="p-1 rounded hover:bg-cs-border transition-colors text-cs-muted"
            >
              <X size={14} />
            </button>
          </div>

          <div className="p-4 space-y-3">
            {/* Job name */}
            {job && (
              <div>
                <p className="text-[10px] text-cs-muted uppercase tracking-wider">{t("cron.calendar.jobName")}</p>
                <p className="text-sm font-medium text-cs-text">{job.name}</p>
              </div>
            )}

            {/* Time */}
            <div className="grid grid-cols-2 gap-3">
              <div>
                <p className="text-[10px] text-cs-muted uppercase tracking-wider">{t("cron.calendar.startedAt")}</p>
                <p className="text-xs font-mono text-cs-text">
                  {new Date(execution.startedAt).toLocaleTimeString()}
                </p>
              </div>
              {execution.durationMs != null && (
                <div>
                  <p className="text-[10px] text-cs-muted uppercase tracking-wider">{t("cron.detail.duration")}</p>
                  <p className="text-xs font-mono text-cs-text">
                    {execution.durationMs > 1000
                      ? `${(execution.durationMs / 1000).toFixed(1)}s`
                      : `${execution.durationMs}ms`}
                  </p>
                </div>
              )}
            </div>

            {/* Error (failed) */}
            {isFailed && execution.error && (
              <div>
                <p className="text-[10px] text-red-400 uppercase tracking-wider mb-1">{t("cron.detail.error")}</p>
                <div className="rounded-lg border border-red-500/20 bg-red-500/5 p-2.5">
                  <p className="text-xs text-red-300 font-mono whitespace-pre-wrap">{execution.error}</p>
                </div>
              </div>
            )}

            {/* Output (success) */}
            {isSuccess && execution.output && (
              <div>
                <p className="text-[10px] text-green-400 uppercase tracking-wider mb-1">{t("cron.detail.output")}</p>
                <div className="rounded-lg border border-green-500/20 bg-green-500/5 p-2.5">
                  <p className="text-xs text-green-300 font-mono whitespace-pre-wrap line-clamp-4">{execution.output}</p>
                </div>
              </div>
            )}

            {/* Pro badge for monitoring features */}
            <div className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-cs-border bg-cs-bg/50">
              <Lock size={10} className="text-cs-muted" />
              <span className="text-[10px] text-cs-muted">
                {t("cron.calendar.proFeature")}
              </span>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

// ---------------------------------------------------------------------------
// Main Calendar Component
// ---------------------------------------------------------------------------

export default function CronCalendar() {
  const { t } = useTranslation();
  const [currentDate, setCurrentDate] = useState(new Date());
  const [selectedExecution, setSelectedExecution] = useState<CronExecution | null>(null);

  const jobs = useCronStore((s) => s.jobs);
  const executions = useCronStore((s) => s.executions);
  const selectJob = useCronStore((s) => s.selectJob);

  const year = currentDate.getFullYear();
  const month = currentDate.getMonth();

  const calendarDays = useMemo(
    () => getCalendarDays(year, month, jobs, executions),
    [year, month, jobs, executions]
  );

  const monthLabel = new Date(year, month).toLocaleString("default", {
    month: "long",
    year: "numeric",
  });

  function prevMonth() {
    setCurrentDate(new Date(year, month - 1, 1));
  }

  function nextMonth() {
    setCurrentDate(new Date(year, month + 1, 1));
  }

  function goToday() {
    setCurrentDate(new Date());
  }

  // Find the job for the selected execution
  const selectedExecJob = selectedExecution
    ? jobs.find((j) => j.id === selectedExecution.jobId)
    : undefined;

  return (
    <div>
      {/* Calendar header */}
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <button
            onClick={prevMonth}
            className="p-1.5 rounded-lg border border-cs-border hover:border-cs-accent/40 transition-colors text-cs-muted hover:text-cs-text"
          >
            <ChevronLeft size={14} />
          </button>
          <h3 className="text-sm font-semibold text-cs-text min-w-[140px] text-center">
            {monthLabel}
          </h3>
          <button
            onClick={nextMonth}
            className="p-1.5 rounded-lg border border-cs-border hover:border-cs-accent/40 transition-colors text-cs-muted hover:text-cs-text"
          >
            <ChevronRight size={14} />
          </button>
          <button
            onClick={goToday}
            className="ml-2 px-2.5 py-1 text-[11px] rounded-lg border border-cs-border text-cs-muted hover:text-cs-accent hover:border-cs-accent/40 transition-colors"
          >
            {t("cron.calendar.today")}
          </button>
        </div>

        {/* Legend */}
        <div className="flex items-center gap-3 text-[10px] text-cs-muted">
          <span className="flex items-center gap-1">
            <CheckCircle size={9} className="text-green-400" />
            {t("cron.calendar.success")}
          </span>
          <span className="flex items-center gap-1">
            <AlertTriangle size={9} className="text-red-400" />
            {t("cron.calendar.failed")}
          </span>
          <span className="flex items-center gap-1">
            <Clock size={9} className="text-cs-muted/60" />
            {t("cron.calendar.scheduled")}
          </span>
        </div>
      </div>

      {/* Weekday headers */}
      <div className="grid grid-cols-7 border-t border-l border-[#2a2a3a]">
        {WEEKDAY_LABELS.map((day) => (
          <div
            key={day}
            className="border-r border-b border-[#2a2a3a] px-2 py-1.5 text-center text-[10px] font-semibold text-cs-muted uppercase tracking-wider"
            style={{ background: "#0e0e16" }}
          >
            {day}
          </div>
        ))}
      </div>

      {/* Calendar grid */}
      <div className="grid grid-cols-7 border-t border-l border-[#2a2a3a]">
        {calendarDays.map((day) => (
          <DayCell
            key={day.dateStr}
            day={day}
            onSelectJob={selectJob}
            onSelectExecution={setSelectedExecution}
          />
        ))}
      </div>

      {/* Execution detail popover */}
      {selectedExecution && (
        <ExecutionPopover
          execution={selectedExecution}
          job={selectedExecJob}
          onClose={() => setSelectedExecution(null)}
        />
      )}
    </div>
  );
}
