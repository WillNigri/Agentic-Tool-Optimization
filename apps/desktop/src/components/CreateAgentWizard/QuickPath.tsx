import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, AlertCircle, CheckCircle2, ChevronDown, ChevronRight, Search } from "lucide-react";
import { createAgent, type AgentRuntime } from "@/lib/agents";
import { listProjects, getSkills, getMcpServers } from "@/lib/api";
import {
  loadQuickDraft,
  saveQuickDraft,
  clearQuickDraft,
  type QuickDraft,
} from "@/lib/agentDraft";
import { cn } from "@/lib/utils";

// Quick (form) path — wired to Rust create_agent. T3.b adds:
//   - Draft persistence via localStorage (auto-save on change, cleared on success)
//   - Project picker (existing listProjects)
//   - Skills multi-select (runtime-filtered, with search)
//   - MCPs multi-select

interface Props {
  onCreated?: (agentId: string) => void;
  onCancel: () => void;
  /** Pre-fills the form on mount; overrides the persisted draft. Used by the Templates path. */
  initialDraft?: QuickDraft;
}

const RUNTIMES: { id: AgentRuntime; label: string; modelHint: string }[] = [
  { id: "claude",   label: "Claude Code",                   modelHint: "claude-sonnet-4-6" },
  { id: "codex",    label: "Codex / OpenAI Agents SDK",     modelHint: "gpt-4.1" },
  { id: "gemini",   label: "Gemini CLI / ADK",              modelHint: "gemini-2.0-flash-exp" },
  { id: "openclaw", label: "OpenClaw",                      modelHint: "" },
  { id: "hermes",   label: "Hermes",                        modelHint: "" },
];

const DEFAULT_DRAFT: QuickDraft = {
  name: "",
  runtime: "claude",
  model: "",
  description: "",
  systemPrompt: "",
  projectId: null,
  skills: [],
  mcps: [],
};

export default function QuickPath({ onCreated, onCancel, initialDraft }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [createdAgentName, setCreatedAgentName] = useState<string | null>(null);

  // Draft (auto-saved). initialDraft (from a template pick) wins over the
  // persisted draft so the user lands on the pre-filled form.
  const [draft, setDraft] = useState<QuickDraft>(
    () => initialDraft ?? loadQuickDraft() ?? DEFAULT_DRAFT
  );

  useEffect(() => {
    saveQuickDraft(draft);
  }, [draft]);

  const update = <K extends keyof QuickDraft>(key: K, value: QuickDraft[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const runtimeMeta = RUNTIMES.find((r) => r.id === draft.runtime);

  const { data: projects = [] } = useQuery({
    queryKey: ["projects"],
    queryFn: listProjects,
    staleTime: 30_000,
  });

  const { data: allSkills = [] } = useQuery({
    queryKey: ["all-skills"],
    queryFn: getSkills,
    staleTime: 30_000,
  });

  const { data: allMcps = [] } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: getMcpServers,
    staleTime: 30_000,
  });

  const runtimeSkills = allSkills.filter((s) => s.runtime === draft.runtime || draft.runtime === "openclaw");

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!draft.name.trim() || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      const agent = await createAgent({
        displayName: draft.name.trim(),
        runtime: draft.runtime,
        description: draft.description.trim() || undefined,
        model: draft.model.trim() || undefined,
        systemPrompt: draft.systemPrompt.trim() || undefined,
        projectId: draft.projectId ?? undefined,
        skills: draft.skills.length > 0 ? draft.skills : undefined,
        mcps: draft.mcps.length > 0 ? draft.mcps : undefined,
      });
      setCreatedAgentName(agent.displayName);
      clearQuickDraft();
      void queryClient.invalidateQueries({ queryKey: ["agents"] });
      void queryClient.invalidateQueries({ queryKey: ["recent-agents"] });
      onCreated?.(agent.id);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  const reset = () => {
    setDraft(DEFAULT_DRAFT);
    clearQuickDraft();
  };

  if (createdAgentName) {
    return (
      <div className="rounded-lg border border-cs-accent/40 bg-cs-accent/10 p-6 flex items-start gap-3">
        <CheckCircle2 size={20} className="text-cs-accent shrink-0" />
        <div className="flex-1">
          <h3 className="text-sm font-medium text-cs-text">
            {t("createAgent.quick.successTitle", "Agent created")}
          </h3>
          <p className="mt-1 text-xs text-cs-muted">
            {t("createAgent.quick.successBody", "{{name}} is ready. Open it from the Agents list.", {
              name: createdAgentName,
            })}
          </p>
          <div className="mt-4 flex items-center gap-2">
            <button
              type="button"
              onClick={onCancel}
              className="rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
            >
              {t("common.close", "Close")}
            </button>
            <button
              type="button"
              onClick={() => {
                setCreatedAgentName(null);
                reset();
              }}
              className="rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
            >
              {t("createAgent.quick.createAnother", "Create another")}
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-5">
      <Field label={t("createAgent.quick.name", "Name")} required>
        <input
          type="text"
          value={draft.name}
          onChange={(e) => update("name", e.target.value)}
          placeholder="pr-reviewer"
          className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
          autoFocus
        />
      </Field>

      <Field label={t("createAgent.quick.runtime", "Runtime")} required>
        <select
          value={draft.runtime}
          onChange={(e) => update("runtime", e.target.value as AgentRuntime)}
          className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
        >
          {RUNTIMES.map((r) => (
            <option key={r.id} value={r.id}>
              {r.label}
            </option>
          ))}
        </select>
      </Field>

      <Field label={t("createAgent.quick.model", "Model")}>
        <input
          type="text"
          value={draft.model}
          onChange={(e) => update("model", e.target.value)}
          placeholder={runtimeMeta?.modelHint || ""}
          className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
        />
      </Field>

      <Field label={t("createAgent.quick.project", "Project")}>
        <select
          value={draft.projectId ?? ""}
          onChange={(e) => update("projectId", e.target.value || null)}
          className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
        >
          <option value="">{t("createAgent.quick.noProject", "(global / no project)")}</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </Field>

      <Field label={t("createAgent.quick.description", "Description")}>
        <input
          type="text"
          value={draft.description}
          onChange={(e) => update("description", e.target.value)}
          placeholder={t(
            "createAgent.quick.descriptionPlaceholder",
            "One-line description of what this agent does"
          )}
          className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
        />
      </Field>

      <Field
        label={t("createAgent.quick.systemPrompt", "System prompt")}
        hint={t("createAgent.quick.systemPromptHint", "Optional. The agent's instructions.")}
      >
        <textarea
          value={draft.systemPrompt}
          onChange={(e) => update("systemPrompt", e.target.value)}
          rows={5}
          placeholder={t(
            "createAgent.quick.systemPromptPlaceholder",
            "You are a code reviewer focused on security…"
          )}
          className="w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
        />
      </Field>

      <MultiSelect
        label={t("createAgent.quick.skills", "Skills")}
        items={runtimeSkills.map((s) => ({ id: s.id, label: s.name, hint: s.description }))}
        selected={draft.skills}
        onToggle={(id) =>
          update(
            "skills",
            draft.skills.includes(id)
              ? draft.skills.filter((s) => s !== id)
              : [...draft.skills, id]
          )
        }
        emptyHint={t("createAgent.quick.noSkillsAvailable", "No skills installed for this runtime.")}
      />

      <MultiSelect
        label={t("createAgent.quick.mcps", "MCPs")}
        items={allMcps.map((m) => ({ id: m.name, label: m.name, hint: m.transport }))}
        selected={draft.mcps}
        onToggle={(id) =>
          update(
            "mcps",
            draft.mcps.includes(id) ? draft.mcps.filter((m) => m !== id) : [...draft.mcps, id]
          )
        }
        emptyHint={t("createAgent.quick.noMcpsAvailable", "No MCP servers configured.")}
      />

      {error && (
        <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3">
          <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
          <span className="text-xs text-cs-text">{error}</span>
        </div>
      )}

      <div className="flex items-center justify-between gap-3 pt-2">
        <button
          type="button"
          onClick={reset}
          className="text-xs text-cs-muted hover:text-cs-text"
        >
          {t("createAgent.quick.clearDraft", "Clear draft")}
        </button>
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-lg border border-cs-border bg-cs-bg-raised px-4 py-2 text-sm text-cs-muted hover:text-cs-text"
          >
            {t("common.cancel", "Cancel")}
          </button>
          <button
            type="submit"
            disabled={!draft.name.trim() || submitting}
            className="inline-flex items-center gap-2 rounded-lg bg-cs-accent px-4 py-2 text-sm font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
          >
            {submitting && <Loader2 size={14} className="animate-spin" />}
            {t("createAgent.quick.create", "Create agent")}
          </button>
        </div>
      </div>
    </form>
  );
}

function Field({
  label,
  required,
  hint,
  children,
}: {
  label: string;
  required?: boolean;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <div className="flex items-center justify-between mb-1">
        <span className="text-xs font-medium text-cs-muted uppercase tracking-wide">
          {label}
          {required && <span className="text-cs-danger ml-0.5">*</span>}
        </span>
        {hint && <span className="text-xs text-cs-muted">{hint}</span>}
      </div>
      {children}
    </label>
  );
}

function MultiSelect({
  label,
  items,
  selected,
  onToggle,
  emptyHint,
}: {
  label: string;
  items: { id: string; label: string; hint?: string }[];
  selected: string[];
  onToggle: (id: string) => void;
  emptyHint: string;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(selected.length > 0);
  const [search, setSearch] = useState("");
  const filtered = search.trim()
    ? items.filter((it) => it.label.toLowerCase().includes(search.toLowerCase()))
    : items;

  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center justify-between text-xs font-medium text-cs-muted uppercase tracking-wide mb-1 hover:text-cs-text"
      >
        <span>
          {label} {selected.length > 0 && <span className="text-cs-accent">({selected.length})</span>}
        </span>
        {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
      </button>
      {open && (
        <div className="rounded-lg border border-cs-border bg-cs-bg-raised">
          {items.length === 0 ? (
            <p className="px-3 py-3 text-xs text-cs-muted">{emptyHint}</p>
          ) : (
            <>
              <div className="relative border-b border-cs-border">
                <Search size={12} className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted" />
                <input
                  type="text"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder={t("createAgent.quick.filterPlaceholder", "Filter…")}
                  className="w-full bg-transparent pl-8 pr-3 py-2 text-xs text-cs-text placeholder:text-cs-muted focus:outline-none"
                />
              </div>
              <div className="max-h-40 overflow-y-auto">
                {filtered.length === 0 ? (
                  <p className="px-3 py-3 text-xs text-cs-muted">{emptyHint}</p>
                ) : (
                  filtered.map((it) => {
                    const isSelected = selected.includes(it.id);
                    return (
                      <button
                        key={it.id}
                        type="button"
                        onClick={() => onToggle(it.id)}
                        className={cn(
                          "w-full flex items-center gap-2 px-3 py-1.5 text-left text-xs transition",
                          isSelected
                            ? "bg-cs-accent/10 text-cs-accent"
                            : "text-cs-text hover:bg-cs-border/40"
                        )}
                      >
                        <span
                          className={cn(
                            "h-3 w-3 rounded border shrink-0",
                            isSelected ? "border-cs-accent bg-cs-accent" : "border-cs-border"
                          )}
                        />
                        <span className="truncate flex-1">{it.label}</span>
                        {it.hint && (
                          <span className="text-[10px] text-cs-muted truncate max-w-[40%]">{it.hint}</span>
                        )}
                      </button>
                    );
                  })
                )}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
