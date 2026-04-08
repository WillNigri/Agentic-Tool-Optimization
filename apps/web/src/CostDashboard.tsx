import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  PieChart,
  Pie,
  Cell,
  AreaChart,
  Area,
} from 'recharts';
import {
  DollarSign,
  TrendingUp,
  Zap,
  Hash,
  Calendar,
} from 'lucide-react';
import { cn } from '@/lib/utils';

const API_BASE = import.meta.env.VITE_API_URL || 'https://api.agentictool.ai/api';

function getAuthHeaders(): Record<string, string> {
  const stored = localStorage.getItem('ato-auth');
  if (!stored) return {};
  try {
    const { state } = JSON.parse(stored);
    if (state?.accessToken) return { Authorization: `Bearer ${state.accessToken}` };
  } catch { /* ignore */ }
  return {};
}

async function fetchApi<T>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, { headers: { ...getAuthHeaders() } });
  if (!res.ok) throw new Error(`API error ${res.status}`);
  const json = await res.json();
  return json.data ?? json;
}

const COLORS = ['#00FFB2', '#6366F1', '#F97316', '#EC4899', '#3B82F6', '#EAB308', '#8B5CF6', '#14B8A6'];

function formatCost(n: number): string {
  if (n >= 1000) return `$${(n / 1000).toFixed(1)}K`;
  if (n >= 1) return `$${n.toFixed(2)}`;
  if (n >= 0.01) return `$${n.toFixed(3)}`;
  return `$${n.toFixed(4)}`;
}

function formatTokens(n: number): string {
  if (n >= 1000000) return `${(n / 1000000).toFixed(1)}M`;
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
  return n.toString();
}

export default function CostDashboard() {
  const [days, setDays] = useState(30);

  const { data: summary } = useQuery({
    queryKey: ['cost-summary', 'day'],
    queryFn: () => fetchApi<any>('/analytics/summary?period=day'),
  });

  const { data: monthlySummary } = useQuery({
    queryKey: ['cost-summary', 'month'],
    queryFn: () => fetchApi<any>('/analytics/summary?period=month'),
  });

  const { data: byModel = [] } = useQuery({
    queryKey: ['cost-by-model', days],
    queryFn: () => fetchApi<any[]>(`/analytics/cost/by-model?days=${days}`),
  });

  const { data: byProvider = [] } = useQuery({
    queryKey: ['cost-by-provider', days],
    queryFn: () => fetchApi<any[]>(`/analytics/cost/by-provider?days=${days}`),
  });

  const { data: timeline = [] } = useQuery({
    queryKey: ['cost-timeline', days],
    queryFn: () => fetchApi<any[]>(`/analytics/cost/timeline?days=${days}`),
  });

  const { data: burnRate } = useQuery({
    queryKey: ['burn-rate'],
    queryFn: () => fetchApi<any>('/analytics/burn-rate'),
    refetchInterval: 30000,
  });

  // Aggregate timeline by date for chart
  const timelineByDate = timeline.reduce((acc: any[], row: any) => {
    const existing = acc.find((d: any) => d.date === row.date);
    if (existing) {
      existing.cost += row.cost;
      existing.tokens += parseInt(row.tokens);
      existing.requests += row.requests;
    } else {
      acc.push({
        date: row.date,
        label: new Date(row.date).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }),
        cost: row.cost,
        tokens: parseInt(row.tokens),
        requests: row.requests,
      });
    }
    return acc;
  }, []);

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <DollarSign className="w-5 h-5 text-cs-accent" />
            Cost Dashboard
          </h2>
          <p className="text-cs-muted text-sm">
            LLM spend across all providers and models
          </p>
        </div>
        <select
          value={days}
          onChange={(e) => setDays(Number(e.target.value))}
          className="px-3 py-1.5 text-sm bg-cs-border/30 border border-cs-border rounded-md"
        >
          <option value={7}>Last 7 days</option>
          <option value={14}>Last 14 days</option>
          <option value={30}>Last 30 days</option>
          <option value={90}>Last 90 days</option>
        </select>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-4 gap-4">
        <div className="card p-4">
          <div className="flex items-center gap-2 mb-2">
            <DollarSign className="w-4 h-4 text-cs-accent" />
            <span className="text-xs text-cs-muted uppercase tracking-wide">Today</span>
          </div>
          <div className="text-2xl font-bold">
            {summary ? formatCost(summary.total_cost) : '—'}
          </div>
          <div className="text-xs text-cs-muted mt-1">
            {summary ? `${summary.record_count} requests` : ''}
          </div>
        </div>

        <div className="card p-4">
          <div className="flex items-center gap-2 mb-2">
            <Calendar className="w-4 h-4 text-blue-400" />
            <span className="text-xs text-cs-muted uppercase tracking-wide">This Month</span>
          </div>
          <div className="text-2xl font-bold">
            {monthlySummary ? formatCost(monthlySummary.total_cost) : '—'}
          </div>
          <div className="text-xs text-cs-muted mt-1">
            {monthlySummary ? `${monthlySummary.record_count} requests` : ''}
          </div>
        </div>

        <div className="card p-4">
          <div className="flex items-center gap-2 mb-2">
            <TrendingUp className="w-4 h-4 text-purple-400" />
            <span className="text-xs text-cs-muted uppercase tracking-wide">Burn Rate</span>
          </div>
          <div className="text-2xl font-bold">
            {burnRate ? `${formatCost(burnRate.cost_per_hour)}/hr` : '—'}
          </div>
          <div className="text-xs text-cs-muted mt-1">
            {burnRate ? `${formatTokens(burnRate.tokens_per_hour)} tok/hr` : ''}
          </div>
        </div>

        <div className="card p-4">
          <div className="flex items-center gap-2 mb-2">
            <Zap className="w-4 h-4 text-yellow-400" />
            <span className="text-xs text-cs-muted uppercase tracking-wide">Total Tokens</span>
          </div>
          <div className="text-2xl font-bold">
            {monthlySummary ? formatTokens(monthlySummary.total_input_tokens + monthlySummary.total_output_tokens) : '—'}
          </div>
        </div>
      </div>

      {/* Cost Timeline Chart */}
      {timelineByDate.length > 0 && (
        <div className="card p-4">
          <h3 className="text-sm font-medium mb-3">Daily Spend</h3>
          <ResponsiveContainer width="100%" height={220}>
            <AreaChart data={timelineByDate}>
              <XAxis dataKey="label" tick={{ fill: '#666', fontSize: 11 }} axisLine={{ stroke: '#333' }} />
              <YAxis tick={{ fill: '#666', fontSize: 11 }} axisLine={{ stroke: '#333' }} tickFormatter={(v) => formatCost(v)} />
              <Tooltip
                contentStyle={{ background: '#111116', border: '1px solid #333', borderRadius: '6px', color: '#e5e5e5', fontSize: '12px' }}
                formatter={(value: number) => [formatCost(value), 'Cost']}
              />
              <Area type="monotone" dataKey="cost" stroke="#00FFB2" fill="#00FFB2" fillOpacity={0.1} strokeWidth={2} />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      )}

      <div className="grid grid-cols-2 gap-4">
        {/* Cost by Model */}
        {byModel.length > 0 && (
          <div className="card p-4">
            <h3 className="text-sm font-medium mb-3">Cost by Model</h3>
            <div className="space-y-2">
              {byModel.map((row: any, i: number) => {
                const maxCost = byModel[0]?.total_cost || 1;
                return (
                  <div key={row.model} className="flex items-center gap-3">
                    <div className="w-2 h-2 rounded-full shrink-0" style={{ backgroundColor: COLORS[i % COLORS.length] }} />
                    <span className="text-sm font-mono flex-1 truncate">{row.model}</span>
                    <div className="w-24 h-1.5 bg-cs-border/30 rounded-full overflow-hidden">
                      <div
                        className="h-full rounded-full"
                        style={{ width: `${(row.total_cost / maxCost) * 100}%`, backgroundColor: COLORS[i % COLORS.length] }}
                      />
                    </div>
                    <span className="text-sm font-medium w-16 text-right">{formatCost(row.total_cost)}</span>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* Cost by Provider */}
        {byProvider.length > 0 && (
          <div className="card p-4">
            <h3 className="text-sm font-medium mb-3">Cost by Provider</h3>
            <ResponsiveContainer width="100%" height={200}>
              <PieChart>
                <Pie
                  data={byProvider.map((p: any) => ({ name: p.provider, value: p.total_cost }))}
                  cx="50%"
                  cy="50%"
                  innerRadius={50}
                  outerRadius={80}
                  dataKey="value"
                  label={({ name, value }) => `${name} ${formatCost(value)}`}
                >
                  {byProvider.map((_: any, i: number) => (
                    <Cell key={i} fill={COLORS[i % COLORS.length]} />
                  ))}
                </Pie>
                <Tooltip formatter={(value: number) => formatCost(value)} />
              </PieChart>
            </ResponsiveContainer>
          </div>
        )}
      </div>

      {/* Model Details Table */}
      {byModel.length > 0 && (
        <div className="card p-4">
          <h3 className="text-sm font-medium mb-3">Model Details</h3>
          <table className="w-full text-sm">
            <thead>
              <tr className="text-cs-muted text-xs uppercase tracking-wide border-b border-cs-border">
                <th className="text-left py-2">Model</th>
                <th className="text-right py-2">Requests</th>
                <th className="text-right py-2">Input Tokens</th>
                <th className="text-right py-2">Output Tokens</th>
                <th className="text-right py-2">Avg Duration</th>
                <th className="text-right py-2">Cost</th>
              </tr>
            </thead>
            <tbody>
              {byModel.map((row: any) => (
                <tr key={row.model} className="border-b border-cs-border/30 hover:bg-cs-border/10">
                  <td className="py-2 font-mono">{row.model}</td>
                  <td className="py-2 text-right">{row.request_count}</td>
                  <td className="py-2 text-right">{formatTokens(parseInt(row.input_tokens))}</td>
                  <td className="py-2 text-right">{formatTokens(parseInt(row.output_tokens))}</td>
                  <td className="py-2 text-right">{row.avg_duration_ms ? `${row.avg_duration_ms}ms` : '—'}</td>
                  <td className="py-2 text-right font-medium">{formatCost(row.total_cost)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
