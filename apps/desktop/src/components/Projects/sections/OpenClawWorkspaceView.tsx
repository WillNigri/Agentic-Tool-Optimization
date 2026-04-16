import { useQuery } from "@tanstack/react-query";
import { User, Wrench, Loader2, ExternalLink } from "lucide-react";
import { parseOpenclawWorkspace } from "@/lib/api";
import SectionShell from "./SectionShell";
import { cn } from "@/lib/utils";

interface OpenClawWorkspaceViewProps {
  projectPath: string;
  onOpenFile: (path: string) => void;
}

export default function OpenClawWorkspaceView({ projectPath, onOpenFile }: OpenClawWorkspaceViewProps) {
  const { data, isLoading } = useQuery({
    queryKey: ["openclaw-workspace", projectPath],
    queryFn: () => parseOpenclawWorkspace(projectPath),
    staleTime: 30_000,
  });

  if (isLoading) {
    return <div className="flex items-center gap-2 py-4 text-xs text-cs-muted"><Loader2 size={12} className="animate-spin" /> Loading workspace…</div>;
  }
  if (!data) return null;

  return (
    <div className="space-y-4">
      {/* Soul card */}
      <SectionShell
        icon={User}
        title={data.soul.name ?? "Agent Soul"}
        subtitle="SOUL.md — agent persona and personality"
        actions={
          <button onClick={() => onOpenFile(projectPath + "/SOUL.md")} className="text-[10px] text-cs-muted hover:text-cs-accent flex items-center gap-1">
            <ExternalLink size={10} /> Edit
          </button>
        }
      >
        <div className="space-y-2">
          {data.soul.role && (
            <div className="flex items-center gap-2">
              <span className="text-[10px] text-cs-muted uppercase tracking-wide w-12 shrink-0">Role</span>
              <span className="text-sm">{data.soul.role}</span>
            </div>
          )}
          {data.soul.traits.length > 0 && (
            <div className="flex items-start gap-2">
              <span className="text-[10px] text-cs-muted uppercase tracking-wide w-12 shrink-0 pt-0.5">Traits</span>
              <div className="flex flex-wrap gap-1">
                {data.soul.traits.map((t, i) => (
                  <span key={i} className="rounded-full bg-purple-500/10 border border-purple-500/20 px-2 py-0.5 text-[11px] text-purple-300">{t}</span>
                ))}
              </div>
            </div>
          )}
        </div>
      </SectionShell>

      {/* Tools grid */}
      {data.tools.length > 0 && (
        <SectionShell
          icon={Wrench}
          title="Tools"
          subtitle="TOOLS.md — available tools for this agent"
          count={data.tools.length}
          actions={
            <button onClick={() => onOpenFile(projectPath + "/TOOLS.md")} className="text-[10px] text-cs-muted hover:text-cs-accent flex items-center gap-1">
              <ExternalLink size={10} /> Edit
            </button>
          }
        >
          <div className="grid grid-cols-1 gap-2 md:grid-cols-2 lg:grid-cols-3">
            {data.tools.map((tool) => (
              <div key={tool.name} className="rounded-md border border-cs-border/60 bg-cs-bg/40 px-3 py-2">
                <div className="text-xs font-medium">{tool.name}</div>
                {tool.description && <p className="mt-0.5 line-clamp-2 text-[11px] text-cs-muted">{tool.description}</p>}
              </div>
            ))}
          </div>
        </SectionShell>
      )}
    </div>
  );
}
