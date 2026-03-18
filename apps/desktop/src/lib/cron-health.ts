// ---------------------------------------------------------------------------
// Smart failure detection for cron jobs
// ---------------------------------------------------------------------------

import type { CronJob, CronExecution, CronJobStatus, CronAlert } from "@/components/cron/types";

interface HealthResult {
  status: CronJobStatus;
  alerts: CronAlert[];
}

/**
 * Analyze cron job health based on execution history.
 *
 * Logic (from the OpenClaw tweet):
 * 1. If paused → "paused"
 * 2. If last execution failed → "failed"
 * 3. If nextRunAt passed but no execution logged → "silent-failure"
 * 4. NOT a failure: quiet-hours skip, retry backoff, one-shot auto-delete, intentionally paused
 * 5. If 3+ recent runs have issues → "warning" (chronic)
 * 6. Alert once per failure chain (dedup)
 * 7. Otherwise → "healthy"
 */
export function analyzeCronHealth(
  job: CronJob,
  executions: CronExecution[],
  existingAlerts: CronAlert[] = []
): HealthResult {
  const alerts: CronAlert[] = [];
  const now = new Date();

  // 1. If paused → "paused" (not a failure)
  if (!job.enabled) {
    return { status: "paused", alerts };
  }

  // Sort executions newest first
  const sorted = [...executions]
    .filter((e) => e.jobId === job.id)
    .sort((a, b) => new Date(b.startedAt).getTime() - new Date(a.startedAt).getTime());

  const lastExecution = sorted[0];

  // 2. If last execution failed → "failed"
  if (lastExecution?.status === "failed") {
    const alreadyAlerted = existingAlerts.some(
      (a) => a.jobId === job.id && a.type === "failed" && !a.acknowledged
    );
    if (!alreadyAlerted) {
      alerts.push(createAlert(job.id, "failed", `"${job.name}" — last execution failed${lastExecution.error ? `: ${lastExecution.error}` : ""}`));
    }
    return { status: "failed", alerts };
  }

  // 3. If nextRunAt passed but no execution logged → "silent-failure"
  if (job.nextRunAt) {
    const nextRun = new Date(job.nextRunAt);
    // Grace period: 5 minutes after expected run time
    const graceMs = 5 * 60 * 1000;
    if (now.getTime() > nextRun.getTime() + graceMs) {
      // Check if there's an execution that started around that time
      const hasMatchingExecution = sorted.some((e) => {
        const startTime = new Date(e.startedAt).getTime();
        return Math.abs(startTime - nextRun.getTime()) < graceMs * 2;
      });

      if (!hasMatchingExecution) {
        // Count consecutive missed runs
        const missedCount = countMissedRuns(job, sorted, now);
        const alreadyAlerted = existingAlerts.some(
          (a) => a.jobId === job.id && a.type === "silent-failure" && !a.acknowledged
        );
        if (!alreadyAlerted) {
          alerts.push(createAlert(
            job.id,
            "silent-failure",
            `"${job.name}" — silent failure (missed ${missedCount} run${missedCount > 1 ? "s" : ""})`
          ));
        }
        return { status: "silent-failure", alerts };
      }
    }
  }

  // 5. If 3+ recent runs have issues → "warning" (chronic)
  const recentRuns = sorted.slice(0, 10);
  const failedCount = recentRuns.filter(
    (e) => e.status === "failed" || e.status === "skipped"
  ).length;

  if (failedCount >= 3) {
    const alreadyAlerted = existingAlerts.some(
      (a) => a.jobId === job.id && a.type === "warning" && !a.acknowledged
    );
    if (!alreadyAlerted) {
      alerts.push(createAlert(
        job.id,
        "warning",
        `"${job.name}" — chronic warning (${failedCount} failures in recent runs)`
      ));
    }
    return { status: "warning", alerts };
  }

  // 7. Otherwise → "healthy"
  return { status: "healthy", alerts };
}

function countMissedRuns(
  job: CronJob,
  executions: CronExecution[],
  now: Date
): number {
  if (!job.lastRunAt) return 1;

  const lastRun = new Date(job.lastRunAt);
  const diffMs = now.getTime() - lastRun.getTime();

  // Estimate interval from schedule (rough: use gap between last two runs or fallback)
  if (executions.length >= 2) {
    const gap =
      new Date(executions[0].startedAt).getTime() -
      new Date(executions[1].startedAt).getTime();
    if (gap > 0) {
      return Math.max(1, Math.floor(diffMs / gap));
    }
  }

  return 1;
}

function createAlert(
  jobId: string,
  type: CronJobStatus,
  message: string
): CronAlert {
  return {
    id: `alert-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    jobId,
    type,
    message,
    createdAt: new Date().toISOString(),
    acknowledged: false,
  };
}

/**
 * Get all unacknowledged alerts across all jobs.
 */
export function getActiveAlerts(
  jobs: CronJob[],
  executions: CronExecution[],
  existingAlerts: CronAlert[] = []
): CronAlert[] {
  const allAlerts: CronAlert[] = [];
  for (const job of jobs) {
    const { alerts } = analyzeCronHealth(job, executions, existingAlerts);
    allAlerts.push(...alerts);
  }
  return allAlerts;
}
