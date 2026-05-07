import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { X, Terminal, Cpu, Server, Globe, Bot, Network, FileCode, Calendar, Clock, Info, Moon } from "lucide-react";
import { cn } from "@/lib/utils";
import type { AgentRuntime } from "./types";
import { validateCron, cronToHuman } from "@/lib/cron-utils";
import { useCronStore } from "@/stores/useCronStore";
import { listAgents, type Agent } from "@/lib/agents";
import { listAgentGroups, type AgentGroup } from "@/lib/agentGroups";
import { cronOsSchedulerSupported, cronOsSchedulerKind } from "@/lib/tauri-api";

// v1.5.0 — Cron job creation reframed: pick an AGENT or GROUP first,
// supply the message it should receive on each fire. Agents already carry
// runtime + system prompt + variables + hooks + memory + skills + MCPs +
// permissions, so the cron is "fire @security-reviewer with this prompt"
// instead of "claude with raw prompt." Backwards-compatible "Raw prompt"
// path stays as an escape hatch.

const RUNTIMES: { id: AgentRuntime; label: string; color: string; Icon: typeof Terminal }[] = [
  { id: "claude", label: "Claude", color: "#f97316", Icon: Terminal },
  { id: "codex", label: "Codex", color: "#22c55e", Icon: Cpu },
  { id: "openclaw", label: "OpenClaw", color: "#06b6d4", Icon: Server },
  { id: "hermes", label: "Hermes", color: "#a855f7", Icon: Globe },
];

const SCHEDULE_PRESETS: { id: string; label: string; cron: string }[] = [
  { id: "weekday-9am", label: "Every weekday at 9am", cron: "0 9 * * 1-5" },
  { id: "daily-7am",   label: "Every day at 7am",     cron: "0 7 * * *" },
  { id: "daily-6pm",   label: "Every day at 6pm",     cron: "0 18 * * *" },
  { id: "hourly",      label: "Every hour",            cron: "0 * * * *" },
  { id: "every-15",    label: "Every 15 minutes",      cron: "*/15 * * * *" },
  { id: "weekly-mon",  label: "Mondays at 9am",        cron: "0 9 * * 1" },
  { id: "monthly-1",   label: "1st of every month, 9am", cron: "0 9 1 * *" },
];

type DispatchKind = "agent" | "group" | "raw";

interface CreateCronJobModalProps {
  onClose: () => void;
}

export default function CreateCronJobModal({ onClose }: CreateCronJobModalProps) {
  const { t } = useTranslation();
  const createJob = useCronStore((s) => s.createJob);

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");

  // Schedule — preset by default, custom cron as advanced.
  const [scheduleMode, setScheduleMode] = useState<"preset" | "custom">("preset");
  const [presetId, setPresetId] = useState<string>(SCHEDULE_PRESETS[0].id);
  const [customCron, setCustomCron] = useState("0 7 * * *");

  // Dispatch target — agent / group / raw. Agent is default.
  const [dispatchKind, setDispatchKind] = useState<DispatchKind>("agent");
  const [selectedAgentId, setSelectedAgentId] = useState<string>("");
  const [selectedGroupSlug, setSelectedGroupSlug] = useState<string>("");

  // Raw fallback fields
  const [rawRuntime, setRawRuntime] = useState<AgentRuntime>("claude");
  const [prompt, setPrompt] = useState("");

  // Wake-from-sleep — backed by launchd (macOS), systemd --user (Linux),
  // or Task Scheduler (Windows). Disabled on unsupported platforms.
  const [osSchedulerSupported, setOsSchedulerSupported] = useState(false);
  const [osSchedulerKind, setOsSchedulerKind] = useState<string>("unsupported");
  const [wakeFromSleep, setWakeFromSleep] = useState(false);
  useEffect(() => {
    cronOsSchedulerSupported().then(setOsSchedulerSupported).catch(() => setOsSchedulerSupported(false));
    cronOsSchedulerKind().then(setOsSchedulerKind).catch(() => setOsSchedulerKind("unsupported"));
  }, []);

  const osSchedulerLabel =
    osSchedulerKind === "launchd" ? "macOS · launchd" :
    osSchedulerKind === "systemd-user" ? "Linux · systemd --user" :
    osSchedulerKind === "schtasks" ? "Windows · Task Scheduler" :
    "unsupported";

  const { data: agents = [] } = useQuery({
    queryKey: ["cron-agents"],
    queryFn: () => listAgents(),
    staleTime: 30_000,
  });
  const { data: groups = [] } = useQuery({
    queryKey: ["cron-groups"],
    queryFn: () => listAgentGroups(),
    staleTime: 30_000,
  });

  const selectedAgent = useMemo<Agent | null>(
    () => agents.find((a) => a.id === selectedAgentId) ?? null,
    [agents, selectedAgentId]
  );
  const selectedGroup = useMemo<AgentGroup | null>(
    () => groups.find((g) => g.slug === selectedGroupSlug) ?? null,
    [groups, selectedGroupSlug]
  );

  const schedule = scheduleMode === "preset"
    ? SCHEDULE_PRESETS.find((p) => p.id === presetId)?.cron ?? "0 7 * * *"
    : customCron;

  const cronError = validateCron(schedule);

  const dispatchValid =
    dispatchKind === "agent" ? !!selectedAgent :
    dispatchKind === "group" ? !!selectedGroup :
    !!prompt.trim();

  const isValid = name.trim() && !cronError && dispatchValid;

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!isValid) return;

    // Determine the runtime stored on the job. For agent/group we store
    // the underlying runtime so existing UI (calendar, cards) keeps working.
    const effectiveRuntime: AgentRuntime =
      dispatchKind === "agent" ? (selectedAgent!.runtime as AgentRuntime) :
      dispatchKind === "group" ? (selectedGroup!.runtime as AgentRuntime) :
      rawRuntime;

    createJob({
      name: name.trim(),
      description: description.trim(),
      schedule: schedule.trim(),
      runtime: effectiveRuntime,
      prompt: prompt.trim(),
      enabled: true,
      wakeFromSleep: osSchedulerSupported && wakeFromSleep,
      ...(dispatchKind === "agent" && selectedAgent ? { agentSlug: selectedAgent.slug } : {}),
      ...(dispatchKind === "group" && selectedGroup ? { groupSlug: selectedGroup.slug } : {}),
    });

    onClose();
  }

  return (
    <>
      <div className="fixed inset-0 bg-black/50 z-50" onClick={onClose} />
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <div
          className="bg-cs-card border border-cs-border rounded-xl w-full max-w-xl max-h-[90vh] overflow-y-auto shadow-2xl"
          onClick={(e) => e.stopPropagation()}
        >
          <div className="flex items-center justify-between p-4 border-b border-cs-border">
            <h3 className="text-lg font-semibold">{t("cron.create.title", "Create Cron Job")}</h3>
            <button
              onClick={onClose}
              className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
            >
              <X size={16} />
            </button>
          </div>

          <form onSubmit={handleSubmit} className="p-4 space-y-4">
            {/* Wake-from-sleep — actual toggle on macOS, honesty banner everywhere else. */}
            {osSchedulerSupported ? (
              <label className="flex items-start gap-2.5 rounded-md border border-cs-border bg-cs-bg-raised/50 p-2.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={wakeFromSleep}
                  onChange={(e) => setWakeFromSleep(e.target.checked)}
                  className="mt-0.5 accent-cs-accent"
                />
                <div className="flex-1">
                  <span className="inline-flex items-center gap-1.5 text-xs font-medium text-cs-text">
                    <Moon size={12} className="text-cs-accent" />
                    {t("cron.create.wakeFromSleep", "Wake from sleep")}
                    <span className="text-[10px] font-mono text-cs-muted">{osSchedulerLabel}</span>
                  </span>
                  <p className="text-[11px] text-cs-muted leading-relaxed mt-0.5">
                    {t(
                      "cron.create.wakeFromSleepHint",
                      "Registers the job with the OS scheduler so it fires even when ATO is closed."
                    )}
                  </p>
                </div>
              </label>
            ) : (
              <div className="flex items-start gap-2 rounded-md border border-cs-border bg-cs-bg-raised/50 p-2.5">
                <Info size={12} className="text-cs-muted shrink-0 mt-0.5" />
                <p className="text-[11px] text-cs-muted leading-relaxed">
                  {t(
                    "cron.create.wakeNotice",
                    "Scheduled jobs only fire while ATO is open on this platform."
                  )}
                </p>
              </div>
            )}

            {/* Name */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("cron.create.name", "Job name")}
              </label>
              <input
                type="text"
                className="input"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={t("cron.create.namePlaceholder", "e.g., Daily security review")}
                required
              />
            </div>

            {/* Description */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("cron.create.description", "Description")}
              </label>
              <input
                type="text"
                className="input"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder={t("cron.create.descriptionPlaceholder", "What does this job do?")}
              />
            </div>

            {/* Schedule — friendly picker by default */}
            <div>
              <div className="flex items-center justify-between mb-1">
                <label className="text-xs font-medium text-cs-muted uppercase tracking-wider">
                  {t("cron.create.schedule", "Schedule")}
                </label>
                <button
                  type="button"
                  onClick={() => setScheduleMode((m) => (m === "preset" ? "custom" : "preset"))}
                  className="text-[10px] text-cs-muted hover:text-cs-accent"
                >
                  {scheduleMode === "preset"
                    ? t("cron.create.customCron", "Use cron expression instead")
                    : t("cron.create.usePresets", "Use preset schedule")}
                </button>
              </div>

              {scheduleMode === "preset" ? (
                <div className="space-y-1.5">
                  <select
                    value={presetId}
                    onChange={(e) => setPresetId(e.target.value)}
                    className="input"
                  >
                    {SCHEDULE_PRESETS.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.label}
                      </option>
                    ))}
                  </select>
                  <p className="text-[10px] text-cs-muted font-mono flex items-center gap-1.5">
                    <Calendar size={10} />
                    cron: {schedule}
                  </p>
                </div>
              ) : (
                <div>
                  <input
                    type="text"
                    className={cn("input font-mono", cronError && customCron.trim() ? "border-red-500/50" : "")}
                    value={customCron}
                    onChange={(e) => setCustomCron(e.target.value)}
                    placeholder="0 7 * * *"
                  />
                  <div className="flex items-center justify-between mt-1">
                    <p className="text-[10px] text-cs-muted">{t("cron.create.scheduleHint", "minute hour day-of-month month day-of-week")}</p>
                    {customCron.trim() && !cronError && (
                      <p className="text-[10px] text-cs-accent flex items-center gap-1">
                        <Clock size={10} />
                        {cronToHuman(customCron)}
                      </p>
                    )}
                    {customCron.trim() && cronError && (
                      <p className="text-[10px] text-red-400">{cronError}</p>
                    )}
                  </div>
                </div>
              )}
            </div>

            {/* What runs — Agent / Group / Raw */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("cron.create.runs", "What runs")}
              </label>
              <div className="grid grid-cols-3 gap-1.5 mb-2">
                <KindButton
                  active={dispatchKind === "agent"}
                  onClick={() => setDispatchKind("agent")}
                  icon={<Bot size={12} />}
                  label={t("cron.create.kindAgent", "Agent")}
                  hint={t("cron.create.kindAgentHint", "recommended")}
                />
                <KindButton
                  active={dispatchKind === "group"}
                  onClick={() => setDispatchKind("group")}
                  icon={<Network size={12} />}
                  label={t("cron.create.kindGroup", "Group")}
                  hint={t("cron.create.kindGroupHint", "routed or pipeline")}
                />
                <KindButton
                  active={dispatchKind === "raw"}
                  onClick={() => setDispatchKind("raw")}
                  icon={<FileCode size={12} />}
                  label={t("cron.create.kindRaw", "Raw")}
                  hint={t("cron.create.kindRawHint", "advanced")}
                />
              </div>

              {dispatchKind === "agent" && (
                <div className="space-y-2">
                  <select
                    value={selectedAgentId}
                    onChange={(e) => setSelectedAgentId(e.target.value)}
                    className="input"
                  >
                    <option value="">{t("cron.create.pickAgent", "Pick an agent…")}</option>
                    {agents.map((a) => (
                      <option key={a.id} value={a.id}>
                        @{a.slug} — {a.runtime}
                      </option>
                    ))}
                  </select>
                  {selectedAgent && (
                    <div className="rounded-md border border-cs-border bg-cs-bg-raised/40 p-2 text-[11px] text-cs-muted">
                      <span className="text-cs-text">@{selectedAgent.slug}</span> runs on{" "}
                      <span className="text-cs-accent font-mono">{selectedAgent.runtime}</span>
                      {selectedAgent.model && <> · model <span className="text-cs-text font-mono">{selectedAgent.model}</span></>}
                      <p className="mt-0.5">
                        {t(
                          "cron.create.agentInheritsHint",
                          "The agent's system prompt, variables, hooks, memory policy, and skills all fire on every run."
                        )}
                      </p>
                    </div>
                  )}
                </div>
              )}

              {dispatchKind === "group" && (
                <div className="space-y-2">
                  <select
                    value={selectedGroupSlug}
                    onChange={(e) => setSelectedGroupSlug(e.target.value)}
                    className="input"
                  >
                    <option value="">{t("cron.create.pickGroup", "Pick a group…")}</option>
                    {groups.map((g) => (
                      <option key={g.id} value={g.slug}>
                        {g.slug} — {g.dispatchKind === "sequential" ? "automation pipeline" : "routed"}
                      </option>
                    ))}
                  </select>
                  {selectedGroup && (
                    <div className="rounded-md border border-cs-border bg-cs-bg-raised/40 p-2 text-[11px] text-cs-muted">
                      <span className="text-cs-text">{selectedGroup.slug}</span>{" "}
                      <span className="text-cs-accent font-mono">{selectedGroup.dispatchKind ?? "routed"}</span>
                      {" · "}
                      {selectedGroup.members.filter((m) => m.role === "child").length} children
                    </div>
                  )}
                </div>
              )}

              {dispatchKind === "raw" && (
                <div className="space-y-2">
                  <p className="text-[10px] text-cs-muted">
                    {t(
                      "cron.create.rawHint",
                      "Skips agent context engineering. Useful for one-off pings or runtime sanity checks."
                    )}
                  </p>
                  <div className="grid grid-cols-4 gap-2">
                    {RUNTIMES.map(({ id, label, color, Icon }) => (
                      <button
                        key={id}
                        type="button"
                        onClick={() => setRawRuntime(id)}
                        className="flex items-center justify-center gap-1.5 px-2 py-2 text-xs font-medium rounded-lg border transition-colors"
                        style={
                          rawRuntime === id
                            ? { borderColor: `${color}66`, background: `${color}18`, color }
                            : { borderColor: "var(--cs-border)", color: "var(--cs-muted)" }
                        }
                      >
                        <Icon size={14} />
                        {label}
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>

            {/* Prompt — applies to all three modes */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {dispatchKind === "raw"
                  ? t("cron.create.rawPrompt", "Prompt")
                  : t("cron.create.message", "Message")}
              </label>
              <textarea
                className="w-full h-24 p-3 bg-cs-bg border border-cs-border rounded-lg text-sm font-mono text-cs-text resize-y focus:outline-none focus:border-cs-accent"
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                placeholder={
                  dispatchKind === "agent"
                    ? t("cron.create.messageAgentPlaceholder", "What the agent should do this run. Variables in its system prompt resolve at fire time.")
                    : dispatchKind === "group"
                    ? t("cron.create.messageGroupPlaceholder", "Prompt sent to the group's router (or first stage of the pipeline).")
                    : t("cron.create.rawPromptPlaceholder", "Raw prompt sent to the runtime.")
                }
              />
            </div>

            <div className="flex gap-2 pt-2">
              <button
                type="submit"
                disabled={!isValid}
                className="flex-1 px-4 py-2 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
              >
                {t("common.create", "Create")}
              </button>
              <button
                type="button"
                onClick={onClose}
                className="px-4 py-2 text-sm rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
              >
                {t("common.cancel", "Cancel")}
              </button>
            </div>
          </form>
        </div>
      </div>
    </>
  );
}

function KindButton({
  active,
  onClick,
  icon,
  label,
  hint,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
  hint: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex flex-col items-start gap-0.5 px-3 py-2 rounded-md border text-left transition",
        active
          ? "border-cs-accent/60 bg-cs-accent/10"
          : "border-cs-border bg-cs-bg-raised hover:border-cs-border/80"
      )}
    >
      <span className={cn(
        "inline-flex items-center gap-1 text-xs font-medium",
        active ? "text-cs-accent" : "text-cs-text"
      )}>
        {icon}
        {label}
      </span>
      <span className="text-[9px] text-cs-muted">{hint}</span>
    </button>
  );
}
