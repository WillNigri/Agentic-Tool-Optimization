import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  BarChart3,
  TrendingUp,
  Clock,
  AlertTriangle,
  RefreshCw,
  Sparkles,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { getSkillUsageStats, type SkillUsageStat } from "@/lib/api";

export default function SkillUsagePanel() {
  const { t } = useTranslation();

  const { data: stats = [], isLoading, refetch } = useQuery({
    queryKey: ["skill-usage-stats"],
    queryFn: getSkillUsageStats,
  });

  const totalTriggers = stats.reduce((sum, s) => sum + s.triggerCount, 0);
  const neverUsed = stats.filter((s) => s.triggerCount === 0);
  const mostUsed = stats.filter((s) => s.triggerCount > 0).slice(0, 5);

  const formatDate = (dateStr?: string) => {
    if (!dateStr) return "Never";
    try {
      const date = new Date(dateStr);
      const now = new Date();
      const diffDays = Math.floor((now.getTime() - date.getTime()) / (1000 * 60 * 60 * 24));

      if (diffDays === 0) return "Today";
      if (diffDays === 1) return "Yesterday";
      if (diffDays < 7) return `${diffDays} days ago`;
      if (diffDays < 30) return `${Math.floor(diffDays / 7)} weeks ago`;
      return date.toLocaleDateString();
    } catch {
      return dateStr;
    }
  };

  return (
    <div className="h-full flex flex-col bg-cs-card">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border">
        <div className="flex items-center gap-2">
          <BarChart3 size={18} className="text-cs-accent" />
          <h3 className="font-medium">
            {t("agentManager.usage.title", "Skill Usage")}
          </h3>
        </div>
        <button
          onClick={() => refetch()}
          disabled={isLoading}
          className="p-1.5 rounded-md border border-cs-border hover:bg-cs-border/50 transition-colors"
          title={t("common.refresh", "Refresh")}
        >
          <RefreshCw size={14} className={isLoading ? "animate-spin" : ""} />
        </button>
      </div>

      {/* Summary cards */}
      <div className="grid grid-cols-3 gap-3 p-4 border-b border-cs-border">
        <div className="bg-cs-bg rounded-lg p-3">
          <div className="flex items-center gap-2 text-cs-muted mb-1">
            <TrendingUp size={12} />
            <span className="text-xs">Total Triggers</span>
          </div>
          <p className="text-lg font-semibold">{totalTriggers}</p>
        </div>
        <div className="bg-cs-bg rounded-lg p-3">
          <div className="flex items-center gap-2 text-cs-muted mb-1">
            <Sparkles size={12} />
            <span className="text-xs">Total Skills</span>
          </div>
          <p className="text-lg font-semibold">{stats.length}</p>
        </div>
        <div className="bg-cs-bg rounded-lg p-3">
          <div className="flex items-center gap-2 text-yellow-400 mb-1">
            <AlertTriangle size={12} />
            <span className="text-xs">Never Used</span>
          </div>
          <p className="text-lg font-semibold">{neverUsed.length}</p>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto">
        {/* Most used */}
        {mostUsed.length > 0 && (
          <div className="p-4 border-b border-cs-border">
            <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wide mb-3">
              Most Used Skills
            </h4>
            <div className="space-y-2">
              {mostUsed.map((stat) => (
                <div
                  key={stat.skillPath}
                  className="flex items-center justify-between p-2 rounded-md bg-cs-bg"
                >
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium truncate">{stat.skillName}</p>
                    <p className="text-xs text-cs-muted">
                      Last used: {formatDate(stat.lastUsed)}
                    </p>
                  </div>
                  <div className="text-right ml-4">
                    <p className="text-sm font-semibold text-cs-accent">
                      {stat.triggerCount}
                    </p>
                    <p className="text-xs text-cs-muted">triggers</p>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Never used */}
        {neverUsed.length > 0 && (
          <div className="p-4">
            <h4 className="text-xs font-medium text-yellow-400 uppercase tracking-wide mb-3 flex items-center gap-2">
              <AlertTriangle size={12} />
              Never Used (Consider Removing)
            </h4>
            <div className="space-y-1">
              {neverUsed.map((stat) => (
                <div
                  key={stat.skillPath}
                  className="flex items-center justify-between px-2 py-1.5 rounded-md hover:bg-cs-border/30"
                >
                  <p className="text-sm text-cs-muted truncate">{stat.skillName}</p>
                  <Clock size={12} className="text-cs-muted shrink-0" />
                </div>
              ))}
            </div>
          </div>
        )}

        {stats.length === 0 && !isLoading && (
          <div className="flex flex-col items-center justify-center h-full text-cs-muted p-8">
            <BarChart3 size={32} className="mb-2 opacity-50" />
            <p className="text-sm">No skill usage data yet</p>
            <p className="text-xs mt-1">Usage will be tracked as you use skills</p>
          </div>
        )}
      </div>
    </div>
  );
}
