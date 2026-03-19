import { useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import {
  ChevronDown,
  ToggleLeft,
  ToggleRight,
  Eye,
  Pencil,
  Save,
  Play,
  Plus,
  Trash2,
  Globe,
} from "lucide-react";
import { SERVICE_COLORS, SERVICE_ICONS } from "./constants";
import { useAutomationStore } from "@/stores/useAutomationStore";
import type { Workflow, BuilderMode } from "./types";

interface WorkflowToolbarProps {
  onRun: () => void;
  onSave: () => void;
}

export default function WorkflowToolbar({ onRun, onSave }: WorkflowToolbarProps) {
  const { t } = useTranslation();
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [showNewDialog, setShowNewDialog] = useState(false);
  const [newName, setNewName] = useState("");
  const [runtimeFilter, setRuntimeFilter] = useState<string>("all");
  const [sourceFilter, setSourceFilter] = useState<string>("all");

  const {
    mode,
    setMode,
    workflows,
    activeWorkflowId,
    setActiveWorkflowId,
    toggleWorkflow,
    dirty,
    createWorkflow,
    deleteWorkflow,
    execution,
  } = useAutomationStore();

  const active = workflows.find((w) => w.id === activeWorkflowId)!;
  if (!active) return null;

  function handleCreate() {
    if (!newName.trim()) return;
    createWorkflow(newName.trim());
    setNewName("");
    setShowNewDialog(false);
    setMode("edit");
  }

  function handleDelete() {
    if (!confirm(t("automation.builder.confirmDeleteWorkflow", "Delete this workflow?"))) return;
    deleteWorkflow(activeWorkflowId);
  }

  return (
    <div className="relative flex items-center gap-3 px-4 py-2.5 border-b border-[#2a2a3a]" style={{ background: "#0e0e16" }}>
      {/* Workflow dropdown */}
      <button
        onClick={() => setDropdownOpen(!dropdownOpen)}
        className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-[#2a2a3a] hover:border-[#00FFB2] transition-colors"
        style={{ background: "#16161e" }}
      >
        <span className="text-sm font-semibold text-[#e8e8f0]">{active.name}</span>
        <ChevronDown size={14} className={cn("text-[#8888a0] transition-transform", dropdownOpen && "rotate-180")} />
      </button>

      <p className="text-xs text-[#8888a0] flex-1 truncate">{active.description}</p>

      {/* Mode toggle */}
      <div className="flex items-center rounded-lg border border-[#2a2a3a] overflow-hidden" style={{ background: "#16161e" }}>
        <button
          onClick={() => setMode("view")}
          className={cn(
            "flex items-center gap-1.5 px-3 py-1.5 text-xs transition-colors",
            mode === "view" ? "bg-[#00FFB210] text-[#00FFB2]" : "text-[#8888a0] hover:text-[#e8e8f0]"
          )}
        >
          <Eye size={12} />
          {t("automation.builder.viewMode", "View")}
        </button>
        <button
          onClick={() => setMode("edit")}
          className={cn(
            "flex items-center gap-1.5 px-3 py-1.5 text-xs transition-colors border-l border-[#2a2a3a]",
            mode === "edit" ? "bg-[#00FFB210] text-[#00FFB2]" : "text-[#8888a0] hover:text-[#e8e8f0]"
          )}
        >
          <Pencil size={12} />
          {t("automation.builder.editMode", "Edit")}
        </button>
      </div>

      {/* Enable/disable toggle */}
      <button
        onClick={() => toggleWorkflow(activeWorkflowId)}
        className="flex items-center gap-1.5 text-xs shrink-0"
      >
        {active.enabled ? (
          <>
            <ToggleRight size={18} className="text-[#00FFB2]" />
            <span className="text-[#00FFB2]">{t("automation.enabled", "Enabled")}</span>
          </>
        ) : (
          <>
            <ToggleLeft size={18} className="text-[#8888a0]" />
            <span className="text-[#8888a0]">{t("automation.disabled", "Disabled")}</span>
          </>
        )}
      </button>

      {/* Save button (edit mode) */}
      {mode === "edit" && (
        <button
          onClick={onSave}
          className={cn(
            "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs transition-colors border",
            dirty
              ? "border-[#00FFB2] text-[#00FFB2] hover:bg-[#00FFB210]"
              : "border-[#2a2a3a] text-[#8888a0]"
          )}
        >
          <Save size={12} />
          {t("automation.builder.save", "Save")}
        </button>
      )}

      {/* Run button */}
      <button
        onClick={onRun}
        disabled={execution.running}
        className={cn(
          "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors",
          execution.running
            ? "bg-[#FFB80020] text-[#FFB800] border border-[#FFB80040]"
            : "bg-[#00FFB210] text-[#00FFB2] border border-[#00FFB240] hover:bg-[#00FFB220]"
        )}
      >
        <Play size={12} />
        {execution.running
          ? t("automation.builder.running", "Running...")
          : t("automation.builder.run", "Run")}
      </button>

      {/* Delete workflow (edit mode) */}
      {mode === "edit" && workflows.length > 1 && (
        <button
          onClick={handleDelete}
          className="flex items-center justify-center w-7 h-7 rounded-md border border-[#2a2a3a] hover:border-[#FF4466] text-[#8888a0] hover:text-[#FF4466] transition-colors"
        >
          <Trash2 size={12} />
        </button>
      )}

      {active.lastRun && (
        <span className="text-[10px] text-[#8888a0] shrink-0">
          {t("automation.lastRun", "Last run")}: {active.lastRun}
        </span>
      )}

      {/* Dropdown menu */}
      {dropdownOpen && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setDropdownOpen(false)} />
          <div
            className="absolute top-full left-4 mt-1 w-80 max-h-[70vh] rounded-lg border border-[#2a2a3a] shadow-xl overflow-hidden flex flex-col z-50"
            style={{ background: "#16161e" }}
          >
            {/* Runtime filter tabs */}
            {(() => {
              const runtimes = [...new Set(workflows.map((w) => w.nodes[0]?.runtime).filter(Boolean))];
              if (runtimes.length <= 1) return null;
              const COLORS: Record<string, string> = { claude: "#f97316", openclaw: "#06b6d4", codex: "#22c55e", hermes: "#a855f7" };
              return (
                <div className="flex items-center gap-1 px-3 py-2 border-b border-[#2a2a3a] shrink-0">
                  <button
                    onClick={() => setRuntimeFilter("all")}
                    className={cn(
                      "px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider rounded-md transition-colors",
                      runtimeFilter === "all" ? "bg-[#00FFB215] text-[#00FFB2]" : "text-[#8888a0] hover:text-[#e8e8f0]"
                    )}
                  >
                    All ({workflows.length})
                  </button>
                  {runtimes.map((rt) => {
                    const count = workflows.filter((w) => w.nodes[0]?.runtime === rt).length;
                    const c = COLORS[rt] || "#8888a0";
                    return (
                      <button
                        key={rt}
                        onClick={() => setRuntimeFilter(rt)}
                        className={cn(
                          "px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider rounded-md transition-colors",
                          runtimeFilter === rt ? "text-[#e8e8f0]" : "text-[#8888a0] hover:text-[#e8e8f0]"
                        )}
                        style={runtimeFilter === rt ? { background: `${c}20`, color: c } : {}}
                      >
                        {rt} ({count})
                      </button>
                    );
                  })}
                </div>
              );
            })()}
            {/* Source type filter */}
            {(() => {
              const sources = [...new Set(workflows.map((w) => w.source).filter(Boolean))];
              if (sources.length <= 1) return null;
              const labels: Record<string, string> = { skill: "Skills", cron: "Cron Jobs", manual: "Manual" };
              return (
                <div className="flex items-center gap-1 px-3 py-1.5 border-b border-[#2a2a3a] shrink-0">
                  <button
                    onClick={() => setSourceFilter("all")}
                    className={cn("px-2 py-0.5 text-[10px] font-medium rounded-md transition-colors", sourceFilter === "all" ? "bg-[#8888a020] text-[#e8e8f0]" : "text-[#8888a0] hover:text-[#e8e8f0]")}
                  >All</button>
                  {sources.map((s) => (
                    <button
                      key={s}
                      onClick={() => setSourceFilter(s!)}
                      className={cn("px-2 py-0.5 text-[10px] font-medium rounded-md transition-colors", sourceFilter === s ? "bg-[#8888a020] text-[#e8e8f0]" : "text-[#8888a0] hover:text-[#e8e8f0]")}
                    >{labels[s!] || s}</button>
                  ))}
                </div>
              );
            })()}
            <div className="overflow-y-auto flex-1">
            {workflows.filter((w) => (runtimeFilter === "all" || w.nodes[0]?.runtime === runtimeFilter) && (sourceFilter === "all" || w.source === sourceFilter)).map((w) => {
              const services = [...new Set(w.nodes.filter((n) => n.service).map((n) => n.service!))];
              return (
                <button
                  key={w.id}
                  onClick={() => { setActiveWorkflowId(w.id); setDropdownOpen(false); }}
                  className={cn(
                    "w-full text-left px-3 py-2.5 border-b border-[#2a2a3a] last:border-0 transition-colors",
                    w.id === activeWorkflowId ? "bg-[#00FFB208]" : "hover:bg-[#0a0a0f]"
                  )}
                >
                  <div className="flex items-center gap-2 mb-1">
                    <span className={cn(
                      "w-2 h-2 rounded-full shrink-0",
                      w.enabled ? "bg-[#00FFB2]" : "bg-[#8888a0]/40"
                    )} />
                    <span className="text-sm font-medium text-[#e8e8f0]">{w.name}</span>
                    {/* Runtime source badge */}
                    {(() => {
                      const rt = w.nodes[0]?.runtime;
                      if (!rt) return null;
                      const colors: Record<string, string> = { claude: "#f97316", codex: "#22c55e", openclaw: "#06b6d4", hermes: "#a855f7" };
                      const c = colors[rt] || "#8888a0";
                      return (
                        <span className="text-[9px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded" style={{ color: c, background: `${c}18` }}>
                          {rt}
                        </span>
                      );
                    })()}
                    <span className="text-[10px] text-[#8888a0] ml-auto">{w.runCount} runs</span>
                  </div>
                  <p className="text-[11px] text-[#8888a0] truncate pl-4 mb-1.5">{w.description}</p>
                  <div className="flex items-center gap-1.5 pl-4">
                    {services.map((s) => {
                      const Icon = SERVICE_ICONS[s] || Globe;
                      const color = SERVICE_COLORS[s] || "#8888a0";
                      return (
                        <div
                          key={s}
                          className="flex items-center gap-1 rounded px-1.5 py-0.5"
                          style={{ background: `${color}15` }}
                        >
                          <Icon size={10} style={{ color }} />
                          <span className="text-[9px] font-medium" style={{ color }}>{s}</span>
                        </div>
                      );
                    })}
                  </div>
                </button>
              );
            })}

            </div>
            {/* New workflow button */}
            <div className="border-t border-[#2a2a3a] shrink-0">
              {showNewDialog ? (
                <div className="p-3 flex gap-2">
                  <input
                    autoFocus
                    type="text"
                    value={newName}
                    onChange={(e) => setNewName(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && handleCreate()}
                    placeholder={t("automation.builder.workflowName", "Workflow name...")}
                    className="flex-1 rounded-md border border-[#2a2a3a] bg-[#0a0a0f] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2]"
                  />
                  <button
                    onClick={handleCreate}
                    className="px-2 py-1.5 rounded-md bg-[#00FFB210] text-[#00FFB2] text-xs border border-[#00FFB240] hover:bg-[#00FFB220]"
                  >
                    {t("automation.builder.create", "Create")}
                  </button>
                </div>
              ) : (
                <button
                  onClick={() => { setShowNewDialog(true); setDropdownOpen(true); }}
                  className="w-full flex items-center gap-2 px-3 py-2.5 text-xs text-[#8888a0] hover:text-[#00FFB2] hover:bg-[#0a0a0f] transition-colors"
                >
                  <Plus size={12} />
                  {t("automation.builder.newWorkflow", "New Workflow")}
                </button>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
