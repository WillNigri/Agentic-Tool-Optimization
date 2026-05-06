import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  GitPullRequest,
  Feather,
  Binary,
  Terminal,
  Headphones,
  Search,
} from "lucide-react";
import { AGENT_TEMPLATES, type AgentTemplate } from "@/lib/agentTemplates";
import { cn } from "@/lib/utils";

// v1.4.0 Polish-T1 — Templates path. Shows a grid of pre-filled agent
// starters; clicking one hands a draft up to the wizard which switches into
// the Quick form with the template's fields applied. The user can still edit
// before saving.

interface Props {
  onPick: (template: AgentTemplate) => void;
}

const ICONS: Record<AgentTemplate["icon"], React.ComponentType<{ size?: number; className?: string }>> = {
  "git-pull-request": GitPullRequest,
  feather: Feather,
  binary: Binary,
  terminal: Terminal,
  headphones: Headphones,
};

const CATEGORY_LABELS: Record<AgentTemplate["category"], string> = {
  engineering: "Engineering",
  writing: "Writing",
  data: "Data",
  ops: "Ops",
  support: "Support",
};

export default function TemplatesPath({ onPick }: Props) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return AGENT_TEMPLATES;
    return AGENT_TEMPLATES.filter(
      (tpl) =>
        tpl.displayName.toLowerCase().includes(q) ||
        tpl.description.toLowerCase().includes(q) ||
        tpl.category.toLowerCase().includes(q)
    );
  }, [search]);

  return (
    <div className="space-y-4">
      <div>
        <h3 className="text-sm font-medium text-cs-text">
          {t("createAgent.templates.title", "Start from a template")}
        </h3>
        <p className="mt-1 text-xs text-cs-muted">
          {t(
            "createAgent.templates.subtitle",
            "Pick a starter, customize the prompt and tools, then save. Faster than starting from blank."
          )}
        </p>
      </div>

      <div className="relative">
        <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted" />
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t("createAgent.templates.searchPlaceholder", "Search templates…")}
          className="w-full rounded-lg border border-cs-border bg-cs-bg pl-9 pr-3 py-2 text-sm text-cs-text placeholder:text-cs-muted focus:border-cs-accent focus:outline-none"
        />
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        {filtered.map((tpl) => {
          const Icon = ICONS[tpl.icon];
          return (
            <button
              key={tpl.id}
              type="button"
              onClick={() => onPick(tpl)}
              className={cn(
                "group flex flex-col gap-2 rounded-lg border border-cs-border bg-cs-bg-raised p-4 text-left",
                "transition hover:border-cs-accent/60 hover:bg-cs-bg-raised/80 focus:border-cs-accent focus:outline-none"
              )}
            >
              <div className="flex items-start gap-3">
                <div className="flex h-9 w-9 items-center justify-center rounded-md bg-cs-accent/10 text-cs-accent shrink-0">
                  <Icon size={16} />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center justify-between gap-2">
                    <h4 className="text-sm font-medium text-cs-text truncate">
                      {tpl.displayName}
                    </h4>
                    <span className="text-[10px] uppercase tracking-wide text-cs-muted shrink-0">
                      {CATEGORY_LABELS[tpl.category]}
                    </span>
                  </div>
                  <p className="mt-1 text-xs text-cs-muted line-clamp-2">
                    {tpl.description}
                  </p>
                </div>
              </div>
              {tpl.recommendedMcps.length > 0 && (
                <div className="flex flex-wrap gap-1 pt-1">
                  {tpl.recommendedMcps.map((mcp) => (
                    <span
                      key={mcp}
                      className="rounded-md bg-cs-border/40 px-1.5 py-0.5 text-[10px] font-mono text-cs-muted"
                    >
                      {mcp}
                    </span>
                  ))}
                </div>
              )}
            </button>
          );
        })}
      </div>

      {filtered.length === 0 && (
        <p className="rounded-lg border border-cs-border bg-cs-bg-raised p-4 text-center text-xs text-cs-muted">
          {t("createAgent.templates.noResults", "No templates match your search.")}
        </p>
      )}
    </div>
  );
}
