// PR-C First-Chat Wizard (2026-05-18) — primary onboarding path.
//
// One screen, one verb. The wizard's whole job is to clear a 60-second
// runway from app launch to the first multi-LLM reply rendering in
// WarRoomDetailView. The 4-seat war-room verdict on the v1 plan
// (war_room_id 258F1FDA…) cut every phase that doesn't appear in the
// Loom — no chooser, no participant picker, no per-runtime persona,
// no title field. Silent auth detection only; we block at 0 enabled
// runtimes (otherwise dispatch_war_room would error anyway), warn at
// 1 (war-rooms shine at 2+, but a one-LLM single-shot still produces
// a valid receipt), and let the user fire when they have ≥1.
//
// Why a dedicated wizard instead of routing the user into the existing
// CreateAgentWizard (which had the "guided" path): CreateAgent's job
// is to mint a persona file. That's downstream of "I have something I
// want my LLMs to discuss." Putting the war-room flow up front matches
// the README/SKILL.md repositioning we shipped this morning ("ATO is
// a local war room"); CreateAgent demotes to a secondary action.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Send, Settings, Swords, X } from "lucide-react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

import {
  queryAllAgentStatuses,
  listLlmApiKeys,
  type AgentStatus,
  type LlmApiKey,
} from "@/lib/tauri-api";
import {
  RUNTIME_TO_PROVIDER,
  type RuntimeId,
} from "@/lib/runtimeAuth";
import { useUiStore } from "@/stores/useUiStore";
import { runtimeBadge } from "@/components/SessionsList/_helpers";

interface WarRoomDispatchResult {
  warRoomId: string;
  round: number;
}

// Subscription-capable runtime slugs the desktop currently surfaces.
// Order is the visual order shown in the counter row. minimax/grok/
// deepseek/qwen aren't here because they have no subscription path —
// they enter the enabled list via the API-key branch below.
const SUBSCRIPTION_RUNTIMES: RuntimeId[] = [
  "claude",
  "codex",
  "gemini",
  "openclaw",
  "hermes",
];

// Provider slug → direct-API runtime slug. Only the direct-dispatch
// providers belong here (the BYOK runtimes — claude/codex/gemini —
// are matched via RUNTIME_TO_PROVIDER instead, so an "anthropic" key
// counts as enabling claude rather than minting a separate row).
const DIRECT_PROVIDER_RUNTIMES: Record<string, string> = {
  minimax: "minimax",
  grok: "grok",
  deepseek: "deepseek",
  qwen: "qwen",
};

interface EnabledRuntime {
  runtime: string;
  source: "subscription" | "api_key";
}

// Build the enabled-runtime list from subscriptions ∪ active API keys.
// Deduplicates: if both a subscription and a key are present for the
// same runtime, the subscription wins (more reliable; no quota cliff).
function computeEnabledRuntimes(
  statuses: AgentStatus[],
  keys: LlmApiKey[]
): EnabledRuntime[] {
  const out = new Map<string, EnabledRuntime>();
  for (const s of statuses) {
    if (s.available && s.healthy) {
      out.set(s.runtime, { runtime: s.runtime, source: "subscription" });
    }
  }
  for (const k of keys) {
    if (!k.isActive) continue;
    const provider = k.provider.toLowerCase();
    // BYOK match: anthropic/openai/google keys enable claude/codex/gemini.
    let matched = false;
    for (const rt of SUBSCRIPTION_RUNTIMES) {
      if (RUNTIME_TO_PROVIDER[rt]?.includes(provider)) {
        if (!out.has(rt)) {
          out.set(rt, { runtime: rt, source: "api_key" });
        }
        matched = true;
        break;
      }
    }
    if (matched) continue;
    // Direct-API match: minimax/grok/deepseek/qwen.
    const direct = DIRECT_PROVIDER_RUNTIMES[provider];
    if (direct && !out.has(direct)) {
      out.set(direct, { runtime: direct, source: "api_key" });
    }
  }
  return Array.from(out.values());
}

interface FirstChatWizardProps {
  open: boolean;
  onClose: () => void;
  onOpenSettings?: () => void;
}

export default function FirstChatWizard({
  open,
  onClose,
  onOpenSettings,
}: FirstChatWizardProps) {
  const { t } = useTranslation();
  const [prompt, setPrompt] = useState("");
  const setSection = useUiStore((s) => s.setSection);
  const setSubTab = useUiStore((s) => s.setSubTab);
  const openSessionDetail = useUiStore((s) => s.openSessionDetail);

  const statusesQuery = useQuery<AgentStatus[]>({
    queryKey: ["agent-statuses"],
    queryFn: queryAllAgentStatuses,
    enabled: open,
    staleTime: 30_000,
  });
  const keysQuery = useQuery<LlmApiKey[]>({
    queryKey: ["llm-api-keys"],
    queryFn: () => listLlmApiKeys(),
    enabled: open,
    staleTime: 30_000,
  });

  const detecting = statusesQuery.isLoading || keysQuery.isLoading;
  const enabled = computeEnabledRuntimes(
    statusesQuery.data ?? [],
    keysQuery.data ?? []
  );

  const fire = useMutation({
    mutationFn: async () => {
      return await invoke<WarRoomDispatchResult>("dispatch_war_room", {
        runtimes: enabled.map((e) => e.runtime),
        prompt: prompt.trim(),
      });
    },
    onSuccess: (result) => {
      // Navigate to Runs → Sessions and pre-open the new war-room so
      // the user lands on N replies as cards (the proof point).
      setSection("runs");
      setSubTab("ato.subtab.runs", "sessions");
      openSessionDetail("war_room", result.warRoomId);
      setPrompt("");
      onClose();
    },
  });

  if (!open) return null;

  const canSend =
    !detecting &&
    !fire.isPending &&
    enabled.length > 0 &&
    prompt.trim().length > 0;

  const handleOpenSettings = () => {
    if (onOpenSettings) onOpenSettings();
    else setSection("settings");
    onClose();
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="first-chat-title"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-xl rounded-2xl border border-cs-border bg-cs-card shadow-2xl flex flex-col">
        <header className="flex items-start justify-between gap-4 p-5 border-b border-cs-border">
          <div className="min-w-0 flex-1 flex items-start gap-3">
            <div className="rounded-lg bg-cs-accent/10 p-2 text-cs-accent">
              <Swords size={20} />
            </div>
            <div className="min-w-0">
              <h2
                id="first-chat-title"
                className="text-base font-semibold text-cs-text"
              >
                {t("firstChat.title", "Start a war room")}
              </h2>
              <p className="mt-0.5 text-xs text-cs-muted">
                {t(
                  "firstChat.subtitle",
                  "Same question, every LLM you've connected, parallel replies."
                )}
              </p>
            </div>
          </div>
          <button
            type="button"
            aria-label={t("common.close", "Close")}
            onClick={onClose}
            className="text-cs-muted hover:text-cs-text shrink-0"
          >
            <X size={18} />
          </button>
        </header>

        <div className="flex-1 p-5 space-y-4">
          <label className="block">
            <span className="sr-only">
              {t("firstChat.promptLabel", "Prompt")}
            </span>
            <textarea
              autoFocus
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              onKeyDown={(e) => {
                if (
                  (e.metaKey || e.ctrlKey) &&
                  e.key === "Enter" &&
                  canSend
                ) {
                  e.preventDefault();
                  fire.mutate();
                }
              }}
              rows={5}
              placeholder={t(
                "firstChat.promptPlaceholder",
                "Ask one question — every LLM will answer independently. See whose answer fits the question best."
              )}
              className="w-full rounded-lg border border-cs-border bg-cs-bg-raised px-3 py-2.5 text-sm text-cs-text placeholder:text-cs-muted focus:border-cs-accent focus:outline-none resize-none"
              disabled={fire.isPending}
            />
          </label>

          <RuntimeCounter
            detecting={detecting}
            enabled={enabled}
            onAdd={handleOpenSettings}
          />

          {fire.isError && (
            <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 px-3 py-2 text-xs text-cs-text">
              {fire.error instanceof Error
                ? fire.error.message
                : t("firstChat.dispatchError", "Could not start war room.")}
            </div>
          )}
        </div>

        <footer className="flex items-center justify-between gap-3 p-5 border-t border-cs-border">
          <p className="text-[11px] text-cs-muted">
            {fire.isPending
              ? t(
                  "firstChat.firing",
                  "Firing in parallel — replies land in the Sessions tab."
                )
              : t("firstChat.cmdEnterHint", "⌘↵ to send")}
          </p>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onClose}
              className="rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
              disabled={fire.isPending}
            >
              {t("common.cancel", "Cancel")}
            </button>
            <button
              type="button"
              onClick={() => fire.mutate()}
              disabled={!canSend}
              className="inline-flex items-center gap-2 rounded-md bg-cs-accent px-4 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {fire.isPending ? (
                <>
                  <Loader2 size={14} className="animate-spin" />
                  {t("firstChat.sending", "Firing…")}
                </>
              ) : (
                <>
                  <Send size={14} />
                  {t("firstChat.send", "Send to {{n}} LLM", {
                    n: enabled.length,
                    count: enabled.length,
                  })}
                </>
              )}
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}

function RuntimeCounter({
  detecting,
  enabled,
  onAdd,
}: {
  detecting: boolean;
  enabled: EnabledRuntime[];
  onAdd: () => void;
}) {
  const { t } = useTranslation();
  if (detecting) {
    return (
      <div className="flex items-center gap-2 text-xs text-cs-muted">
        <Loader2 size={12} className="animate-spin" />
        {t("firstChat.detecting", "Detecting connected LLMs…")}
      </div>
    );
  }
  if (enabled.length === 0) {
    return (
      <div className="flex items-center justify-between gap-3 rounded-md border border-cs-warning/40 bg-cs-warning/10 px-3 py-2 text-xs text-cs-text">
        <span>
          {t(
            "firstChat.noRuntimes",
            "No LLMs connected. Add a CLI subscription or API key to start a war room."
          )}
        </span>
        <button
          type="button"
          onClick={onAdd}
          className="inline-flex items-center gap-1 rounded border border-cs-border bg-cs-bg-raised px-2 py-1 text-[11px] font-medium text-cs-text hover:border-cs-hover whitespace-nowrap"
        >
          <Settings size={11} />
          {t("firstChat.openSettings", "Open Settings")}
        </button>
      </div>
    );
  }
  return (
    <div className="flex flex-wrap items-center gap-2 text-xs">
      <span className="text-cs-muted">
        {t("firstChat.firingTo", "Firing to {{n}}:", { n: enabled.length })}
      </span>
      {enabled.map((e) => (
        <span key={e.runtime} className={runtimeBadge(e.runtime)}>
          {e.runtime}
        </span>
      ))}
      <button
        type="button"
        onClick={onAdd}
        className="ml-1 inline-flex items-center gap-1 rounded border border-dashed border-cs-border px-2 py-0.5 text-[11px] text-cs-muted hover:border-cs-hover hover:text-cs-text"
      >
        + {t("firstChat.addAnother", "add another")}
      </button>
    </div>
  );
}
