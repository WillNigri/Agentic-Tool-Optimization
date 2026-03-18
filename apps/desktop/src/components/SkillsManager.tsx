import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Search, Plus, FolderOpen, File, ChevronDown, ArrowDown, AlertTriangle, ChevronRight } from "lucide-react";
import { getSkills, toggleSkill, type Skill } from "@/lib/api";
import { formatNumber, cn } from "@/lib/utils";
import { analyzeSkillConflicts, type SkillConflict } from "@/lib/skill-similarity";
import SkillDetailPanel from "./SkillDetailPanel";
import CreateSkillModal from "./CreateSkillModal";

const SCOPE_ORDER = ["enterprise", "personal", "project", "plugin"] as const;

const SCOPE_COLORS: Record<string, { border: string; bg: string; text: string; label: string }> = {
  enterprise: { border: "border-red-500/40", bg: "bg-red-500/10", text: "text-red-400", label: "ENT" },
  personal:   { border: "border-cs-accent/40", bg: "bg-cs-accent/10", text: "text-cs-accent", label: "USR" },
  project:    { border: "border-blue-500/40", bg: "bg-blue-500/10", text: "text-blue-400", label: "PRJ" },
  plugin:     { border: "border-purple-500/40", bg: "bg-purple-500/10", text: "text-purple-400", label: "PLG" },
};

export default function SkillsManager() {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [selectedSkillId, setSelectedSkillId] = useState<string | null>(null);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const queryClient = useQueryClient();

  const { data: skills = [], isLoading, error } = useQuery({
    queryKey: ["skills"],
    queryFn: getSkills,
    retry: false,
  });

  const toggle = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      toggleSkill(id, enabled),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["skills"] }),
  });

  const filtered = skills.filter((s) =>
    s.name.toLowerCase().includes(search.toLowerCase())
  );

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
  if (skills.length === 0) return null;

  const colors = SCOPE_COLORS[scope] || SCOPE_COLORS.personal;

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
        {skills.map((skill) => {
          const isDir = skill.filePath.endsWith("/");
          return (
            <div
              key={skill.id}
              onClick={() => onSelect(skill.id)}
              className={cn(
                "card flex items-center justify-between gap-4 cursor-pointer transition-colors",
                selectedId === skill.id
                  ? "border-cs-accent/50 bg-cs-accent/5"
                  : "hover:border-cs-border/80"
              )}
            >
              <div className="min-w-0 flex-1 flex items-start gap-2.5">
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
