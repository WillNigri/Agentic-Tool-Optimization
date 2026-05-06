import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Terminal, Key, Check, X, ExternalLink, Cpu, Bot, Globe, Sparkles } from "lucide-react";
import { cn } from "@/lib/utils";
import { queryAllAgentStatuses, listLlmApiKeys } from "@/lib/api";
import type { AgentStatus, LlmApiKey } from "@/lib/tauri-api";
import {
  loadRuntimeAuth,
  setRuntimeAuthMethod,
  isProviderForRuntime,
  type RuntimeId,
  type AuthMethod,
  type RuntimeAuthState,
} from "@/lib/runtimeAuth";

// T6 — Dual-auth UI. Shows for each runtime:
//   - "CLI subscription" card (✓ if CLI detected + healthy)
//   - "API key" card (✓ if any matching provider key isActive)
// Plus a radio toggle picking the ACTIVE method. Both can coexist; the active
// one is what outbound calls and the agent-suggest wizard prefer.

type RuntimeMeta = {
  id: RuntimeId;
  name: string;
  icon: React.ComponentType<{ size?: number; className?: string }>;
  providerLabel: string; // shown in the API key card
  cliCommand: string;
};

const RUNTIMES: RuntimeMeta[] = [
  { id: "claude",   name: "Claude Code",      icon: Sparkles, providerLabel: "Anthropic", cliCommand: "claude" },
  { id: "codex",    name: "Codex / OpenAI",   icon: Cpu,      providerLabel: "OpenAI",    cliCommand: "codex" },
  { id: "gemini",   name: "Gemini CLI",       icon: Bot,      providerLabel: "Google",    cliCommand: "gemini" },
  { id: "openclaw", name: "OpenClaw",         icon: Globe,    providerLabel: "OpenClaw",  cliCommand: "openclaw" },
  { id: "hermes",   name: "Hermes",           icon: Bot,      providerLabel: "Hermes",    cliCommand: "hermes" },
];

interface Props {
  onOpenApiKeys?: () => void;
}

export default function AuthMethodMatrix({ onOpenApiKeys }: Props) {
  const { t } = useTranslation();
  const [authState, setAuthState] = useState<RuntimeAuthState>({});

  useEffect(() => {
    setAuthState(loadRuntimeAuth());
  }, []);

  const { data: statuses = [] } = useQuery({
    queryKey: ["agent-statuses"],
    queryFn: queryAllAgentStatuses,
    refetchInterval: 30_000,
    staleTime: 10_000,
  });
  const { data: keys = [] } = useQuery({
    queryKey: ["llm-api-keys"],
    queryFn: () => listLlmApiKeys(),
    staleTime: 30_000,
  });

  const handleSelect = (runtime: RuntimeId, method: AuthMethod) => {
    const next = setRuntimeAuthMethod(runtime, method);
    setAuthState(next);
  };

  return (
    <section className="rounded-xl border border-cs-border bg-cs-card p-5 mb-6">
      <header className="mb-4">
        <h3 className="text-sm font-semibold text-cs-text">
          {t("runtimes.authTitle", "Auth methods")}
        </h3>
        <p className="text-xs text-cs-muted mt-1">
          {t(
            "runtimes.authSubtitle",
            "Use your existing CLI subscription (like VS Code rides your GitHub login) or a stored API key. Both can be configured simultaneously — pick which one is active per runtime."
          )}
        </p>
      </header>

      <div className="space-y-3">
        {RUNTIMES.map((rt) => (
          <RuntimeAuthRow
            key={rt.id}
            meta={rt}
            cliStatus={findStatus(statuses, rt.id)}
            apiKeyForRuntime={findKey(keys, rt.id)}
            activeMethod={authState[rt.id]}
            onSelect={(m) => handleSelect(rt.id, m)}
            onOpenApiKeys={onOpenApiKeys}
          />
        ))}
      </div>
    </section>
  );
}

function RuntimeAuthRow({
  meta,
  cliStatus,
  apiKeyForRuntime,
  activeMethod,
  onSelect,
  onOpenApiKeys,
}: {
  meta: RuntimeMeta;
  cliStatus: AgentStatus | undefined;
  apiKeyForRuntime: LlmApiKey | undefined;
  activeMethod: AuthMethod | undefined;
  onSelect: (method: AuthMethod) => void;
  onOpenApiKeys?: () => void;
}) {
  const { t } = useTranslation();
  const Icon = meta.icon;
  const cliReady = !!(cliStatus?.available && cliStatus?.healthy);
  const keyReady = !!apiKeyForRuntime?.isActive;

  // Default the visible "active" method to the only ready option if user hasn't picked.
  const effectiveActive: AuthMethod | undefined =
    activeMethod ?? (cliReady && !keyReady ? "subscription" : !cliReady && keyReady ? "apiKey" : undefined);

  const eitherReady = cliReady || keyReady;

  return (
    <div className="rounded-lg border border-cs-border bg-cs-bg-raised">
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-cs-border">
        <div className="flex items-center gap-2">
          <Icon size={16} className="text-cs-muted" />
          <span className="text-sm font-medium text-cs-text">{meta.name}</span>
          {eitherReady ? (
            <span className="ml-2 inline-flex items-center gap-1 rounded-full bg-cs-accent/10 px-2 py-0.5 text-[10px] font-medium text-cs-accent">
              <Check size={10} />
              {t("runtimes.ready", "Ready")}
            </span>
          ) : (
            <span className="ml-2 inline-flex items-center gap-1 rounded-full bg-cs-muted/10 px-2 py-0.5 text-[10px] font-medium text-cs-muted">
              {t("runtimes.notReady", "Not configured")}
            </span>
          )}
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-3 p-3">
        <AuthCard
          icon={<Terminal size={14} />}
          title={t("runtimes.cliSubscription", "CLI subscription")}
          ready={cliReady}
          active={effectiveActive === "subscription"}
          disabled={!cliReady}
          onSelect={() => onSelect("subscription")}
          detail={
            cliReady
              ? t("runtimes.cliReady", "{{cmd}} detected — riding your existing login", {
                  cmd: meta.cliCommand,
                })
              : t("runtimes.cliNotFound", "{{cmd}} not detected. Install + log in to use this method.", {
                  cmd: meta.cliCommand,
                })
          }
        />
        <AuthCard
          icon={<Key size={14} />}
          title={t("runtimes.apiKey", "API key")}
          ready={keyReady}
          active={effectiveActive === "apiKey"}
          disabled={!keyReady}
          onSelect={() => onSelect("apiKey")}
          detail={
            keyReady
              ? t("runtimes.keyReady", "{{provider}} key configured", { provider: meta.providerLabel })
              : t("runtimes.noKey", "No {{provider}} key.", { provider: meta.providerLabel })
          }
          actionLabel={!keyReady && onOpenApiKeys ? t("runtimes.addKey", "Add key") : undefined}
          onAction={onOpenApiKeys}
        />
      </div>
    </div>
  );
}

function AuthCard({
  icon,
  title,
  ready,
  active,
  disabled,
  onSelect,
  detail,
  actionLabel,
  onAction,
}: {
  icon: React.ReactNode;
  title: string;
  ready: boolean;
  active: boolean;
  disabled: boolean;
  onSelect: () => void;
  detail: string;
  actionLabel?: string;
  onAction?: () => void;
}) {
  return (
    <div
      className={cn(
        "rounded-md border p-3 transition-colors",
        active ? "border-cs-accent bg-cs-accent/5" : "border-cs-border bg-cs-bg",
        !disabled && !active && "hover:border-cs-hover"
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-center gap-2 text-sm font-medium text-cs-text">
          <span className={cn(ready ? "text-cs-accent" : "text-cs-muted")}>{icon}</span>
          {title}
          {ready ? (
            <Check size={12} className="text-cs-accent" />
          ) : (
            <X size={12} className="text-cs-muted" />
          )}
        </div>
        <button
          type="button"
          role="radio"
          aria-checked={active}
          disabled={disabled}
          onClick={onSelect}
          className={cn(
            "h-4 w-4 shrink-0 rounded-full border-2 transition",
            active
              ? "border-cs-accent bg-cs-accent/30"
              : disabled
              ? "border-cs-border opacity-40 cursor-not-allowed"
              : "border-cs-border hover:border-cs-hover"
          )}
          aria-label={`Select ${title}`}
        >
          {active && <span className="block h-full w-full rounded-full bg-cs-accent scale-50" />}
        </button>
      </div>
      <p className="mt-2 text-xs text-cs-muted">{detail}</p>
      {actionLabel && onAction && (
        <button
          type="button"
          onClick={onAction}
          className="mt-2 inline-flex items-center gap-1 text-xs text-cs-accent hover:underline"
        >
          {actionLabel}
          <ExternalLink size={10} />
        </button>
      )}
    </div>
  );
}

function findStatus(statuses: AgentStatus[], runtime: RuntimeId): AgentStatus | undefined {
  return statuses.find((s) => s.runtime.toLowerCase() === runtime);
}

function findKey(keys: LlmApiKey[], runtime: RuntimeId): LlmApiKey | undefined {
  // Prefer keys explicitly tagged for this runtime, then fall back to provider match.
  const byRuntime = keys.find((k) => k.runtime?.toLowerCase() === runtime && k.isActive);
  if (byRuntime) return byRuntime;
  return keys.find((k) => isProviderForRuntime(k.provider, runtime) && k.isActive);
}
