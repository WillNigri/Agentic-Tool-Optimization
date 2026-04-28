import { useMemo } from "react";
import { useWorkspaceStore } from "@/stores/useWorkspaceStore";

const MINIMAP_W = 160;
const MINIMAP_H = 100;
const NODE_H = 85;

const RUNTIME_COLORS: Record<string, string> = {
  claude: "#f97316",
  codex: "#22c55e",
  gemini: "#3b82f6",
  openclaw: "#06b6d4",
  hermes: "#a855f7",
};

const KIND_COLORS: Record<string, string> = {
  runtime: "#00FFB2",
  skill: "#a78bfa",
  mcp: "#3b82f6",
  process: "#f59e0b",
  memory: "#FFB800",
};

export default function WorkspaceMinimap({ canvasRect }: { canvasRect?: DOMRect }) {
  const nodes = useWorkspaceStore((s) => s.nodes.filter((n) => !n.hidden));
  const scale = useWorkspaceStore((s) => s.scale);
  const panOffset = useWorkspaceStore((s) => s.panOffset);
  const setPanOffset = useWorkspaceStore((s) => s.setPanOffset);

  const bounds = useMemo(() => {
    if (nodes.length === 0) return { minX: 0, minY: 0, maxX: 1000, maxY: 600 };
    const minX = Math.min(...nodes.map((n) => n.x)) - 50;
    const minY = Math.min(...nodes.map((n) => n.y)) - 50;
    const maxX = Math.max(...nodes.map((n) => n.x + (n.width ?? 200))) + 50;
    const maxY = Math.max(...nodes.map((n) => n.y + NODE_H)) + 50;
    return { minX, minY, maxX, maxY };
  }, [nodes]);

  const contentW = bounds.maxX - bounds.minX;
  const contentH = bounds.maxY - bounds.minY;
  const scaleX = MINIMAP_W / contentW;
  const scaleY = MINIMAP_H / contentH;
  const mapScale = Math.min(scaleX, scaleY);

  // Viewport rectangle
  const vpW = canvasRect ? canvasRect.width / scale * mapScale : 40;
  const vpH = canvasRect ? canvasRect.height / scale * mapScale : 25;
  const vpX = (-panOffset.x / scale - bounds.minX) * mapScale;
  const vpY = (-panOffset.y / scale - bounds.minY) * mapScale;

  function handleClick(e: React.MouseEvent) {
    const rect = e.currentTarget.getBoundingClientRect();
    const clickX = (e.clientX - rect.left) / mapScale + bounds.minX;
    const clickY = (e.clientY - rect.top) / mapScale + bounds.minY;
    if (canvasRect) {
      setPanOffset({
        x: -(clickX - canvasRect.width / scale / 2) * scale,
        y: -(clickY - canvasRect.height / scale / 2) * scale,
      });
    }
  }

  if (nodes.length === 0) return null;

  return (
    <div
      className="absolute bottom-3 right-3 z-10 rounded-lg border border-cs-border/60 bg-cs-card/90 backdrop-blur overflow-hidden cursor-crosshair"
      style={{ width: MINIMAP_W, height: MINIMAP_H }}
      onClick={handleClick}
    >
      <svg width={MINIMAP_W} height={MINIMAP_H}>
        {/* Node dots */}
        {nodes.map((n) => {
          const x = (n.x - bounds.minX) * mapScale;
          const y = (n.y - bounds.minY) * mapScale;
          const color = n.runtime ? (RUNTIME_COLORS[n.runtime] ?? KIND_COLORS[n.kind]) : KIND_COLORS[n.kind] ?? "#666";
          return (
            <rect
              key={n.id}
              x={x}
              y={y}
              width={Math.max(4, (n.width ?? 200) * mapScale)}
              height={Math.max(3, NODE_H * mapScale)}
              rx={1}
              fill={color}
              fillOpacity={n.status === "online" ? 0.8 : 0.3}
            />
          );
        })}

        {/* Viewport rectangle */}
        <rect
          x={vpX}
          y={vpY}
          width={vpW}
          height={vpH}
          fill="none"
          stroke="#00FFB2"
          strokeWidth={1}
          strokeOpacity={0.6}
          rx={1}
        />
      </svg>
    </div>
  );
}
