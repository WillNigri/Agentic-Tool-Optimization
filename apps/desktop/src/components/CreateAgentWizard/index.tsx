import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles, FormInput, LayoutGrid, X } from "lucide-react";
import GuidedPath from "./GuidedPath";
import QuickPath from "./QuickPath";
import TemplatesPath from "./TemplatesPath";
import { AGENT_TEMPLATES, type AgentTemplate } from "@/lib/agentTemplates";
import type { QuickDraft } from "@/lib/agentDraft";
import { useUiStore } from "@/stores/useUiStore";

// v1.3.0 — Create Agent wizard. T3 in docs/V1.3.0-IMPLEMENTATION.md.
// v1.4.0 Polish-T1 — Adds a Templates path (5 starters). Picking a template
// hands a pre-filled QuickDraft to QuickPath so the user edits a sensible
// default instead of a blank form.

export type WizardPath = "guided" | "quick" | "templates";

interface CreateAgentWizardProps {
  open: boolean;
  initialPath?: WizardPath;
  onClose: () => void;
  onCreated?: (agentId: string) => void;
}

function templateToDraft(tpl: AgentTemplate): QuickDraft {
  return {
    name: tpl.displayName,
    runtime: tpl.runtime,
    model: tpl.model,
    description: tpl.description,
    systemPrompt: tpl.systemPrompt,
    projectId: null,
    skills: [],
    // Pre-select recommended MCPs only if the user already has them configured;
    // for this draft we just pre-fill the slugs and let QuickPath's MultiSelect
    // intersect with what's actually installed.
    mcps: tpl.recommendedMcps,
    contextFiles: [],
  };
}

export default function CreateAgentWizard({
  open,
  initialPath = "guided",
  onClose,
  onCreated,
}: CreateAgentWizardProps) {
  const { t } = useTranslation();
  const [path, setPath] = useState<WizardPath>(initialPath);
  const [seedDraft, setSeedDraft] = useState<QuickDraft | null>(null);
  const [seedScaffold, setSeedScaffold] = useState<AgentTemplate["dynamicScaffold"] | undefined>(undefined);
  const pendingTemplateId = useUiStore((s) => s.createAgentTemplateId);
  const consumeTemplateId = useUiStore((s) => s.consumeTemplateId);

  // Re-sync the path when the store reopens us with a different one.
  useEffect(() => {
    if (open) setPath(initialPath);
  }, [open, initialPath]);

  // If a coordinator (e.g. the demo runner) pre-picked a template, auto-pick
  // it after the wizard sees the id. Watching the id directly means a
  // late-arriving id (after open) still triggers. Consumes after applying.
  useEffect(() => {
    if (!open || !pendingTemplateId) return;
    const tpl = AGENT_TEMPLATES.find((tt) => tt.id === pendingTemplateId);
    if (tpl) {
      setSeedDraft(templateToDraft(tpl));
      setSeedScaffold(tpl.dynamicScaffold);
      setPath("quick");
    }
    consumeTemplateId();
  }, [open, pendingTemplateId, consumeTemplateId]);

  if (!open) return null;

  const handlePickTemplate = (tpl: AgentTemplate) => {
    setSeedDraft(templateToDraft(tpl));
    setSeedScaffold(tpl.dynamicScaffold);
    setPath("quick");
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-3xl max-h-[90vh] flex flex-col rounded-2xl border border-cs-border bg-cs-card shadow-2xl">
        {/* Header */}
        <header className="flex items-start justify-between gap-4 p-5 border-b border-cs-border">
          <div className="min-w-0 flex-1">
            <h2 className="text-lg font-semibold text-cs-text">
              {t("createAgent.title", "Create Agent")}
            </h2>
            <p className="mt-0.5 text-[11px] text-cs-muted leading-relaxed">
              {t(
                "createAgent.dynamicHint",
                "Build agents whose prompts adapt — variables resolve from files, env vars, databases, or other LLMs at fire time. Static system prompts are the floor, not the ceiling."
              )}
            </p>
          </div>
          <button
            type="button"
            aria-label={t("common.close", "Close")}
            onClick={onClose}
            className="text-cs-muted hover:text-cs-text shrink-0"
          >
            <X size={18} />
          </button>
        </header>

        {/* Path toggle */}
        <div className="px-5 pt-4 flex items-center gap-2">
          <PathPill
            demoId="wizard-path-guided"
            active={path === "guided"}
            onClick={() => {
              setSeedDraft(null);
              setPath("guided");
            }}
            icon={<Sparkles size={14} />}
            label={t("createAgent.pathGuided", "Guided (chat)")}
          />
          <PathPill
            demoId="wizard-path-quick"
            active={path === "quick"}
            onClick={() => {
              setSeedDraft(null);
              setPath("quick");
            }}
            icon={<FormInput size={14} />}
            label={t("createAgent.pathQuick", "Quick (form)")}
          />
          <PathPill
            demoId="wizard-path-templates"
            active={path === "templates"}
            onClick={() => {
              setSeedDraft(null);
              setPath("templates");
            }}
            icon={<LayoutGrid size={14} />}
            label={t("createAgent.pathTemplates", "Templates")}
          />
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto p-5">
          {path === "guided" && <GuidedPath onCreated={onCreated} onCancel={onClose} />}
          {path === "quick" && (
            <QuickPath
              key={seedDraft ? `seed-${seedDraft.name}` : "draft"}
              onCreated={onCreated}
              onCancel={onClose}
              initialDraft={seedDraft ?? undefined}
              initialScaffold={seedScaffold}
            />
          )}
          {path === "templates" && <TemplatesPath onPick={handlePickTemplate} />}
        </div>
      </div>
    </div>
  );
}

function PathPill({
  active,
  onClick,
  icon,
  label,
  demoId,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
  demoId?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-demo-id={demoId}
      className={`inline-flex items-center gap-2 rounded-full px-3 py-1.5 text-xs font-medium transition ${
        active
          ? "bg-cs-accent text-cs-bg"
          : "bg-cs-bg-raised text-cs-muted border border-cs-border hover:text-cs-text"
      }`}
    >
      {icon}
      {label}
    </button>
  );
}
