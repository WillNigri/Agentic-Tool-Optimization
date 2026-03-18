import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Search, AlertTriangle, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { useCronStore } from "@/stores/useCronStore";
import CronJobCard from "./CronJobCard";
import CronJobDetail from "./CronJobDetail";
import CreateCronJobModal from "./CreateCronJobModal";

export default function CronDashboard() {
  const { t } = useTranslation();
  const [showCreateModal, setShowCreateModal] = useState(false);

  const {
    alerts,
    selectedJobId,
    selectJob,
    searchQuery,
    setSearchQuery,
    triggerJob,
    acknowledgeAlert,
    getFilteredJobs,
    getJobExecutions,
    executions,
  } = useCronStore();

  const filteredJobs = getFilteredJobs();
  const activeAlerts = alerts.filter((a) => !a.acknowledged);
  const selectedJob = useCronStore((s) => s.jobs.find((j) => j.id === s.selectedJobId));

  // Check if a job has a running execution
  function isJobRunning(jobId: string): boolean {
    return executions.some((e) => e.jobId === jobId && e.status === "running");
  }

  return (
    <>
      <div className="space-y-6">
        {/* Header */}
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-xl font-semibold mb-1">{t("cron.title")}</h2>
            <p className="text-cs-muted text-sm">{t("cron.subtitle")}</p>
          </div>
          <button
            onClick={() => setShowCreateModal(true)}
            className="inline-flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors"
          >
            <Plus size={16} />
            {t("cron.newJob")}
          </button>
        </div>

        {/* Alert banner */}
        {activeAlerts.length > 0 && (
          <div className="rounded-lg border border-red-500/30 bg-red-500/5 p-4">
            <div className="flex items-center gap-2 mb-2">
              <AlertTriangle size={16} className="text-red-400" />
              <h3 className="text-sm font-semibold text-red-400">
                {t("cron.alert.title", { count: activeAlerts.length })}
              </h3>
            </div>
            <div className="space-y-1.5">
              {activeAlerts.map((alert) => (
                <div
                  key={alert.id}
                  className="flex items-center justify-between gap-3 px-3 py-2 rounded-lg border border-red-500/20 bg-red-500/5"
                >
                  <p className="text-xs text-red-300 flex-1">{alert.message}</p>
                  <button
                    onClick={() => acknowledgeAlert(alert.id)}
                    className="flex items-center gap-1 px-2 py-1 text-[10px] rounded border border-red-500/30 text-red-400 hover:bg-red-500/10 transition-colors shrink-0"
                  >
                    <X size={10} />
                    {t("cron.alert.acknowledge")}
                  </button>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Search */}
        <div className="relative">
          <Search
            size={16}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted"
          />
          <input
            type="text"
            className="input pl-9"
            placeholder={t("cron.search")}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>

        {/* Job list */}
        <div className="space-y-2">
          {filteredJobs.map((job) => (
            <CronJobCard
              key={job.id}
              job={job}
              executions={getJobExecutions(job.id)}
              isSelected={selectedJobId === job.id}
              isRunning={isJobRunning(job.id)}
              onClick={() => selectJob(job.id)}
              onTrigger={() => triggerJob(job.id)}
            />
          ))}
        </div>

        {filteredJobs.length === 0 && (
          <p className="text-cs-muted text-sm text-center py-8">
            {searchQuery ? t("common.noResults") : t("cron.noJobs")}
          </p>
        )}
      </div>

      {/* Detail panel */}
      {selectedJob && (
        <CronJobDetail
          job={selectedJob}
          onClose={() => selectJob(null)}
        />
      )}

      {/* Create modal */}
      {showCreateModal && (
        <CreateCronJobModal onClose={() => setShowCreateModal(false)} />
      )}
    </>
  );
}
