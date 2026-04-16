import { useMemo } from "react";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from "recharts";
import { BarChart3 } from "lucide-react";
import type { ProjectBundle } from "@/lib/api";
import SectionShell, { EmptyRow } from "./sections/SectionShell";

interface TokenBreakdownChartProps {
  bundle: ProjectBundle;
}

interface Slice {
  name: string;
  value: number;
  color: string;
}

const COLORS = {
  memory: "#FFB800",
  skills: "#00FFB2",
  subagents: "#A78BFA",
  commands: "#60A5FA",
  mcp: "#F472B6",
  permissions: "#FB923C",
};

function sumTokens(list: { tokenEstimate: number; exists: boolean }[]): number {
  return list.filter((f) => f.exists).reduce((s, f) => s + f.tokenEstimate, 0);
}

function sumSkillTokens(list: { tokenCount: number; enabled: boolean }[]): number {
  // Skills are loaded on-demand; count enabled ones as a ceiling.
  return list.filter((s) => s.enabled).reduce((s, x) => s + x.tokenCount, 0);
}

export default function TokenBreakdownChart({ bundle }: TokenBreakdownChartProps) {
  const slices = useMemo<Slice[]>(() => {
    const raw: Slice[] = [
      { name: "Memory", value: sumTokens(bundle.memoryFiles), color: COLORS.memory },
      { name: "Skills (enabled)", value: sumSkillTokens(bundle.skills), color: COLORS.skills },
      { name: "Subagents", value: sumTokens(bundle.subagents), color: COLORS.subagents },
      { name: "Commands", value: sumTokens(bundle.commands), color: COLORS.commands },
      { name: "MCP / Settings", value: sumTokens(bundle.settingsFiles), color: COLORS.mcp },
    ];
    return raw.filter((s) => s.value > 0);
  }, [bundle]);

  const total = slices.reduce((sum, s) => sum + s.value, 0);

  return (
    <SectionShell
      icon={BarChart3}
      title="Token breakdown"
      subtitle="Estimated tokens this project's config would consume when loaded"
      count={total > 0 ? `~${total.toLocaleString()} tok` : "—"}
    >
      {slices.length === 0 ? (
        <EmptyRow message="No measurable tokens yet. Add some CLAUDE.md content and skills to see breakdown." />
      ) : (
        <div className="grid gap-4 md:grid-cols-2">
          <div className="h-56">
            <ResponsiveContainer width="100%" height="100%">
              <PieChart>
                <Pie
                  data={slices}
                  dataKey="value"
                  innerRadius={50}
                  outerRadius={85}
                  paddingAngle={2}
                  stroke="#0a0a0f"
                  strokeWidth={2}
                >
                  {slices.map((s) => (
                    <Cell key={s.name} fill={s.color} />
                  ))}
                </Pie>
                <Tooltip
                  contentStyle={{
                    backgroundColor: "#0a0a0f",
                    border: "1px solid rgba(255,255,255,0.1)",
                    borderRadius: "8px",
                    fontSize: "11px",
                  }}
                  formatter={(value: number) => [`~${value.toLocaleString()} tok`, null]}
                />
              </PieChart>
            </ResponsiveContainer>
          </div>
          <ul className="space-y-1.5 self-center">
            {slices.map((s) => {
              const pct = total > 0 ? (s.value / total) * 100 : 0;
              return (
                <li key={s.name} className="flex items-center gap-3 text-xs">
                  <span
                    className="inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
                    style={{ background: s.color }}
                  />
                  <span className="flex-1 truncate">{s.name}</span>
                  <span className="shrink-0 font-mono text-cs-muted">
                    ~{s.value.toLocaleString()}
                  </span>
                  <span className="w-10 shrink-0 text-right text-[10px] text-cs-muted">
                    {pct.toFixed(0)}%
                  </span>
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </SectionShell>
  );
}
