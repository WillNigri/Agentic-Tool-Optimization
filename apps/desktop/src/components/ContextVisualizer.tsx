import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  Cell,
  PieChart,
  Pie,
} from "recharts";
import { useTranslation } from "react-i18next";
import { getContextBreakdown, getContextForRuntime, getLiveSessionData, getLiveContextBreakdown } from "@/lib/api";
import { formatNumber, cn } from "@/lib/utils";
import { ChevronRight, AlertTriangle, Shield, FolderTree, FileText, BarChart3, ExternalLink, Terminal, Cpu, Server, Globe, Zap, Clock, FileCode, MessageSquare, Database } from "lucide-react";
import FileViewer from "./FileViewer";
import type { AgentRuntime } from "@/components/cron/types";

const RUNTIME_TABS: { id: AgentRuntime; label: string; icon: typeof Terminal; color: string }[] = [
  { id: "claude", label: "Claude", icon: Terminal, color: "#f97316" },
  { id: "codex", label: "Codex", icon: Cpu, color: "#22c55e" },
  { id: "openclaw", label: "OpenClaw", icon: Server, color: "#06b6d4" },
  { id: "hermes", label: "Hermes", icon: Globe, color: "#a855f7" },
];

// Runtime-specific dependency examples for the detail view
type DepType = "system" | "config" | "skill" | "mcp";
const RUNTIME_DEPENDENCIES: Record<AgentRuntime, { name: string; path: string; tokens: number; type: DepType; loaded: boolean }[]> = {
  claude: [
    { name: "CLAUDE.md", path: "CLAUDE.md", tokens: 2100, type: "config", loaded: true },
    { name: "System Prompt", path: "(built-in)", tokens: 28450, type: "system", loaded: true },
    { name: "~/.claude/skills/", path: "~/.claude/skills/", tokens: 5430, type: "skill", loaded: true },
    { name: ".claude/skills/", path: ".claude/skills/", tokens: 4400, type: "skill", loaded: true },
    { name: "MCP Schemas", path: "~/.claude/settings.json", tokens: 5000, type: "mcp", loaded: true },
  ],
  codex: [
    { name: "AGENTS.md", path: "AGENTS.md", tokens: 1800, type: "config", loaded: true },
    { name: "System Prompt", path: "(built-in)", tokens: 20000, type: "system", loaded: true },
    { name: "~/.codex/skills/", path: "~/.codex/skills/", tokens: 3200, type: "skill", loaded: true },
    { name: "config.toml", path: "~/.codex/config.toml", tokens: 400, type: "config", loaded: true },
  ],
  openclaw: [
    { name: "AGENTS.md", path: "~/.openclaw/workspace/AGENTS.md", tokens: 3500, type: "config", loaded: true },
    { name: "SOUL.md", path: "~/.openclaw/workspace/SOUL.md", tokens: 1200, type: "config", loaded: true },
    { name: "TOOLS.md", path: "~/.openclaw/workspace/TOOLS.md", tokens: 800, type: "config", loaded: true },
    { name: "System Prompt", path: "(built-in)", tokens: 15000, type: "system", loaded: true },
    { name: "Skills", path: "~/.openclaw/skills/", tokens: 4000, type: "skill", loaded: true },
    { name: "Memory", path: "~/.openclaw/workspace/memory/", tokens: 2000, type: "mcp", loaded: true },
  ],
  hermes: [
    { name: "SOUL.md", path: "~/.hermes/SOUL.md", tokens: 1500, type: "config", loaded: true },
    { name: "System Prompt", path: "(built-in)", tokens: 12000, type: "system", loaded: true },
    { name: "Skills", path: "~/.hermes/skills/", tokens: 3800, type: "skill", loaded: true },
    { name: "MEMORY.md", path: "~/.hermes/memories/MEMORY.md", tokens: 800, type: "mcp", loaded: true },
    { name: "USER.md", path: "~/.hermes/memories/USER.md", tokens: 500, type: "mcp", loaded: true },
    { name: "config.yaml", path: "~/.hermes/config.yaml", tokens: 300, type: "config", loaded: true },
  ],
};

// Permissions vary by runtime — only Claude has the full tool permission system
const RUNTIME_PERMISSIONS: Record<AgentRuntime, { tool: string; scope: string; status: "allowed" | "ask" | "denied" }[]> = {
  claude: [
    { tool: "Read", scope: "global", status: "allowed" },
    { tool: "Write", scope: "project", status: "allowed" },
    { tool: "Edit", scope: "project", status: "allowed" },
    { tool: "Bash", scope: "project", status: "ask" },
    { tool: "Grep", scope: "global", status: "allowed" },
    { tool: "Glob", scope: "global", status: "allowed" },
    { tool: "WebFetch", scope: "global", status: "denied" },
    { tool: "Agent", scope: "project", status: "allowed" },
  ],
  codex: [
    { tool: "shell", scope: "sandbox", status: "allowed" },
    { tool: "file_read", scope: "sandbox", status: "allowed" },
    { tool: "file_write", scope: "sandbox", status: "allowed" },
    { tool: "browser", scope: "sandbox", status: "denied" },
  ],
  openclaw: [],
  hermes: [],
};

const DEP_TYPE_COLORS = {
  system: "#FF4466",
  config: "#FFB800",
  skill: "#00FFB2",
  mcp: "#3b82f6",
};

type DetailView = "chart" | "dependencies" | "permissions" | "live";

export default function ContextVisualizer() {
  const { t } = useTranslation();
  const [detailView, setDetailView] = useState<DetailView>("chart");
  const [viewingFile, setViewingFile] = useState<string | null>(null);
  const [activeRuntime, setActiveRuntime] = useState<AgentRuntime>("claude");

  const { data, isLoading } = useQuery({
    queryKey: ["context-breakdown", activeRuntime],
    queryFn: () => getContextForRuntime(activeRuntime),
  });

  // Live session data (only for Claude)
  const { data: liveSession, isLoading: liveLoading } = useQuery({
    queryKey: ["live-session"],
    queryFn: () => getLiveSessionData(),
    refetchInterval: 5000, // Refresh every 5 seconds
    enabled: activeRuntime === "claude",
  });

  // Live context breakdown (uses actual session data)
  const { data: liveContext } = useQuery({
    queryKey: ["live-context-breakdown"],
    queryFn: () => getLiveContextBreakdown(),
    refetchInterval: 10000, // Refresh every 10 seconds
    enabled: activeRuntime === "claude",
  });

  if (isLoading) {
    return <LoadingSkeleton />;
  }

  if (!data) {
    return (
      <div className="text-cs-muted text-sm">
        No context data available. Start a session to see context breakdown.
      </div>
    );
  }

  // Runtime not installed — limit=0 signals "not connected"
  const runtimeNotConnected = data.limit === 0;

  const usagePercent = data.limit > 0 ? (data.totalTokens / data.limit) * 100 : 0;
  const barColor =
    usagePercent >= 90
      ? "text-cs-danger"
      : usagePercent >= 75
        ? "text-cs-warning"
        : "text-cs-accent";

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-semibold mb-1">{t('context.title')}</h2>
        <p className="text-cs-muted text-sm">
          {t('context.subtitle')}
        </p>
      </div>

      {/* Runtime tabs */}
      <div className="flex items-center gap-1.5">
        {RUNTIME_TABS.map((rt) => {
          const Icon = rt.icon;
          return (
            <button
              key={rt.id}
              onClick={() => setActiveRuntime(rt.id)}
              className={cn(
                "flex items-center gap-1.5 px-3 py-2 text-xs font-medium rounded-lg border transition-colors",
                activeRuntime === rt.id
                  ? "text-white"
                  : "border-cs-border text-cs-muted hover:text-cs-text"
              )}
              style={
                activeRuntime === rt.id
                  ? { borderColor: `${rt.color}66`, background: `${rt.color}20`, color: rt.color }
                  : undefined
              }
            >
              <Icon size={14} />
              {rt.label}
            </button>
          );
        })}
      </div>

      {/* Not connected state */}
      {runtimeNotConnected && (
        <div className="card text-center py-10">
          <div className="w-14 h-14 rounded-full bg-cs-border/20 flex items-center justify-center mx-auto mb-4">
            {(() => {
              const rt = RUNTIME_TABS.find((r) => r.id === activeRuntime);
              const Icon = rt?.icon || Terminal;
              return <Icon size={24} className="text-cs-muted/40" />;
            })()}
          </div>
          <p className="text-sm font-medium text-cs-muted mb-1">
            {RUNTIME_TABS.find((r) => r.id === activeRuntime)?.label} is not connected
          </p>
          <p className="text-xs text-cs-muted/60 max-w-sm mx-auto">
            Install the CLI or configure it in the Setup Wizard to see context usage for this runtime.
          </p>
        </div>
      )}

      {/* Overall progress — only show when connected */}
      {!runtimeNotConnected && <><div className="card">
        <div className="flex items-end justify-between mb-2">
          <span className="text-sm text-cs-muted">{t('context.totalUsed')}</span>
          <span className={cn("text-lg font-semibold", barColor)}>
            {formatNumber(data.totalTokens)}{" "}
            <span className="text-sm text-cs-muted font-normal">
              / {formatNumber(data.limit)}
            </span>
          </span>
        </div>
        <div className="w-full h-3 bg-cs-bg rounded-full overflow-hidden">
          <div
            className={cn(
              "h-full rounded-full transition-all duration-500",
              usagePercent >= 90
                ? "bg-cs-danger"
                : usagePercent >= 75
                  ? "bg-cs-warning"
                  : "bg-cs-accent"
            )}
            style={{ width: `${Math.min(usagePercent, 100)}%` }}
          />
        </div>
        <p className="text-xs text-cs-muted mt-1">
          {t('context.percentage', { percentage: usagePercent.toFixed(1) })}
        </p>

        {/* Warning */}
        {usagePercent >= 75 && (
          <div className={cn(
            "mt-3 flex items-center gap-2 px-3 py-2 rounded-lg text-xs",
            usagePercent >= 90
              ? "bg-red-500/10 text-red-400 border border-red-500/20"
              : "bg-yellow-500/10 text-yellow-400 border border-yellow-500/20"
          )}>
            <AlertTriangle size={14} />
            {usagePercent >= 90
              ? t('context.warnings.critical')
              : t('context.warnings.high')}
          </div>
        )}
      </div>

      {/* View switcher tabs */}
      <div className="flex gap-1 p-1 bg-cs-bg rounded-lg border border-cs-border">
        {([
          { id: "chart" as const, label: t('context.views.breakdown'), icon: BarChart3, claudeOnly: false },
          { id: "live" as const, label: "Live Session", icon: Zap, claudeOnly: true },
          { id: "dependencies" as const, label: t('context.views.dependencies'), icon: FolderTree, claudeOnly: false },
          { id: "permissions" as const, label: t('context.views.permissions'), icon: Shield, claudeOnly: false },
        ] as const)
          .filter(item => !item.claudeOnly || activeRuntime === "claude")
          .map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => setDetailView(id)}
            className={cn(
              "flex-1 flex items-center justify-center gap-2 px-3 py-2 rounded-md text-xs font-medium transition-colors",
              detailView === id
                ? "bg-cs-card text-cs-accent"
                : "text-cs-muted hover:text-cs-text"
            )}
          >
            <Icon size={14} />
            {label}
            {id === "live" && liveSession?.isActive && (
              <span className="w-2 h-2 rounded-full bg-green-400 animate-pulse" />
            )}
          </button>
        ))}
      </div>

      {/* Chart View */}
      {detailView === "chart" && (
        <>
          <div className="card">
            <h3 className="text-sm font-medium text-cs-muted mb-4">
              {t('context.subtitle')}
            </h3>
            <div className="h-64">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart
                  data={data.categories}
                  layout="vertical"
                  margin={{ left: 20, right: 20, top: 0, bottom: 0 }}
                >
                  <XAxis
                    type="number"
                    tick={{ fill: "#8888a0", fontSize: 12 }}
                    tickFormatter={formatNumber}
                  />
                  <YAxis
                    type="category"
                    dataKey="name"
                    width={120}
                    tick={{ fill: "#8888a0", fontSize: 12 }}
                  />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: "#16161e",
                      border: "1px solid #2a2a3a",
                      borderRadius: 6,
                      fontSize: 13,
                    }}
                    labelStyle={{ color: "#e8e8f0" }}
                    formatter={(value: number) => [
                      t('context.tokens', { count: formatNumber(value) }),
                      "",
                    ]}
                  />
                  <Bar dataKey="tokens" radius={[0, 4, 4, 0]}>
                    {data.categories.map((cat, i) => (
                      <Cell key={i} fill={cat.color} />
                    ))}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>

          {/* Category legend cards */}
          <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
            {data.categories.map((cat) => {
              // Determine if this category is a viewable file
              const isFile = cat.name.endsWith(".md") || cat.name.endsWith(".yaml") || cat.name.endsWith(".json") || cat.name.endsWith(".toml");
              const filePathMap: Record<string, Record<string, string>> = {
                claude: { "CLAUDE.md": "CLAUDE.md", "System Prompts": "", "MCP Schemas": "", "Conversation": "" },
                openclaw: { "SOUL.md": "~/.openclaw/workspace/SOUL.md", "AGENTS.md": "~/.openclaw/workspace/AGENTS.md", "TOOLS.md": "~/.openclaw/workspace/TOOLS.md" },
                hermes: { "SOUL.md": "~/.hermes/SOUL.md", "config.yaml": "~/.hermes/config.yaml" },
              };
              const filePath = filePathMap[activeRuntime]?.[cat.name];
              const isClickable = isFile && cat.tokens > 0;

              return (
                <div
                  key={cat.name}
                  onClick={() => {
                    if (isClickable && filePath) {
                      // For remote files, use the context file reader
                      const resolved = filePath.replace("~", process.env.HOME || "/root");
                      setViewingFile(resolved);
                    }
                  }}
                  className={cn(
                    "card flex items-center gap-3",
                    isClickable && "cursor-pointer hover:border-cs-accent/30 transition-colors"
                  )}
                >
                  <div
                    className="w-3 h-3 rounded-full shrink-0"
                    style={{ backgroundColor: cat.color }}
                  />
                  <div className="min-w-0 flex-1">
                    <p className="text-sm truncate">{cat.name}</p>
                    <p className="text-xs text-cs-muted">
                      {t('context.tokens', { count: formatNumber(cat.tokens) })}
                    </p>
                  </div>
                  {isClickable && <ExternalLink size={12} className="text-cs-muted/40 shrink-0" />}
                </div>
              );
            })}
          </div>
        </>
      )}

      {/* Dependencies View */}
      {detailView === "dependencies" && (
        <div className="space-y-3">
          <div className="card !p-3">
            <p className="text-xs text-cs-muted mb-2">
              {t('context.depInfo')}
            </p>
            <div className="flex gap-3">
              {Object.entries(DEP_TYPE_COLORS).map(([type, color]) => (
                <div key={type} className="flex items-center gap-1.5">
                  <div className="w-2.5 h-2.5 rounded-full" style={{ backgroundColor: color }} />
                  <span className="text-xs text-cs-muted capitalize">{type}</span>
                </div>
              ))}
            </div>
          </div>

          {RUNTIME_DEPENDENCIES[activeRuntime].map((dep) => {
            const isClickable = dep.path !== "(built-in)" && !dep.path.startsWith("npx ");
            return (
              <div
                key={dep.name}
                onClick={() => isClickable && setViewingFile(dep.path)}
                className={cn(
                  "card flex items-center gap-3 !py-3 transition-colors",
                  isClickable && "cursor-pointer hover:border-cs-accent/30"
                )}
              >
                <div
                  className="w-1 h-8 rounded-full shrink-0"
                  style={{ backgroundColor: DEP_TYPE_COLORS[dep.type] }}
                />
                <FileText size={16} className="text-cs-muted shrink-0" />
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-medium truncate">{dep.name}</p>
                  <p className="text-xs text-cs-muted font-mono truncate">{dep.path}</p>
                </div>
                <div className="flex items-center gap-3 shrink-0">
                  <span className="text-xs text-cs-muted font-mono">
                    {formatNumber(dep.tokens)}
                  </span>
                  {isClickable && <ExternalLink size={12} className="text-cs-muted/40" />}
                  <span className={cn(
                    "w-2 h-2 rounded-full",
                    dep.loaded ? "bg-green-400" : "bg-cs-muted/40"
                  )} />
                </div>
              </div>
            );
          })}

          <div className="card !p-3 border-dashed">
            <p className="text-xs text-cs-muted text-center">
              {t('context.totalDeps', { count: RUNTIME_DEPENDENCIES[activeRuntime].length, tokens: formatNumber(RUNTIME_DEPENDENCIES[activeRuntime].reduce((s, d) => s + d.tokens, 0)) })}
            </p>
          </div>
        </div>
      )}

      {/* Permissions View */}
      {detailView === "permissions" && (
        <div className="space-y-2">
          <div className="card !p-3">
            <p className="text-xs text-cs-muted">
              {t('context.permInfo')}
            </p>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            {RUNTIME_PERMISSIONS[activeRuntime].map((perm) => (
              <div key={perm.tool} className="card flex items-center gap-3 !py-3">
                <div className={cn(
                  "w-8 h-8 rounded-lg flex items-center justify-center text-xs font-mono font-bold shrink-0",
                  perm.status === "allowed"
                    ? "bg-green-500/10 text-green-400 border border-green-500/20"
                    : perm.status === "ask"
                      ? "bg-yellow-500/10 text-yellow-400 border border-yellow-500/20"
                      : "bg-red-500/10 text-red-400 border border-red-500/20"
                )}>
                  {perm.status === "allowed" ? "\u2713" : perm.status === "ask" ? "?" : "\u2717"}
                </div>
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-medium font-mono">{perm.tool}</p>
                  <p className="text-xs text-cs-muted">{perm.scope}</p>
                </div>
                <span className={cn(
                  "text-[10px] font-medium uppercase px-1.5 py-0.5 rounded",
                  perm.status === "allowed"
                    ? "bg-green-500/10 text-green-400"
                    : perm.status === "ask"
                      ? "bg-yellow-500/10 text-yellow-400"
                      : "bg-red-500/10 text-red-400"
                )}>
                  {perm.status}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Live Session View (Phase 4 - Claude only) */}
      {detailView === "live" && activeRuntime === "claude" && (
        <div className="space-y-4">
          {/* Session Status */}
          <div className="card">
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-2">
                <div className={cn(
                  "w-3 h-3 rounded-full",
                  liveSession?.isActive ? "bg-green-400 animate-pulse" : "bg-cs-muted/40"
                )} />
                <span className="text-sm font-medium">
                  {liveSession?.isActive ? "Active Session" : "No Active Session"}
                </span>
              </div>
              {liveSession?.model && (
                <span className="text-xs px-2 py-1 rounded bg-cs-border/50 text-cs-muted font-mono">
                  {liveSession.model.split('-').slice(-2).join('-')}
                </span>
              )}
            </div>

            {liveSession?.sessionId ? (
              <div className="space-y-3">
                {/* Session Info */}
                <div className="grid grid-cols-2 gap-3">
                  <div className="flex items-center gap-2">
                    <Clock size={14} className="text-cs-muted" />
                    <div>
                      <p className="text-[10px] text-cs-muted uppercase">Started</p>
                      <p className="text-xs font-mono">
                        {liveSession.startedAt
                          ? new Date(liveSession.startedAt).toLocaleTimeString()
                          : "—"}
                      </p>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <MessageSquare size={14} className="text-cs-muted" />
                    <div>
                      <p className="text-[10px] text-cs-muted uppercase">Messages</p>
                      <p className="text-xs font-mono">{formatNumber(liveSession.messageCount)}</p>
                    </div>
                  </div>
                </div>

                {/* Token Usage */}
                <div className="pt-3 border-t border-cs-border">
                  <p className="text-xs text-cs-muted mb-2">Token Usage</p>
                  <div className="grid grid-cols-2 lg:grid-cols-4 gap-2">
                    <div className="bg-cs-bg rounded-lg p-2">
                      <p className="text-[10px] text-cs-muted">Input</p>
                      <p className="text-sm font-semibold text-blue-400">
                        {formatNumber(liveSession.totalInputTokens)}
                      </p>
                    </div>
                    <div className="bg-cs-bg rounded-lg p-2">
                      <p className="text-[10px] text-cs-muted">Output</p>
                      <p className="text-sm font-semibold text-green-400">
                        {formatNumber(liveSession.totalOutputTokens)}
                      </p>
                    </div>
                    <div className="bg-cs-bg rounded-lg p-2">
                      <p className="text-[10px] text-cs-muted">Cache Read</p>
                      <p className="text-sm font-semibold text-cyan-400">
                        {formatNumber(liveSession.cacheReadTokens)}
                      </p>
                    </div>
                    <div className="bg-cs-bg rounded-lg p-2">
                      <p className="text-[10px] text-cs-muted">Cache Write</p>
                      <p className="text-sm font-semibold text-purple-400">
                        {formatNumber(liveSession.cacheCreationTokens)}
                      </p>
                    </div>
                  </div>
                </div>

                {/* Tool Calls */}
                <div className="flex items-center justify-between pt-3 border-t border-cs-border">
                  <div className="flex items-center gap-2">
                    <Terminal size={14} className="text-cs-muted" />
                    <span className="text-xs text-cs-muted">Tool Calls</span>
                  </div>
                  <span className="text-sm font-mono">{formatNumber(liveSession.toolCallCount)}</span>
                </div>
              </div>
            ) : (
              <div className="text-center py-6">
                <p className="text-sm text-cs-muted">
                  Start a Claude Code session to see live context tracking.
                </p>
              </div>
            )}
          </div>

          {/* Files Read in Session */}
          {liveSession?.filesRead && liveSession.filesRead.length > 0 && (
            <div className="card">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <FileCode size={14} className="text-cs-muted" />
                  <span className="text-sm font-medium">Files Read ({liveSession.filesRead.length})</span>
                </div>
                <span className="text-xs text-cs-muted">
                  ~{formatNumber(liveSession.filesRead.reduce((s, f) => s + f.tokenEstimate, 0))} tokens
                </span>
              </div>
              <div className="space-y-1 max-h-64 overflow-y-auto">
                {liveSession.filesRead.slice(-20).reverse().map((file, i) => (
                  <div
                    key={`${file.path}-${i}`}
                    onClick={() => setViewingFile(file.path)}
                    className="flex items-center gap-2 px-2 py-1.5 rounded hover:bg-cs-border/30 cursor-pointer transition-colors"
                  >
                    <FileText size={12} className="text-cs-muted shrink-0" />
                    <span className="text-xs font-mono truncate flex-1" title={file.path}>
                      {file.path.split('/').slice(-2).join('/')}
                    </span>
                    <span className="text-[10px] text-cs-muted shrink-0">
                      {formatNumber(file.tokenEstimate)}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Cache Performance */}
          {liveSession && (liveSession.cacheReadTokens > 0 || liveSession.cacheCreationTokens > 0) && (
            <div className="card">
              <div className="flex items-center gap-2 mb-3">
                <Database size={14} className="text-cs-muted" />
                <span className="text-sm font-medium">Cache Performance</span>
              </div>
              <div className="space-y-2">
                <div>
                  <div className="flex justify-between text-xs mb-1">
                    <span className="text-cs-muted">Cache Hit Rate</span>
                    <span className="text-cyan-400">
                      {liveSession.totalInputTokens > 0
                        ? ((liveSession.cacheReadTokens / (liveSession.totalInputTokens + liveSession.cacheReadTokens)) * 100).toFixed(1)
                        : 0}%
                    </span>
                  </div>
                  <div className="w-full h-2 bg-cs-bg rounded-full overflow-hidden">
                    <div
                      className="h-full bg-cyan-400 rounded-full transition-all"
                      style={{
                        width: `${liveSession.totalInputTokens > 0
                          ? (liveSession.cacheReadTokens / (liveSession.totalInputTokens + liveSession.cacheReadTokens)) * 100
                          : 0}%`
                      }}
                    />
                  </div>
                </div>
                <p className="text-[10px] text-cs-muted">
                  {formatNumber(liveSession.cacheReadTokens)} tokens served from cache,
                  saving ~${((liveSession.cacheReadTokens * 0.9 * 15) / 1_000_000).toFixed(2)} on this session
                </p>
              </div>
            </div>
          )}

          {/* Live Context Breakdown */}
          {liveContext && (
            <div className="card">
              <div className="flex items-center gap-2 mb-3">
                <BarChart3 size={14} className="text-cs-muted" />
                <span className="text-sm font-medium">Live Context Breakdown</span>
              </div>
              <div className="h-48">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart
                    data={liveContext.categories}
                    layout="vertical"
                    margin={{ left: 0, right: 10, top: 0, bottom: 0 }}
                  >
                    <XAxis
                      type="number"
                      tick={{ fill: "#8888a0", fontSize: 10 }}
                      tickFormatter={formatNumber}
                    />
                    <YAxis
                      type="category"
                      dataKey="name"
                      width={100}
                      tick={{ fill: "#8888a0", fontSize: 10 }}
                    />
                    <Tooltip
                      contentStyle={{
                        backgroundColor: "#16161e",
                        border: "1px solid #2a2a3a",
                        borderRadius: 6,
                        fontSize: 12,
                      }}
                      formatter={(value: number) => [formatNumber(value), "tokens"]}
                    />
                    <Bar dataKey="tokens" radius={[0, 4, 4, 0]}>
                      {liveContext.categories.map((cat, i) => (
                        <Cell key={i} fill={cat.color} />
                      ))}
                    </Bar>
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Close runtimeNotConnected wrapper */}
      </> }

      {/* File viewer slide-over */}
      {viewingFile && (
        <FileViewer filePath={viewingFile} onClose={() => setViewingFile(null)} />
      )}
    </div>
  );
}


function LoadingSkeleton() {
  return (
    <div className="space-y-6 animate-pulse">
      <div>
        <div className="h-6 w-40 bg-cs-border rounded" />
        <div className="h-4 w-64 bg-cs-border rounded mt-2" />
      </div>
      <div className="card">
        <div className="h-3 w-full bg-cs-bg rounded-full" />
      </div>
      <div className="card h-64" />
    </div>
  );
}
