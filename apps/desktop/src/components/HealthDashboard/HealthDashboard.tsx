import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Area,
  AreaChart,
} from "recharts";
import {
  Activity,
  RefreshCw,
  Loader2,
  CheckCircle,
  XCircle,
  AlertTriangle,
  Clock,
  Wifi,
  WifiOff,
  Zap,
  Server,
  TrendingUp,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  getHealthStatus,
  getHealthHistory,
  type RuntimeHealth,
  type RuntimeHealthHistory,
} from "@/lib/api";

const RUNTIME_CONFIG = {
  claude: {
    name: "Claude",
    color: "#f97316",
    bgColor: "bg-orange-500/10",
    borderColor: "border-orange-500/30",
    textColor: "text-orange-400",
  },
  codex: {
    name: "Codex",
    color: "#22c55e",
    bgColor: "bg-green-500/10",
    borderColor: "border-green-500/30",
    textColor: "text-green-400",
  },
  hermes: {
    name: "Hermes",
    color: "#a855f7",
    bgColor: "bg-purple-500/10",
    borderColor: "border-purple-500/30",
    textColor: "text-purple-400",
  },
  openclaw: {
    name: "OpenClaw",
    color: "#06b6d4",
    bgColor: "bg-cyan-500/10",
    borderColor: "border-cyan-500/30",
    textColor: "text-cyan-400",
  },
};

const STATUS_CONFIG = {
  healthy: {
    icon: CheckCircle,
    color: "text-green-400",
    bg: "bg-green-400/10",
    label: "Healthy",
  },
  degraded: {
    icon: AlertTriangle,
    color: "text-yellow-400",
    bg: "bg-yellow-400/10",
    label: "Degraded",
  },
  down: {
    icon: XCircle,
    color: "text-red-400",
    bg: "bg-red-400/10",
    label: "Down",
  },
  unknown: {
    icon: Clock,
    color: "text-cs-muted",
    bg: "bg-cs-border",
    label: "Unknown",
  },
};

export default function HealthDashboard() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [selectedRuntime, setSelectedRuntime] = useState<string | null>(null);
  const [timeRange, setTimeRange] = useState<number>(24); // hours

  // Fetch health status
  const { data: healthStatuses = [], isLoading, refetch, isFetching } = useQuery({
    queryKey: ["health-status"],
    queryFn: getHealthStatus,
    refetchInterval: 30000, // Auto-refresh every 30s
  });

  // Fetch health history for charts
  const { data: healthHistory = [] } = useQuery({
    queryKey: ["health-history", timeRange],
    queryFn: () => getHealthHistory(undefined, timeRange),
    refetchInterval: 60000, // Refresh every minute
  });

  // Listen for real-time health updates
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    const setupListener = async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<RuntimeHealth[]>("health-update", (event) => {
          queryClient.setQueryData(["health-status"], event.payload);
        });
      } catch (err) {
        console.error("Failed to setup health listener:", err);
      }
    };

    setupListener();

    return () => {
      if (unlisten) unlisten();
    };
  }, [queryClient]);

  const getOverallStatus = () => {
    if (healthStatuses.length === 0) return "unknown";
    const healthyCount = healthStatuses.filter((h) => h.status === "healthy").length;
    const downCount = healthStatuses.filter((h) => h.status === "down").length;

    if (downCount === healthStatuses.length) return "down";
    if (healthyCount === healthStatuses.length) return "healthy";
    return "degraded";
  };

  const getLatencyColor = (ms?: number) => {
    if (!ms) return "text-cs-muted";
    if (ms < 100) return "text-green-400";
    if (ms < 500) return "text-yellow-400";
    return "text-red-400";
  };

  const formatLatency = (ms?: number) => {
    if (!ms) return "N/A";
    return `${ms}ms`;
  };

  const formatTime = (timestamp: string) => {
    const date = new Date(timestamp);
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  };

  // Prepare chart data for selected runtime or all
  const getChartData = () => {
    const runtime = selectedRuntime
      ? healthHistory.find((h) => h.runtime === selectedRuntime)
      : null;

    if (runtime) {
      return runtime.dataPoints.map((point) => ({
        time: formatTime(point.timestamp),
        latency: point.latencyMs || 0,
        status: point.status === "healthy" ? 1 : 0,
      }));
    }

    // If no runtime selected, show combined view
    const allPoints: Record<string, { time: string; [key: string]: number | string }> = {};

    healthHistory.forEach((runtimeHistory) => {
      runtimeHistory.dataPoints.forEach((point) => {
        const time = formatTime(point.timestamp);
        if (!allPoints[time]) {
          allPoints[time] = { time };
        }
        allPoints[time][runtimeHistory.runtime] = point.latencyMs || 0;
      });
    });

    return Object.values(allPoints);
  };

  const chartData = getChartData();

  const overallStatus = getOverallStatus();
  const OverallIcon = STATUS_CONFIG[overallStatus as keyof typeof STATUS_CONFIG]?.icon || Clock;

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="animate-spin text-cs-accent" size={32} />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <Activity className="text-cs-accent" size={24} />
            {t("health.title", "System Health")}
          </h2>
          <p className="text-sm text-cs-muted mt-1">
            {t("health.subtitle", "Monitor runtime connectivity and performance")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <select
            value={timeRange}
            onChange={(e) => setTimeRange(Number(e.target.value))}
            className="bg-cs-card border border-cs-border rounded-md px-3 py-2 text-sm"
          >
            <option value={1}>Last hour</option>
            <option value={6}>Last 6 hours</option>
            <option value={24}>Last 24 hours</option>
            <option value={168}>Last 7 days</option>
          </select>
          <button
            onClick={() => refetch()}
            disabled={isFetching}
            className="flex items-center gap-2 p-2 rounded-md border border-cs-border hover:bg-cs-border/50 transition-colors disabled:opacity-50"
          >
            <RefreshCw size={16} className={isFetching ? "animate-spin" : ""} />
          </button>
        </div>
      </div>

      {/* Overall Status */}
      <div
        className={cn(
          "rounded-lg border p-6",
          STATUS_CONFIG[overallStatus as keyof typeof STATUS_CONFIG]?.bg || "bg-cs-card",
          "border-cs-border"
        )}
      >
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <OverallIcon
              size={48}
              className={STATUS_CONFIG[overallStatus as keyof typeof STATUS_CONFIG]?.color || "text-cs-muted"}
            />
            <div>
              <h3 className="text-lg font-semibold">Overall System Status</h3>
              <p className={cn(
                "text-sm font-medium",
                STATUS_CONFIG[overallStatus as keyof typeof STATUS_CONFIG]?.color || "text-cs-muted"
              )}>
                {STATUS_CONFIG[overallStatus as keyof typeof STATUS_CONFIG]?.label || "Unknown"}
              </p>
            </div>
          </div>
          <div className="text-right text-sm text-cs-muted">
            <div className="flex items-center gap-2 justify-end">
              <Server size={14} />
              {healthStatuses.length} runtimes monitored
            </div>
            <div className="flex items-center gap-2 justify-end mt-1">
              <Zap size={14} />
              Auto-refreshing every 30s
            </div>
          </div>
        </div>

        {/* Quick stats */}
        <div className="grid grid-cols-4 gap-4 mt-6">
          <div className="text-center">
            <p className="text-2xl font-bold text-green-400">
              {healthStatuses.filter((h) => h.status === "healthy").length}
            </p>
            <p className="text-xs text-cs-muted">Healthy</p>
          </div>
          <div className="text-center">
            <p className="text-2xl font-bold text-yellow-400">
              {healthStatuses.filter((h) => h.status === "degraded").length}
            </p>
            <p className="text-xs text-cs-muted">Degraded</p>
          </div>
          <div className="text-center">
            <p className="text-2xl font-bold text-red-400">
              {healthStatuses.filter((h) => h.status === "down").length}
            </p>
            <p className="text-xs text-cs-muted">Down</p>
          </div>
          <div className="text-center">
            <p className="text-2xl font-bold text-cs-muted">
              {healthStatuses.filter((h) => h.status === "unknown").length}
            </p>
            <p className="text-xs text-cs-muted">Unknown</p>
          </div>
        </div>
      </div>

      {/* Latency Chart */}
      <div className="rounded-lg border border-cs-border bg-cs-card p-4">
        <div className="flex items-center justify-between mb-4">
          <h3 className="font-medium flex items-center gap-2">
            <TrendingUp size={18} />
            Latency History
          </h3>
          <div className="flex gap-2">
            <button
              onClick={() => setSelectedRuntime(null)}
              className={cn(
                "px-3 py-1 text-xs rounded-md transition-colors",
                !selectedRuntime
                  ? "bg-cs-accent text-black"
                  : "bg-cs-border text-cs-muted hover:text-white"
              )}
            >
              All
            </button>
            {Object.entries(RUNTIME_CONFIG).map(([key, config]) => (
              <button
                key={key}
                onClick={() => setSelectedRuntime(key)}
                className={cn(
                  "px-3 py-1 text-xs rounded-md transition-colors",
                  selectedRuntime === key
                    ? config.bgColor + " " + config.textColor
                    : "bg-cs-border text-cs-muted hover:text-white"
                )}
              >
                {config.name}
              </button>
            ))}
          </div>
        </div>

        <div className="h-64">
          <ResponsiveContainer width="100%" height="100%">
            {selectedRuntime ? (
              <AreaChart data={chartData}>
                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                <XAxis dataKey="time" stroke="#666" fontSize={10} />
                <YAxis stroke="#666" fontSize={10} unit="ms" />
                <Tooltip
                  contentStyle={{
                    backgroundColor: "#1a1a1f",
                    border: "1px solid #333",
                    borderRadius: "8px",
                  }}
                />
                <Area
                  type="monotone"
                  dataKey="latency"
                  stroke={RUNTIME_CONFIG[selectedRuntime as keyof typeof RUNTIME_CONFIG]?.color}
                  fill={RUNTIME_CONFIG[selectedRuntime as keyof typeof RUNTIME_CONFIG]?.color}
                  fillOpacity={0.2}
                  strokeWidth={2}
                />
              </AreaChart>
            ) : (
              <LineChart data={chartData}>
                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                <XAxis dataKey="time" stroke="#666" fontSize={10} />
                <YAxis stroke="#666" fontSize={10} unit="ms" />
                <Tooltip
                  contentStyle={{
                    backgroundColor: "#1a1a1f",
                    border: "1px solid #333",
                    borderRadius: "8px",
                  }}
                />
                {Object.entries(RUNTIME_CONFIG).map(([key, config]) => (
                  <Line
                    key={key}
                    type="monotone"
                    dataKey={key}
                    stroke={config.color}
                    strokeWidth={2}
                    dot={false}
                  />
                ))}
              </LineChart>
            )}
          </ResponsiveContainer>
        </div>

        {/* Legend */}
        {!selectedRuntime && (
          <div className="flex items-center justify-center gap-6 mt-4">
            {Object.entries(RUNTIME_CONFIG).map(([key, config]) => (
              <div key={key} className="flex items-center gap-2">
                <div
                  className="w-3 h-3 rounded-full"
                  style={{ backgroundColor: config.color }}
                />
                <span className="text-xs text-cs-muted">{config.name}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Runtime Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {healthStatuses.map((health) => {
          const runtime = RUNTIME_CONFIG[health.runtime as keyof typeof RUNTIME_CONFIG];
          const status = STATUS_CONFIG[health.status as keyof typeof STATUS_CONFIG] || STATUS_CONFIG.unknown;
          const StatusIcon = status.icon;
          const historyData = healthHistory.find((h) => h.runtime === health.runtime);

          return (
            <div
              key={health.runtime}
              className={cn(
                "rounded-lg border p-4 transition-all",
                runtime?.bgColor || "bg-cs-card",
                runtime?.borderColor || "border-cs-border"
              )}
            >
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-3">
                  <div className={cn("w-10 h-10 rounded-lg flex items-center justify-center", status.bg)}>
                    <StatusIcon size={20} className={status.color} />
                  </div>
                  <div>
                    <h4 className={cn("font-semibold", runtime?.textColor || "text-white")}>
                      {runtime?.name || health.runtime}
                    </h4>
                    <p className={cn("text-xs font-medium", status.color)}>{status.label}</p>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  {health.status === "healthy" ? (
                    <Wifi size={16} className="text-green-400" />
                  ) : health.status === "down" ? (
                    <WifiOff size={16} className="text-red-400" />
                  ) : (
                    <Wifi size={16} className="text-yellow-400" />
                  )}
                </div>
              </div>

              <div className="grid grid-cols-3 gap-4 text-sm">
                <div>
                  <p className="text-xs text-cs-muted">Latency</p>
                  <p className={cn("font-mono font-medium", getLatencyColor(health.latencyMs ?? undefined))}>
                    {formatLatency(health.latencyMs ?? undefined)}
                  </p>
                </div>
                <div>
                  <p className="text-xs text-cs-muted">Uptime</p>
                  <p className={cn(
                    "font-mono font-medium",
                    (historyData?.uptimePercent ?? 0) >= 99 ? "text-green-400" :
                    (historyData?.uptimePercent ?? 0) >= 90 ? "text-yellow-400" : "text-red-400"
                  )}>
                    {historyData?.uptimePercent?.toFixed(1) ?? "0"}%
                  </p>
                </div>
                <div>
                  <p className="text-xs text-cs-muted">Avg Latency</p>
                  <p className="font-mono font-medium text-cs-muted">
                    {historyData?.avgLatencyMs ? `${Math.round(historyData.avgLatencyMs)}ms` : "N/A"}
                  </p>
                </div>
              </div>

              {health.errorMessage && (
                <div className="mt-3 p-2 rounded bg-red-500/10 border border-red-500/30">
                  <p className="text-xs text-red-400">{health.errorMessage}</p>
                </div>
              )}

              <div className="mt-3 text-xs text-cs-muted">
                Last check: {health.lastCheck ? new Date(health.lastCheck).toLocaleTimeString() : "Never"}
              </div>
            </div>
          );
        })}
      </div>

      {/* Empty state */}
      {healthStatuses.length === 0 && (
        <div className="text-center py-12 text-cs-muted">
          <Activity size={48} className="mx-auto mb-4 opacity-50" />
          <p>No runtimes detected</p>
          <p className="text-sm mt-1">Configure your runtimes to start monitoring</p>
        </div>
      )}

      {/* Info */}
      <div className="p-4 rounded-lg border border-cs-border bg-cs-card">
        <p className="text-sm text-cs-muted">
          Health checks run automatically every 30 seconds in the background.
          The latency chart shows response times over the selected time period.
          Click on a runtime filter to see detailed latency history.
        </p>
      </div>
    </div>
  );
}
