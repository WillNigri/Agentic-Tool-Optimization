import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
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
import { promptAgent, saveWorkflow as persistWorkflow, openclawListCronJobs } from "@/lib/tauri-api";
import { getSkills, getSkillDetail } from "@/lib/api";
import { generateWorkflowsFromSkills } from "@/lib/skill-to-workflow";
import type { SkillDetail } from "@/lib/tauri-api";
import type { Workflow as WorkflowType, FlowNode, FlowEdge } from "./automation/types";

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

  // Load skill-based workflows (gstack etc.) on mount
  // Fetch skill list, then load details for each to parse automation steps
  const { data: skills = [] } = useQuery({
    queryKey: ["skills"],
    queryFn: getSkills,
  });

  useEffect(() => {
    if (skills.length > 0) {
      console.log("[ATO] Automation: fetching details for", skills.length, "skills");
      // Fetch full content for each skill to detect automation steps
      Promise.all(
        skills.map((s) =>
          getSkillDetail(s.id).catch((err) => {
            console.warn("[ATO] Failed to get detail for skill", s.name, s.id, err);
            return null;
          })
        )
      ).then((details) => {
        const validDetails = details.filter((d): d is SkillDetail => d !== null);
        console.log("[ATO] Automation: got", validDetails.length, "valid details from", details.length, "total");
        for (const d of validDetails) {
          console.log("[ATO] Skill:", d.name, "content length:", d.content?.length ?? 0, "first 100:", d.content?.slice(0, 100));
        }
        const skillWorkflows = generateWorkflowsFromSkills(validDetails);
        console.log("[ATO] Automation: generated", skillWorkflows.length, "workflows");
        if (skillWorkflows.length > 0) {
          const store = useAutomationStore.getState();
          // Merge skill workflows with existing ones (avoid duplicates)
          const existingIds = new Set(store.workflows.map((w) => w.id));
          const newWorkflows = skillWorkflows.filter((w) => !existingIds.has(w.id));
          console.log("[ATO] Automation: new workflows to add:", newWorkflows.length);
          if (newWorkflows.length > 0) {
            store.loadWorkflows([...store.workflows, ...newWorkflows]);
          }
        }
      });
    }
  }, [skills]);

  // Also load OpenClaw cron jobs as workflows
  useEffect(() => {
    openclawListCronJobs().then((result) => {
      const raw = (result as Record<string, unknown>)?.jobs ?? [];
      if (!Array.isArray(raw) || raw.length === 0) return;

      const ocWorkflows: WorkflowType[] = (raw as Record<string, unknown>[]).map((job) => {
        const name = (job.name as string) || "Unnamed";
        const id = `oc-wf-${job.id || name}`;
        const prompt = ((job.payload as Record<string, unknown>)?.message as string) || "";
        const schedule = job.schedule as Record<string, unknown> | undefined;
        const delivery = job.delivery as Record<string, unknown> | undefined;
        const state = job.state as Record<string, unknown> | undefined;
        const enabled = job.enabled !== false;

        // Schedule label
        let scheduleLabel = "Scheduled";
        if (schedule?.kind === "every" && schedule?.everyMs) {
          const ms = schedule.everyMs as number;
          scheduleLabel = ms >= 86400000 ? `Every ${Math.round(ms / 86400000)}d` : ms >= 3600000 ? `Every ${Math.round(ms / 3600000)}h` : `Every ${Math.round(ms / 60000)}m`;
        } else if (schedule?.kind === "cron") {
          scheduleLabel = (schedule.expression as string) || "Cron";
        }

        const agentId = (job.agentId as string) || "main";
        const sessionKey = (job.sessionKey as string) || "";
        const deliveryChannel = (delivery?.channel as string) || "";
        const deliveryTo = (delivery?.to as string) || "";

        // Parse numbered steps from prompt (1. Do X, 2. Do Y, etc.)
        const promptSteps = prompt.match(/\d+\.\s+[^\n]+/g) || [];
        const agentNameStr = agentId === "main" ? "Growdor" : agentId;

        const nodes: FlowNode[] = [
          {
            id: `${id}-trigger`, label: scheduleLabel, description: `Trigger: ${name}`,
            type: "trigger", runtime: "openclaw",
            x: 50, y: 180, stats: { executions: 0, errors: 0, avgTimeMs: 0 },
            status: enabled ? "active" : "idle",
          },
        ];

        if (promptSteps.length >= 2) {
          // Multiple steps in the prompt — show each as a node
          promptSteps.forEach((step, i) => {
            const stepLabel = step.replace(/^\d+\.\s+/, "").slice(0, 40);
            const isLast = i === promptSteps.length - 1;
            nodes.push({
              id: `${id}-step-${i}`, label: stepLabel, description: step.replace(/^\d+\.\s+/, "").slice(0, 80),
              type: isLast ? "output" : i === 0 ? "action" : "process",
              runtime: "openclaw",
              agentId, agentName: agentNameStr,
              x: 50 + (i + 1) * 230, y: 180,
              stats: { executions: 0, errors: 0, avgTimeMs: 0 },
              status: state?.lastRunStatus === "ok" ? "active" : "idle",
            });
          });
        } else {
          // Single action node
          nodes.push({
            id: `${id}-action`, label: name, description: prompt.slice(0, 80),
            type: "action", runtime: "openclaw",
            agentId, agentName: agentNameStr,
            tool: sessionKey.includes("discord") ? "Discord" : sessionKey.includes("slack") ? "Slack" : undefined,
            x: 310, y: 180, stats: { executions: 0, errors: 0, avgTimeMs: 0 },
            status: state?.lastRunStatus === "ok" ? "active" : state?.lastRunStatus === "error" ? "error" : "idle",
          });
        }

        if (deliveryChannel) {
          const lastNode = nodes[nodes.length - 1];
          nodes.push({
            id: `${id}-delivery`, label: deliveryChannel,
            description: `Deliver to ${deliveryTo || deliveryChannel}`,
            type: "service", service: deliveryChannel, runtime: "openclaw",
            tool: deliveryChannel,
            x: lastNode.x + 230, y: 180,
            stats: { executions: 0, errors: 0, avgTimeMs: 0 },
            status: "idle",
          });
        }

        const edges: FlowEdge[] = [];
        for (let i = 0; i < nodes.length - 1; i++) {
          edges.push({ from: nodes[i].id, to: nodes[i + 1].id, animated: i === 0 });
        }

        return { id, name: `⚡ ${name}`, description: `OpenClaw: ${scheduleLabel}`, enabled, runCount: 0, errorCount: 0, nodes, edges, source: "cron" as const };
      });

      const store = useAutomationStore.getState();
      const existingIds = new Set(store.workflows.map((w) => w.id));
      const newOc = ocWorkflows.filter((w) => !existingIds.has(w.id));
      if (newOc.length > 0) {
        store.loadWorkflows([...store.workflows, ...newOc]);
      }
    }).catch(() => { /* OpenClaw not connected, skip */ });
  }, []);

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
