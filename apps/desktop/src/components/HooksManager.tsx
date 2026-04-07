import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Search,
  Plus,
  ChevronDown,
  ChevronRight,
  Globe,
  FolderOpen,
  Trash2,
  Terminal,
  Loader2,
  AlertCircle,
  CheckCircle2,
  RefreshCw,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { getHooks, saveHook, deleteHook, type HookConfig } from "@/lib/api";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type HookEvent =
  | "PreToolUse"
  | "PostToolUse"
  | "Notification"
  | "Stop"
  | "SubagentStop";

interface Hook {
  id: string;
  name: string;
  event: HookEvent;
  command: string;
  matcher?: string;
  timeout?: number;
  scope: "global" | "project";
  enabled: boolean;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const EVENT_TYPES: HookEvent[] = [
  "PreToolUse",
  "PostToolUse",
  "Notification",
  "Stop",
  "SubagentStop",
];

const EVENT_COLORS: Record<HookEvent, string> = {
  PreToolUse: "#FFB800",
  PostToolUse: "#00FFB2",
  Notification: "#a78bfa",
  Stop: "#FF4466",
  SubagentStop: "#3b82f6",
};

// Convert HookConfig from API to our Hook type
function hookConfigToHook(config: HookConfig): Hook {
  return {
    id: config.id,
    name: config.name,
    event: config.event as HookEvent,
    command: config.command,
    matcher: config.matcher ?? undefined,
    timeout: config.timeout ?? undefined,
    scope: config.scope as "global" | "project",
    enabled: config.enabled,
  };
}

// Convert Hook to HookConfig for API
function hookToHookConfig(hook: Hook): HookConfig {
  return {
    id: hook.id,
    name: hook.name,
    event: hook.event,
    command: hook.command,
    matcher: hook.matcher ?? null,
    timeout: hook.timeout ?? null,
    scope: hook.scope,
    enabled: hook.enabled,
  };
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export default function HooksManager() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState<Hook | null>(null);
  const [creatingNew, setCreatingNew] = useState(false);
  const [saveSuccess, setSaveSuccess] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  // Fetch hooks from settings files
  const { data: hooksData = [], isLoading, error, refetch } = useQuery({
    queryKey: ["hooks"],
    queryFn: getHooks,
  });

  // Convert API data to our Hook type
  const hooks: Hook[] = hooksData.map(hookConfigToHook);

  // Save mutation
  const saveMutation = useMutation({
    mutationFn: (hook: Hook) => saveHook(hookToHookConfig(hook)),
    onSuccess: () => {
      setSaveSuccess(true);
      setSaveError(null);
      queryClient.invalidateQueries({ queryKey: ["hooks"] });
      setExpandedId(null);
      setEditDraft(null);
      setCreatingNew(false);
      setTimeout(() => setSaveSuccess(false), 2500);
    },
    onError: (err) => {
      setSaveError(err instanceof Error ? err.message : "Failed to save hook");
    },
  });

  // Delete mutation
  const deleteMutation = useMutation({
    mutationFn: deleteHook,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["hooks"] });
      setExpandedId(null);
      setEditDraft(null);
      setCreatingNew(false);
    },
    onError: (err) => {
      setSaveError(err instanceof Error ? err.message : "Failed to delete hook");
    },
  });

  // ── helpers ──────────────────────────────────────────────────────────

  const filtered = hooks.filter(
    (h) =>
      h.name.toLowerCase().includes(search.toLowerCase()) ||
      h.command.toLowerCase().includes(search.toLowerCase())
  );

  const groupedByEvent = EVENT_TYPES.map((event) => ({
    event,
    hooks: filtered.filter((h) => h.event === event),
  })).filter((g) => g.hooks.length > 0);

  function handleToggle(id: string) {
    const hook = hooks.find((h) => h.id === id);
    if (hook) {
      saveMutation.mutate({ ...hook, enabled: !hook.enabled });
    }
  }

  function handleExpand(hook: Hook) {
    if (expandedId === hook.id) {
      setExpandedId(null);
      setEditDraft(null);
    } else {
      setExpandedId(hook.id);
      setEditDraft({ ...hook });
      setCreatingNew(false);
      setSaveError(null);
    }
  }

  function handleSave() {
    if (!editDraft) return;
    saveMutation.mutate(editDraft);
  }

  function handleDelete(id: string) {
    deleteMutation.mutate(id);
  }

  function handleCancel() {
    setExpandedId(null);
    setEditDraft(null);
    setCreatingNew(false);
    setSaveError(null);
  }

  function handleNewHook() {
    const newHook: Hook = {
      id: `hook-${Date.now()}`,
      name: "",
      event: "PreToolUse",
      command: "",
      scope: "project",
      enabled: true,
    };
    setEditDraft(newHook);
    setExpandedId(newHook.id);
    setCreatingNew(true);
    setSaveError(null);
  }

  // Loading state
  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 size={24} className="text-cs-muted animate-spin" />
      </div>
    );
  }

  // ── render ───────────────────────────────────────────────────────────

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold mb-1">{t("hooks.title")}</h2>
          <p className="text-cs-muted text-sm">{t("hooks.subtitle")}</p>
        </div>
        <button
          onClick={() => refetch()}
          className="p-2 rounded-lg hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
          title="Refresh hooks"
        >
          <RefreshCw size={16} />
        </button>
      </div>

      {/* Error message */}
      {(error || saveError) && (
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/20">
          <AlertCircle size={14} className="text-red-400 shrink-0" />
          <p className="text-xs text-red-300">
            {error ? "Failed to load hooks from settings" : saveError}
          </p>
        </div>
      )}

      {/* Success message */}
      {saveSuccess && (
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-cs-success/10 border border-cs-success/20">
          <CheckCircle2 size={14} className="text-cs-success shrink-0" />
          <p className="text-xs text-cs-success">Hook saved to settings file</p>
        </div>
      )}

      {/* Search */}
      <div className="relative">
        <Search
          size={16}
          className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted"
        />
        <input
          type="text"
          className="input pl-9"
          placeholder={t("hooks.search")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      {/* Hook groups by event */}
      {groupedByEvent.map(({ event, hooks: eventHooks }) => (
        <EventGroup
          key={event}
          event={event}
          hooks={eventHooks}
          expandedId={expandedId}
          editDraft={editDraft}
          onExpand={handleExpand}
          onToggle={handleToggle}
          onDraftChange={setEditDraft}
          onSave={handleSave}
          onCancel={handleCancel}
          onDelete={handleDelete}
        />
      ))}

      {/* New hook being created (not yet saved) */}
      {creatingNew && editDraft && (
        <div className="card">
          <HookEditForm
            draft={editDraft}
            onChange={setEditDraft}
            onSave={handleSave}
            onCancel={handleCancel}
            onDelete={() => handleCancel()}
            isNew
          />
        </div>
      )}

      {/* Empty state */}
      {filtered.length === 0 && !creatingNew && (
        <p className="text-cs-muted text-sm text-center py-8">
          {search ? t("common.noResults") : t("hooks.noHooks")}
        </p>
      )}

      {/* + New Hook button */}
      <button
        onClick={handleNewHook}
        className="w-full flex items-center justify-center gap-2 py-3 rounded-lg border border-dashed border-cs-border text-cs-muted hover:text-cs-accent hover:border-cs-accent/40 transition-colors text-sm"
      >
        <Plus size={16} />
        {t("hooks.createNew")}
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Event Group
// ---------------------------------------------------------------------------

function EventGroup({
  event,
  hooks,
  expandedId,
  editDraft,
  onExpand,
  onToggle,
  onDraftChange,
  onSave,
  onCancel,
  onDelete,
}: {
  event: HookEvent;
  hooks: Hook[];
  expandedId: string | null;
  editDraft: Hook | null;
  onExpand: (hook: Hook) => void;
  onToggle: (id: string) => void;
  onDraftChange: (draft: Hook) => void;
  onSave: () => void;
  onCancel: () => void;
  onDelete: (id: string) => void;
}) {
  const { t } = useTranslation();
  const color = EVENT_COLORS[event];

  return (
    <div>
      <div className="flex items-center gap-2 mb-2">
        <span
          className="w-1 h-4 rounded-full"
          style={{ backgroundColor: color }}
        />
        <h3 className="text-sm font-medium text-cs-muted uppercase tracking-wider">
          {t(`hooks.events.${event}`)}
        </h3>
      </div>

      <div className="space-y-2">
        {hooks.map((hook) => {
          const isExpanded = expandedId === hook.id;

          return (
            <div key={hook.id} className="card overflow-hidden p-0">
              {/* Summary row */}
              <div
                onClick={() => onExpand(hook)}
                className={cn(
                  "flex items-center justify-between gap-4 cursor-pointer p-4 transition-colors",
                  isExpanded
                    ? "border-b border-cs-border"
                    : "hover:bg-cs-card/80"
                )}
              >
                <div className="min-w-0 flex-1 flex items-center gap-2.5">
                  {isExpanded ? (
                    <ChevronDown size={14} className="text-cs-muted shrink-0" />
                  ) : (
                    <ChevronRight size={14} className="text-cs-muted shrink-0" />
                  )}
                  <Terminal
                    size={16}
                    className="shrink-0"
                    style={{ color }}
                  />
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <p className="text-sm font-medium truncate">
                        {hook.name}
                      </p>
                      <ScopeBadge scope={hook.scope} />
                    </div>
                    <p className="text-xs text-cs-muted font-mono truncate">
                      {hook.command}
                    </p>
                    {hook.matcher && (
                      <p className="text-xs text-cs-muted truncate">
                        {t("hooks.matcherLabel")}: {hook.matcher}
                      </p>
                    )}
                  </div>
                </div>

                {/* Toggle */}
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggle(hook.id);
                  }}
                  className={cn(
                    "relative w-9 h-5 rounded-full transition-colors duration-200 shrink-0",
                    hook.enabled ? "bg-cs-accent" : "bg-cs-border"
                  )}
                >
                  <span
                    className={cn(
                      "absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full transition-transform duration-200",
                      hook.enabled && "translate-x-4"
                    )}
                  />
                </button>
              </div>

              {/* Inline detail */}
              {isExpanded && editDraft && editDraft.id === hook.id && (
                <div className="p-4">
                  <HookEditForm
                    draft={editDraft}
                    onChange={onDraftChange}
                    onSave={onSave}
                    onCancel={onCancel}
                    onDelete={() => onDelete(hook.id)}
                  />
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Scope Badge
// ---------------------------------------------------------------------------

function ScopeBadge({ scope }: { scope: "global" | "project" }) {
  const { t } = useTranslation();

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 text-[10px] font-medium px-1.5 py-0.5 rounded-full shrink-0",
        scope === "global"
          ? "bg-blue-500/10 text-blue-400"
          : "bg-cs-accent/10 text-cs-accent"
      )}
    >
      {scope === "global" ? (
        <Globe size={10} />
      ) : (
        <FolderOpen size={10} />
      )}
      {t(`hooks.scope.${scope}`)}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Hook Edit Form (inline)
// ---------------------------------------------------------------------------

function HookEditForm({
  draft,
  onChange,
  onSave,
  onCancel,
  onDelete,
  isNew = false,
}: {
  draft: Hook;
  onChange: (draft: Hook) => void;
  onSave: () => void;
  onCancel: () => void;
  onDelete: () => void;
  isNew?: boolean;
}) {
  const { t } = useTranslation();

  function update(patch: Partial<Hook>) {
    onChange({ ...draft, ...patch });
  }

  return (
    <div className="space-y-4">
      {/* Name */}
      <div>
        <label className="block text-xs text-cs-muted mb-1">
          {t("hooks.fields.name")}
        </label>
        <input
          type="text"
          className="input"
          value={draft.name}
          onChange={(e) => update({ name: e.target.value })}
          placeholder={t("hooks.fields.namePlaceholder")}
        />
      </div>

      {/* Event type */}
      <div>
        <label className="block text-xs text-cs-muted mb-1">
          {t("hooks.fields.event")}
        </label>
        <select
          className="input"
          value={draft.event}
          onChange={(e) => update({ event: e.target.value as HookEvent })}
        >
          {EVENT_TYPES.map((ev) => (
            <option key={ev} value={ev}>
              {ev}
            </option>
          ))}
        </select>
      </div>

      {/* Command */}
      <div>
        <label className="block text-xs text-cs-muted mb-1">
          {t("hooks.fields.command")}
        </label>
        <input
          type="text"
          className="input font-mono"
          value={draft.command}
          onChange={(e) => update({ command: e.target.value })}
          placeholder={t("hooks.fields.commandPlaceholder")}
        />
      </div>

      {/* Matcher */}
      <div>
        <label className="block text-xs text-cs-muted mb-1">
          {t("hooks.fields.matcher")}
        </label>
        <input
          type="text"
          className="input"
          value={draft.matcher ?? ""}
          onChange={(e) =>
            update({ matcher: e.target.value || undefined })
          }
          placeholder={t("hooks.fields.matcherHint")}
        />
        <p className="text-[10px] text-cs-muted mt-1">
          {t("hooks.fields.matcherDescription")}
        </p>
      </div>

      {/* Timeout */}
      <div>
        <label className="block text-xs text-cs-muted mb-1">
          {t("hooks.fields.timeout")}
        </label>
        <input
          type="number"
          className="input w-32"
          value={draft.timeout ?? ""}
          onChange={(e) =>
            update({
              timeout: e.target.value ? Number(e.target.value) : undefined,
            })
          }
          placeholder="ms"
          min={0}
        />
      </div>

      {/* Scope toggle */}
      <div>
        <label className="block text-xs text-cs-muted mb-1">
          {t("hooks.fields.scope")}
        </label>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => update({ scope: "project" })}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors",
              draft.scope === "project"
                ? "bg-cs-accent/15 text-cs-accent border border-cs-accent/30"
                : "bg-cs-card border border-cs-border text-cs-muted hover:text-cs-text"
            )}
          >
            <FolderOpen size={12} />
            {t("hooks.scope.project")}
          </button>
          <button
            type="button"
            onClick={() => update({ scope: "global" })}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors",
              draft.scope === "global"
                ? "bg-blue-500/15 text-blue-400 border border-blue-500/30"
                : "bg-cs-card border border-cs-border text-cs-muted hover:text-cs-text"
            )}
          >
            <Globe size={12} />
            {t("hooks.scope.global")}
          </button>
        </div>
      </div>

      {/* Actions */}
      <div className="flex items-center justify-between pt-2 border-t border-cs-border">
        <div className="flex items-center gap-2">
          <button onClick={onSave} className="btn-primary text-sm">
            {t("common.save")}
          </button>
          <button onClick={onCancel} className="btn-secondary text-sm">
            {t("common.cancel")}
          </button>
        </div>
        {!isNew && (
          <button
            onClick={onDelete}
            className="flex items-center gap-1 text-xs text-cs-danger hover:text-red-400 transition-colors"
          >
            <Trash2 size={14} />
            {t("common.delete")}
          </button>
        )}
      </div>
    </div>
  );
}
