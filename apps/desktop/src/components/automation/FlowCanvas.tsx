import { useRef, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { ZoomIn, ZoomOut, Globe, Activity, Search, X } from "lucide-react";
import { NODE_W, NODE_H, TYPE_COLORS, SERVICE_COLORS, SERVICE_ICONS, NODE_ICONS } from "./constants";
import { getConnectionPoints, buildBezierPath, screenToCanvas, wouldCreateCycle } from "./helpers";
import FlowNodeCard from "./FlowNodeCard";
import { useAutomationStore } from "@/stores/useAutomationStore";
import type { FlowNode, NodeTemplate } from "./types";

export default function FlowCanvas() {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(0.85);
  const [panOffset, setPanOffset] = useState({ x: 20, y: 20 });
  const [isPanning, setIsPanning] = useState(false);
  const [panStart, setPanStart] = useState({ x: 0, y: 0 });
  const [draggingNodeId, setDraggingNodeId] = useState<string | null>(null);
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 });
  const [searchQuery, setSearchQuery] = useState("");

  const {
    mode,
    getActiveWorkflow,
    selectedNodeId,
    selectedEdgeKey,
    selectNode,
    selectEdge,
    moveNode,
    addNode,
    connecting,
    startConnecting,
    cancelConnecting,
    addEdge,
    deleteNode,
    deleteEdge,
    execution,
  } = useAutomationStore();

  const workflow = getActiveWorkflow();
  const nodes = workflow.nodes;
  const edges = workflow.edges;

  const activeWorkflowId = useAutomationStore((s) => s.activeWorkflowId);

  // Reset pan when switching workflows
  useEffect(() => {
    setPanOffset({ x: 20, y: 20 });
  }, [activeWorkflowId]);

  // Canvas dimensions
  const canvasW = Math.max(1400, ...nodes.map((n) => n.x + NODE_W + 80));
  const canvasH = Math.max(450, ...nodes.map((n) => n.y + NODE_H + 80));

  const nodeMap = new Map(nodes.map((n) => [n.id, n]));

  // Zoom with wheel
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const handler = (e: WheelEvent) => {
      e.preventDefault();
      const delta = e.deltaY > 0 ? -0.05 : 0.05;
      setScale((s) => Math.min(2, Math.max(0.3, s + delta)));
    };
    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  }, []);

  // Keyboard: delete selected node/edge
  useEffect(() => {
    if (mode !== "edit") return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Delete" || e.key === "Backspace") {
        if (document.activeElement?.tagName === "INPUT" || document.activeElement?.tagName === "TEXTAREA" || document.activeElement?.tagName === "SELECT") return;
        if (selectedNodeId) {
          deleteNode(selectedNodeId);
        } else if (selectedEdgeKey) {
          const [from, to] = selectedEdgeKey.split("->>");
          deleteEdge(from, to);
        }
      }
      if (e.key === "Escape") {
        cancelConnecting();
        selectNode(null);
        selectEdge(null);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [mode, selectedNodeId, selectedEdgeKey, deleteNode, deleteEdge, cancelConnecting, selectNode, selectEdge]);

  // Pan handlers
  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (e.button !== 0) return;
      if ((e.target as HTMLElement).closest("[data-node]")) return;
      if (connecting) {
        cancelConnecting();
        return;
      }
      setIsPanning(true);
      setPanStart({ x: e.clientX - panOffset.x, y: e.clientY - panOffset.y });
    },
    [panOffset, connecting, cancelConnecting]
  );

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (isPanning) {
        setPanOffset({ x: e.clientX - panStart.x, y: e.clientY - panStart.y });
        return;
      }
      if (draggingNodeId && mode === "edit") {
        const rect = canvasRef.current?.getBoundingClientRect();
        if (!rect) return;
        const pos = screenToCanvas(e.clientX, e.clientY, rect, panOffset, scale);
        moveNode(draggingNodeId, pos.x - dragStart.x, pos.y - dragStart.y);
      }
    },
    [isPanning, panStart, draggingNodeId, mode, panOffset, scale, dragStart, moveNode]
  );

  const handleMouseUp = useCallback(() => {
    setIsPanning(false);
    setDraggingNodeId(null);
  }, []);

  // Drop handler for palette items
  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      const data = e.dataTransfer.getData("application/automation-node");
      if (!data) return;

      const template: NodeTemplate = JSON.parse(data);
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const pos = screenToCanvas(e.clientX, e.clientY, rect, panOffset, scale);

      const newNode: FlowNode = {
        id: `node-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        label: template.label,
        description: template.description,
        type: template.type,
        service: template.service,
        x: pos.x - NODE_W / 2,
        y: pos.y - NODE_H / 2,
        stats: { executions: 0, errors: 0, avgTimeMs: 0 },
        status: "idle",
        config: template.action ? { params: { action: template.action } } : { params: {} },
      };

      addNode(newNode);
      selectNode(newNode.id);
    },
    [panOffset, scale, addNode, selectNode]
  );

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
  }, []);

  // Node pointer down for dragging
  function handleNodePointerDown(nodeId: string, e: React.PointerEvent) {
    if (mode !== "edit") return;
    e.stopPropagation();
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const pos = screenToCanvas(e.clientX, e.clientY, rect, panOffset, scale);
    const node = nodeMap.get(nodeId);
    if (!node) return;
    setDraggingNodeId(nodeId);
    setDragStart({ x: pos.x - node.x, y: pos.y - node.y });
  }

  // Port click handlers
  function handleOutputPortClick(nodeId: string) {
    if (connecting) {
      // Already connecting from another node — cancel
      cancelConnecting();
    }
    startConnecting(nodeId);
  }

  function handleInputPortClick(nodeId: string) {
    if (!connecting) return;
    if (connecting.fromNodeId === nodeId) {
      cancelConnecting();
      return;
    }
    // Check for cycles
    if (wouldCreateCycle(edges, connecting.fromNodeId, nodeId)) {
      cancelConnecting();
      return;
    }
    addEdge({ from: connecting.fromNodeId, to: nodeId });
  }

  const zoomIn = () => setScale((s) => Math.min(2, s + 0.1));
  const zoomOut = () => setScale((s) => Math.max(0.3, s - 0.1));

  // Stats for right panel
  const services = [...new Set(nodes.filter((n) => n.service).map((n) => n.service!))];
  const filteredNodes = nodes.filter((n) =>
    n.label.toLowerCase().includes(searchQuery.toLowerCase()) ||
    (n.service || "").toLowerCase().includes(searchQuery.toLowerCase())
  );

  return (
    <div className="flex flex-1 overflow-hidden">
      {/* Canvas */}
      <div
        ref={canvasRef}
        className="relative flex-1 overflow-hidden"
        style={{ cursor: connecting ? "crosshair" : isPanning ? "grabbing" : "grab" }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
        onDrop={handleDrop}
        onDragOver={handleDragOver}
      >
        {/* Empty state */}
        {nodes.length === 0 && mode === "view" && (
          <div className="absolute inset-0 flex items-center justify-center">
            <div className="text-center">
              <p className="text-sm text-[#8888a0] mb-2">
                {t("automation.builder.emptyState", "No workflows yet")}
              </p>
              <p className="text-xs text-[#8888a0]/60">
                {t("automation.builder.emptyStateHint", "Switch to Edit mode to create your first workflow")}
              </p>
            </div>
          </div>
        )}

        {nodes.length === 0 && mode === "edit" && (
          <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
            <div className="text-center">
              <p className="text-sm text-[#8888a0] mb-2">
                {t("automation.builder.dropHint", "Drag nodes from the palette")}
              </p>
            </div>
          </div>
        )}

        <div
          style={{
            transform: `translate(${panOffset.x}px, ${panOffset.y}px) scale(${scale})`,
            transformOrigin: "0 0",
            width: canvasW,
            height: canvasH,
            position: "relative",
          }}
        >
          {/* SVG layer */}
          <svg
            width={canvasW}
            height={canvasH}
            className="absolute inset-0"
            style={{ zIndex: 1 }}
          >
            <defs>
              <pattern id="grid" width="20" height="20" patternUnits="userSpaceOnUse">
                <circle cx="10" cy="10" r="1" fill="#2a2a3a" />
              </pattern>
              <style>{`
                @keyframes flowDash {
                  to { stroke-dashoffset: -20; }
                }
                .edge-animated {
                  stroke-dasharray: 6 4;
                  animation: flowDash 0.8s linear infinite;
                }
              `}</style>
            </defs>

            <rect width={canvasW} height={canvasH} fill="url(#grid)" />

            {edges.map((edge) => {
              const fromNode = nodeMap.get(edge.from);
              const toNode = nodeMap.get(edge.to);
              if (!fromNode || !toNode) return null;

              const { x1, y1, x2, y2 } = getConnectionPoints(fromNode, toNode);
              const path = buildBezierPath(x1, y1, x2, y2);
              const edgeColor = toNode.service
                ? SERVICE_COLORS[toNode.service] || "#00FFB2"
                : "#00FFB2";
              const isActive = edge.animated || fromNode.status === "active" || toNode.status === "active";
              const edgeKey = `${edge.from}->>${edge.to}`;
              const isEdgeSelected = selectedEdgeKey === edgeKey;

              return (
                <g
                  key={edgeKey}
                  onClick={(e) => {
                    e.stopPropagation();
                    if (mode === "edit") selectEdge(edgeKey);
                  }}
                  style={{ cursor: mode === "edit" ? "pointer" : "default" }}
                >
                  {/* Hit area (wider invisible path) */}
                  <path d={path} fill="none" stroke="transparent" strokeWidth={12} />
                  <path d={path} fill="none" stroke={edgeColor} strokeWidth={3} strokeOpacity={0.08} />
                  <path
                    d={path}
                    fill="none"
                    stroke={isEdgeSelected ? "#FF4466" : edgeColor}
                    strokeWidth={isEdgeSelected ? 2.5 : 1.5}
                    strokeOpacity={isActive || isEdgeSelected ? 0.7 : 0.3}
                    className={cn(edge.animated && "edge-animated")}
                  />
                  <circle cx={x1} cy={y1} r={3} fill={edgeColor} opacity={0.5} />
                  <circle cx={x2} cy={y2} r={3} fill={edgeColor} opacity={0.5} />
                </g>
              );
            })}
          </svg>

          {/* Edge labels */}
          {edges.map((edge) => {
            if (!edge.label) return null;
            const fromNode = nodeMap.get(edge.from);
            const toNode = nodeMap.get(edge.to);
            if (!fromNode || !toNode) return null;
            const { x1, y1, x2, y2 } = getConnectionPoints(fromNode, toNode);
            return (
              <div
                key={`label-${edge.from}-${edge.to}`}
                className="absolute pointer-events-none"
                style={{ left: (x1 + x2) / 2 - 30, top: (y1 + y2) / 2 - 10, zIndex: 5 }}
              >
                <span
                  className="rounded px-1.5 py-0.5 text-[#e8e8f0] font-medium"
                  style={{ fontSize: 9, background: "#16161e", border: "1px solid #2a2a3a" }}
                >
                  {edge.label}
                </span>
              </div>
            );
          })}

          {/* Node cards */}
          {nodes.map((node) => (
            <div key={node.id} data-node>
              <FlowNodeCard
                node={node}
                isSelected={selectedNodeId === node.id}
                mode={mode}
                execStatus={execution.nodeStatuses[node.id]}
                onClick={() => selectNode(selectedNodeId === node.id ? null : node.id)}
                onPointerDown={(e) => handleNodePointerDown(node.id, e)}
                onOutputPortClick={() => handleOutputPortClick(node.id)}
                onInputPortClick={() => handleInputPortClick(node.id)}
              />
            </div>
          ))}
        </div>

        {/* Zoom controls */}
        <div className="absolute bottom-4 right-4 flex flex-col gap-1" style={{ zIndex: 30 }}>
          <button
            onClick={zoomIn}
            className="flex items-center justify-center w-8 h-8 rounded-md border border-[#2a2a3a] hover:border-[#00FFB2] transition-colors"
            style={{ background: "#16161e" }}
          >
            <ZoomIn size={14} className="text-[#e8e8f0]" />
          </button>
          <button
            onClick={zoomOut}
            className="flex items-center justify-center w-8 h-8 rounded-md border border-[#2a2a3a] hover:border-[#00FFB2] transition-colors"
            style={{ background: "#16161e" }}
          >
            <ZoomOut size={14} className="text-[#e8e8f0]" />
          </button>
          <span className="text-center text-[10px] text-[#8888a0] mt-0.5" style={{ fontVariantNumeric: "tabular-nums" }}>
            {Math.round(scale * 100)}%
          </span>
        </div>
      </div>

      {/* Right panel (view mode) */}
      {mode === "view" && (
        <div className="w-72 flex-shrink-0 border-l border-[#2a2a3a] overflow-y-auto" style={{ background: "#0e0e16" }}>
          <div className="p-4">
            <h2 className="text-[#e8e8f0] font-semibold text-sm mb-1">
              {workflow.name}
            </h2>
            <p className="text-[11px] text-[#8888a0] mb-4">{workflow.description}</p>

            {/* Services used */}
            <div className="mb-4">
              <h3 className="text-[10px] text-[#8888a0] uppercase tracking-wider mb-2 font-medium">
                {t("automation.services", "Connected Services")}
              </h3>
              <div className="flex flex-wrap gap-1.5">
                {services.map((s) => {
                  const Icon = SERVICE_ICONS[s] || Globe;
                  const color = SERVICE_COLORS[s] || "#8888a0";
                  return (
                    <div
                      key={s}
                      className="flex items-center gap-1.5 rounded-md px-2 py-1 border"
                      style={{ borderColor: `${color}40`, background: `${color}10` }}
                    >
                      <Icon size={12} style={{ color }} />
                      <span className="text-xs font-medium capitalize" style={{ color }}>{s}</span>
                    </div>
                  );
                })}
              </div>
            </div>

            {/* Stats */}
            <div className="grid grid-cols-2 gap-2 mb-4">
              <div className="rounded-md border border-[#2a2a3a] px-2.5 py-2 text-center" style={{ background: "#16161e" }}>
                <p className="text-lg font-bold text-[#e8e8f0]">{workflow.runCount}</p>
                <p className="text-[10px] text-[#8888a0]">{t("automation.totalRuns", "Total Runs")}</p>
              </div>
              <div className="rounded-md border border-[#2a2a3a] px-2.5 py-2 text-center" style={{ background: "#16161e" }}>
                <p className={cn("text-lg font-bold", workflow.errorCount > 0 ? "text-[#FF4466]" : "text-[#e8e8f0]")}>
                  {workflow.errorCount}
                </p>
                <p className="text-[10px] text-[#8888a0]">{t("automation.errors", "Errors")}</p>
              </div>
            </div>

            {/* Search */}
            <div className="relative mb-3">
              <Search size={12} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-[#8888a0]" />
              <input
                type="text"
                placeholder={t("automation.search", "Search nodes...")}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] text-xs py-1.5 pl-7 pr-2 placeholder-[#8888a0] focus:outline-none focus:border-[#00FFB2] transition-colors"
              />
            </div>

            {/* Node list */}
            <h3 className="text-[10px] text-[#8888a0] uppercase tracking-wider mb-2 font-medium">
              {t("automation.flowSteps", "Flow Steps")} ({nodes.length})
            </h3>
            <div className="flex flex-col gap-1.5">
              {filteredNodes.map((node) => {
                const isService = node.type === "service" && node.service;
                const barColor = isService
                  ? SERVICE_COLORS[node.service!] || TYPE_COLORS.service
                  : TYPE_COLORS[node.type];
                const IconComponent = isService
                  ? SERVICE_ICONS[node.service!] || Globe
                  : NODE_ICONS[node.type] || Activity;
                const isSelected = selectedNodeId === node.id;

                return (
                  <button
                    key={node.id}
                    onClick={() => selectNode(isSelected ? null : node.id)}
                    className={cn(
                      "flex items-center gap-2 w-full rounded-md px-2.5 py-2 text-left transition-colors border",
                      isSelected
                        ? "border-[#00FFB2] bg-[#00FFB208]"
                        : "border-transparent hover:bg-[#16161e]"
                    )}
                  >
                    <IconComponent size={12} style={{ color: barColor, flexShrink: 0 }} />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-1.5">
                        <p className="text-[11px] font-medium text-[#e8e8f0] truncate">
                          {node.label}
                        </p>
                        {isService && (
                          <span className="text-[8px] font-bold uppercase" style={{ color: barColor }}>
                            {node.service}
                          </span>
                        )}
                      </div>
                      <p className="text-[10px] text-[#8888a0]">
                        {node.stats.executions} runs
                        {node.stats.errors > 0 && (
                          <span className="text-[#FF4466] ml-1">{node.stats.errors} err</span>
                        )}
                        <span className="ml-1">{node.stats.avgTimeMs}ms</span>
                      </p>
                    </div>
                    <div
                      className="w-1.5 h-1.5 rounded-full flex-shrink-0"
                      style={{
                        background:
                          node.status === "active" ? "#00FFB2"
                            : node.status === "error" ? "#FF4466"
                            : "#8888a0",
                      }}
                    />
                  </button>
                );
              })}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
