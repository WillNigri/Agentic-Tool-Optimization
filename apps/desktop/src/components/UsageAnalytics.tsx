import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  CartesianGrid,
  BarChart,
  Bar,
  PieChart,
  Pie,
  Cell,
  Legend,
} from "recharts";
import { useTranslation } from "react-i18next";
import {
  BarChart3,
  TrendingUp,
  CheckCircle,
  XCircle,
  Zap,
  Clock,
  Activity,
} from "lucide-react";
import { getUsageSummary, getDailyUsage, getBurnRate } from "@/lib/api";
import { getUsageMetrics } from "@/lib/tauri-api";
import { formatNumber, formatCurrency, cn } from "@/lib/utils";

const RUNTIME_COLORS: Record<string, string> = {
  claude: "#f97316", // orange-500
  codex: "#22c55e", // green-500
  hermes: "#a855f7", // purple-500
  openclaw: "#06b6d4", // cyan-500
};

const STATUS_COLORS = {
  success: "#22c55e",
  error: "#ef4444",
};

export default function UsageAnalytics() {
  const { t } = useTranslation();
  const [metricsRange, setMetricsRange] = useState<number>(30);

  const { data: summary, isLoading: loadingSummary } = useQuery({
    queryKey: ["usage-summary"],
    queryFn: getUsageSummary,
  });

  const { data: daily = [], isLoading: loadingDaily } = useQuery({
    queryKey: ["daily-usage"],
    queryFn: () => getDailyUsage(30),
  });

  const { data: burnRate } = useQuery({
    queryKey: ["burn-rate"],
    queryFn: getBurnRate,
  });

  const { data: executionMetrics } = useQuery({
    queryKey: ["usage-metrics", metricsRange],
    queryFn: () => getUsageMetrics(metricsRange),
  });

  const isLoading = loadingSummary || loadingDaily;

  if (isLoading) {
    return <LoadingSkeleton />;
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-semibold mb-1">{t('analytics.title')}</h2>
        <p className="text-cs-muted text-sm">
          {t('analytics.subtitle')}
        </p>
      </div>

      {/* Summary cards */}
      {summary && (
        <div className="grid grid-cols-3 gap-3">
          <SummaryCard
            label={t('analytics.today')}
            tokens={summary.today.inputTokens + summary.today.outputTokens}
            cost={summary.today.costCents}
          />
          <SummaryCard
            label={t('analytics.thisWeek')}
            tokens={summary.week.inputTokens + summary.week.outputTokens}
            cost={summary.week.costCents}
          />
          <SummaryCard
            label={t('analytics.thisMonth')}
            tokens={summary.month.inputTokens + summary.month.outputTokens}
            cost={summary.month.costCents}
          />
        </div>
      )}

      {/* Burn rate */}
      {burnRate && (
        <div className="card">
          <h3 className="text-sm font-medium text-cs-muted mb-3">{t('analytics.burnRate')}</h3>
          <div className="grid grid-cols-3 gap-4">
            <div>
              <p className="text-2xl font-semibold">
                {formatNumber(burnRate.tokensPerHour)}
              </p>
              <p className="text-xs text-cs-muted">{t('analytics.tokensPerHour', { count: burnRate.tokensPerHour })}</p>
            </div>
            <div>
              <p className="text-2xl font-semibold">
                {formatCurrency(burnRate.costPerHour)}
              </p>
              <p className="text-xs text-cs-muted">{t('analytics.cost', { amount: burnRate.costPerHour })}</p>
            </div>
            <div>
              <p className="text-2xl font-semibold">
                {burnRate.estimatedHoursToLimit != null
                  ? `${burnRate.estimatedHoursToLimit.toFixed(1)}h`
                  : "--"}
              </p>
              <p className="text-xs text-cs-muted">
                {burnRate.limit
                  ? `until ${formatNumber(burnRate.limit)} limit`
                  : t('analytics.unlimited')}
              </p>
            </div>
          </div>
        </div>
      )}

      {/* Execution Metrics Section */}
      {executionMetrics && (
        <>
          {/* Execution Overview Cards */}
          <div className="card">
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-sm font-medium text-cs-muted flex items-center gap-2">
                <Activity size={16} className="text-cs-accent" />
                Execution Metrics
              </h3>
              <div className="flex items-center gap-2">
                {[7, 14, 30].map((days) => (
                  <button
                    key={days}
                    onClick={() => setMetricsRange(days)}
                    className={cn(
                      "px-2 py-1 text-xs rounded transition-colors",
                      metricsRange === days
                        ? "bg-cs-accent text-cs-bg font-medium"
                        : "text-cs-muted hover:text-cs-text hover:bg-cs-border/50"
                    )}
                  >
                    {days}d
                  </button>
                ))}
              </div>
            </div>
            <div className="grid grid-cols-4 gap-4">
              <MetricCard
                icon={<Zap size={18} />}
                label="Total Executions"
                value={executionMetrics.totalExecutions}
                color="text-cs-accent"
              />
              <MetricCard
                icon={<CheckCircle size={18} />}
                label="Successful"
                value={executionMetrics.successfulExecutions}
                color="text-green-400"
                subtext={executionMetrics.totalExecutions > 0
                  ? `${((executionMetrics.successfulExecutions / executionMetrics.totalExecutions) * 100).toFixed(1)}%`
                  : "0%"
                }
              />
              <MetricCard
                icon={<XCircle size={18} />}
                label="Failed"
                value={executionMetrics.failedExecutions}
                color="text-red-400"
                subtext={executionMetrics.totalExecutions > 0
                  ? `${((executionMetrics.failedExecutions / executionMetrics.totalExecutions) * 100).toFixed(1)}%`
                  : "0%"
                }
              />
              <MetricCard
                icon={<Clock size={18} />}
                label="Avg Duration"
                value={executionMetrics.avgDurationMs
                  ? `${(executionMetrics.avgDurationMs / 1000).toFixed(1)}s`
                  : "-"
                }
                color="text-yellow-400"
              />
            </div>
          </div>

          {/* Runtime Breakdown */}
          <div className="grid grid-cols-2 gap-4">
            {/* Executions by Runtime Pie Chart */}
            <div className="card">
              <h3 className="text-sm font-medium text-cs-muted mb-4 flex items-center gap-2">
                <BarChart3 size={16} className="text-cs-accent" />
                Executions by Runtime
              </h3>
              {executionMetrics.executionsByRuntime.length > 0 ? (
                <div className="h-64">
                  <ResponsiveContainer width="100%" height="100%">
                    <PieChart>
                      <Pie
                        data={executionMetrics.executionsByRuntime}
                        dataKey="count"
                        nameKey="runtime"
                        cx="50%"
                        cy="50%"
                        outerRadius={80}
                        label={({ runtime, percent }) =>
                          `${runtime} ${(percent * 100).toFixed(0)}%`
                        }
                        labelLine={false}
                      >
                        {executionMetrics.executionsByRuntime.map((entry) => (
                          <Cell
                            key={entry.runtime}
                            fill={RUNTIME_COLORS[entry.runtime] || "#6b7280"}
                          />
                        ))}
                      </Pie>
                      <Tooltip
                        contentStyle={{
                          backgroundColor: "#16161e",
                          border: "1px solid #2a2a3a",
                          borderRadius: 6,
                          fontSize: 13,
                        }}
                        formatter={(value: number, name: string) => [
                          formatNumber(value),
                          name.charAt(0).toUpperCase() + name.slice(1),
                        ]}
                      />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
              ) : (
                <div className="h-64 flex items-center justify-center text-cs-muted text-sm">
                  No execution data available
                </div>
              )}
            </div>

            {/* Success/Error by Runtime Bar Chart */}
            <div className="card">
              <h3 className="text-sm font-medium text-cs-muted mb-4 flex items-center gap-2">
                <TrendingUp size={16} className="text-cs-accent" />
                Success Rate by Runtime
              </h3>
              {executionMetrics.executionsByRuntime.length > 0 ? (
                <div className="h-64">
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart
                      data={executionMetrics.executionsByRuntime}
                      layout="vertical"
                      margin={{ left: 60, right: 20 }}
                    >
                      <CartesianGrid strokeDasharray="3 3" stroke="#2a2a3a" horizontal={false} />
                      <XAxis type="number" tick={{ fill: "#8888a0", fontSize: 11 }} />
                      <YAxis
                        type="category"
                        dataKey="runtime"
                        tick={{ fill: "#8888a0", fontSize: 11 }}
                        tickFormatter={(v: string) => v.charAt(0).toUpperCase() + v.slice(1)}
                      />
                      <Tooltip
                        contentStyle={{
                          backgroundColor: "#16161e",
                          border: "1px solid #2a2a3a",
                          borderRadius: 6,
                          fontSize: 13,
                        }}
                        formatter={(value: number, name: string) => [
                          formatNumber(value),
                          name === "successCount" ? "Success" : "Error",
                        ]}
                      />
                      <Bar
                        dataKey="successCount"
                        stackId="a"
                        fill={STATUS_COLORS.success}
                        name="successCount"
                      />
                      <Bar
                        dataKey="errorCount"
                        stackId="a"
                        fill={STATUS_COLORS.error}
                        name="errorCount"
                      />
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              ) : (
                <div className="h-64 flex items-center justify-center text-cs-muted text-sm">
                  No execution data available
                </div>
              )}
              <div className="flex items-center gap-6 mt-3">
                <div className="flex items-center gap-2 text-xs text-cs-muted">
                  <div className="w-3 h-3 rounded" style={{ backgroundColor: STATUS_COLORS.success }} />
                  Success
                </div>
                <div className="flex items-center gap-2 text-xs text-cs-muted">
                  <div className="w-3 h-3 rounded" style={{ backgroundColor: STATUS_COLORS.error }} />
                  Error
                </div>
              </div>
            </div>
          </div>

          {/* Daily Executions Chart */}
          <div className="card">
            <h3 className="text-sm font-medium text-cs-muted mb-4 flex items-center gap-2">
              <TrendingUp size={16} className="text-cs-accent" />
              Daily Execution Trend
            </h3>
            {executionMetrics.executionsByDay.length > 0 ? (
              <div className="h-64">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart
                    data={executionMetrics.executionsByDay}
                    margin={{ left: 10, right: 10, top: 5, bottom: 5 }}
                  >
                    <CartesianGrid strokeDasharray="3 3" stroke="#2a2a3a" />
                    <XAxis
                      dataKey="date"
                      tick={{ fill: "#8888a0", fontSize: 11 }}
                      tickFormatter={(d: string) => {
                        const date = new Date(d);
                        return `${date.getMonth() + 1}/${date.getDate()}`;
                      }}
                    />
                    <YAxis tick={{ fill: "#8888a0", fontSize: 11 }} />
                    <Tooltip
                      contentStyle={{
                        backgroundColor: "#16161e",
                        border: "1px solid #2a2a3a",
                        borderRadius: 6,
                        fontSize: 13,
                      }}
                      labelFormatter={(d: string) => new Date(d).toLocaleDateString()}
                      formatter={(value: number, name: string) => [
                        formatNumber(value),
                        name === "successCount" ? "Success" : "Error",
                      ]}
                    />
                    <Bar
                      dataKey="successCount"
                      stackId="a"
                      fill={STATUS_COLORS.success}
                      name="successCount"
                    />
                    <Bar
                      dataKey="errorCount"
                      stackId="a"
                      fill={STATUS_COLORS.error}
                      name="errorCount"
                    />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            ) : (
              <div className="h-64 flex items-center justify-center text-cs-muted text-sm">
                No daily execution data available
              </div>
            )}
            <div className="flex items-center gap-6 mt-3">
              <div className="flex items-center gap-2 text-xs text-cs-muted">
                <div className="w-3 h-3 rounded" style={{ backgroundColor: STATUS_COLORS.success }} />
                Successful
              </div>
              <div className="flex items-center gap-2 text-xs text-cs-muted">
                <div className="w-3 h-3 rounded" style={{ backgroundColor: STATUS_COLORS.error }} />
                Failed
              </div>
            </div>
          </div>

          {/* Token Breakdown */}
          <div className="card">
            <h3 className="text-sm font-medium text-cs-muted mb-4">Token Usage Breakdown</h3>
            <div className="grid grid-cols-2 gap-6">
              <div>
                <p className="text-xs text-cs-muted mb-1">Input Tokens</p>
                <p className="text-2xl font-semibold text-cs-accent">
                  {formatNumber(executionMetrics.totalTokensIn)}
                </p>
              </div>
              <div>
                <p className="text-xs text-cs-muted mb-1">Output Tokens</p>
                <p className="text-2xl font-semibold text-green-400">
                  {formatNumber(executionMetrics.totalTokensOut)}
                </p>
              </div>
            </div>
            {(executionMetrics.totalTokensIn > 0 || executionMetrics.totalTokensOut > 0) && (
              <div className="mt-4">
                <div className="h-3 rounded-full bg-cs-border overflow-hidden flex">
                  <div
                    className="h-full bg-cs-accent"
                    style={{
                      width: `${(executionMetrics.totalTokensIn / (executionMetrics.totalTokensIn + executionMetrics.totalTokensOut)) * 100}%`,
                    }}
                  />
                  <div
                    className="h-full bg-green-400"
                    style={{
                      width: `${(executionMetrics.totalTokensOut / (executionMetrics.totalTokensIn + executionMetrics.totalTokensOut)) * 100}%`,
                    }}
                  />
                </div>
                <div className="flex items-center justify-between mt-2 text-xs text-cs-muted">
                  <span>Input: {((executionMetrics.totalTokensIn / (executionMetrics.totalTokensIn + executionMetrics.totalTokensOut)) * 100).toFixed(1)}%</span>
                  <span>Output: {((executionMetrics.totalTokensOut / (executionMetrics.totalTokensIn + executionMetrics.totalTokensOut)) * 100).toFixed(1)}%</span>
                </div>
              </div>
            )}
          </div>
        </>
      )}

      {/* 30-day chart */}
      <div className="card">
        <h3 className="text-sm font-medium text-cs-muted mb-4">
          {t('analytics.dailyUsage')}
        </h3>
        <div className="h-72">
          <ResponsiveContainer width="100%" height="100%">
            <LineChart
              data={daily}
              margin={{ left: 10, right: 10, top: 5, bottom: 5 }}
            >
              <CartesianGrid strokeDasharray="3 3" stroke="#2a2a3a" />
              <XAxis
                dataKey="date"
                tick={{ fill: "#8888a0", fontSize: 11 }}
                tickFormatter={(d: string) => {
                  const date = new Date(d);
                  return `${date.getMonth() + 1}/${date.getDate()}`;
                }}
              />
              <YAxis
                tick={{ fill: "#8888a0", fontSize: 11 }}
                tickFormatter={formatNumber}
              />
              <Tooltip
                contentStyle={{
                  backgroundColor: "#16161e",
                  border: "1px solid #2a2a3a",
                  borderRadius: 6,
                  fontSize: 13,
                }}
                labelStyle={{ color: "#e8e8f0" }}
                formatter={(value: number, name: string) => [
                  formatNumber(value),
                  name === "inputTokens" ? t('analytics.inputTokens') : t('analytics.outputTokens'),
                ]}
              />
              <Line
                type="monotone"
                dataKey="inputTokens"
                stroke="#00FFB2"
                strokeWidth={2}
                dot={false}
              />
              <Line
                type="monotone"
                dataKey="outputTokens"
                stroke="#00e6a0"
                strokeWidth={2}
                dot={false}
              />
            </LineChart>
          </ResponsiveContainer>
        </div>
        <div className="flex items-center gap-6 mt-3">
          <div className="flex items-center gap-2 text-xs text-cs-muted">
            <div className="w-3 h-0.5 bg-cs-accent rounded" />
            {t('analytics.inputTokens')}
          </div>
          <div className="flex items-center gap-2 text-xs text-cs-muted">
            <div className="w-3 h-0.5 bg-cs-success rounded" />
            {t('analytics.outputTokens')}
          </div>
        </div>
      </div>
    </div>
  );
}

function SummaryCard({
  label,
  tokens,
  cost,
}: {
  label: string;
  tokens: number;
  cost: number;
}) {
  return (
    <div className="card">
      <p className="text-xs text-cs-muted mb-1">{label}</p>
      <p className="text-xl font-semibold">{formatNumber(tokens)}</p>
      <p className="text-sm text-cs-muted">{formatCurrency(cost)}</p>
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="space-y-6 animate-pulse">
      <div>
        <div className="h-6 w-40 bg-cs-border rounded" />
        <div className="h-4 w-56 bg-cs-border rounded mt-2" />
      </div>
      <div className="grid grid-cols-3 gap-3">
        {[1, 2, 3].map((i) => (
          <div key={i} className="card h-20" />
        ))}
      </div>
      <div className="card h-72" />
    </div>
  );
}

function MetricCard({
  icon,
  label,
  value,
  color,
  subtext,
}: {
  icon: React.ReactNode;
  label: string;
  value: string | number;
  color: string;
  subtext?: string;
}) {
  return (
    <div className="p-3 rounded-lg bg-cs-bg border border-cs-border">
      <div className={cn("mb-2", color)}>{icon}</div>
      <p className="text-2xl font-semibold">
        {typeof value === "number" ? formatNumber(value) : value}
      </p>
      <p className="text-xs text-cs-muted">{label}</p>
      {subtext && <p className="text-xs text-cs-muted mt-1">{subtext}</p>}
    </div>
  );
}
