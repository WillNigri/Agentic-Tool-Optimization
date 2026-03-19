import { useState } from "react";
import { X, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import type { CronJob } from "./types";
import { openclawEditCronJob } from "@/lib/tauri-api";

interface EditCronJobModalProps {
  job: CronJob;
  onClose: () => void;
  onSaved: () => void;
}

type ScheduleType = "every" | "cron";
type EveryUnit = "minutes" | "hours" | "days";
type SessionType = "main" | "isolated";
type RunType = "system-event" | "agent-turn";
type DeliveryType = "announce" | "none";

function parseEverySchedule(schedule: string): { type: ScheduleType; amount: string; unit: EveryUnit; cron: string } {
  // Handle "Every Nd/Nh/Nm" format from OpenClaw normalizer
  const everyMatch = schedule.match(/^Every\s+(\d+)([dhm])$/i);
  if (everyMatch) {
    const units: Record<string, EveryUnit> = { d: "days", h: "hours", m: "minutes" };
    return { type: "every", amount: everyMatch[1], unit: units[everyMatch[2]] || "hours", cron: "" };
  }

  // Handle "unknown" or empty
  if (schedule === "unknown" || !schedule) {
    return { type: "every", amount: "1", unit: "days", cron: "" };
  }

  // Try to detect "every"-style patterns from cron expressions
  const minMatch = schedule.match(/^\*\/(\d+)\s+\*\s+\*\s+\*\s+\*$/);
  if (minMatch) return { type: "every", amount: minMatch[1], unit: "minutes", cron: schedule };

  const hourMatch = schedule.match(/^0\s+\*\/(\d+)\s+\*\s+\*\s+\*$/);
  if (hourMatch) return { type: "every", amount: hourMatch[1], unit: "hours", cron: schedule };

  const dayMatch = schedule.match(/^0\s+0\s+\*\/(\d+)\s+\*\s+\*$/);
  if (dayMatch) return { type: "every", amount: dayMatch[1], unit: "days", cron: schedule };

  return { type: "cron", amount: "1", unit: "hours", cron: schedule };
}

function buildEveryFlag(amount: string, unit: EveryUnit): string {
  const n = parseInt(amount, 10) || 1;
  switch (unit) {
    case "minutes": return `${n}m`;
    case "hours": return `${n}h`;
    case "days": return `${n}d`;
  }
}

export default function EditCronJobModal({ job, onClose, onSaved }: EditCronJobModalProps) {
  const parsed = parseEverySchedule(job.schedule);

  // Basics
  const [name, setName] = useState(job.name);
  const [description, setDescription] = useState(job.description);
  const [agentId, setAgentId] = useState("");
  const [enabled, setEnabled] = useState(job.enabled);

  // Schedule
  const [scheduleType, setScheduleType] = useState<ScheduleType>(parsed.type);
  const [everyAmount, setEveryAmount] = useState(parsed.amount);
  const [everyUnit, setEveryUnit] = useState<EveryUnit>(parsed.unit);
  const [cronExpr, setCronExpr] = useState(parsed.cron);

  // Execution
  const [session, setSession] = useState<SessionType>("main");
  const [runType, setRunType] = useState<RunType>("agent-turn");
  const [message, setMessage] = useState(job.prompt);

  // Delivery
  const [delivery, setDelivery] = useState<DeliveryType>("none");
  const [channel, setChannel] = useState("");
  const [deliveryTo, setDeliveryTo] = useState("");

  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Strip "oc-" prefix to get real OpenClaw UUID
  const realId = job.id.startsWith("oc-") ? job.id.slice(3) : job.id;

  async function handleSave() {
    setSaving(true);
    setError(null);
    try {
      const parts: string[] = [];

      if (name.trim()) parts.push(`--name "${name.trim()}"`);
      if (description.trim()) parts.push(`--description "${description.trim()}"`);
      if (agentId.trim()) parts.push(`--agent "${agentId.trim()}"`);

      // Schedule
      if (scheduleType === "every") {
        parts.push(`--every ${buildEveryFlag(everyAmount, everyUnit)}`);
      } else {
        parts.push(`--cron "${cronExpr.trim()}"`);
      }

      // Message
      if (message.trim()) parts.push(`--message "${message.trim()}"`);

      // Session
      parts.push(`--session ${session}`);

      // Run type
      if (runType === "system-event") parts.push("--system-event");

      // Enabled
      parts.push(enabled ? "--enable" : "--disable");

      // Delivery
      if (delivery === "announce") {
        parts.push("--announce");
        if (channel.trim()) parts.push(`--channel ${channel.trim()}`);
        if (deliveryTo.trim()) parts.push(`--to ${deliveryTo.trim()}`);
      } else {
        parts.push("--no-deliver");
      }

      await openclawEditCronJob(realId, parts.join(" "));
      onSaved();
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }

  const inputClass =
    "w-full px-3 py-2 bg-[#0a0a0f] border border-[#2a2a3a] rounded-lg text-sm text-[#e8e8f0] focus:outline-none focus:border-[#00FFB2] transition-colors";
  const labelClass = "text-[11px] font-semibold text-[#8888a0] uppercase tracking-wider block mb-1";
  const sectionClass = "space-y-3";
  const sectionHeaderClass =
    "text-xs font-bold text-[#00FFB2] uppercase tracking-wider pb-1 border-b border-[#2a2a3a] mb-3";

  return (
    <>
      {/* Backdrop */}
      <div className="fixed inset-0 bg-black/60 z-50" onClick={onClose} />

      {/* Modal */}
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <div
          className="bg-[#16161e] border border-[#2a2a3a] rounded-xl w-full max-w-2xl max-h-[85vh] overflow-y-auto shadow-2xl"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className="flex items-center justify-between p-4 border-b border-[#2a2a3a]">
            <h3 className="text-lg font-semibold text-[#e8e8f0]">Edit Cron Job</h3>
            <button
              onClick={onClose}
              className="p-1.5 rounded hover:bg-[#2a2a3a] transition-colors text-[#8888a0] hover:text-[#e8e8f0]"
            >
              <X size={16} />
            </button>
          </div>

          <div className="p-5 space-y-6">
            {/* Basics */}
            <div className={sectionClass}>
              <h4 className={sectionHeaderClass}>Basics</h4>
              <div>
                <label className={labelClass}>Name</label>
                <input
                  type="text"
                  className={inputClass}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="My cron job"
                />
              </div>
              <div>
                <label className={labelClass}>Description</label>
                <input
                  type="text"
                  className={inputClass}
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  placeholder="What this job does"
                />
              </div>
              <div>
                <label className={labelClass}>Agent ID</label>
                <input
                  type="text"
                  className={inputClass}
                  value={agentId}
                  onChange={(e) => setAgentId(e.target.value)}
                  placeholder="Leave blank for default agent"
                />
              </div>
              <div className="flex items-center gap-3">
                <label className="relative inline-flex items-center cursor-pointer">
                  <input
                    type="checkbox"
                    checked={enabled}
                    onChange={(e) => setEnabled(e.target.checked)}
                    className="sr-only peer"
                  />
                  <div className={cn(
                    "w-9 h-5 rounded-full transition-colors",
                    enabled ? "bg-[#00FFB2]" : "bg-[#2a2a3a]"
                  )}>
                    <div className={cn(
                      "w-4 h-4 mt-0.5 rounded-full bg-[#0a0a0f] transition-transform",
                      enabled ? "translate-x-[18px]" : "translate-x-0.5"
                    )} />
                  </div>
                </label>
                <span className="text-sm text-[#e8e8f0]">Enabled</span>
              </div>
            </div>

            {/* Schedule */}
            <div className={sectionClass}>
              <h4 className={sectionHeaderClass}>Schedule</h4>
              <div>
                <label className={labelClass}>Schedule Type</label>
                <select
                  className={inputClass}
                  value={scheduleType}
                  onChange={(e) => setScheduleType(e.target.value as ScheduleType)}
                >
                  <option value="every">Every (interval)</option>
                  <option value="cron">Cron expression</option>
                </select>
              </div>
              {scheduleType === "every" ? (
                <div className="flex gap-2">
                  <div className="flex-1">
                    <label className={labelClass}>Amount</label>
                    <input
                      type="number"
                      min="1"
                      className={inputClass}
                      value={everyAmount}
                      onChange={(e) => setEveryAmount(e.target.value)}
                    />
                  </div>
                  <div className="flex-1">
                    <label className={labelClass}>Unit</label>
                    <select
                      className={inputClass}
                      value={everyUnit}
                      onChange={(e) => setEveryUnit(e.target.value as EveryUnit)}
                    >
                      <option value="minutes">Minutes</option>
                      <option value="hours">Hours</option>
                      <option value="days">Days</option>
                    </select>
                  </div>
                </div>
              ) : (
                <div>
                  <label className={labelClass}>Cron Expression</label>
                  <input
                    type="text"
                    className={cn(inputClass, "font-mono")}
                    value={cronExpr}
                    onChange={(e) => setCronExpr(e.target.value)}
                    placeholder="0 * * * *"
                  />
                  <p className="text-[10px] text-[#8888a0] mt-1">Standard 5-field cron (min hour dom mon dow)</p>
                </div>
              )}
            </div>

            {/* Execution */}
            <div className={sectionClass}>
              <h4 className={sectionHeaderClass}>Execution</h4>
              <div>
                <label className={labelClass}>Session</label>
                <select
                  className={inputClass}
                  value={session}
                  onChange={(e) => setSession(e.target.value as SessionType)}
                >
                  <option value="main">Main session</option>
                  <option value="isolated">Isolated session</option>
                </select>
              </div>
              <div>
                <label className={labelClass}>What should run</label>
                <select
                  className={inputClass}
                  value={runType}
                  onChange={(e) => setRunType(e.target.value as RunType)}
                >
                  <option value="agent-turn">Agent turn</option>
                  <option value="system-event">System event</option>
                </select>
              </div>
              <div>
                <label className={labelClass}>Message / Prompt</label>
                <textarea
                  className={cn(inputClass, "h-24 resize-y font-mono")}
                  value={message}
                  onChange={(e) => setMessage(e.target.value)}
                  placeholder="What the agent should do when this job runs"
                />
              </div>
            </div>

            {/* Delivery */}
            <div className={sectionClass}>
              <h4 className={sectionHeaderClass}>Delivery</h4>
              <div>
                <label className={labelClass}>Result Delivery</label>
                <select
                  className={inputClass}
                  value={delivery}
                  onChange={(e) => setDelivery(e.target.value as DeliveryType)}
                >
                  <option value="none">None</option>
                  <option value="announce">Announce</option>
                </select>
              </div>
              {delivery === "announce" && (
                <>
                  <div>
                    <label className={labelClass}>Channel</label>
                    <input
                      type="text"
                      className={inputClass}
                      value={channel}
                      onChange={(e) => setChannel(e.target.value)}
                      placeholder="e.g. discord, slack"
                    />
                  </div>
                  <div>
                    <label className={labelClass}>To</label>
                    <input
                      type="text"
                      className={inputClass}
                      value={deliveryTo}
                      onChange={(e) => setDeliveryTo(e.target.value)}
                      placeholder="Channel/user ID"
                    />
                  </div>
                </>
              )}
            </div>

            {/* Error */}
            {error && (
              <div className="text-sm text-red-400 bg-red-500/10 border border-red-500/30 rounded-lg px-3 py-2">
                {error}
              </div>
            )}

            {/* Actions */}
            <div className="flex gap-2 pt-2">
              <button
                onClick={handleSave}
                disabled={saving || !name.trim()}
                className="flex-1 flex items-center justify-center gap-2 px-4 py-2 text-sm rounded-lg bg-[#00FFB2] text-[#0a0a0f] font-medium hover:bg-[#00FFB2]/90 transition-colors disabled:opacity-50"
              >
                {saving && <Loader2 size={14} className="animate-spin" />}
                {saving ? "Saving..." : "Save Changes"}
              </button>
              <button
                onClick={onClose}
                className="px-4 py-2 text-sm rounded-lg border border-[#2a2a3a] text-[#8888a0] hover:text-[#e8e8f0] transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
