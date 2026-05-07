import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Plus,
  Trash2,
  Loader2,
  AlertCircle,
  Variable as VariableIcon,
  ChevronDown,
  ChevronRight,
  Lock,
  Crown,
} from "lucide-react";
import {
  listAgentVariables,
  saveAgentVariable,
  deleteAgentVariable,
  parseConfig,
  configToJson,
  findReferencedVariables,
  FREE_VARIABLE_KINDS,
  PRO_VARIABLE_KINDS,
  type AgentVariable,
  type VariableKind,
  type VariableConfig,
} from "@/lib/agentVariables";
import { useFeatureFlag } from "@/lib/tier";
import UpgradePrompt from "@/components/Tier/UpgradePrompt";
import type { Agent } from "@/lib/agents";
import { cn } from "@/lib/utils";

// v1.4.0 F1 — Variables tab on Agent detail.
//
// The user's central ask: "we need the prompts not to be stable but with
// caveats." This tab lets them define `{name} → resolver` pairs and see what
// the rendered prompt actually looks like at dispatch time.

interface Props {
  agent: Agent;
}

const KIND_LABEL: Record<VariableKind, string> = {
  "static": "Static value",
  "env": "Env var",
  "project-path": "Active project path",
  "file": "File contents",
  "db-query": "Database query",
  "mcp-call": "MCP tool call",
  "computed": "Computed expression",
};

const KIND_HINT: Record<VariableKind, string> = {
  "static": "A literal value you type in.",
  "env": "Read an environment variable from the desktop's PATH.",
  "project-path": "Resolves to the currently-active project's path.",
  "file": "Read a file from disk and inject its contents.",
  "db-query": "Run a SQL query and use a column of the first row.",
  "mcp-call": "Call an MCP tool and use its result as the value.",
  "computed": "Tiny JS expression evaluated at dispatch time.",
};

export default function VariablesTab({ agent }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const advancedAllowed = useFeatureFlag("variables.advanced");
  const [editing, setEditing] = useState<AgentVariable | "new" | null>(null);
  const [proPrompt, setProPrompt] = useState(false);

  const { data: variables = [], isLoading, error } = useQuery({
    queryKey: ["agent-variables", agent.id],
    queryFn: () => listAgentVariables(agent.id),
    staleTime: 5_000,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteAgentVariable(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["agent-variables", agent.id] });
    },
  });

  const referenced = findReferencedVariables(agent.systemPrompt ?? "");

  return (
    <div className="space-y-5">
      <header>
        <h3 className="text-sm font-medium text-cs-text">
          {t("agentDetail.variables.title", "Variables")}
        </h3>
        <p className="mt-1 text-xs text-cs-muted">
          {t(
            "agentDetail.variables.subtitle",
            "Use {variableName} in your prompts. ATO resolves each variable at dispatch time so the agent always sees fresh, contextual data — not a static blob."
          )}
        </p>
      </header>

      {/* Referenced-but-undefined check */}
      {referenced.length > 0 && (
        <ReferencedSummary referenced={referenced} variables={variables} />
      )}

      {error && (
        <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3">
          <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
          <span className="text-xs text-cs-text">
            {error instanceof Error ? error.message : String(error)}
          </span>
        </div>
      )}

      <div className="space-y-2">
        {isLoading ? (
          <div className="flex items-center justify-center h-20">
            <Loader2 size={16} className="animate-spin text-cs-muted" />
          </div>
        ) : variables.length === 0 && editing !== "new" ? (
          <EmptyState onAdd={() => setEditing("new")} />
        ) : (
          variables.map((v) => (
            <VariableRow
              key={v.id}
              variable={v}
              onEdit={() => setEditing(v)}
              onDelete={() => deleteMutation.mutate(v.id)}
              deleting={deleteMutation.isPending && deleteMutation.variables === v.id}
            />
          ))
        )}
      </div>

      {variables.length > 0 && editing !== "new" && (
        <button
          type="button"
          onClick={() => setEditing("new")}
          className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
        >
          <Plus size={12} />
          {t("agentDetail.variables.add", "Add variable")}
        </button>
      )}

      {editing && (
        <VariableEditor
          agentId={agent.id}
          existing={editing === "new" ? null : editing}
          advancedAllowed={advancedAllowed}
          onClose={() => setEditing(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({ queryKey: ["agent-variables", agent.id] });
            setEditing(null);
          }}
          onProPicked={() => setProPrompt(true)}
        />
      )}

      <UpgradePrompt
        feature="variables.advanced"
        open={proPrompt}
        onClose={() => setProPrompt(false)}
      />
    </div>
  );
}

function EmptyState({ onAdd }: { onAdd: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 flex items-start gap-3">
      <VariableIcon size={20} className="text-cs-muted shrink-0" />
      <div className="flex-1 min-w-0">
        <p className="text-sm text-cs-text">
          {t("agentDetail.variables.emptyTitle", "Why your agent should have variables")}
        </p>
        <p className="mt-1 text-xs text-cs-muted leading-relaxed">
          {t(
            "agentDetail.variables.emptyBody",
            'A static system prompt is the same string every turn. Variables make it adapt: {user_name} pulls from the OS, {today} resolves at fire time, {project_root} reflects the active project, {recent_orders} hits a database. The agent reads them fresh on every dispatch.'
          )}
        </p>
        <p className="mt-2 text-xs text-cs-muted">
          {t(
            "agentDetail.variables.emptyResolvers",
            "Resolvers available: static · env var · project path · file · database query · MCP call · computed JS."
          )}
        </p>
        <button
          type="button"
          onClick={onAdd}
          className="mt-3 inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
        >
          <Plus size={12} />
          {t("agentDetail.variables.add", "Add variable")}
        </button>
      </div>
    </div>
  );
}

function ReferencedSummary({
  referenced,
  variables,
}: {
  referenced: string[];
  variables: AgentVariable[];
}) {
  const { t } = useTranslation();
  const defined = new Set(variables.map((v) => v.name));
  const undefinedNames = referenced.filter((n) => !defined.has(n));
  if (undefinedNames.length === 0) return null;
  return (
    <div className="rounded-md border border-cs-warning/40 bg-cs-warning/10 p-3 flex items-start gap-2 text-xs">
      <AlertCircle size={12} className="text-cs-warning shrink-0 mt-0.5" />
      <div className="flex-1 text-cs-text">
        <span className="font-medium">
          {t("agentDetail.variables.unresolvedTitle", "Unresolved tokens in your prompt")}:
        </span>
        <span className="ml-1 font-mono text-[11px]">
          {undefinedNames.map((n) => `{${n}}`).join(" ")}
        </span>
        <span className="block mt-1 text-cs-muted">
          {t(
            "agentDetail.variables.unresolvedHint",
            "Add a resolver below or remove the token from the prompt."
          )}
        </span>
      </div>
    </div>
  );
}

function VariableRow({
  variable,
  onEdit,
  onDelete,
  deleting,
}: {
  variable: AgentVariable;
  onEdit: () => void;
  onDelete: () => void;
  deleting: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const cfg = parseConfig(variable);
  const isPro = PRO_VARIABLE_KINDS.includes(variable.kind);

  return (
    <div className="rounded-lg border border-cs-border bg-cs-card overflow-hidden">
      <div className="flex items-center gap-3 px-3 py-2">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className="text-cs-muted hover:text-cs-text shrink-0"
          aria-label={open ? t("common.close", "Close") : t("common.send", "Open")}
        >
          {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </button>
        <code className="text-xs font-mono text-cs-accent shrink-0">{`{${variable.name}}`}</code>
        <span className="text-[10px] uppercase tracking-wide text-cs-muted shrink-0">
          {KIND_LABEL[variable.kind]}
        </span>
        {isPro && <Crown size={10} className="text-cs-accent shrink-0" />}
        <span className="flex-1 truncate text-xs text-cs-muted">
          {summaryFor(cfg)}
        </span>
        {!variable.enabled && (
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
          aria-label={t("common.delete", "Delete")}
        >
          {deleting ? <Loader2 size={12} className="animate-spin" /> : <Trash2 size={12} />}
        </button>
      </div>
      {open && (
        <pre className="border-t border-cs-border bg-cs-bg p-3 text-[11px] text-cs-muted font-mono whitespace-pre-wrap">
{JSON.stringify(JSON.parse(variable.configJson), null, 2)}
        </pre>
      )}
    </div>
  );
}

function summaryFor(cfg: VariableConfig): string {
  switch (cfg.kind) {
    case "static": return `"${cfg.value.length > 40 ? cfg.value.slice(0, 40) + "…" : cfg.value}"`;
    case "env": return `$${cfg.var}`;
    case "project-path": return "Active project path";
    case "file": return cfg.path;
    case "db-query": return `${cfg.path}: ${cfg.sql.slice(0, 40)}…`;
    case "mcp-call": return `${cfg.server}.${cfg.tool}`;
    case "computed": return cfg.expr.slice(0, 60);
  }
}

function VariableEditor({
  agentId,
  existing,
  advancedAllowed,
  onClose,
  onSaved,
  onProPicked,
}: {
  agentId: string;
  existing: AgentVariable | null;
  advancedAllowed: boolean;
  onClose: () => void;
  onSaved: () => void;
  onProPicked: () => void;
}) {
  const { t } = useTranslation();
  const initial = existing
    ? parseConfig(existing)
    : ({ kind: "static", value: "" } as VariableConfig);

  const [name, setName] = useState(existing?.name ?? "");
  const [enabled, setEnabled] = useState(existing?.enabled ?? true);
  const [kind, setKind] = useState<VariableKind>(initial.kind);
  const [config, setConfig] = useState<VariableConfig>(initial);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const save = async () => {
    if (!name.trim() || saving) return;
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
      setErr(t("agentDetail.variables.badName", "Name must be letters/digits/underscores"));
      return;
    }
    setErr(null);
    setSaving(true);
    try {
      await saveAgentVariable({
        id: existing?.id,
        agentId,
        name: name.trim(),
        kind,
        configJson: configToJson(config),
        enabled,
      });
      onSaved();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const pickKind = (k: VariableKind) => {
    if (PRO_VARIABLE_KINDS.includes(k) && !advancedAllowed) {
      onProPicked();
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
            ? t("agentDetail.variables.editTitle", "Edit variable")
            : t("agentDetail.variables.newTitle", "New variable")}
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

      {/* Name */}
      <Field label={t("agentDetail.variables.name", "Name")} required>
        <div className="flex items-center gap-2">
          <span className="text-xs text-cs-muted font-mono">{"{"}</span>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="user_name"
            className="flex-1 rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
            autoFocus
          />
          <span className="text-xs text-cs-muted font-mono">{"}"}</span>
        </div>
      </Field>

      {/* Kind picker */}
      <Field label={t("agentDetail.variables.kind", "Resolver kind")}>
        <div className="grid grid-cols-2 gap-1.5">
          {[...FREE_VARIABLE_KINDS, ...PRO_VARIABLE_KINDS].map((k) => {
            const isPro = PRO_VARIABLE_KINDS.includes(k);
            const locked = isPro && !advancedAllowed;
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

      {/* Config — kind-specific */}
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
    </div>
  );
}

function defaultsFor(kind: VariableKind): VariableConfig {
  switch (kind) {
    case "static": return { kind, value: "" };
    case "env": return { kind, var: "" };
    case "project-path": return { kind };
    case "file": return { kind, path: "", maxBytes: 8192 };
    case "db-query": return { kind, path: "", sql: "", maxRows: 20 };
    case "mcp-call": return { kind, server: "", tool: "", args: {} };
    case "computed": return { kind, expr: "" };
  }
}

function KindConfigEditor({
  kind,
  config,
  onChange,
}: {
  kind: VariableKind;
  config: VariableConfig;
  onChange: (c: VariableConfig) => void;
}) {
  const { t } = useTranslation();
  if (kind === "static" && config.kind === "static") {
    return (
      <Field label={t("agentDetail.variables.fields.value", "Value")}>
        <input
          type="text"
          value={config.value}
          onChange={(e) => onChange({ kind, value: e.target.value })}
          className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text focus:border-cs-accent focus:outline-none"
        />
      </Field>
    );
  }
  if (kind === "env" && config.kind === "env") {
    return (
      <Field label={t("agentDetail.variables.fields.envVar", "Env var name")}>
        <input
          type="text"
          value={config.var}
          onChange={(e) => onChange({ kind, var: e.target.value })}
          placeholder="OPENAI_API_KEY"
          className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
        />
      </Field>
    );
  }
  if (kind === "project-path") {
    return (
      <p className="text-xs text-cs-muted">
        {t(
          "agentDetail.variables.fields.projectPathHint",
          "No config — resolves to whichever project is active when the agent runs."
        )}
      </p>
    );
  }
  if (kind === "file" && config.kind === "file") {
    return (
      <>
        <Field label={t("agentDetail.variables.fields.filePath", "File path")}>
          <input
            type="text"
            value={config.path}
            onChange={(e) => onChange({ kind, path: e.target.value, maxBytes: config.maxBytes })}
            placeholder="~/notes/style-guide.md"
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
        <Field label={t("agentDetail.variables.fields.maxBytes", "Max bytes (truncate after)")}>
          <input
            type="number"
            value={config.maxBytes ?? 8192}
            min={256}
            onChange={(e) =>
              onChange({ kind, path: config.path, maxBytes: parseInt(e.target.value, 10) })
            }
            className="w-32 rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
      </>
    );
  }
  if (kind === "db-query" && config.kind === "db-query") {
    return (
      <>
        <Field label={t("agentDetail.variables.fields.dbPath", "SQLite file path")}>
          <input
            type="text"
            value={config.path}
            onChange={(e) =>
              onChange({ kind, path: e.target.value, sql: config.sql, maxRows: config.maxRows })
            }
            placeholder="~/data/app.db"
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
        <Field label={t("agentDetail.variables.fields.dbSql", "SELECT query")}>
          <textarea
            value={config.sql}
            onChange={(e) =>
              onChange({ kind, path: config.path, sql: e.target.value, maxRows: config.maxRows })
            }
            rows={3}
            placeholder="SELECT id, plan FROM users WHERE active = 1 LIMIT 5"
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
        <Field label={t("agentDetail.variables.fields.dbMaxRows", "Max rows")}>
          <input
            type="number"
            value={config.maxRows ?? 20}
            min={1}
            max={500}
            onChange={(e) =>
              onChange({ kind, path: config.path, sql: config.sql, maxRows: parseInt(e.target.value, 10) })
            }
            className="w-24 rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
        <p className="text-[10px] text-cs-muted">
          {t(
            "agentDetail.variables.fields.dbHint",
            "Read-only. Only SELECT/WITH queries are allowed; everything else returns an error."
          )}
        </p>
      </>
    );
  }
  if (kind === "computed" && config.kind === "computed") {
    return (
      <>
        <Field label={t("agentDetail.variables.fields.computedExpr", "Expression")}>
          <input
            type="text"
            value={config.expr}
            onChange={(e) => onChange({ kind, expr: e.target.value })}
            placeholder='"prefix-" + project_path()'
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
        <p className="text-[10px] text-cs-muted">
          {t(
            "agentDetail.variables.fields.computedHint",
            'Supports literals (numbers, "strings"), arithmetic (+ - * /), string concat with +, parentheses, and project_path(). No arbitrary JS.'
          )}
        </p>
      </>
    );
  }
  // mcp-call still stubbed.
  return (
    <p className="text-xs text-cs-warning">
      {t(
        "agentDetail.variables.fields.mcpCallStub",
        "MCP-call resolver lands in a follow-up. Storing the config now is fine; runtime resolution returns 'mcp-call-not-yet-implemented' until then."
      )}
    </p>
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
