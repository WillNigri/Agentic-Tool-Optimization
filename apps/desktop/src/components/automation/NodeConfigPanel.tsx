import { useTranslation } from "react-i18next";
import { Trash2, Globe, Activity, Terminal, Cpu, Server } from "lucide-react";
import { CONFIG_PANEL_W, TYPE_COLORS, SERVICE_COLORS, SERVICE_ICONS, NODE_ICONS } from "./constants";
import { SERVICE_ACTIONS } from "./service-catalog";
import { useAutomationStore } from "@/stores/useAutomationStore";
import type { FlowNode, AgentRuntime } from "./types";

const RUNTIMES: { id: AgentRuntime; label: string; color: string; Icon: typeof Terminal }[] = [
  { id: "claude", label: "Claude", color: "#f97316", Icon: Terminal },
  { id: "codex", label: "Codex", color: "#22c55e", Icon: Cpu },
  { id: "openclaw", label: "OpenClaw", color: "#06b6d4", Icon: Server },
  { id: "hermes", label: "Hermes", color: "#a855f7", Icon: Globe },
];

interface NodeConfigPanelProps {
  node: FlowNode;
  onDelete: () => void;
}

export default function NodeConfigPanel({ node, onDelete }: NodeConfigPanelProps) {
  const { t } = useTranslation();
  const updateNode = useAutomationStore((s) => s.updateNode);

  const isService = node.type === "service" && node.service;
  const barColor = isService
    ? SERVICE_COLORS[node.service!] || TYPE_COLORS.service
    : TYPE_COLORS[node.type];
  const Icon = isService
    ? SERVICE_ICONS[node.service!] || Globe
    : NODE_ICONS[node.type] || Activity;

  const actions = node.service ? SERVICE_ACTIONS[node.service] || [] : [];
  const selectedAction = actions.find(
    (a) => a.id === node.config?.params?.action
  );

  function setParam(key: string, value: string) {
    const params = { ...node.config?.params, [key]: value };
    updateNode(node.id, { config: { ...node.config, params } });
  }

  function setLabel(label: string) {
    updateNode(node.id, { label });
  }

  function setDescription(description: string) {
    updateNode(node.id, { description });
  }

  function setCondition(condition: string) {
    updateNode(node.id, { config: { ...node.config, params: node.config?.params || {}, condition } });
  }

  function setService(service: string) {
    updateNode(node.id, { service, config: { params: {} } });
  }

  function setAction(actionId: string) {
    setParam("action", actionId);
  }

  const availableServices = Object.keys(SERVICE_ACTIONS);

  return (
    <div
      className="flex-shrink-0 border-l border-[#2a2a3a] overflow-y-auto"
      style={{ width: CONFIG_PANEL_W, background: "#0e0e16" }}
    >
      <div className="p-3">
        {/* Header */}
        <div className="flex items-center gap-2 mb-4">
          <Icon size={16} style={{ color: barColor }} />
          <h3 className="text-sm font-semibold text-[#e8e8f0] flex-1">
            {t("automation.builder.configure", "Configure Node")}
          </h3>
          <span
            className="rounded px-1.5 py-0.5 text-[9px] font-bold uppercase"
            style={{ color: barColor, background: `${barColor}18` }}
          >
            {node.type}
          </span>
        </div>

        {/* Label */}
        <div className="mb-3">
          <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
            {t("automation.builder.label", "Label")}
          </label>
          <input
            type="text"
            value={node.label}
            onChange={(e) => setLabel(e.target.value)}
            className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
          />
        </div>

        {/* Description */}
        <div className="mb-3">
          <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
            {t("automation.builder.description", "Description")}
          </label>
          <input
            type="text"
            value={node.description}
            onChange={(e) => setDescription(e.target.value)}
            className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
          />
        </div>

        {/* Service dropdown (for service nodes) */}
        {(node.type === "service" || node.type === "trigger") && (
          <div className="mb-3">
            <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
              {t("automation.builder.service", "Service")}
            </label>
            <select
              value={node.service || ""}
              onChange={(e) => setService(e.target.value)}
              className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
            >
              <option value="">-- select --</option>
              {availableServices.map((s) => (
                <option key={s} value={s}>{s}</option>
              ))}
            </select>
          </div>
        )}

        {/* Action dropdown */}
        {actions.length > 0 && (
          <div className="mb-3">
            <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
              {t("automation.builder.action", "Action")}
            </label>
            <select
              value={node.config?.params?.action || ""}
              onChange={(e) => setAction(e.target.value)}
              className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
            >
              <option value="">-- select --</option>
              {actions.map((a) => (
                <option key={a.id} value={a.id}>{a.label}</option>
              ))}
            </select>
            {selectedAction && (
              <p className="text-[9px] text-[#8888a0] mt-1">{selectedAction.description}</p>
            )}
          </div>
        )}

        {/* Dynamic param fields */}
        {selectedAction?.params.map((param) => (
          <div key={param.key} className="mb-3">
            <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
              {param.label}
              {param.required && <span className="text-[#FF4466] ml-0.5">*</span>}
            </label>
            {param.type === "textarea" ? (
              <textarea
                value={node.config?.params?.[param.key] || ""}
                onChange={(e) => setParam(param.key, e.target.value)}
                placeholder={param.placeholder}
                rows={3}
                className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors resize-none"
              />
            ) : param.type === "select" ? (
              <select
                value={node.config?.params?.[param.key] || ""}
                onChange={(e) => setParam(param.key, e.target.value)}
                className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
              >
                <option value="">-- select --</option>
                {param.options?.map((opt) => (
                  <option key={opt} value={opt}>{opt}</option>
                ))}
              </select>
            ) : (
              <input
                type="text"
                value={node.config?.params?.[param.key] || ""}
                onChange={(e) => setParam(param.key, e.target.value)}
                placeholder={param.placeholder}
                className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
              />
            )}
          </div>
        ))}

        {/* Runtime selector for action/process nodes */}
        {(node.type === "action" || node.type === "process") && (
          <div className="mb-3">
            <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
              Runtime
            </label>
            <div className="grid grid-cols-2 gap-1.5">
              {RUNTIMES.map(({ id, label, color, Icon }) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => updateNode(node.id, { runtime: id })}
                  className="flex items-center gap-1.5 px-2 py-1.5 text-[10px] font-medium rounded-md border transition-colors"
                  style={
                    (node.runtime || "claude") === id
                      ? { borderColor: `${color}66`, background: `${color}18`, color }
                      : { borderColor: "#2a2a3a", color: "#8888a0" }
                  }
                >
                  <Icon size={12} />
                  {label}
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Condition field for decision nodes */}
        {node.type === "decision" && (
          <div className="mb-3">
            <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
              {t("automation.builder.condition", "Condition")}
            </label>
            <textarea
              value={node.config?.condition || ""}
              onChange={(e) => setCondition(e.target.value)}
              placeholder="e.g. If security issues found..."
              rows={3}
              className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors resize-none font-mono"
            />
          </div>
        )}

        {/* Delete button */}
        <div className="mt-6 pt-3 border-t border-[#2a2a3a]">
          <button
            onClick={onDelete}
            className="flex items-center gap-2 w-full px-3 py-2 rounded-md border border-[#FF4466]/30 text-[#FF4466] text-xs hover:bg-[#FF446610] transition-colors"
          >
            <Trash2 size={12} />
            {t("automation.builder.deleteNode", "Delete Node")}
          </button>
        </div>
      </div>
    </div>
  );
}
