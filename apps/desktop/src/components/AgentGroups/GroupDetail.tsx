import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation } from "@tanstack/react-query";
import {
  ArrowLeft,
  Plus,
  X,
  Loader2,
  AlertCircle,
  Save,
  Network,
  Sparkles,
  Crown,
  ChevronDown,
  ChevronRight,
  GitBranch,
  FormInput,
} from "lucide-react";
import {
  createAgentGroup,
  updateAgentGroup,
  parseRouterConfig,
  DEFAULT_ROUTER_CONFIG,
  type AgentGroup,
  type GroupMemberInput,
  type RouterConfig,
  type RouterRule,
} from "@/lib/agentGroups";
import { listAgents, type Agent, type AgentRuntime } from "@/lib/agents";
import { useFeatureFlag } from "@/lib/tier";
import UpgradePrompt from "@/components/Tier/UpgradePrompt";
import GroupGraphEditor from "./GroupGraphEditor";
import { useDemoStore } from "@/stores/useDemoStore";
import { cn } from "@/lib/utils";

// v1.4.0 F4 — Group create/edit form. Wave 3.2 will add the visual graph
// editor on top of this; for v1.4.0-α the form gives users the full create
// → save → dispatch flow.

interface Props {
  existing: AgentGroup | null;
  onClose: () => void;
  onSaved: () => void;
}

const RUNTIMES: AgentRuntime[] = ["claude", "codex", "gemini", "openclaw", "hermes"];

const FREE_CHILD_LIMIT = 3;

export default function GroupDetail({ existing, onClose, onSaved }: Props) {
  const { t } = useTranslation();
  const unlimited = useFeatureFlag("groups.unlimited");

  const [displayName, setDisplayName] = useState(existing?.displayName ?? "");
  const [description, setDescription] = useState(existing?.description ?? "");
  const [runtime, setRuntime] = useState<AgentRuntime>(existing?.runtime ?? "claude");
  const [members, setMembers] = useState<GroupMemberInput[]>(
    existing?.members.map((m) => ({
      agentSlug: m.agentSlug,
      role: m.role,
      position: m.position,
    })) ?? []
  );
  const [routerConfig, setRouterConfig] = useState<RouterConfig>(
    existing ? parseRouterConfig(existing.routerConfig) : DEFAULT_ROUTER_CONFIG
  );
  const [dispatchKind, setDispatchKind] = useState<"routed" | "sequential">(
    existing?.dispatchKind ?? "routed"
  );
  const [error, setError] = useState<string | null>(null);
  const [proPrompt, setProPrompt] = useState(false);
  // Wave 3.2 — Graph view of router + children. Toggle between graph + form.
  const [view, setView] = useState<"form" | "graph">("form");
  const routerSectionRef = useRef<HTMLDivElement | null>(null);
  // Demo-only refs — used by the autoFillGroupForm effect below to scroll
  // each new section into view as the form animates. Without these the
  // recording stays pinned at the top showing NAME / DESC / TYPE while the
  // CHILDREN and ROUTER sections being populated are below the fold.
  const childrenSectionRef = useRef<HTMLDivElement | null>(null);
  const saveButtonRef = useRef<HTMLButtonElement | null>(null);
  const [focusedChild, setFocusedChild] = useState<string | null>(null);

  // Reset member list when runtime changes (mismatched-runtime children would
  // be rejected by the Rust validator anyway).
  useEffect(() => {
    if (existing) return;
    setMembers([]);
  }, [runtime, existing]);

  // Demo bridge — when the demo runner pushes a `pendingGroupAutoFill`,
  // animate the form so the recording shows the same "watching it being
  // built" UX as the agent quick form. New groups only.
  const demoAutoFill = useDemoStore((s) => s.pendingGroupAutoFill);
  const lastAutoFillSeqRef = useRef(0);
  useEffect(() => {
    if (existing) return;
    if (!demoAutoFill || demoAutoFill.seq === lastAutoFillSeqRef.current) return;
    lastAutoFillSeqRef.current = demoAutoFill.seq;
    let cancelled = false;
    const { spec } = demoAutoFill;
    const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
    (async () => {
      if (spec.runtime) setRuntime(spec.runtime);
      await wait(180);
      if (cancelled) return;
      setDisplayName(spec.displayName);
      await wait(420);
      if (cancelled) return;
      if (spec.description) setDescription(spec.description);
      await wait(420);
      if (cancelled) return;
      setDispatchKind(spec.dispatchKind);
      await wait(380);
      if (cancelled) return;
      // Scroll the Children section into view BEFORE we start adding
      // children. The form is taller than the viewport once it's
      // populated, and the recording was getting stuck at the top showing
      // only NAME / DESCRIPTION / TYPE while children + router rules
      // animated below the fold (Beatriz feedback, 2026-05-07).
      childrenSectionRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
      await wait(450);
      if (cancelled) return;
      // Children — append one at a time so the list grows visibly.
      for (const slug of spec.childSlugs) {
        if (cancelled) return;
        setMembers((prev) =>
          prev.some((m) => m.agentSlug === slug)
            ? prev
            : [...prev, { agentSlug: slug, role: "child", position: prev.length }]
        );
        await wait(380);
      }
      if (cancelled) return;
      // Router rule — only meaningful for routed groups. Scroll the
      // router section into view so the rule animation is visible.
      if (spec.dispatchKind === "routed" && spec.routerRule) {
        routerSectionRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
        await wait(450);
        if (cancelled) return;
        const rule: RouterRule = {
          if: { keyword: spec.routerRule.keywords },
          then: spec.routerRule.thenSlug,
        };
        setRouterConfig((rc) => ({ ...rc, rules: [...(rc.rules ?? []), rule] }));
        await wait(800);
      }
      if (cancelled) return;
      // Final beat: scroll back up to the Save button so the next demo
      // step (which clicks group-save) lands on a visible target.
      saveButtonRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
    })();
    return () => {
      cancelled = true;
    };
    // intentional: react only to seq bumps
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [demoAutoFill?.seq]);

  const { data: allAgents = [] } = useQuery({
    queryKey: ["agents-for-group", runtime],
    queryFn: () => listAgents({ runtime }),
    staleTime: 5_000,
  });

  const childMembers = members.filter((m) => m.role === "child");
  const childLimitReached = !unlimited && childMembers.length >= FREE_CHILD_LIMIT;

  const saveMutation = useMutation({
    mutationFn: async () => {
      if (existing) {
        return updateAgentGroup({
          id: existing.id,
          description: description || undefined,
          routerConfig,
          members,
        });
      }
      return createAgentGroup({
        displayName,
        runtime,
        description: description || undefined,
        routerConfig,
        members,
        dispatchKind,
      });
    },
    onSuccess: () => {
      setError(null);
      onSaved();
    },
    onError: (e) => setError(e instanceof Error ? e.message : String(e)),
  });

  const addChild = (agent: Agent) => {
    if (childLimitReached) {
      setProPrompt(true);
      return;
    }
    if (members.some((m) => m.agentSlug === agent.slug)) return;
    setMembers([
      ...members,
      {
        agentSlug: agent.slug,
        role: "child",
        position: members.length,
      },
    ]);
  };

  const removeMember = (slug: string) => {
    setMembers(members.filter((m) => m.agentSlug !== slug).map((m, i) => ({ ...m, position: i })));
  };

  const availableForChild = allAgents.filter(
    (a) => !members.some((m) => m.agentSlug === a.slug)
  );

  const canSave =
    displayName.trim().length > 0 &&
    childMembers.length > 0 &&
    !saveMutation.isPending;

  return (
    <div className="space-y-5">
      <header className="flex items-start justify-between gap-3">
        <div className="flex items-center gap-3 min-w-0">
          <button
            type="button"
            onClick={onClose}
            className="text-cs-muted hover:text-cs-text shrink-0"
          >
            <ArrowLeft size={16} />
          </button>
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <Network size={16} className="text-cs-accent" />
              <h3 className="text-sm font-medium text-cs-text">
                {existing
                  ? t("agentGroups.editTitle", "Edit group")
                  : t("agentGroups.newTitle", "New group")}
              </h3>
            </div>
            <p className="mt-0.5 text-xs text-cs-muted">
              {t(
                "agentGroups.detailHint",
                "Pick the runtime, add child agents, and configure how the router decides who handles each prompt."
              )}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <div className="inline-flex rounded-md border border-cs-border bg-cs-bg-raised p-0.5">
            <button
              type="button"
              onClick={() => setView("form")}
              className={cn(
                "inline-flex items-center gap-1 rounded px-2 py-1 text-[11px] font-medium transition",
                view === "form"
                  ? "bg-cs-accent/15 text-cs-accent"
                  : "text-cs-muted hover:text-cs-text"
              )}
              aria-pressed={view === "form"}
            >
              <FormInput size={11} />
              {t("agentGroups.viewForm", "Form")}
            </button>
            <button
              type="button"
              onClick={() => setView("graph")}
              className={cn(
                "inline-flex items-center gap-1 rounded px-2 py-1 text-[11px] font-medium transition",
                view === "graph"
                  ? "bg-cs-accent/15 text-cs-accent"
                  : "text-cs-muted hover:text-cs-text"
              )}
              aria-pressed={view === "graph"}
            >
              <GitBranch size={11} />
              {t("agentGroups.viewGraph", "Graph")}
            </button>
          </div>
          <button
            type="button"
            data-demo-id="group-save"
            ref={saveButtonRef}
            disabled={!canSave}
            onClick={() => saveMutation.mutate()}
            className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
          >
            {saveMutation.isPending ? <Loader2 size={12} className="animate-spin" /> : <Save size={12} />}
            {existing ? t("common.save", "Save") : t("agentGroups.create", "Create group")}
          </button>
        </div>
      </header>

      {error && (
        <div className="flex items-start gap-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
          <AlertCircle size={12} className="text-cs-danger shrink-0 mt-0.5" />
          <span>{error}</span>
        </div>
      )}

      {view === "graph" && (
        <GroupGraphEditor
          members={members}
          routerConfig={routerConfig}
          runtime={runtime}
          onSelectChild={(slug) => {
            setFocusedChild(slug);
            // Scroll the rule(s) for this child into view in the router
            // section. The router section is below the children list.
            routerSectionRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
          }}
          onSelectRouter={() => {
            setFocusedChild(null);
            routerSectionRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
          }}
        />
      )}

      {/* Basic info */}
      <section className="space-y-3">
        <Field label={t("agentGroups.fields.name", "Name")} required>
          <input
            type="text"
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            disabled={!!existing}
            placeholder="customer-support"
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text focus:border-cs-accent focus:outline-none disabled:opacity-50"
          />
        </Field>

        <Field label={t("agentGroups.fields.description", "Description")}>
          <input
            type="text"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder={t(
              "agentGroups.fields.descriptionPlaceholder",
              "Routes customer support emails to billing/technical/general specialists"
            )}
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
          />
        </Field>

        {!existing && (
          <Field label={t("agentGroups.fields.runtime", "Runtime")} required>
            <select
              value={runtime}
              onChange={(e) => setRuntime(e.target.value as AgentRuntime)}
              className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
            >
              {RUNTIMES.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </select>
            <p className="mt-1 text-[10px] text-cs-muted">
              {t(
                "agentGroups.fields.runtimeHint",
                "All children must run on the same runtime. The router runs there too."
              )}
            </p>
          </Field>
        )}

        {/* Dispatch kind — Routed (current) vs Sequential ("automation"
            pipeline). Hidden for existing groups since changing it on the
            fly would invalidate the router rules; keep it create-only. */}
        {!existing && (
          <Field label={t("agentGroups.fields.dispatchKind", "Type")} required>
            <div className="grid grid-cols-2 gap-2">
              <button
                type="button"
                onClick={() => setDispatchKind("routed")}
                className={cn(
                  "rounded-md border px-3 py-2 text-left transition",
                  dispatchKind === "routed"
                    ? "border-cs-accent/60 bg-cs-accent/10"
                    : "border-cs-border bg-cs-bg-raised hover:border-cs-border/80"
                )}
              >
                <div className={cn(
                  "text-xs font-medium",
                  dispatchKind === "routed" ? "text-cs-accent" : "text-cs-text"
                )}>
                  {t("agentGroups.dispatchKind.routed", "Routed")}
                </div>
                <p className="mt-0.5 text-[10px] text-cs-muted leading-snug">
                  {t(
                    "agentGroups.dispatchKind.routedHint",
                    "Router picks one child per prompt. Keyword rules + LLM fallback."
                  )}
                </p>
              </button>
              <button
                type="button"
                onClick={() => setDispatchKind("sequential")}
                className={cn(
                  "rounded-md border px-3 py-2 text-left transition",
                  dispatchKind === "sequential"
                    ? "border-cs-accent/60 bg-cs-accent/10"
                    : "border-cs-border bg-cs-bg-raised hover:border-cs-border/80"
                )}
              >
                <div className={cn(
                  "text-xs font-medium",
                  dispatchKind === "sequential" ? "text-cs-accent" : "text-cs-text"
                )}>
                  {t("agentGroups.dispatchKind.sequential", "Automation pipeline")}
                </div>
                <p className="mt-0.5 text-[10px] text-cs-muted leading-snug">
                  {t(
                    "agentGroups.dispatchKind.sequentialHint",
                    "Children run in order. Each agent's output feeds the next. One prompt → full pipeline."
                  )}
                </p>
              </button>
            </div>
          </Field>
        )}
        {existing && (
          <Field label={t("agentGroups.fields.dispatchKind", "Type")}>
            <div className="rounded-md border border-cs-border bg-cs-bg-raised px-3 py-2 text-xs text-cs-muted">
              {existing.dispatchKind === "sequential"
                ? t("agentGroups.dispatchKind.sequential", "Automation pipeline")
                : t("agentGroups.dispatchKind.routed", "Routed")}
              <span className="ml-2 text-[10px]">
                {t("agentGroups.dispatchKind.locked", "(locked after create)")}
              </span>
            </div>
          </Field>
        )}
      </section>

      {/* Children */}
      <section ref={childrenSectionRef}>
        <header className="flex items-center justify-between mb-2">
          <h4 className="text-xs font-medium text-cs-text uppercase tracking-wide">
            {t("agentGroups.children", "Children")}{" "}
            <span className="text-cs-muted">
              ({childMembers.length}
              {!unlimited && ` / ${FREE_CHILD_LIMIT}`})
            </span>
          </h4>
        </header>

        {childMembers.length > 0 && (
          <div className="space-y-1.5 mb-3">
            {childMembers.map((m) => (
              <div
                key={m.agentSlug}
                className="flex items-center gap-2 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5"
              >
                <Sparkles size={12} className="text-cs-accent" />
                <code className="text-xs font-mono text-cs-text flex-1">{m.agentSlug}</code>
                <button
                  type="button"
                  onClick={() => removeMember(m.agentSlug)}
                  className="text-cs-muted hover:text-cs-danger"
                  aria-label={t("common.remove", "Remove")}
                >
                  <X size={12} />
                </button>
              </div>
            ))}
          </div>
        )}

        <ChildPicker
          available={availableForChild}
          locked={childLimitReached}
          onAdd={addChild}
          onLockedClick={() => setProPrompt(true)}
        />

        {childMembers.length === 0 && (
          <p className="mt-2 text-[11px] text-cs-warning">
            {t("agentGroups.needChild", "Pick at least one child agent before saving.")}
          </p>
        )}
      </section>

      {/* Router config — only relevant for "routed" groups. Sequential
          groups don't have a router; they run children in position order. */}
      <div ref={routerSectionRef}>
        {dispatchKind === "routed" ? (
          <RouterEditor
            config={routerConfig}
            onChange={setRouterConfig}
            children={childMembers.map((m) => m.agentSlug)}
            focusedChild={focusedChild}
          />
        ) : (
          <section>
            <header className="mb-2">
              <h4 className="text-xs font-medium text-cs-text uppercase tracking-wide">
                {t("agentGroups.pipeline", "Pipeline order")}
              </h4>
              <p className="text-[11px] text-cs-muted mt-0.5">
                {t(
                  "agentGroups.pipelineHint",
                  "Children run top-to-bottom. The first agent receives the user's prompt; every later agent receives the previous agent's output."
                )}
              </p>
            </header>
            {childMembers.length === 0 ? (
              <p className="text-[11px] text-cs-muted italic">
                {t("agentGroups.pipelineEmpty", "Add children above to define the pipeline.")}
              </p>
            ) : (
              <ol className="space-y-1.5">
                {childMembers.map((m, i) => (
                  <li
                    key={m.agentSlug}
                    className="flex items-center gap-2 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-2"
                  >
                    <span className="text-[10px] font-mono text-cs-muted w-5 shrink-0">
                      {i + 1}.
                    </span>
                    <Sparkles size={11} className="text-cs-accent shrink-0" />
                    <code className="text-xs font-mono text-cs-text flex-1">{m.agentSlug}</code>
                    {i < childMembers.length - 1 && (
                      <span className="text-[10px] text-cs-muted">→</span>
                    )}
                  </li>
                ))}
              </ol>
            )}
          </section>
        )}
      </div>

      <UpgradePrompt
        feature="groups.unlimited"
        open={proPrompt}
        onClose={() => setProPrompt(false)}
      />
    </div>
  );
}

function ChildPicker({
  available,
  locked,
  onAdd,
  onLockedClick,
}: {
  available: Agent[];
  locked: boolean;
  onAdd: (agent: Agent) => void;
  onLockedClick: () => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  if (available.length === 0) {
    return (
      <p className="rounded-md border border-dashed border-cs-border bg-cs-bg-raised/40 p-3 text-xs text-cs-muted">
        {t(
          "agentGroups.noChildAgents",
          "No agents available for this runtime. Create individual agents first, then come back to bundle them into a group."
        )}
      </p>
    );
  }

  return (
    <div className="rounded-md border border-cs-border bg-cs-bg-raised">
      <button
        type="button"
        onClick={() => (locked ? onLockedClick() : setOpen((v) => !v))}
        className="w-full flex items-center gap-2 px-3 py-2 text-left text-xs text-cs-text hover:bg-cs-border/40"
      >
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Plus size={12} className="text-cs-accent" />
        <span className="flex-1">
          {t("agentGroups.addChild", "Add child agent")} ({available.length}{" "}
          {t("agentGroups.available", "available")})
        </span>
        {locked && <Crown size={11} className="text-cs-accent" />}
      </button>
      {open && !locked && (
        <ul className="border-t border-cs-border max-h-48 overflow-y-auto">
          {available.map((a) => (
            <li key={a.id}>
              <button
                type="button"
                onClick={() => onAdd(a)}
                className="w-full flex items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-cs-border/40"
              >
                <Sparkles size={11} className="text-cs-accent" />
                <code className="font-mono text-cs-text">{a.slug}</code>
                {a.description && (
                  <span className="text-cs-muted truncate flex-1">{a.description}</span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function RouterEditor({
  config,
  onChange,
  children,
  focusedChild,
}: {
  config: RouterConfig;
  onChange: (c: RouterConfig) => void;
  children: string[];
  focusedChild?: string | null;
}) {
  const { t } = useTranslation();

  const updateRule = (i: number, rule: RouterRule) => {
    const next = [...config.rules];
    next[i] = rule;
    onChange({ ...config, rules: next });
  };

  const addRule = () => {
    onChange({
      ...config,
      rules: [...config.rules, { if: { keyword: [] }, then: children[0] ?? "" }],
    });
  };

  const removeRule = (i: number) => {
    onChange({ ...config, rules: config.rules.filter((_, idx) => idx !== i) });
  };

  return (
    <section>
      <header className="mb-2">
        <h4 className="text-xs font-medium text-cs-text uppercase tracking-wide">
          {t("agentGroups.router", "Router")}
        </h4>
        <p className="text-[11px] text-cs-muted mt-0.5">
          {t(
            "agentGroups.routerHint",
            "Rules are evaluated in order. The first match wins. If no rule matches, the LLM fallback (if enabled) classifies; otherwise the first child is used."
          )}
        </p>
      </header>

      <div className="space-y-2">
        {config.rules.map((rule, i) => (
          <RuleRow
            key={i}
            rule={rule}
            children={children}
            onChange={(r) => updateRule(i, r)}
            onRemove={() => removeRule(i)}
            highlighted={!!focusedChild && rule.then === focusedChild}
          />
        ))}
      </div>

      <button
        type="button"
        onClick={addRule}
        disabled={children.length === 0}
        className="mt-2 inline-flex items-center gap-1 rounded-md border border-cs-border bg-cs-bg-raised px-2.5 py-1 text-xs text-cs-text hover:border-cs-hover disabled:opacity-50"
      >
        <Plus size={11} />
        {t("agentGroups.addRule", "Add rule")}
      </button>

      <div className="mt-4 rounded-md border border-cs-border bg-cs-bg-raised p-3">
        <label className="flex items-center gap-2 text-xs text-cs-text cursor-pointer">
          <input
            type="checkbox"
            checked={config.llmFallback.enabled}
            onChange={(e) =>
              onChange({
                ...config,
                llmFallback: { ...config.llmFallback, enabled: e.target.checked },
              })
            }
            className="h-3 w-3 accent-cs-accent"
          />
          <span className="font-medium">
            {t("agentGroups.llmFallback", "LLM fallback (when no rule matches)")}
          </span>
        </label>
        <p className="mt-1 text-[10px] text-cs-muted ml-5">
          {t(
            "agentGroups.llmFallbackHint",
            "Asks the runtime's classifier to pick a child by name. Cheap fast model recommended."
          )}
        </p>
        {config.llmFallback.enabled && (
          <input
            type="text"
            value={config.llmFallback.model ?? ""}
            onChange={(e) =>
              onChange({
                ...config,
                llmFallback: { ...config.llmFallback, model: e.target.value || undefined },
              })
            }
            placeholder={t(
              "agentGroups.llmFallbackModelPlaceholder",
              "Optional model override (e.g. claude-haiku-4-5)"
            )}
            className="mt-2 w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-xs text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        )}
      </div>
    </section>
  );
}

function RuleRow({
  rule,
  children,
  onChange,
  onRemove,
  highlighted,
}: {
  rule: RouterRule;
  children: string[];
  onChange: (r: RouterRule) => void;
  onRemove: () => void;
  highlighted?: boolean;
}) {
  const { t } = useTranslation();
  const keywords = rule.if?.keyword ?? [];
  const target = rule.then ?? "";

  return (
    <div className={cn(
      "rounded-md border bg-cs-bg-raised p-3 space-y-2 transition",
      highlighted ? "border-cs-accent/60 bg-cs-accent/5" : "border-cs-border"
    )}>
      <div className="flex items-center justify-between">
        <span className="text-[10px] uppercase tracking-wide text-cs-muted">
          {t("agentGroups.if", "If")}
        </span>
        <button
          type="button"
          onClick={onRemove}
          className="text-cs-muted hover:text-cs-danger"
          aria-label={t("common.remove", "Remove")}
        >
          <X size={12} />
        </button>
      </div>
      <div>
        <label className="block text-[10px] text-cs-muted mb-1">
          {t("agentGroups.keywordsLabel", "any of these keywords (comma-separated)")}
        </label>
        <input
          type="text"
          value={keywords.join(", ")}
          onChange={(e) =>
            onChange({
              ...rule,
              if: {
                ...rule.if,
                keyword: e.target.value
                  .split(",")
                  .map((s) => s.trim())
                  .filter(Boolean),
              },
            })
          }
          placeholder="fatura, cobrança, payment"
          className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-xs text-cs-text font-mono focus:border-cs-accent focus:outline-none"
        />
      </div>
      <div>
        <label className="block text-[10px] uppercase tracking-wide text-cs-muted mb-1">
          {t("agentGroups.then", "Then route to")}
        </label>
        <select
          value={target}
          onChange={(e) => onChange({ ...rule, then: e.target.value })}
          className={cn(
            "w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-xs text-cs-text font-mono focus:border-cs-accent focus:outline-none",
            target === "" && "text-cs-muted"
          )}
        >
          <option value="" disabled>
            {t("agentGroups.pickChild", "(pick a child)")}
          </option>
          {children.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
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
