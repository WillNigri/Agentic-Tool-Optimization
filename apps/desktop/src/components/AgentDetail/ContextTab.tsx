import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Plus,
  Trash2,
  Loader2,
  AlertCircle,
  Layers,
  ChevronDown,
  ChevronRight,
  Crown,
  Lock,
} from "lucide-react";
import {
  listAgentHooks,
  saveAgentHook,
  deleteAgentHook,
  parseHookConfig,
  hookConfigToJson,
  FREE_HOOK_KINDS,
  PRO_HOOK_KINDS,
  type AgentHook,
  type HookKind,
  type HookConfig,
} from "@/lib/agentHooks";
import { useFeatureFlag } from "@/lib/tier";
import TierGate from "@/components/Tier/TierGate";
import UpgradePrompt from "@/components/Tier/UpgradePrompt";
import type { Agent } from "@/lib/agents";
import { cn } from "@/lib/utils";

// v1.4.0 F2 — Context hooks tab.
//
// Whole tab gated behind `context-hooks` (Pro). Free users see the lock badge
// + upgrade prompt. Pro users get full CRUD + an executor preview.

interface Props {
  agent: Agent;
}

const KIND_LABEL: Record<HookKind, string> = {
  "file": "File contents",
  "webhook": "Webhook (HTTP GET)",
  "mcp-call": "MCP tool call",
  "db-query": "Database query",
  "computed": "Computed expression",
};

const KIND_HINT: Record<HookKind, string> = {
  "file": "Read a file from disk and inject its contents.",
  "webhook": "GET a URL, inject the response body (max 16KB).",
  "mcp-call": "Call an MCP tool and use its result.",
  "db-query": "Run a SQL query and inject the result.",
  "computed": "Tiny JS expression evaluated at dispatch time.",
};

export default function ContextTab({ agent }: Props) {
  return (
    <TierGate feature="context-hooks">
      <ContextEditor agent={agent} />
    </TierGate>
  );
}

function ContextEditor({ agent }: { agent: Agent }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState<AgentHook | "new" | null>(null);

  const { data: hooks = [], isLoading, error } = useQuery({
    queryKey: ["agent-hooks", agent.id],
    queryFn: () => listAgentHooks(agent.id),
    staleTime: 5_000,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteAgentHook(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["agent-hooks", agent.id] });
    },
  });

  return (
    <div className="space-y-5">
      <header>
        <div className="flex items-center gap-2">
          <Layers size={16} className="text-cs-accent" />
          <h3 className="text-sm font-medium text-cs-text">
            {t("agentDetail.context.title", "Pre-call context hooks")}
          </h3>
        </div>
        <p className="mt-1 text-xs text-cs-muted">
          {t(
            "agentDetail.context.subtitle",
            "Each hook fetches data before every dispatch and injects the result into a <context> block in the user message. The CRM-as-context pattern, without writing a server."
          )}
        </p>
      </header>

      {error && (
        <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
          <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
          <span>{error instanceof Error ? error.message : String(error)}</span>
        </div>
      )}

      <div className="space-y-2">
        {isLoading ? (
          <div className="flex items-center justify-center h-20">
            <Loader2 size={16} className="animate-spin text-cs-muted" />
          </div>
        ) : hooks.length === 0 && editing !== "new" ? (
          <EmptyState onAdd={() => setEditing("new")} />
        ) : (
          hooks.map((h) => (
            <HookRow
              key={h.id}
              hook={h}
              onEdit={() => setEditing(h)}
              onDelete={() => deleteMutation.mutate(h.id)}
              deleting={deleteMutation.isPending && deleteMutation.variables === h.id}
            />
          ))
        )}
      </div>

      {hooks.length > 0 && editing !== "new" && (
        <button
          type="button"
          onClick={() => setEditing("new")}
          className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
        >
          <Plus size={12} />
          {t("agentDetail.context.add", "Add hook")}
        </button>
      )}

      {editing && (
        <HookEditor
          agentId={agent.id}
          existing={editing === "new" ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({ queryKey: ["agent-hooks", agent.id] });
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
      <Layers size={20} className="text-cs-muted shrink-0" />
      <div className="flex-1 min-w-0">
        <p className="text-sm text-cs-text">
          {t("agentDetail.context.emptyTitle", "Make your agent context-aware")}
        </p>
        <p className="mt-1 text-xs text-cs-muted leading-relaxed">
          {t(
            "agentDetail.context.emptyBody",
            "Pre-call hooks fetch fresh data on every turn and inject it into a <context> block before the user message. A file the agent re-reads, a CRM webhook, an MCP tool — anything that should be current, not stale."
          )}
        </p>
        <p className="mt-2 text-xs text-cs-muted">
          {t(
            "agentDetail.context.emptyKinds",
            "Hook kinds: file · MCP call · database query · webhook · computed."
          )}
        </p>
        <button
          type="button"
          onClick={onAdd}
          className="mt-3 inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
        >
          <Plus size={12} />
          {t("agentDetail.context.add", "Add hook")}
        </button>
      </div>
    </div>
  );
}

function HookRow({
  hook,
  onEdit,
  onDelete,
  deleting,
}: {
  hook: AgentHook;
  onEdit: () => void;
  onDelete: () => void;
  deleting: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const cfg = parseHookConfig(hook);
  const isPro = PRO_HOOK_KINDS.includes(hook.kind);

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
        <span className="text-xs font-mono text-cs-accent shrink-0">{hook.name}</span>
        <span className="text-[10px] uppercase tracking-wide text-cs-muted shrink-0">
          {KIND_LABEL[hook.kind]}
        </span>
        {isPro && <Crown size={10} className="text-cs-accent shrink-0" />}
        <span className="flex-1 truncate text-xs text-cs-muted">{hookSummary(cfg)}</span>
        {!hook.enabled && (
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
{JSON.stringify(JSON.parse(hook.configJson), null, 2)}
        </pre>
      )}
    </div>
  );
}

function hookSummary(cfg: HookConfig): string {
  switch (cfg.kind) {
    case "file": return cfg.path;
    case "webhook": return cfg.url;
    case "mcp-call": return `${cfg.server}.${cfg.tool}`;
    case "db-query": return `${cfg.connection}: ${cfg.sql.slice(0, 40)}…`;
    case "computed": return cfg.expr.slice(0, 60);
  }
}

function HookEditor({
  agentId,
  existing,
  onClose,
  onSaved,
}: {
  agentId: string;
  existing: AgentHook | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const advancedAllowed = useFeatureFlag("variables.advanced");
  const initial: HookConfig = existing
    ? parseHookConfig(existing)
    : { kind: "file", path: "", maxBytes: 8192 };
  const [name, setName] = useState(existing?.name ?? "");
  const [enabled, setEnabled] = useState(existing?.enabled ?? true);
  const [kind, setKind] = useState<HookKind>(initial.kind);
  const [config, setConfig] = useState<HookConfig>(initial);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [proPrompt, setProPrompt] = useState(false);

  const save = async () => {
    if (!name.trim() || saving) return;
    setErr(null);
    setSaving(true);
    try {
      await saveAgentHook({
        id: existing?.id,
        agentId,
        position: existing?.position,
        name: name.trim(),
        kind,
        configJson: hookConfigToJson(config),
        enabled,
      });
      onSaved();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const pickKind = (k: HookKind) => {
    if (PRO_HOOK_KINDS.includes(k) && !advancedAllowed) {
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
            ? t("agentDetail.context.editTitle", "Edit hook")
            : t("agentDetail.context.newTitle", "New hook")}
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

      <Field label={t("agentDetail.context.name", "Name")} required>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="recent_pull_requests"
          className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          autoFocus
        />
      </Field>

      <Field label={t("agentDetail.context.kind", "Hook kind")}>
        <div className="grid grid-cols-2 gap-1.5">
          {[...FREE_HOOK_KINDS, ...PRO_HOOK_KINDS].map((k) => {
            const isPro = PRO_HOOK_KINDS.includes(k);
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

      <UpgradePrompt feature="variables.advanced" open={proPrompt} onClose={() => setProPrompt(false)} />
    </div>
  );
}

function defaultsFor(kind: HookKind): HookConfig {
  switch (kind) {
    case "file": return { kind, path: "", maxBytes: 8192 };
    case "webhook": return { kind, url: "", headers: {}, maxBytes: 16384 };
    case "mcp-call": return { kind, server: "", tool: "", args: {} };
    case "db-query": return { kind, connection: "", sql: "" };
    case "computed": return { kind, expr: "" };
  }
}

function KindConfigEditor({
  kind,
  config,
  onChange,
}: {
  kind: HookKind;
  config: HookConfig;
  onChange: (c: HookConfig) => void;
}) {
  const { t } = useTranslation();
  if (kind === "file" && config.kind === "file") {
    return (
      <>
        <Field label={t("agentDetail.context.fields.filePath", "File path")}>
          <input
            type="text"
            value={config.path}
            onChange={(e) => onChange({ ...config, path: e.target.value })}
            placeholder="~/notes/style-guide.md"
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
        <Field label={t("agentDetail.context.fields.maxBytes", "Max bytes")}>
          <input
            type="number"
            value={config.maxBytes ?? 8192}
            min={256}
            onChange={(e) => onChange({ ...config, maxBytes: parseInt(e.target.value, 10) })}
            className="w-32 rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
      </>
    );
  }
  if (kind === "webhook" && config.kind === "webhook") {
    return (
      <>
        <Field label={t("agentDetail.context.fields.url", "URL")}>
          <input
            type="text"
            value={config.url}
            onChange={(e) => onChange({ ...config, url: e.target.value })}
            placeholder="https://api.example.com/account/123"
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
        <Field
          label={t("agentDetail.context.fields.headers", "Headers (one per line, KEY: value)")}
        >
          <textarea
            rows={3}
            value={Object.entries(config.headers ?? {})
              .map(([k, v]) => `${k}: ${v}`)
              .join("\n")}
            onChange={(e) => {
              const headers: Record<string, string> = {};
              e.target.value
                .split("\n")
                .map((l) => l.trim())
                .filter(Boolean)
                .forEach((line) => {
                  const i = line.indexOf(":");
                  if (i > 0) headers[line.slice(0, i).trim()] = line.slice(i + 1).trim();
                });
              onChange({ ...config, headers });
            }}
            placeholder="Authorization: Bearer xxx"
            className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-sm text-cs-text font-mono focus:border-cs-accent focus:outline-none"
          />
        </Field>
      </>
    );
  }
  return (
    <p className="text-xs text-cs-warning">
      {t(
        "agentDetail.context.fields.proStub",
        "This hook kind is wired in Wave 2.2. Storing the config now is fine; runtime execution will activate when the implementation lands."
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
