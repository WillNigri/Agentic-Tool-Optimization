import { useQuery } from "@tanstack/react-query";
import { Clock, FileEdit, Loader2 } from "lucide-react";
import { getAuditLogs, type AuditLogEntry } from "@/lib/api";
import SectionShell, { EmptyRow } from "./sections/SectionShell";

interface ProjectActivityFeedProps {
  projectPath: string;
}

interface FileWriteDetails {
  path?: string;
  oldHash?: string;
  newHash?: string;
  addedLines?: number;
  removedLines?: number;
  bytesWritten?: number;
  backupPath?: string | null;
}

function parseDetails(raw?: string): FileWriteDetails {
  if (!raw) return {};
  try {
    return JSON.parse(raw);
  } catch {
    return {};
  }
}

function relativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  const now = Date.now();
  const secs = Math.max(0, Math.floor((now - then) / 1000));
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(iso).toLocaleDateString();
}

export default function ProjectActivityFeed({ projectPath }: ProjectActivityFeedProps) {
  const { data: logs = [], isLoading } = useQuery({
    queryKey: ["audit-logs-file-write", projectPath],
    queryFn: () => getAuditLogs({ action: "file_write", limit: 100 }),
    refetchInterval: 10_000,
    staleTime: 5_000,
  });

  // Filter to entries that touched files inside this project.
  const projectLogs = logs.filter((log) => {
    if (!log.resourceId) return false;
    if (log.resourceId.startsWith(projectPath)) return true;
    // Global config edits also affect the project — include ~/.claude/ entries.
    const details = parseDetails(log.details);
    const path = details.path ?? log.resourceId;
    return path.includes("/.claude/") || path.includes("/.codex/") ||
      path.includes("/.openclaw/") || path.includes("/.hermes/");
  }).slice(0, 15);

  return (
    <SectionShell
      icon={Clock}
      title="Recent edits"
      subtitle="Last 15 file writes (this project + inherited globals)"
      count={projectLogs.length}
    >
      {isLoading ? (
        <div className="flex items-center gap-2 py-4 text-xs text-cs-muted">
          <Loader2 size={12} className="animate-spin" /> Loading…
        </div>
      ) : projectLogs.length === 0 ? (
        <EmptyRow message="No edits yet. Changes you save will appear here." />
      ) : (
        <ul className="space-y-1.5">
          {projectLogs.map((log) => (
            <ActivityRow key={log.id} log={log} projectPath={projectPath} />
          ))}
        </ul>
      )}
    </SectionShell>
  );
}

function ActivityRow({ log, projectPath }: { log: AuditLogEntry; projectPath: string }) {
  const details = parseDetails(log.details);
  const fullPath = details.path ?? log.resourceId ?? "";
  const displayPath = fullPath.startsWith(projectPath)
    ? "." + fullPath.slice(projectPath.length)
    : fullPath.replace(/^\/Users\/[^/]+/, "~");
  const filename = log.resourceName ?? fullPath.split("/").pop();

  return (
    <li className="flex items-start gap-3 rounded-md border border-cs-border/60 bg-cs-bg/40 px-3 py-2">
      <FileEdit size={13} className="mt-0.5 shrink-0 text-cs-accent" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-xs">
          <span className="truncate font-medium">{filename}</span>
          {(details.addedLines !== undefined || details.removedLines !== undefined) && (
            <span className="shrink-0 font-mono text-[10px]">
              <span className="text-green-400">+{details.addedLines ?? 0}</span>
              <span className="mx-1 text-cs-muted">/</span>
              <span className="text-red-400">−{details.removedLines ?? 0}</span>
            </span>
          )}
        </div>
        <p className="mt-0.5 truncate font-mono text-[10px] text-cs-muted">{displayPath}</p>
      </div>
      <span className="shrink-0 text-[10px] text-cs-muted whitespace-nowrap">
        {relativeTime(log.createdAt)}
      </span>
    </li>
  );
}
