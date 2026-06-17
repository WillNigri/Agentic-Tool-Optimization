import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Loader2,
  KeyRound,
  CheckCircle2,
  AlertCircle,
  Plus,
  X,
  Cloud,
  Cpu,
} from "lucide-react";
import { listLlmApiKeys, saveLlmApiKey } from "@/lib/tauri-api";
import type { AgentRuntime, AgentKind } from "@/lib/agents";
import { cn } from "@/lib/utils";

// v2.0.0 — Authentication requirements panel for the agent create wizard.
//
// Beatriz feedback (2026-05-08): the Internal/External toggle didn't change
// anything in the form — same fields, same options. This panel is the
// missing piece. Renders right under the kind picker and shows:
//
//   Internal:  "Riding your Claude Code subscription" if detected, OR
//              the list of API keys available, OR a hint that you can use
//              either a CLI subscription or an API key.
//
//   External:  hard requirement — needs an API key (deployed bundles can't
//              ride a local CLI subscription). Lists matching keys; if
//              none exist, shows an inline "Add API key" form so the user
//              can add one without leaving the wizard.

interface Props {
  kind: AgentKind;
  runtime: AgentRuntime;
}

// Map agent runtime → the LLM provider whose key is needed. Self-hosted
// runtimes (openclaw / hermes) don't need API keys; this returns null.
// 2026-05-20 — extended for the v2.3.21 API-key dispatch surface
// (minimax/grok/deepseek/qwen/openrouter); each maps to its provider slug.
function providerForRuntime(runtime: AgentRuntime): string | null {
  switch (runtime) {
    case "claude":     return "anthropic";
    case "codex":      return "openai";
    case "gemini":     return "gemini";
    case "openclaw":   return null; // self-hosted via SSH
    case "hermes":     return null; // self-hosted runtime
    case "minimax":    return "minimax";
    case "grok":       return "xai";
    case "deepseek":   return "deepseek";
    case "qwen":       return "qwen";
    case "openrouter": return "openrouter";
    case "zai":        return "zai";
  }
}

// Friendly display name for the provider's API key.
function providerLabel(provider: string): string {
  switch (provider) {
    case "anthropic": return "Anthropic";
    case "openai":    return "OpenAI";
    case "gemini":    return "Google AI Studio";
    case "groq":      return "Groq";
    case "mistral":   return "Mistral";
    case "deepseek":  return "DeepSeek";
    case "xai":       return "xAI Grok";
    case "together":  return "Together AI";
    case "fireworks": return "Fireworks";
    case "zai":       return "Z.AI";
    default:          return provider;
  }
}

// Where the user can sign up for the key — surfaced as a quick link in the
// inline-add form so they're not lost looking for it.
function signupUrl(provider: string): string {
  switch (provider) {
    case "anthropic": return "https://console.anthropic.com/settings/keys";
    case "openai":    return "https://platform.openai.com/api-keys";
    case "gemini":    return "https://aistudio.google.com/apikey";
    case "groq":      return "https://console.groq.com/keys";
    case "mistral":   return "https://console.mistral.ai/api-keys";
    case "deepseek":  return "https://platform.deepseek.com/api_keys";
    case "xai":       return "https://console.x.ai/";
    case "together":  return "https://api.together.ai/settings/api-keys";
    case "fireworks": return "https://fireworks.ai/account/api-keys";
    case "zai":       return "https://z.ai/manage-apikey/apikey-list";
    default:          return "";
  }
}

/** All chat-capable providers the deploy bundle generators support.
 *  Used for the External-agent variant of AuthRequirements where the
 *  user can pick any of these at Deploy time. */
const CHAT_PROVIDERS = [
  "anthropic", "openai", "gemini", "groq", "mistral",
  "deepseek", "xai", "together", "fireworks",
] as const;

export default function AuthRequirements({ kind, runtime }: Props) {
  const { t } = useTranslation();
  const provider = providerForRuntime(runtime);

  const { data: keys = [], isLoading: keysLoading } = useQuery({
    queryKey: ["llm-api-keys"],
    queryFn: () => listLlmApiKeys(),
    staleTime: 5_000,
  });

  if (keysLoading) {
    return (
      <div className="text-xs text-cs-muted">
        <Loader2 size={11} className="inline animate-spin mr-1" />
        {t("createAgent.auth.loading", "Checking available keys…")}
      </div>
    );
  }

  // External agents can use ANY of the 9 chat-capable providers at deploy
  // time — the runtime selection in the wizard mostly determines the
  // *internal* dispatch path, but external bundles pick their provider in
  // the Deploy tab. So the wizard panel should surface ALL provider keys
  // the user has, not just the one matching the runtime. Beatriz feedback
  // 2026-05-08.
  if (kind === "external") {
    const matching = keys.filter(
      (k) => CHAT_PROVIDERS.includes(k.provider as typeof CHAT_PROVIDERS[number]) && k.isActive,
    );
    return (
      <div className="space-y-2">
        <ExternalProviderSummary count={matching.length} />
        {matching.length > 0 && <KeyList keys={matching} />}
        <InlineAddKey kind="external" />
      </div>
    );
  }

  // Internal agents: runtime → provider mapping is meaningful (the local
  // CLI dispatch path is tied to the runtime). Self-hosted runtimes need
  // no key.
  if (provider === null) {
    return (
      <div className="rounded-lg border border-cs-border bg-cs-bg-raised/40 px-3 py-2 text-[11px] text-cs-muted">
        <span className="inline-flex items-center gap-1.5">
          <Cpu size={11} />
          {t(
            "createAgent.auth.selfHosted",
            "Self-hosted runtime — no LLM API key needed; uses your own infrastructure.",
          )}
        </span>
      </div>
    );
  }

  const matching = keys.filter((k) => k.provider === provider && k.isActive);

  return (
    <div className="space-y-2">
      <KeyAvailability
        kind={kind}
        provider={provider}
        matchingCount={matching.length}
      />
      {matching.length === 0 ? (
        <InlineAddKey kind="internal" provider={provider} />
      ) : (
        <KeyList keys={matching} />
      )}
    </div>
  );
}

function ExternalProviderSummary({ count }: { count: number }) {
  const { t } = useTranslation();
  const have = count > 0;
  return (
    <div
      className={cn(
        "rounded-lg border px-3 py-2 text-[11px]",
        have
          ? "border-cs-accent/40 bg-cs-accent/10 text-cs-accent"
          : "border-cs-warn/40 bg-cs-warn/10 text-cs-text",
      )}
    >
      <span className="inline-flex items-start gap-1.5">
        {have ? <CheckCircle2 size={11} className="mt-0.5 shrink-0" /> : <AlertCircle size={11} className="mt-0.5 shrink-0" />}
        <span>
          {have
            ? t(
                "createAgent.auth.externalAnyReady",
                "{{c}} chat-provider key{{plural}} on file — pick which one at Deploy time. Add more below if you want options.",
                { c: count, plural: count === 1 ? "" : "s" },
              )
            : t(
                "createAgent.auth.externalAnyMissing",
                "External bundles need a chat-LLM key (Anthropic / OpenAI / Gemini / Groq / Mistral / DeepSeek / xAI / Together / Fireworks). Add at least one — you'll pick which to use per-deployment.",
              )}
        </span>
      </span>
    </div>
  );
}

function KeyAvailability({
  kind,
  provider,
  matchingCount,
}: {
  kind: AgentKind;
  provider: string;
  matchingCount: number;
}) {
  const { t } = useTranslation();
  const have = matchingCount > 0;
  const label = providerLabel(provider);

  // External: API key is REQUIRED. Internal: optional (could ride CLI sub).
  if (kind === "external") {
    return (
      <div
        className={cn(
          "rounded-lg border px-3 py-2 text-[11px]",
          have
            ? "border-cs-accent/40 bg-cs-accent/10 text-cs-accent"
            : "border-cs-warn/40 bg-cs-warn/10 text-cs-text",
        )}
      >
        <span className="inline-flex items-center gap-1.5">
          {have ? <CheckCircle2 size={11} /> : <AlertCircle size={11} />}
          {have
            ? t(
                "createAgent.auth.externalReady",
                "{{c}} {{provider}} key{{plural}} on file — the deployed bundle will use it as PROVIDER_API_KEY.",
                { c: matchingCount, provider: label, plural: matchingCount === 1 ? "" : "s" },
              )
            : t(
                "createAgent.auth.externalMissing",
                "External agents need a {{provider}} API key — deployed bundles can't ride your local CLI subscription. Add one below to unlock deploy.",
                { provider: label },
              )}
        </span>
      </div>
    );
  }

  // Internal — softer copy; CLI subscription is also valid.
  return (
    <div
      className={cn(
        "rounded-lg border px-3 py-2 text-[11px]",
        have
          ? "border-cs-accent/40 bg-cs-accent/10 text-cs-accent"
          : "border-cs-border bg-cs-bg-raised text-cs-muted",
      )}
    >
      <span className="inline-flex items-center gap-1.5">
        {have ? <CheckCircle2 size={11} /> : <Cloud size={11} />}
        {have
          ? t(
              "createAgent.auth.internalReady",
              "Your {{provider}} key is on file — agent dispatches will fall back to it if your CLI subscription isn't logged in.",
              { provider: label },
            )
          : t(
              "createAgent.auth.internalCliFirst",
              "Will use your {{provider}}-compatible CLI subscription. Add an API key below if you'd rather pay per-token (useful for company-wide / multi-user agents).",
              { provider: label },
            )}
      </span>
    </div>
  );
}

function KeyList({
  keys,
}: {
  keys: { id: string; name: string; keyPreview: string; provider: string }[];
}) {
  return (
    <div className="space-y-1">
      {keys.map((k) => (
        <div
          key={k.id}
          className="flex items-center gap-2 rounded-md border border-cs-border bg-cs-bg-raised/40 px-2 py-1 text-[11px]"
        >
          <KeyRound size={10} className="text-cs-muted" />
          <span className="text-cs-text font-medium">{k.name}</span>
          <code className="font-mono text-cs-muted">{k.keyPreview}</code>
        </div>
      ))}
    </div>
  );
}

function InlineAddKey({
  kind,
  provider: pinnedProvider,
}: {
  kind: AgentKind;
  /** When set, the form is locked to this provider (Internal path).
   *  When unset (External), the form lets the user pick from CHAT_PROVIDERS. */
  provider?: string;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [keyName, setKeyName] = useState("");
  const [keyValue, setKeyValue] = useState("");
  const [pickedProvider, setPickedProvider] = useState<string>(pinnedProvider ?? "anthropic");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const provider = pinnedProvider ?? pickedProvider;

  const onSave = async () => {
    if (!keyValue.trim()) {
      setError(t("createAgent.auth.errKeyEmpty", "Paste your API key first."));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const name = keyName.trim() || `${providerLabel(provider)} (default)`;
      await saveLlmApiKey(provider, name, keyValue.trim(), undefined, undefined);
      setKeyValue("");
      setKeyName("");
      setOpen(false);
      await queryClient.invalidateQueries({ queryKey: ["llm-api-keys"] });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-[11px] font-medium text-cs-text hover:border-cs-accent/40 hover:text-cs-accent"
      >
        <Plus size={11} />
        {pinnedProvider
          ? t("createAgent.auth.addKey", "Add {{provider}} API key", {
              provider: providerLabel(pinnedProvider),
            })
          : t("createAgent.auth.addAnyKey", "Add a chat-provider API key")}
      </button>
    );
  }

  return (
    <div className="rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3 space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-cs-muted">
          {t("createAgent.auth.addingKey", "Adding {{provider}} key", {
            provider: providerLabel(provider),
          })}
        </span>
        <button
          type="button"
          onClick={() => setOpen(false)}
          className="text-cs-muted hover:text-cs-text"
        >
          <X size={12} />
        </button>
      </div>

      {/* External path lets the user pick which provider the key belongs
          to. Internal path is locked because the runtime already determines
          the provider. */}
      {kind === "external" && !pinnedProvider && (
        <select
          value={pickedProvider}
          onChange={(e) => setPickedProvider(e.target.value)}
          className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-xs text-cs-text [&>option]:bg-cs-bg-raised [&>option]:text-cs-text"
        >
          {CHAT_PROVIDERS.map((p) => (
            <option key={p} value={p}>
              {providerLabel(p)}
            </option>
          ))}
        </select>
      )}

      <input
        type="text"
        value={keyName}
        onChange={(e) => setKeyName(e.target.value)}
        placeholder={t("createAgent.auth.keyName", "Name (optional, e.g. 'Acme prod')")}
        className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-xs text-cs-text"
      />
      <input
        type="password"
        value={keyValue}
        onChange={(e) => setKeyValue(e.target.value)}
        placeholder={t("createAgent.auth.keyValue", "sk-... or your provider's API key")}
        className="w-full rounded-md border border-cs-border bg-cs-bg px-2.5 py-1.5 text-xs text-cs-text font-mono"
        autoFocus
      />
      {error && (
        <div className="text-[11px] text-cs-danger">{error}</div>
      )}
      <div className="flex items-center justify-between gap-2">
        <a
          href={signupUrl(provider)}
          target="_blank"
          rel="noreferrer"
          className="text-[11px] text-cs-accent hover:underline"
        >
          {t("createAgent.auth.signupLink", "Get a key from {{provider}} →", {
            provider: providerLabel(provider),
          })}
        </a>
        <button
          type="button"
          onClick={onSave}
          disabled={saving}
          className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
        >
          {saving ? <Loader2 size={11} className="animate-spin" /> : <KeyRound size={11} />}
          {t("createAgent.auth.saveKey", "Save key")}
        </button>
      </div>
    </div>
  );
}
