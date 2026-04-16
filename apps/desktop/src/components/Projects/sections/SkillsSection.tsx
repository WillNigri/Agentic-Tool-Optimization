import { Sparkles } from "lucide-react";
import type { LocalSkill } from "@/lib/api";
import SectionShell, { ScopeBadge, EmptyRow } from "./SectionShell";

interface SkillsSectionProps {
  skills: LocalSkill[];
  onOpen: (path: string) => void;
}

export default function SkillsSection({ skills, onOpen }: SkillsSectionProps) {
  const personal = skills.filter((s) => s.scope === "personal");
  const project = skills.filter((s) => s.scope === "project");

  return (
    <SectionShell
      icon={Sparkles}
      title="Skills"
      subtitle="Inherited from ~/.claude/skills plus this project's .claude/skills"
      count={skills.length}
    >
      <div className="space-y-4">
        <SkillGroup title="Global" scope="user" skills={personal} onOpen={onOpen} />
        <SkillGroup title="Project" scope="project" skills={project} onOpen={onOpen} />
      </div>
    </SectionShell>
  );
}

function SkillGroup({
  title,
  scope,
  skills,
  onOpen,
}: {
  title: string;
  scope: "user" | "project";
  skills: LocalSkill[];
  onOpen: (path: string) => void;
}) {
  return (
    <div>
      <div className="mb-2 flex items-center gap-2">
        <h3 className="text-xs font-medium text-cs-muted">{title}</h3>
        <ScopeBadge scope={scope} />
        <span className="text-[10px] text-cs-muted">({skills.length})</span>
      </div>
      {skills.length === 0 ? (
        <EmptyRow message={scope === "project" ? "No project-scoped skills." : "No global skills."} />
      ) : (
        <ul className="space-y-1">
          {skills.map((skill) => (
            <li key={skill.id}>
              <button
                onClick={() => onOpen(skill.filePath)}
                className="flex w-full items-center gap-3 rounded-md border border-cs-border/60 px-3 py-2 text-left hover:border-cs-accent/40 hover:bg-cs-bg"
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-sm">{skill.name}</span>
                    {!skill.enabled && (
                      <span className="rounded bg-cs-border/60 px-1.5 py-0.5 text-[10px] text-cs-muted">
                        disabled
                      </span>
                    )}
                  </div>
                  {skill.description && (
                    <p className="mt-0.5 line-clamp-1 text-[11px] text-cs-muted">{skill.description}</p>
                  )}
                </div>
                <div className="shrink-0 text-right text-[10px] text-cs-muted">
                  ~{skill.tokenCount.toLocaleString()} tok
                </div>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
