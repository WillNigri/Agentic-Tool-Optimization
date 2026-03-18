import { useState } from "react";
import { useTranslation } from "react-i18next";
import { X, Terminal, Cpu, Server, Globe } from "lucide-react";
import { cn } from "@/lib/utils";
import type { AgentRuntime } from "./types";
import { validateCron, cronToHuman } from "@/lib/cron-utils";
import { useCronStore } from "@/stores/useCronStore";

const RUNTIMES: { id: AgentRuntime; label: string; color: string; Icon: typeof Terminal }[] = [
  { id: "claude", label: "Claude", color: "#f97316", Icon: Terminal },
  { id: "codex", label: "Codex", color: "#22c55e", Icon: Cpu },
  { id: "openclaw", label: "OpenClaw", color: "#06b6d4", Icon: Server },
  { id: "hermes", label: "Hermes", color: "#a855f7", Icon: Globe },
];

interface CreateCronJobModalProps {
  onClose: () => void;
}

export default function CreateCronJobModal({ onClose }: CreateCronJobModalProps) {
  const { t } = useTranslation();
  const createJob = useCronStore((s) => s.createJob);

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [schedule, setSchedule] = useState("0 7 * * *");
  const [runtime, setRuntime] = useState<AgentRuntime>("claude");
  const [prompt, setPrompt] = useState("");

  const cronError = validateCron(schedule);
  const isValid = name.trim() && !cronError && prompt.trim();

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!isValid) return;

    createJob({
      name: name.trim(),
      description: description.trim(),
      schedule: schedule.trim(),
      runtime,
      prompt: prompt.trim(),
      enabled: true,
    });

    onClose();
  }

  return (
    <>
      {/* Backdrop */}
      <div className="fixed inset-0 bg-black/50 z-50" onClick={onClose} />

      {/* Modal */}
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <div
          className="bg-cs-card border border-cs-border rounded-xl w-full max-w-lg max-h-[90vh] overflow-y-auto shadow-2xl"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className="flex items-center justify-between p-4 border-b border-cs-border">
            <h3 className="text-lg font-semibold">{t("cron.create.title")}</h3>
            <button
              onClick={onClose}
              className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
            >
              <X size={16} />
            </button>
          </div>

          <form onSubmit={handleSubmit} className="p-4 space-y-4">
            {/* Name */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("cron.create.name")}
              </label>
              <input
                type="text"
                className="input"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={t("cron.create.namePlaceholder")}
                required
              />
            </div>

            {/* Description */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("cron.create.description")}
              </label>
              <input
                type="text"
                className="input"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder={t("cron.create.descriptionPlaceholder")}
              />
            </div>

            {/* Cron expression */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("cron.create.schedule")}
              </label>
              <input
                type="text"
                className={cn(
                  "input font-mono",
                  cronError && schedule.trim() ? "border-red-500/50" : ""
                )}
                value={schedule}
                onChange={(e) => setSchedule(e.target.value)}
                placeholder={t("cron.create.schedulePlaceholder")}
              />
              <div className="flex items-center justify-between mt-1">
                <p className="text-[10px] text-cs-muted">{t("cron.create.scheduleHint")}</p>
                {schedule.trim() && !cronError && (
                  <p className="text-[10px] text-cs-accent">{cronToHuman(schedule)}</p>
                )}
                {schedule.trim() && cronError && (
                  <p className="text-[10px] text-red-400">{cronError}</p>
                )}
              </div>
            </div>

            {/* Runtime selector */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("cron.create.runtime")}
              </label>
              <div className="grid grid-cols-4 gap-2">
                {RUNTIMES.map(({ id, label, color, Icon }) => (
                  <button
                    key={id}
                    type="button"
                    onClick={() => setRuntime(id)}
                    className="flex items-center justify-center gap-1.5 px-2 py-2 text-xs font-medium rounded-lg border transition-colors"
                    style={
                      runtime === id
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

            {/* Prompt */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("cron.create.prompt")}
              </label>
              <textarea
                className="w-full h-28 p-3 bg-cs-bg border border-cs-border rounded-lg text-sm font-mono text-cs-text resize-y focus:outline-none focus:border-cs-accent"
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                placeholder={t("cron.create.promptPlaceholder")}
              />
            </div>

            {/* Actions */}
            <div className="flex gap-2 pt-2">
              <button
                type="submit"
                disabled={!isValid}
                className="flex-1 px-4 py-2 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
              >
                {t("common.create")}
              </button>
              <button
                type="button"
                onClick={onClose}
                className="px-4 py-2 text-sm rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
              >
                {t("common.cancel")}
              </button>
            </div>
          </form>
        </div>
      </div>
    </>
  );
}
