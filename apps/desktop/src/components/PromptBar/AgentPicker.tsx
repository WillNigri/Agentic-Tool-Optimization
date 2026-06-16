// PromptBar/AgentPicker.tsx — agent / group selector popover.
//
// Extracted from PromptBar/index.tsx 2026-05-19 (v2.7.7 frontend
// elegance push). Lets the user pick a single agent OR a group (the
// two are mutually exclusive) OR "no agent" for single-shot dispatch.
//
// The orchestrator owns:
//   - agentId / setAgentId, groupSlug / setGroupSlug — selection state
//   - selectedAgent, selectedGroup — derived from the two ids above
//   - runtimeAgents, runtimeGroups — runtime-scoped picker source
//   - stickAgentToThread — persists the selection on the current
//     chat-thread row so the next time the thread is opened the
//     same agent/group is restored
//   - open / setOpen — shared `openPicker` mutex state so only one
//     popover is open at a time

import { Bot, Check, Network, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";
import { parseMemoryPolicy, type Agent } from "@/lib/agents";
import type { AgentGroup } from "@/lib/agentGroups";

interface Props {
  /** Current runtime (claude/codex/...) — used to scope the picker
   *  source AND to render the empty-state runtime label. */
  runtime: string;
  agentId: string | null;
  setAgentId: (id: string | null) => void;
  groupSlug: string | null;
  setGroupSlug: (slug: string | null) => void;
  selectedAgent: Agent | null;
  selectedGroup: AgentGroup | null;
  runtimeAgents: Agent[];
  runtimeGroups: AgentGroup[];
  /** Called whenever the user picks (or clears) an agent so the
   *  current chat thread persists the selection. Pass null to clear. */
  stickAgentToThread: (agentId: string | null) => Promise<void>;
  open: boolean;
  setOpen: (next: boolean | ((v: boolean) => boolean)) => void;
  /**
   * #83 — current model override from ModelPicker (when the runtime is an
   * API provider). When BOTH this and selectedAgent.model are set,
   * the picker wins — but the user has no way to tell that from looking
   * at the chips. We use this in the chip's `title` to surface the
   * precedence on hover ("@slug · prefers gpt-4o, using gpt-4.1 from picker").
   */
  modelOverride?: string | null;
}

export function AgentPicker({
  runtime,
  agentId,
  setAgentId,
  groupSlug,
  setGroupSlug,
  selectedAgent,
  selectedGroup,
  modelOverride,
  runtimeAgents,
  runtimeGroups,
  stickAgentToThread,
  open,
  setOpen,
}: Props) {
  const { t } = useTranslation();

  // #83 — precedence-aware tooltip + visual cue. When both an agent
  // (with a stored model) and a model-picker override are set AND they
  // differ, surface that on hover and tint the chip amber so the user
  // sees at a glance that their dispatch will run with the picker's
  // model — not the agent's stored preference. No clutter, no extra
  // copy in the chip itself; just a richer title attribute and a
  // distinct border colour.
  const agentModel = selectedAgent?.model ?? null;
  const precedenceMismatch =
    !!agentModel && !!modelOverride && agentModel !== modelOverride;
  const chipTitle = precedenceMismatch
    ? t(
        "prompt.agentPickerTitleOverride",
        "@{{slug}} prefers {{agentModel}} · using {{overrideModel}} from the model picker",
        {
          slug: selectedAgent?.slug,
          agentModel,
          overrideModel: modelOverride,
        },
      )
    : agentModel
      ? t(
          "prompt.agentPickerTitleAgentModel",
          "@{{slug}} · model: {{agentModel}}",
          { slug: selectedAgent?.slug, agentModel },
        )
      : t("prompt.agentPickerTitle", "Pick an agent or group");

  return (
    <div className="relative shrink-0">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        data-demo-id="agent-picker"
        className={cn(
          "flex items-center gap-1 px-2 py-1.5 rounded-lg border transition-colors",
          selectedAgent || selectedGroup
            ? precedenceMismatch
              ? "border-amber-500/40 bg-amber-500/5"
              : "border-cs-accent/40 bg-cs-accent/5"
            : "border-cs-border hover:border-cs-border/80",
        )}
        title={chipTitle}
      >
        {selectedGroup ? (
          <Network size={12} className="text-cs-accent" />
        ) : (
          <Bot
            size={12}
            className={selectedAgent ? "text-cs-accent" : "text-cs-muted"}
          />
        )}
        <span
          className={cn(
            "text-[10px] font-medium font-mono",
            selectedAgent || selectedGroup
              ? "text-cs-accent"
              : "text-cs-muted",
          )}
        >
          {selectedGroup
            ? `${selectedGroup.slug}/`
            : selectedAgent
              ? `@${selectedAgent.slug}`
              : t("prompt.noAgent", "no agent")}
        </span>
      </button>

      {open && (
        <>
          <div
            className="fixed inset-0 z-30"
            onClick={() => setOpen(false)}
          />
          <div className="absolute bottom-full left-0 mb-1 w-72 max-h-80 overflow-y-auto rounded-lg border border-cs-border bg-cs-card shadow-xl z-40">
            {/* No-agent / single-shot row */}
            <button
              type="button"
              onClick={() => {
                setAgentId(null);
                setGroupSlug(null);
                setOpen(false);
                void stickAgentToThread(null);
              }}
              className={cn(
                "w-full flex items-center gap-2 px-3 py-2 text-xs transition-colors border-b border-cs-border",
                !agentId && !groupSlug
                  ? "bg-cs-accent/5 text-cs-accent"
                  : "text-cs-muted hover:bg-cs-bg",
              )}
            >
              {!agentId && !groupSlug ? <Check size={11} /> : <X size={11} />}
              <span>{t("prompt.noAgent", "no agent")}</span>
              <span className="ml-auto text-[9px] text-cs-muted">
                single-shot
              </span>
            </button>

            {/* Groups section — shown above individual agents because
                they're the more powerful primitive (router-routed
                dispatch per prompt). */}
            {runtimeGroups.length > 0 && (
              <>
                <div className="px-3 py-1.5 text-[9px] uppercase tracking-wider text-cs-muted bg-cs-bg-raised/40 border-b border-cs-border">
                  {t("prompt.groupsHeader", "Groups · routed dispatch")}
                </div>
                {runtimeGroups.map((g) => {
                  const isActive = groupSlug === g.slug;
                  const childCount = g.members.filter(
                    (m) => m.role === "child",
                  ).length;
                  return (
                    <button
                      key={g.id}
                      type="button"
                      onClick={() => {
                        setGroupSlug(g.slug);
                        setAgentId(null);
                        setOpen(false);
                        void stickAgentToThread(null);
                      }}
                      className={cn(
                        "w-full flex items-start gap-2 px-3 py-2 text-xs transition-colors text-left border-b border-cs-border/40",
                        isActive ? "bg-cs-accent/5" : "hover:bg-cs-bg",
                      )}
                    >
                      <Network
                        size={11}
                        className={cn(
                          "shrink-0 mt-0.5",
                          isActive ? "text-cs-accent" : "text-cs-muted",
                        )}
                      />
                      <div className="min-w-0 flex-1">
                        <code
                          className={cn(
                            "font-mono truncate",
                            isActive ? "text-cs-accent" : "text-cs-text",
                          )}
                        >
                          {g.slug}
                        </code>
                        <p className="text-[9px] text-cs-muted truncate">
                          {t(
                            "prompt.groupChildren",
                            "{{n}} children · router routes per prompt",
                            { n: childCount },
                          )}
                        </p>
                      </div>
                    </button>
                  );
                })}
              </>
            )}

            {/* Individual agents */}
            {runtimeAgents.length > 0 && (
              <div className="px-3 py-1.5 text-[9px] uppercase tracking-wider text-cs-muted bg-cs-bg-raised/40 border-b border-cs-border">
                {t("prompt.agentsHeader", "Agents")}
              </div>
            )}
            {runtimeAgents.length === 0 && runtimeGroups.length === 0 ? (
              <p className="px-3 py-3 text-[11px] text-cs-muted">
                {t(
                  "prompt.noAgentsForRuntime",
                  "No agents created for {{runtime}} yet.",
                  { runtime },
                )}
              </p>
            ) : (
              runtimeAgents.map((a) => {
                const policy = parseMemoryPolicy(a);
                return (
                  <button
                    key={a.id}
                    type="button"
                    onClick={() => {
                      setAgentId(a.id);
                      setGroupSlug(null);
                      setOpen(false);
                      void stickAgentToThread(a.id);
                    }}
                    className={cn(
                      "w-full flex items-start gap-2 px-3 py-2 text-xs transition-colors text-left",
                      agentId === a.id ? "bg-cs-accent/5" : "hover:bg-cs-bg",
                    )}
                  >
                    <Bot
                      size={11}
                      className={cn(
                        "shrink-0 mt-0.5",
                        agentId === a.id ? "text-cs-accent" : "text-cs-muted",
                      )}
                    />
                    <div className="min-w-0 flex-1">
                      <code
                        className={cn(
                          "font-mono truncate",
                          agentId === a.id ? "text-cs-accent" : "text-cs-text",
                        )}
                      >
                        @{a.slug}
                      </code>
                      <p className="text-[9px] text-cs-muted truncate">
                        {t(
                          "prompt.summarizesAfter",
                          "summarizes after {{n}} msgs · keeps last {{k}}",
                          {
                            n: policy.summarizeAfter,
                            k: policy.keepLastK,
                          },
                        )}
                      </p>
                    </div>
                  </button>
                );
              })
            )}
          </div>
        </>
      )}
    </div>
  );
}
