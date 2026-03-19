import { cn } from "@/lib/utils";
import { Hash, AlertTriangle, Clock, Globe, Activity } from "lucide-react";
import { NODE_W, NODE_H, PORT_SIZE, TYPE_COLORS, SERVICE_COLORS, SERVICE_ICONS, NODE_ICONS } from "./constants";
import type { FlowNode, BuilderMode, ExecutionNodeStatus } from "./types";
import { useAutomationStore } from "@/stores/useAutomationStore";

interface FlowNodeCardProps {
  node: FlowNode;
  isSelected: boolean;
  mode: BuilderMode;
  execStatus?: ExecutionNodeStatus;
  onClick: () => void;
  onPointerDown?: (e: React.PointerEvent) => void;
  onOutputPortClick?: () => void;
  onInputPortClick?: () => void;
}

const EXEC_RING_COLORS: Record<ExecutionNodeStatus, string> = {
  pending: "#8888a0",
  running: "#FFB800",
  completed: "#00FFB2",
  failed: "#FF4466",
};

export default function FlowNodeCard({
  node,
  isSelected,
  mode,
  execStatus,
  onClick,
  onPointerDown,
  onOutputPortClick,
  onInputPortClick,
}: FlowNodeCardProps) {
  const connecting = useAutomationStore((s) => s.connecting);
  const isService = node.type === "service" && node.service;
  const barColor = isService
    ? SERVICE_COLORS[node.service!] || TYPE_COLORS.service
    : TYPE_COLORS[node.type];
  const IconComponent = isService
    ? SERVICE_ICONS[node.service!] || Globe
    : NODE_ICONS[node.type] || Activity;

  const execRing = execStatus ? EXEC_RING_COLORS[execStatus] : null;

  return (
    <div
      onClick={onClick}
      onPointerDown={onPointerDown}
      className={cn(
        "absolute select-none rounded-lg transition-all duration-200",
        mode === "edit" ? "cursor-move" : "cursor-pointer",
        "hover:brightness-110"
      )}
      style={{
        left: node.x,
        top: node.y,
        width: node.width || NODE_W,
        height: NODE_H,
        zIndex: isSelected ? 20 : 10,
      }}
    >
      <div
        className={cn(
          "relative h-full w-full rounded-lg border overflow-hidden",
          isSelected ? "ring-2" : "",
          node.status === "error" ? "border-[#FF4466]" : "border-[#2a2a3a]"
        )}
        style={{
          background: "#16161e",
          ...(isSelected ? { borderColor: barColor } : {}),
          ...(node.status === "active"
            ? { boxShadow: `0 0 12px 2px ${barColor}33` }
            : {}),
          ...(isSelected ? { boxShadow: `0 0 16px 3px ${barColor}44` } : {}),
          ...(execRing ? { boxShadow: `0 0 16px 3px ${execRing}55`, borderColor: execRing } : {}),
        }}
      >
        {/* Top color bar */}
        <div className="w-full" style={{ height: 3, background: execRing || barColor }} />

        {/* Content */}
        <div className="px-2.5 pt-1.5 pb-1">
          <div className="flex items-center gap-1.5 mb-0.5">
            <IconComponent size={13} style={{ color: barColor, flexShrink: 0 }} />
            <span
              className="font-semibold text-[#e8e8f0] truncate"
              style={{ fontSize: 13, lineHeight: "18px" }}
            >
              {node.label}
            </span>
            {isService && (
              <span
                className="ml-auto rounded px-1 py-0 text-[8px] font-bold uppercase tracking-wider shrink-0"
                style={{ color: barColor, background: `${barColor}18` }}
              >
                {node.service}
              </span>
            )}
          </div>
          <p
            className="text-[#8888a0] truncate"
            style={{ fontSize: 11, lineHeight: "14px" }}
          >
            {node.description}
          </p>
          {/* WHO / WHAT / HOW shown as inline tag when present */}
          {node.agentName && (
            <p className="text-[#06b6d4] font-medium truncate mt-0.5" style={{ fontSize: 10, lineHeight: "13px" }}>
              {node.agentName}
            </p>
          )}
        </div>

        {/* Stats row */}
        <div
          className="absolute bottom-0 left-0 right-0 flex items-center gap-2 px-2.5 py-1 border-t border-[#2a2a3a]"
          style={{ fontSize: 10 }}
        >
          <span className="flex items-center gap-0.5 text-[#8888a0]">
            <Hash size={9} />
            {node.stats.executions}
          </span>
          <span
            className={cn(
              "flex items-center gap-0.5",
              node.stats.errors > 0 ? "text-[#FF4466]" : "text-[#8888a0]"
            )}
          >
            <AlertTriangle size={9} />
            {node.stats.errors}
          </span>
          <span className="flex items-center gap-0.5 text-[#8888a0] ml-auto">
            <Clock size={9} />
            {node.stats.avgTimeMs}ms
          </span>
        </div>
      </div>

      {/* Connection ports (edit mode only) */}
      {mode === "edit" && (
        <>
          {/* Input port (left) */}
          <div
            className="absolute rounded-full border-2 border-[#2a2a3a] hover:border-[#00FFB2] transition-colors cursor-crosshair"
            style={{
              width: PORT_SIZE,
              height: PORT_SIZE,
              left: -PORT_SIZE / 2,
              top: NODE_H / 2 - PORT_SIZE / 2,
              background: connecting ? "#00FFB2" : "#16161e",
              zIndex: 30,
            }}
            onClick={(e) => {
              e.stopPropagation();
              onInputPortClick?.();
            }}
          />
          {/* Output port (right) */}
          <div
            className="absolute rounded-full border-2 border-[#2a2a3a] hover:border-[#00FFB2] transition-colors cursor-crosshair"
            style={{
              width: PORT_SIZE,
              height: PORT_SIZE,
              right: -PORT_SIZE / 2,
              top: NODE_H / 2 - PORT_SIZE / 2,
              background: "#16161e",
              zIndex: 30,
            }}
            onClick={(e) => {
              e.stopPropagation();
              onOutputPortClick?.();
            }}
          />
        </>
      )}
    </div>
  );
}
