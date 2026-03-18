import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw, X, Server, Wrench, Terminal, Globe, ChevronRight, AlertCircle } from "lucide-react";
import { getMcpServers, restartMcpServer, type McpServer } from "@/lib/api";
import { cn } from "@/lib/utils";

const STATUS_COLORS: Record<McpServer["status"], string> = {
  running: "bg-cs-success",
  stopped: "bg-cs-muted",
  error: "bg-cs-danger",
};

// Mock tool details for each MCP server
const MOCK_SERVER_TOOLS: Record<string, { name: string; description: string }[]> = {
  "1": [
    { name: "read_file", description: "Read contents of a file" },
    { name: "write_file", description: "Write content to a file" },
    { name: "list_directory", description: "List directory contents" },
    { name: "create_directory", description: "Create a new directory" },
    { name: "move_file", description: "Move or rename a file" },
    { name: "delete_file", description: "Delete a file" },
    { name: "get_file_info", description: "Get file metadata" },
    { name: "search_files", description: "Search for files by pattern" },
    { name: "read_multiple_files", description: "Read multiple files at once" },
    { name: "edit_file", description: "Apply edits to a file" },
    { name: "copy_file", description: "Copy a file" },
    { name: "get_directory_tree", description: "Get recursive directory tree" },
  ],
  "2": [
    { name: "create_issue", description: "Create a GitHub issue" },
    { name: "list_issues", description: "List repository issues" },
    { name: "create_pull_request", description: "Create a pull request" },
    { name: "get_pull_request", description: "Get PR details" },
    { name: "list_commits", description: "List recent commits" },
    { name: "search_code", description: "Search code in repository" },
    { name: "get_file_contents", description: "Get file from GitHub" },
    { name: "create_branch", description: "Create a new branch" },
  ],
  "3": [],
  "4": [
    { name: "query", description: "Execute SQL query" },
    { name: "list_tables", description: "List database tables" },
    { name: "describe_table", description: "Get table schema" },
    { name: "insert_row", description: "Insert a row" },
    { name: "update_rows", description: "Update rows" },
    { name: "delete_rows", description: "Delete rows" },
  ],
};

const MOCK_SERVER_DETAILS: Record<string, { env: Record<string, string>; configPath: string; permissions: string[] }> = {
  "1": { env: { "MCP_FS_ROOT": "/Users/dev/projects" }, configPath: "~/.claude/settings.json", permissions: ["read", "write", "list"] },
  "2": { env: { "GITHUB_TOKEN": "ghp_***...***" }, configPath: "~/.claude/settings.json", permissions: ["read", "write"] },
  "3": { env: { "SLACK_WEBHOOK": "https://hooks.slack.com/..." }, configPath: "~/.claude/settings.json", permissions: ["read"] },
  "4": { env: { "DATABASE_URL": "postgresql://localhost:5432/dev" }, configPath: ".claude/settings.json", permissions: ["read", "write"] },
};

export default function McpDashboard() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const { data: servers = [], isLoading } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: getMcpServers,
  });

  const restart = useMutation({
    mutationFn: restartMcpServer,
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ["mcp-servers"] }),
  });

  const selectedServer = servers.find((s) => s.id === selectedId);
  const selectedTools = selectedId ? MOCK_SERVER_TOOLS[selectedId] || [] : [];
  const selectedDetails = selectedId ? MOCK_SERVER_DETAILS[selectedId] : null;

  if (isLoading) {
    return <LoadingSkeleton />;
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-semibold mb-1">{t('mcp.title')}</h2>
        <p className="text-cs-muted text-sm">
          {t('mcp.subtitle')}
        </p>
      </div>

      {/* Status overview */}
      <div className="grid grid-cols-3 gap-3">
        <div className="card text-center">
          <p className="text-2xl font-semibold text-cs-accent">{servers.filter(s => s.status === "running").length}</p>
          <p className="text-xs text-cs-muted">{t('mcp.status.connected')}</p>
        </div>
        <div className="card text-center">
          <p className="text-2xl font-semibold text-cs-muted">{servers.filter(s => s.status === "stopped").length}</p>
          <p className="text-xs text-cs-muted">{t('mcp.status.disconnected')}</p>
        </div>
        <div className="card text-center">
          <p className="text-2xl font-semibold text-cs-danger">{servers.filter(s => s.status === "error").length}</p>
          <p className="text-xs text-cs-muted">{t('mcp.status.error')}</p>
        </div>
      </div>

      {servers.length === 0 ? (
        <div className="card text-center py-12">
          <p className="text-cs-muted text-sm">
            {t('mcp.noServers')}
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {servers.map((server) => (
            <div
              key={server.id}
              onClick={() => setSelectedId(selectedId === server.id ? null : server.id)}
              className={cn(
                "card cursor-pointer transition-colors",
                selectedId === server.id
                  ? "border-cs-accent/50 bg-cs-accent/5"
                  : "hover:border-cs-border/80"
              )}
            >
              <div className="flex items-center justify-between gap-4">
                <div className="flex items-center gap-3 min-w-0 flex-1">
                  <div
                    className={cn(
                      "w-2.5 h-2.5 rounded-full shrink-0",
                      STATUS_COLORS[server.status]
                    )}
                  />
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <p className="text-sm font-medium truncate">{server.name}</p>
                      <span className="text-[10px] font-mono uppercase px-1.5 py-0.5 rounded bg-cs-border/50 text-cs-muted">
                        {server.transport}
                      </span>
                    </div>
                    <div className="flex items-center gap-3 text-xs text-cs-muted">
                      <span>{t(`mcp.status.${server.status === 'running' ? 'connected' : server.status === 'stopped' ? 'disconnected' : 'error'}`)}</span>
                      <span className="text-cs-border">|</span>
                      <span>
                        {t('mcp.tools', { count: server.toolCount })}
                      </span>
                      {server.url && (
                        <>
                          <span className="text-cs-border">|</span>
                          <span className="truncate">{server.url}</span>
                        </>
                      )}
                    </div>
                  </div>
                </div>

                <div className="flex items-center gap-2 shrink-0">
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      restart.mutate(server.id);
                    }}
                    disabled={restart.isPending}
                    className="btn-secondary flex items-center gap-1.5 text-xs py-1.5 px-3"
                    title="Restart server"
                  >
                    <RefreshCw
                      size={14}
                      className={cn(restart.isPending && "animate-spin")}
                    />
                    {t('mcp.restart')}
                  </button>
                  <ChevronRight
                    size={16}
                    className={cn(
                      "text-cs-muted transition-transform",
                      selectedId === server.id && "rotate-90"
                    )}
                  />
                </div>
              </div>

              {/* Expanded detail */}
              {selectedId === server.id && selectedDetails && (
                <div className="mt-4 pt-4 border-t border-cs-border space-y-4">
                  {/* Connection info */}
                  <div>
                    <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-2">
                      {t('mcp.connection')}
                    </h4>
                    <div className="grid grid-cols-2 gap-2">
                      <div className="bg-cs-bg rounded-lg p-2.5">
                        <p className="text-[10px] text-cs-muted uppercase">Transport</p>
                        <p className="text-sm font-mono">{server.transport}</p>
                      </div>
                      <div className="bg-cs-bg rounded-lg p-2.5">
                        <p className="text-[10px] text-cs-muted uppercase">{t('mcp.configSource')}</p>
                        <p className="text-sm font-mono truncate">{selectedDetails.configPath}</p>
                      </div>
                    </div>
                    {server.command && (
                      <div className="bg-cs-bg rounded-lg p-2.5 mt-2">
                        <p className="text-[10px] text-cs-muted uppercase">{t('mcp.command')}</p>
                        <p className="text-sm font-mono text-cs-accent">{server.command}</p>
                      </div>
                    )}
                  </div>

                  {/* Environment variables */}
                  <div>
                    <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-2">
                      {t('mcp.environment')}
                    </h4>
                    <div className="space-y-1">
                      {Object.entries(selectedDetails.env).map(([key, val]) => (
                        <div key={key} className="bg-cs-bg rounded-lg px-2.5 py-2 flex items-center gap-2">
                          <span className="text-xs font-mono text-cs-accent">{key}</span>
                          <span className="text-xs text-cs-muted">=</span>
                          <span className="text-xs font-mono text-cs-muted truncate">{val}</span>
                        </div>
                      ))}
                    </div>
                  </div>

                  {/* Permissions */}
                  <div>
                    <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-2">
                      {t('mcp.permissions')}
                    </h4>
                    <div className="flex gap-2">
                      {selectedDetails.permissions.map((perm) => (
                        <span
                          key={perm}
                          className="px-2 py-0.5 text-xs font-mono rounded-full border border-cs-accent/40 bg-cs-accent/10 text-cs-accent"
                        >
                          {perm}
                        </span>
                      ))}
                    </div>
                  </div>

                  {/* Tools */}
                  {selectedTools.length > 0 && (
                    <div>
                      <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-2">
                        {t('mcp.tools', { count: selectedTools.length })}
                      </h4>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
                        {selectedTools.map((tool) => (
                          <div key={tool.name} className="bg-cs-bg rounded-lg px-2.5 py-2 flex items-start gap-2">
                            <Wrench size={12} className="text-cs-muted mt-0.5 shrink-0" />
                            <div className="min-w-0">
                              <p className="text-xs font-mono font-medium">{tool.name}</p>
                              <p className="text-[10px] text-cs-muted truncate">{tool.description}</p>
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* Error state */}
                  {server.status === "error" && (
                    <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/20">
                      <AlertCircle size={14} className="text-red-400 shrink-0" />
                      <p className="text-xs text-red-400">
                        Connection failed. Check server command and configuration.
                      </p>
                    </div>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="space-y-6 animate-pulse">
      <div>
        <div className="h-6 w-32 bg-cs-border rounded" />
        <div className="h-4 w-64 bg-cs-border rounded mt-2" />
      </div>
      {[1, 2, 3].map((i) => (
        <div key={i} className="card h-16" />
      ))}
    </div>
  );
}
