import { useRef, useState, useCallback, useEffect } from "react";
import { ZoomIn, ZoomOut, Maximize2, Cpu, Sparkles, Server, BookOpen, X, ExternalLink, ToggleLeft, ToggleRight, Eye, EyeOff, FolderOpen } from "lucide-react";
import { cn } from "@/lib/utils";
import { useQuery } from "@tanstack/react-query";
import { getProjectBundle, listProjects, toggleSkill } from "@/lib/api";
import { useProjectStore } from "@/stores/useProjectStore";
import { useProjectStore as useProjectNav } from "@/stores/useProjectStore";
import { lazy, Suspense } from "react";
import { Plus, Command } from "lucide-react";
import { installMarketplaceSkill } from "@/lib/api";
import SkillPalette from "./SkillPalette";
import CommandPalette from "./CommandPalette";
const FileViewer = lazy(() => import("@/components/FileViewer"));

interface CanvasNode {
  id: string;
  label: string;
  kind: "runtime" | "skill" | "mcp" | "memory";
  color: string;
  x: number;
  y: number;
  w: number;
  h: number;
  active: boolean;
  detail?: string;
  parentId?: string;
  filePath?: string;
  skillId?: string;
  runtime?: string;
  tokenCount?: number;
}

interface CanvasEdge {
  from: string;
  to: string;
  color: string;
}

const RUNTIME_COLORS: Record<string, string> = {
  claude: "#f97316", codex: "#22c55e", gemini: "#3b82f6", openclaw: "#06b6d4", hermes: "#a855f7",
};
const KIND_ICONS = { runtime: Cpu, skill: Sparkles, mcp: Server, memory: BookOpen };

export default function WorkspaceCanvas() {
  const canvasRef = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(0.85);
  const [pan, setPan] = useState({ x: 30, y: 30 });
  const [isPanning, setIsPanning] = useState(false);
  const [panStart, setPanStart] = useState({ x: 0, y: 0 });
  const [selected, setSelected] = useState<string | null>(null);
  const [openFile, setOpenFile] = useState<string | null>(null);
  const [activeRuntime, setActiveRuntime] = useState<string | null>(null);
  const [showPalette, setShowPalette] = useState(false);
  const [multiSelect, setMultiSelect] = useState<Set<string>>(new Set());
  const [showCommandPalette, setShowCommandPalette] = useState(false);
  const [dragging, setDragging] = useState<{ id: string; ox: number; oy: number } | null>(null);
  const [nodes, setNodes] = useState<CanvasNode[]>([]);
  const [edges, setEdges] = useState<CanvasEdge[]>([]);

  const activeProject = useProjectStore((s) => s.activeProject);
  const { data: projects = [] } = useQuery({ queryKey: ["projects"], queryFn: listProjects, staleTime: 30_000 });
  const projectPath = activeProject?.path ?? projects[0]?.path;

  const { data: bundle } = useQuery({
    queryKey: ["project-bundle", projectPath],
    queryFn: () => getProjectBundle(projectPath!),
    enabled: !!projectPath,
    staleTime: 10_000,
  });

  // Build nodes from bundle
  useEffect(() => {
    if (!bundle || nodes.length > 0) return;
    const n: CanvasNode[] = [];
    const e: CanvasEdge[] = [];
    let rx = 80;

    const runtimes = [
      { key: "claude", label: "Claude Code", has: bundle.hasClaude, skills: bundle.skills },
      { key: "codex", label: "Codex", has: bundle.hasCodex, skills: bundle.codexSkills },
      { key: "gemini", label: "Gemini CLI", has: bundle.hasGemini, skills: bundle.geminiSkills },
      { key: "openclaw", label: "OpenClaw", has: bundle.hasOpenclaw, skills: bundle.openclawSkills },
      { key: "hermes", label: "Hermes", has: bundle.hasHermes, skills: bundle.hermesSkills },
    ];

    for (const rt of runtimes) {
      if (!rt.has) continue;
      const rid = `rt-${rt.key}`;
      const color = RUNTIME_COLORS[rt.key];
      n.push({ id: rid, label: rt.label, kind: "runtime", color, x: rx, y: 60, w: 200, h: 70, active: true, detail: `${rt.skills.length} skills`, runtime: rt.key });

      let sx = rx - 20;
      for (const skill of rt.skills.slice(0, 8)) {
        const sid = `sk-${skill.id}`;
        n.push({ id: sid, label: skill.name, kind: "skill", color, x: sx, y: 200 + Math.random() * 30, w: 160, h: 50, active: skill.enabled, parentId: rid, filePath: skill.filePath, skillId: skill.id, runtime: rt.key, tokenCount: skill.tokenCount });
        e.push({ from: rid, to: sid, color });
        sx += 170;
      }
      rx += 300;
    }

    let mx = 80;
    for (const mcp of bundle.mcpServers) {
      n.push({ id: `mcp-${mcp.name}`, label: mcp.name, kind: "mcp", color: "#3b82f6", x: mx, y: 380, w: 160, h: 50, active: true, detail: mcp.kind });
      mx += 180;
    }

    setNodes(n);
    setEdges(e);
  }, [bundle]);

  // Zoom
  const handleWheel = useCallback((ev: WheelEvent) => {
    ev.preventDefault();
    setScale((s) => Math.max(0.2, Math.min(2.5, s + (ev.deltaY > 0 ? -0.05 : 0.05))));
  }, []);
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    el.addEventListener("wheel", handleWheel, { passive: false });
    return () => el.removeEventListener("wheel", handleWheel);
  }, [handleWheel]);

  // Listen for live agent activity
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen("log-entry", (event: any) => {
          const entry = event.payload;
          const runtime = entry?.runtime || (typeof entry === "string" && entry.includes("claude") ? "claude" : null);
          if (runtime) {
            setActiveRuntime(runtime);
            setTimeout(() => setActiveRuntime(null), 3000);
          }
        });
      } catch {}
    })();
    return () => unlisten?.();
  }, []);

  // ⌘K command palette
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") { e.preventDefault(); setShowCommandPalette((v) => !v); }
      if ((e.metaKey || e.ctrlKey) && e.key === "0") { e.preventDefault(); fitAll(); }
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [nodes]);

  // Pan + drag
  function onMouseDown(e: React.MouseEvent) {
    if (e.button === 0 && (e.shiftKey || e.target === canvasRef.current || (e.target as HTMLElement).closest("[data-bg]"))) {
      setIsPanning(true);
      setPanStart({ x: e.clientX - pan.x, y: e.clientY - pan.y });
    }
  }
  function onMouseMove(e: React.MouseEvent) {
    if (isPanning) setPan({ x: e.clientX - panStart.x, y: e.clientY - panStart.y });
    if (dragging) {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const cx = (e.clientX - rect.left - pan.x) / scale;
      const cy = (e.clientY - rect.top - pan.y) / scale;
      setNodes((prev) => prev.map((n) => n.id === dragging.id ? { ...n, x: cx - dragging.ox, y: cy - dragging.oy } : n));
    }
  }
  function onMouseUp() { setIsPanning(false); setDragging(null); }

  function startDrag(id: string, e: React.MouseEvent) {
    const node = nodes.find((n) => n.id === id);
    if (!node) return;
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const cx = (e.clientX - rect.left - pan.x) / scale;
    const cy = (e.clientY - rect.top - pan.y) / scale;
    setDragging({ id, ox: cx - node.x, oy: cy - node.y });
    setSelected(id);
    e.stopPropagation();
  }

  function fitAll() {
    if (nodes.length === 0) return;
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const minX = Math.min(...nodes.map((n) => n.x)) - 40;
    const minY = Math.min(...nodes.map((n) => n.y)) - 40;
    const maxX = Math.max(...nodes.map((n) => n.x + n.w)) + 40;
    const maxY = Math.max(...nodes.map((n) => n.y + n.h)) + 40;
    const ns = Math.min(rect.width / (maxX - minX), rect.height / (maxY - minY), 1.5);
    setScale(ns);
    setPan({ x: (rect.width - (maxX - minX) * ns) / 2 - minX * ns, y: (rect.height - (maxY - minY) * ns) / 2 - minY * ns });
  }

  const cw = Math.max(1600, ...nodes.map((n) => n.x + n.w + 80));
  const ch = Math.max(700, ...nodes.map((n) => n.y + n.h + 80));
  const zoomLevel = scale < 0.5 ? "bird" : scale > 1.2 ? "focused" : "normal";
  const [canvasRect, setCanvasRect] = useState<DOMRect | undefined>();

  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const update = () => setCanvasRect(el.getBoundingClientRect());
    update();
    const obs = new ResizeObserver(update);
    obs.observe(el);
    return () => obs.disconnect();
  }, []);

  return (
    <div className="flex-1 overflow-hidden bg-[#0a0a0f] relative" ref={canvasRef}
      onMouseDown={onMouseDown} onMouseMove={onMouseMove} onMouseUp={onMouseUp} onMouseLeave={onMouseUp}
      onClick={(e) => { if (e.target === e.currentTarget) setSelected(null); }}
    >
      {/* Toolbar */}
      <div className="absolute top-3 right-3 z-10 flex items-center gap-1 rounded-lg border border-cs-border/60 bg-cs-card/90 backdrop-blur px-1 py-0.5">
        <button onClick={() => setShowPalette((v) => !v)} className={cn("p-1.5 rounded transition-colors", showPalette ? "text-cs-accent bg-cs-accent/10" : "text-cs-muted hover:text-cs-text")} title="Add Skills"><Plus size={14} /></button>
        <div className="w-px h-4 bg-cs-border/60" />
        <button onClick={() => setScale((s) => Math.max(0.2, s - 0.1))} className="p-1.5 text-cs-muted hover:text-cs-text rounded"><ZoomOut size={14} /></button>
        <span className="text-[10px] text-cs-muted font-mono w-10 text-center">{Math.round(scale * 100)}%</span>
        <button onClick={() => setScale((s) => Math.min(2.5, s + 0.1))} className="p-1.5 text-cs-muted hover:text-cs-text rounded"><ZoomIn size={14} /></button>
        <div className="w-px h-4 bg-cs-border/60" />
        <button onClick={fitAll} className="p-1.5 text-cs-muted hover:text-cs-text rounded" title="Fit all"><Maximize2 size={14} /></button>
        <div className="w-px h-4 bg-cs-border/60" />
        <button onClick={() => setShowCommandPalette(true)} className="flex items-center gap-1 px-1.5 py-1 text-cs-muted hover:text-cs-text rounded text-[10px]" title="Command palette"><Command size={11} />K</button>
      </div>

      {/* Canvas */}
      <div style={{ transform: `translate(${pan.x}px, ${pan.y}px) scale(${scale})`, transformOrigin: "0 0", width: cw, height: ch, position: "relative" }}>
        {/* Grid */}
        <svg className="absolute inset-0 pointer-events-none" width={cw} height={ch} data-bg>
          <defs><pattern id="wg" width="30" height="30" patternUnits="userSpaceOnUse"><path d="M 30 0 L 0 0 0 30" fill="none" stroke="rgba(255,255,255,0.03)" strokeWidth="1" /></pattern></defs>
          <rect width="100%" height="100%" fill="url(#wg)" />
        </svg>

        {/* Edges */}
        <svg className="absolute inset-0 pointer-events-none" width={cw} height={ch}>
          {edges.map((edge) => {
            const from = nodes.find((n) => n.id === edge.from);
            const to = nodes.find((n) => n.id === edge.to);
            if (!from || !to) return null;
            const x1 = from.x + from.w / 2, y1 = from.y + from.h;
            const x2 = to.x + to.w / 2, y2 = to.y;
            const cy1 = y1 + (y2 - y1) * 0.4, cy2 = y2 - (y2 - y1) * 0.4;
            const pathD = `M ${x1} ${y1} C ${x1} ${cy1}, ${x2} ${cy2}, ${x2} ${y2}`;
            const edgeActive = activeRuntime && from.runtime === activeRuntime;
            const pathId = `edge-${edge.from}-${edge.to}`;
            return (
              <g key={pathId}>
                <path d={pathD} fill="none" stroke={edge.color} strokeWidth={edgeActive ? 2 : 1.5}
                  strokeOpacity={edgeActive ? 0.6 : 0.2}
                  strokeDasharray={edgeActive ? undefined : "6 4"}
                  style={!edgeActive ? { animation: "wdash 2s linear infinite" } : undefined}
                />
                {edgeActive && (
                  <>
                    <path id={pathId} d={pathD} fill="none" stroke="none" />
                    <circle r="3" fill={edge.color} opacity={0.8}>
                      <animateMotion dur="1.5s" repeatCount="indefinite">
                        <mpath href={`#${pathId}`} />
                      </animateMotion>
                    </circle>
                    <circle r="2" fill={edge.color} opacity={0.5}>
                      <animateMotion dur="1.5s" repeatCount="indefinite" begin="0.5s">
                        <mpath href={`#${pathId}`} />
                      </animateMotion>
                    </circle>
                  </>
                )}
              </g>
            );
          })}
        </svg>

        {/* Nodes */}
        {nodes.map((node) => {
          const Icon = KIND_ICONS[node.kind];
          const isSel = selected === node.id || multiSelect.has(node.id);
          const isActive = activeRuntime && node.runtime === activeRuntime;
          const isMulti = multiSelect.has(node.id);

          const handleClick = (e: React.MouseEvent) => {
            e.stopPropagation();
            if (e.metaKey || e.ctrlKey) {
              setMultiSelect((prev) => { const next = new Set(prev); next.has(node.id) ? next.delete(node.id) : next.add(node.id); return next; });
            } else { setMultiSelect(new Set()); setSelected(node.id); }
          };

          // Bird's eye — minimal circle
          if (zoomLevel === "bird") {
            return (
              <div key={node.id} className="absolute flex flex-col items-center gap-1 cursor-pointer"
                style={{ left: node.x, top: node.y, width: node.w, animation: "nodeAppear 0.3s ease-out" }}
                onMouseDown={(e) => startDrag(node.id, e)} onClick={handleClick}
              >
                <div className="w-8 h-8 rounded-full flex items-center justify-center border-2" style={{
                  backgroundColor: `${node.color}20`, borderColor: isSel ? node.color : `${node.color}50`,
                  boxShadow: isActive ? `0 0 12px ${node.color}50` : undefined,
                }}>
                  <div className={cn("w-2 h-2 rounded-full", node.active ? "bg-green-400" : "bg-gray-600")} />
                </div>
                <span className="text-[8px] text-cs-muted text-center truncate max-w-[60px]">{node.label}</span>
              </div>
            );
          }

          // Normal + Focused
          return (
            <div key={node.id}
              onMouseDown={(e) => startDrag(node.id, e)} onClick={handleClick}
              className={cn(
                "absolute rounded-lg border cursor-grab active:cursor-grabbing transition-all duration-300",
                isSel && "ring-1 ring-offset-1 ring-offset-transparent",
                isActive && "animate-pulse"
              )}
              style={{
                left: node.x, top: node.y, width: node.w, height: zoomLevel === "focused" && node.kind === "runtime" ? node.h + 30 : node.h,
                backgroundColor: "#161620",
                borderColor: isMulti ? "#3b82f6" : isSel ? node.color : isActive ? node.color : "#2a2a3a",
                boxShadow: isActive ? `0 0 24px ${node.color}50` : isSel ? `0 0 16px ${node.color}30` : node.active ? `0 0 6px ${node.color}10` : undefined,
                transition: "border-color 300ms, box-shadow 300ms, height 200ms",
                animation: "nodeAppear 0.3s ease-out",
              }}
            >
              <div className="h-[2px] rounded-t-lg" style={{ backgroundColor: node.color }} />
              <div className="px-2.5 py-1.5">
                <div className="flex items-center gap-1.5">
                  <Icon size={12} style={{ color: node.color }} />
                  <span className="text-xs font-medium truncate flex-1">{node.label}</span>
                  {isActive && <span className="text-[8px] text-amber-400 animate-pulse shrink-0">working...</span>}
                  <div className="relative shrink-0">
                    <div className={cn("w-1.5 h-1.5 rounded-full", node.active ? "bg-green-400" : "bg-gray-600")} />
                    {isActive && <div className="absolute inset-0 w-1.5 h-1.5 rounded-full bg-green-400 animate-ping" />}
                  </div>
                </div>
                {node.detail && <p className="text-[9px] text-cs-muted mt-0.5 truncate">{node.detail}</p>}
                {/* Focused mode extras */}
                {zoomLevel === "focused" && node.tokenCount && (
                  <div className="flex items-center gap-2 mt-1.5 pt-1 border-t border-white/5">
                    <span className="text-[8px] text-cs-muted font-mono">{node.tokenCount.toLocaleString()} tok</span>
                    {node.runtime && <span className="text-[8px] text-cs-muted capitalize">{node.runtime}</span>}
                  </div>
                )}
                {zoomLevel === "focused" && node.kind === "runtime" && (
                  <div className="flex items-center gap-1 mt-1 pt-1 border-t border-white/5">
                    <Sparkles size={8} className="text-cs-muted" />
                    <span className="text-[8px] text-cs-muted">{nodes.filter((n) => n.parentId === node.id).length} skills attached</span>
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>

      <style>{`
        @keyframes wdash { to { stroke-dashoffset: -10; } }
        @keyframes nodeAppear { from { transform: scale(0); opacity: 0; } to { transform: scale(1); opacity: 1; } }
        @keyframes scorePop { 0% { transform: scale(1); } 50% { transform: scale(1.3); } 100% { transform: scale(1); } }
      `}</style>

      {/* Minimap */}
      {nodes.length > 0 && (() => {
        const MW = 150, MH = 90;
        const minX = Math.min(...nodes.map((n) => n.x)) - 30;
        const minY = Math.min(...nodes.map((n) => n.y)) - 30;
        const maxX = Math.max(...nodes.map((n) => n.x + n.w)) + 30;
        const maxY = Math.max(...nodes.map((n) => n.y + n.h)) + 30;
        const contentW = maxX - minX, contentH = maxY - minY;
        const ms = Math.min(MW / contentW, MH / contentH);
        const vpW = canvasRect ? canvasRect.width / scale * ms : 30;
        const vpH = canvasRect ? canvasRect.height / scale * ms : 20;
        const vpX = (-pan.x / scale - minX) * ms;
        const vpY = (-pan.y / scale - minY) * ms;
        return (
          <div className="absolute bottom-3 right-3 z-10 rounded-lg border border-cs-border/60 bg-cs-card/90 backdrop-blur overflow-hidden cursor-crosshair"
            style={{ width: MW, height: MH }}
            onClick={(e) => {
              const rect = e.currentTarget.getBoundingClientRect();
              const cx = (e.clientX - rect.left) / ms + minX;
              const cy = (e.clientY - rect.top) / ms + minY;
              if (canvasRect) setPan({ x: -(cx - canvasRect.width / scale / 2) * scale, y: -(cy - canvasRect.height / scale / 2) * scale });
            }}
          >
            <svg width={MW} height={MH}>
              {nodes.map((n) => (
                <rect key={n.id} x={(n.x - minX) * ms} y={(n.y - minY) * ms}
                  width={Math.max(3, n.w * ms)} height={Math.max(2, n.h * ms)}
                  rx={1} fill={n.color} fillOpacity={n.active ? 0.7 : 0.2} />
              ))}
              <rect x={vpX} y={vpY} width={vpW} height={vpH}
                fill="none" stroke="#00FFB2" strokeWidth={1} strokeOpacity={0.5} rx={1} />
            </svg>
          </div>
        );
      })()}

      {/* Zoom level indicator */}
      <div className="absolute bottom-3 left-3 z-10 rounded-md bg-cs-card/80 border border-cs-border/40 px-2 py-1 text-[9px] text-cs-muted uppercase tracking-wider">
        {zoomLevel === "bird" && "Overview"}
        {zoomLevel === "normal" && "Workspace"}
        {zoomLevel === "focused" && "Detail"}
      </div>

      {/* Skill Palette */}
      {showPalette && (
        <SkillPalette
          existingSkillNames={nodes.filter((n) => n.kind === "skill").map((n) => n.label)}
          onClose={() => setShowPalette(false)}
          onInstall={(skill, runtime) => {
            const rt = nodes.find((n) => n.kind === "runtime" && n.runtime === runtime);
            if (!rt) return;
            const newId = `sk-new-${Date.now()}`;
            const childCount = nodes.filter((n) => n.parentId === rt.id).length;
            setNodes((prev) => [...prev, {
              id: newId, label: skill.name, kind: "skill" as const, color: rt.color,
              x: rt.x - 20 + childCount * 170, y: 200 + Math.random() * 30,
              w: 160, h: 50, active: true, parentId: rt.id, runtime,
              detail: skill.description,
            }]);
            setEdges((prev) => [...prev, { from: rt.id, to: newId, color: rt.color }]);
            installMarketplaceSkill(skill as any).catch(() => {});
          }}
        />
      )}

      {/* Detail Panel */}
      {selected && (() => {
        const node = nodes.find((n) => n.id === selected);
        if (!node) return null;
        const Icon = KIND_ICONS[node.kind];
        const childSkills = nodes.filter((n) => n.parentId === node.id);
        return (
          <div className="absolute right-0 top-0 bottom-0 w-72 bg-cs-card border-l border-cs-border z-20 flex flex-col shadow-2xl">
            <div className="flex items-start justify-between p-3 border-b border-cs-border">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <Icon size={14} style={{ color: node.color }} />
                  <h3 className="text-sm font-semibold truncate">{node.label}</h3>
                </div>
                <div className="flex items-center gap-2 mt-1">
                  <div className={cn("w-2 h-2 rounded-full", node.active ? "bg-green-400" : "bg-gray-500")} />
                  <span className="text-[10px] text-cs-muted capitalize">{node.kind}</span>
                  {node.runtime && <span className="text-[10px] text-cs-muted">· {node.runtime}</span>}
                </div>
              </div>
              <button onClick={() => setSelected(null)} className="p-1 rounded hover:bg-cs-border text-cs-muted"><X size={14} /></button>
            </div>

            <div className="flex-1 overflow-y-auto p-3 space-y-3">
              {node.detail && <p className="text-[11px] text-cs-muted">{node.detail}</p>}

              {node.tokenCount && (
                <div className="rounded-md border border-cs-border/60 bg-cs-bg/40 px-3 py-2">
                  <p className="text-[9px] text-cs-muted uppercase">Tokens</p>
                  <p className="text-sm font-mono">{node.tokenCount.toLocaleString()}</p>
                </div>
              )}

              {/* Child skills for runtime nodes */}
              {childSkills.length > 0 && (
                <div>
                  <p className="text-[9px] text-cs-muted uppercase mb-1.5">Skills ({childSkills.length})</p>
                  <div className="space-y-1">
                    {childSkills.map((s) => (
                      <div key={s.id} className="flex items-center gap-2 rounded border border-cs-border/60 bg-cs-bg/40 px-2 py-1">
                        <Sparkles size={10} style={{ color: s.color }} />
                        <span className="text-[11px] truncate flex-1">{s.label}</span>
                        <button
                          onClick={() => {
                            if (s.skillId) {
                              toggleSkill(s.skillId, !s.active).then(() => {
                                setNodes((prev) => prev.map((n) => n.id === s.id ? { ...n, active: !n.active } : n));
                              });
                            }
                          }}
                          className="shrink-0"
                          title={s.active ? "Disable" : "Enable"}
                        >
                          {s.active ? <ToggleRight size={14} className="text-green-400" /> : <ToggleLeft size={14} className="text-gray-500" />}
                        </button>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>

            {/* Actions */}
            <div className="p-3 border-t border-cs-border space-y-1.5">
              {node.filePath && (
                <button
                  onClick={() => setOpenFile(node.filePath!)}
                  className="flex w-full items-center gap-2 rounded-md border border-cs-border px-2.5 py-1.5 text-xs text-cs-muted hover:bg-cs-border/30 hover:text-cs-text"
                >
                  <ExternalLink size={12} /> Open in editor
                </button>
              )}
              {node.kind === "runtime" && (
                <button
                  onClick={() => {
                    // Navigate to Projects tab with this project
                    const setSection = (window as any).__atoNavigate;
                    if (setSection) setSection("projects");
                  }}
                  className="flex w-full items-center gap-2 rounded-md border border-cs-border px-2.5 py-1.5 text-xs text-cs-muted hover:bg-cs-border/30 hover:text-cs-text"
                >
                  <FolderOpen size={12} /> View in Projects
                </button>
              )}
              <button
                onClick={() => {
                  setNodes((prev) => prev.map((n) => n.id === node.id ? { ...n, active: !n.active } : n));
                }}
                className="flex w-full items-center gap-2 rounded-md border border-cs-border px-2.5 py-1.5 text-xs text-cs-muted hover:bg-cs-border/30 hover:text-cs-text"
              >
                {node.active ? <><EyeOff size={12} /> Hide from workspace</> : <><Eye size={12} /> Show in workspace</>}
              </button>
            </div>
          </div>
        );
      })()}

      {/* Command Palette */}
      {showCommandPalette && (
        <CommandPalette
          items={[
            ...nodes.map((n) => ({
              id: n.id, label: n.label, group: "canvas" as const,
              icon: KIND_ICONS[n.kind], color: n.color,
              onSelect: () => { setSelected(n.id); },
            })),
            { id: "act-fit", label: "Fit all nodes to screen", group: "action" as const, icon: Maximize2, onSelect: fitAll },
            { id: "act-palette", label: "Open skill palette", group: "action" as const, icon: Plus, onSelect: () => setShowPalette(true) },
            { id: "act-deselect", label: "Clear selection", group: "action" as const, icon: X, onSelect: () => { setSelected(null); setMultiSelect(new Set()); } },
          ]}
          onClose={() => setShowCommandPalette(false)}
        />
      )}

      {/* File viewer */}
      {openFile && (
        <Suspense fallback={null}>
          <FileViewer filePath={openFile} onClose={() => setOpenFile(null)} />
        </Suspense>
      )}
    </div>
  );
}
