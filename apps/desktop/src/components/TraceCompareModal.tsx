import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Loader2,
  AlertCircle,
  CheckCircle2,
  XCircle,
  Cpu,
  Clock,
  DollarSign,
  ArrowLeftRight,
  RotateCw,
  Send,
} from "lucide-react";
import {
  getAgentTraces,
  getTraceById,
  type CloudAgentTrace,
} from "@/lib/cloudAgentTraces";
import {
  startReplay,
  getReplayJob,
  type ReplayJob,
} from "@/lib/tauri-api";
import { cn } from "@/lib/utils";

// v2.1.0 Phase 9 — Eval workbench (compare).
//
// Pick a baseline trace, pick a comparison trace from recent runs of
// the same agent, see them side-by-side. The "what changed?" answer
// becomes obvious when you can read both at once instead of
// alt-tabbing between rows.
//
// Why this is the right v1 of "eval workbench":
//   - Replay infra (re-dispatch the same prompt against a tweaked
//     config) needs full-prompt storage which has PII implications
//     we don't want to make today.
//   - Side-by-side reads off existing trace data, no new schema, no
//     new dispatch endpoint.
//   - It's also the right UX for "did the regression detector catch
//     a real thing?" — open the regression, click compare, look.

interface Props {
  baselineTraceId: string;
  /** Constrain the comparison candidates to this agent slug — usually
   *  the same as baseline.agent_slug so the comparison is meaningful.
   *  Pass null to allow cross-agent comparison (rare). */
  agentSlug: string | null;
  onClose: () => void;
}

export default function TraceCompareModal({ baselineTraceId, agentSlug, onClose }: Props) {
  const { t } = useTranslation();
  const [comparisonId, setComparisonId] = useState<string | null>(null);
  // v2.1.0 Replay infra. Open the picker submodal when user clicks
  // "Replay this prompt"; null otherwise.
  const [replayPickerOpen, setReplayPickerOpen] = useState(false);
  // Currently-running or just-finished replay. The result panel
  // renders in-place once we have a job id to track.
  const [activeReplayId, setActiveReplayId] = useState<string | null>(null);

  const baselineQuery = useQuery({
    queryKey: ["trace-by-id", baselineTraceId],
    queryFn: () => getTraceById(baselineTraceId),
    staleTime: 60_000,
  });

  // Fetch recent traces (filtered to agentSlug if provided) so the
  // user can pick a comparison without typing trace IDs by hand.
  const candidatesQuery = useQuery({
    queryKey: ["compare-candidates", agentSlug],
    queryFn: () => getAgentTraces(agentSlug ?? undefined, 50),
    staleTime: 30_000,
  });

  const comparisonQuery = useQuery({
    queryKey: ["trace-by-id", comparisonId],
    queryFn: () => getTraceById(comparisonId!),
    enabled: !!comparisonId,
    staleTime: 60_000,
  });

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-5xl max-h-[90vh] overflow-hidden rounded-lg border border-cs-border bg-cs-bg-raised shadow-2xl flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-start justify-between gap-3 border-b border-cs-border p-4">
          <div className="min-w-0">
            <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
              <ArrowLeftRight size={14} className="text-cs-accent shrink-0" />
              {t("insights.compare.title", "Compare traces")}
            </h3>
            <p className="mt-1 text-[11px] text-cs-muted">
              {t(
                "insights.compare.subtitle",
                "Side-by-side view of two runs. Pick a comparison from the list below.",
              )}
            </p>
          </div>
          <div className="shrink-0 flex items-center gap-2">
            {/* v2.1.0 Replay infra. Opens the picker submodal for
                target runtime + model. The actual dispatch goes
                through prompt_agent_inner so it's killable + shows
                up in Live runs. */}
            <button
              type="button"
              onClick={() => setReplayPickerOpen(true)}
              className="inline-flex items-center gap-1 rounded-md border border-cs-accent/40 bg-cs-accent/10 px-2 py-1 text-[11px] font-medium text-cs-accent hover:bg-cs-accent/20"
              title={t(
                "insights.compare.replayHint",
                "Re-run the baseline prompt against a different runtime",
              )}
            >
              <RotateCw size={11} />
              {t("insights.compare.replay", "Replay")}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded-md border border-cs-border bg-cs-bg px-2 py-1 text-[11px] text-cs-muted hover:text-cs-text"
            >
              {t("common.close", "Close")}
            </button>
          </div>
        </header>

        <div className="flex-1 min-h-0 overflow-y-auto p-4 space-y-4">
          {/* Side-by-side. Vertical stack on narrow viewports — the
              modal is max-w-5xl so most desktop windows show both. */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            <TracePane
              label={t("insights.compare.baseline", "Baseline")}
              query={baselineQuery}
            />
            <TracePane
              label={t("insights.compare.comparison", "Comparison")}
              query={comparisonQuery}
              empty={!comparisonId}
            />
          </div>

          {/* Diff highlights — duration, cost, ok rate, files diff.
              Only shows when both traces are loaded so the section
              doesn't flicker into view too early. */}
          {baselineQuery.data && comparisonQuery.data && (
            <DiffSummary a={baselineQuery.data} b={comparisonQuery.data} />
          )}

          {/* Candidate picker. Visible while we don't have a
              comparison selected yet; collapses afterward. */}
          {!comparisonId && (
            <CandidatePicker
              query={candidatesQuery}
              excludeId={baselineTraceId}
              onPick={setComparisonId}
            />
          )}
          {comparisonId && (
            <button
              type="button"
              onClick={() => setComparisonId(null)}
              className="text-[11px] text-cs-muted hover:text-cs-text underline-offset-2 hover:underline"
            >
              ← {t("insights.compare.changeComparison", "Pick a different comparison")}
            </button>
          )}

          {/* v2.1.0 Replay result panel. Rendered when a replay is in
              flight or just completed. Side-by-side with baseline. */}
          {activeReplayId && (
            <ReplayResultPanel
              jobId={activeReplayId}
              baselineTrace={baselineQuery.data ?? null}
              onClear={() => setActiveReplayId(null)}
            />
          )}
        </div>
      </div>

      {/* v2.1.0 Replay picker submodal. */}
      {replayPickerOpen && (
        <ReplayPicker
          baselineTrace={baselineQuery.data ?? null}
          baselineTraceId={baselineTraceId}
          onClose={() => setReplayPickerOpen(false)}
          onStarted={(jobId) => {
            setReplayPickerOpen(false);
            setActiveReplayId(jobId);
          }}
        />
      )}
    </div>
  );
}

function TracePane({
  label,
  query,
  empty,
}: {
  label: string;
  query: { isLoading: boolean; isError: boolean; error: unknown; data: CloudAgentTrace | null | undefined };
  empty?: boolean;
}) {
  const { t } = useTranslation();
  if (empty) {
    return (
      <section className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-4 min-h-[180px] flex items-center justify-center text-center">
        <div>
          <div className="text-[10px] uppercase tracking-wide text-cs-muted">{label}</div>
          <p className="mt-2 text-[11px] text-cs-muted">
            {t("insights.compare.pickPrompt", "Pick a trace below to compare against the baseline.")}
          </p>
        </div>
      </section>
    );
  }
  if (query.isLoading) {
    return (
      <section className="rounded-lg border border-cs-border bg-cs-bg-raised/40 p-4 min-h-[180px] flex items-center justify-center text-cs-muted text-xs">
        <Loader2 size={14} className="animate-spin mr-2" />
        {t("common.loading", "Loading…")}
      </section>
    );
  }
  if (query.isError || !query.data) {
    return (
      <section className="rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger inline mr-1" />
        {t("insights.compare.notFound", "Trace not found in the recent window. Older traces aren't loadable yet.")}
      </section>
    );
  }
  const tr = query.data;
  return (
    <section className="rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3 space-y-2">
      <div className="flex items-center gap-2">
        {tr.ok ? (
          <CheckCircle2 size={11} className="text-cs-accent shrink-0" />
        ) : (
          <XCircle size={11} className="text-cs-danger shrink-0" />
        )}
        <span className="text-[10px] uppercase tracking-wide text-cs-muted">{label}</span>
        <span className="ml-auto font-mono text-[10px] text-cs-muted">
          {new Date(tr.started_at).toLocaleString()}
        </span>
      </div>
      <div className="flex items-center gap-2 text-xs">
        <code className="font-mono text-cs-text font-medium">@{tr.agent_slug}</code>
        <span className="text-[10px] uppercase tracking-wide text-cs-muted">
          <Cpu size={9} className="inline" /> {tr.runtime}
        </span>
      </div>
      <div className="grid grid-cols-3 gap-2 text-[11px]">
        <PaneStat label={t("insights.compare.duration", "Duration")} value={`${tr.duration_ms}ms`} />
        <PaneStat
          label={t("insights.compare.cost", "Cost")}
          value={tr.cost_usd != null ? `$${tr.cost_usd.toFixed(4)}` : "—"}
        />
        <PaneStat
          label={t("insights.compare.tokens", "Tokens")}
          value={
            (tr.prompt_tokens ?? 0) + (tr.response_tokens ?? 0) > 0
              ? `${(tr.prompt_tokens ?? 0).toLocaleString()} / ${(tr.response_tokens ?? 0).toLocaleString()}`
              : "—"
          }
        />
      </div>
      {tr.prompt_summary && (
        <div className="rounded border border-cs-border bg-cs-bg p-2 text-[11px]">
          <div className="text-[9px] uppercase tracking-wide text-cs-muted mb-0.5">
            {t("insights.compare.prompt", "Prompt")}
          </div>
          <div className="text-cs-text break-words">{tr.prompt_summary}</div>
        </div>
      )}
      {tr.error && (
        <div className="rounded border border-cs-danger/40 bg-cs-danger/10 p-2 text-[11px] text-cs-danger break-words">
          {tr.error}
        </div>
      )}
      {tr.files_touched && tr.files_touched.length > 0 && (
        <div className="text-[11px]">
          <div className="text-[9px] uppercase tracking-wide text-cs-muted mb-0.5">
            {t("insights.compare.files", "Files touched ({{n}})", { n: tr.files_touched.length })}
          </div>
          <ul className="space-y-0.5">
            {tr.files_touched.slice(0, 8).map((f) => (
              <li key={f} className="font-mono text-[10px] text-cs-text truncate">
                {f}
              </li>
            ))}
            {tr.files_touched.length > 8 && (
              <li className="text-[10px] text-cs-muted">
                {t("insights.compare.moreFiles", "+ {{n}} more", { n: tr.files_touched.length - 8 })}
              </li>
            )}
          </ul>
        </div>
      )}
    </section>
  );
}

function PaneStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded border border-cs-border bg-cs-bg p-2">
      <div className="text-[9px] uppercase tracking-wide text-cs-muted">{label}</div>
      <div className="mt-0.5 font-mono text-cs-text">{value}</div>
    </div>
  );
}

function CandidatePicker({
  query,
  excludeId,
  onPick,
}: {
  query: {
    isLoading: boolean;
    data: { traces: CloudAgentTrace[] } | null | undefined;
  };
  excludeId: string;
  onPick: (id: string) => void;
}) {
  const { t } = useTranslation();
  if (query.isLoading) {
    return (
      <section className="text-xs text-cs-muted">
        <Loader2 size={11} className="inline animate-spin mr-1" />
        {t("insights.compare.loadingCandidates", "Loading candidates…")}
      </section>
    );
  }
  const candidates = (query.data?.traces ?? []).filter((t) => t.id !== excludeId);
  if (candidates.length === 0) {
    return (
      <p className="text-[11px] text-cs-muted">
        {t("insights.compare.noCandidates", "No other traces to compare against in the recent window.")}
      </p>
    );
  }
  return (
    <section>
      <h4 className="text-[10px] uppercase tracking-wide text-cs-muted mb-1.5">
        {t("insights.compare.pickHeader", "Pick comparison ({{n}})", { n: candidates.length })}
      </h4>
      <ul className="space-y-1 max-h-64 overflow-y-auto">
        {candidates.map((tr, i) => (
          <li key={tr.id}>
            <button
              type="button"
              data-demo-id={i === 0 ? "compare-candidate-first" : undefined}
              onClick={() => onPick(tr.id)}
              className="w-full text-left flex items-center gap-2 rounded border border-cs-border bg-cs-bg px-2 py-1.5 text-[11px] hover:border-cs-accent/40"
            >
              {tr.ok ? (
                <CheckCircle2 size={10} className="text-cs-accent shrink-0" />
              ) : (
                <XCircle size={10} className="text-cs-danger shrink-0" />
              )}
              <code className="font-mono text-cs-muted shrink-0">
                {new Date(tr.started_at).toLocaleString()}
              </code>
              <span className="text-[10px] uppercase tracking-wide text-cs-muted shrink-0">
                {tr.runtime}
              </span>
              <span className="font-mono text-cs-muted shrink-0">{tr.duration_ms}ms</span>
              {tr.prompt_summary && (
                <span className="text-cs-text truncate flex-1">{tr.prompt_summary}</span>
              )}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function DiffSummary({ a, b }: { a: CloudAgentTrace; b: CloudAgentTrace }) {
  const { t } = useTranslation();
  const durationDelta = b.duration_ms - a.duration_ms;
  const durationPct = a.duration_ms > 0 ? (durationDelta / a.duration_ms) * 100 : 0;
  const costDelta = (b.cost_usd ?? 0) - (a.cost_usd ?? 0);
  const okChanged = a.ok !== b.ok;
  const filesA = new Set(a.files_touched ?? []);
  const filesB = new Set(b.files_touched ?? []);
  const onlyA = [...filesA].filter((f) => !filesB.has(f));
  const onlyB = [...filesB].filter((f) => !filesA.has(f));

  return (
    <section className="rounded-md border border-cs-accent/30 bg-cs-accent/5 p-3">
      <h4 className="text-[10px] uppercase tracking-wide text-cs-accent mb-2 flex items-center gap-1">
        <ArrowLeftRight size={10} />
        {t("insights.compare.diffTitle", "Diff")}
      </h4>
      <div className="grid grid-cols-1 md:grid-cols-3 gap-2 text-[11px]">
        <DiffStat
          icon={<Clock size={10} />}
          label={t("insights.compare.diffDuration", "Duration")}
          delta={`${durationDelta >= 0 ? "+" : ""}${durationDelta}ms (${durationPct >= 0 ? "+" : ""}${durationPct.toFixed(0)}%)`}
          good={durationDelta <= 0}
        />
        <DiffStat
          icon={<DollarSign size={10} />}
          label={t("insights.compare.diffCost", "Cost")}
          delta={`${costDelta >= 0 ? "+" : ""}$${Math.abs(costDelta).toFixed(4)}`}
          good={costDelta <= 0}
        />
        <DiffStat
          icon={okChanged ? <XCircle size={10} /> : <CheckCircle2 size={10} />}
          label={t("insights.compare.diffStatus", "Status")}
          delta={
            okChanged
              ? a.ok
                ? t("insights.compare.regressed", "OK → fail")
                : t("insights.compare.recovered", "fail → OK")
              : t("insights.compare.unchanged", "unchanged")
          }
          good={!okChanged || (b.ok && !a.ok)}
        />
      </div>
      {(onlyA.length > 0 || onlyB.length > 0) && (
        <div className="mt-2 text-[11px]">
          <div className="text-[9px] uppercase tracking-wide text-cs-muted mb-0.5">
            {t("insights.compare.fileDiff", "Files different between runs")}
          </div>
          <div className="grid grid-cols-2 gap-2">
            <div>
              <div className="text-[10px] text-cs-muted mb-0.5">
                {t("insights.compare.onlyBaseline", "Only in baseline ({{n}})", { n: onlyA.length })}
              </div>
              <ul>
                {onlyA.slice(0, 8).map((f) => (
                  <li key={f} className="font-mono text-[10px] text-cs-text truncate">
                    {f}
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <div className="text-[10px] text-cs-muted mb-0.5">
                {t("insights.compare.onlyComparison", "Only in comparison ({{n}})", {
                  n: onlyB.length,
                })}
              </div>
              <ul>
                {onlyB.slice(0, 8).map((f) => (
                  <li key={f} className="font-mono text-[10px] text-cs-text truncate">
                    {f}
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function DiffStat({
  icon,
  label,
  delta,
  good,
}: {
  icon: React.ReactNode;
  label: string;
  delta: string;
  good: boolean;
}) {
  return (
    <div className="rounded border border-cs-border bg-cs-bg p-2">
      <div className="flex items-center gap-1 text-cs-muted">
        {icon}
        <span className="text-[9px] uppercase tracking-wide">{label}</span>
      </div>
      <div className={cn("mt-0.5 font-mono", good ? "text-cs-accent" : "text-cs-danger")}>
        {delta}
      </div>
    </div>
  );
}

// v2.1.0 — Replay picker submodal. User picks target runtime + optional
// model override; on confirm we call start_replay and hand the job id
// back to the parent so it can render the result panel inline.
const REPLAY_RUNTIME_OPTIONS = ["claude", "codex", "gemini", "openclaw", "hermes"] as const;

function ReplayPicker({
  baselineTrace,
  baselineTraceId,
  onClose,
  onStarted,
}: {
  baselineTrace: CloudAgentTrace | null;
  baselineTraceId: string;
  onClose: () => void;
  onStarted: (jobId: string) => void;
}) {
  const { t } = useTranslation();
  const sourceRuntime = baselineTrace?.runtime ?? "claude";
  // Default target: anything that's NOT the source so the diff is
  // immediately meaningful. User can change.
  const defaultTarget =
    REPLAY_RUNTIME_OPTIONS.find((r) => r !== sourceRuntime) ?? "claude";
  const [targetRuntime, setTargetRuntime] = useState<string>(defaultTarget);
  const [targetModel, setTargetModel] = useState<string>("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setSubmitting(true);
    setError(null);
    try {
      const jobId = await startReplay(
        baselineTraceId,
        targetRuntime,
        targetModel.trim() || undefined,
      );
      onStarted(jobId);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      // The Rust side returns "prompt-not-local" when the trace
      // wasn't dispatched on this machine. Surface a friendlier
      // message — this is a v1 limitation, not a bug.
      if (msg.includes("prompt-not-local")) {
        setError(
          t(
            "insights.replay.errPromptNotLocal",
            "Source prompt not available locally — replay requires the original dispatch machine. Multi-device replay is on the roadmap.",
          ),
        );
      } else {
        setError(msg);
      }
      setSubmitting(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/70 p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-lg border border-cs-border bg-cs-bg-raised shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-center justify-between gap-2 border-b border-cs-border p-3">
          <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
            <RotateCw size={13} className="text-cs-accent" />
            {t("insights.replay.pickerTitle", "Replay this prompt")}
          </h3>
          <button
            type="button"
            onClick={onClose}
            className="rounded-md border border-cs-border bg-cs-bg px-2 py-1 text-[11px] text-cs-muted hover:text-cs-text"
          >
            {t("common.cancel", "Cancel")}
          </button>
        </header>
        <div className="p-3 space-y-3">
          <div>
            <label className="text-[10px] uppercase tracking-wide text-cs-muted">
              {t("insights.replay.targetRuntime", "Target runtime")}
            </label>
            <select
              value={targetRuntime}
              onChange={(e) => setTargetRuntime(e.target.value)}
              className="mt-1 w-full rounded-md border border-cs-border bg-cs-bg px-2 py-1.5 text-[12px] text-cs-text"
            >
              {REPLAY_RUNTIME_OPTIONS.map((r) => (
                <option key={r} value={r}>
                  {r}
                  {r === sourceRuntime ? ` (${t("insights.replay.sameAsSource", "same as source")})` : ""}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="text-[10px] uppercase tracking-wide text-cs-muted">
              {t("insights.replay.targetModel", "Model override (optional)")}
            </label>
            <input
              type="text"
              value={targetModel}
              onChange={(e) => setTargetModel(e.target.value)}
              placeholder={t(
                "insights.replay.targetModelPlaceholder",
                "Leave blank for runtime default",
              )}
              className="mt-1 w-full rounded-md border border-cs-border bg-cs-bg px-2 py-1.5 font-mono text-[12px] text-cs-text"
            />
          </div>
          {/* Pre-dispatch disclosure — explicit consent through the
              click. Replays send prompt content to a different
              provider's API; we surface that plainly. */}
          <div className="rounded border border-cs-warn/30 bg-cs-warn/5 p-2 text-[11px] text-cs-text">
            {t(
              "insights.replay.dataResidencyDisclosure",
              "This will send the original prompt content to {{runtime}}'s API. Continue only if your data policy allows it.",
              { runtime: targetRuntime },
            )}
          </div>
          {error && (
            <div className="rounded border border-cs-danger/40 bg-cs-danger/10 p-2 text-[11px] text-cs-danger">
              {error}
            </div>
          )}
          <button
            type="button"
            onClick={submit}
            disabled={submitting}
            className="inline-flex items-center justify-center gap-1.5 w-full rounded-md bg-cs-accent px-3 py-2 text-[12px] font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-60"
          >
            {submitting ? <Loader2 size={12} className="animate-spin" /> : <Send size={12} />}
            {t("insights.replay.run", "Run replay")}
          </button>
        </div>
      </div>
    </div>
  );
}

// v2.1.0 — Replay result panel. Polls get_replay_job until status is
// terminal, then renders side-by-side with the baseline. Cost shown as
// "—" because v1 doesn't capture token usage on UI dispatches; it'll
// earn a real number in a follow-up that adds the PRICING_PER_M_TOKENS
// path to the desktop dispatch flow.
function ReplayResultPanel({
  jobId,
  baselineTrace,
  onClear,
}: {
  jobId: string;
  baselineTrace: CloudAgentTrace | null;
  onClear: () => void;
}) {
  const { t } = useTranslation();
  const [job, setJob] = useState<ReplayJob | null>(null);
  const [pollError, setPollError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: number | null = null;
    const tick = async () => {
      try {
        const next = await getReplayJob(jobId);
        if (cancelled) return;
        setJob(next);
        if (next.status === "pending" || next.status === "running") {
          // 800ms cadence — fast enough that the spinner feels live,
          // slow enough not to spam the DB.
          timer = window.setTimeout(tick, 800);
        }
      } catch (e) {
        if (cancelled) return;
        setPollError(e instanceof Error ? e.message : String(e));
      }
    };
    void tick();
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [jobId]);

  if (pollError) {
    return (
      <div className="rounded border border-cs-danger/40 bg-cs-danger/5 p-3 text-[11px] text-cs-danger">
        <AlertCircle size={11} className="inline mr-1" />
        {t("insights.replay.pollError", "Couldn't fetch replay status")}: {pollError}
      </div>
    );
  }
  if (!job) {
    return (
      <div className="rounded border border-cs-border bg-cs-bg-raised/40 p-3 text-[11px] text-cs-muted">
        <Loader2 size={12} className="inline animate-spin mr-1" />
        {t("insights.replay.queuing", "Queuing replay…")}
      </div>
    );
  }

  const running = job.status === "pending" || job.status === "running";
  const sourceDuration = baselineTrace?.duration_ms ?? null;
  const replayDuration = job.duration_ms ?? null;
  const durationDeltaMs =
    sourceDuration !== null && replayDuration !== null
      ? replayDuration - sourceDuration
      : null;

  return (
    <section className="rounded-lg border border-cs-accent/30 bg-cs-accent/5 p-3 space-y-2">
      <header className="flex items-center gap-2 flex-wrap">
        <RotateCw size={12} className={cn("text-cs-accent", running && "animate-spin")} />
        <h4 className="text-[12px] font-medium text-cs-text">
          {t("insights.replay.resultTitle", "Replay")}
        </h4>
        <span className="inline-flex items-center gap-1 text-[10px] font-mono text-cs-muted">
          <span className="text-cs-text">{job.source_runtime}</span>
          <span>→</span>
          <span className="text-cs-accent">{job.target_runtime}</span>
          {job.target_model && <span className="text-cs-muted">/ {job.target_model}</span>}
        </span>
        <span
          className={cn(
            "inline-flex items-center gap-1 rounded-full border px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide",
            job.status === "done" && "border-cs-accent/40 bg-cs-accent/10 text-cs-accent",
            job.status === "failed" && "border-cs-danger/40 bg-cs-danger/10 text-cs-danger",
            running && "border-cs-border bg-cs-bg text-cs-muted",
          )}
        >
          {job.status === "running" && <Loader2 size={9} className="animate-spin" />}
          {job.status === "done" && <CheckCircle2 size={9} />}
          {job.status === "failed" && <XCircle size={9} />}
          {job.status}
        </span>
        <button
          type="button"
          onClick={onClear}
          className="ml-auto text-[10px] text-cs-muted hover:text-cs-text underline-offset-2 hover:underline"
        >
          {t("insights.replay.clear", "Clear")}
        </button>
      </header>

      {running && (
        <p className="text-[11px] text-cs-muted">
          {t(
            "insights.replay.runningHint",
            "Dispatching to {{runtime}}. Watch this run live in Insights → Live; you can kill it from there.",
            { runtime: job.target_runtime },
          )}
        </p>
      )}

      {job.status === "failed" && job.error_message && (
        <div className="rounded border border-cs-danger/40 bg-cs-danger/10 p-2 text-[11px] text-cs-danger break-words">
          <strong className="font-medium">
            {t("insights.replay.error", "Replay failed")}:
          </strong>{" "}
          {job.error_message}
        </div>
      )}

      {job.status === "done" && (
        <>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            <div className="rounded border border-cs-border bg-cs-bg p-2">
              <div className="text-[9px] uppercase tracking-wide text-cs-muted mb-1">
                {t("insights.replay.sourceLabel", "Source ({{runtime}})", {
                  runtime: job.source_runtime,
                })}
              </div>
              <pre className="whitespace-pre-wrap break-words font-sans text-[11px] text-cs-text max-h-[300px] overflow-y-auto">
                {baselineTrace?.response ?? t("insights.replay.sourceMissing", "Source response unavailable")}
              </pre>
            </div>
            <div className="rounded border border-cs-accent/30 bg-cs-bg p-2">
              <div className="text-[9px] uppercase tracking-wide text-cs-accent mb-1">
                {t("insights.replay.replayLabel", "Replay ({{runtime}})", {
                  runtime: job.target_runtime,
                })}
              </div>
              <pre className="whitespace-pre-wrap break-words font-sans text-[11px] text-cs-text max-h-[300px] overflow-y-auto">
                {job.response ?? ""}
              </pre>
            </div>
          </div>
          <div className="grid grid-cols-3 gap-2 text-[11px]">
            <DiffStat
              icon={<Clock size={10} />}
              label={t("insights.replay.duration", "Duration")}
              delta={
                durationDeltaMs !== null
                  ? `${durationDeltaMs >= 0 ? "+" : ""}${durationDeltaMs}ms`
                  : "—"
              }
              good={durationDeltaMs !== null ? durationDeltaMs <= 0 : true}
            />
            <DiffStat
              icon={<DollarSign size={10} />}
              label={t("insights.replay.cost", "Cost")}
              delta="—"
              good
            />
            <DiffStat
              icon={<Cpu size={10} />}
              label={t("insights.replay.runtimePair", "Runtime")}
              delta={`${job.source_runtime} → ${job.target_runtime}`}
              good
            />
          </div>
          <p className="text-[10px] text-cs-muted">
            {t(
              "insights.replay.costPunt",
              "Cost capture for replay results lands in a follow-up — desktop dispatches don't track tokens yet, only deployed bundles do.",
            )}
          </p>
        </>
      )}
    </section>
  );
}
