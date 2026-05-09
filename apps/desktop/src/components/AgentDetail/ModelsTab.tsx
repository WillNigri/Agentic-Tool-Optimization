import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Loader2, AlertCircle, Cpu, Save, Crown } from "lucide-react";
import {
  parseRoleModels,
  updateAgentRoleModels,
  type Agent,
  type RoleModels,
} from "@/lib/agents";
import { useFeatureFlag } from "@/lib/tier";
import UpgradePrompt from "@/components/Tier/UpgradePrompt";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/useUiStore";

// v1.4.0 F5 — Per-task model selection.
//
// Free: only the `response` slot is editable; everything else inherits.
// Pro: all four slots (router / summarizer / response / evaluator).
// The cheap-fast/expensive split is the article's biggest cost win.

interface Props {
  agent: Agent;
}

const ROLE_HINTS: Record<keyof RoleModels, string> = {
  router: "Used for routing/classification (groups). Cheap + fast wins here.",
  summarizer: "Used for conversation summarization. Cheap + fast also good.",
  response: "Used for the final user-visible response. The advanced model.",
  evaluator: "Used for LLM-as-judge evaluators. Intermediate is enough.",
};

const MODEL_PRESETS: Record<string, string[]> = {
  claude: [
    "claude-sonnet-4-6",
    "claude-opus-4-7",
    "claude-haiku-4-5",
  ],
  codex: ["gpt-4.1", "gpt-4o", "gpt-4o-mini", "o1-mini"],
  gemini: ["gemini-2.0-flash-exp", "gemini-2.0-pro-exp", "gemini-1.5-flash"],
  openclaw: ["custom"],
  hermes: ["hermes-3"],
};

export default function ModelsTab({ agent }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const advancedAllowed = useFeatureFlag("role-models");
  const openCreateAgent = useUiStore((s) => s.openCreateAgent);

  const initial = parseRoleModels(agent);
  const [models, setModels] = useState<RoleModels>({
    router: initial.router ?? "",
    summarizer: initial.summarizer ?? "",
    response: initial.response ?? agent.model ?? "",
    evaluator: initial.evaluator ?? "",
  });
  const [error, setError] = useState<string | null>(null);
  const [proPrompt, setProPrompt] = useState(false);

  const presets = MODEL_PRESETS[agent.runtime] ?? [];
  const initialResponse = initial.response ?? agent.model ?? "";

  const dirty =
    (models.router ?? "") !== (initial.router ?? "") ||
    (models.summarizer ?? "") !== (initial.summarizer ?? "") ||
    (models.response ?? "") !== initialResponse ||
    (models.evaluator ?? "") !== (initial.evaluator ?? "");

  const saveMutation = useMutation({
    mutationFn: () => {
      // Strip empties so the JSON stays compact.
      const out: RoleModels = {};
      if (models.router?.trim()) out.router = models.router.trim();
      if (models.summarizer?.trim()) out.summarizer = models.summarizer.trim();
      if (models.response?.trim()) out.response = models.response.trim();
      if (models.evaluator?.trim()) out.evaluator = models.evaluator.trim();
      return updateAgentRoleModels(agent.id, out);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["agents"] });
      setError(null);
    },
    onError: (e) => setError(e instanceof Error ? e.message : String(e)),
  });

  return (
    <div className="space-y-5">
      <header>
        <div className="flex items-center gap-2">
          <Cpu size={16} className="text-cs-accent" />
          <h3 className="text-sm font-medium text-cs-text">
            {t("agentDetail.models.title", "Per-task model selection")}
          </h3>
        </div>
        <p className="mt-1 text-xs text-cs-muted">
          {t(
            "agentDetail.models.subtitle",
            "Use cheap fast models for routing/summarization; advanced models for the final response. Free agents use one model for everything."
          )}
        </p>
        {/* v1.5.5 — Discoverability hint. Per-task model selection
            is one of the easiest cost wins but invisible if you don't
            know it exists. Production template uses it; nudge the user
            to see it wired up before they start guessing values. */}
        <button
          type="button"
          onClick={() => openCreateAgent("templates", "production-grade")}
          className="mt-2 inline-flex items-center gap-1.5 text-[11px] text-cs-muted hover:text-cs-accent"
        >
          {t(
            "agentDetail.models.tryTemplate",
            "See per-task model selection in the Production template →",
          )}
        </button>
      </header>

      <ModelRow
        role="response"
        label={t("agentDetail.models.role.response", "Response")}
        value={models.response ?? ""}
        onChange={(v) => setModels((m) => ({ ...m, response: v }))}
        presets={presets}
        locked={false}
      />

      {(["router", "summarizer", "evaluator"] as const).map((role) => (
        <ModelRow
          key={role}
          role={role}
          label={
            role === "router"
              ? t("agentDetail.models.role.router", "Router")
              : role === "summarizer"
              ? t("agentDetail.models.role.summarizer", "Summarizer")
              : t("agentDetail.models.role.evaluator", "Evaluator")
          }
          value={models[role] ?? ""}
          onChange={(v) => setModels((m) => ({ ...m, [role]: v }))}
          presets={presets}
          locked={!advancedAllowed}
          onLockedClick={() => setProPrompt(true)}
        />
      ))}

      {error && (
        <div className="flex items-start gap-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
          <AlertCircle size={12} className="text-cs-danger shrink-0 mt-0.5" />
          <span>{error}</span>
        </div>
      )}

      <div className="flex items-center justify-end pt-2">
        <button
          type="button"
          onClick={() => saveMutation.mutate()}
          disabled={!dirty || saveMutation.isPending}
          className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
        >
          {saveMutation.isPending ? <Loader2 size={12} className="animate-spin" /> : <Save size={12} />}
          {t("common.save", "Save")}
        </button>
      </div>

      <UpgradePrompt feature="role-models" open={proPrompt} onClose={() => setProPrompt(false)} />
    </div>
  );
}

function ModelRow({
  role,
  label,
  value,
  onChange,
  presets,
  locked,
  onLockedClick,
}: {
  role: keyof RoleModels;
  label: string;
  value: string;
  onChange: (v: string) => void;
  presets: string[];
  locked: boolean;
  onLockedClick?: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div
      className={cn(
        "rounded-lg border p-3 space-y-2",
        locked
          ? "border-cs-border bg-cs-bg-raised/40 opacity-70 cursor-pointer"
          : "border-cs-border bg-cs-bg-raised"
      )}
      onClick={() => locked && onLockedClick?.()}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-xs font-medium text-cs-text">{label}</span>
          {locked && <Crown size={11} className="text-cs-accent" />}
        </div>
        <span className="text-[10px] text-cs-muted">{ROLE_HINTS[role]}</span>
      </div>

      <div className="flex flex-col sm:flex-row sm:items-center gap-2">
        <input
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={t("agentDetail.models.placeholder", "Model id (empty = inherits from response)")}
          disabled={locked}
          onClick={(e) => locked && e.preventDefault()}
          className="flex-1 rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none disabled:opacity-50"
        />
      </div>

      {!locked && presets.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {presets.map((m) => (
            <button
              key={m}
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onChange(m);
              }}
              className={cn(
                "rounded-md border px-2 py-0.5 text-[10px] font-mono transition",
                value === m
                  ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                  : "border-cs-border bg-cs-bg text-cs-muted hover:border-cs-hover"
              )}
            >
              {m}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
