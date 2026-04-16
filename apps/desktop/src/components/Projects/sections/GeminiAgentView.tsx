import { useQuery } from "@tanstack/react-query";
import { Bot, Cpu, ExternalLink, Loader2, Wrench } from "lucide-react";
import { parseGeminiAgent } from "@/lib/api";
import { cn } from "@/lib/utils";

interface GeminiAgentViewProps {
  agentPath: string;
  onOpenFile: (path: string) => void;
}

export default function GeminiAgentView({ agentPath, onOpenFile }: GeminiAgentViewProps) {
  const { data, isLoading, isError } = useQuery({
    queryKey: ["gemini-agent", agentPath],
    queryFn: () => parseGeminiAgent(agentPath),
    staleTime: 30_000,
  });

  if (isLoading) {
    return <div className="flex items-center gap-2 py-4 text-xs text-cs-muted"><Loader2 size={12} className="animate-spin" /> Parsing agent…</div>;
  }
  if (isError || !data) return null;

  return (
    <div className="rounded-xl border border-cs-border bg-cs-card overflow-hidden">
      {/* Root agent card */}
      <div className="border-b border-cs-border px-4 py-3">
        <div className="flex items-start justify-between gap-3">
          <div className="flex items-start gap-3">
            <div className="mt-0.5 rounded-md bg-blue-500/10 p-1.5 text-blue-400"><Bot size={14} /></div>
            <div>
              <h3 className="text-sm font-semibold">{data.name ?? "root_agent"}</h3>
              {data.model && (
                <div className="mt-0.5 flex items-center gap-1.5">
                  <Cpu size={10} className="text-cs-muted" />
                  <span className="text-[11px] text-cs-muted font-mono">{data.model}</span>
                </div>
              )}
              {data.instruction && (
                <p className="mt-1 line-clamp-2 text-[11px] text-cs-muted max-w-lg">{data.instruction}</p>
              )}
            </div>
          </div>
          <button
            onClick={() => onOpenFile(agentPath)}
            className="text-[10px] text-cs-muted hover:text-cs-accent flex items-center gap-1 shrink-0"
          >
            <ExternalLink size={10} /> Edit YAML
          </button>
        </div>
      </div>

      <div className="p-4 space-y-4">
        {/* Sub-agents */}
        {data.subAgents.length > 0 && (
          <div>
            <h4 className="mb-2 text-[10px] font-medium text-cs-muted uppercase tracking-wide">
              Sub-agents ({data.subAgents.length})
            </h4>
            <div className="grid gap-2 md:grid-cols-2">
              {data.subAgents.map((sa) => (
                <div key={sa.name} className="flex items-start gap-2 rounded-md border border-cs-border/60 bg-cs-bg/40 px-3 py-2">
                  <Bot size={12} className="mt-0.5 shrink-0 text-blue-300" />
                  <div className="min-w-0">
                    <div className="text-xs font-medium">{sa.name}</div>
                    {sa.model && <div className="font-mono text-[10px] text-cs-muted">{sa.model}</div>}
                    {sa.description && <p className="mt-0.5 line-clamp-1 text-[10px] text-cs-muted">{sa.description}</p>}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Tools */}
        {data.tools.length > 0 && (
          <div>
            <h4 className="mb-2 text-[10px] font-medium text-cs-muted uppercase tracking-wide">
              Tools ({data.tools.length})
            </h4>
            <div className="flex flex-wrap gap-1.5">
              {data.tools.map((t) => (
                <span
                  key={t.name}
                  className="rounded-md border border-cs-border/60 bg-cs-bg/40 px-2 py-1 text-[11px]"
                >
                  <Wrench size={10} className="inline mr-1 text-cs-muted" />
                  {t.name}
                  {t.kind && <span className="ml-1 text-cs-muted">({t.kind})</span>}
                </span>
              ))}
            </div>
          </div>
        )}

        {data.subAgents.length === 0 && data.tools.length === 0 && (
          <p className="text-center text-xs text-cs-muted py-2">No sub-agents or tools defined in this agent.</p>
        )}
      </div>
    </div>
  );
}
