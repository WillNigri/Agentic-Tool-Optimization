import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Shield,
  Search,
  Filter,
  Trash2,
  Download,
  Clock,
  Activity,
  ChevronDown,
  FileText,
  Key,
  Settings,
  Users,
  Zap,
  AlertTriangle,
} from "lucide-react";
import { getAuditLogs, getAuditLogStats, clearAuditLogs, type AuditLogEntry } from "@/lib/tauri-api";
import { cn } from "@/lib/utils";

const ACTION_ICONS: Record<string, any> = {
  "skill.create": Zap,
  "skill.update": FileText,
  "skill.delete": Trash2,
  "skill.toggle": Activity,
  "secret.create": Key,
  "secret.delete": Key,
  "config.update": Settings,
  "project.create": FileText,
  "project.delete": Trash2,
  "auth.login": Users,
  "auth.logout": Users,
  "cron.create": Clock,
  "cron.trigger": Zap,
  "notification.send": AlertTriangle,
};

const ACTION_COLORS: Record<string, string> = {
  create: "text-emerald-400",
  update: "text-blue-400",
  delete: "text-red-400",
  toggle: "text-yellow-400",
  login: "text-cyan-400",
  logout: "text-gray-400",
  trigger: "text-purple-400",
  send: "text-orange-400",
};

function getActionColor(action: string): string {
  const verb = action.split(".")[1] || action;
  return ACTION_COLORS[verb] || "text-cs-muted";
}

function getActionIcon(action: string) {
  return ACTION_ICONS[action] || Activity;
}

function formatTimeAgo(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const seconds = Math.floor((now.getTime() - date.getTime()) / 1000);

  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  if (seconds < 604800) return `${Math.floor(seconds / 86400)}d ago`;
  return date.toLocaleDateString();
}

export default function AuditLog() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [actionFilter, setActionFilter] = useState<string>("");
  const [resourceFilter, setResourceFilter] = useState<string>("");
  const [searchQuery, setSearchQuery] = useState("");
  const [showFilters, setShowFilters] = useState(false);
  const [page, setPage] = useState(0);
  const pageSize = 50;

  const { data: logs = [], isLoading } = useQuery({
    queryKey: ["audit-logs", actionFilter, resourceFilter, page],
    queryFn: () =>
      getAuditLogs({
        action: actionFilter || undefined,
        resourceType: resourceFilter || undefined,
        limit: pageSize,
        offset: page * pageSize,
      }),
    refetchInterval: 10000,
  });

  const { data: stats } = useQuery({
    queryKey: ["audit-log-stats"],
    queryFn: getAuditLogStats,
    refetchInterval: 30000,
  });

  const clearMutation = useMutation({
    mutationFn: (beforeDate?: string) => clearAuditLogs(beforeDate),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["audit-logs"] });
      queryClient.invalidateQueries({ queryKey: ["audit-log-stats"] });
    },
  });

  const filteredLogs = searchQuery
    ? logs.filter(
        (log) =>
          log.action.toLowerCase().includes(searchQuery.toLowerCase()) ||
          log.resourceName?.toLowerCase().includes(searchQuery.toLowerCase()) ||
          log.details?.toLowerCase().includes(searchQuery.toLowerCase())
      )
    : logs;

  const exportLogs = () => {
    const json = JSON.stringify(filteredLogs, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `ato-audit-logs-${new Date().toISOString().split("T")[0]}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  if (isLoading) {
    return (
      <div className="space-y-6 animate-pulse">
        <div className="h-8 bg-cs-border/30 rounded w-48" />
        {[1, 2, 3, 4, 5].map((i) => (
          <div key={i} className="card h-16" />
        ))}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <Shield className="w-5 h-5 text-cs-accent" />
            Audit Log
          </h2>
          <p className="text-cs-muted text-sm">
            Track all actions across your agentic systems
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={exportLogs}
            className="flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-md bg-cs-border/50 hover:bg-cs-border transition-colors"
          >
            <Download className="w-3.5 h-3.5" />
            Export
          </button>
          <button
            onClick={() => clearMutation.mutate(undefined)}
            className="flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-md bg-red-500/10 text-red-400 hover:bg-red-500/20 transition-colors"
          >
            <Trash2 className="w-3.5 h-3.5" />
            Clear All
          </button>
        </div>
      </div>

      {/* Stats Cards */}
      {stats && (
        <div className="grid grid-cols-3 gap-4">
          <div className="card p-4">
            <div className="text-cs-muted text-xs uppercase tracking-wide mb-1">Total Events</div>
            <div className="text-2xl font-bold">{stats.total.toLocaleString()}</div>
          </div>
          <div className="card p-4">
            <div className="text-cs-muted text-xs uppercase tracking-wide mb-1">Today</div>
            <div className="text-2xl font-bold text-cs-accent">{stats.today.toLocaleString()}</div>
          </div>
          <div className="card p-4">
            <div className="text-cs-muted text-xs uppercase tracking-wide mb-1">This Week</div>
            <div className="text-2xl font-bold">{stats.thisWeek.toLocaleString()}</div>
          </div>
        </div>
      )}

      {/* Search & Filters */}
      <div className="flex items-center gap-3">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-cs-muted" />
          <input
            type="text"
            placeholder="Search audit logs..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full pl-9 pr-3 py-2 bg-cs-border/30 border border-cs-border rounded-md text-sm focus:outline-none focus:border-cs-accent/50"
          />
        </div>
        <button
          onClick={() => setShowFilters(!showFilters)}
          className={cn(
            "flex items-center gap-1.5 px-3 py-2 text-sm rounded-md border transition-colors",
            showFilters
              ? "border-cs-accent/50 text-cs-accent bg-cs-accent/10"
              : "border-cs-border bg-cs-border/30 hover:bg-cs-border/50"
          )}
        >
          <Filter className="w-3.5 h-3.5" />
          Filters
          <ChevronDown className={cn("w-3.5 h-3.5 transition-transform", showFilters && "rotate-180")} />
        </button>
      </div>

      {showFilters && (
        <div className="card p-4 flex gap-4">
          <div className="flex-1">
            <label className="text-xs text-cs-muted uppercase tracking-wide mb-1 block">Action</label>
            <select
              value={actionFilter}
              onChange={(e) => { setActionFilter(e.target.value); setPage(0); }}
              className="w-full px-3 py-1.5 bg-cs-border/30 border border-cs-border rounded-md text-sm"
            >
              <option value="">All Actions</option>
              <option value="skill.create">Skill Created</option>
              <option value="skill.update">Skill Updated</option>
              <option value="skill.delete">Skill Deleted</option>
              <option value="skill.toggle">Skill Toggled</option>
              <option value="secret.create">Secret Created</option>
              <option value="secret.delete">Secret Deleted</option>
              <option value="config.update">Config Updated</option>
              <option value="cron.create">Cron Created</option>
              <option value="cron.trigger">Cron Triggered</option>
              <option value="notification.send">Notification Sent</option>
            </select>
          </div>
          <div className="flex-1">
            <label className="text-xs text-cs-muted uppercase tracking-wide mb-1 block">Resource Type</label>
            <select
              value={resourceFilter}
              onChange={(e) => { setResourceFilter(e.target.value); setPage(0); }}
              className="w-full px-3 py-1.5 bg-cs-border/30 border border-cs-border rounded-md text-sm"
            >
              <option value="">All Resources</option>
              <option value="skill">Skills</option>
              <option value="secret">Secrets</option>
              <option value="config">Configurations</option>
              <option value="cron">Cron Jobs</option>
              <option value="project">Projects</option>
              <option value="notification">Notifications</option>
              <option value="api_key">API Keys</option>
            </select>
          </div>
        </div>
      )}

      {/* Log Entries */}
      <div className="space-y-1">
        {filteredLogs.length === 0 ? (
          <div className="card text-center py-12">
            <Shield className="w-8 h-8 text-cs-muted mx-auto mb-3" />
            <p className="text-cs-muted">No audit log entries found</p>
            <p className="text-cs-muted text-xs mt-1">Actions will appear here as you use ATO</p>
          </div>
        ) : (
          filteredLogs.map((log) => {
            const Icon = getActionIcon(log.action);
            return (
              <div
                key={log.id}
                className="card px-4 py-3 flex items-center gap-3 hover:border-cs-accent/20 transition-colors"
              >
                <div className={cn("p-1.5 rounded-md bg-cs-border/30", getActionColor(log.action))}>
                  <Icon className="w-3.5 h-3.5" />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className={cn("text-sm font-medium", getActionColor(log.action))}>
                      {log.action}
                    </span>
                    {log.resourceName && (
                      <span className="text-sm text-cs-muted truncate">
                        — {log.resourceName}
                      </span>
                    )}
                  </div>
                  {log.details && (
                    <p className="text-xs text-cs-muted mt-0.5 truncate">{log.details}</p>
                  )}
                </div>
                <div className="text-xs text-cs-muted whitespace-nowrap flex items-center gap-1">
                  <Clock className="w-3 h-3" />
                  {formatTimeAgo(log.createdAt)}
                </div>
              </div>
            );
          })
        )}
      </div>

      {/* Pagination */}
      {filteredLogs.length >= pageSize && (
        <div className="flex items-center justify-center gap-4">
          <button
            onClick={() => setPage(Math.max(0, page - 1))}
            disabled={page === 0}
            className="px-3 py-1.5 text-sm rounded-md bg-cs-border/30 hover:bg-cs-border disabled:opacity-30 transition-colors"
          >
            Previous
          </button>
          <span className="text-sm text-cs-muted">Page {page + 1}</span>
          <button
            onClick={() => setPage(page + 1)}
            className="px-3 py-1.5 text-sm rounded-md bg-cs-border/30 hover:bg-cs-border transition-colors"
          >
            Next
          </button>
        </div>
      )}
    </div>
  );
}
