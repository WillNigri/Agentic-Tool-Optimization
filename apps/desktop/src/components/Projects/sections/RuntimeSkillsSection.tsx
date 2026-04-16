import { Sparkles } from "lucide-react";
import type { LocalSkill } from "@/lib/api";
import SectionShell, { ScopeBadge, EmptyRow } from "./SectionShell";

interface RuntimeSkillsSectionProps {
  runtime: "codex" | "openclaw" | "hermes";
  skills: LocalSkill[];
  onOpen: (path: string) => void;
}

const LABELS = {
  codex: "Codex",
  openclaw: "OpenClaw",
  hermes: "Hermes",
} as const;

export default function RuntimeSkillsSection({ runtime, skills, onOpen }: RuntimeSkillsSectionProps) {
  const personal = skills.filter((s) => s.scope === "personal");
  const project = skills.filter((s) => s.scope === "project");

  return (
    <SectionShell
      icon={Sparkles}
      title={`${LABELS[runtime]} Skills`}
      subtitle="Global + project-scoped"
      count={skills.length}
    >
      <div className="space-y-4">
        <Group title="Global" scope="user" skills={personal} onOpen={onOpen} />
        <Group title="Project" scope="project" skills={project} onOpen={onOpen} />
      </div>
    </SectionShell>
  );
}

function Group({
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
                  <span className="truncate text-sm">{skill.name}</span>
                  {skill.description && (
                    <p className="mt-0.5 line-clamp-1 text-[11px] text-cs-muted">{skill.description}</p>
                  )}
                </div>
                <div className="shrink-0 text-[10px] text-cs-muted">
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
