import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Loader2,
  AlertCircle,
  Cloud,
  Cpu,
  Server,
  FileText,
  Variable,
  Layers,
  Brain,
  Globe,
  Lock,
  Zap,
  Sparkles,
  ChevronDown,
  ChevronRight,
  History as HistoryIcon,
} from "lucide-react";
import type { Agent } from "@/lib/agents";
import { listConfigChanges, type ConfigChange } from "@/lib/cloudConfigChanges";
import { useFeatureFlag } from "@/lib/tier";
import { useAuthStore } from "@/hooks/useAuth";
import { cn } from "@/lib/utils";

// v2.1.0 — Configuration impact ledger UI.
//
// Per-agent timeline of every config change recorded by the cloud
// ledger: model swaps, prompt edits, MCP attaches, role-model overrides.
// This is the read surface; writes happen automatically via the
// recordConfigChange() hooks in lib/agents.ts whenever an update fn
// fires.
//
// Why this matters: when traces show "p95 spiked yesterday" the
// dashboard can pivot here to see "the model was swapped from sonnet-4-6
// to opus-4-7 18 hours before the spike began."

interface Props {
  agent: Agent;
}

// Field-name → friendly label + icon. Anything not in the map renders
// as a generic gear icon with the raw field string.
function fieldMeta(field: string): { label: string; Icon: typeof FileText } {
  switch (field) {
    case "created":       return { label: "Created",        Icon: Sparkles };
    case "model":         return { label: "Model",          Icon: Cpu };
    case "runtime":       return { label: "Runtime",        Icon: Server };
    case "system_prompt": return { label: "System prompt",  Icon: FileText };
    case "description":   return { label: "Description",    Icon: FileText };
    case "variables":     return { label: "Variables",      Icon: Variable };
    case "hooks":         return { label: "Context hooks",  Icon: Layers };
    case "role_models":   return { label: "Per-role models", Icon: Cpu };
    case "memory_policy": return { label: "Memory policy",  Icon: Brain };
    case "kind":          return { label: "Kind",           Icon: Globe };
    case "permissions":   return { label: "Permissions",    Icon: Lock };
    case "mcps":          return { label: "MCPs",           Icon: Zap };
    case "skills":        return { label: "Skills",         Icon: Sparkles };
    default:              return { label: field,            Icon: FileText };
  }
}

export default function HistoryTab({ agent }: Props) {
  const { t } = useTranslation();
  const isPro = useFeatureFlag("cloud-traces");
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const accessToken = useAuthStore((s) => s.accessToken);
  const canQuery = isCloudUser && accessToken;
  const [windowDays, setWindowDays] = useState<7 | 30 | 90>(30);

  const query = useQuery({
    queryKey: ["agent-config-changes", agent.slug, windowDays],
    queryFn: () => listConfigChanges({ agentSlug: agent.slug, days: windowDays, limit: 200 }),
    enabled: !!canQuery && isPro,
    staleTime: 30_000,
  });

  if (!isPro) {
    return (
      <Empty
        icon={<Zap size={20} />}
        title={t("agentDetail.history.proRequired", "Config history is a Pro feature")}
        body={t(
          "agentDetail.history.proBody",
          "Every model swap, prompt edit, and hook change gets logged to the cloud so you can correlate trace regressions with config edits. Pro tier unlocks it.",
        )}
      />
    );
  }

  if (!canQuery) {
    return (
      <Empty
        icon={<Cloud size={20} />}
        title={t("agentDetail.history.signInRequired", "Sign in to view config history")}
        body={t(
          "agentDetail.history.signInBody",
          "History lives on ato-cloud so it's accessible across all your machines. Settings → Cloud → Sign in.",
        )}
      />
    );
  }

  if (query.isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-cs-muted">
        <Loader2 size={18} className="animate-spin mr-2" />
        {t("agentDetail.history.loading", "Loading history…")}
      </div>
    );
  }

  if (query.isError) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
        <span>
          {t("agentDetail.history.error", "Couldn't load history")}: {String(query.error)}
        </span>
      </div>
    );
  }

  const changes = query.data?.changes ?? [];

  return (
    <div className="space-y-4">
      <header className="flex items-center justify-between gap-3">
        <div>
          <h3 className="flex items-center gap-2 text-sm font-medium text-cs-text">
            <HistoryIcon size={14} className="text-cs-accent" />
            {t("agentDetail.history.title", "Configuration history")}
          </h3>
          <p className="mt-0.5 text-[11px] text-cs-muted">
            {t(
              "agentDetail.history.subtitle",
              "Every meaningful edit, with timestamp + actor. Used by the External Insights dashboard to overlay change markers on trace timelines.",
            )}
          </p>
        </div>
        <div className="inline-flex rounded-md border border-cs-border bg-cs-bg-raised p-0.5">
          {([7, 30, 90] as const).map((d) => (
            <button
              key={d}
              type="button"
              onClick={() => setWindowDays(d)}
              className={cn(
                "rounded px-3 py-1.5 text-[11px] font-medium transition",
                windowDays === d
                  ? "bg-cs-accent/15 text-cs-accent"
                  : "text-cs-muted hover:text-cs-text",
              )}
            >
              {d}d
            </button>
          ))}
        </div>
      </header>

      {changes.length === 0 ? (
        <Empty
          icon={<HistoryIcon size={20} />}
          title={t("agentDetail.history.empty", "No changes in this window")}
          body={t(
            "agentDetail.history.emptyBody",
            "Either this agent hasn't been edited in {{n}} days, or the changes happened before v2.1.0 (when ledger recording was added).",
            { n: windowDays },
          )}
        />
      ) : (
        <ol className="relative space-y-1 border-l border-cs-border pl-4 ml-2">
          {changes.map((c) => (
            <ChangeRow key={c.id} change={c} />
          ))}
        </ol>
      )}
    </div>
  );
}

function ChangeRow({ change }: { change: ConfigChange }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const { label, Icon } = fieldMeta(change.field);
  const ts = new Date(change.changed_at);
  const newPreview = previewValue(change.new_value);
  const oldPreview = change.old_value !== null && change.old_value !== undefined
    ? previewValue(change.old_value)
    : null;
  const hasDetail = !!newPreview || !!oldPreview;

  return (
    <li className="relative">
      {/* Timeline dot — sits on the left rail. */}
      <span className="absolute -left-[22px] top-2 flex h-3 w-3 items-center justify-center rounded-full bg-cs-bg-raised ring-2 ring-cs-border">
        <span className="h-1.5 w-1.5 rounded-full bg-cs-accent" />
      </span>
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="w-full text-left rounded-md border border-cs-border bg-cs-bg-raised/40 p-2 hover:border-cs-accent/40"
      >
        <div className="flex items-center gap-2">
          <Icon size={11} className="text-cs-muted shrink-0" />
          <span className="text-xs font-medium text-cs-text">{label}</span>
          <span className="text-[11px] text-cs-muted truncate flex-1">
            {newPreview ?? t("agentDetail.history.changed", "changed")}
          </span>
          <time className="text-[10px] font-mono text-cs-muted shrink-0" dateTime={change.changed_at}>
            {ts.toLocaleString()}
          </time>
          {hasDetail && (
            <span className="text-cs-muted shrink-0">
              {expanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
            </span>
          )}
        </div>
        <div className="mt-0.5 text-[10px] text-cs-muted pl-5">
          {t("agentDetail.history.by", "by")}{" "}
          <code className="font-mono">{change.changed_by}</code>
        </div>
      </button>
      {expanded && hasDetail && (
        <div className="mt-1 ml-2 space-y-1 text-[11px]">
          {oldPreview && (
            <div className="rounded border border-cs-border bg-cs-bg p-2">
              <div className="text-[10px] uppercase tracking-wide text-cs-muted mb-0.5">
                {t("agentDetail.history.was", "Was")}
              </div>
              <pre className="whitespace-pre-wrap break-words text-cs-text font-mono">
                {oldPreview}
              </pre>
            </div>
          )}
          <div className="rounded border border-cs-accent/30 bg-cs-accent/5 p-2">
            <div className="text-[10px] uppercase tracking-wide text-cs-accent mb-0.5">
              {t("agentDetail.history.now", "Now")}
            </div>
            <pre className="whitespace-pre-wrap break-words text-cs-text font-mono">
              {newPreview}
            </pre>
          </div>
        </div>
      )}
    </li>
  );
}

/** Render a JSONB value as a single-line preview (in row) and a
 *  pretty-printed multi-line block (in expanded detail). Returns null
 *  for nullish — the row falls back to "changed". */
function previewValue(v: unknown): string | null {
  if (v === null || v === undefined) return null;
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}

function Empty({
  icon,
  title,
  body,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
}) {
  return (
    <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-6 text-center text-sm">
      <div className="mx-auto mb-2 flex h-10 w-10 items-center justify-center rounded-full bg-cs-accent/10 text-cs-accent">
        {icon}
      </div>
      <p className="text-cs-text font-medium mb-1">{title}</p>
      <p className="text-[12px] text-cs-muted">{body}</p>
    </div>
  );
}
