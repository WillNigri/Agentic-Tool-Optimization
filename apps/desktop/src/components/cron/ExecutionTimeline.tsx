import { cn } from "@/lib/utils";
import type { CronExecution } from "./types";

interface ExecutionTimelineProps {
  executions: CronExecution[];
  days?: number;
}

/**
 * 7-day colored grid (GitHub contribution graph style).
 * Green = success, Red = failed, Yellow = running, Gray = skipped, Empty = no execution.
 */
export default function ExecutionTimeline({ executions, days = 7 }: ExecutionTimelineProps) {
  const now = new Date();
  const cells: { date: string; status: CronExecution["status"] | "none" }[] = [];

  for (let i = days - 1; i >= 0; i--) {
    const date = new Date(now.getTime() - i * 86_400_000);
    const dateStr = date.toISOString().slice(0, 10);

    // Find the most relevant execution for this day
    const dayExecs = executions.filter(
      (e) => e.startedAt.slice(0, 10) === dateStr
    );

    if (dayExecs.length === 0) {
      cells.push({ date: dateStr, status: "none" });
    } else {
      // Show worst status for the day
      const hasFailed = dayExecs.some((e) => e.status === "failed");
      const hasRunning = dayExecs.some((e) => e.status === "running");
      const hasSkipped = dayExecs.some((e) => e.status === "skipped");
      const status = hasFailed
        ? "failed"
        : hasRunning
          ? "running"
          : hasSkipped
            ? "skipped"
            : "success";
      cells.push({ date: dateStr, status });
    }
  }

  const STATUS_BG: Record<string, string> = {
    success: "bg-green-500",
    failed: "bg-red-500",
    running: "bg-yellow-500 animate-pulse",
    skipped: "bg-gray-500",
    none: "bg-[#2a2a3a]",
  };

  return (
    <div className="flex items-center gap-1">
      {cells.map((cell) => (
        <div
          key={cell.date}
          className={cn("w-3.5 h-3.5 rounded-sm", STATUS_BG[cell.status])}
          title={`${cell.date}: ${cell.status}`}
        />
      ))}
    </div>
  );
}
