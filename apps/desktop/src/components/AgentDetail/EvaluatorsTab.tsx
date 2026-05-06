import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Plus,
  Trash2,
  Loader2,
  AlertCircle,
  CheckCircle2,
  XCircle,
  PlayCircle,
  ChevronDown,
  ChevronRight,
  Crown,
  Lock,
  Zap,
} from "lucide-react";
import {
  listAgentEvaluators,
  saveAgentEvaluator,
  deleteAgentEvaluator,
  evaluateRecentTraces,
  parseEvaluatorConfig,
  evaluatorConfigToJson,
  FREE_EVALUATOR_KINDS,
  PRO_EVALUATOR_KINDS,
  type AgentEvaluator,
  type EvaluatorKind,
  type EvaluatorConfig,
  type EvaluatedTrace,
} from "@/lib/agentObservability";
import { useFeatureFlag } from "@/lib/tier";
import TierGate from "@/components/Tier/TierGate";
import UpgradePrompt from "@/components/Tier/UpgradePrompt";
import type { Agent } from "@/lib/agents";
import { cn } from "@/lib/utils";

// v1.4.0 F7 — Evaluators tab.
//
// Whole tab gated behind `evaluators` (Pro). Free users see lock + upgrade
// prompt. Pro users get heuristic CRUD + manual "Run on last N traces". The
// LLM-as-judge kind is stubbed in the OSS Rust executor (Wave 4.5 wires it
// to a Pro cloud endpoint).

interface Props {
  agent: Agent;
}

const KIND_LABEL: Record<EvaluatorKind, string> = {
  "contains": "Response contains",
  "not-contains": "Response excludes",
  "length-range": "Length range",
  "tool-called": "Tool was called",
  "llm-judge": "LLM-as-judge",
};

const KIND_HINT: Record<EvaluatorKind, string> = {
  "contains": "Pass when the response contains a given substring.",
  "not-contains": "Pass when the response does NOT contain the substring.",
  "length-range": "Pass when the response is between min and max characters.",
  "tool-called": "Pass when the response references the named tool.",
  "llm-judge": "Ask a small model to score the response from 0–1 with a reason.",
};

export default function EvaluatorsTab({ agent }: Props) {
  return (
    <TierGate feature="evaluators">
      <EvaluatorsEditor agent={agent} />
    </TierGate>
  );
}

function EvaluatorsEditor({ agent }: { agent: Agent }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState<AgentEvaluator | "new" | null>(null);
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<EvaluatedTrace[] | null>(null);
  const [runError, setRunError] = useState<string | null>(null);

  const { data: evaluators = [], isLoading, error } = useQuery({
    queryKey: ["agent-evaluators", agent.slug],
    queryFn: () => listAgentEvaluators(agent.slug),
    staleTime: 5_000,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteAgentEvaluator(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["agent-evaluators", agent.slug] });
    },
  });

  const runEvaluators = async (lastN: number) => {
    if (running) return;
    setRunning(true);
    setRunError(null);
    setResults(null);
    try {
      const out = await evaluateRecentTraces(agent.slug, lastN);
      setResults(out);
    } catch (e) {
      setRunError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="space-y-5">
      <header className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Zap size={16} className="text-cs-accent" />
            <h3 className="text-sm font-medium text-cs-text">
              {t("agentDetail.evaluators.title", "Evaluators")}
            </h3>
          </div>
          <p className="mt-1 text-xs text-cs-muted max-w-2xl">
            {t(
              "agentDetail.evaluators.subtitle",
              "Score every run as pass/fail. Manual + scheduled batch only — never live on every dispatch (predictable cost, no added latency)."
            )}
          </p>
        </div>
        {evaluators.length > 0 && (
          <button
            type="button"
            onClick={() => runEvaluators(10)}
            disabled={running}
            className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50 shrink-0"
          >
            {running ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <PlayCircle size={12} />
            )}
            {t("agentDetail.evaluators.runLast", "Run on last 10 traces")}
          </button>
        )}
      </header>

      {error && (
        <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
          <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
          <span>{error instanceof Error ? error.message : String(error)}</span>
        </div>
      )}

      {/* Run results */}
      {(results || runError) && (
        <RunResults
          results={results ?? []}
          error={runError}
          onClose={() => {
            setResults(null);
            setRunError(null);
          }}
        />
      )}

      {/* Evaluators list */}
      <div className="space-y-2">
        {isLoading ? (
          <div className="flex items-center justify-center h-20">
            <Loader2 size={16} className="animate-spin text-cs-muted" />
          </div>
        ) : evaluators.length === 0 && editing !== "new" ? (
          <EmptyState onAdd={() => setEditing("new")} />
        ) : (
          evaluators.map((e) => (
            <EvaluatorRow
              key={e.id}
              evaluator={e}
              onEdit={() => setEditing(e)}
              onDelete={() => deleteMutation.mutate(e.id)}
              deleting={deleteMutation.isPending && deleteMutation.variables === e.id}
            />
          ))
        )}
      </div>

      {evaluators.length > 0 && editing !== "new" && (
        <button
          type="button"
          onClick={() => setEditing("new")}
          className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
        >
          <Plus size={12} />
          {t("agentDetail.evaluators.add", "Add evaluator")}
        </button>
      )}

      {editing && (
        <EvaluatorEditorForm
          agentSlug={agent.slug}
          existing={editing === "new" ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({ queryKey: ["agent-evaluators", agent.slug] });
            setEditing(null);
          }}
        />
      )}
    </div>
  );
}

function EmptyState({ onAdd }: { onAdd: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 flex items-start gap-3">
      <Zap size={20} className="text-cs-muted shrink-0" />
      <div className="flex-1 min-w-0">
        <p className="text-sm text-cs-text">
          {t("agentDetail.evaluators.emptyTitle", "No evaluators yet")}
        </p>
        <p className="mt-1 text-xs text-cs-muted">
          {t(
            "agentDetail.evaluators.emptyBody",
            "Add a heuristic check (e.g. \"response contains 'success'\") and run it against recent traces. Catch regressions before users do."
          )}
        </p>
        <button
          type="button"
          onClick={onAdd}
          className="mt-3 inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
        >
          <Plus size={12} />
          {t("agentDetail.evaluators.add", "Add evaluator")}
        </button>
      </div>
    </div>
  );
}

function EvaluatorRow({
  evaluator,
  onEdit,
  onDelete,
  deleting,
}: {
  evaluator: AgentEvaluator;
  onEdit: () => void;
  onDelete: () => void;
  deleting: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const cfg = parseEvaluatorConfig(evaluator);
  const isPro = PRO_EVALUATOR_KINDS.includes(evaluator.kind);

  return (
    <div className="rounded-lg border border-cs-border bg-cs-card overflow-hidden">
      <div className="flex items-center gap-3 px-3 py-2">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className="text-cs-muted hover:text-cs-text shrink-0"
        >
          {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </button>
        <span className="text-xs font-mono text-cs-accent shrink-0">{evaluator.name}</span>
        <span className="text-[10px] uppercase tracking-wide text-cs-muted shrink-0">
          {KIND_LABEL[evaluator.kind]}
        </span>
        {isPro && <Crown size={10} className="text-cs-accent shrink-0" />}
        <span className="flex-1 truncate text-xs text-cs-muted">{summary(cfg)}</span>
        {!evaluator.enabled && (
          <span className="text-[10px] uppercase tracking-wide text-cs-muted shrink-0">
            disabled
          </span>
        )}
        <button
          type="button"
          onClick={onEdit}
          className="text-xs text-cs-muted hover:text-cs-text shrink-0"
        >
          {t("agentDetail.variables.edit", "Edit")}
        </button>
        <button
          type="button"
          onClick={onDelete}
          disabled={deleting}
          className="text-cs-muted hover:text-cs-danger shrink-0"
        >
          {deleting ? <Loader2 size={12} className="animate-spin" /> : <Trash2 size={12} />}
        </button>
      </div>
      {open && (
        <pre className="border-t border-cs-border bg-cs-bg p-3 text-[11px] text-cs-muted font-mono whitespace-pre-wrap">
{JSON.stringify(JSON.parse(evaluator.configJson), null, 2)}
        </pre>
      )}
    </div>
  );
}

function summary(cfg: EvaluatorConfig): string {
  switch (cfg.kind) {
    case "contains":
    case "not-contains":
      return `"${cfg.needle}"${cfg.caseSensitive ? " (case-sensitive)" : ""}`;
    case "length-range":
      return `${cfg.min}–${cfg.max} chars`;
    case "tool-called":
      return cfg.tool;
    case "llm-judge":
      return cfg.prompt.slice(0, 60);
  }
}

function EvaluatorEditorForm({
  agentSlug,
  existing,
  onClose,
  onSaved,
}: {
  agentSlug: string;
  existing: AgentEvaluator | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const llmJudgeAllowed = useFeatureFlag("evaluators"); // already gated at the tab; safety check
  const initial: EvaluatorConfig = existing
    ? parseEvaluatorConfig(existing)
    : { kind: "contains", needle: "" };
  const [name, setName] = useState(existing?.name ?? "");
  const [enabled, setEnabled] = useState(existing?.enabled ?? true);
  const [kind, setKind] = useState<EvaluatorKind>(initial.kind);
  const [config, setConfig] = useState<EvaluatorConfig>(initial);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [proPrompt, setProPrompt] = useState(false);

  const save = async () => {
    if (!name.trim() || saving) return;
    setErr(null);
    setSaving(true);
    try {
      await saveAgentEvaluator({
        id: existing?.id,
        agentSlug,
        name: name.trim(),
        kind,
        configJson: evaluatorConfigToJson(config),
        enabled,
      });
      onSaved();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const pickKind = (k: EvaluatorKind) => {
    if (PRO_EVALUATOR_KINDS.includes(k) && !llmJudgeAllowed) {
      setProPrompt(true);
      return;
    }
    setKind(k);
    setConfig(defaultsFor(k));
  };

  return (
    <div className="rounded-lg border border-cs-accent/40 bg-cs-card p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h4 className="text-sm font-medium text-cs-text">
          {existing
            ? t("agentDetail.evaluators.editTitle", "Edit evaluator")
            : t("agentDetail.evaluators.newTitle", "New evaluator")}
        </h4>
        <label className="inline-flex items-center gap-1.5 text-xs text-cs-muted cursor-pointer">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
            className="h-3 w-3 accent-cs-accent"
          />
          {t("agentDetail.variables.enabled", "enabled")}
        </label>
      </div>

      <Field label={t("agentDetail.evaluators.name", "Name")} required>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="contains-success"
          className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          autoFocus
        />
      </Field>

      <Field label={t("agentDetail.evaluators.kind", "Kind")}>
        <div className="grid grid-cols-2 gap-1.5">
          {[...FREE_EVALUATOR_KINDS, ...PRO_EVALUATOR_KINDS].map((k) => {
            const isPro = PRO_EVALUATOR_KINDS.includes(k);
            const locked = isPro && !llmJudgeAllowed;
            const active = k === kind;
            return (
              <button
                key={k}
                type="button"
                onClick={() => pickKind(k)}
                className={cn(
                  "rounded-md border px-2.5 py-1.5 text-left text-xs transition",
                  active
                    ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                    : "border-cs-border bg-cs-bg-raised text-cs-text hover:border-cs-hover",
                  locked && !active && "opacity-60"
                )}
              >
                <div className="flex items-center gap-1.5">
                  <span className="font-medium">{KIND_LABEL[k]}</span>
                  {isPro && <Crown size={10} className="text-cs-accent" />}
                  {locked && <Lock size={10} className="text-cs-muted" />}
                </div>
                <p className="text-[10px] text-cs-muted mt-0.5">{KIND_HINT[k]}</p>
              </button>
            );
          })}
        </div>
      </Field>

      <KindConfigEditor kind={kind} config={config} onChange={setConfig} />

      {err && (
        <div className="flex items-start gap-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 p-2 text-xs text-cs-text">
          <AlertCircle size={12} className="text-cs-danger shrink-0 mt-0.5" />
          <span>{err}</span>
        </div>
      )}

      <div className="flex items-center justify-end gap-2 pt-1">
        <button
          type="button"
          onClick={onClose}
          className="text-xs text-cs-muted hover:text-cs-text"
        >
          {t("common.cancel", "Cancel")}
        </button>
        <button
          type="button"
          onClick={save}
          disabled={!name.trim() || saving}
          className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
        >
          {saving && <Loader2 size={12} className="animate-spin" />}
          {t("common.save", "Save")}
        </button>
      </div>

      <UpgradePrompt feature="evaluators" open={proPrompt} onClose={() => setProPrompt(false)} />
    </div>
  );
}

function defaultsFor(kind: EvaluatorKind): EvaluatorConfig {
  switch (kind) {
    case "contains": return { kind, needle: "", caseSensitive: false };
    case "not-contains": return { kind, needle: "", caseSensitive: false };
    case "length-range": return { kind, min: 50, max: 2000 };
    case "tool-called": return { kind, tool: "" };
    case "llm-judge": return { kind, prompt: "Did the agent satisfy the user's need? Reply with 0-1 score and a brief reason." };
  }
}

function KindConfigEditor({
  kind,
  config,
  onChange,
}: {
  kind: EvaluatorKind;
  config: EvaluatorConfig;
  onChange: (c: EvaluatorConfig) => void;
}) {
  const { t } = useTranslation();
  if ((kind === "contains" || kind === "not-contains") && (config.kind === "contains" || config.kind === "not-contains")) {
    return (
      <>
        <Field label={t("agentDetail.evaluators.fields.needle", "Substring")}>
          <input
            type="text"
            value={config.needle}
            onChange={(e) =>
              onChange({ kind, needle: e.target.value, caseSensitive: config.caseSensitive } as EvaluatorConfig)
            }
            placeholder="success"
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
        <label className="inline-flex items-center gap-1.5 text-xs text-cs-muted cursor-pointer">
          <input
            type="checkbox"
            checked={!!config.caseSensitive}
            onChange={(e) =>
              onChange({ kind, needle: config.needle, caseSensitive: e.target.checked } as EvaluatorConfig)
            }
            className="h-3 w-3 accent-cs-accent"
          />
          {t("agentDetail.evaluators.fields.caseSensitive", "case-sensitive")}
        </label>
      </>
    );
  }
  if (kind === "length-range" && config.kind === "length-range") {
    return (
      <div className="grid grid-cols-2 gap-2">
        <Field label={t("agentDetail.evaluators.fields.min", "Min chars")}>
          <input
            type="number"
            min={0}
            value={config.min}
            onChange={(e) => onChange({ ...config, min: parseInt(e.target.value, 10) || 0 })}
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
        <Field label={t("agentDetail.evaluators.fields.max", "Max chars")}>
          <input
            type="number"
            min={1}
            value={config.max}
            onChange={(e) => onChange({ ...config, max: parseInt(e.target.value, 10) || 1 })}
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
      </div>
    );
  }
  if (kind === "tool-called" && config.kind === "tool-called") {
    return (
      <Field label={t("agentDetail.evaluators.fields.tool", "Tool name")}>
        <input
          type="text"
          value={config.tool}
          onChange={(e) => onChange({ ...config, tool: e.target.value })}
          placeholder="gmail.search_messages"
          className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
        />
      </Field>
    );
  }
  if (kind === "llm-judge" && config.kind === "llm-judge") {
    return (
      <>
        <Field label={t("agentDetail.evaluators.fields.judgePrompt", "Judge prompt")}>
          <textarea
            rows={3}
            value={config.prompt}
            onChange={(e) => onChange({ ...config, prompt: e.target.value })}
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-xs text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
        <p className="text-[11px] text-cs-warning">
          {t(
            "agentDetail.evaluators.fields.llmJudgeStub",
            "LLM-as-judge runs server-side in Wave 4.5. Storing the config is fine; the judge runner activates when the cloud endpoint lands."
          )}
        </p>
      </>
    );
  }
  return null;
}

function RunResults({
  results,
  error,
  onClose,
}: {
  results: EvaluatedTrace[];
  error: string | null;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  if (error) {
    return (
      <div className="rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 flex items-start gap-2 text-xs text-cs-text">
        <XCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <div className="flex-1">{error}</div>
        <button type="button" onClick={onClose} className="text-cs-muted hover:text-cs-text">
          ×
        </button>
      </div>
    );
  }

  const total = results.reduce((sum, r) => sum + r.results.length, 0);
  const passed = results.reduce(
    (sum, r) => sum + r.results.filter((x) => x.verdict === "pass").length,
    0
  );

  return (
    <div className="rounded-lg border border-cs-accent/40 bg-cs-accent/5 p-4 space-y-3">
      <header className="flex items-center justify-between">
        <h4 className="text-xs font-medium text-cs-text">
          {t("agentDetail.evaluators.runResults", "Evaluator results")}{" "}
          <span className="text-cs-muted">
            ({passed} / {total} {t("agentDetail.evaluators.passed", "passed")})
          </span>
        </h4>
        <button type="button" onClick={onClose} className="text-cs-muted hover:text-cs-text text-xs">
          {t("common.close", "Close")}
        </button>
      </header>
      <div className="space-y-2 max-h-72 overflow-y-auto">
        {results.length === 0 ? (
          <p className="text-xs text-cs-muted">
            {t("agentDetail.evaluators.noTraces", "No traces to evaluate yet — run the agent first.")}
          </p>
        ) : (
          results.map((r, i) => (
            <div
              key={i}
              className="rounded-md border border-cs-border bg-cs-bg-raised p-2.5 space-y-1.5"
            >
              <div className="flex items-center gap-2 text-[11px] text-cs-muted">
                <span className="font-mono text-cs-text">{r.trace.slug ?? "?"}</span>
                {r.trace.ts && <span>· {new Date(r.trace.ts).toLocaleString()}</span>}
                {r.trace.durationMs !== undefined && <span>· {r.trace.durationMs}ms</span>}
              </div>
              <div className="space-y-1">
                {r.results.map((res, j) => (
                  <div key={j} className="flex items-start gap-2 text-xs">
                    {res.verdict === "pass" ? (
                      <CheckCircle2 size={11} className="text-cs-accent shrink-0 mt-0.5" />
                    ) : res.verdict === "fail" ? (
                      <XCircle size={11} className="text-cs-danger shrink-0 mt-0.5" />
                    ) : (
                      <span className="text-cs-muted">·</span>
                    )}
                    <span className="text-cs-muted">{res.reason}</span>
                  </div>
                ))}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function Field({
  label,
  required,
  children,
}: {
  label: string;
  required?: boolean;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <span className="block text-[10px] font-medium text-cs-muted uppercase tracking-wide mb-1">
        {label}
        {required && <span className="text-cs-danger ml-0.5">*</span>}
      </span>
      {children}
    </label>
  );
}
