import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw, X, Server, Wrench, Terminal, Globe, ChevronRight, AlertCircle, Loader2, CheckCircle2, XCircle, Code } from "lucide-react";
import { getMcpServers, restartMcpServer, getMcpServersWithTools, discoverMcpServerTools, type McpServer, type McpServerDetails } from "@/lib/api";
import { cn } from "@/lib/utils";

const STATUS_COLORS: Record<McpServer["status"], string> = {
  running: "bg-cs-success",
  stopped: "bg-cs-muted",
  error: "bg-cs-danger",
};

export default function McpDashboard() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [expandedTools, setExpandedTools] = useState<string | null>(null);

  const { data: servers = [], isLoading } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: getMcpServers,
  });

  // Real-time tool discovery
  const { data: serversWithTools = [], isLoading: isDiscovering, refetch: refetchTools } = useQuery({
    queryKey: ["mcp-servers-with-tools"],
    queryFn: getMcpServersWithTools,
    staleTime: 60000, // Cache for 1 minute
    refetchOnWindowFocus: false,
  });

  // Create a map of server details by name
  const serverDetailsMap = new Map<string, McpServerDetails>(
    serversWithTools.map(s => [s.serverName, s])
  );

  const restart = useMutation({
    mutationFn: restartMcpServer,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["mcp-servers"] });
      queryClient.invalidateQueries({ queryKey: ["mcp-servers-with-tools"] });
    },
  });

  const selectedServer = servers.find((s) => s.id === selectedId);
  const getServerDetails = (serverName: string): McpServerDetails | undefined => {
    // Extract clean name without source suffix
    const cleanName = serverName.split(" (")[0];
    return serverDetailsMap.get(cleanName);
  };

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
      <div className="grid grid-cols-4 gap-3">
        <div className="card text-center">
          <p className="text-2xl font-semibold text-cs-accent">
            {isDiscovering ? "..." : serversWithTools.filter(s => s.connected).length}
          </p>
          <p className="text-xs text-cs-muted">{t('mcp.status.connected')}</p>
        </div>
        <div className="card text-center">
          <p className="text-2xl font-semibold text-cs-muted">
            {isDiscovering ? "..." : serversWithTools.filter(s => !s.connected).length}
          </p>
          <p className="text-xs text-cs-muted">{t('mcp.status.disconnected')}</p>
        </div>
        <div className="card text-center">
          <p className="text-2xl font-semibold text-purple-400">
            {isDiscovering ? "..." : serversWithTools.reduce((sum, s) => sum + s.tools.length, 0)}
          </p>
          <p className="text-xs text-cs-muted">Total Tools</p>
        </div>
        <div className="card text-center">
          <button
            onClick={() => refetchTools()}
            disabled={isDiscovering}
            className="w-full h-full flex flex-col items-center justify-center gap-1 hover:text-cs-accent transition-colors"
          >
            <RefreshCw size={20} className={cn(isDiscovering && "animate-spin")} />
            <p className="text-xs text-cs-muted">{isDiscovering ? "Discovering..." : "Refresh"}</p>
          </button>
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
          {servers.map((server) => {
            const details = getServerDetails(server.name);
            const toolCount = details?.tools.length ?? server.toolCount;
            const isConnected = details?.connected ?? false;

            return (
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
                      isConnected ? "bg-cs-success" : details?.error ? "bg-cs-danger" : "bg-cs-muted"
                    )}
                  />
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <p className="text-sm font-medium truncate">{server.name}</p>
                      <span className="text-[10px] font-mono uppercase px-1.5 py-0.5 rounded bg-cs-border/50 text-cs-muted">
                        {server.transport}
                      </span>
                      {details?.serverVersion && (
                        <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-purple-500/10 text-purple-400">
                          v{details.serverVersion}
                        </span>
                      )}
                    </div>
                    <div className="flex items-center gap-3 text-xs text-cs-muted">
                      {isConnected ? (
                        <span className="flex items-center gap-1 text-green-400">
                          <CheckCircle2 size={10} />
                          Connected
                        </span>
                      ) : details?.error ? (
                        <span className="flex items-center gap-1 text-red-400">
                          <XCircle size={10} />
                          Error
                        </span>
                      ) : isDiscovering ? (
                        <span className="flex items-center gap-1">
                          <Loader2 size={10} className="animate-spin" />
                          Discovering...
                        </span>
                      ) : (
                        <span>Not connected</span>
                      )}
                      <span className="text-cs-border">|</span>
                      <span className={cn(toolCount > 0 && "text-purple-400")}>
                        {t('mcp.tools', { count: toolCount })}
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
              {selectedId === server.id && (
                <div className="mt-4 pt-4 border-t border-cs-border space-y-4">
                  {/* Connection info */}
                  <div>
                    <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-2">
                      {t('mcp.connection')}
                    </h4>
                    <div className="grid grid-cols-2 lg:grid-cols-4 gap-2">
                      <div className="bg-cs-bg rounded-lg p-2.5">
                        <p className="text-[10px] text-cs-muted uppercase">Transport</p>
                        <p className="text-sm font-mono">{server.transport}</p>
                      </div>
                      <div className="bg-cs-bg rounded-lg p-2.5">
                        <p className="text-[10px] text-cs-muted uppercase">Status</p>
                        <p className={cn(
                          "text-sm font-mono",
                          isConnected ? "text-green-400" : "text-red-400"
                        )}>
                          {isConnected ? "Connected" : "Disconnected"}
                        </p>
                      </div>
                      {details?.protocolVersion && (
                        <div className="bg-cs-bg rounded-lg p-2.5">
                          <p className="text-[10px] text-cs-muted uppercase">Protocol</p>
                          <p className="text-sm font-mono">{details.protocolVersion}</p>
                        </div>
                      )}
                      <div className="bg-cs-bg rounded-lg p-2.5">
                        <p className="text-[10px] text-cs-muted uppercase">Tools</p>
                        <p className="text-sm font-mono text-purple-400">{toolCount}</p>
                      </div>
                    </div>
                    {server.command && (
                      <div className="bg-cs-bg rounded-lg p-2.5 mt-2">
                        <p className="text-[10px] text-cs-muted uppercase">{t('mcp.command')}</p>
                        <p className="text-sm font-mono text-cs-accent truncate">{server.command}</p>
                      </div>
                    )}
                  </div>

                  {/* Error state */}
                  {details?.error && (
                    <div className="flex items-start gap-2 px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/20">
                      <AlertCircle size={14} className="text-red-400 shrink-0 mt-0.5" />
                      <div>
                        <p className="text-xs font-medium text-red-400">Connection Error</p>
                        <p className="text-[10px] text-red-400/80 mt-0.5">{details.error}</p>
                      </div>
                    </div>
                  )}

                  {/* Discovered Tools */}
                  {details && details.tools.length > 0 && (
                    <div>
                      <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-2">
                        Discovered Tools ({details.tools.length})
                      </h4>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
                        {details.tools.map((tool) => (
                          <div
                            key={tool.name}
                            onClick={(e) => {
                              e.stopPropagation();
                              setExpandedTools(expandedTools === tool.name ? null : tool.name);
                            }}
                            className={cn(
                              "bg-cs-bg rounded-lg px-2.5 py-2 cursor-pointer transition-colors hover:bg-cs-bg/80",
                              expandedTools === tool.name && "ring-1 ring-purple-500/30"
                            )}
                          >
                            <div className="flex items-start gap-2">
                              <Wrench size={12} className="text-purple-400 mt-0.5 shrink-0" />
                              <div className="min-w-0 flex-1">
                                <p className="text-xs font-mono font-medium text-purple-400">{tool.name}</p>
                                <p className="text-[10px] text-cs-muted line-clamp-2">{tool.description || "No description"}</p>
                              </div>
                              {tool.inputSchema && (
                                <Code size={10} className="text-cs-muted shrink-0" />
                              )}
                            </div>
                            {/* Expanded tool schema */}
                            {expandedTools === tool.name && tool.inputSchema && (
                              <div className="mt-2 pt-2 border-t border-cs-border">
                                <p className="text-[10px] text-cs-muted uppercase mb-1">Input Schema</p>
                                <pre className="text-[10px] font-mono text-cs-muted bg-cs-card rounded p-2 overflow-x-auto max-h-32">
                                  {JSON.stringify(tool.inputSchema, null, 2)}
                                </pre>
                              </div>
                            )}
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* No tools discovered */}
                  {details && details.connected && details.tools.length === 0 && (
                    <div className="text-center py-4">
                      <p className="text-xs text-cs-muted">No tools exposed by this server.</p>
                    </div>
                  )}
                </div>
              )}
            </div>
          );
          })}
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
