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

import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Key, Loader2, Send, Settings, Swords, Terminal, X } from "lucide-react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

import { useUiStore } from "@/stores/useUiStore";
import { runtimeBadge } from "@/components/SessionsList/_helpers";
import {
  useEnabledRuntimes,
  type EnabledRuntimeRow,
} from "@/lib/enabledRuntimes";
import { listAgents, type Agent } from "@/lib/agents";

interface WarRoomDispatchResult {
  warRoomId: string;
  round: number;
}

// v2.7.7 — runtime enablement now flows through useEnabledRuntimes()
// (shared cache, single backend call, mirrors PromptBar). The old
// SUBSCRIPTION_RUNTIMES + DIRECT_PROVIDER_RUNTIMES + computeEnabled-
// Runtimes composition lived here because the wizard pre-dated the
// unified `list_available_runtimes` Tauri command; backend composition
// is now canonical so the frontend duplicate is gone.

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

  const enabledQuery = useEnabledRuntimes();
  const detecting = enabledQuery.isLoading;
  // Filter to only currently-dispatchable rows. `available=false` rows
  // (binary missing, no key) shouldn't show in the war-room counter
  // since the user can't fire to them anyway.
  const enabled: EnabledRuntimeRow[] = useMemo(
    () => (enabledQuery.data ?? []).filter((r) => r.available),
    [enabledQuery.data]
  );

  // 2026-05-19 — Will: pills must be deselectable. Track explicit
  // exclusions only; derive `selected` from enabled \ excluded. War-room
  // (claude + codex, war_room_id F009D1D3…) was unanimous: a
  // reseed-from-enabled effect re-includes deselected runtimes after a
  // health flap. Storing `excluded` keeps manual opt-outs sticky across
  // status churn while still auto-including newly-connected runtimes
  // (they're absent from `excluded` by default).
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const selected = useMemo(() => {
    const out = new Set<string>();
    for (const e of enabled) {
      if (!excluded.has(e.slug)) out.add(e.slug);
    }
    return out;
  }, [enabled, excluded]);

  // v2.7.8 PR-3c — agents-per-seat. Will's dogfood (2026-05-20) caught
  // that war-rooms had ZERO agent surface: dispatch_war_room received
  // only runtime slugs, so agent.permissions never reached the
  // per-seat dispatch and API providers stayed text-only. Each
  // selected runtime can now opt in to one of its agents; default is
  // "no agent" (text-only, preserves prior behaviour).
  const agentsQuery = useQuery({
    queryKey: ["agents"],
    queryFn: () => listAgents(),
  });
  const agentsByRuntime = useMemo(() => {
    const out = new Map<string, Agent[]>();
    for (const a of agentsQuery.data ?? []) {
      const arr = out.get(a.runtime) ?? [];
      arr.push(a);
      out.set(a.runtime, arr);
    }
    return out;
  }, [agentsQuery.data]);
  const [seatAgents, setSeatAgents] = useState<Map<string, string>>(new Map());

  const toggleRuntime = (slug: string) => {
    setExcluded((prev) => {
      const next = new Set(prev);
      if (next.has(slug)) next.delete(slug);
      else next.add(slug);
      return next;
    });
  };

  const fire = useMutation({
    mutationFn: async () => {
      // PR-3c — parallel arrays. Index N of agent_slugs maps to the
      // agent picked for runtimes[N]; null means "no agent" (text-
      // only). Backend defaults to null when array length doesn't
      // match runtimes length, so this is safe even for older agents.
      const runtimeList = Array.from(selected);
      const agentSlugs = runtimeList.map((r) => seatAgents.get(r) ?? null);
      return await invoke<WarRoomDispatchResult>("dispatch_war_room", {
        runtimes: runtimeList,
        agentSlugs,
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
    selected.size > 0 &&
    prompt.trim().length > 0;

  // Navigate to a specific Settings sub-tab so "Add API key" and
  // "Set up CLI subscription" land on the right surface instead of
  // dumping the user on whatever tab Settings was last on.
  // 2026-05-19 war-room (codex) caught: the subtab write was inside
  // the else branch — if a parent passed `onOpenSettings` (e.g.
  // Dashboard does), the subtab routing was skipped entirely and the
  // user landed on whatever Settings tab was last open. Always own
  // the subtab write; the section nav is the optional override.
  const goToSettings = (subTab: "api-keys" | "runtimes") => {
    setSubTab("ato.subtab.settings", subTab);
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
            selected={selected}
            onToggle={toggleRuntime}
            onAddKey={() => goToSettings("api-keys")}
            onAddSubscription={() => goToSettings("runtimes")}
          />

          {selected.size > 0 && (agentsQuery.data?.length ?? 0) > 0 && (
            <SeatAgentsPicker
              selectedRuntimes={Array.from(selected)}
              agentsByRuntime={agentsByRuntime}
              seatAgents={seatAgents}
              onChange={(runtime, slug) =>
                setSeatAgents((prev) => {
                  const next = new Map(prev);
                  if (slug) next.set(runtime, slug);
                  else next.delete(runtime);
                  return next;
                })
              }
            />
          )}

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
                    n: selected.size,
                    count: selected.size,
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

// v2.7.8 PR-3c — collapsible seat-level agent picker. Defaults to
// hidden (no agent rows shown) since most users will fire the war-
// room without agents. One click on "Use my agents" expands the
// panel and shows one row per selected runtime with that runtime's
// agents in a dropdown.
function SeatAgentsPicker({
  selectedRuntimes,
  agentsByRuntime,
  seatAgents,
  onChange,
}: {
  selectedRuntimes: string[];
  agentsByRuntime: Map<string, Agent[]>;
  seatAgents: Map<string, string>;
  onChange: (runtime: string, slug: string | null) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const totalConfigured = seatAgents.size;
  return (
    <div className="rounded-md border border-cs-border/60 bg-cs-bg-raised/40">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center justify-between px-3 py-2 text-[11px] text-cs-muted hover:text-cs-text"
      >
        <span>
          {t("firstChat.useAgentsLabel", "Use my agents")}{" "}
          {totalConfigured > 0 && (
            <span className="ml-1 text-cs-accent">({totalConfigured})</span>
          )}
        </span>
        <span className="text-[10px]">{open ? "▾" : "▸"}</span>
      </button>
      {open && (
        <div className="border-t border-cs-border/60 p-3 space-y-2">
          <p className="text-[10px] text-cs-muted leading-relaxed">
            {t(
              "firstChat.seatAgentsHint",
              "Pick an agent per seat to apply its permissions + persona. Skip to send raw prompts (today's behaviour)."
            )}
          </p>
          {selectedRuntimes.map((runtime) => {
            const agents = agentsByRuntime.get(runtime) ?? [];
            const current = seatAgents.get(runtime) ?? "";
            return (
              <div
                key={runtime}
                className="flex items-center gap-2 text-[11px]"
              >
                <span
                  className={`${runtimeBadge(runtime)} min-w-[80px] justify-center`}
                >
                  {runtime}
                </span>
                {agents.length === 0 ? (
                  <span className="text-cs-muted">
                    {t("firstChat.noAgentsForRuntime", "(no agents)")}
                  </span>
                ) : (
                  <select
                    value={current}
                    onChange={(e) =>
                      onChange(runtime, e.target.value || null)
                    }
                    className="flex-1 rounded border border-cs-border bg-cs-bg-raised px-2 py-1 text-cs-text focus:border-cs-accent focus:outline-none"
                  >
                    <option value="">
                      {t("firstChat.noAgent", "— no agent —")}
                    </option>
                    {agents.map((a) => (
                      <option key={a.id} value={a.slug}>
                        {a.displayName} ({a.slug})
                      </option>
                    ))}
                  </select>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function RuntimeCounter({
  detecting,
  enabled,
  selected,
  onToggle,
  onAddKey,
  onAddSubscription,
}: {
  detecting: boolean;
  enabled: EnabledRuntimeRow[];
  selected: Set<string>;
  onToggle: (slug: string) => void;
  onAddKey: () => void;
  onAddSubscription: () => void;
}) {
  const { t } = useTranslation();
  // Soft "add another" — instead of slamming the user into Settings,
  // surface an inline explainer so they understand we're showing every
  // LLM they've already connected and that adding more means choosing
  // between an API key or a CLI subscription. 2026-05-19 — Will's call.
  const [showAddPanel, setShowAddPanel] = useState(false);

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
      <div className="space-y-2">
        <p className="rounded-md border border-cs-warning/40 bg-cs-warning/10 px-3 py-2 text-xs text-cs-text">
          {t(
            "firstChat.noRuntimes",
            "No LLMs connected yet. You can either add an API key for direct dispatch, or set up a CLI subscription (Claude / Codex / Gemini)."
          )}
        </p>
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={onAddKey}
            className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
          >
            <Key size={12} />
            {t("firstChat.addApiKey", "Add API key")}
          </button>
          <button
            type="button"
            onClick={onAddSubscription}
            className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
          >
            <Terminal size={12} />
            {t("firstChat.addSubscription", "Set up CLI subscription")}
          </button>
        </div>
      </div>
    );
  }
  return (
    <div className="space-y-2 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-cs-muted">
          {t("firstChat.firingTo", "Firing to {{n}}:", { n: selected.size })}
        </span>
        {enabled.map((e) => {
          const isOn = selected.has(e.slug);
          return (
            <button
              key={e.slug}
              type="button"
              onClick={() => onToggle(e.slug)}
              title={
                isOn
                  ? t("firstChat.clickToExclude", "Click to exclude")
                  : t("firstChat.clickToInclude", "Click to include")
              }
              className={
                isOn
                  ? `${runtimeBadge(e.slug)} cursor-pointer hover:opacity-80`
                  : "rounded-md border border-dashed border-cs-border/60 bg-transparent px-2 py-0.5 text-cs-muted hover:text-cs-text hover:border-cs-hover cursor-pointer"
              }
              aria-pressed={isOn}
            >
              {e.slug}
            </button>
          );
        })}
        <button
          type="button"
          onClick={() => setShowAddPanel((v) => !v)}
          className="ml-1 inline-flex items-center gap-1 rounded border border-dashed border-cs-border px-2 py-0.5 text-[11px] text-cs-muted hover:border-cs-hover hover:text-cs-text"
        >
          + {t("firstChat.addAnother", "add another")}
        </button>
      </div>
      {showAddPanel && (
        <div className="rounded-md border border-cs-border bg-cs-bg-raised/60 p-3 space-y-2">
          <p className="text-[11px] text-cs-muted leading-relaxed">
            {t(
              "firstChat.addExplainer",
              "We show every LLM you've already connected. To add another, pick how you want to dispatch:"
            )}
          </p>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={onAddKey}
              className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-card px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
            >
              <Key size={12} />
              {t("firstChat.addApiKey", "Add API key")}
            </button>
            <button
              type="button"
              onClick={onAddSubscription}
              className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-card px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-hover"
            >
              <Terminal size={12} />
              {t("firstChat.addSubscription", "Set up CLI subscription")}
            </button>
            <button
              type="button"
              onClick={() => setShowAddPanel(false)}
              className="ml-auto inline-flex items-center gap-1 rounded px-3 py-1.5 text-[11px] text-cs-muted hover:text-cs-text"
            >
              {t("common.cancel", "Cancel")}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
