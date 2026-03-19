import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Search,
  Bot,
  Compass,
  Map,
  Wrench,
  Link2,
  ChevronDown,
  Cpu,
  Terminal,
  Server,
  Globe,
  Star,
  Loader2,
  AlertCircle,
  Settings,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { getSkills } from "@/lib/api";
import type { AgentRuntime } from "@/components/cron/types";
import { useRuntimeAgents } from "@/hooks/useRuntimeData";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface OpenClawAgent {
  id: string;
  identityName?: string;
  workspace?: string;
  agentDir?: string;
  model?: string;
  isDefault?: boolean;
  routes?: string[];
}

interface DisplayAgent {
  id: string;
  name: string;
  identityName?: string;
  description: string;
  runtime: AgentRuntime;
  model?: string;
  isDefault?: boolean;
  skills: string[];
  workspace?: string;
  agentDir?: string;
  routes?: string[];
  source: "built-in" | "openclaw" | "local-skills";
  type: "general-purpose" | "Explore" | "Plan" | "custom";
}

// ---------------------------------------------------------------------------
// Claude Code built-in subagent definitions
// ---------------------------------------------------------------------------

const CLAUDE_BUILTIN_AGENTS: DisplayAgent[] = [
  {
    id: "claude-general",
    name: "general-purpose",
    description: "General-purpose agent for complex multi-step tasks",
    runtime: "claude",
    model: "claude-sonnet-4-5",
    isDefault: true,
    skills: [],
    source: "built-in",
    type: "general-purpose",
  },
  {
    id: "claude-explore",
    name: "Explore",
    description: "Fast agent for codebase exploration",
    runtime: "claude",
    model: "claude-haiku-4-5",
    isDefault: false,
    skills: [],
    source: "built-in",
    type: "Explore",
  },
  {
    id: "claude-plan",
    name: "Plan",
    description: "Software architect agent for implementation plans",
    runtime: "claude",
    model: "claude-sonnet-4-5",
    isDefault: false,
    skills: [],
    source: "built-in",
    type: "Plan",
  },
];

// ---------------------------------------------------------------------------
// Known OpenClaw skills (from workspace detection)
// ---------------------------------------------------------------------------

const OPENCLAW_SKILLS = [
  "agent-pixel-billboard",
  "ai-influencer-core",
  "content-marketing",
  "crowterminal",
  "github",
  "ika-move",
  "ika-operator",
  "ika-sdk",
  "security-audit",
  "x-twitter-collector",
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type RuntimeFilter = "all" | AgentRuntime;

const TYPE_ICON: Record<DisplayAgent["type"], typeof Bot> = {
  "general-purpose": Bot,
  Explore: Compass,
  Plan: Map,
  custom: Wrench,
};

const TYPE_COLOR: Record<DisplayAgent["type"], string> = {
  "general-purpose": "border-blue-500/40 bg-blue-500/10 text-blue-400",
  Explore: "border-amber-500/40 bg-amber-500/10 text-amber-400",
  Plan: "border-violet-500/40 bg-violet-500/10 text-violet-400",
  custom: "border-rose-500/40 bg-rose-500/10 text-rose-400",
};

const RUNTIME_COLOR: Record<AgentRuntime, string> = {
  claude: "border-orange-500/40 bg-orange-500/10 text-orange-400",
  codex: "border-green-500/40 bg-green-500/10 text-green-400",
  openclaw: "border-cyan-500/40 bg-cyan-500/10 text-cyan-400",
  hermes: "border-purple-500/40 bg-purple-500/10 text-purple-400",
};

const RUNTIME_DOT_COLOR: Record<AgentRuntime, string> = {
  claude: "#f97316",
  codex: "#22c55e",
  openclaw: "#06b6d4",
  hermes: "#a855f7",
};

const RUNTIME_ICON: Record<AgentRuntime, typeof Bot> = {
  claude: Terminal,
  codex: Cpu,
  openclaw: Server,
  hermes: Globe,
};

function TypeBadge({ type }: { type: DisplayAgent["type"] }) {
  const Icon = TYPE_ICON[type];
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider rounded-full border",
        TYPE_COLOR[type]
      )}
    >
      <Icon size={12} />
      {type}
    </span>
  );
}

function RuntimeBadge({ runtime }: { runtime: AgentRuntime }) {
  const Icon = RUNTIME_ICON[runtime];
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider rounded-full border",
        RUNTIME_COLOR[runtime]
      )}
    >
      <Icon size={12} />
      {runtime}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Agent Card (expandable)
// ---------------------------------------------------------------------------

function AgentCard({
  agent,
  expanded,
  onToggleExpand,
}: {
  agent: DisplayAgent;
  expanded: boolean;
  onToggleExpand: () => void;
}) {
  return (
    <div
      onClick={onToggleExpand}
      className={cn(
        "rounded-lg border cursor-pointer transition-all duration-200",
        "bg-[#16161e] border-[#2a2a3a] hover:border-[#3a3a4a]",
        expanded && "border-[#00FFB2]/30 bg-[#00FFB2]/[0.02]"
      )}
    >
      <div className="p-4">
        <div className="flex items-start justify-between gap-4">
          {/* Left content */}
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2 mb-1.5 flex-wrap">
              {agent.isDefault && (
                <Star size={14} className="text-yellow-400 fill-yellow-400 shrink-0" />
              )}
              <p className="text-sm font-medium text-[#e8e8f0] truncate">
                {agent.identityName || agent.name}
              </p>
              {agent.identityName && agent.identityName !== agent.name && (
                <span className="text-[11px] font-mono text-[#8888a0]">
                  ({agent.id})
                </span>
              )}
              <TypeBadge type={agent.type} />
              <RuntimeBadge runtime={agent.runtime} />
              {agent.source === "built-in" && (
                <span className="inline-flex items-center px-1.5 py-0.5 text-[10px] font-semibold rounded border border-[#2a2a3a] text-[#8888a0]">
                  Built-in
                </span>
              )}
            </div>

            <p className="text-xs text-[#8888a0] line-clamp-1 mb-2">
              {agent.description}
            </p>

            {/* Model badge */}
            {agent.model && (
              <div className="flex items-center gap-1.5 mb-2">
                <Cpu size={12} className="text-[#8888a0]" />
                <span className="text-[11px] font-mono text-[#8888a0] border border-[#2a2a3a] rounded px-1.5 py-0.5">
                  {agent.model}
                </span>
              </div>
            )}

            {/* Skill pills */}
            {agent.skills.length > 0 && (
              <div className="flex items-center gap-1.5 flex-wrap">
                <Link2 size={12} className="text-[#00FFB2] shrink-0" />
                {agent.skills.slice(0, expanded ? agent.skills.length : 5).map((skill) => (
                  <span
                    key={skill}
                    className="px-2 py-0.5 text-[11px] font-mono rounded-full border border-[#00FFB2]/30 bg-[#00FFB2]/10 text-[#00FFB2]"
                  >
                    {skill}
                  </span>
                ))}
                {!expanded && agent.skills.length > 5 && (
                  <span className="text-[11px] text-[#8888a0]">
                    +{agent.skills.length - 5} more
                  </span>
                )}
              </div>
            )}
          </div>

          {/* Right side */}
          <div className="flex items-center gap-2 shrink-0 pt-1">
            <ChevronDown
              size={16}
              className={cn(
                "text-[#8888a0] transition-transform duration-200",
                expanded && "rotate-180"
              )}
            />
          </div>
        </div>
      </div>

      {/* Expanded details */}
      {expanded && (
        <div className="px-4 pb-4 pt-0 border-t border-[#2a2a3a] mt-0">
          <div className="pt-3 space-y-3">
            {agent.workspace && (
              <div>
                <span className="text-[10px] font-semibold text-[#8888a0] uppercase tracking-wider block mb-0.5">
                  Workspace
                </span>
                <span className="text-xs font-mono text-[#e8e8f0]">
                  {agent.workspace}
                </span>
              </div>
            )}
            {agent.agentDir && (
              <div>
                <span className="text-[10px] font-semibold text-[#8888a0] uppercase tracking-wider block mb-0.5">
                  Agent Directory
                </span>
                <span className="text-xs font-mono text-[#e8e8f0]">
                  {agent.agentDir}
                </span>
              </div>
            )}
            {agent.model && (
              <div>
                <span className="text-[10px] font-semibold text-[#8888a0] uppercase tracking-wider block mb-0.5">
                  Model
                </span>
                <span className="text-xs font-mono text-[#e8e8f0]">
                  {agent.model}
                </span>
              </div>
            )}
            {agent.routes && agent.routes.length > 0 && (
              <div>
                <span className="text-[10px] font-semibold text-[#8888a0] uppercase tracking-wider block mb-0.5">
                  Routes
                </span>
                <div className="flex flex-wrap gap-1.5">
                  {agent.routes.map((route) => (
                    <span
                      key={route}
                      className="px-2 py-0.5 text-[11px] font-mono rounded border border-[#2a2a3a] text-[#e8e8f0]"
                    >
                      {route}
                    </span>
                  ))}
                </div>
              </div>
            )}
            {agent.skills.length > 0 && (
              <div>
                <span className="text-[10px] font-semibold text-[#8888a0] uppercase tracking-wider block mb-0.5">
                  Skills ({agent.skills.length})
                </span>
                <div className="flex flex-wrap gap-1.5">
                  {agent.skills.map((skill) => (
                    <span
                      key={skill}
                      className="px-2 py-0.5 text-[11px] font-mono rounded-full border border-[#00FFB2]/30 bg-[#00FFB2]/10 text-[#00FFB2]"
                    >
                      {skill}
                    </span>
                  ))}
                </div>
              </div>
            )}
            <div>
              <span className="text-[10px] font-semibold text-[#8888a0] uppercase tracking-wider block mb-0.5">
                Source
              </span>
              <span className="text-xs text-[#e8e8f0]">
                {agent.source === "built-in"
                  ? "Claude Code built-in"
                  : agent.source === "openclaw"
                  ? "OpenClaw gateway (SSH)"
                  : "Local skills directory"}
              </span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Empty State
// ---------------------------------------------------------------------------

function EmptyState({ hasSearch }: { hasSearch: boolean }) {
  const { t } = useTranslation();

  if (hasSearch) {
    return (
      <div className="text-center py-12">
        <Search size={32} className="mx-auto mb-3 text-[#8888a0]/40" />
        <p className="text-[#8888a0] text-sm">{t("common.noResults")}</p>
      </div>
    );
  }

  return (
    <div className="text-center py-12 space-y-3">
      <AlertCircle size={32} className="mx-auto text-[#8888a0]/40" />
      <p className="text-[#e8e8f0] text-sm font-medium">No agents detected</p>
      <p className="text-[#8888a0] text-xs max-w-sm mx-auto">
        Connect a runtime (Claude Code, OpenClaw, Codex, or Hermes) to see agents here.
        Built-in Claude Code agents will appear automatically.
      </p>
      <div className="flex items-center justify-center gap-2 pt-2">
        <Settings size={14} className="text-[#00FFB2]" />
        <span className="text-xs text-[#00FFB2]">
          Check Configuration to set up runtimes
        </span>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

export default function SubagentsManager() {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [runtimeFilter, setRuntimeFilter] = useState<RuntimeFilter>("all");
  const [expandedId, setExpandedId] = useState<string | null>(null);

  // Fetch OpenClaw agents
  const openclawQuery = useRuntimeAgents();

  // Fetch local skills
  const skillsQuery = useQuery({
    queryKey: ["local-skills"],
    queryFn: async () => {
      try {
        return await getSkills();
      } catch {
        return [];
      }
    },
    refetchInterval: 60_000,
    retry: 1,
  });

  // Normalize OpenClaw agents into DisplayAgents
  const openclawAgents = useMemo<DisplayAgent[]>(() => {
    const raw = (openclawQuery.data || []) as OpenClawAgent[];
    return raw.map((agent) => ({
      id: `oc-${agent.id}`,
      name: agent.id,
      identityName: agent.identityName,
      description: agent.isDefault
        ? "Default OpenClaw agent"
        : `OpenClaw agent: ${agent.id}`,
      runtime: "openclaw" as AgentRuntime,
      model: agent.model,
      isDefault: agent.isDefault,
      skills: OPENCLAW_SKILLS,
      workspace: agent.workspace,
      agentDir: agent.agentDir,
      routes: agent.routes,
      source: "openclaw" as const,
      type: "custom" as const,
    }));
  }, [openclawQuery.data]);

  // Attach local skills to Claude built-in agents
  const claudeAgents = useMemo<DisplayAgent[]>(() => {
    const localSkillNames = (skillsQuery.data || [])
      .filter((s) => s.enabled && s.runtime === "claude")
      .map((s) => s.name);
    return CLAUDE_BUILTIN_AGENTS.map((agent) => ({
      ...agent,
      skills: localSkillNames,
    }));
  }, [skillsQuery.data]);

  // Merge all agents
  const allAgents = useMemo<DisplayAgent[]>(
    () => [...claudeAgents, ...openclawAgents],
    [claudeAgents, openclawAgents]
  );

  // Filter by search and runtime
  const filtered = useMemo(() => {
    let agents = allAgents;
    if (runtimeFilter !== "all") {
      agents = agents.filter((a) => a.runtime === runtimeFilter);
    }
    if (search.trim()) {
      const q = search.toLowerCase();
      agents = agents.filter(
        (a) =>
          a.name.toLowerCase().includes(q) ||
          (a.identityName?.toLowerCase().includes(q) ?? false) ||
          a.description.toLowerCase().includes(q) ||
          a.skills.some((s) => s.toLowerCase().includes(q))
      );
    }
    return agents;
  }, [allAgents, runtimeFilter, search]);

  // Count agents per runtime for filter tabs
  const counts = useMemo(() => {
    const c: Record<string, number> = { all: allAgents.length };
    for (const a of allAgents) {
      c[a.runtime] = (c[a.runtime] || 0) + 1;
    }
    return c;
  }, [allAgents]);

  const isLoading = openclawQuery.isLoading || skillsQuery.isLoading;

  // Determine which runtimes have agents
  const activeRuntimes = useMemo(() => {
    const runtimes: AgentRuntime[] = ["claude", "openclaw", "codex", "hermes"];
    return runtimes.filter((rt) => (counts[rt] || 0) > 0);
  }, [counts]);

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h2 className="text-xl font-semibold text-[#e8e8f0] mb-1">
          {t("subagents.title")}
        </h2>
        <p className="text-[#8888a0] text-sm">
          {t("subagents.subtitle")}
        </p>
      </div>

      {/* Search */}
      <div className="relative">
        <Search
          size={16}
          className="absolute left-3 top-1/2 -translate-y-1/2 text-[#8888a0]"
        />
        <input
          type="text"
          className="input pl-9"
          placeholder={t("subagents.search")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      {/* Runtime filter tabs */}
      <div className="flex items-center gap-1 flex-wrap">
        <button
          onClick={() => setRuntimeFilter("all")}
          className={cn(
            "px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider rounded-md transition-colors",
            runtimeFilter === "all"
              ? "bg-[#00FFB2]/10 text-[#00FFB2]"
              : "text-[#8888a0] hover:text-[#e8e8f0]"
          )}
        >
          All ({counts.all || 0})
        </button>
        {activeRuntimes.map((rt) => {
          const color = RUNTIME_DOT_COLOR[rt];
          const Icon = RUNTIME_ICON[rt];
          return (
            <button
              key={rt}
              onClick={() => setRuntimeFilter(rt)}
              className={cn(
                "inline-flex items-center gap-1 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider rounded-md transition-colors",
                runtimeFilter === rt
                  ? "text-[#e8e8f0]"
                  : "text-[#8888a0] hover:text-[#e8e8f0]"
              )}
              style={
                runtimeFilter === rt
                  ? { background: `${color}20`, color }
                  : {}
              }
            >
              <Icon size={12} />
              {rt} ({counts[rt] || 0})
            </button>
          );
        })}
      </div>

      {/* Loading state */}
      {isLoading && (
        <div className="flex items-center justify-center gap-2 py-8">
          <Loader2 size={18} className="animate-spin text-[#00FFB2]" />
          <span className="text-sm text-[#8888a0]">Detecting agents...</span>
        </div>
      )}

      {/* Error states */}
      {openclawQuery.isError && (
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg border border-red-500/30 bg-red-500/5 text-red-400 text-xs">
          <AlertCircle size={14} />
          <span>
            Failed to connect to OpenClaw gateway. Check SSH configuration.
          </span>
        </div>
      )}

      {/* Agent sections */}
      {!isLoading && (
        <>
          {/* Claude Code built-in agents section */}
          {(runtimeFilter === "all" || runtimeFilter === "claude") &&
            filtered.some((a) => a.source === "built-in") && (
              <div className="space-y-2">
                <div className="flex items-center gap-2">
                  <Terminal size={14} className="text-orange-400" />
                  <h3 className="text-xs font-semibold text-[#8888a0] uppercase tracking-wider">
                    Claude Code Built-in Agents
                  </h3>
                </div>
                <div className="space-y-2">
                  {filtered
                    .filter((a) => a.source === "built-in")
                    .map((agent) => (
                      <AgentCard
                        key={agent.id}
                        agent={agent}
                        expanded={expandedId === agent.id}
                        onToggleExpand={() =>
                          setExpandedId(expandedId === agent.id ? null : agent.id)
                        }
                      />
                    ))}
                </div>
              </div>
            )}

          {/* OpenClaw agents section */}
          {(runtimeFilter === "all" || runtimeFilter === "openclaw") &&
            filtered.some((a) => a.source === "openclaw") && (
              <div className="space-y-2">
                <div className="flex items-center gap-2">
                  <Server size={14} className="text-cyan-400" />
                  <h3 className="text-xs font-semibold text-[#8888a0] uppercase tracking-wider">
                    OpenClaw Agents
                  </h3>
                  <span className="text-[10px] text-[#8888a0] font-mono">
                    (via SSH)
                  </span>
                </div>
                <div className="space-y-2">
                  {filtered
                    .filter((a) => a.source === "openclaw")
                    .map((agent) => (
                      <AgentCard
                        key={agent.id}
                        agent={agent}
                        expanded={expandedId === agent.id}
                        onToggleExpand={() =>
                          setExpandedId(expandedId === agent.id ? null : agent.id)
                        }
                      />
                    ))}
                </div>
              </div>
            )}

          {/* Empty state */}
          {filtered.length === 0 && (
            <EmptyState hasSearch={search.trim().length > 0} />
          )}

          {/* Summary */}
          {filtered.length > 0 && (
            <div className="flex items-center justify-between text-[11px] text-[#8888a0] px-1 pt-2 border-t border-[#2a2a3a]">
              <span>
                {filtered.length} agent{filtered.length !== 1 ? "s" : ""} detected
              </span>
              <div className="flex items-center gap-3">
                {activeRuntimes.map((rt) => (
                  <span key={rt} className="flex items-center gap-1">
                    <span
                      className="w-2 h-2 rounded-full"
                      style={{ backgroundColor: RUNTIME_DOT_COLOR[rt] }}
                    />
                    {rt}
                  </span>
                ))}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
