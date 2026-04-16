import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Activity,
  Zap,
  Clock,
  Cpu,
  Wifi,
  WifiOff,
  AlertCircle,
  BarChart3,
  RefreshCw,
  Crown,
} from "lucide-react";
import {
  getMonitoringSnapshot,
  type AgentSession,
} from "@/lib/api";
import { cn } from "@/lib/utils";
import { useAuthStore } from "@/hooks/useAuth";

const RUNTIME_COLORS: Record<string, string> = {
  claude: "#D4A574",
  codex: "#74AA9C",
  openclaw: "#F97316",
  hermes: "#8B5CF6",
};

function formatDuration(ms?: number): string {
  if (!ms) return "-";
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60000).toFixed(1)}m`;
}

function formatTokens(n: number): string {
  if (n >= 1000000) return `${(n / 1000000).toFixed(1)}M`;
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
  return n.toString();
}

function formatTimeAgo(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const seconds = Math.floor((now.getTime() - date.getTime()) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return date.toLocaleDateString();
}

export default function AgentMonitor() {
  const { t } = useTranslation();
  const isCloudUser = useAuthStore((s) => s.isCloudUser);

  // Free tier: manual refresh only (no refetchInterval)
  const { data: snapshot, isLoading, refetch, isFetching } = useQuery({
    queryKey: ["monitoring-snapshot"],
    queryFn: getMonitoringSnapshot,
  });

  if (isLoading || !snapshot) {
    return (
      <div className="space-y-6 animate-pulse">
        <div className="h-8 bg-cs-border/30 rounded w-48" />
        <div className="grid grid-cols-4 gap-4">
          {[1, 2, 3, 4].map((i) => (
            <div key={i} className="card h-24" />
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <Activity className="w-5 h-5 text-cs-accent" />
            Agent Monitor
          </h2>
          <p className="text-cs-muted text-sm">
            Session overview across all agent runtimes
          </p>
        </div>
        <button
          onClick={() => refetch()}
          disabled={isFetching}
          className="flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-md bg-cs-border/50 hover:bg-cs-border transition-colors disabled:opacity-50"
        >
          <RefreshCw className={cn("w-3.5 h-3.5", isFetching && "animate-spin")} />
          Refresh
        </button>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-4 gap-4">
        <div className="card p-4">
          <div className="flex items-center gap-2 mb-2">
            <Zap className="w-4 h-4 text-cs-accent" />
            <span className="text-xs text-cs-muted uppercase tracking-wide">Tokens Today</span>
          </div>
          <div className="text-2xl font-bold">{formatTokens(snapshot.totalTokensToday)}</div>
        </div>

        <div className="card p-4">
          <div className="flex items-center gap-2 mb-2">
            <Cpu className="w-4 h-4 text-blue-400" />
            <span className="text-xs text-cs-muted uppercase tracking-wide">Sessions</span>
          </div>
          <div className="text-2xl font-bold">{snapshot.totalSessionsToday}</div>
        </div>

        <div className="card p-4">
          <div className="flex items-center gap-2 mb-2">
            <Clock className="w-4 h-4 text-purple-400" />
            <span className="text-xs text-cs-muted uppercase tracking-wide">Avg Duration</span>
          </div>
          <div className="text-2xl font-bold">{formatDuration(snapshot.avgDurationMs)}</div>
        </div>

        <div className="card p-4">
          <div className="flex items-center gap-2 mb-2">
            <AlertCircle className={cn("w-4 h-4", snapshot.errorsToday > 0 ? "text-red-400" : "text-emerald-400")} />
            <span className="text-xs text-cs-muted uppercase tracking-wide">Errors</span>
          </div>
          <div className={cn("text-2xl font-bold", snapshot.errorsToday > 0 ? "text-red-400" : "")}>
            {snapshot.errorsToday}
          </div>
        </div>
      </div>

      {/* Runtime Status */}
      <div className="card p-4">
        <h3 className="text-sm font-medium mb-3 flex items-center gap-2">
          <BarChart3 className="w-4 h-4 text-cs-accent" />
          Runtime Status
        </h3>
        <div className="flex gap-4">
          {snapshot.runtimesOnline.map((rt) => (
            <div key={rt} className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-emerald-500/10">
              <Wifi className="w-3.5 h-3.5 text-emerald-400" />
              <span className="text-sm font-medium" style={{ color: RUNTIME_COLORS[rt] || "#888" }}>
                {rt}
              </span>
            </div>
          ))}
          {snapshot.runtimesOffline.map((rt) => (
            <div key={rt} className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-red-500/10">
              <WifiOff className="w-3.5 h-3.5 text-red-400" />
              <span className="text-sm text-cs-muted">{rt}</span>
            </div>
          ))}
          {snapshot.runtimesOnline.length === 0 && snapshot.runtimesOffline.length === 0 && (
            <span className="text-sm text-cs-muted">No runtime health data available</span>
          )}
        </div>
      </div>

      {/* Pro Upgrade Banner */}
      {!isCloudUser && (
        <div className="card p-4 flex items-center gap-4 border-cs-accent/20 bg-cs-accent/5">
          <Crown className="w-5 h-5 text-cs-accent shrink-0" />
          <div className="flex-1">
            <p className="text-sm font-medium">Upgrade to Pro for real-time monitoring</p>
            <p className="text-xs text-cs-muted mt-0.5">
              Live auto-refresh, smart alerts, token usage charts, team-wide monitoring, and cost projections.
            </p>
          </div>
        </div>
      )}

      {/* Recent Sessions */}
      <div className="card p-4">
        <h3 className="text-sm font-medium mb-3">Recent Sessions</h3>
        {snapshot.recentSessions.length === 0 ? (
          <p className="text-sm text-cs-muted py-4 text-center">No recent sessions</p>
        ) : (
          <div className="space-y-1">
            {snapshot.recentSessions.map((session) => (
              <SessionRow key={session.id} session={session} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function SessionRow({ session }: { session: AgentSession }) {
  return (
    <div className="flex items-center gap-3 px-3 py-2 rounded-md hover:bg-cs-border/20 transition-colors">
      <div
        className="w-2 h-2 rounded-full shrink-0"
        style={{
          backgroundColor: session.status === "running"
            ? "#34D399"
            : session.status === "error"
            ? "#F87171"
            : "#60A5FA",
        }}
      />
      <span
        className="text-xs font-medium px-1.5 py-0.5 rounded"
        style={{
          color: RUNTIME_COLORS[session.runtime] || "#888",
          backgroundColor: `${RUNTIME_COLORS[session.runtime] || "#888"}15`,
        }}
      >
        {session.runtime}
      </span>
      <div className="flex-1 min-w-0">
        <span className="text-sm truncate block">
          {session.skillName || session.prompt?.slice(0, 60) || "Agent session"}
        </span>
      </div>
      <div className="flex items-center gap-3 text-xs text-cs-muted">
        <span>{formatTokens(session.tokensIn + session.tokensOut)} tok</span>
        <span>{formatDuration(session.durationMs)}</span>
        <span>{formatTimeAgo(session.startedAt)}</span>
      </div>
    </div>
  );
}
