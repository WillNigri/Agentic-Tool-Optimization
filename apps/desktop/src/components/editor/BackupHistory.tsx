import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { History, RotateCcw, Loader2, ChevronDown, ChevronRight, Check } from "lucide-react";
import { cn } from "@/lib/utils";
import { listBackups, restoreBackup, type BackupEntry } from "@/lib/api";

interface BackupHistoryProps {
  filePath: string;
  currentHash?: string;
  onRestored?: () => void;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

function formatTimestamp(unix: number): string {
  const date = new Date(unix * 1000);
  const now = Date.now();
  const diffSecs = Math.floor((now - date.getTime()) / 1000);
  if (diffSecs < 60) return `${diffSecs}s ago`;
  if (diffSecs < 3600) return `${Math.floor(diffSecs / 60)}m ago`;
  if (diffSecs < 86400) return `${Math.floor(diffSecs / 3600)}h ago`;
  return date.toLocaleString();
}

export default function BackupHistory({ filePath, currentHash, onRestored }: BackupHistoryProps) {
  const [expanded, setExpanded] = useState(false);
  const [confirmId, setConfirmId] = useState<string | null>(null);
  const queryClient = useQueryClient();

  const { data: backups = [], isLoading } = useQuery({
    queryKey: ["backups", filePath],
    queryFn: () => listBackups(filePath),
    enabled: expanded,
    staleTime: 10_000,
  });

  const restoreMutation = useMutation({
    mutationFn: (backup: BackupEntry) => restoreBackup(backup.backupPath, filePath, currentHash),
    onSuccess: () => {
      setConfirmId(null);
      queryClient.invalidateQueries({ queryKey: ["config-file", filePath] });
      queryClient.invalidateQueries({ queryKey: ["backups", filePath] });
      queryClient.invalidateQueries({ queryKey: ["audit-logs-file-write"] });
      onRestored?.();
    },
  });

  return (
    <div className="mx-4 mb-2 rounded-lg border border-cs-border/60 bg-cs-bg/30">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left text-xs text-cs-muted transition-colors hover:text-cs-text"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <History size={12} />
        <span className="flex-1">Backup history</span>
        {expanded && backups.length > 0 && (
          <span className="text-[10px] text-cs-muted">{backups.length} backup{backups.length === 1 ? "" : "s"}</span>
        )}
      </button>

      {expanded && (
        <div className="border-t border-cs-border/60 px-2 py-2">
          {isLoading ? (
            <div className="flex items-center gap-2 px-1 py-2 text-[11px] text-cs-muted">
              <Loader2 size={11} className="animate-spin" /> Loading backups…
            </div>
          ) : backups.length === 0 ? (
            <p className="px-1 py-2 text-[11px] text-cs-muted">
              No backups yet. Each save automatically creates one in ~/.ato/backups/.
            </p>
          ) : (
            <ul className="space-y-1">
              {backups.map((backup) => {
                const confirming = confirmId === backup.backupPath;
                return (
                  <li
                    key={backup.backupPath}
                    className="flex items-center gap-2 rounded-md border border-cs-border/60 bg-cs-card px-2 py-1.5"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2 text-[11px]">
                        <span className="font-mono">{backup.sha8}</span>
                        <span className="text-cs-muted">{formatTimestamp(backup.timestamp)}</span>
                        <span className="text-cs-muted">·</span>
                        <span className="text-cs-muted">{formatSize(backup.sizeBytes)}</span>
                      </div>
                    </div>
                    {confirming ? (
                      <div className="flex items-center gap-1">
                        <button
                          onClick={() => restoreMutation.mutate(backup)}
                          disabled={restoreMutation.isPending}
                          className={cn(
                            "flex items-center gap-1 rounded px-2 py-0.5 text-[10px] font-medium",
                            restoreMutation.isPending
                              ? "bg-cs-border text-cs-muted"
                              : "bg-cs-accent text-cs-bg hover:bg-cs-accent/90"
                          )}
                        >
                          {restoreMutation.isPending ? (
                            <Loader2 size={10} className="animate-spin" />
                          ) : (
                            <Check size={10} />
                          )}
                          Confirm
                        </button>
                        <button
                          onClick={() => setConfirmId(null)}
                          className="rounded px-2 py-0.5 text-[10px] text-cs-muted hover:bg-cs-border"
                        >
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <button
                        onClick={() => setConfirmId(backup.backupPath)}
                        className="flex items-center gap-1 rounded px-2 py-0.5 text-[10px] text-cs-muted hover:bg-cs-border hover:text-cs-text"
                      >
                        <RotateCcw size={10} /> Restore
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
          {restoreMutation.isError && (
            <p className="mt-2 rounded border border-red-500/30 bg-red-500/10 px-2 py-1 text-[10px] text-red-300">
              {restoreMutation.error instanceof Error
                ? restoreMutation.error.message
                : String(restoreMutation.error)}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
