// v2.16 PR-7 — Mission-control board.
//
// Board view: four columns (open / in_progress / blocked / complete).
// Click a card → detail drawer.
// Mutations: set-category + set-state (confirm dialog on complete).
// Read-only everything else; dispatch/tick/merge stay CLI-side this PR.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ChevronDown,
  ChevronUp,
  RefreshCw,
  X,
  Loader2,
  Flag,
  Coins,
  Zap,
  CheckCircle2,
  Clock,
  Ban,
  HelpCircle,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  missionsList,
  missionDetail,
  missionSetCategory,
  missionSetState,
  VALID_CATEGORIES,
  VALID_STATES,
  type MissionSummary,
  type MissionDetail,
  type MissionState,
  type MissionCategory,
  type MissionEvent,
} from "@/lib/missions";
import { formatRelativeTime } from "@/lib/cron-utils";
import MarkdownContent from "@/components/MarkdownContent";
import LiveCursors from "@/components/livePresence/LiveCursors";
import InitiatorBadge from "@/components/InitiatorBadge";
import ExecutionLogReceipt from "@/components/receipts/ExecutionLogReceipt";
import PresencePills from "@/components/livePresence/PresencePills";

// ── Constants ─────────────────────────────────────────────────────────

const STATE_ORDER: MissionState[] = ["open", "in_progress", "blocked", "complete"];

// Translation keys for state/category labels — resolved via t() at render
// time so the labels follow the active locale. Fallbacks are the canonical
// English labels (used when i18n isn't initialized, e.g. tests).
const STATE_KEY: Record<MissionState, string> = {
  open: "missions.state_open",
  in_progress: "missions.state_in_progress",
  blocked: "missions.state_blocked",
  complete: "missions.state_complete",
};

const STATE_FALLBACK: Record<MissionState, string> = {
  open: "Open",
  in_progress: "In Progress",
  blocked: "Blocked",
  complete: "Complete",
};

const CATEGORY_KEY: Record<MissionCategory, string> = {
  autonomous: "missions.category_autonomous",
  needs_owner: "missions.category_needs_owner",
  ignored: "missions.category_ignored",
  done: "missions.category_done",
};

const CATEGORY_FALLBACK: Record<MissionCategory, string> = {
  autonomous: "autonomous",
  needs_owner: "needs owner",
  ignored: "ignored",
  done: "done",
};

const STATE_ICON: Record<MissionState, React.ElementType> = {
  open: Clock,
  in_progress: Zap,
  blocked: Ban,
  complete: CheckCircle2,
};

const STATE_COLOR: Record<MissionState, string> = {
  open: "text-cs-muted border-cs-border/60",
  in_progress: "text-cs-accent border-cs-accent/30",
  blocked: "text-amber-400 border-amber-400/30",
  complete: "text-emerald-400 border-emerald-400/30",
};

// autonomous=accent, needs_owner=amber, ignored=muted, done=neutral
const CATEGORY_COLOR: Record<MissionCategory, string> = {
  autonomous: "bg-cs-accent/15 text-cs-accent",
  needs_owner: "bg-amber-400/15 text-amber-400",
  ignored: "bg-cs-border/30 text-cs-muted",
  done: "bg-cs-bg-raised text-cs-muted border border-cs-border",
};

// ── Event kind icons ──────────────────────────────────────────────────

function eventIcon(kind: string): React.ElementType {
  switch (kind) {
    case "state_changed": return Flag;
    case "category_changed": return HelpCircle;
    case "dispatched":
    case "dispatch_started": return Zap;
    case "escalated": return AlertTriangle;
    case "loop_run_completed":
    case "loop_run_started": return RefreshCw;
    case "worktree_created":
    case "worktree_cleaned": return CheckCircle2;
    default: return Clock;
  }
}

// ── Budget bar ────────────────────────────────────────────────────────

function BudgetBar({ spent, budget }: { spent: number; budget: number }) {
  const { t } = useTranslation();
  const pct = Math.min((spent / budget) * 100, 100);
  const over = spent > budget;
  return (
    <div className="space-y-0.5">
      <div className="flex items-center justify-between text-[10px] text-cs-muted">
        <span className="flex items-center gap-1">
          <Coins size={9} />
          ${spent.toFixed(3)} / ${budget.toFixed(2)}
        </span>
        {over && <span className="text-red-400">{t("missions.budgetOver", "over")}</span>}
      </div>
      <div className="h-1 w-full rounded-full bg-cs-border/40 overflow-hidden">
        <div
          className={cn(
            "h-full rounded-full transition-all",
            over ? "bg-red-400" : pct > 80 ? "bg-amber-400" : "bg-cs-accent"
          )}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

// ── Mission card ──────────────────────────────────────────────────────

function MissionCard({
  mission,
  onClick,
}: {
  mission: MissionSummary;
  onClick: () => void;
}) {
  const { t } = useTranslation();
  const hasPendingEscalation = false; // summary doesn't carry pending_escalations; detail does

  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "w-full text-left p-3 rounded-lg border bg-cs-card transition-colors hover:border-cs-accent/40 hover:bg-cs-card/80",
        "border-cs-border/50"
      )}
    >
      {/* Name + alert */}
      <div className="flex items-start justify-between gap-2 mb-1.5">
        <p className="text-xs font-medium text-cs-text leading-tight">{mission.name}</p>
        {hasPendingEscalation && (
          <AlertTriangle size={12} className="text-amber-400 shrink-0 mt-0.5" />
        )}
      </div>

      {/* Slug */}
      <p className="text-[10px] text-cs-muted font-mono mb-2 truncate">{mission.slug}</p>

      {/* Category chip */}
      <div className="flex flex-wrap items-center gap-1.5 mb-2">
        <span
          className={cn(
            "inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-medium",
            CATEGORY_COLOR[mission.category]
          )}
        >
          {t(CATEGORY_KEY[mission.category], CATEGORY_FALLBACK[mission.category])}
        </span>
        {mission.dispatchCount > 0 && (
          <span className="text-[10px] text-cs-muted">
            {/* i18next plural keys: missions.dispatchesShort_one / _other. */}
            {t(
              mission.dispatchCount === 1
                ? "missions.dispatchesShort_one"
                : "missions.dispatchesShort_other",
              `${mission.dispatchCount} dispatch${mission.dispatchCount === 1 ? "" : "es"}`,
              { count: mission.dispatchCount }
            )}
          </span>
        )}
      </div>

      {/* Budget bar when budget is set */}
      {mission.tokenBudgetUsd != null && (
        <div className="mb-2">
          <BudgetBar spent={mission.spentUsd} budget={mission.tokenBudgetUsd} />
        </div>
      )}

      {/* Initiator + updated-at */}
      <div className="flex items-center justify-between gap-2 flex-wrap">
        {(mission.initiatorKind || mission.clientSurface) && (
          <InitiatorBadge
            initiatorKind={mission.initiatorKind}
            clientSurface={mission.clientSurface}
            initiatorId={mission.initiatorId}
          />
        )}
        <p className="text-[10px] text-cs-muted/70 ml-auto">
          {formatRelativeTime(mission.updatedAt)}
        </p>
      </div>
    </button>
  );
}

// ── Board column ──────────────────────────────────────────────────────

function BoardColumn({
  state,
  missions,
  onCardClick,
}: {
  state: MissionState;
  missions: MissionSummary[];
  onCardClick: (m: MissionSummary) => void;
}) {
  const { t } = useTranslation();
  const Icon = STATE_ICON[state];
  return (
    <div className="flex flex-col gap-2 min-w-[220px] flex-1">
      {/* Column header */}
      <div
        className={cn(
          "flex items-center gap-1.5 px-2 py-1.5 rounded-md border text-xs font-medium",
          STATE_COLOR[state]
        )}
      >
        <Icon size={12} />
        {t(STATE_KEY[state], STATE_FALLBACK[state])}
        <span className="ml-auto tabular-nums text-[10px] opacity-70">
          {missions.length}
        </span>
      </div>

      {/* Cards */}
      <div className="space-y-2">
        {missions.map((m) => (
          <MissionCard key={m.id} mission={m} onClick={() => onCardClick(m)} />
        ))}
        {missions.length === 0 && (
          <p className="text-[11px] text-cs-muted/50 text-center py-4">—</p>
        )}
      </div>
    </div>
  );
}

// ── Expandable payload JSON ───────────────────────────────────────────

function EventPayload({ payload }: { payload: Record<string, unknown> | null }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  if (!payload) return null;

  // FOLLOWUPS #1 — pull the execution_log_id out of the payload (if present)
  // and surface it as a clickable receipt above the JSON dump. Missions emit
  // `dispatched` events whose payload carries `execution_log_id`; that's the
  // "where" reference Will called out.
  const executionLogId =
    typeof payload.execution_log_id === "string"
      ? payload.execution_log_id
      : null;

  return (
    <div className="mt-1">
      <div className="flex items-center gap-2 flex-wrap">
        <button
          type="button"
          onClick={() => setOpen((p) => !p)}
          className="flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-text transition-colors"
        >
          {open ? <ChevronUp size={10} /> : <ChevronDown size={10} />}
          {open
            ? t("missions.hidePayload", "hide payload")
            : t("missions.showPayload", "show payload")}
        </button>
        {executionLogId && (
          <span className="text-[10px] text-cs-muted">
            run: <ExecutionLogReceipt logId={executionLogId} />
          </span>
        )}
      </div>
      {open && (
        <pre className="mt-1 rounded border border-cs-border bg-cs-bg p-2 text-[10px] font-mono text-cs-muted overflow-x-auto max-h-40">
          {JSON.stringify(payload, null, 2)}
        </pre>
      )}
    </div>
  );
}

// ── Events timeline ───────────────────────────────────────────────────

function EventsTimeline({ events }: { events: MissionEvent[] }) {
  const { t } = useTranslation();
  if (events.length === 0) {
    return (
      <p className="text-xs text-cs-muted py-4 text-center">
        {t("missions.noEvents", "No events yet.")}
      </p>
    );
  }
  return (
    <div className="space-y-2">
      {events.map((ev) => {
        const Icon = eventIcon(ev.kind);
        return (
          <div key={ev.id} className="flex gap-2.5">
            <div className="flex flex-col items-center">
              <span className="w-5 h-5 rounded-full border border-cs-border bg-cs-bg-raised flex items-center justify-center shrink-0">
                <Icon size={10} className="text-cs-muted" />
              </span>
              <div className="flex-1 w-px bg-cs-border/30 mt-1" />
            </div>
            <div className="pb-3 flex-1 min-w-0">
              <div className="flex items-baseline gap-1.5">
                <span className="text-[11px] font-medium text-cs-text font-mono">
                  {ev.kind}
                </span>
                <span className="text-[10px] text-cs-muted shrink-0">
                  {formatRelativeTime(ev.occurredAt)}
                </span>
              </div>
              <EventPayload payload={ev.payload} />
            </div>
          </div>
        );
      })}
    </div>
  );
}

// ── Confirm dialog ────────────────────────────────────────────────────

function ConfirmDialog({
  message,
  onConfirm,
  onCancel,
}: {
  message: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="w-80 rounded-xl border border-cs-border bg-cs-bg p-5 shadow-xl space-y-4">
        <p className="text-sm text-cs-text">{message}</p>
        <div className="flex gap-2 justify-end">
          <button
            type="button"
            onClick={onCancel}
            className="px-3 py-1.5 text-xs rounded-md border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
          >
            {t("missions.confirmDialog.cancel", "Cancel")}
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="px-3 py-1.5 text-xs rounded-md bg-emerald-600 text-white hover:bg-emerald-500 transition-colors"
          >
            {t("missions.confirmDialog.confirm", "Confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Detail drawer ─────────────────────────────────────────────────────

function MissionDetailPanel({
  slugOrId,
  onClose,
}: {
  slugOrId: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [confirmComplete, setConfirmComplete] = useState(false);

  const {
    data: detail,
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: ["mission_detail", slugOrId],
    queryFn: () => missionDetail(slugOrId),
    staleTime: 15_000,
  });

  // Classify "not found" vs. transient errors so we can show the right
  // message. Anchor on the Rust backend's exact prefix to avoid false
  // positives like "command mission_detail not found" (R2 [MED] fix).
  const notFound =
    isError && /^mission not found:/i.test(String((error as Error)?.message ?? ""));

  const setCategory = useMutation({
    mutationFn: ({ category }: { category: MissionCategory }) =>
      missionSetCategory(slugOrId, category),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["missions_list"] });
      void qc.invalidateQueries({ queryKey: ["mission_detail", slugOrId] });
    },
  });

  const setState = useMutation({
    mutationFn: ({ state }: { state: MissionState }) =>
      missionSetState(slugOrId, state),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["missions_list"] });
      void qc.invalidateQueries({ queryKey: ["mission_detail", slugOrId] });
    },
  });

  const handleStateChange = (newState: MissionState) => {
    if (newState === "complete") {
      setConfirmComplete(true);
    } else {
      setState.mutate({ state: newState });
    }
  };

  return (
    <>
      <div className="fixed inset-y-0 right-0 z-40 w-[480px] max-w-full flex flex-col border-l border-cs-border bg-cs-bg shadow-2xl relative">
        <LiveCursors resourceKind="mission" resourceId={slugOrId} />
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border shrink-0">
          <h2 className="text-sm font-semibold text-cs-text truncate">
            {detail?.name ?? slugOrId}
          </h2>
          <div className="flex items-center gap-2">
            <PresencePills resourceKind="mission" resourceId={slugOrId} />
            <button
              type="button"
              onClick={() => void refetch()}
              className="p-1.5 rounded text-cs-muted hover:text-cs-accent transition-colors"
              title={t("common.refresh", "Refresh")}
            >
              <RefreshCw size={13} />
            </button>
            <button
              type="button"
              onClick={onClose}
              className="p-1.5 rounded text-cs-muted hover:text-cs-text transition-colors"
            >
              <X size={14} />
            </button>
          </div>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto px-4 py-4 space-y-5">
          {isLoading && (
            <div className="flex items-center justify-center py-12">
              <Loader2 size={20} className="animate-spin text-cs-muted" />
            </div>
          )}
          {isError && (
            <div className="rounded-lg border border-red-500/30 bg-red-500/5 p-3 text-xs text-red-400">
              {notFound
                ? t("missions.detailNotFound", "Mission not found.")
                : t("missions.detailError", "Failed to load mission detail.")}
            </div>
          )}

          {detail && (
            <>
              {/* Slug */}
              <p className="text-[10px] text-cs-muted font-mono">{detail.slug}</p>

              {/* Pending escalations */}
              {detail.pendingEscalations.length > 0 && (
                <div className="rounded-lg border border-amber-400/30 bg-amber-400/5 p-3 space-y-2">
                  <div className="flex items-center gap-1.5 text-xs font-semibold text-amber-400">
                    <AlertTriangle size={13} />
                    {t("missions.pendingEscalations", "Pending Escalations")} ({detail.pendingEscalations.length})
                  </div>
                  {detail.pendingEscalations.map((esc) => (
                    <div key={esc.id} className="rounded border border-amber-400/20 bg-amber-400/5 p-2">
                      <p className="text-[11px] text-amber-300">
                        {String(esc.payload?.reason ?? t("missions.escalationLabel", "escalation"))}
                      </p>
                      {Array.isArray(esc.payload?.options) && (
                        <ul className="mt-1 space-y-0.5 text-[10px] text-cs-muted list-disc pl-4">
                          {(esc.payload!.options as string[]).map((opt, i) => (
                            <li key={i}>{opt}</li>
                          ))}
                        </ul>
                      )}
                    </div>
                  ))}
                </div>
              )}

              {/* Goal */}
              <section>
                <h3 className="text-[10px] text-cs-muted uppercase tracking-wide mb-1">
                  {t("missions.goal", "Goal")}
                </h3>
                <p className="text-xs text-cs-text leading-relaxed">{detail.goal}</p>
              </section>

              {/* Strategy fields */}
              <section className="grid grid-cols-2 gap-2 text-[11px]">
                <div>
                  <p className="text-cs-muted mb-0.5">{t("missions.workspace", "Workspace")}</p>
                  <p className="text-cs-text font-mono">{detail.workspaceStrategy}</p>
                </div>
                <div>
                  <p className="text-cs-muted mb-0.5">{t("missions.merge", "Merge")}</p>
                  <p className="text-cs-text font-mono">{detail.mergeStrategy}</p>
                </div>
                <div>
                  <p className="text-cs-muted mb-0.5">{t("missions.cleanup", "Cleanup")}</p>
                  <p className="text-cs-text font-mono">{detail.cleanupPolicy}</p>
                </div>
                {detail.repoRoot && (
                  <div className="col-span-2">
                    <p className="text-cs-muted mb-0.5">{t("missions.repoRoot", "Repo")}</p>
                    <p className="text-cs-text font-mono text-[10px] truncate">{detail.repoRoot}</p>
                  </div>
                )}
              </section>

              {/* Worker config */}
              {detail.workerConfig && (
                <section>
                  <h3 className="text-[10px] text-cs-muted uppercase tracking-wide mb-1">
                    {t("missions.workerConfig", "Worker Config")}
                  </h3>
                  <pre className="rounded border border-cs-border bg-cs-bg-raised p-2 text-[10px] font-mono text-cs-muted overflow-x-auto">
                    {JSON.stringify(detail.workerConfig, null, 2)}
                  </pre>
                </section>
              )}

              {/* Budgets */}
              <section>
                <h3 className="text-[10px] text-cs-muted uppercase tracking-wide mb-1">
                  {t("missions.budgets", "Budgets")}
                </h3>
                <div className="space-y-1 text-[11px]">
                  <div className="flex justify-between">
                    <span className="text-cs-muted">{t("missions.maxLoops", "Max loops")}</span>
                    <span className="text-cs-text">{detail.maxLoops ?? "∞"}</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-cs-muted">{t("missions.dispatches", "Dispatches")}</span>
                    <span className="text-cs-text">{detail.dispatchCount}</span>
                  </div>
                  {detail.tokenBudgetUsd != null ? (
                    <BudgetBar spent={detail.spentUsd} budget={detail.tokenBudgetUsd} />
                  ) : (
                    <div className="flex justify-between">
                      <span className="text-cs-muted">{t("missions.spent", "Spent")}</span>
                      <span className="text-cs-text">${detail.spentUsd.toFixed(4)}</span>
                    </div>
                  )}
                </div>
              </section>

              {/* Actions */}
              <section className="space-y-2">
                <h3 className="text-[10px] text-cs-muted uppercase tracking-wide">
                  {t("missions.actions", "Actions")}
                </h3>
                {/* Category select */}
                <div>
                  <label className="text-[10px] text-cs-muted block mb-1">
                    {t("missions.category", "Category")}
                  </label>
                  <select
                    className="w-full input text-xs"
                    value={detail.category}
                    onChange={(e) =>
                      setCategory.mutate({ category: e.target.value as MissionCategory })
                    }
                    disabled={setCategory.isPending}
                  >
                    {VALID_CATEGORIES.map((c) => (
                      <option key={c} value={c}>
                        {t(CATEGORY_KEY[c], CATEGORY_FALLBACK[c])}
                      </option>
                    ))}
                  </select>
                </div>
                {/* State select */}
                <div>
                  <label className="text-[10px] text-cs-muted block mb-1">
                    {t("missions.state", "State")}
                  </label>
                  <select
                    className="w-full input text-xs"
                    value={detail.state}
                    onChange={(e) => handleStateChange(e.target.value as MissionState)}
                    disabled={setState.isPending}
                  >
                    {VALID_STATES.map((s) => (
                      <option key={s} value={s}>
                        {t(STATE_KEY[s], STATE_FALLBACK[s])}
                      </option>
                    ))}
                  </select>
                </div>
                {(setCategory.isError || setState.isError) && (
                  <p className="text-[10px] text-red-400">
                    {String(
                      (setCategory.error as Error)?.message ??
                        (setState.error as Error)?.message ??
                        t("missions.errorGeneric", "Error")
                    )}
                  </p>
                )}
              </section>

              {/* Narrative */}
              {detail.narrativeBody && (
                <section>
                  <h3 className="text-[10px] text-cs-muted uppercase tracking-wide mb-2">
                    {t("missions.narrative", "Narrative")}
                  </h3>
                  <div className="rounded border border-cs-border bg-cs-bg-raised p-3 max-h-64 overflow-y-auto">
                    <MarkdownContent content={detail.narrativeBody} />
                  </div>
                </section>
              )}

              {/* Events timeline */}
              <section>
                <h3 className="text-[10px] text-cs-muted uppercase tracking-wide mb-2">
                  {t("missions.events", "Events")} ({detail.events.length})
                </h3>
                <EventsTimeline events={detail.events} />
              </section>
            </>
          )}
        </div>
      </div>

      {/* Confirm complete */}
      {confirmComplete && (
        <ConfirmDialog
          message={t(
            "missions.confirmComplete",
            "Mark this mission as complete? Worktree cleanup runs CLI-side via `ato missions cleanup`."
          )}
          onConfirm={() => {
            setConfirmComplete(false);
            setState.mutate({ state: "complete" });
          }}
          onCancel={() => setConfirmComplete(false)}
        />
      )}
    </>
  );
}

// ── Main board ────────────────────────────────────────────────────────

export default function MissionBoard() {
  const { t } = useTranslation();
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const {
    data: missions = [],
    isLoading,
    isError,
    refetch,
  } = useQuery({
    queryKey: ["missions_list"],
    queryFn: () => missionsList(),
    staleTime: 30_000,
  });

  const byState: Record<MissionState, MissionSummary[]> = {
    open: [],
    in_progress: [],
    blocked: [],
    complete: [],
  };
  for (const m of missions) {
    const s = m.state as MissionState;
    if (byState[s]) byState[s].push(m);
  }

  return (
    <div className="space-y-4 h-full flex flex-col">
      {/* Header */}
      <div className="flex items-start justify-between shrink-0">
        <div>
          <h2 className="text-xl font-semibold mb-1">
            {t("missions.title", "Missions")}
          </h2>
          <p className="text-cs-muted text-sm">
            {t(
              "missions.subtitle",
              "Local mission-control board. Dispatch and tick run CLI-side."
            )}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void refetch()}
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg border border-cs-border text-cs-muted hover:text-cs-accent transition-colors"
        >
          <RefreshCw size={12} />
          {t("common.refresh", "Refresh")}
        </button>
      </div>

      {/* States */}
      {isLoading && (
        <div className="flex items-center justify-center py-16">
          <Loader2 size={22} className="animate-spin text-cs-muted" />
        </div>
      )}
      {isError && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/5 p-4 text-sm text-red-400">
          {t("missions.loadError", "Failed to load missions — is the ATO DB available?")}
        </div>
      )}

      {!isLoading && !isError && missions.length === 0 && (
        <div className="text-center py-16">
          <p className="text-cs-muted text-sm">
            {t("missions.empty", "No missions yet.")}
          </p>
          <p className="text-cs-muted/60 text-xs mt-1">
            {t(
              "missions.emptyHint",
              "Create one with `ato missions create --name ... --goal ...`"
            )}
          </p>
        </div>
      )}

      {!isLoading && !isError && missions.length > 0 && (
        <div className="flex gap-3 overflow-x-auto pb-2 flex-1 min-h-0">
          {STATE_ORDER.map((state) => (
            <BoardColumn
              key={state}
              state={state}
              missions={byState[state]}
              onCardClick={(m) => setSelectedId(m.id)}
            />
          ))}
        </div>
      )}

      {/* Detail drawer */}
      {selectedId && (
        <>
          {/* Overlay */}
          <div
            className="fixed inset-0 z-30 bg-black/40"
            onClick={() => setSelectedId(null)}
          />
          <MissionDetailPanel
            slugOrId={selectedId}
            onClose={() => setSelectedId(null)}
          />
        </>
      )}
    </div>
  );
}
