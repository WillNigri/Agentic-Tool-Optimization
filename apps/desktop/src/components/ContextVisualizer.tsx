import { useQuery } from "@tanstack/react-query";
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  Cell,
} from "recharts";
import { useTranslation } from "react-i18next";
import { getContextBreakdown } from "@/lib/api";
import { formatNumber, cn } from "@/lib/utils";

export default function ContextVisualizer() {
  const { t } = useTranslation();
  const { data, isLoading } = useQuery({
    queryKey: ["context-breakdown"],
    queryFn: getContextBreakdown,
  });

  if (isLoading) {
    return <LoadingSkeleton />;
  }

  if (!data) {
    return (
      <div className="text-cs-muted text-sm">
        No context data available. Start a Claude Code session to see context
        breakdown.
      </div>
    );
  }

  const usagePercent = (data.totalTokens / data.limit) * 100;
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

      {/* Overall progress */}
      <div className="card">
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
      </div>

      {/* Category breakdown chart */}
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
        {data.categories.map((cat) => (
          <div key={cat.name} className="card flex items-center gap-3">
            <div
              className="w-3 h-3 rounded-full shrink-0"
              style={{ backgroundColor: cat.color }}
            />
            <div className="min-w-0">
              <p className="text-sm truncate">{cat.name}</p>
              <p className="text-xs text-cs-muted">
                {t('context.tokens', { count: formatNumber(cat.tokens) })}
              </p>
            </div>
          </div>
        ))}
      </div>
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
