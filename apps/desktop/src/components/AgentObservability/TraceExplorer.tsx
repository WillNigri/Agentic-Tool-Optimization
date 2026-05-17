import { useTranslation } from "react-i18next";
import {
  X,
  CheckCircle2,
  XCircle,
  Clock,
  Network,
  ArrowRight,
} from "lucide-react";
import type { AgentTraceLine } from "@/lib/agentObservability";
import { cn } from "@/lib/utils";

// v1.4.0 F6 — Trace explorer modal.
// Shows the full structured breakdown of a single trace line: who dispatched
// it, the prompt, the response, latency, errors, routing decision (if it was
// a group dispatch). Free-tier surface; reads from ~/.ato/agent-logs.jsonl.

const RUNTIME_DOT: Record<string, string> = {
  claude: "bg-orange-500",
  codex: "bg-green-500",
  gemini: "bg-blue-500",
  openclaw: "bg-cyan-400",
  hermes: "bg-purple-500",
};

interface Props {
  trace: AgentTraceLine;
  onClose: () => void;
}

const STANDARD_KEYS = new Set([
  "ts",
  "durationMs",
  "runtime",
  "slug",
  "filePath",
  "promptPreview",
  "responsePreview",
  "ok",
  "error",
  "source",
  "routedTo",
]);

export default function TraceExplorer({ trace, onClose }: Props) {
  const { t } = useTranslation();
  const ok = trace.ok !== false;
  const extra = Object.entries(trace).filter(
    ([k, v]) => !STANDARD_KEYS.has(k) && v !== undefined && v !== null
  );

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-3xl max-h-[88vh] flex flex-col rounded-2xl border border-cs-border bg-cs-card shadow-2xl">
        <header className="flex items-center justify-between p-5 border-b border-cs-border">
          <div className="flex items-center gap-3 min-w-0">
            {ok ? (
              <CheckCircle2 size={18} className="text-cs-accent shrink-0" />
            ) : (
              <XCircle size={18} className="text-cs-danger shrink-0" />
            )}
            <div className="min-w-0">
              <h2 className="text-sm font-semibold text-cs-text truncate">
                {trace.slug ?? (
                  <span className="text-cs-muted italic">
                    {t("traceExplorer.generalist", "generalist")}
                  </span>
                )}
              </h2>
              <p className="text-[11px] text-cs-muted">
                {trace.ts && (
                  <span className="tabular-nums">
                    {new Date(trace.ts).toLocaleString()}
                  </span>
                )}
                {trace.runtime && (
                  <span className="ml-2 inline-flex items-center gap-1">
                    <span
                      className={cn(
                        "inline-block w-1.5 h-1.5 rounded-full",
                        RUNTIME_DOT[trace.runtime] ?? "bg-cs-muted"
                      )}
                    />
                    {trace.runtime}
                  </span>
                )}
                {trace.source && (
                  <span className="ml-2 text-cs-muted">via {trace.source}</span>
                )}
              </p>
            </div>
          </div>
          <button
            type="button"
            aria-label={t("common.close", "Close")}
            onClick={onClose}
            className="text-cs-muted hover:text-cs-text"
          >
            <X size={18} />
          </button>
        </header>

        <div className="flex-1 overflow-y-auto p-5 space-y-4 min-h-0">
          {/* Top-line stats */}
          <div className="grid grid-cols-3 gap-3 text-xs">
            <Stat
              label={t("traceExplorer.duration", "Duration")}
              value={
                trace.durationMs !== undefined ? (
                  <span className="inline-flex items-center gap-1">
                    <Clock size={11} />
                    {trace.durationMs}ms
                  </span>
                ) : (
                  "—"
                )
              }
            />
            <Stat
              label={t("traceExplorer.status", "Status")}
              value={
                ok ? (
                  <span className="text-cs-accent">success</span>
                ) : (
                  <span className="text-cs-danger">failed</span>
                )
              }
            />
            <Stat
              label={t("traceExplorer.routedTo", "Routed to")}
              value={
                trace.routedTo ? (
                  <span className="inline-flex items-center gap-1">
                    <Network size={11} className="text-cs-accent" />
                    <code className="font-mono">{trace.routedTo}</code>
                  </span>
                ) : (
                  "—"
                )
              }
            />
          </div>

          {/* Routing path (visualized for group dispatches) */}
          {trace.routedTo && trace.slug && (
            <Section label={t("traceExplorer.routingPath", "Routing path")}>
              <div className="flex items-center gap-2 text-xs">
                <code className="font-mono text-cs-text rounded bg-cs-bg-raised px-2 py-1">
                  {trace.slug}
                </code>
                <ArrowRight size={12} className="text-cs-muted" />
                <code className="font-mono text-cs-accent rounded bg-cs-bg-raised px-2 py-1">
                  {trace.routedTo}
                </code>
              </div>
            </Section>
          )}

          {/* Prompt */}
          {trace.promptPreview && (
            <Section label={t("traceExplorer.prompt", "Prompt")}>
              <pre className="rounded bg-cs-bg p-3 text-[12px] text-cs-text font-mono whitespace-pre-wrap max-h-48 overflow-y-auto">
                {trace.promptPreview}
              </pre>
            </Section>
          )}

          {/* Response */}
          {trace.responsePreview && (
            <Section label={t("traceExplorer.response", "Response")}>
              <pre className="rounded bg-cs-bg p-3 text-[12px] text-cs-text font-mono whitespace-pre-wrap max-h-64 overflow-y-auto">
                {trace.responsePreview}
              </pre>
            </Section>
          )}

          {/* Error */}
          {trace.error && (
            <Section label={t("traceExplorer.error", "Error")}>
              <pre className="rounded border border-cs-danger/40 bg-cs-danger/10 p-3 text-[12px] text-cs-danger font-mono whitespace-pre-wrap">
                {trace.error}
              </pre>
            </Section>
          )}

          {/* File path of the agent */}
          {trace.filePath && (
            <Section label={t("traceExplorer.filePath", "Agent file")}>
              <code className="text-[11px] font-mono text-cs-muted break-all">
                {trace.filePath}
              </code>
            </Section>
          )}

          {/* Anything the trace producer added beyond the standard keys —
              hooks output, evaluator scores once F7 lands, etc. */}
          {extra.length > 0 && (
            <Section label={t("traceExplorer.extra", "Additional fields")}>
              <pre className="rounded bg-cs-bg p-3 text-[11px] text-cs-muted font-mono whitespace-pre-wrap max-h-48 overflow-y-auto">
                {JSON.stringify(Object.fromEntries(extra), null, 2)}
              </pre>
            </Section>
          )}
        </div>
      </div>
    </div>
  );
}

function Section({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <h3 className="text-[10px] uppercase tracking-wide text-cs-muted mb-1.5">
        {label}
      </h3>
      {children}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-cs-border bg-cs-bg-raised p-2.5">
      <div className="text-[10px] uppercase tracking-wide text-cs-muted">{label}</div>
      <div className="mt-1 text-cs-text">{value}</div>
    </div>
  );
}
