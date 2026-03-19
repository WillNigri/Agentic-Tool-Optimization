import { create } from "zustand";
import type {
  CronJob,
  CronExecution,
  CronAlert,
  CronJobStatus,
} from "@/components/cron/types";
import { getActiveAlerts } from "@/lib/cron-health";
import { getNextRun } from "@/lib/cron-utils";

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

  // External data loading
  loadJobs: (jobs: CronJob[]) => void;

  // Computed
  getJobExecutions: (jobId: string) => CronExecution[];
  getActiveAlertCount: () => number;
  getFilteredJobs: () => CronJob[];
}

export const useCronStore = create<CronStore>((set, get) => {
  return {
    jobs: [],
    executions: [],
    alerts: [],

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

    loadJobs: (jobs) => {
      set((s) => {
        // Merge: keep ATO-local jobs, replace external ones
        const localJobs = s.jobs.filter((j) => !j.source || j.source === "ato");
        const externalJobs = jobs.filter((j) => j.source && j.source !== "ato");
        return { jobs: [...localJobs, ...externalJobs] };
      });
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
