import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { X, Globe, Activity, Workflow } from "lucide-react";
import { TYPE_COLORS, SERVICE_COLORS, SERVICE_ICONS, NODE_ICONS } from "./automation/constants";
import { serializeWorkflowToPrompt } from "./automation/helpers";
import { useAutomationStore } from "@/stores/useAutomationStore";
import WorkflowToolbar from "./automation/WorkflowToolbar";
import NodePalette from "./automation/NodePalette";
import NodeConfigPanel from "./automation/NodeConfigPanel";
import FlowCanvas from "./automation/FlowCanvas";
import ExecutionOverlay from "./automation/ExecutionOverlay";
import { promptAgent, saveWorkflow as persistWorkflow } from "@/lib/tauri-api";

export default function AutomationFlow() {
  const { t } = useTranslation();
  const {
    mode,
    getActiveWorkflow,
    selectedNodeId,
    selectNode,
    deleteNode,
    saveWorkflow: markSaved,
    startExecution,
    updateNodeExecStatus,
    appendOutput,
    finishExecution,
    execution,
    workflows,
  } = useAutomationStore();

  const workflow = getActiveWorkflow();
  const selectedNode = workflow.nodes.find((n) => n.id === selectedNodeId) || null;

  // Save workflow to disk
  const handleSave = useCallback(async () => {
    try {
      await persistWorkflow(workflow);
      markSaved();
    } catch {
      // localStorage fallback is handled in tauri-api
      markSaved();
    }
  }, [workflow, markSaved]);

  // Execute workflow via Claude CLI
  const handleRun = useCallback(async () => {
    if (execution.running) return;
    if (workflow.nodes.length === 0) return;

    const prompt = serializeWorkflowToPrompt(workflow);
    startExecution();

    // Set all nodes to pending
    for (const node of workflow.nodes) {
      updateNodeExecStatus(node.id, "pending");
    }

    try {
      // Use the dominant runtime from action nodes, fallback to claude
      const actionNodes = workflow.nodes.filter((n) => n.type === "action" || n.type === "process");
      const runtime = actionNodes.find((n) => n.runtime)?.runtime || "claude";
      const response = await promptAgent(runtime, prompt);

      // Parse step markers from response
      const lines = response.split("\n");
      for (const line of lines) {
        const match = line.match(/\[STEP (\d+)\] (STARTED|COMPLETED|FAILED)/);
        if (match) {
          const stepIdx = parseInt(match[1], 10) - 1;
          const status = match[2];
          const node = workflow.nodes[stepIdx];
          if (node) {
            updateNodeExecStatus(
              node.id,
              status === "STARTED" ? "running" : status === "COMPLETED" ? "completed" : "failed"
            );
          }
        }
      }

      // Mark remaining as completed
      for (const node of workflow.nodes) {
        const current = useAutomationStore.getState().execution.nodeStatuses[node.id];
        if (!current || current === "pending" || current === "running") {
          updateNodeExecStatus(node.id, "completed");
        }
      }

      appendOutput(response);
      finishExecution();
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      appendOutput(message);
      finishExecution(message);
    }
  }, [workflow, execution.running, startExecution, updateNodeExecStatus, appendOutput, finishExecution]);

  if (workflows.length === 0) {
    return (
      <div className="flex flex-col h-full w-full items-center justify-center" style={{ background: "#0a0a0f" }}>
        <div className="text-center">
          <div className="w-16 h-16 rounded-full bg-[#2a2a3a]/30 flex items-center justify-center mx-auto mb-4">
            <Activity size={24} className="text-cs-muted/50" />
          </div>
          <p className="text-cs-muted text-sm mb-1">
            {t("automation.builder.emptyState")}
          </p>
          <p className="text-cs-muted/60 text-xs mb-4">
            {t("automation.builder.emptyStateHint")}
          </p>
          <button
            onClick={() => {
              const { createWorkflow, setMode } = useAutomationStore.getState();
              createWorkflow("My First Workflow");
              setMode("edit");
            }}
            className="inline-flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors"
          >
            {t("automation.builder.newWorkflow")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full w-full" style={{ background: "#0a0a0f" }}>
      {/* Toolbar */}
      <WorkflowToolbar onRun={handleRun} onSave={handleSave} />

      <div className="flex flex-1 overflow-hidden">
        {/* Node Palette (edit mode only) */}
        {mode === "edit" && <NodePalette onDragStart={() => {}} />}

        {/* Canvas + right panel */}
        <FlowCanvas />

        {/* Config Panel (edit mode, when node selected) */}
        {mode === "edit" && selectedNode && (
          <NodeConfigPanel
            node={selectedNode}
            onDelete={() => {
              if (confirm(t("automation.builder.confirmDelete", "Delete this node?"))) {
                deleteNode(selectedNode.id);
              }
            }}
          />
        )}
      </div>

      {/* Bottom detail panel (view mode) */}
      {mode === "view" && selectedNode && (
        <div
          className="border-t border-[#2a2a3a]"
          style={{ background: "#16161e", zIndex: 40 }}
        >
          <div className="flex items-start justify-between px-4 py-3">
            <div className="flex gap-6">
              <div>
                <div className="flex items-center gap-2 mb-1">
                  {(() => {
                    const isService = selectedNode.type === "service" && selectedNode.service;
                    const barColor = isService
                      ? SERVICE_COLORS[selectedNode.service!] || TYPE_COLORS.service
                      : TYPE_COLORS[selectedNode.type];
                    const Icon = isService
                      ? SERVICE_ICONS[selectedNode.service!] || Globe
                      : NODE_ICONS[selectedNode.type] || Activity;
                    return <Icon size={16} style={{ color: barColor }} />;
                  })()}
                  <h3 className="text-sm font-semibold text-[#e8e8f0]">{selectedNode.label}</h3>
                  {selectedNode.service && (
                    <span
                      className="rounded px-1.5 py-0.5 text-[9px] font-bold uppercase"
                      style={{
                        color: SERVICE_COLORS[selectedNode.service] || "#f97316",
                        background: `${SERVICE_COLORS[selectedNode.service] || "#f97316"}18`,
                      }}
                    >
                      {selectedNode.service} MCP
                    </span>
                  )}
                  <span
                    className="rounded px-1.5 py-0.5 text-[9px] font-medium uppercase"
                    style={{
                      color: TYPE_COLORS[selectedNode.type],
                      background: `${TYPE_COLORS[selectedNode.type]}18`,
                    }}
                  >
                    {selectedNode.type}
                  </span>
                </div>
                <p className="text-xs text-[#8888a0]">{selectedNode.description}</p>
              </div>

              <div className="flex gap-3">
                <div className="rounded-md border border-[#2a2a3a] px-3 py-2 text-center">
                  <p className="text-lg font-bold text-[#e8e8f0]">{selectedNode.stats.executions}</p>
                  <p className="text-[10px] text-[#8888a0]">{t("automation.executions", "Executions")}</p>
                </div>
                <div className="rounded-md border border-[#2a2a3a] px-3 py-2 text-center">
                  <p className={cn("text-lg font-bold", selectedNode.stats.errors > 0 ? "text-[#FF4466]" : "text-[#e8e8f0]")}>
                    {selectedNode.stats.errors}
                  </p>
                  <p className="text-[10px] text-[#8888a0]">{t("automation.errors", "Errors")}</p>
                </div>
                <div className="rounded-md border border-[#2a2a3a] px-3 py-2 text-center">
                  <p className="text-lg font-bold text-[#e8e8f0]">
                    {selectedNode.stats.avgTimeMs}
                    <span className="text-xs font-normal text-[#8888a0]">ms</span>
                  </p>
                  <p className="text-[10px] text-[#8888a0]">{t("automation.avgTime", "Avg Time")}</p>
                </div>
              </div>
            </div>

            <button
              onClick={() => selectNode(null)}
              className="flex items-center justify-center w-7 h-7 rounded-md border border-[#2a2a3a] hover:border-[#FF4466] transition-colors flex-shrink-0"
              style={{ background: "#0e0e16" }}
            >
              <X size={14} className="text-[#8888a0]" />
            </button>
          </div>
        </div>
      )}

      {/* Execution overlay */}
      <ExecutionOverlay />
    </div>
  );
}
