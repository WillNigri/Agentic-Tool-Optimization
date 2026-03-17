import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { getMcpServers, restartMcpServer, type McpServer } from "@/lib/api";
import { cn } from "@/lib/utils";

const STATUS_COLORS: Record<McpServer["status"], string> = {
  running: "bg-cs-success",
  stopped: "bg-cs-muted",
  error: "bg-cs-danger",
};

export default function McpDashboard() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const { data: servers = [], isLoading } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: getMcpServers,
  });

  const restart = useMutation({
    mutationFn: restartMcpServer,
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ["mcp-servers"] }),
  });

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
              className="card flex items-center justify-between gap-4"
            >
              <div className="flex items-center gap-3 min-w-0 flex-1">
                {/* Status dot */}
                <div
                  className={cn(
                    "w-2.5 h-2.5 rounded-full shrink-0",
                    STATUS_COLORS[server.status]
                  )}
                />
                <div className="min-w-0">
                  <p className="text-sm font-medium truncate">{server.name}</p>
                  <div className="flex items-center gap-3 text-xs text-cs-muted">
                    <span>{t(`mcp.status.${server.status === 'running' ? 'connected' : server.status === 'stopped' ? 'disconnected' : 'error'}`)}</span>
                    <span className="text-cs-border">|</span>
                    <span>{server.transport}</span>
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

              <button
                onClick={() => restart.mutate(server.id)}
                disabled={restart.isPending}
                className="btn-secondary flex items-center gap-1.5 text-xs py-1.5 px-3 shrink-0"
                title="Restart server"
              >
                <RefreshCw
                  size={14}
                  className={cn(restart.isPending && "animate-spin")}
                />
                {t('mcp.restart')}
              </button>
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
