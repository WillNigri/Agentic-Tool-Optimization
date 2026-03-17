import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Search } from "lucide-react";
import { getSkills, toggleSkill, type Skill } from "@/lib/api";
import { formatNumber, cn } from "@/lib/utils";

export default function SkillsManager() {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
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
  const personal = filtered.filter((s) => s.scope === "personal");
  const project = filtered.filter((s) => s.scope === "project");

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
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-semibold mb-1">{t('skills.title')}</h2>
        <p className="text-cs-muted text-sm">
          {t('skills.subtitle')}
        </p>
      </div>

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

      {/* Personal Skills */}
      <SkillGroup
        title={t('skills.personal')}
        skills={personal}
        onToggle={(id, enabled) => toggle.mutate({ id, enabled })}
      />

      {/* Project Skills */}
      <SkillGroup
        title={t('skills.project')}
        skills={project}
        onToggle={(id, enabled) => toggle.mutate({ id, enabled })}
      />

      {filtered.length === 0 && (
        <p className="text-cs-muted text-sm text-center py-8">
          {search ? t('common.noResults') : t('skills.noSkills')}
        </p>
      )}
    </div>
  );
}

function SkillGroup({
  title,
  skills,
  onToggle,
}: {
  title: string;
  skills: Skill[];
  onToggle: (id: string, enabled: boolean) => void;
}) {
  if (skills.length === 0) return null;

  return (
    <div>
      <h3 className="text-sm font-medium text-cs-muted mb-2 uppercase tracking-wider">
        {title}
      </h3>
      <div className="space-y-2">
        {skills.map((skill) => (
          <div
            key={skill.id}
            className="card flex items-center justify-between gap-4"
          >
            <div className="min-w-0 flex-1">
              <p className="text-sm font-medium truncate">{skill.name}</p>
              <p className="text-xs text-cs-muted truncate">
                {skill.filePath}
              </p>
            </div>
            <div className="flex items-center gap-3 shrink-0">
              <span className="text-xs text-cs-muted">
                {formatNumber(skill.tokenCount)} tokens
              </span>
              <button
                onClick={() => onToggle(skill.id, !skill.enabled)}
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
        ))}
      </div>
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
