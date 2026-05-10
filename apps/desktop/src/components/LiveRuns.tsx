import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Loader2,
  Activity,
  Square,
  AlertCircle,
  Folder,
  Cpu,
  Clock,
  Sparkles,
  StopCircle,
} from "lucide-react";
import { listActiveRuns, killActiveRun, type ActiveRun } from "@/lib/activeRuns";
import { cn } from "@/lib/utils";

// v2.1.0 Phase 4 — Live runs panel.
//
// The "missing ops layer" Timur Yessenov asked for on Twitter: which
// runtime is in which workspace, what's running right now, and a
// kill button per row so you don't have to read every terminal
// buffer to stop a stuck dispatch.
//
// Data lives in the Rust active_runs registry (process-wide
// Mutex<HashMap>). Polled every 2s — frequent enough for the panel
// to feel live, infrequent enough not to hammer the IPC bridge.

export default function LiveRuns() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [killing, setKilling] = useState<Set<string>>(new Set());

  const query = useQuery({
    queryKey: ["active-runs"],
    queryFn: listActiveRuns,
    refetchInterval: 2_000,
    staleTime: 0,
  });

  const onKill = async (runId: string) => {
    setKilling((prev) => new Set(prev).add(runId));
    try {
      await killActiveRun(runId);
      // Force refresh so the row updates to status='killing' or
      // disappears entirely.
      await queryClient.invalidateQueries({ queryKey: ["active-runs"] });
    } finally {
      setKilling((prev) => {
        const next = new Set(prev);
        next.delete(runId);
        return next;
      });
    }
  };

  // v2.1.8+ — bulk kill. Confirm before firing N kills since this is
  // destructive (each run gets SIGKILL'd, in-flight responses lost).
  // Fires sequentially with Promise.allSettled so a single hang
  // doesn't block the rest.
  const onKillAll = async () => {
    const runs = query.data ?? [];
    if (runs.length === 0) return;
    const confirmed = window.confirm(
      t(
        "insights.live.confirmKillAll",
        "Kill all {{n}} running dispatches? In-flight responses will be lost.",
        { n: runs.length },
      ),
    );
    if (!confirmed) return;
    setKilling(new Set(runs.map((r) => r.run_id)));
    try {
      await Promise.allSettled(runs.map((r) => killActiveRun(r.run_id)));
      await queryClient.invalidateQueries({ queryKey: ["active-runs"] });
    } finally {
      setKilling(new Set());
    }
  };

  if (query.isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-cs-muted">
        <Loader2 size={18} className="animate-spin mr-2" />
        {t("insights.live.loading", "Loading active runs…")}
      </div>
    );
  }

  if (query.isError) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <span>
          {t("insights.live.error", "Couldn't load active runs")}: {String(query.error)}
        </span>
      </div>
    );
  }

  const runs = query.data ?? [];

  return (
    <div className="space-y-3">
      <header className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
            <Activity size={14} className="text-cs-accent" />
            {t("insights.live.title", "Live runs")}
            {runs.length > 0 && (
              <span className="ml-1 inline-flex h-1.5 w-1.5 animate-pulse rounded-full bg-cs-accent" />
            )}
          </h3>
          <p className="mt-0.5 text-[11px] text-cs-muted">
            {t(
              "insights.live.subtitle",
              "Every dispatch in flight right now. Kill stuck runs without reading the terminal buffer.",
            )}
          </p>
        </div>
        {/* v2.1.8 — bulk kill. Only renders when ≥2 runs (single-row
            kill is one click already). Confirmation dialog avoids
            accidental nuke during demos. */}
        {runs.length >= 2 && (
          <button
            type="button"
            onClick={onKillAll}
            disabled={killing.size > 0}
            className="shrink-0 inline-flex items-center gap-1.5 rounded-md border border-cs-warn/40 bg-cs-warn/10 px-2 py-1 text-[11px] font-medium text-cs-warn hover:bg-cs-warn/20 disabled:opacity-60"
          >
            <StopCircle size={11} />
            {t("insights.live.killAll", "Kill all ({{n}})", { n: runs.length })}
          </button>
        )}
      </header>

      {runs.length === 0 ? (
        <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 text-center text-sm">
          <div className="mx-auto mb-2 flex h-10 w-10 items-center justify-center rounded-full bg-cs-bg-raised text-cs-muted">
            <Activity size={20} />
          </div>
          <p className="text-cs-text font-medium mb-1">
            {t("insights.live.idle", "No agents running")}
          </p>
          <p className="text-[12px] text-cs-muted">
            {t(
              "insights.live.idleBody",
              "When you dispatch an agent (Quick test, chat pane, MCP run_agent, scheduled cron), it'll appear here with a kill button.",
            )}
          </p>
        </div>
      ) : (
        <ul className="space-y-1.5">
          {runs.map((run) => (
            <RunRow
              key={run.run_id}
              run={run}
              killing={killing.has(run.run_id)}
              onKill={() => onKill(run.run_id)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function RunRow({
  run,
  killing,
  onKill,
}: {
  run: ActiveRun;
  killing: boolean;
  onKill: () => void;
}) {
  const { t } = useTranslation();
  const elapsed = Math.max(0, Math.floor(Date.now() / 1000) - run.started_at_unix);
  const elapsedLabel =
    elapsed < 60
      ? `${elapsed}s`
      : elapsed < 3600
        ? `${Math.floor(elapsed / 60)}m ${elapsed % 60}s`
        : `${Math.floor(elapsed / 3600)}h ${Math.floor((elapsed % 3600) / 60)}m`;
  const isKilling = run.status === "killing" || killing;
  const workspaceShort = run.workspace
    ? run.workspace.split("/").filter(Boolean).slice(-2).join("/")
    : null;

  return (
    <li
      className={cn(
        "flex items-center gap-3 rounded-lg border p-3 transition-colors",
        isKilling
          ? "border-cs-warn/40 bg-cs-warn/10"
          : "border-cs-border bg-cs-bg-raised/40 hover:border-cs-accent/40",
      )}
    >
      <span className="relative inline-flex shrink-0">
        <Sparkles size={14} className="text-cs-accent" />
        {!isKilling && (
          <span className="absolute -right-1 -top-1 inline-flex h-2 w-2 rounded-full bg-cs-accent">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-cs-accent opacity-60" />
          </span>
        )}
      </span>

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-xs">
          {run.agent_slug ? (
            <code className="font-mono text-cs-text font-medium">@{run.agent_slug}</code>
          ) : (
            <span className="text-cs-muted italic">{t("insights.live.adhoc", "ad-hoc")}</span>
          )}
          <span className="inline-flex items-center gap-1 text-[10px] uppercase tracking-wide text-cs-muted">
            <Cpu size={10} />
            {run.runtime}
          </span>
          <span className="inline-flex items-center gap-1 text-[10px] font-mono text-cs-muted">
            <Clock size={10} />
            {elapsedLabel}
          </span>
        </div>
        {workspaceShort && (
          <div className="mt-0.5 inline-flex items-center gap-1 text-[11px] text-cs-muted truncate">
            <Folder size={10} />
            <code className="font-mono truncate" title={run.workspace ?? undefined}>
              {workspaceShort}
            </code>
          </div>
        )}
        {run.source && (
          <div className="mt-0.5 text-[10px] text-cs-muted">{run.source}</div>
        )}
      </div>

      <button
        type="button"
        onClick={onKill}
        disabled={isKilling}
        className={cn(
          "inline-flex items-center gap-1 rounded-md border px-3 py-1.5 text-[11px] font-medium transition-colors shrink-0",
          isKilling
            ? "border-cs-warn/40 bg-cs-warn/10 text-cs-warn"
            : "border-cs-danger/40 bg-cs-danger/5 text-cs-danger hover:bg-cs-danger/15",
        )}
      >
        {isKilling ? (
          <>
            <Loader2 size={11} className="animate-spin" />
            {t("insights.live.killing", "Killing…")}
          </>
        ) : (
          <>
            <Square size={11} />
            {t("insights.live.kill", "Kill")}
          </>
        )}
      </button>
    </li>
  );
}
