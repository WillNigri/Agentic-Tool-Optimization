import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Terminal, Key, Check, X, ExternalLink, Cpu, Bot, Globe, Sparkles, type LucideIcon } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
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
  type RuntimeAuthInfo,
} from "@/lib/runtimeAuth";

// Per-runtime auth info — the badge that says "Subscription" /
// "API Key (anthropic)" combines the user's explicit choice with the
// stored-key signal. Driven by the same Tauri command the dispatch
// path reads, so the badge can't drift from actual behavior.
type RuntimeAuthInfoMap = Partial<Record<RuntimeId, RuntimeAuthInfo>>;

const BYOK_RUNTIMES: RuntimeId[] = ["claude", "codex", "gemini"];

// T6 — Dual-auth UI. Shows for each runtime:
//   - "CLI subscription" card (✓ if CLI detected + healthy)
//   - "API key" card (✓ if any matching provider key isActive)
// Plus a radio toggle picking the ACTIVE method. Both can coexist; the active
// one is what outbound calls and the agent-suggest wizard prefer.

type RuntimeMeta = {
  id: RuntimeId;
  name: string;
  icon: LucideIcon;
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

  // Per-runtime effective auth info from the backend. Refetches when
  // the user changes the radio so the badge updates immediately.
  const { data: authInfoMap = {}, refetch: refetchAuthInfo } = useQuery<RuntimeAuthInfoMap>({
    queryKey: ["runtime-auth-info"],
    queryFn: async () => {
      const out: RuntimeAuthInfoMap = {};
      await Promise.all(
        BYOK_RUNTIMES.map(async (runtime) => {
          try {
            const info = await invoke<RuntimeAuthInfo>("get_runtime_auth_info", {
              runtime,
            });
            out[runtime] = info;
          } catch {
            // ignore
          }
        }),
      );
      return out;
    },
    refetchInterval: 30_000,
    staleTime: 10_000,
  });

  useEffect(() => {
    let cancelled = false;
    loadRuntimeAuth().then((s) => {
      if (!cancelled) setAuthState(s);
    });
    return () => {
      cancelled = true;
    };
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

  const handleSelect = async (runtime: RuntimeId, method: AuthMethod) => {
    // Optimistic local update so the radio flips immediately.
    setAuthState((prev) => ({ ...prev, [runtime]: method }));
    try {
      const next = await setRuntimeAuthMethod(runtime, method);
      setAuthState(next);
      // Effective-mode badge depends on the backend setting, so kick
      // a refresh after the write lands.
      refetchAuthInfo();
    } catch (e) {
      const fresh = await loadRuntimeAuth();
      setAuthState(fresh);
      // eslint-disable-next-line no-console
      console.error("setRuntimeAuthMethod failed", e);
    }
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
            authInfo={authInfoMap[rt.id]}
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
  authInfo,
  onSelect,
  onOpenApiKeys,
}: {
  meta: RuntimeMeta;
  cliStatus: AgentStatus | undefined;
  apiKeyForRuntime: LlmApiKey | undefined;
  activeMethod: AuthMethod | undefined;
  authInfo: RuntimeAuthInfo | undefined;
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
          {authInfo?.supportsByok && eitherReady && (
            <EffectiveModeBadge info={authInfo} />
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

const PROVIDER_LABELS: Record<string, string> = {
  anthropic: "anthropic",
  openai: "openai",
  google: "google",
};

function EffectiveModeBadge({ info }: { info: RuntimeAuthInfo }) {
  const isApiKey = info.effective === "api_key";
  // When the effective mode disagrees with the user's explicit choice
  // (e.g., user picked api_key but no key is stored), surface that
  // tension with an "override" hint. Without this, the badge would
  // silently misrepresent what's about to happen.
  const userChoseApiKey = info.userChoice === "api_key";
  const override = userChoseApiKey && !isApiKey;

  const providerSlug = (() => {
    const r = info.runtime;
    if (r === "claude") return "anthropic";
    if (r === "codex") return "openai";
    if (r === "gemini") return "google";
    return null;
  })();
  const providerLabel = providerSlug ? PROVIDER_LABELS[providerSlug] ?? providerSlug : null;

  const label = isApiKey
    ? providerLabel
      ? `API Key (${providerLabel})`
      : "API Key"
    : "Subscription";

  const tooltip = (() => {
    if (override) {
      return `You picked API Key but no key is stored for ${providerLabel ?? info.runtime} — falling back to subscription. Add a key in Settings → API Keys.`;
    }
    if (isApiKey) {
      return `Next dispatch will use the stored ${providerLabel ?? "API"} key. Anthropic / OpenAI / Google bills the key account directly.`;
    }
    return "Next dispatch will use your CLI OAuth credentials (subscription).";
  })();

  return (
    <span
      title={tooltip}
      className={cn(
        "ml-2 inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium border",
        isApiKey
          ? "bg-emerald-500/10 text-emerald-300 border-emerald-500/30"
          : "bg-sky-500/10 text-sky-300 border-sky-500/30",
        override && "ring-1 ring-amber-500/40",
      )}
    >
      {isApiKey ? <Key size={10} /> : <Terminal size={10} />}
      {label}
      {override && <span className="text-amber-400" aria-hidden="true">!</span>}
    </span>
  );
}
