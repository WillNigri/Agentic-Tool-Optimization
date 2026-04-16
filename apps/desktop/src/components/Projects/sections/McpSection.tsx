import { useTranslation } from "react-i18next";
import { Server } from "lucide-react";
import type { ProjectMcpSummary } from "@/lib/api";
import SectionShell, { ScopeBadge, EmptyRow } from "./SectionShell";

interface McpSectionProps {
  servers: ProjectMcpSummary[];
  onCreateMcpJson?: () => void;
}

export default function McpSection({ servers, onCreateMcpJson }: McpSectionProps) {
  const { t } = useTranslation();
  return (
    <SectionShell
      icon={Server}
      title={t("projects.mcp", "MCP Servers")}
      subtitle={t("projects.mcpSubtitle", "From ~/.claude/settings.json and .mcp.json")}
      count={servers.length}
    >
      {servers.length === 0 ? (
        <EmptyRow
          message={t("projects.mcpEmpty", "No MCP servers configured. MCP servers extend your agent with external tools.")}
          actionLabel={onCreateMcpJson ? t("projects.mcpCreate", "Create .mcp.json") : undefined}
          onAction={onCreateMcpJson}
        />
      ) : (
        <ul className="space-y-1.5">
          {servers.map((s, i) => (
            <li
              key={`${s.scope}-${s.name}-${i}`}
              className="rounded-md border border-cs-border/60 bg-cs-bg/40 px-3 py-2"
            >
              <div className="mb-1 flex items-center gap-2">
                <span className="text-sm font-medium">{s.name}</span>
                <KindBadge kind={s.kind} />
                <ScopeBadge scope={s.scope} />
              </div>
              <p className="truncate font-mono text-[11px] text-cs-muted">{s.commandOrUrl}</p>
            </li>
          ))}
        </ul>
      )}
    </SectionShell>
  );
}

function KindBadge({ kind }: { kind: string }) {
  const colors: Record<string, string> = {
    stdio: "bg-cs-accent/10 text-cs-accent",
    http: "bg-blue-500/10 text-blue-300",
    sse: "bg-purple-500/10 text-purple-300",
    unknown: "bg-cs-border text-cs-muted",
  };
  return (
    <span className={`rounded px-1.5 py-0.5 text-[10px] font-medium uppercase ${colors[kind] ?? colors.unknown}`}>
      {kind}
    </span>
  );
}
