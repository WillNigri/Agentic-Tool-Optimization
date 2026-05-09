import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Loader2, AlertCircle, Brain, Save, RotateCcw } from "lucide-react";
import {
  parseMemoryPolicy,
  updateAgentMemoryPolicy,
  DEFAULT_MEMORY_POLICY,
  type Agent,
  type MemoryPolicy,
} from "@/lib/agents";
import { useFeatureFlag } from "@/lib/tier";
import TierGate from "@/components/Tier/TierGate";
import { useUiStore } from "@/stores/useUiStore";

// v1.4.0 F3 — Memory / summarizer policy tab.
//
// Free: fixed defaults, can view but cannot edit.
// Pro: tunable threshold + keep-last-K + summarizer model.
//
// Note (honestly stated in the UI): summarization only fires for
// ATO-mediated dispatches that pass conversation history. Quick Test today is
// single-shot, so the policy is stored but not exercised. Multi-turn
// summarization activates when groups (Wave 3) or cron-driven sessions land.

interface Props {
  agent: Agent;
}

export default function MemoryTab({ agent }: Props) {
  return (
    <TierGate feature="summarizer.tunable" mode="overlay">
      <MemoryEditor agent={agent} />
    </TierGate>
  );
}

function MemoryEditor({ agent }: { agent: Agent }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const tunable = useFeatureFlag("summarizer.tunable");
  const openCreateAgent = useUiStore((s) => s.openCreateAgent);

  const [policy, setPolicy] = useState<MemoryPolicy>(parseMemoryPolicy(agent));
  const [error, setError] = useState<string | null>(null);

  const initial = parseMemoryPolicy(agent);
  const dirty =
    policy.summarizeAfter !== initial.summarizeAfter ||
    policy.keepLastK !== initial.keepLastK ||
    policy.summarizerModel !== initial.summarizerModel;

  const saveMutation = useMutation({
    mutationFn: () => updateAgentMemoryPolicy(agent.id, policy),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["agents"] });
      void queryClient.invalidateQueries({ queryKey: ["recent-agents"] });
      setError(null);
    },
    onError: (e) => setError(e instanceof Error ? e.message : String(e)),
  });

  const reset = () => {
    setPolicy(DEFAULT_MEMORY_POLICY);
    setError(null);
  };

  return (
    <div className="space-y-5">
      <header>
        <div className="flex items-center gap-2">
          <Brain size={16} className="text-cs-accent" />
          <h3 className="text-sm font-medium text-cs-text">
            {t("agentDetail.memory.title", "Conversation memory")}
          </h3>
        </div>
        <p className="mt-1 text-xs text-cs-muted">
          {t(
            "agentDetail.memory.subtitle",
            "Long sessions accumulate noise — quality drops. Summarize once you cross a threshold; keep the most recent turns verbatim. Free uses fixed defaults; Pro lets you tune."
          )}
        </p>
        {/* v1.5.5 — Discoverability hint. Memory tab is configured-by-
            default so it never goes truly empty; nudge users at the
            Production-grade template instead of leaving them to figure
            out what "good" looks like from scratch. */}
        <button
          type="button"
          onClick={() => openCreateAgent("templates", "production-grade")}
          className="mt-2 inline-flex items-center gap-1.5 text-[11px] text-cs-muted hover:text-cs-accent"
        >
          {t(
            "agentDetail.memory.tryTemplate",
            "See a sensible memory policy in the Production template →",
          )}
        </button>
      </header>

      <Field
        label={t("agentDetail.memory.summarizeAfter", "Summarize after N messages")}
        hint={t("agentDetail.memory.summarizeAfterHint", "Trigger summarization when message count exceeds this")}
      >
        <input
          type="number"
          min={5}
          max={200}
          value={policy.summarizeAfter}
          onChange={(e) =>
            setPolicy((p) => ({ ...p, summarizeAfter: Math.max(5, parseInt(e.target.value, 10) || 30) }))
          }
          disabled={!tunable}
          className="w-32 rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none disabled:opacity-50"
        />
      </Field>

      <Field
        label={t("agentDetail.memory.keepLastK", "Keep last K verbatim")}
        hint={t("agentDetail.memory.keepLastKHint", "Recent turns to keep unsummarized for context continuity")}
      >
        <input
          type="number"
          min={1}
          max={50}
          value={policy.keepLastK}
          onChange={(e) =>
            setPolicy((p) => ({ ...p, keepLastK: Math.max(1, parseInt(e.target.value, 10) || 5) }))
          }
          disabled={!tunable}
          className="w-32 rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none disabled:opacity-50"
        />
      </Field>

      <Field
        label={t("agentDetail.memory.summarizerModel", "Summarizer model")}
        hint={t(
          "agentDetail.memory.summarizerModelHint",
          "A cheap fast model is best for summarization. Empty = runtime default."
        )}
      >
        <input
          type="text"
          placeholder="claude-haiku-4-5"
          value={policy.summarizerModel}
          onChange={(e) => setPolicy((p) => ({ ...p, summarizerModel: e.target.value }))}
          disabled={!tunable}
          className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none disabled:opacity-50"
        />
      </Field>

      <div className="rounded-md border border-cs-warning/40 bg-cs-warning/10 p-3 text-xs text-cs-text flex items-start gap-2">
        <AlertCircle size={12} className="text-cs-warning shrink-0 mt-0.5" />
        <span>
          {t(
            "agentDetail.memory.honestyNote",
            "Summarization fires for ATO-mediated dispatches that pass conversation history. Today's Quick Test is single-shot, so the policy is stored but not exercised. Multi-turn summarization activates with groups (Wave 3) and scheduled sessions."
          )}
        </span>
      </div>

      {error && (
        <div className="flex items-start gap-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
          <AlertCircle size={12} className="text-cs-danger shrink-0 mt-0.5" />
          <span>{error}</span>
        </div>
      )}

      <div className="flex items-center justify-end gap-2 pt-2">
        <button
          type="button"
          onClick={reset}
          disabled={!tunable || saveMutation.isPending}
          className="inline-flex items-center gap-1 text-xs text-cs-muted hover:text-cs-text disabled:opacity-50"
        >
          <RotateCcw size={11} />
          {t("agentDetail.memory.resetDefaults", "Reset defaults")}
        </button>
        <button
          type="button"
          onClick={() => saveMutation.mutate()}
          disabled={!tunable || !dirty || saveMutation.isPending}
          className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
        >
          {saveMutation.isPending ? <Loader2 size={12} className="animate-spin" /> : <Save size={12} />}
          {t("common.save", "Save")}
        </button>
      </div>
    </div>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <div className="flex items-center justify-between mb-1">
        <span className="text-xs font-medium text-cs-text">{label}</span>
        {hint && <span className="text-[10px] text-cs-muted">{hint}</span>}
      </div>
      {children}
    </label>
  );
}
