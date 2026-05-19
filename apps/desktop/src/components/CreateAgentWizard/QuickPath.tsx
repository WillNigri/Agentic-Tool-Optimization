import { useState, useEffect, useRef } from "react";
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
import { saveAgentHook, hookConfigToJson } from "@/lib/agentHooks";
import { saveAgentVariable } from "@/lib/agentVariables";
import { updateAgentMemoryPolicy } from "@/lib/agents";
import { useDemoStore } from "@/stores/useDemoStore";
import { cn } from "@/lib/utils";
import { FileText, Plus, Trash2 } from "lucide-react";
import AuthRequirements from "./AuthRequirements";
import { useUiStore } from "@/stores/useUiStore";

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
  /** Dynamic-prompt scaffold from a template — variables, context hooks,
   *  and memory policy applied to the agent post-create. The "Production-grade
   *  Agent" template ships one; future templates can opt in. */
  initialScaffold?: import("@/lib/agentTemplates").AgentTemplate["dynamicScaffold"];
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
  contextFiles: [],
  kind: "internal",
};

export default function QuickPath({ onCreated, onCancel, initialDraft, initialScaffold }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [createdAgentName, setCreatedAgentName] = useState<string | null>(null);
  const [createdAgentSlug, setCreatedAgentSlug] = useState<string | null>(null);
  const [createdAgentKind, setCreatedAgentKind] = useState<"internal" | "external">("internal");
  const openAgentDetail = useUiStore((s) => s.openAgentDetail);
  const setSection = useUiStore((s) => s.setSection);

  // Draft (auto-saved). initialDraft (from a template pick) wins over the
  // persisted draft so the user lands on the pre-filled form. Merge with
  // DEFAULT_DRAFT so old persisted drafts get any new fields filled in.
  const [draft, setDraft] = useState<QuickDraft>(() => {
    const seed = initialDraft ?? loadQuickDraft() ?? DEFAULT_DRAFT;
    return { ...DEFAULT_DRAFT, ...seed };
  });

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

  // ── Demo mode plumbing ────────────────────────────────────────────────
  // When the demo runner emits typeAgentField / setAgentField steps, it
  // writes a patch with a monotonic seq number. We merge each new patch
  // exactly once into local draft state. This makes form creation look
  // like a human typing while keeping the runner deterministic.
  const demoPatch = useDemoStore((s) => s.pendingAgentFormPatch);
  const demoSubmit = useDemoStore((s) => s.pendingAgentFormSubmit);
  const demoNotifyAgentCreated = useDemoStore((s) => s.notifyAgentCreated);
  const lastSeenPatchSeqRef = useRef(0);
  const lastSeenSubmitRef = useRef(0);
  const systemPromptRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    if (!demoPatch) return;
    if (demoPatch.seq <= lastSeenPatchSeqRef.current) return;
    lastSeenPatchSeqRef.current = demoPatch.seq;
    setDraft((d) => ({ ...d, ...demoPatch.patch }));
    // While the demo is typing into a long field (system prompt), scroll
    // the field into view so the recording follows what's being written.
    if (demoPatch.patch.systemPrompt !== undefined && systemPromptRef.current) {
      systemPromptRef.current.scrollIntoView({ behavior: "smooth", block: "nearest" });
      // Also keep the textarea's own viewport pinned to the bottom so the
      // most recently typed line is always visible.
      const ta = systemPromptRef.current;
      ta.scrollTop = ta.scrollHeight;
    }
  }, [demoPatch]);

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
        kind: draft.kind,
      });
      // F2 Context Hooks — turn the user's "context files" picks into real
      // file hooks on the new agent. Each one fires before every turn and
      // injects the file's contents into a <context> block. Best-effort:
      // an individual hook failure doesn't block the create.
      const cleanedFiles = draft.contextFiles
        .map((p) => p.trim())
        .filter((p) => p.length > 0);
      for (let i = 0; i < cleanedFiles.length; i++) {
        const path = cleanedFiles[i];
        const fallbackName = path.split("/").filter(Boolean).pop() || `file-${i + 1}`;
        try {
          await saveAgentHook({
            agentId: agent.id,
            position: i,
            name: fallbackName,
            kind: "file",
            configJson: hookConfigToJson({ kind: "file", path, maxBytes: 16 * 1024 }),
            enabled: true,
          });
        } catch {
          // ignore — user can re-add manually under Context tab
        }
      }

      // v1.5.5 — Apply the template's dynamic-prompt scaffold (variables,
      // pre-call hooks, memory policy) so the production agent template
      // ships ready-to-run instead of leaving the user to set everything
      // up manually. All best-effort.
      if (initialScaffold) {
        for (const v of initialScaffold.variables ?? []) {
          try {
            await saveAgentVariable({
              agentId: agent.id,
              name: v.name,
              kind: v.kind,
              configJson: v.configJson,
              enabled: v.enabled,
            });
          } catch {
            // ignore — user can add manually under Variables tab
          }
        }
        const startPos = cleanedFiles.length;
        for (let i = 0; i < (initialScaffold.contextHooks ?? []).length; i++) {
          const h = initialScaffold.contextHooks![i];
          try {
            await saveAgentHook({
              agentId: agent.id,
              position: startPos + i,
              name: h.name,
              kind: h.kind,
              configJson: h.configJson,
              enabled: h.enabled,
            });
          } catch {
            // ignore — user can add manually under Context tab
          }
        }
        if (initialScaffold.memoryPolicy) {
          try {
            await updateAgentMemoryPolicy(agent.id, {
              summarizeAfter: initialScaffold.memoryPolicy.summarizeAfterMessages,
              keepLastK: initialScaffold.memoryPolicy.keepRecentMessages,
              summarizerModel: "",
            });
          } catch {
            // ignore — user can set under Memory tab
          }
        }
      }
      setCreatedAgentName(agent.displayName);
      setCreatedAgentSlug(agent.slug);
      setCreatedAgentKind(agent.kind === "external" ? "external" : "internal");
      clearQuickDraft();
      void queryClient.invalidateQueries({ queryKey: ["agents"] });
      void queryClient.invalidateQueries({ queryKey: ["recent-agents"] });
      // Tell the demo runner the create completed — it can advance now.
      demoNotifyAgentCreated();
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

  // Demo runner asked us to submit the form.
  useEffect(() => {
    if (demoSubmit > lastSeenSubmitRef.current) {
      lastSeenSubmitRef.current = demoSubmit;
      requestAnimationFrame(() => {
        const fakeEvent = { preventDefault: () => {} } as React.FormEvent;
        void handleSubmit(fakeEvent);
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [demoSubmit]);

  if (createdAgentName) {
    const isExternalCreated = createdAgentKind === "external";
    const goToAgent = (tab: string) => {
      if (createdAgentSlug) {
        setSection("agents");
        openAgentDetail(createdAgentSlug, tab);
      }
      onCancel(); // close wizard
    };
    return (
      <div className="rounded-lg border border-cs-accent/40 bg-cs-accent/10 p-6 flex items-start gap-3">
        <CheckCircle2 size={20} className="text-cs-accent shrink-0" />
        <div className="flex-1">
          <h3 className="text-sm font-medium text-cs-text">
            {t("createAgent.quick.successTitle", "Agent created")}
          </h3>
          <p className="mt-1 text-xs text-cs-muted">
            {isExternalCreated
              ? t(
                  "createAgent.quick.successBodyExternal",
                  "{{name}} is ready. Next: drop knowledge files and pick a deploy target.",
                  { name: createdAgentName },
                )
              : t("createAgent.quick.successBody", "{{name}} is ready. Open it from the Agents list.", {
                  name: createdAgentName,
                })}
          </p>
          <div className="mt-4 flex flex-wrap items-center gap-2">
            {isExternalCreated && (
              <>
                <button
                  type="button"
                  data-demo-id="quick-success-knowledge"
                  onClick={() => goToAgent("knowledge")}
                  className="rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
                >
                  {t("createAgent.quick.openKnowledge", "Add knowledge →")}
                </button>
                <button
                  type="button"
                  data-demo-id="quick-success-deploy"
                  onClick={() => goToAgent("deploy")}
                  className="rounded-md border border-cs-accent/40 bg-cs-accent/10 px-3 py-1.5 text-xs font-medium text-cs-accent hover:bg-cs-accent/20"
                >
                  {t("createAgent.quick.openDeploy", "Generate deploy bundle →")}
                </button>
              </>
            )}
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
                setCreatedAgentSlug(null);
                reset();
              }}
              className="rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
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
      {/* v2.0.0 — first decision: this agent for me, or for my customers? */}
      <Field
        label={t("createAgent.quick.kind", "Where does this agent run?")}
        hint={t(
          "createAgent.quick.kindHint",
          "Internal runs on your laptop via your local CLI. External is designed for customer-facing deployment (embed widget, Cloudflare Worker, Docker) and auto-locks to read-only permissions.",
        )}
      >
        <div className="grid grid-cols-2 gap-2">
          {(["internal", "external"] as const).map((k) => {
            const active = draft.kind === k;
            return (
              <button
                key={k}
                type="button"
                data-demo-id={`agent-kind-${k}`}
                onClick={() => update("kind", k)}
                className={cn(
                  "rounded-lg border px-3 py-3 text-left text-xs transition-colors",
                  active
                    ? "border-cs-accent bg-cs-accent/10 text-cs-text"
                    : "border-cs-border bg-cs-bg text-cs-muted hover:border-cs-accent/40 hover:text-cs-text",
                )}
              >
                <div className="text-sm font-medium text-cs-text">
                  {k === "internal"
                    ? t("createAgent.quick.kindInternal", "Internal")
                    : t("createAgent.quick.kindExternal", "External")}
                </div>
                <div className="mt-1 text-[11px] leading-tight text-cs-muted">
                  {k === "internal"
                    ? t(
                        "createAgent.quick.kindInternalHint",
                        "Lives in your runtime's agent dir. Full local capabilities.",
                      )
                    : t(
                        "createAgent.quick.kindExternalHint",
                        "Deployable bundle. Read-only by default. Talks to your customers.",
                      )}
                </div>
              </button>
            );
          })}
        </div>
      </Field>

      {/* Live auth/availability summary that responds to kind + runtime.
          Shows whether the user has the right key, hard-blocks External
          with no key behind an inline "Add API key" form. Beatriz feedback
          (2026-05-08): the kind toggle didn't change anything in the form
          before this. */}
      <AuthRequirements kind={draft.kind} runtime={draft.runtime} />

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

      {/* Project picker is Internal-only — external agents run in the
          customer's deployed infra, not in your local project workspace. */}
      {draft.kind === "internal" && (
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
      )}

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
          ref={systemPromptRef}
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

      {/* Skills + MCPs + on-disk context files are Internal-only.
          - Most skills + MCPs assume filesystem / shell capabilities, which
            external (read-only, deployed-to-customer-infra) agents lack.
          - Context files (.txt / .md the agent always reads from disk)
            don't make sense in a deployed Worker — the customer's server
            doesn't have the developer's files.
          External agents get the equivalent on the post-create AgentDetail
          tabs: Knowledge (RAG-backed docs that ship in the bundle) and
          Context (hooks fired before each LLM call). */}
      {draft.kind === "internal" ? (
        <>
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

          <ContextFilesField
            files={draft.contextFiles}
            onChange={(files) => update("contextFiles", files)}
          />
        </>
      ) : (
        <div className="rounded-lg border border-cs-border bg-cs-bg-raised/40 px-3 py-3 text-[11px] text-cs-muted">
          <div className="text-xs font-semibold text-cs-text mb-1">
            {t("createAgent.quick.externalNextSteps", "After saving, you'll get two tabs:")}
          </div>
          <ul className="space-y-1 list-disc list-inside">
            <li>
              <span className="text-cs-text">{t("createAgent.quick.externalKnowledge", "Knowledge")}</span>
              {" — "}
              {t(
                "createAgent.quick.externalKnowledgeHint",
                "drop .md / .txt files; ATO embeds + bakes them into the deploy bundle for RAG retrieval.",
              )}
            </li>
            <li>
              <span className="text-cs-text">{t("createAgent.quick.externalDeploy", "Deploy")}</span>
              {" — "}
              {t(
                "createAgent.quick.externalDeployHint",
                "generate worker.js / Vercel route / Dockerfile / Node script. Customer's API key, customer's infra.",
              )}
            </li>
            <li>
              <span className="text-cs-text">{t("createAgent.quick.externalContext", "Context (existing tab)")}</span>
              {" — "}
              {t(
                "createAgent.quick.externalContextHint",
                "pre-call hooks for live data (DB query, webhook, MCP call) that should fire before every customer message.",
              )}
            </li>
          </ul>
        </div>
      )}

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
            data-demo-id="quick-form-save"
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

/** Context Files — wired on agent create as F2 file Hooks. We give the user
 *  text inputs (paths support `~/...` expansion server-side). They can add
 *  any number; each one ends up as a Hook row that fires on every dispatch. */
function ContextFilesField({
  files,
  onChange,
}: {
  files: string[];
  onChange: (files: string[]) => void;
}) {
  const { t } = useTranslation();
  const update = (i: number, value: string) =>
    onChange(files.map((f, idx) => (idx === i ? value : f)));
  const remove = (i: number) => onChange(files.filter((_, idx) => idx !== i));
  const add = () => onChange([...files, ""]);
  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <span className="text-xs font-medium text-cs-muted uppercase tracking-wide flex items-center gap-1.5">
          <FileText size={11} />
          {t("createAgent.quick.contextFiles", "Context files")}
        </span>
        <span className="text-[10px] text-cs-muted">
          {t(
            "createAgent.quick.contextFilesHint",
            "Loaded on every turn into <context>, not into the system prompt"
          )}
        </span>
      </div>
      <div className="space-y-1.5">
        {files.map((path, i) => (
          <div key={i} className="flex items-center gap-1.5">
            <input
              type="text"
              value={path}
              onChange={(e) => update(i, e.target.value)}
              placeholder="~/notes/style-guide.md"
              className="flex-1 rounded-lg border border-cs-border bg-cs-bg px-3 py-1.5 text-xs text-cs-text font-mono focus:border-cs-accent focus:outline-none"
            />
            <button
              type="button"
              onClick={() => remove(i)}
              className="text-cs-muted hover:text-cs-danger p-1.5 shrink-0"
              aria-label={t("common.remove", "Remove")}
            >
              <Trash2 size={12} />
            </button>
          </div>
        ))}
        <button
          type="button"
          onClick={add}
          className="inline-flex items-center gap-1 text-xs text-cs-accent hover:underline"
        >
          <Plus size={11} />
          {files.length === 0
            ? t("createAgent.quick.addContextFile", "Add a context file")
            : t("createAgent.quick.addAnotherContextFile", "Add another file")}
        </button>
      </div>
      {files.length > 0 && (
        <p className="mt-1.5 text-[10px] text-cs-muted leading-snug">
          {t(
            "createAgent.quick.contextFilesNote",
            "Each file becomes a Context Hook on the agent. You can edit, reorder, or remove them later from the agent's Context tab."
          )}
        </p>
      )}
    </div>
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
