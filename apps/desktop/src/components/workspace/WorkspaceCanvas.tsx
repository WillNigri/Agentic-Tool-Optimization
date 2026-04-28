import { useRef, useState, useCallback, useEffect } from "react";
import { ZoomIn, ZoomOut, Maximize2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useWorkspaceStore } from "@/stores/useWorkspaceStore";
import { getConnectionPoints, buildBezierPath, screenToCanvas } from "@/components/automation/helpers";
import WorkspaceNodeCard from "./WorkspaceNodeCard";
import WorkspaceMinimap from "./WorkspaceMinimap";

const NODE_H = 85;

type ZoomLevel = "bird" | "normal" | "focused";

function getZoomLevel(scale: number): ZoomLevel {
  if (scale < 0.55) return "bird";
  if (scale > 1.2) return "focused";
  return "normal";
}

export default function WorkspaceCanvas() {
  const canvasRef = useRef<HTMLDivElement>(null);
  const {
    nodes,
    edges,
    scale,
    panOffset,
    selectedNodeId,
    setScale,
    setPanOffset,
    selectNode,
    moveNode,
  } = useWorkspaceStore();

  const [isPanning, setIsPanning] = useState(false);
  const [panStart, setPanStart] = useState({ x: 0, y: 0 });
  const [draggingNodeId, setDraggingNodeId] = useState<string | null>(null);
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 });
  const [canvasRect, setCanvasRect] = useState<DOMRect | undefined>();

  // Track canvas rect for minimap
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const update = () => setCanvasRect(el.getBoundingClientRect());
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const zoomLevel = getZoomLevel(scale);
  const visibleNodes = nodes.filter((n) => !n.hidden);

  // Canvas auto-size
  const canvasW = Math.max(1600, ...visibleNodes.map((n) => n.x + (n.width ?? 200) + 100));
  const canvasH = Math.max(800, ...visibleNodes.map((n) => n.y + NODE_H + 100));

  // Zoom via scroll wheel
  const handleWheel = useCallback(
    (e: WheelEvent) => {
      e.preventDefault();
      const delta = e.deltaY > 0 ? -0.05 : 0.05;
      setScale(scale + delta);
    },
    [scale, setScale]
  );

  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    el.addEventListener("wheel", handleWheel, { passive: false });
    return () => el.removeEventListener("wheel", handleWheel);
  }, [handleWheel]);

  // Keyboard shortcuts
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if ((e.metaKey || e.ctrlKey) && e.key === "0") { e.preventDefault(); fitAll(); }
      if ((e.metaKey || e.ctrlKey) && e.key === "1") { e.preventDefault(); setScale(1); }
      if (e.key === "f" && selectedNodeId) {
        const node = visibleNodes.find((n) => n.id === selectedNodeId);
        if (node && canvasRect) {
          setPanOffset({
            x: canvasRect.width / 2 - node.x * scale - (node.width ?? 200) / 2 * scale,
            y: canvasRect.height / 2 - node.y * scale - NODE_H / 2 * scale,
          });
        }
      }
      if (e.key === "Tab") {
        e.preventDefault();
        const idx = visibleNodes.findIndex((n) => n.id === selectedNodeId);
        const next = visibleNodes[(idx + 1) % visibleNodes.length];
        if (next) selectNode(next.id);
      }
      if (e.key === "Escape") selectNode(null);
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [selectedNodeId, visibleNodes, scale, canvasRect, selectNode, setScale, setPanOffset]);

  // Pan
  function handleMouseDown(e: React.MouseEvent) {
    if (e.button === 1 || (e.button === 0 && e.shiftKey)) {
      setIsPanning(true);
      setPanStart({ x: e.clientX - panOffset.x, y: e.clientY - panOffset.y });
      e.preventDefault();
    }
  }

  function handleMouseMove(e: React.MouseEvent) {
    if (isPanning) {
      setPanOffset({ x: e.clientX - panStart.x, y: e.clientY - panStart.y });
    }
    if (draggingNodeId) {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const pos = screenToCanvas(e.clientX, e.clientY, rect, panOffset, scale);
      moveNode(draggingNodeId, pos.x - dragStart.x, pos.y - dragStart.y);
    }
  }

  function handleMouseUp() {
    setIsPanning(false);
    setDraggingNodeId(null);
  }

  function handleNodeDragStart(nodeId: string, e: React.MouseEvent) {
    const node = nodes.find((n) => n.id === nodeId);
    if (!node) return;
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const pos = screenToCanvas(e.clientX, e.clientY, rect, panOffset, scale);
    setDraggingNodeId(nodeId);
    setDragStart({ x: pos.x - node.x, y: pos.y - node.y });
    e.stopPropagation();
  }

  function fitAll() {
    if (visibleNodes.length === 0) return;
    const minX = Math.min(...visibleNodes.map((n) => n.x));
    const maxX = Math.max(...visibleNodes.map((n) => n.x + (n.width ?? 200)));
    const minY = Math.min(...visibleNodes.map((n) => n.y));
    const maxY = Math.max(...visibleNodes.map((n) => n.y + NODE_H));
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const contentW = maxX - minX + 100;
    const contentH = maxY - minY + 100;
    const newScale = Math.min(rect.width / contentW, rect.height / contentH, 1.5);
    setScale(newScale);
    setPanOffset({
      x: (rect.width - contentW * newScale) / 2 - minX * newScale + 50,
      y: (rect.height - contentH * newScale) / 2 - minY * newScale + 50,
    });
  }

  return (
    <div className="relative flex-1 overflow-hidden bg-[#0a0a0f]">
      {/* Toolbar */}
      <div className="absolute top-3 right-3 z-10 flex items-center gap-1 rounded-lg border border-cs-border/60 bg-cs-card/90 backdrop-blur px-1 py-0.5">
        <button onClick={() => setScale(scale - 0.1)} className="p-1.5 text-cs-muted hover:text-cs-text rounded"><ZoomOut size={14} /></button>
        <span className="text-[10px] text-cs-muted font-mono w-10 text-center">{Math.round(scale * 100)}%</span>
        <button onClick={() => setScale(scale + 0.1)} className="p-1.5 text-cs-muted hover:text-cs-text rounded"><ZoomIn size={14} /></button>
        <div className="w-px h-4 bg-cs-border/60 mx-0.5" />
        <button onClick={fitAll} className="p-1.5 text-cs-muted hover:text-cs-text rounded" title="Fit all"><Maximize2 size={14} /></button>
      </div>

      {/* Zoom level indicator */}
      <div className="absolute top-3 left-3 z-10 rounded-md bg-cs-card/80 border border-cs-border/40 px-2 py-1 text-[9px] text-cs-muted uppercase tracking-wider">
        {zoomLevel === "bird" && "Overview"}
        {zoomLevel === "normal" && "Workspace"}
        {zoomLevel === "focused" && "Detail"}
      </div>

      {/* Canvas */}
      <div
        ref={canvasRef}
        className={cn("w-full h-full", isPanning ? "cursor-grabbing" : "cursor-default")}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
        onClick={(e) => {
          if (e.target === e.currentTarget || (e.target as HTMLElement).closest("[data-canvas-bg]")) {
            selectNode(null);
          }
        }}
      >
        <div
          style={{
            transform: `translate(${panOffset.x}px, ${panOffset.y}px) scale(${scale})`,
            transformOrigin: "0 0",
            width: canvasW,
            height: canvasH,
            position: "relative",
          }}
        >
          {/* Grid background */}
          <svg className="absolute inset-0 pointer-events-none" width={canvasW} height={canvasH} data-canvas-bg>
            <defs>
              <pattern id="ws-grid" width="30" height="30" patternUnits="userSpaceOnUse">
                <path d="M 30 0 L 0 0 0 30" fill="none" stroke="rgba(255,255,255,0.03)" strokeWidth="1" />
              </pattern>
            </defs>
            <rect width="100%" height="100%" fill="url(#ws-grid)" />
          </svg>

          {/* Edges */}
          <svg className="absolute inset-0 pointer-events-none" width={canvasW} height={canvasH}>
            {edges.map((edge) => {
              const fromNode = visibleNodes.find((n) => n.id === edge.from);
              const toNode = visibleNodes.find((n) => n.id === edge.to);
              if (!fromNode || !toNode) return null;

              const fromRect = { x: fromNode.x, y: fromNode.y, width: fromNode.width ?? 200, height: NODE_H };
              const toRect = { x: toNode.x, y: toNode.y, width: toNode.width ?? 200, height: NODE_H };
              const pts = getConnectionPoints(fromRect, toRect);
              const path = buildBezierPath(pts.x1, pts.y1, pts.x2, pts.y2);

              const edgeColor = edge.kind === "uses-skill" ? "#00FFB2" : edge.kind === "connects-mcp" ? "#3b82f6" : "#a855f7";

              return (
                <g key={`${edge.from}-${edge.to}`}>
                  <path d={path} fill="none" stroke={edgeColor} strokeWidth={1.5} strokeOpacity={0.2} />
                  <path
                    d={path}
                    fill="none"
                    stroke={edgeColor}
                    strokeWidth={1.5}
                    strokeOpacity={edge.animated ? 0.6 : 0.35}
                    strokeDasharray={edge.animated ? "6 4" : undefined}
                    style={edge.animated ? { animation: "dash 1s linear infinite" } : undefined}
                  />
                </g>
              );
            })}
          </svg>

          {/* Nodes */}
          {visibleNodes.map((node) => (
            <div
              key={node.id}
              onMouseDown={(e) => handleNodeDragStart(node.id, e)}
            >
              <WorkspaceNodeCard
                node={node}
                isSelected={selectedNodeId === node.id}
                zoomLevel={zoomLevel}
                onSelect={() => selectNode(node.id)}
              />
            </div>
          ))}
        </div>
      </div>

      {/* Minimap */}
      <WorkspaceMinimap canvasRect={canvasRect} />

      <style>{`
        @keyframes dash {
          to { stroke-dashoffset: -10; }
        }
      `}</style>
    </div>
  );
}
