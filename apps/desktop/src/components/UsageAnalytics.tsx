import { useQuery } from "@tanstack/react-query";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  CartesianGrid,
} from "recharts";
import { useTranslation } from "react-i18next";
import { getUsageSummary, getDailyUsage, getBurnRate } from "@/lib/api";
import { formatNumber, formatCurrency } from "@/lib/utils";

export default function UsageAnalytics() {
  const { t } = useTranslation();
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
