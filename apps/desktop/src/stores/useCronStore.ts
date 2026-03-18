import { create } from "zustand";
import type {
  CronJob,
  CronExecution,
  CronAlert,
  CronJobStatus,
  AgentRuntime,
  RuntimeConfig,
} from "@/components/cron/types";
import { analyzeCronHealth, getActiveAlerts } from "@/lib/cron-health";
import { getNextRun } from "@/lib/cron-utils";

// ---------------------------------------------------------------------------
// Mock data for development
// ---------------------------------------------------------------------------

const now = new Date();
const hourAgo = new Date(now.getTime() - 3_600_000);
const twoHoursAgo = new Date(now.getTime() - 7_200_000);
const dayAgo = new Date(now.getTime() - 86_400_000);

const MOCK_JOBS: CronJob[] = [
  {
    id: "cron-daily-briefing",
    name: "Daily Briefing",
    description: "Summarize overnight PRs and Slack messages",
    schedule: "0 7 * * *",
    runtime: "openclaw",
    prompt: "Summarize all unread Slack messages and open PRs from overnight",
    enabled: true,
    status: "healthy",
    createdAt: "2026-01-15T10:00:00Z",
    updatedAt: "2026-03-10T08:00:00Z",
    lastRunAt: twoHoursAgo.toISOString(),
    nextRunAt: new Date(now.getTime() + 18 * 3_600_000).toISOString(),
  },
  {
    id: "cron-db-backup",
    name: "DB Backup Check",
    description: "Verify database backup completed successfully",
    schedule: "0 2 * * *",
    runtime: "claude",
    prompt: "Check that the nightly database backup completed. Verify backup size is within 10% of yesterday.",
    enabled: true,
    status: "warning",
    createdAt: "2026-02-01T10:00:00Z",
    updatedAt: "2026-03-15T02:00:00Z",
    lastRunAt: dayAgo.toISOString(),
    nextRunAt: new Date(now.getTime() + 8 * 3_600_000).toISOString(),
  },
  {
    id: "cron-pr-review",
    name: "PR Review Pipeline",
    description: "Auto-review open PRs every 4 hours",
    schedule: "0 */4 * * *",
    runtime: "claude",
    prompt: "Review all open PRs that haven't been reviewed in the last 4 hours",
    enabled: true,
    status: "healthy",
    linkedWorkflowId: "pr-review-notify",
    createdAt: "2026-01-20T10:00:00Z",
    updatedAt: "2026-03-18T12:00:00Z",
    lastRunAt: hourAgo.toISOString(),
    nextRunAt: new Date(now.getTime() + 3 * 3_600_000).toISOString(),
  },
  {
    id: "cron-dep-audit",
    name: "Dependency Audit",
    description: "Weekly security audit of npm dependencies",
    schedule: "0 9 * * 1",
    runtime: "codex",
    prompt: "Run npm audit and report any high/critical vulnerabilities",
    enabled: true,
    status: "healthy",
    createdAt: "2026-02-10T10:00:00Z",
    updatedAt: "2026-03-11T09:00:00Z",
    lastRunAt: new Date(now.getTime() - 3 * 86_400_000).toISOString(),
    nextRunAt: getNextRun("0 9 * * 1")?.toISOString(),
  },
  {
    id: "cron-standup-bot",
    name: "Standup Collector",
    description: "Collect and summarize daily standups from Slack",
    schedule: "30 9 * * 1-5",
    runtime: "hermes",
    prompt: "Collect standup updates from #standup channel, summarize, post to Notion",
    enabled: false,
    status: "paused",
    createdAt: "2026-03-01T10:00:00Z",
    updatedAt: "2026-03-14T09:30:00Z",
  },
];

function generateMockExecutions(): CronExecution[] {
  const executions: CronExecution[] = [];
  const jobs = MOCK_JOBS;

  for (const job of jobs) {
    if (!job.enabled) continue;
    // Generate 7 days of executions
    for (let day = 0; day < 7; day++) {
      const startedAt = new Date(now.getTime() - day * 86_400_000);
      startedAt.setHours(
        parseInt(job.schedule.split(" ")[1]) || 7,
        parseInt(job.schedule.split(" ")[0]) || 0,
      );

      const isFailed = job.id === "cron-db-backup" && day < 3;
      const durationMs = 1000 + Math.random() * 10000;

      const successOutputs: Record<string, string[]> = {
        "cron-daily-briefing": [
          "Summarized 12 PRs and 34 Slack messages. 3 items need attention: PR #421 (breaking API change), #security-alerts channel (2 new CVEs), standup thread (blocker from @alex).",
          "Quiet night. 4 PRs (all approved), 8 Slack messages (no action items). Team standup summary posted to #general.",
          "Summarized 7 PRs and 19 Slack messages. 1 action item: PR #398 needs review before EOD. Posted digest to #daily-brief.",
        ],
        "cron-db-backup": [
          "Backup completed. Size: 2.4GB (within 5% of yesterday's 2.3GB). Checksum verified. Uploaded to S3 bucket ato-backups/2026-03-18.",
          "Backup completed. Size: 2.5GB. All tables verified. Retention policy applied: deleted backups older than 30 days (removed 2 files).",
        ],
        "cron-pr-review": [
          "Reviewed 3 open PRs:\n- PR #445: 2 suggestions (naming, test coverage) → commented\n- PR #442: LGTM, approved\n- PR #440: 1 security concern (SQL injection risk in user input) → requested changes",
          "Reviewed 1 open PR:\n- PR #447: Clean refactor, no issues found → approved",
          "No open PRs requiring review. All caught up.",
        ],
        "cron-dep-audit": [
          "npm audit complete. 0 critical, 0 high, 2 moderate vulnerabilities found.\n- lodash@4.17.20: prototype pollution (moderate) → update available\n- axios@0.21.1: SSRF (moderate) → update available\nRecommendation: run `npm audit fix`",
        ],
      };

      const jobOutputs = successOutputs[job.id] || ["Completed successfully"];
      const output = jobOutputs[day % jobOutputs.length];

      executions.push({
        id: `exec-${job.id}-${day}`,
        jobId: job.id,
        startedAt: startedAt.toISOString(),
        finishedAt: new Date(startedAt.getTime() + durationMs).toISOString(),
        durationMs: Math.round(durationMs),
        status: isFailed ? "failed" : "success",
        output: isFailed ? undefined : output,
        error: isFailed ? "Backup verification failed: expected size ~2.4GB but got 1.1GB. Possible incomplete dump. Last successful backup: 3 days ago. Check pg_dump logs at /var/log/postgres/backup.log" : undefined,
      });
    }
  }

  return executions;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

interface CronStore {
  // Data
  jobs: CronJob[];
  executions: CronExecution[];
  alerts: CronAlert[];

  // Selection
  selectedJobId: string | null;
  selectJob: (id: string | null) => void;

  // Search
  searchQuery: string;
  setSearchQuery: (q: string) => void;

  // CRUD
  createJob: (job: Omit<CronJob, "id" | "createdAt" | "updatedAt" | "status">) => void;
  updateJob: (id: string, updates: Partial<CronJob>) => void;
  deleteJob: (id: string) => void;
  toggleJob: (id: string) => void;

  // Execution
  triggerJob: (id: string) => void;
  retryExecution: (executionId: string) => void;

  // Alerts
  acknowledgeAlert: (alertId: string) => void;
  refreshAlerts: () => void;

  // Computed
  getJobExecutions: (jobId: string) => CronExecution[];
  getActiveAlertCount: () => number;
  getFilteredJobs: () => CronJob[];
}

export const useCronStore = create<CronStore>((set, get) => {
  const mockExecutions = generateMockExecutions();
  const initialAlerts = getActiveAlerts(MOCK_JOBS, mockExecutions);

  return {
    jobs: MOCK_JOBS,
    executions: mockExecutions,
    alerts: initialAlerts,

    selectedJobId: null,
    selectJob: (id) => set({ selectedJobId: id }),

    searchQuery: "",
    setSearchQuery: (q) => set({ searchQuery: q }),

    createJob: (jobData) => {
      const id = `cron-${Date.now()}`;
      const now = new Date().toISOString();
      const nextRunAt = getNextRun(jobData.schedule)?.toISOString();
      const job: CronJob = {
        ...jobData,
        id,
        status: jobData.enabled ? "healthy" : "paused",
        createdAt: now,
        updatedAt: now,
        nextRunAt,
      };
      set((s) => ({ jobs: [...s.jobs, job] }));
    },

    updateJob: (id, updates) =>
      set((s) => ({
        jobs: s.jobs.map((j) =>
          j.id === id ? { ...j, ...updates, updatedAt: new Date().toISOString() } : j
        ),
      })),

    deleteJob: (id) =>
      set((s) => ({
        jobs: s.jobs.filter((j) => j.id !== id),
        executions: s.executions.filter((e) => e.jobId !== id),
        alerts: s.alerts.filter((a) => a.jobId !== id),
        selectedJobId: s.selectedJobId === id ? null : s.selectedJobId,
      })),

    toggleJob: (id) =>
      set((s) => ({
        jobs: s.jobs.map((j) =>
          j.id === id
            ? {
                ...j,
                enabled: !j.enabled,
                status: !j.enabled ? "healthy" : "paused",
                updatedAt: new Date().toISOString(),
              }
            : j
        ),
      })),

    triggerJob: (id) => {
      const job = get().jobs.find((j) => j.id === id);
      if (!job) return;

      const executionId = `exec-${id}-manual-${Date.now()}`;
      const startedAt = new Date().toISOString();

      // Add a "running" execution
      set((s) => ({
        executions: [
          {
            id: executionId,
            jobId: id,
            startedAt,
            status: "running" as const,
          },
          ...s.executions,
        ],
      }));

      // Simulate completion after 2-5 seconds
      setTimeout(() => {
        const durationMs = 2000 + Math.random() * 3000;
        set((s) => ({
          executions: s.executions.map((e) =>
            e.id === executionId
              ? {
                  ...e,
                  status: "success" as const,
                  finishedAt: new Date().toISOString(),
                  durationMs: Math.round(durationMs),
                  output: "Manual trigger completed successfully",
                }
              : e
          ),
          jobs: s.jobs.map((j) =>
            j.id === id
              ? { ...j, lastRunAt: new Date().toISOString(), status: "healthy" as CronJobStatus }
              : j
          ),
        }));
      }, 2000 + Math.random() * 3000);
    },

    retryExecution: (executionId) => {
      const execution = get().executions.find((e) => e.id === executionId);
      if (!execution) return;

      const retryId = `exec-${execution.jobId}-retry-${Date.now()}`;
      const startedAt = new Date().toISOString();

      set((s) => ({
        executions: [
          {
            id: retryId,
            jobId: execution.jobId,
            startedAt,
            status: "running" as const,
            retryOf: executionId,
          },
          ...s.executions,
        ],
      }));

      setTimeout(() => {
        set((s) => ({
          executions: s.executions.map((e) =>
            e.id === retryId
              ? {
                  ...e,
                  status: "success" as const,
                  finishedAt: new Date().toISOString(),
                  durationMs: Math.round(1500 + Math.random() * 3000),
                  output: "Retry completed successfully",
                }
              : e
          ),
        }));
      }, 1500 + Math.random() * 3000);
    },

    acknowledgeAlert: (alertId) =>
      set((s) => ({
        alerts: s.alerts.map((a) =>
          a.id === alertId ? { ...a, acknowledged: true } : a
        ),
      })),

    refreshAlerts: () => {
      const { jobs, executions, alerts } = get();
      const newAlerts = getActiveAlerts(jobs, executions, alerts);
      if (newAlerts.length > 0) {
        set((s) => ({ alerts: [...s.alerts, ...newAlerts] }));
      }
    },

    getJobExecutions: (jobId) =>
      get()
        .executions.filter((e) => e.jobId === jobId)
        .sort((a, b) => new Date(b.startedAt).getTime() - new Date(a.startedAt).getTime()),

    getActiveAlertCount: () =>
      get().alerts.filter((a) => !a.acknowledged).length,

    getFilteredJobs: () => {
      const { jobs, searchQuery } = get();
      if (!searchQuery.trim()) return jobs;
      const q = searchQuery.toLowerCase();
      return jobs.filter(
        (j) =>
          j.name.toLowerCase().includes(q) ||
          j.description.toLowerCase().includes(q) ||
          j.runtime.includes(q)
      );
    },
  };
});
