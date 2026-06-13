import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Trash2, Globe, Activity, Terminal, Cpu, Server } from "lucide-react";
import { CONFIG_PANEL_W, TYPE_COLORS, SERVICE_COLORS, SERVICE_ICONS, NODE_ICONS } from "./constants";
import { SERVICE_ACTIONS } from "./service-catalog";
import { useAutomationStore } from "@/stores/useLoopStore";
import InputPicker from "@/components/inputs/InputPicker";
import type { FlowNode, AgentRuntime, LoopStepKind, Workflow } from "./types";

// ── v2.14 — per-kind field schemas for the LLM-aware Loop Composer ──────
//
// Each LLM-aware LoopStepKind owns a small declarative schema of fields
// the user fills in for that step. The renderer below is the same shape
// as the existing service-action param renderer (text / textarea /
// select), so the LLM-aware path doesn't need a bespoke widget set —
// future polish (autocomplete for methodology slugs, multi-select for
// reviewers, etc.) can replace individual rows without touching the
// dispatch logic.
//
// All field values land in `node.config.params.<key>` so the loop
// executor (#14) can read uniform `params.runtime`, `params.slug`, etc.
// regardless of kind.
interface LlmKindField {
  key: string;
  label: string;
  type: "text" | "textarea" | "select";
  placeholder?: string;
  required?: boolean;
  options?: string[];
}

const LLM_KIND_FIELDS: Record<LoopStepKind, LlmKindField[]> = {
  dispatch: [
    { key: "runtime", label: "Runtime", type: "select", required: true, options: ["claude", "codex", "gemini", "openclaw", "hermes"] },
    { key: "model", label: "Model (optional)", type: "text", placeholder: "sonnet-4.6, gpt-4o, gemini-2.5-flash …" },
    { key: "prompt", label: "Prompt template", type: "textarea", required: true, placeholder: "Use {{vars.x}} / {{steps.previous.output.field}} …" },
    { key: "agent_slug", label: "Agent slug (optional)", type: "text", placeholder: "eng-manager, code-reviewer …" },
  ],
  methodology_run: [
    { key: "slug", label: "Methodology slug", type: "text", required: true, placeholder: "weekly-security-eval" },
    { key: "models", label: "Models (comma-separated)", type: "text", placeholder: "claude-sonnet-4.6, gpt-4o" },
    { key: "reps", label: "Reps per cell", type: "text", placeholder: "10" },
    { key: "context_input", label: "Context input", type: "text" },
  ],
  diagnose: [
    { key: "input_ref", label: "Source run reference", type: "text", required: true, placeholder: "{{steps.run.output.run_id}}" },
    { key: "model", label: "Diagnose model", type: "text", placeholder: "claude-opus-4.7" },
  ],
  apply: [
    { key: "diagnose_ref", label: "Diagnose proposal reference", type: "text", required: true, placeholder: "{{steps.diagnose.output}}" },
  ],
  review: [
    { key: "reviewers", label: "Reviewers (comma-separated)", type: "text", required: true, placeholder: "claude, codex, gemini" },
    { key: "against", label: "Against ref", type: "text", placeholder: "main, HEAD~1 …" },
  ],
  war_room: [
    { key: "seats", label: "Seats (comma-separated runtimes/agents)", type: "text", required: true, placeholder: "codex, gemini, minimax …" },
    { key: "framing", label: "Framing prompt", type: "textarea", required: true, placeholder: "Decision under debate: A vs B …" },
    { key: "context_input", label: "Context input", type: "text" },
  ],
  score: [
    { key: "rubric", label: "Rubric slug", type: "text", required: true, placeholder: "regression-watch" },
    { key: "target_ref", label: "Target output reference", type: "text", required: true, placeholder: "{{steps.previous.output}}" },
  ],
  input: [
    { key: "slug", label: "Input slug", type: "text", required: true, placeholder: "weekly-security-context" },
    { key: "paths", label: "File paths (comma-separated)", type: "text", placeholder: "docs/PRD.md, src/types.ts" },
  ],
};

function isLlmKind(t: string): t is LoopStepKind {
  return Object.prototype.hasOwnProperty.call(LLM_KIND_FIELDS, t);
}

const RUNTIMES: { id: AgentRuntime; label: string; color: string; Icon: typeof Terminal }[] = [
  { id: "claude", label: "Claude", color: "#f97316", Icon: Terminal },
  { id: "codex", label: "Codex", color: "#22c55e", Icon: Cpu },
  { id: "openclaw", label: "OpenClaw", color: "#06b6d4", Icon: Server },
  { id: "hermes", label: "Hermes", color: "#a855f7", Icon: Globe },
];

interface NodeConfigPanelProps {
  node?: FlowNode | null;
  workflow: Workflow;
  onDelete: () => void;
}

interface VariableRow {
  id: string;
  key: string;
  value: string;
}

function variablesToRows(variables: Record<string, string> | undefined): VariableRow[] {
  return Object.entries(variables ?? {}).map(([key, value], index) => ({
    id: `${key}-${index}`,
    key,
    value,
  }));
}

function rowsToVariables(rows: VariableRow[]): Record<string, string> {
  return Object.fromEntries(
    rows
      .map((row) => [row.key.trim(), row.value] as const)
      .filter(([key]) => key.length > 0)
  );
}

export default function NodeConfigPanel({ node, workflow, onDelete }: NodeConfigPanelProps) {
  const { t } = useTranslation();
  const updateNode = useAutomationStore((s) => s.updateNode);
  const updateActiveWorkflow = useAutomationStore((s) => s.updateActiveWorkflow);
  const [variableRows, setVariableRows] = useState<VariableRow[]>(() => variablesToRows(workflow.variables));

  useEffect(() => {
    setVariableRows(variablesToRows(workflow.variables));
  }, [workflow.id, workflow.variables]);

  function syncVariableRows(nextRows: VariableRow[]) {
    setVariableRows(nextRows);
    updateActiveWorkflow({ variables: rowsToVariables(nextRows) });
  }

  function addVariableRow() {
    syncVariableRows([
      ...variableRows,
      { id: `var-${Date.now()}`, key: "", value: "" },
    ]);
  }

  function updateVariableRow(id: string, field: "key" | "value", value: string) {
    syncVariableRows(variableRows.map((row) => (
      row.id === id ? { ...row, [field]: value } : row
    )));
  }

  function removeVariableRow(id: string) {
    syncVariableRows(variableRows.filter((row) => row.id !== id));
  }

  const hasNode = Boolean(node);

  const isService = node?.type === "service" && node.service;
  const barColor = isService
    ? SERVICE_COLORS[node.service!] || TYPE_COLORS.service
    : hasNode ? TYPE_COLORS[node!.type] : "#00FFB2";
  const Icon = isService
    ? SERVICE_ICONS[node.service!] || Globe
    : hasNode ? NODE_ICONS[node!.type] || Activity : Activity;

  const actions = node?.service ? SERVICE_ACTIONS[node.service] || [] : [];
  const selectedAction = actions.find(
    (a) => a.id === node?.config?.params?.action
  );

  function setParam(key: string, value: string) {
    if (!node) return;
    const params = { ...node.config?.params, [key]: value };
    updateNode(node.id, { config: { ...node.config, params } });
  }

  function setLabel(label: string) {
    if (!node) return;
    updateNode(node.id, { label });
  }

  function setDescription(description: string) {
    if (!node) return;
    updateNode(node.id, { description });
  }

  function setCondition(condition: string) {
    if (!node) return;
    updateNode(node.id, { config: { ...node.config, params: node.config?.params || {}, condition } });
  }

  function setService(service: string) {
    if (!node) return;
    updateNode(node.id, { service, config: { params: {} } });
  }

  function setAction(actionId: string) {
    setParam("action", actionId);
  }

  const availableServices = Object.keys(SERVICE_ACTIONS);
  const currentNode = node;

  function renderNodeEditor() {
    if (!currentNode) {
      return (
        <p className="mb-4 text-xs text-[#8888a0]">
          {t("loopComposer.variables.emptyHint")}
        </p>
      );
    }

    return (
      <>
        <div className="mb-3">
          <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
            {t("automation.builder.label", "Label")}
          </label>
          <input
            type="text"
            value={currentNode.label}
            onChange={(e) => setLabel(e.target.value)}
            className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
          />
        </div>

        <div className="mb-3">
          <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
            {t("automation.builder.description", "Description")}
          </label>
          <input
            type="text"
            value={currentNode.description}
            onChange={(e) => setDescription(e.target.value)}
            className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
          />
        </div>

        {(currentNode.type === "service" || currentNode.type === "trigger") && (
          <div className="mb-3">
            <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
              {t("automation.builder.service", "Service")}
            </label>
            <select
              value={currentNode.service || ""}
              onChange={(e) => setService(e.target.value)}
              className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
            >
              <option value="">{t("automation.builder.selectOption", "-- select --")}</option>
              {availableServices.map((service) => (
                <option key={service} value={service}>{service}</option>
              ))}
            </select>
          </div>
        )}

        {actions.length > 0 && (
          <div className="mb-3">
            <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
              {t("automation.builder.action", "Action")}
            </label>
            <select
              value={currentNode.config?.params?.action || ""}
              onChange={(e) => setAction(e.target.value)}
              className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
            >
              <option value="">{t("automation.builder.selectOption", "-- select --")}</option>
              {actions.map((action) => (
                <option key={action.id} value={action.id}>{action.label}</option>
              ))}
            </select>
            {selectedAction && (
              <p className="mt-1 text-[9px] text-[#8888a0]">{selectedAction.description}</p>
            )}
          </div>
        )}

        {selectedAction?.params.map((param) => (
          <div key={param.key} className="mb-3">
            <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
              {param.label}
              {param.required && <span className="ml-0.5 text-[#FF4466]">*</span>}
            </label>
            {param.type === "textarea" ? (
              <textarea
                value={currentNode.config?.params?.[param.key] || ""}
                onChange={(e) => setParam(param.key, e.target.value)}
                placeholder={param.placeholder}
                rows={3}
                className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors resize-none"
              />
            ) : param.type === "select" ? (
              <select
                value={currentNode.config?.params?.[param.key] || ""}
                onChange={(e) => setParam(param.key, e.target.value)}
                className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
              >
                <option value="">{t("automation.builder.selectOption", "-- select --")}</option>
                {param.options?.map((opt) => (
                  <option key={opt} value={opt}>{opt}</option>
                ))}
              </select>
            ) : (
              <input
                type="text"
                value={currentNode.config?.params?.[param.key] || ""}
                onChange={(e) => setParam(param.key, e.target.value)}
                placeholder={param.placeholder}
                className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
              />
            )}
          </div>
        ))}

        {isLlmKind(currentNode.type) && (
          <div className="mb-2">
            <div className="mb-2 text-[10px] font-medium uppercase tracking-wider text-[#8888a0]">
              {currentNode.type.replace(/_/g, " ")} config
            </div>
            {LLM_KIND_FIELDS[currentNode.type].map((field) => (
              <div key={field.key} className="mb-3">
                <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
                  {field.key === "context_input"
                    ? t("inputPicker.contextInput", "Context input")
                    : field.label}
                  {field.required && <span className="ml-0.5 text-[#FF4466]">*</span>}
                </label>
                {field.key === "context_input" ? (
                  <InputPicker
                    value={currentNode.config?.params?.[field.key] || ""}
                    onSelect={(slug) => setParam(field.key, slug)}
                    kindFilter="markdown"
                  />
                ) : field.type === "textarea" ? (
                  <textarea
                    value={currentNode.config?.params?.[field.key] || ""}
                    onChange={(e) => setParam(field.key, e.target.value)}
                    placeholder={field.placeholder}
                    rows={3}
                    className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors resize-none font-mono"
                  />
                ) : field.type === "select" ? (
                  <select
                    value={currentNode.config?.params?.[field.key] || ""}
                    onChange={(e) => setParam(field.key, e.target.value)}
                    className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
                  >
                    <option value="">{t("automation.builder.selectOption", "-- select --")}</option>
                    {field.options?.map((opt) => (
                      <option key={opt} value={opt}>{opt}</option>
                    ))}
                  </select>
                ) : (
                  <input
                    type="text"
                    value={currentNode.config?.params?.[field.key] || ""}
                    onChange={(e) => setParam(field.key, e.target.value)}
                    placeholder={field.placeholder}
                    className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors"
                  />
                )}
              </div>
            ))}
          </div>
        )}

        {(currentNode.type === "action" || currentNode.type === "process") && (
          <div className="mb-3">
            <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
              {t("automation.builder.runtime", "Runtime")}
            </label>
            <div className="grid grid-cols-2 gap-1.5">
              {RUNTIMES.map(({ id, label, color, Icon: RuntimeIcon }) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => updateNode(currentNode.id, { runtime: id })}
                  className="flex items-center gap-1.5 rounded-md border px-2 py-1.5 text-[10px] font-medium transition-colors"
                  style={
                    (currentNode.runtime || "claude") === id
                      ? { borderColor: `${color}66`, background: `${color}18`, color }
                      : { borderColor: "#2a2a3a", color: "#8888a0" }
                  }
                >
                  <RuntimeIcon size={12} />
                  {label}
                </button>
              ))}
            </div>
          </div>
        )}

        {currentNode.type === "decision" && (
          <div className="mb-3">
            <label className="block text-[10px] text-[#8888a0] uppercase tracking-wider mb-1 font-medium">
              {t("automation.builder.condition", "Condition")}
            </label>
            <textarea
              value={currentNode.config?.condition || ""}
              onChange={(e) => setCondition(e.target.value)}
              placeholder={t("automation.builder.conditionPlaceholder", "e.g. If security issues found...")}
              rows={3}
              className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 px-2 focus:outline-none focus:border-[#00FFB2] transition-colors resize-none font-mono"
            />
          </div>
        )}

        <div className="mt-6 border-t border-[#2a2a3a] pt-3">
          <button
            onClick={onDelete}
            className="flex w-full items-center gap-2 rounded-md border border-[#FF4466]/30 px-3 py-2 text-xs text-[#FF4466] transition-colors hover:bg-[#FF446610]"
          >
            <Trash2 size={12} />
            {t("automation.builder.deleteNode", "Delete Node")}
          </button>
        </div>
      </>
    );
  }

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
            {hasNode
              ? t("automation.builder.configure", "Configure Node")
              : t("loopComposer.variables.title")}
          </h3>
          {node && (
            <span
              className="rounded px-1.5 py-0.5 text-[9px] font-bold uppercase"
              style={{ color: barColor, background: `${barColor}18` }}
            >
              {node.type}
            </span>
          )}
        </div>

        {renderNodeEditor()}

        <div className="mt-6 pt-3 border-t border-[#2a2a3a]">
          <div className="mb-3 flex items-center justify-between">
            <div>
              <h4 className="text-xs font-semibold text-[#e8e8f0]">
                {t("loopComposer.variables.title")}
              </h4>
              <p className="text-[10px] text-[#8888a0]">
                {t("loopComposer.variables.description")}
              </p>
            </div>
            <button
              type="button"
              onClick={addVariableRow}
              className="rounded-md border border-[#2a2a3a] px-2 py-1 text-[10px] text-[#e8e8f0] hover:border-[#00FFB2] hover:text-[#00FFB2] transition-colors"
            >
              {t("loopComposer.variables.add")}
            </button>
          </div>

          {variableRows.length === 0 && (
            <p className="mb-3 text-[10px] text-[#8888a0]">
              {t("loopComposer.variables.empty")}
            </p>
          )}

          {variableRows.map((row) => (
            <div key={row.id} className="mb-2 grid grid-cols-[1fr_1fr_auto] gap-2">
              <input
                type="text"
                value={row.key}
                onChange={(e) => updateVariableRow(row.id, "key", e.target.value)}
                placeholder={t("loopComposer.variables.keyPlaceholder")}
                className="rounded-md border border-[#2a2a3a] bg-[#16161e] px-2 py-1.5 text-xs text-[#e8e8f0] focus:outline-none focus:border-[#00FFB2] transition-colors"
              />
              <input
                type="text"
                value={row.value}
                onChange={(e) => updateVariableRow(row.id, "value", e.target.value)}
                placeholder={t("loopComposer.variables.valuePlaceholder")}
                className="rounded-md border border-[#2a2a3a] bg-[#16161e] px-2 py-1.5 text-xs text-[#e8e8f0] focus:outline-none focus:border-[#00FFB2] transition-colors"
              />
              <button
                type="button"
                onClick={() => removeVariableRow(row.id)}
                className="flex h-[30px] w-[30px] items-center justify-center rounded-md border border-[#2a2a3a] text-[#8888a0] hover:border-[#FF4466] hover:text-[#FF4466] transition-colors"
                aria-label={t("loopComposer.variables.remove")}
              >
                <Trash2 size={12} />
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
