import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { cn } from "@/lib/utils";
import { X, Globe, Activity, Workflow, BarChart3 } from "lucide-react";
import { TYPE_COLORS, SERVICE_COLORS, SERVICE_ICONS, NODE_ICONS } from "./loops/constants";
import { useUiStore } from "@/stores/useUiStore";
import { serializeWorkflowToPrompt } from "./loops/helpers";
import { useAutomationStore } from "@/stores/useLoopStore";
import WorkflowToolbar from "./loops/WorkflowToolbar";
import NodePalette from "./loops/NodePalette";
import NodeConfigPanel from "./loops/NodeConfigPanel";
import FlowCanvas from "./loops/FlowCanvas";
import ExecutionOverlay from "./loops/ExecutionOverlay";
import { promptAgent, openclawListCronJobs } from "@/lib/api";
import { getSkills, getSkillDetail } from "@/lib/api";
import { generateWorkflowsFromSkills } from "@/lib/skill-to-workflow";
import { groupsToWorkflows, cronsToWorkflows, hooksToWorkflows, decorateWorkflowsWithStatus } from "@/lib/loopsAggregator";
import { listAgentGroups } from "@/lib/agentGroups";
import { listAgents } from "@/lib/agents";
import { listAgentHooks } from "@/lib/agentHooks";
import { listCronJobs, migrateWorkflowsToLoops } from "@/lib/tauri-api";
import { getAgentMetrics } from "@/lib/agentObservability";
import { create_loop, delete_loop, list_loops, run_loop_by_slug, toggle_loop_enabled, update_loop } from "@/lib/loops-api";
import { loopToWorkflow, workflowToLoopCreateInput, workflowToLoopUpdateInput } from "./loops/loopMapper";
import type { SkillDetail } from "@/lib/api";
import type { Workflow as WorkflowType, FlowNode, FlowEdge } from "./loops/types";

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export default function LoopComposer() {
  const { t } = useTranslation();
  const [legacyWorkflowCount, setLegacyWorkflowCount] = useState<number | null>(null);
  // Codex R2 — guards the migration UI so concurrent fires can't race
  // (startup auto-check + button click + Run during in-flight migration).
  const [isMigrating, setIsMigrating] = useState(false);
  const [migrationError, setMigrationError] = useState<string | null>(null);
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
    toggleWorkflow,
    deleteWorkflow,
  } = useAutomationStore();

  // Load skill-based workflows (gstack etc.) on mount
  // Fetch skill list, then load details for each to parse automation steps
  const { data: skills = [] } = useQuery({
    queryKey: ["skills"],
    queryFn: getSkills,
  });

  // v1.6.0 wave 1 — also load agent groups so the canvas shows them
  // alongside skill flows. Sequential groups render as left-to-right
  // pipelines; routed groups render as router-fanout to children.
  const { data: agentGroups = [] } = useQuery({
    queryKey: ["agent-groups-for-automations"],
    queryFn: () => listAgentGroups(),
    staleTime: 30_000,
  });

  // v1.6.0 wave 2 — agents + their hooks + cron jobs. Each becomes a
  // Workflow on the canvas:
  //   - cron jobs: clock-trigger node → dispatch target
  //   - hooks: input nodes feeding into their parent agent
  // The agent list also resolves the dispatch target on cron workflows.
  const { data: allAgents = [] } = useQuery({
    queryKey: ["agents-for-automations"],
    queryFn: () => listAgents(),
    staleTime: 30_000,
  });
  const { data: allCronJobs = [] } = useQuery({
    queryKey: ["cron-jobs-for-automations"],
    queryFn: () => listCronJobs(),
    staleTime: 30_000,
  });
  const { data: hooksByAgent = new Map() } = useQuery<Map<string, import("@/lib/agentHooks").AgentHook[]>>({
    queryKey: ["agent-hooks-for-automations", allAgents.map((a) => a.id).sort().join(",")],
    queryFn: async () => {
      const out = new Map<string, import("@/lib/agentHooks").AgentHook[]>();
      // Fan out one query per agent. Each agent's hook count is small
      // (typically 0-3), and listAgentHooks itself is one SQLite scan.
      await Promise.all(
        allAgents.map(async (a) => {
          try {
            const hooks = await listAgentHooks(a.id);
            if (hooks.length > 0) out.set(a.id, hooks);
          } catch {
            // ignore — best-effort.
          }
        })
      );
      return out;
    },
    enabled: allAgents.length > 0,
    staleTime: 30_000,
  });

  // v1.6.0 wave 3 — pull per-agent metrics from agent-logs.jsonl so we
  // can paint live status on each node (idle / active / error) instead
  // of every node showing "idle" forever. Refetches every 30s.
  const { data: agentMetrics } = useQuery({
    queryKey: ["agent-metrics-for-automations"],
    queryFn: () => getAgentMetrics({ limit: 500 }),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  // v2.14 — one-shot migration of v2.13 file-based workflows into the
  // new SQLite `loops` table. Idempotent on the Rust side (re-running
  // skips rows already tagged with source='migrated-from-automations').
  // Logs the report to the console so an upgrading user can confirm
  // their old automations were picked up.
  useEffect(() => {
    migrateWorkflowsToLoops()
      .then((report) => {
        if (report.scanned > 0) {
          console.log("[ATO] Loop Composer migration:", report);
        }
      })
      .catch((err) => {
        console.warn("[ATO] Loop Composer migration failed:", err);
      });
  }, []);

  const refreshLoops = useCallback(() => {
    list_loops()
      .then((loops) => {
        const persisted = loops.map(loopToWorkflow);
        const store = useAutomationStore.getState();
        const nonPersisted = store.workflows.filter((w) => w.source !== "manual" || !UUID_RE.test(w.id));
        store.loadWorkflows([...persisted, ...nonPersisted]);
      })
      .catch((err) => {
        console.warn("[ATO] Loop Composer load failed:", err);
      });
  }, []);

  useEffect(() => {
    refreshLoops();
  }, [refreshLoops]);

  useEffect(() => {
    try {
      if (localStorage.getItem("ato-workflows-migration-dismissed") === "1") return;
      const raw = localStorage.getItem("ato-workflows");
      if (!raw || !raw.trim()) return;
      const parsed = JSON.parse(raw) as unknown;
      if (!Array.isArray(parsed) || parsed.length === 0) return;
      setLegacyWorkflowCount(parsed.length);
    } catch (err) {
      console.warn("[ATO] Loop Composer localStorage migration check failed:", err);
    }
  }, []);

  useEffect(() => {
    const groupWorkflows = groupsToWorkflows(agentGroups);
    const cronWorkflows = cronsToWorkflows(allCronJobs, allAgents, agentGroups);
    const hookWorkflows = hooksToWorkflows(allAgents, hooksByAgent);
    let incoming = [...groupWorkflows, ...cronWorkflows, ...hookWorkflows];
    // Decorate with live status when metrics are available.
    if (agentMetrics?.perAgent && agentMetrics.perAgent.length > 0) {
      incoming = decorateWorkflowsWithStatus(incoming, agentMetrics.perAgent);
    }
    if (incoming.length === 0) return;
    const store = useAutomationStore.getState();
    const existingIds = new Set(store.workflows.map((w) => w.id));
    const newOnes = incoming.filter((w) => !existingIds.has(w.id));
    if (newOnes.length > 0) {
      store.loadWorkflows([...store.workflows, ...newOnes]);
    }
  }, [agentGroups, allCronJobs, allAgents, hooksByAgent, agentMetrics]);

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
        const rawDeliveryChannel = (delivery?.channel as string) || "";
        const deliveryTo = (delivery?.to as string) || "";

        // Normalize delivery channel — infer from channel name, `to` field, or delivery metadata
        const DELIVERY_MAP: Record<string, string> = {
          telegram: "telegram", tg: "telegram",
          email: "email", resend: "resend",
          slack: "slack", discord: "discord",
          announce: "announce", webhook: "webhook",
        };
        function resolveDeliveryChannel(channel: string, to: string): string {
          const lower = channel.toLowerCase();
          if (DELIVERY_MAP[lower]) return DELIVERY_MAP[lower];
          // Infer from `to` field patterns
          if (to.startsWith("chat:") || to.includes("telegram")) return "telegram";
          if (to.includes("@") && to.includes(".")) return "email";
          if (to.startsWith("#") || to.includes("slack")) return "slack";
          // If channel is a generic placeholder like "last", skip it
          if (["last", "none", "stdout", ""].includes(lower)) return "";
          return channel;
        }
        const deliveryChannel = resolveDeliveryChannel(rawDeliveryChannel, deliveryTo);

        // Parse numbered steps from prompt (1. Do X, 2. Do Y, etc.)
        const promptSteps = prompt.match(/\d+\.\s+[^\n]+/g) || [];
        const agentNameStr = agentId === "main" ? "Growdor" : agentId;

        // Detect tools/APIs mentioned in prompt text
        // Tool detection uses regex patterns to avoid false positives
        // e.g. "emails sent" should NOT match as Email API
        const KNOWN_TOOLS: [RegExp, string][] = [
          [/\bresend\b/i, "Resend API"],
          [/\bemail\s*api\b/i, "Email API"],
          [/\bsend\s*email/i, "Email API"],
          [/\bgh\s+cli\b/i, "GitHub CLI"],
          [/\bgh\s+api\b/i, "GitHub API"],
          [/\bgithub\b/i, "GitHub"],
          [/\bdiscord\b/i, "Discord"],
          [/\bslack\b/i, "Slack"],
          [/\btwitter\b/i, "X/Twitter"],
          [/\bnotion\b/i, "Notion"],
          [/\blinear\b/i, "Linear"],
          [/\bpostgres/i, "PostgreSQL"],
          [/\bredis\b/i, "Redis"],
          [/\bhttp\.server\b/i, "HTTP Server"],
          [/\bpuppeteer\b/i, "Browser"],
          [/\bchromium\b/i, "Browser"],
          [/\btelegram\b/i, "Telegram"],
        ];
        function detectTools(text: string): string[] {
          const seen = new Set<string>();
          const results: string[] = [];
          for (const [pattern, label] of KNOWN_TOOLS) {
            if (pattern.test(text) && !seen.has(label)) {
              seen.add(label);
              results.push(label);
            }
          }
          return results;
        }

        // Layout constants — 3 rows
        const ROW_TOOLS = 50;    // top row: APIs/tools
        const ROW_ACTIONS = 180; // middle row: action steps
        const ROW_AGENTS = 310;  // bottom row: agents
        const COL_START = 50;
        const COL_SPACING = 230;

        const nodes: FlowNode[] = [];
        const edges: FlowEdge[] = [];

        // Trigger node (left, on action row)
        nodes.push({
          id: `${id}-trigger`, label: scheduleLabel, description: `Trigger: ${name}`,
          type: "trigger", runtime: "openclaw",
          x: COL_START, y: ROW_ACTIONS, stats: { executions: 0, errors: 0, avgTimeMs: 0 },
          status: enabled ? "active" : "idle",
        });

        // Build action steps
        const steps = promptSteps.length >= 2
          ? promptSteps.map((s) => s.replace(/^\d+\.\s+/, ""))
          : [prompt || name];

        steps.forEach((stepText, i) => {
          const col = COL_START + (i + 1) * COL_SPACING;
          const stepLabel = stepText.slice(0, 40);
          const isLast = i === steps.length - 1;
          const stepId = `${id}-step-${i}`;

          // Middle row: action step (WHAT)
          nodes.push({
            id: stepId, label: stepLabel, description: stepText.slice(0, 80),
            type: isLast && !deliveryChannel ? "output" : i === 0 ? "action" : "process",
            runtime: "openclaw",
            x: col, y: ROW_ACTIONS, stats: { executions: 0, errors: 0, avgTimeMs: 0 },
            status: state?.lastRunStatus === "ok" ? "active" : "idle",
          });

          // Connect horizontally
          const prevId = i === 0 ? `${id}-trigger` : `${id}-step-${i - 1}`;
          edges.push({ from: prevId, to: stepId, animated: i === 0 });

          // Top row: detected tools for this step (HOW)
          const stepTools = detectTools(stepText);
          if (stepTools.length > 0) {
            const toolId = `${id}-tool-${i}`;
            nodes.push({
              id: toolId, label: stepTools[0], description: stepTools.join(", "),
              type: "service", service: stepTools[0].toLowerCase().split(" ")[0],
              runtime: "openclaw", tool: stepTools[0],
              x: col, y: ROW_TOOLS, stats: { executions: 0, errors: 0, avgTimeMs: 0 },
              status: "idle",
            });
            edges.push({ from: toolId, to: stepId });
          }

          // Bottom row: agent (WHO) — show once at first step
          if (i === 0) {
            const agentNodeId = `${id}-agent`;
            nodes.push({
              id: agentNodeId, label: agentNameStr, description: `Agent: ${agentId}`,
              type: "process", runtime: "openclaw",
              agentName: agentNameStr,
              x: col, y: ROW_AGENTS, stats: { executions: 0, errors: 0, avgTimeMs: 0 },
              status: "active",
            });
            edges.push({ from: agentNodeId, to: stepId });
          }
        });

        // Delivery node (end of action row)
        if (deliveryChannel) {
          const lastStepX = COL_START + (steps.length + 1) * COL_SPACING;
          const deliveryId = `${id}-delivery`;
          nodes.push({
            id: deliveryId, label: deliveryChannel,
            description: `Deliver to ${deliveryTo || deliveryChannel}`,
            type: "service", service: deliveryChannel, runtime: "openclaw",
            tool: deliveryChannel,
            x: lastStepX, y: ROW_ACTIONS, stats: { executions: 0, errors: 0, avgTimeMs: 0 },
            status: "idle",
          });
          const lastStepId = `${id}-step-${steps.length - 1}`;
          edges.push({ from: lastStepId, to: deliveryId });
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

  const replaceWorkflow = useCallback((nextWorkflow: WorkflowType, priorId?: string) => {
    const store = useAutomationStore.getState();
    const currentId = priorId ?? nextWorkflow.id;
    const nextWorkflows = store.workflows.map((item) => (item.id === currentId ? nextWorkflow : item));
    store.loadWorkflows(nextWorkflows);
    store.setActiveWorkflowId(nextWorkflow.id);
  }, []);

  // Save workflow to loops table
  const handleSave = useCallback(async () => {
    if (workflow.triggerKind === "cron" && !workflow.triggerConfig?.cron?.trim()) {
      window.alert(t("loopComposer.trigger.validationCron"));
      return;
    }
    if (workflow.triggerKind === "event" && !workflow.triggerConfig?.event?.trim()) {
      window.alert(t("loopComposer.trigger.validationEvent"));
      return;
    }
    try {
      if (UUID_RE.test(workflow.id)) {
        const saved = await update_loop(workflow.id, workflowToLoopUpdateInput(workflow));
        replaceWorkflow(loopToWorkflow(saved));
      } else {
        const created = await create_loop(workflowToLoopCreateInput(workflow));
        replaceWorkflow(loopToWorkflow(created), workflow.id);
      }
      markSaved();
    } catch {
      markSaved();
    }
  }, [workflow, markSaved, replaceWorkflow, t]);

  const handleToggle = useCallback(async () => {
    if (!workflow.id) return;
    if (!UUID_RE.test(workflow.id)) {
      toggleWorkflow(workflow.id);
      return;
    }
    const enabled = !workflow.enabled;
    const saved = await toggle_loop_enabled(workflow.id, enabled);
    replaceWorkflow(loopToWorkflow(saved));
  }, [workflow, toggleWorkflow, replaceWorkflow]);

  const handleDelete = useCallback(async () => {
    if (!workflow.id) return;
    if (UUID_RE.test(workflow.id)) {
      await delete_loop(workflow.id);
    }
    deleteWorkflow(workflow.id);
  }, [workflow, deleteWorkflow]);

  // Execute workflow via Claude CLI
  const handleRun = useCallback(async () => {
    if (execution.running) return;
    if (workflow.nodes.length === 0) return;
    // Codex R2 — block Run while migration is in flight: the migration
    // can rewrite the workflow's underlying row, and firing the loop
    // mid-migration would risk dispatching against stale data or a
    // half-written loops entry.
    if (isMigrating) return;

    const prompt = serializeWorkflowToPrompt(workflow);
    startExecution();

    // Set all nodes to pending
    for (const node of workflow.nodes) {
      updateNodeExecStatus(node.id, "pending");
    }

    try {
      // v2.14 step 3 — for loops that are persisted in the v2.14 Loop
      // table (UUID id), fire the real `ato loop run <slug>` engine
      // via the new run_loop_by_slug Tauri command. The CLI writes
      // loop_runs + loop_run_steps with attribution. Fall back to the
      // legacy promptAgent shim for browser-only workflows (cron/skill
      // imports) that haven't been persisted to the Loop table yet.
      const isPersistedLoop = typeof workflow.id === "string"
        && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(workflow.id);

      if (isPersistedLoop) {
        const result = await run_loop_by_slug(workflow.id);
        for (const node of workflow.nodes) {
          updateNodeExecStatus(node.id, "completed");
        }
        appendOutput(`Loop run started — run_id=${result.runId} status=${result.status}`);
        finishExecution();
        return;
      }

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
  }, [workflow, execution.running, isMigrating, startExecution, updateNodeExecStatus, appendOutput, finishExecution]);

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
      {legacyWorkflowCount !== null && (
        <div className="flex items-center justify-between gap-3 border-b border-[#2a2a3a] bg-[#16161e] px-4 py-2">
          <p className="text-sm text-[#e8e8f0]">
            {isMigrating
              ? t("loopComposer.migration.inProgress", { count: legacyWorkflowCount, defaultValue: "Migrating {{count}} workflows…" })
              : migrationError
                ? t("loopComposer.migration.error", { defaultValue: "Migration failed — try again." })
                : t("loopComposer.migration.banner", { count: legacyWorkflowCount })}
          </p>
          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={isMigrating}
              onClick={async () => {
                if (isMigrating) return;
                setIsMigrating(true);
                setMigrationError(null);
                try {
                  const result = await migrateWorkflowsToLoops();
                  // Codex R2 — only clear localStorage when EVERY legacy
                  // entry was migrated. If any were skipped (dup, parse
                  // error, etc.) keep them around so the user can retry.
                  const migrated = (result as { migrated?: number } | null | undefined)?.migrated ?? 0;
                  const skipped = (result as { skipped?: number } | null | undefined)?.skipped ?? 0;
                  if (skipped === 0 && migrated > 0) {
                    localStorage.removeItem("ato-workflows");
                    setLegacyWorkflowCount(null);
                  } else if (skipped > 0) {
                    setMigrationError(`${migrated} migrated, ${skipped} skipped`);
                  } else {
                    // No rows actually migrated; treat as dismiss.
                    localStorage.removeItem("ato-workflows");
                    setLegacyWorkflowCount(null);
                  }
                  refreshLoops();
                } catch (err) {
                  setMigrationError(err instanceof Error ? err.message : String(err));
                } finally {
                  setIsMigrating(false);
                }
              }}
              className={cn(
                "rounded-md border px-3 py-1.5 text-xs transition-colors",
                isMigrating
                  ? "border-[#2a2a3a] bg-[#0e0e16] text-[#8888a0] cursor-not-allowed"
                  : "border-[#00FFB2]/40 bg-[#00FFB210] text-[#00FFB2] hover:bg-[#00FFB220]"
              )}
            >
              {isMigrating
                ? t("loopComposer.migration.migrating", { defaultValue: "Migrating…" })
                : t("loopComposer.migration.migrate")}
            </button>
            <button
              type="button"
              disabled={isMigrating}
              onClick={() => {
                if (isMigrating) return;
                localStorage.setItem("ato-workflows-migration-dismissed", "1");
                setLegacyWorkflowCount(null);
              }}
              className={cn(
                "rounded-md border border-[#2a2a3a] px-3 py-1.5 text-xs transition-colors",
                isMigrating
                  ? "text-[#3a3a4a] cursor-not-allowed"
                  : "text-[#8888a0] hover:text-[#e8e8f0]"
              )}
            >
              {t("loopComposer.migration.dismiss")}
            </button>
          </div>
        </div>
      )}

      {/* Toolbar */}
      <WorkflowToolbar onRun={handleRun} onSave={handleSave} onToggle={handleToggle} onDelete={handleDelete} />

      <div className="flex flex-1 overflow-hidden">
        {/* Node Palette (edit mode only) */}
        {mode === "edit" && <NodePalette onDragStart={() => {}} />}

        {/* Canvas + right panel */}
        <FlowCanvas />

        {/* Config Panel (edit mode, when node selected) */}
        {mode === "edit" && (
          <NodeConfigPanel
            node={selectedNode}
            workflow={workflow}
            onDelete={() => {
              if (!selectedNode) return;
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

            <div className="flex items-center gap-2 flex-shrink-0">
              {/* v1.6.0 — click-through to Insights → Agents pre-
                  filtered for this node's agent, so users can jump
                  from "this routed group's @reviewer step" straight
                  into its trace explorer without re-navigating. */}
              {selectedNode.agentName && (
                <button
                  onClick={() => {
                    const ui = useUiStore.getState();
                    ui.setSection("insights");
                    try {
                      localStorage.setItem("ato.subtab.insights", "agents");
                      // Soft hint for AgentObservability to preselect
                      // this slug; component reads + clears it on mount.
                      localStorage.setItem(
                        "ato.insights.preselectAgentSlug",
                        selectedNode.agentName ?? "",
                      );
                    } catch {
                      // localStorage failure is non-fatal — user lands
                      // on the Insights tab without preselection.
                    }
                  }}
                  className="inline-flex items-center gap-1 rounded-md border border-[#2a2a3a] bg-[#0e0e16] px-2 py-1 text-[11px] text-[#8888a0] hover:text-[#00FFB2] hover:border-[#00FFB2]/40 transition-colors"
                  title={t(
                    "automation.openInInsights",
                    "Open this agent's trace history in Insights",
                  )}
                >
                  <BarChart3 size={11} />
                  {t("automation.viewRuns", "View runs")}
                </button>
              )}
              <button
                onClick={() => selectNode(null)}
                className="flex items-center justify-center w-7 h-7 rounded-md border border-[#2a2a3a] hover:border-[#FF4466] transition-colors"
                style={{ background: "#0e0e16" }}
              >
                <X size={14} className="text-[#8888a0]" />
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Execution overlay */}
      <ExecutionOverlay />
    </div>
  );
}
