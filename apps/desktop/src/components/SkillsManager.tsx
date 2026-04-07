import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Search, Plus, FolderOpen, File, ChevronDown, ArrowDown, AlertTriangle, ChevronRight, Store, Terminal, Cpu, Server, Globe, FolderKanban } from "lucide-react";
import { getSkills, toggleSkill, type Skill } from "@/lib/api";
import { openclawListSkills, listProjects, type Project } from "@/lib/tauri-api";
import { formatNumber, cn } from "@/lib/utils";
import { analyzeSkillConflicts, type SkillConflict } from "@/lib/skill-similarity";
import SkillDetailPanel from "./SkillDetailPanel";
import CreateSkillModal from "./CreateSkillModal";
import MarketplaceGrid from "./MarketplaceGrid";

const RUNTIME_FILTERS = [
  { id: "all" as const, label: "All", icon: null },
  { id: "claude" as const, label: "Claude", icon: Terminal, color: "#f97316" },
  { id: "codex" as const, label: "Codex", icon: Cpu, color: "#22c55e" },
  { id: "openclaw" as const, label: "OpenClaw", icon: Server, color: "#06b6d4" },
  { id: "hermes" as const, label: "Hermes", icon: Globe, color: "#a855f7" },
] as const;

type RuntimeFilter = typeof RUNTIME_FILTERS[number]["id"];

const SCOPE_ORDER = ["enterprise", "personal", "project", "plugin"] as const;

const SCOPE_COLORS: Record<string, { border: string; bg: string; text: string; label: string }> = {
  enterprise: { border: "border-red-500/40", bg: "bg-red-500/10", text: "text-red-400", label: "ENT" },
  personal:   { border: "border-cs-accent/40", bg: "bg-cs-accent/10", text: "text-cs-accent", label: "USR" },
  project:    { border: "border-blue-500/40", bg: "bg-blue-500/10", text: "text-blue-400", label: "PRJ" },
  plugin:     { border: "border-purple-500/40", bg: "bg-purple-500/10", text: "text-purple-400", label: "PLG" },
};

type SkillsTab = "my-skills" | "marketplace";

export default function SkillsManager() {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<SkillsTab>("my-skills");
  const [runtimeFilter, setRuntimeFilter] = useState<RuntimeFilter>("all");
  const [projectFilter, setProjectFilter] = useState<string>("all");
  const [search, setSearch] = useState("");
  const [selectedSkillId, setSelectedSkillId] = useState<string | null>(null);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const queryClient = useQueryClient();

  const { data: localSkills = [], isLoading, error } = useQuery({
    queryKey: ["skills"],
    queryFn: getSkills,
    retry: false,
  });

  // Fetch projects for filtering
  const { data: projects = [] } = useQuery<Project[]>({
    queryKey: ["projects"],
    queryFn: listProjects,
    retry: 1,
  });

  // Fetch remote OpenClaw skills
  const { data: ocSkills = [] } = useQuery({
    queryKey: ["openclaw-skills-list"],
    queryFn: async () => {
      try {
        return await openclawListSkills();
      } catch {
        return [];
      }
    },
    retry: 1,
  });

  // Merge local + remote skills, dedup by name
  const skills = useMemo(() => {
    const localNames = new Set(localSkills.map((s) => s.name));
    const newOc = ocSkills.filter((s) => !localNames.has(s.name));
    return [...localSkills, ...newOc];
  }, [localSkills, ocSkills]);

  const toggle = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      toggleSkill(id, enabled),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["skills"] }),
  });

  const filtered = skills.filter((s) => {
    const matchesSearch = s.name.toLowerCase().includes(search.toLowerCase());
    const matchesRuntime = runtimeFilter === "all" || s.runtime === runtimeFilter;
    // Filter by project: check if skill's filePath starts with project path
    const matchesProject = projectFilter === "all" ||
      (projectFilter === "global" && (s.filePath.startsWith("~") || s.filePath.startsWith("/Users"))) ||
      s.filePath.includes(projectFilter);
    return matchesSearch && matchesRuntime && matchesProject;
  });

  // Get unique runtimes that have skills (for showing runtime counts)
  const runtimeCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const s of skills) {
      counts[s.runtime] = (counts[s.runtime] || 0) + 1;
    }
    return counts;
  }, [skills]);

  const groupedSkills = SCOPE_ORDER.map((scope) => ({
    scope,
    skills: filtered.filter((s) => s.scope === scope),
  }));

  // Analyze enabled skills for description conflicts
  const conflicts = useMemo(() => {
    const enabled = skills.filter((s) => s.enabled);
    if (enabled.length < 2) return [];
    return analyzeSkillConflicts(enabled);
  }, [skills]);

  const highConflicts = conflicts.filter((c) => c.severity === "high");
  const mediumConflicts = conflicts.filter((c) => c.severity === "medium");

  if (isLoading) {
    return <LoadingSkeleton />;
  }

  if (error) {
    return (
      <div className="space-y-6">
        <div>
          <h2 className="section-title mb-1">{t('skills.title')}</h2>
          <p className="text-cs-muted text-sm">{t('skills.subtitle')}</p>
        </div>
        <div className="card text-center py-8">
          <p className="text-cs-danger text-sm mb-2">{t('common.error')}</p>
          <p className="text-cs-muted text-xs">{String(error)}</p>
        </div>
      </div>
    );
  }

  return (
    <>
      <div className="space-y-6">
        <div>
          <h2 className="text-xl font-semibold mb-1">{t('skills.title')}</h2>
          <p className="text-cs-muted text-sm">
            {t('skills.subtitle')}
          </p>
        </div>

        {/* Tab bar */}
        <div className="flex items-center border-b border-cs-border">
          <button
            onClick={() => setActiveTab("my-skills")}
            className={cn(
              "px-4 py-2.5 text-sm font-medium border-b-2 transition-colors -mb-px",
              activeTab === "my-skills"
                ? "border-cs-accent text-cs-accent"
                : "border-transparent text-cs-muted hover:text-cs-text"
            )}
          >
            {t("skills.tabs.mySkills")}
          </button>
          <button
            onClick={() => setActiveTab("marketplace")}
            className={cn(
              "flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium border-b-2 transition-colors -mb-px",
              activeTab === "marketplace"
                ? "border-cs-accent text-cs-accent"
                : "border-transparent text-cs-muted hover:text-cs-text"
            )}
          >
            <Store size={14} />
            {t("skills.tabs.marketplace")}
          </button>
        </div>

        {activeTab === "marketplace" ? (
          <MarketplaceGrid />
        ) : (
          <>
            {/* Filters row */}
            <div className="flex items-center gap-4 flex-wrap">
              {/* Runtime filter */}
              <div className="flex items-center gap-1.5 flex-wrap">
                {RUNTIME_FILTERS.map((rf) => {
                  const count = rf.id === "all" ? skills.length : (runtimeCounts[rf.id] || 0);
                  const Icon = rf.icon;
                  // Skip runtimes with 0 skills (except "all")
                  if (rf.id !== "all" && count === 0) return null;
                  return (
                    <button
                      key={rf.id}
                      onClick={() => setRuntimeFilter(rf.id)}
                      className={cn(
                        "flex items-center gap-1.5 px-2.5 py-1 text-[11px] font-medium rounded-full border transition-colors",
                        runtimeFilter === rf.id
                          ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                          : "border-cs-border text-cs-muted hover:text-cs-text"
                      )}
                      style={
                        runtimeFilter === rf.id && rf.id !== "all"
                          ? { borderColor: `${rf.color}66`, background: `${rf.color}18`, color: rf.color }
                          : undefined
                      }
                    >
                      {Icon && <Icon size={11} />}
                      {rf.label}
                      <span className="text-[9px] opacity-60">({count})</span>
                    </button>
                  );
                })}
              </div>

              {/* Project filter */}
              {projects.length > 0 && (
                <div className="flex items-center gap-2">
                  <FolderKanban size={14} className="text-cs-muted" />
                  <select
                    value={projectFilter}
                    onChange={(e) => setProjectFilter(e.target.value)}
                    className="bg-cs-card border border-cs-border rounded-md px-2 py-1 text-xs focus:outline-none focus:border-cs-accent"
                  >
                    <option value="all">{t("skills.filters.allProjects", "All Projects")}</option>
                    <option value="global">{t("skills.filters.global", "Global Skills")}</option>
                    {projects.map((p) => (
                      <option key={p.id} value={p.path}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                </div>
              )}
            </div>

            {/* Priority legend */}
            <PriorityIndicator />

            {/* Conflict alerts */}
            {(highConflicts.length > 0 || mediumConflicts.length > 0) && (
              <ConflictAlerts conflicts={conflicts} onSelectSkill={setSelectedSkillId} />
            )}

            {/* Search */}
            <div className="relative">
              <Search
                size={16}
                className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted"
              />
              <input
                type="text"
                className="input pl-9"
                placeholder={t('skills.search')}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>

            {/* Drag hint */}
            <p className="text-[10px] text-cs-muted italic">
              {t("skills.dragHint")}
            </p>

            {/* Skill groups by scope */}
            {groupedSkills.map(({ scope, skills: scopeSkills }) => (
              <SkillGroup
                key={scope}
                scope={scope}
                title={t(`skills.scopes.${scope}`)}
                skills={scopeSkills}
                selectedId={selectedSkillId}
                onSelect={setSelectedSkillId}
                onToggle={(id, enabled) => toggle.mutate({ id, enabled })}
              />
            ))}

            {filtered.length === 0 && (
              <p className="text-cs-muted text-sm text-center py-8">
                {search ? t('common.noResults') : t('skills.noSkills')}
              </p>
            )}

            {/* New Skill button */}
            <button
              onClick={() => setShowCreateModal(true)}
              className="w-full flex items-center justify-center gap-2 py-3 rounded-lg border border-dashed border-cs-border text-cs-muted hover:text-cs-accent hover:border-cs-accent/40 transition-colors text-sm"
            >
              <Plus size={16} />
              {t('skills.createNew')}
            </button>
          </>
        )}
      </div>

      {/* Detail panel */}
      {selectedSkillId && (
        <SkillDetailPanel
          skillId={selectedSkillId}
          onClose={() => setSelectedSkillId(null)}
        />
      )}

      {/* Create modal */}
      {showCreateModal && (
        <CreateSkillModal onClose={() => setShowCreateModal(false)} />
      )}
    </>
  );
}

/** Visual priority indicator showing Enterprise → Personal → Project → Plugin */
function PriorityIndicator() {
  const { t } = useTranslation();

  return (
    <div className="card !p-3">
      <p className="text-xs text-cs-muted uppercase tracking-wider mb-2 font-medium">
        {t('skills.priority.title')}
      </p>
      <div className="flex items-center gap-1">
        {SCOPE_ORDER.map((scope, i) => {
          const colors = SCOPE_COLORS[scope];
          return (
            <div key={scope} className="flex items-center gap-1">
              <div className={cn(
                "px-2.5 py-1 rounded-md border text-xs font-medium",
                colors.border, colors.bg, colors.text
              )}>
                {t(`skills.scopes.${scope}`)}
              </div>
              {i < SCOPE_ORDER.length - 1 && (
                <div className="flex items-center text-cs-muted">
                  <svg width="20" height="12" viewBox="0 0 20 12" className="text-cs-muted">
                    <path d="M2 6 L14 6 M10 2 L14 6 L10 10" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </div>
              )}
            </div>
          );
        })}
        <span className="text-[10px] text-cs-muted ml-2 italic">
          {t('skills.priority.hint')}
        </span>
      </div>
    </div>
  );
}

function SkillGroup({
  scope,
  title,
  skills,
  selectedId,
  onSelect,
  onToggle,
}: {
  scope: string;
  title: string;
  skills: Skill[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onToggle: (id: string, enabled: boolean) => void;
}) {
  const [orderedSkills, setOrderedSkills] = useState<Skill[] | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);

  if (skills.length === 0) return null;

  const colors = SCOPE_COLORS[scope] || SCOPE_COLORS.personal;
  const displaySkills = orderedSkills || skills;

  function handleDragStart(e: React.DragEvent, skillId: string) {
    e.dataTransfer.setData("text/plain", skillId);
    e.dataTransfer.effectAllowed = "move";
  }

  function handleDragOver(e: React.DragEvent, skillId: string) {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    setDragOverId(skillId);
  }

  function handleDrop(e: React.DragEvent, targetId: string) {
    e.preventDefault();
    const sourceId = e.dataTransfer.getData("text/plain");
    if (sourceId === targetId) return;

    const current = orderedSkills || [...skills];
    const sourceIdx = current.findIndex((s) => s.id === sourceId);
    const targetIdx = current.findIndex((s) => s.id === targetId);
    if (sourceIdx === -1 || targetIdx === -1) return;

    const reordered = [...current];
    const [moved] = reordered.splice(sourceIdx, 1);
    reordered.splice(targetIdx, 0, moved);
    setOrderedSkills(reordered);

    // Persist order to localStorage
    const orderMap = JSON.parse(localStorage.getItem("ato-skill-order") || "{}");
    orderMap[scope] = reordered.map((s) => s.id);
    localStorage.setItem("ato-skill-order", JSON.stringify(orderMap));

    setDragOverId(null);
  }

  function handleDragEnd() {
    setDragOverId(null);
  }

  return (
    <div>
      <div className="flex items-center gap-2 mb-2">
        <span className={cn(
          "text-[10px] font-mono font-bold uppercase px-1.5 py-0.5 rounded border",
          colors.border, colors.bg, colors.text
        )}>
          {colors.label}
        </span>
        <h3 className="text-sm font-medium text-cs-muted uppercase tracking-wider">
          {title}
        </h3>
        <span className="text-xs text-cs-muted">({skills.length})</span>
      </div>
      <div className="space-y-2">
        {displaySkills.map((skill, index) => {
          const isDir = skill.filePath.endsWith("/");
          return (
            <div
              key={skill.id}
              draggable
              onDragStart={(e) => handleDragStart(e, skill.id)}
              onDragOver={(e) => handleDragOver(e, skill.id)}
              onDrop={(e) => handleDrop(e, skill.id)}
              onDragEnd={handleDragEnd}
              onClick={() => onSelect(skill.id)}
              className={cn(
                "card flex items-center justify-between gap-4 cursor-pointer transition-colors",
                selectedId === skill.id
                  ? "border-cs-accent/50 bg-cs-accent/5"
                  : "hover:border-cs-border/80",
                dragOverId === skill.id && "border-cs-accent/60 bg-cs-accent/10"
              )}
            >
              <div className="min-w-0 flex-1 flex items-start gap-2.5">
                {/* Drag handle + priority number */}
                <div className="flex flex-col items-center gap-0.5 shrink-0 mt-0.5 cursor-grab active:cursor-grabbing">
                  <span className="text-[9px] font-mono text-cs-muted">{index + 1}</span>
                  <span className="text-cs-muted/40 text-[8px]">&#x2630;</span>
                </div>
                {isDir ? (
                  <FolderOpen size={16} className="text-cs-accent shrink-0 mt-0.5" />
                ) : (
                  <File size={16} className="text-cs-muted shrink-0 mt-0.5" />
                )}
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <p className="text-sm font-semibold truncate">{skill.name}</p>
                    <span
                      className={cn(
                        "w-2 h-2 rounded-full shrink-0",
                        skill.enabled ? "bg-green-400" : "bg-cs-muted/40"
                      )}
                    />
                  </div>
                  <p className="text-xs text-cs-text/70 mt-0.5 line-clamp-2">
                    {skill.description}
                  </p>
                  <p className="text-[10px] text-cs-muted font-mono mt-1 truncate">
                    {skill.filePath}
                  </p>
                </div>
              </div>
              <div className="flex items-center gap-3 shrink-0">
                <span className="text-xs text-cs-muted font-mono">
                  {formatNumber(skill.tokenCount)}
                </span>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggle(skill.id, !skill.enabled);
                  }}
                  className={cn(
                    "relative w-9 h-5 rounded-full transition-colors duration-200",
                    skill.enabled ? "bg-cs-accent" : "bg-cs-border"
                  )}
                >
                  <span
                    className={cn(
                      "absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full transition-transform duration-200",
                      skill.enabled && "translate-x-4"
                    )}
                  />
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function ConflictAlerts({
  conflicts,
  onSelectSkill,
}: {
  conflicts: SkillConflict[];
  onSelectSkill: (id: string) => void;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);

  const highCount = conflicts.filter((c) => c.severity === "high").length;
  const mediumCount = conflicts.filter((c) => c.severity === "medium").length;
  const displayConflicts = expanded ? conflicts.filter((c) => c.severity !== "low") : conflicts.filter((c) => c.severity === "high").slice(0, 2);

  return (
    <div className={cn(
      "rounded-lg border p-3",
      highCount > 0
        ? "border-red-500/30 bg-red-500/5"
        : "border-yellow-500/30 bg-yellow-500/5"
    )}>
      <div className="flex items-center gap-2 mb-2">
        <AlertTriangle size={14} className={highCount > 0 ? "text-red-400" : "text-yellow-400"} />
        <span className={cn("text-xs font-medium", highCount > 0 ? "text-red-400" : "text-yellow-400")}>
          {t("skills.conflicts.title")}
        </span>
        <span className="text-[10px] text-cs-muted ml-auto">
          {highCount > 0 && <span className="text-red-400">{highCount} {t("skills.conflicts.high")}</span>}
          {highCount > 0 && mediumCount > 0 && <span className="text-cs-muted"> · </span>}
          {mediumCount > 0 && <span className="text-yellow-400">{mediumCount} {t("skills.conflicts.medium")}</span>}
        </span>
      </div>
      <p className="text-[10px] text-cs-muted mb-2">
        {t("skills.conflicts.hint")}
      </p>

      <div className="space-y-2">
        {displayConflicts.map((conflict, i) => (
          <div
            key={i}
            className="rounded-md bg-cs-bg/50 border border-cs-border p-2.5"
          >
            <div className="flex items-center gap-2 mb-1.5">
              <span className={cn(
                "text-[9px] font-bold uppercase px-1.5 py-0.5 rounded",
                conflict.severity === "high"
                  ? "bg-red-500/15 text-red-400"
                  : "bg-yellow-500/15 text-yellow-400"
              )}>
                {conflict.similarity}%
              </span>
              <button
                onClick={() => onSelectSkill(conflict.skillA.id)}
                className="text-xs font-medium text-cs-accent hover:underline truncate"
              >
                {conflict.skillA.name}
              </button>
              <span className="text-[10px] text-cs-muted shrink-0">&harr;</span>
              <button
                onClick={() => onSelectSkill(conflict.skillB.id)}
                className="text-xs font-medium text-cs-accent hover:underline truncate"
              >
                {conflict.skillB.name}
              </button>
            </div>

            {/* Shared keywords */}
            <div className="flex flex-wrap gap-1 mb-1.5">
              {conflict.sharedKeywords.slice(0, 6).map((kw) => (
                <span key={kw} className="text-[9px] font-mono px-1.5 py-0.5 rounded bg-cs-border/60 text-cs-muted">
                  {kw}
                </span>
              ))}
              {conflict.sharedKeywords.length > 6 && (
                <span className="text-[9px] text-cs-muted">+{conflict.sharedKeywords.length - 6}</span>
              )}
            </div>

            <p className="text-[10px] text-cs-muted italic">{conflict.suggestion}</p>
          </div>
        ))}
      </div>

      {conflicts.filter((c) => c.severity !== "low").length > 2 && (
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-text mt-2 transition-colors"
        >
          <ChevronRight size={10} className={cn("transition-transform", expanded && "rotate-90")} />
          {expanded ? t("skills.conflicts.showLess") : t("skills.conflicts.showAll", { count: conflicts.filter((c) => c.severity !== "low").length })}
        </button>
      )}
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="space-y-6 animate-pulse">
      <div>
        <div className="h-6 w-24 bg-cs-border rounded" />
        <div className="h-4 w-56 bg-cs-border rounded mt-2" />
      </div>
      <div className="h-10 bg-cs-border rounded" />
      {[1, 2, 3].map((i) => (
        <div key={i} className="card h-14" />
      ))}
    </div>
  );
}
